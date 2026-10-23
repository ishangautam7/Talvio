//! The account server's WebSocket (`server/PROTOCOL.md`, "WebSocket"): one
//! connection per signed-in session, kept up with pings and reconnected with
//! backoff, carrying the device list in and relayed requests both ways.
//!
//! Every wait here is bounded (connect, `ready`, each send, server silence),
//! so a half-open connection costs a reconnect, never a stuck task.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header::AUTHORIZATION, HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::api;
use super::remote::{Router, CAPABILITIES};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The server sends `ready` first; a connection that does not is not usable.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const PING_EVERY: Duration = Duration::from_secs(25);
/// Two missed pongs and change without a byte from the server: the connection
/// is gone even if TCP has not noticed. Bytes, not messages, so a large relay
/// still arriving on a slow link is not silence. The server's own limit is 75 s.
const SILENCE_LIMIT: Duration = Duration::from_secs(60);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// The server's frame limit.
const MAX_FRAME: usize = 16 * 1024 * 1024;

const CLOSE_REVOKED: u16 = 4401;
const CLOSE_REPLACED: u16 = 4000;

type Socket = WebSocketStream<MaybeTlsStream<Counted<TcpStream>>>;

/// When the connection last read a byte from the server.
type Heard = Arc<Mutex<std::time::Instant>>;

/// A transport that notes when it last read anything, which is what the
/// silence limit measures.
struct Counted<S> {
    inner: S,
    heard: Heard,
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            *self.heard.lock().unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// What the connection reports to whoever runs it.
#[derive(Debug)]
pub enum Event {
    Connecting,
    Ready {
        device_id: String,
        user: api::User,
    },
    Devices(Vec<api::Device>),
    Offline,
    /// Connecting failed or timed out without an answer from the server
    /// (unlike an HTTP error), so its address may have moved.
    Unreachable,
    /// The token was revoked (close 4401, or 401 on connect). The task has
    /// ended; the session must be cleared.
    Revoked,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Ready {
        #[serde(rename = "deviceId")]
        device_id: String,
        user: api::User,
    },
    Devices {
        devices: Vec<api::Device>,
    },
    Relay {
        from: String,
        body: serde_json::Value,
    },
    RelayFailed {},
    #[serde(other)]
    Other,
}

/// The socket URL for a server URL (`http` → `ws`, `https` → `wss`).
pub fn ws_url(server_url: &str) -> Option<String> {
    let (scheme, rest) = if let Some(rest) = server_url.strip_prefix("https://") {
        ("wss://", rest)
    } else {
        ("ws://", server_url.strip_prefix("http://")?)
    };
    Some(format!("{scheme}{}/v1/ws", rest.trim_end_matches('/')))
}

/// Keep a connection up until the session is revoked or replaced. Replies the
/// router queues while the connection is down go out after it reconnects.
pub async fn run(
    ws_url: String,
    token: String,
    router: Arc<Router>,
    mut outbox: mpsc::Receiver<String>,
    events: mpsc::UnboundedSender<Event>,
) {
    let mut backoff = Backoff::default();
    loop {
        let _ = events.send(Event::Connecting);
        let end = match connect(&ws_url, &token).await {
            Ok((socket, heard)) => session(socket, heard, &router, &mut outbox, &events).await,
            Err(Refused::Unauthorized) => End::Revoked,
            Err(Refused::Failed) => End::Lost { was_ready: false },
            Err(Refused::Unreachable) => {
                let _ = events.send(Event::Unreachable);
                End::Lost { was_ready: false }
            }
        };
        match end {
            End::Revoked => {
                info!("Account session revoked by the server");
                let _ = events.send(Event::Revoked);
                return;
            }
            End::Replaced => {
                // Reconnecting would only close the newer connection in turn.
                warn!(
                    "Account connection replaced by another one for this session; staying offline"
                );
                let _ = events.send(Event::Offline);
                return;
            }
            End::Lost { was_ready } => {
                if was_ready {
                    backoff.reset();
                }
                let _ = events.send(Event::Offline);
            }
        }
        let delay = backoff.next_delay();
        debug!("Account connection retrying in {delay:?}");
        tokio::time::sleep(delay).await;
    }
}

enum Refused {
    Unauthorized,
    /// Something answered, but not with a usable connection.
    Failed,
    Unreachable,
}

enum End {
    Revoked,
    Replaced,
    Lost { was_ready: bool },
}

/// `connect_async_with_config`, over a transport that notes what it reads.
async fn connect(url: &str, token: &str) -> Result<(Socket, Heard), Refused> {
    let mut request = url.into_client_request().map_err(|_| Refused::Failed)?;
    let auth =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Refused::Unauthorized)?;
    request.headers_mut().insert(AUTHORIZATION, auth);
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let uri = request.uri();
    let host = uri.host().ok_or(Refused::Failed)?.to_string();
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("wss") {
            443
        } else {
            80
        });
    let heard: Heard = Arc::new(Mutex::new(std::time::Instant::now()));
    let transport = heard.clone();
    let connecting = async move {
        let tcp = TcpStream::connect(format!("{host}:{port}")).await?;
        tcp.set_nodelay(true)?;
        let inner = Counted {
            inner: tcp,
            heard: transport,
        };
        tokio_tungstenite::client_async_tls_with_config(request, inner, Some(config), None).await
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Ok(Ok((socket, _))) => Ok((socket, heard)),
        Ok(Err(WsError::Http(response))) if response.status() == StatusCode::UNAUTHORIZED => {
            Err(Refused::Unauthorized)
        }
        Ok(Err(e @ (WsError::Io(_) | WsError::Tls(_)))) => {
            debug!("Account server unreachable: {e}");
            Err(Refused::Unreachable)
        }
        Ok(Err(e)) => {
            debug!("Account connection failed: {e}");
            Err(Refused::Failed)
        }
        Err(_) => {
            debug!("Account connection timed out");
            Err(Refused::Unreachable)
        }
    }
}

