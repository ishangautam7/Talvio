//! Live checks against a running account server.
//!
//! `#[ignore]`d, so `cargo test` stays offline. They exist because the unit
//! tests next door can only show that this app reads what it expects; whether
//! the server accepts what it sends, and routes it, has one authority.
//!
//! ```text
//! TALVIO_ACCOUNT_TEST_SERVER=http://localhost:8787 \
//!   cargo test --lib account_live -- --ignored --nocapture
//! ```
//!
//! Each run signs up a fresh throwaway account and deletes it at the end.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::api::{Api, DeviceInfo};
use super::remote::testing::FakeHost;
use super::remote::{Router, CAPABILITIES};
use super::socket::{self, Event};

const WAIT: Duration = Duration::from_secs(10);

fn server() -> String {
    std::env::var("TALVIO_ACCOUNT_TEST_SERVER")
        .expect("set TALVIO_ACCOUNT_TEST_SERVER to the server's URL")
        .trim_end_matches('/')
        .to_string()
}

fn random_hex() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).unwrap();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn device(name: &str, platform: &'static str, kind: &'static str) -> DeviceInfo {
    DeviceInfo {
        name: name.to_string(),
        platform,
        kind,
        app_version: "test",
    }
}

/// A silent 16 kHz mono 16-bit WAV, base64, as the phone sends it.
fn wav_base64(samples: u32) -> String {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
    for _ in 0..samples {
        writer.write_sample(0i16).unwrap();
    }
    writer.finalize().unwrap();
    STANDARD.encode(bytes.into_inner())
}

async fn next_event(events: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .expect("an event in time")
        .expect("the socket task is running")
}

type PhoneSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn phone_connect(server: &str, token: &str) -> PhoneSocket {
    let mut request = socket::ws_url(server)
        .unwrap()
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

async fn phone_send(phone: &mut PhoneSocket, frame: Value) {
    phone.send(Message::text(frame.to_string())).await.unwrap();
}

/// The next server message of `kind`, skipping the others (devices updates).
async fn phone_next(phone: &mut PhoneSocket, kind: &str) -> Value {
    tokio::time::timeout(WAIT, async {
        loop {
            let message = phone.next().await.expect("open").expect("readable");
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == kind {
                    return value;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no '{kind}' message in time"))
}

#[tokio::test]
#[ignore]
async fn account_live_phone_requests_reach_the_computer() {
    let server = server();
    let api = Api::new(&server).unwrap();
    let email = format!("zv-test-{}@example.com", random_hex());
    let password = random_hex() + &random_hex();

    let computer = api
        .signup(
            &email,
            &password,
            None,
            &device("Live test PC", "linux", "desktop"),
        )
        .await
        .expect("sign up");

    // The computer's own socket, with a canned transcript, a fake paste, and
    // approval questions the test answers through the router.
    let host = Arc::new(FakeHost::default());
    let (router, outbox) = Router::new(host.clone());
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let task = tauri::async_runtime::spawn(socket::run(
        socket::ws_url(&server).unwrap(),
        computer.token.clone(),
        router.clone(),
        outbox,
        events_tx,
    ));
    assert!(matches!(next_event(&mut events).await, Event::Connecting));
    match next_event(&mut events).await {
        Event::Ready { device_id, user } => {
            assert_eq!(device_id, computer.device.id);
            assert_eq!(user.email, email);
        }
        other => panic!("expected ready, got {other:?}"),
    }
    match next_event(&mut events).await {
        Event::Devices(devices) => {
            assert!(devices
                .iter()
                .any(|d| d.id == computer.device.id && d.current));
        }
        other => panic!("expected devices, got {other:?}"),
    }

    // A phone on the same account, and a computer that never connects.
    let phone = api
        .login(
            &email,
            &password,
            &device("Android phone", "android", "mobile"),
        )
        .await
        .expect("phone login");
    let offline = api
        .login(
            &email,
            &password,
            &device("Offline PC", "windows", "desktop"),
        )
        .await
        .expect("second login");
    let mut ws = phone_connect(&server, &phone.token).await;
    assert_eq!(
        phone_next(&mut ws, "ready").await["deviceId"],
        phone.device.id
    );
    phone_send(&mut ws, json!({ "type": "hello", "capabilities": [] })).await;
    let devices = phone_next(&mut ws, "devices").await;
    let listed = devices["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == computer.device.id.as_str())
        .expect("the computer is listed")
        .clone();
    assert_eq!(listed["online"], true);
    assert_eq!(listed["capabilities"], json!(CAPABILITIES));

    let to = computer.device.id.as_str();
    phone_send(
        &mut ws,
        json!({ "type": "relay", "to": to, "body": { "kind": "agent.test", "requestId": "r1" } }),
    )
    .await;
    let reply = phone_next(&mut ws, "relay").await;
    assert_eq!(reply["from"], to);
    assert_eq!(
        reply["body"],
        json!({ "kind": "result", "requestId": "r1", "ok": false, "error": "unsupported" })
    );

    // The phone is new to this computer: its first request waits for Allow.
    phone_send(
        &mut ws,
        json!({ "type": "relay", "id": "r2", "to": to, "body": { "kind": "text", "requestId": "r2", "text": "hi" } }),
    )
    .await;
    assert_eq!(
        phone_next(&mut ws, "relay").await["body"],
        json!({ "kind": "progress", "requestId": "r2", "stage": "approval" })
    );
    assert_eq!(host.asked.lock().unwrap()[0].id, phone.device.id);
    router.answer(&phone.device.id, true);
    assert_eq!(
        phone_next(&mut ws, "relay").await["body"],
        json!({ "kind": "progress", "requestId": "r2", "stage": "pasting" })
    );
    assert_eq!(
        phone_next(&mut ws, "relay").await["body"],
        json!({ "kind": "result", "requestId": "r2", "ok": true, "pasted": true })
    );

    // The longest recording the phone sends (4:59) fits every frame limit on
    // the way: the phone's frame, the server's, and this client's.
    phone_send(
        &mut ws,
        json!({ "type": "relay", "id": "r4", "to": to, "body": {
            "kind": "dictate", "requestId": "r4", "deliver": "paste", "audio": wav_base64(299 * 16_000)
        } }),
    )
    .await;
    for stage in ["transcribing", "pasting"] {
        assert_eq!(phone_next(&mut ws, "relay").await["body"]["stage"], stage);
    }
    assert_eq!(
        phone_next(&mut ws, "relay").await["body"],
        json!({ "kind": "result", "requestId": "r4", "ok": true, "text": "hello there", "pasted": true })
    );
    assert_eq!(*host.pasted.lock().unwrap(), ["hi", "hello there"]);

    // A second phone the computer's user does not allow.
    let other = api
        .login(&email, &password, &device("iPhone", "ios", "mobile"))
        .await
        .expect("second phone login");
    let mut other_ws = phone_connect(&server, &other.token).await;
    phone_next(&mut other_ws, "ready").await;
    phone_send(
        &mut other_ws,
        json!({ "type": "hello", "capabilities": [] }),
    )
    .await;
    phone_send(
        &mut other_ws,
        json!({ "type": "relay", "id": "r5", "to": to, "body": { "kind": "text", "requestId": "r5", "text": "no" } }),
    )
    .await;
    assert_eq!(
        phone_next(&mut other_ws, "relay").await["body"]["stage"],
        "approval"
    );
    router.answer(&other.device.id, false);
    let refused = |id: &str| json!({ "kind": "result", "requestId": id, "ok": false, "error": "not_approved" });
    assert_eq!(
        phone_next(&mut other_ws, "relay").await["body"],
        refused("r5")
    );
    phone_send(
        &mut other_ws,
        json!({ "type": "relay", "id": "r6", "to": to, "body": { "kind": "text", "requestId": "r6", "text": "no" } }),
    )
    .await;
    assert_eq!(
        phone_next(&mut other_ws, "relay").await["body"],
        refused("r6")
    );
    assert_eq!(host.asked.lock().unwrap().len(), 2);
    assert_eq!(*host.pasted.lock().unwrap(), ["hi", "hello there"]);

    phone_send(
        &mut ws,
        json!({ "type": "relay", "id": "x1", "to": offline.device.id, "body": { "kind": "text", "requestId": "r3", "text": "hi" } }),
    )
    .await;
    let failed = phone_next(&mut ws, "relay_failed").await;
    assert_eq!(failed["id"], "x1");
    assert_eq!(failed["to"], offline.device.id.as_str());
    assert_eq!(failed["reason"], "offline");

    // The server's error codes come through as the commands' codes.
    let pc = device("Live test PC", "linux", "desktop");
    assert!(api.config().await.is_ok());
    assert_eq!(
        api.login(&email, "not-the-password", &pc)
            .await
            .err()
            .as_deref(),
        Some("invalid_credentials")
    );
    assert_eq!(
        api.signup(&email, &password, None, &pc)
            .await
            .err()
            .as_deref(),
        Some("email_taken")
    );
    assert_eq!(
        api.devices("zv1_not-a-token").await.err().as_deref(),
        Some("unauthorized")
    );

    // Deleting the account closes the computer's socket with 4401.
    api.delete_account(&phone.token).await.expect("delete");
    loop {
        match next_event(&mut events).await {
            Event::Revoked => break,
            Event::Devices(_) => continue,
            other => panic!("expected the session to be revoked, got {other:?}"),
        }
    }
    task.await.expect("the socket task ends after a revoke");
}

/// A computer that serves a live view of this machine's primary screen to a
/// real phone app, for checking the phone side by hand (or on an emulator):
/// it signs in to (or creates) the account in `TALVIO_LIVE_EMAIL` /
/// `TALVIO_LIVE_PASSWORD`, allows every phone on it, and runs for
/// `TALVIO_LIVE_MINUTES` (default 15). Only the capture is real; input is
/// recorded, not delivered.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn account_live_serve_a_live_view() {
    use super::remote::screen::video::{self, CaptureTarget, VideoSettings};
    use super::remote::testing::FakeVideo;
    use crate::command_mode::effects::WindowEntry;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    let server = server();
    let api = Api::new(&server).unwrap();
    let email = std::env::var("TALVIO_LIVE_EMAIL").expect("TALVIO_LIVE_EMAIL");
    let password = std::env::var("TALVIO_LIVE_PASSWORD").expect("TALVIO_LIVE_PASSWORD");
    let minutes: u64 = std::env::var("TALVIO_LIVE_MINUTES")
        .ok()
        .and_then(|m| m.parse().ok())
        .unwrap_or(15);
    let pc = device("Live view test PC", "windows", "desktop");
    let computer = match api.login(&email, &password, &pc).await {
        Ok(auth) => auth,
        Err(_) => api.signup(&email, &password, None, &pc).await.expect("sign up"),
    };

    let host = FakeHost {
        approve_all: true,
        ..FakeHost::default()
    };
    *host.windows.lock().unwrap() = vec![WindowEntry {
        id: 1,
        title: "Primary screen (as a window)".into(),
        app: "LiveTest".into(),
    }];
    *host.front.lock().unwrap() = Some(1);
    let host = Arc::new(host);
    let (router, outbox) = Router::new(host.clone());
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let _socket = tauri::async_runtime::spawn(socket::run(
        socket::ws_url(&server).unwrap(),
        computer.token.clone(),
        router.clone(),
        outbox,
        events_tx,
    ));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(minutes * 60);
    let mut streams = Vec::new();
    loop {
        // Each view takes the capture the fake hands out; keep one ready.
        if host.video.lock().unwrap().is_none() {
            let screen = video::list_screens().into_iter().next().expect("a screen");
            let (stream, frames) =
                video::start(CaptureTarget::Screen(screen.id), VideoSettings::default())
                    .expect("capture starts");
            *host.video.lock().unwrap() = Some(FakeVideo {
                frames: Some(frames),
                geometry: stream.geometry(),
                keyframes: Arc::new(AtomicUsize::new(0)),
                stopped: Arc::new(AtomicBool::new(false)),
            });
            streams.push(stream);
            println!("serving: a capture is ready");
        }
        tokio::select! {
            event = events.recv() => println!("socket: {event:?}"),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
        if tokio::time::Instant::now() > deadline {
            break;
        }
        let pressed = host.pressed.lock().unwrap().clone();
        if !pressed.is_empty() {
            println!("keys so far: {pressed:?}; clicks: {:?}", host.clicks.lock().unwrap());
        }
    }
}
