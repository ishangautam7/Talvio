//! Google sign-in through the system browser (`server/PROTOCOL.md`, "Google
//! sign-in"): a loopback listener on 127.0.0.1 receives the one-time code, and
//! only this process can redeem it, because only it holds the PKCE verifier.
//! That binding is also why the callback needs no `state` check: a code planted
//! by someone else was issued for their challenge and fails the exchange.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use log::{debug, info};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use super::api::{Api, AuthResponse, DeviceInfo};

/// How long the browser has to come back.
const FLOW_DEADLINE: Duration = Duration::from_secs(5 * 60);
/// A browser connection that has not sent its request line by then is a
/// speculative preconnect, not the redirect.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The redirect waits this long for the exchange to decide its page.
const PAGE_WAIT: Duration = Duration::from_secs(30);
const MAX_REQUEST_LINE: usize = 8 * 1024;

/// A PKCE verifier and its S256 challenge.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Result<Self, String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| "server".to_string())?;
        Ok(Self::from_verifier(URL_SAFE_NO_PAD.encode(bytes)))
    }

    fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

/// What the browser brought back to `/callback`.
#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Error(String),
}

/// Reads an HTTP request line; only `GET /callback?…` with a code or an error
/// counts. Everything else (a favicon, a stray probe) is `None`.
fn parse_request_line(line: &str) -> Option<Callback> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    if !target.starts_with("/callback?") {
        return None;
    }
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if !value.is_empty() && value.len() <= 512 => code = Some(value.into_owned()),
            "error" if !value.is_empty() => error = Some(value.chars().take(64).collect()),
            _ => {}
        }
    }
    error.map(Callback::Error).or(code.map(Callback::Code))
}

/// The command error for a callback's `error=…`.
fn callback_error(error: &str) -> String {
    match error {
        "access_denied" => "cancelled",
        "google_unavailable" => "google_unavailable",
        // The account with that email is linked to another Google account.
        "account_conflict" => "account_conflict",
        _ => "server",
    }
    .to_string()
}

/// The browser's request: what it brought, and where to send its page.
struct Arrival {
    callback: Callback,
    page: oneshot::Sender<bool>,
}

/// Read one connection's request line. A callback is handed to the flow and
/// answered with the page it decides; anything else gets a 404.
async fn serve(mut stream: TcpStream, arrivals: mpsc::Sender<Arrival>) {
    let line =
        match tokio::time::timeout(REQUEST_READ_TIMEOUT, read_request_line(&mut stream)).await {
            Ok(Some(line)) => line,
            _ => return,
        };
    let Some(callback) = parse_request_line(&line) else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    };
    let (page, decided) = oneshot::channel();
    if arrivals.send(Arrival { callback, page }).await.is_err() {
        return;
    }
    let signed_in = matches!(tokio::time::timeout(PAGE_WAIT, decided).await, Ok(Ok(true)));
    let body = page_html(signed_in);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = tokio::time::timeout(REQUEST_READ_TIMEOUT, stream.write_all(response.as_bytes())).await;
}

async fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(2).position(|w| w == b"\r\n") {
            return String::from_utf8(buffer[..end].to_vec()).ok();
        }
        if buffer.len() > MAX_REQUEST_LINE {
            return None;
        }
    }
}