async fn send(socket: &mut Socket, frame: String) -> bool {
    matches!(
        tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::text(frame))).await,
        Ok(Ok(()))
    )
}

async fn session(
    mut socket: Socket,
    heard: Heard,
    router: &Arc<Router>,
    outbox: &mut mpsc::Receiver<String>,
    events: &mpsc::UnboundedSender<Event>,
) -> End {
    let ready_by = Instant::now() + READY_TIMEOUT;
    let mut ready = false;
    let mut ping = tokio::time::interval_at(Instant::now() + PING_EVERY, PING_EVERY);
    let lost = |ready| End::Lost { was_ready: ready };

    loop {
        tokio::select! {
            message = socket.next() => {
                let Some(Ok(message)) = message else {
                    return lost(ready);
                };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(frame) => {
                        return match frame.map(|f| u16::from(f.code)) {
                            Some(CLOSE_REVOKED) => End::Revoked,
                            Some(CLOSE_REPLACED) => End::Replaced,
                            _ => lost(ready),
                        };
                    }
                    _ => continue,
                };
                match serde_json::from_str::<ServerMessage>(&text) {
                    Ok(ServerMessage::Ready { device_id, user }) => {
                        let hello = json!({ "type": "hello", "capabilities": CAPABILITIES });
                        if !send(&mut socket, hello.to_string()).await {
                            return lost(ready);
                        }
                        ready = true;
                        info!("Account connection ready");
                        let _ = events.send(Event::Ready { device_id, user });
                    }
                    Ok(ServerMessage::Devices { devices }) => {
                        // Before the next frame, which may be a request it judges.
                        router.devices_changed(&devices);
                        let _ = events.send(Event::Devices(devices));
                    }
                    Ok(ServerMessage::Relay { from, body }) => router.handle(&from, body),
                    Ok(ServerMessage::RelayFailed {}) => {
                        debug!("A reply was not delivered: the device it was for is offline");
                    }
                    Ok(ServerMessage::Other) => {}
                    Err(_) => debug!("Ignoring an unreadable server message ({} bytes)", text.len()),
                }
            }
            Some(frame) = outbox.recv(), if ready => {
                if !send(&mut socket, frame).await {
                    return lost(ready);
                }
            }
            _ = tokio::time::sleep_until(ready_by), if !ready => {
                debug!("Account connection never became ready");
                return lost(false);
            }
            _ = ping.tick() => {
                if heard.lock().unwrap_or_else(|e| e.into_inner()).elapsed() > SILENCE_LIMIT {
                    debug!("Account server went silent");
                    return lost(ready);
                }
                if !send(&mut socket, json!({ "type": "ping" }).to_string()).await {
                    return lost(ready);
                }
            }
        }
    }
}

/// Reconnect delays: 1 s doubling to 30 s, each up to a quarter shorter at
/// random so a server restart is not met by every client at once.
#[derive(Default)]
struct Backoff {
    attempt: u32,
}

impl Backoff {
    fn next_delay(&mut self) -> Duration {
        let nominal = BACKOFF_MIN
            .saturating_mul(1 << self.attempt.min(16))
            .min(BACKOFF_MAX);
        self.attempt = self.attempt.saturating_add(1);
        let mut random = [0u8; 2];
        let _ = getrandom::fill(&mut random);
        jittered(nominal, u16::from_le_bytes(random))
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }
}

fn jittered(nominal: Duration, random: u16) -> Duration {
    nominal - nominal.mul_f64(f64::from(random) / f64::from(u16::MAX) / 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_thirty_seconds_with_bounded_jitter() {
        let mut backoff = Backoff::default();
        let nominal = [1, 2, 4, 8, 16, 30, 30, 30].map(Duration::from_secs);
        for expected in nominal {
            let delay = backoff.next_delay();
            assert!(delay <= expected, "{delay:?} > {expected:?}");
            assert!(
                delay >= expected.mul_f64(0.75),
                "{delay:?} < 3/4 of {expected:?}"
            );
        }
        for _ in 0..100 {
            assert!(backoff.next_delay() <= BACKOFF_MAX);
        }
        backoff.reset();
        assert!(backoff.next_delay() <= BACKOFF_MIN);
    }

    #[test]
    fn jitter_only_shortens() {
        let second = Duration::from_secs(1);
        assert_eq!(jittered(second, 0), second);
        assert_eq!(jittered(second, u16::MAX), Duration::from_millis(750));
    }

    #[tokio::test]
    async fn the_transport_notes_when_it_read_something() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut server, client) = tokio::io::duplex(64);
        let start = std::time::Instant::now();
        let heard: Heard = Arc::new(Mutex::new(start));
        let mut counted = Counted {
            inner: client,
            heard: heard.clone(),
        };
        counted.write_all(b"ping").await.unwrap();
        let mut sent = [0u8; 4];
        server.read_exact(&mut sent).await.unwrap();
        assert_eq!(*heard.lock().unwrap(), start, "writing is not hearing");
        server.write_all(b"part of a frame").await.unwrap();
        let mut buf = [0u8; 4];
        counted.read_exact(&mut buf).await.unwrap();
        assert!(*heard.lock().unwrap() > start);
    }

    #[test]
    fn socket_url_follows_the_server_scheme() {
        assert_eq!(
            ws_url("http://localhost:8787").as_deref(),
            Some("ws://localhost:8787/v1/ws")
        );
        assert_eq!(
            ws_url("https://sync.example.com/zv/").as_deref(),
            Some("wss://sync.example.com/zv/v1/ws")
        );
        assert_eq!(ws_url("ftp://example.com"), None);
    }

    #[test]
    fn reads_server_messages() {
        let ready: ServerMessage = serde_json::from_str(
            r#"{"type":"ready","deviceId":"d1","user":{"id":"u","email":"a@b.c","name":null,
                "hasPassword":true,"googleLinked":false,"createdAt":1}}"#,
        )
        .unwrap();
        assert!(matches!(ready, ServerMessage::Ready { device_id, .. } if device_id == "d1"));
        let failed: ServerMessage =
            serde_json::from_str(r#"{"type":"relay_failed","id":"x","to":"d","reason":"offline"}"#)
                .unwrap();
        assert!(matches!(failed, ServerMessage::RelayFailed {}));
        let pong: ServerMessage = serde_json::from_str(r#"{"type":"pong"}"#).unwrap();
        assert!(matches!(pong, ServerMessage::Other));
        let future: ServerMessage = serde_json::from_str(r#"{"type":"news","x":1}"#).unwrap();
        assert!(matches!(future, ServerMessage::Other));
    }
}