fn page_html(signed_in: bool) -> String {
    let (title, message) = if signed_in {
        (
            "You're signed in",
            "You can close this tab and go back to Talvio.",
        )
    } else {
        (
            "Sign-in didn't finish",
            "You can close this tab and try again in Talvio.",
        )
    };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Talvio</title><style>\
         :root{{color-scheme:light dark;--bg:#f3f6f6;--card:#fff;--ink:#1b292e;--muted:#5b6b70;--accent:#007e70}}\
         @media (prefers-color-scheme:dark){{:root{{--bg:#0f1517;--card:#172024;--ink:#e6eef0;--muted:#9fb0b4;--accent:#5cd6be}}}}\
         body{{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--ink);\
         font:16px/1.5 system-ui,-apple-system,\"Segoe UI\",sans-serif}}\
         main{{background:var(--card);padding:32px 36px;border-radius:16px;max-width:360px;margin:16px;text-align:center}}\
         h1{{font-size:20px;margin:0 0 8px;color:var(--accent)}}p{{margin:0;color:var(--muted)}}\
         </style></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    )
}

/// Run the whole flow: listen, open the browser, wait for the redirect, and
/// exchange its code. Ends at the first callback, on `cancel`, or after five
/// minutes (`timeout`).
pub async fn sign_in(
    api: &Api,
    server_url: &str,
    device: &DeviceInfo,
    open_browser: impl FnOnce(&str) -> Result<(), String>,
    mut cancel: oneshot::Receiver<()>,
) -> Result<AuthResponse, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| {
        debug!("Google sign-in: could not listen on loopback: {e}");
        "server".to_string()
    })?;
    let port = listener
        .local_addr()
        .map_err(|_| "server".to_string())?
        .port();
    let pkce = Pkce::new()?;

    let mut start = reqwest::Url::parse(&format!("{server_url}/v1/auth/google/start"))
        .map_err(|_| "invalid_url".to_string())?;
    start
        .query_pairs_mut()
        .append_pair("redirect_uri", &format!("http://127.0.0.1:{port}/callback"))
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256");
    open_browser(start.as_str())?;
    info!("Google sign-in: waiting for the browser on loopback port {port}");

    let (arrivals_tx, mut arrivals) = mpsc::channel::<Arrival>(1);
    let deadline = tokio::time::sleep(FLOW_DEADLINE);
    tokio::pin!(deadline);
    let arrival = loop {
        tokio::select! {
            _ = &mut cancel => return Err("cancelled".to_string()),
            _ = &mut deadline => return Err("timeout".to_string()),
            Some(arrival) = arrivals.recv() => break arrival,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    tauri::async_runtime::spawn(serve(stream, arrivals_tx.clone()));
                }
                // Out of descriptors, say: wait instead of spinning on it.
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    };
    // One redirect per flow; later connections are refused.
    drop(listener);

    let result = match arrival.callback {
        Callback::Error(error) => {
            info!("Google sign-in ended in the browser: {error}");
            Err(callback_error(&error))
        }
        Callback::Code(code) => api.exchange(&code, &pkce.verifier, device).await,
    };
    let _ = arrival.page.send(result.is_ok());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_is_s256_of_the_verifier() {
        // RFC 7636, appendix B.
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_32_random_bytes_unpadded() {
        let a = Pkce::new().unwrap();
        let b = Pkce::new().unwrap();
        assert_eq!(a.verifier.len(), 43);
        assert_ne!(a.verifier, b.verifier);
        assert!(a
            .verifier
            .chars()
            .chain(a.challenge.chars())
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn reads_the_code_or_the_error() {
        assert_eq!(
            parse_request_line("GET /callback?code=abc_DEF-1 HTTP/1.1"),
            Some(Callback::Code("abc_DEF-1".into()))
        );
        assert_eq!(
            parse_request_line("GET /callback?code=a%2Bb HTTP/1.1"),
            Some(Callback::Code("a+b".into()))
        );
        assert_eq!(
            parse_request_line("GET /callback?error=access_denied HTTP/1.1"),
            Some(Callback::Error("access_denied".into()))
        );
        // An error wins over a code sent beside it.
        assert_eq!(
            parse_request_line("GET /callback?code=x&error=google_failed HTTP/1.1"),
            Some(Callback::Error("google_failed".into()))
        );
    }

    #[test]
    fn ignores_everything_else() {
        for line in [
            "GET /favicon.ico HTTP/1.1",
            "GET /callback HTTP/1.1",
            "GET /callback? HTTP/1.1",
            "GET /callback?code= HTTP/1.1",
            "GET /callbackx?code=a HTTP/1.1",
            "GET /callback/../x?code=a HTTP/1.1",
            "POST /callback?code=a HTTP/1.1",
            "",
            "GET",
        ] {
            assert_eq!(parse_request_line(line), None, "{line:?}");
        }
        let long = format!("GET /callback?code={} HTTP/1.1", "a".repeat(513));
        assert_eq!(parse_request_line(&long), None);
    }

    #[test]
    fn browser_errors_map_to_command_codes() {
        assert_eq!(callback_error("access_denied"), "cancelled");
        assert_eq!(callback_error("google_unavailable"), "google_unavailable");
        assert_eq!(callback_error("account_conflict"), "account_conflict");
        assert_eq!(callback_error("google_failed"), "server");
    }
}
