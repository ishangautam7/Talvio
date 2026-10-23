//! `screen.watch`: one live view, from the request to its one `result`
//! (`PROTOCOL.md`, "Live view").
//!
//! The session is a single loop over everything that can happen to it: the
//! phone's handshake messages (relayed, [`Router::signal`]), the connection's
//! events, encoded frames and sound, and a quarter-second tick that checks
//! everything that ends a view without telling it (a cancel, a switch turned
//! off, Stop on this computer, the 30 s to connect, 10 minutes idle) and keeps
//! the phone's caret and target notices current. One view runs at a time.
//!
//! Input from the phone is aimed at the watched window the way `windows.keys`
//! aims, except the window is left in front (`stay`): someone who is watching
//! and typing into a window wants it to stay where they are working.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, info};
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::control::{self, Ack, Input, Message};
use super::input::{Button, Rect};
use super::rtc::{IceServer, Peer, PeerEvent};
use super::video::{CaptureTarget, Ended, Geometry, StartError, VideoSettings};
use crate::account::remote::windows::{self, Target};
use crate::account::remote::{Pasted, Reply, Router};
use crate::command_mode::agent::tree::{needs_confirmation, UiAction};

/// How long the phone has to connect after the offer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// No input from the phone and no change in the picture for this long ends
/// the view.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const TICK: Duration = Duration::from_millis(250);
/// The longest a keepalive frame may be apart and still count as "nothing
/// changed": frames closer together than this mean the picture moves.
const KEEPALIVE_SLACK: Duration = Duration::from_millis(1_500);
/// Handshake messages from the phone not yet read.
const SIGNAL_CAPACITY: usize = 64;

/// What the phone is watching.
#[derive(Clone, Debug)]
pub enum Watched {
    Window(crate::command_mode::effects::WindowEntry),
    Screen(super::video::ScreenInfo),
}

impl Watched {
    fn capture(&self) -> CaptureTarget {
        match self {
            Self::Window(window) => CaptureTarget::Window(window.id),
            Self::Screen(screen) => CaptureTarget::Screen(screen.id.clone()),
        }
    }

    fn title(&self) -> String {
        match self {
            Self::Window(window) => window.title.clone(),
            Self::Screen(screen) => screen.name.clone(),
        }
    }
}

/// Keeps "one view at a time" true however the session ends.
struct Watching(Arc<Router>, String);

impl Drop for Watching {
    fn drop(&mut self) {
        self.0.watches().remove(&self.1);
        self.0.watching.store(false, Ordering::Release);
        self.0.host().viewing(None);
    }
}

pub(in crate::account::remote) async fn watch(router: Arc<Router>, body: Value, reply: Reply) {
    if let Err(code) = windows::allowed(&router) {
        return reply.fail(code);
    }
    let watched = match windows::watch_target(&router, &reply, &body) {
        Ok(watched) => watched,
        Err(code) => return reply.fail(code),
    };
    let control_allowed = body
        .get("control")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let audio_on = body.get("audio").and_then(Value::as_bool).unwrap_or(false);
    if router
        .watching
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return reply.fail("busy");
    }
    let (signals, signal_rx) = mpsc::channel(SIGNAL_CAPACITY);
    router
        .watches()
        .insert(reply.request_id.clone(), (reply.to.clone(), signals));
    let watching = Watching(router.clone(), reply.request_id.clone());
    router.stop_watch.store(false, Ordering::Release);

    let outcome = run(
        &router,
        &reply,
        &watched,
        control_allowed,
        audio_on,
        signal_rx,
    )
    .await;
    // Free before answering: a phone switching to Control sends its next
    // watch the moment this result arrives, and must not find it `busy`.
    drop(watching);
    match outcome {
        Ok(()) => reply.ok(json!({})),
        Err(code) => reply.fail(code),
    }
}

/// The view itself. `Ok` when the phone stopped it; otherwise the code it ends
/// with.
async fn run(
    router: &Arc<Router>,
    reply: &Reply,
    watched: &Watched,
    control_allowed: bool,
    mut audio_on: bool,
    mut signals: mpsc::Receiver<Value>,
) -> Result<(), &'static str> {
    let host = router.host();
    let (video, mut frames) = host
        .start_video(watched.capture(), VideoSettings::default())
        .map_err(|e| {
            info!("A live view could not start capturing: {e}");
            match e {
                StartError::TargetGone => "target_gone",
                StartError::Unsupported(_) | StartError::Failed(_) => "capture_failed",
            }
        })?;
    let _video = Stop(Some(video.as_ref()));
    // Sound is optional: a view without it is still a view.
    let (audio, mut packets) = match host.start_audio() {
        Ok((audio, packets)) => {
            audio.set_enabled(audio_on);
            (Some(audio), Some(packets))
        }
        Err(e) => {
            debug!("A live view runs without sound: {e}");
            (None, None)
        }
    };
    let ice = host.ice_servers().await;
    let (peer, mut events) = Peer::new(&ice).await.map_err(|e| {
        info!("A live view's connection could not be made: {e}");
        "connect_failed"
    })?;
    let peer = PeerGuard(peer);
    let offer = peer.0.offer().await.map_err(|e| {
        info!("A live view's offer could not be made: {e}");
        "connect_failed"
    })?;
    reply.send_progress("rtc.offer", offer_fields(offer, &ice));
    host.viewing(Some(Viewing {
        phone: router.phone_name(&reply.to),
        title: watched.title(),
    }));
    info!("Live view offered to a phone");

    let started = Instant::now();
    let mut live = false;
    let mut last_input = Instant::now();
    let mut last_change = Instant::now();
    let mut last_frame: Option<Duration> = None;
    let mut sent_caret: Option<Option<(f64, f64, f64, f64)>> = None;
    let mut sent_target: Option<(u32, u32, &'static str)> = None;
    let mut tick = tokio::time::interval(TICK);

    loop {
        tokio::select! {
            signal = signals.recv() => {
                let Some(signal) = signal else { return Err("cancelled") };
                handshake(&peer.0, signal).await;
            }
            event = events.recv() => {
                let Some(event) = event else { return Err("connect_failed") };
                match event {
                    PeerEvent::Candidate(candidate) => {
                        reply.send_progress("rtc.ice", json!({ "candidate": candidate }));
                    }
                    PeerEvent::Connected => {
                        if !live {
                            live = true;
                            reply.progress("live");
                            info!("Live view connected");
                        }
                        video.request_keyframe();
                    }
                    PeerEvent::Closed => return Err("connect_failed"),
                    PeerEvent::ControlOpen => {
                        sent_target = None;
                        sent_caret = None;
                    }
                    PeerEvent::KeyframeRequest => video.request_keyframe(),
                    PeerEvent::Control(text) => {
                        match control::parse(&text, windows::parse_keys) {
                            Ok(Message::Audio(on)) => {
                                audio_on = on;
                                if let Some(audio) = &audio {
                                    audio.set_enabled(on);
                                }
                            }
                            Ok(Message::Keyframe) => video.request_keyframe(),
                            Ok(Message::Input { id, input }) => {
                                last_input = Instant::now();
                                let outcome = deliver(
                                    router,
                                    watched,
                                    video.geometry(),
                                    control_allowed,
                                    input,
                                )
                                .await;
                                peer.0.send_control(&control::ack(&id, outcome)).await;
                            }
                            Err(refused) => {
                                if let Some(id) = refused.id {
                                    let ack = control::ack(&id, Err(Ack::Error(refused.code)));
                                    peer.0.send_control(&ack).await;
                                }
                            }
                        }
                    }
                }
            }
            frame = frames.recv() => {
                let Some(frame) = frame else {
                    return Err(match video.ended() {
                        Some(Ended::TargetGone) => "target_gone",
                        _ => "capture_failed",
                    });
                };
                let since_last = last_frame
                    .map(|last| frame.timestamp.saturating_sub(last))
                    .unwrap_or_default();
                last_frame = Some(frame.timestamp);
                if since_last < KEEPALIVE_SLACK {
                    last_change = Instant::now();
                }
                if live {
                    if let Err(e) = peer.0.write_video(frame.data, since_last).await {
                        debug!("A live-view frame was not sent: {e}");
                    }
                }
            }
            packet = recv_or_wait(&mut packets) => {
                match packet {
                    Some(packet) if live && audio_on => {
                        if let Err(e) = peer.0.write_audio(packet.data, packet.duration).await {
                            debug!("A live-view sound packet was not sent: {e}");
                        }
                    }
                    Some(_) => {}
                    // The sound capture ended; the view carries on without it.
                    None => packets = None,
                }
            }
            _ = tick.tick() => {
                if reply.is_cancelled() {
                    // The phone stopped watching: the one ending that is not an error.
                    return if reply.revoked() { Err("not_approved") } else { Ok(()) };
                }
                if !router.input_allowed() {
                    return Err("remote_input_off");
                }
                if !host.windows_enabled() {
                    return Err("remote_windows_off");
                }
                if router.stop_watch.swap(false, Ordering::AcqRel) {
                    return Err("stopped");
                }
                if !live && started.elapsed() > CONNECT_TIMEOUT {
                    return Err("connect_failed");
                }
                if last_input.elapsed() > IDLE_TIMEOUT && last_change.elapsed() > IDLE_TIMEOUT {
                    return Err("idle");
                }
                if live {
                    let geometry = video.geometry();
                    let notice = target_notice(geometry, host, watched);
                    if sent_target != Some(notice) {
                        let (width, height, state) = notice;
                        peer.0
                            .send_control(&control::target(&watched.title(), width, height, state))
                            .await;
                        sent_target = Some(notice);
                    }
                    let caret = across(host.caret().await, geometry);
                    if sent_caret != Some(caret) {
                        peer.0.send_control(&control::caret(caret)).await;
                        sent_caret = Some(caret);
                    }
                }
            }
        }
    }
}

/// A relayed `rtc.answer` or `rtc.ice` from the phone.
async fn handshake(peer: &Peer, signal: Value) {
    match signal.get("kind").and_then(Value::as_str) {
        Some("rtc.answer") => {
            let Some(sdp) = signal.get("sdp").and_then(Value::as_str) else {
                return;
            };
            if let Err(e) = peer.answer(sdp.to_string()).await {
                info!("A live view's answer was refused: {e}");
            }
        }
        Some("rtc.ice") => {
            let Some(candidate) = signal
                .get("candidate")
                .cloned()
                .and_then(|c| serde_json::from_value(c).ok())
            else {
                return;
            };
            if let Err(e) = peer.add_candidate(candidate).await {
                debug!("A phone's ICE candidate was refused: {e}");
            }
        }
        _ => {}
    }
}

fn offer_fields(sdp: String, ice: &[IceServer]) -> Value {
    json!({ "sdp": sdp, "iceServers": ice })
}

/// Size and state of what is watched, for the `target` notice.
fn target_notice(
    geometry: Option<Geometry>,
    host: &dyn crate::account::remote::Host,
    watched: &Watched,
) -> (u32, u32, &'static str) {
    let Some(geometry) = geometry else {
        return (0, 0, "minimized");
    };
    let state = match watched {
        Watched::Window(window)
            if host
                .foreground_window()
                .is_some_and(|front| front.id != window.id) =>
        {
            "covered"
        }
        _ => "normal",
    };
    (geometry.width, geometry.height, state)
}

/// A screen rectangle as fractions of the picture, when it is inside it.
fn across(rect: Option<Rect>, geometry: Option<Geometry>) -> Option<(f64, f64, f64, f64)> {
    let (rect, geometry) = (rect?, geometry?);
    if geometry.width == 0 || geometry.height == 0 {
        return None;
    }
    let (w, h) = (geometry.width as f64, geometry.height as f64);
    let x = (rect.left - geometry.left) as f64 / w;
    let y = (rect.top - geometry.top) as f64 / h;
    let inside = (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y);
    let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
    inside.then(|| {
        (
            round(x),
            round(y),
            round((rect.width as f64 / w).min(1.0)),
            round((rect.height as f64 / h).min(1.0)),
        )
    })
}

/// A point across the picture as a screen point.
fn on_screen(at: control::Point, geometry: Geometry) -> (i32, i32) {
    let x = geometry.left + (at.x * geometry.width.saturating_sub(1) as f64).round() as i32;
    let y = geometry.top + (at.y * geometry.height.saturating_sub(1) as f64).round() as i32;
    (x, y)
}

/// Deliver one input message to the watched window (left in front) or, on a
/// watched screen, wherever the keyboard is.
async fn deliver(
    router: &Arc<Router>,
    watched: &Watched,
    geometry: Option<Geometry>,
    control_allowed: bool,
    input: Input,
) -> Result<(), Ack> {
    let host = router.host();
    if !router.input_allowed() {
        return Err(Ack::Error("remote_input_off"));
    }
    let target = match watched {
        Watched::Window(window) => Some(Target::staying(window.clone())),
        Watched::Screen(_) => None,
    };
    match input {
        Input::Keys(combos) => {
            match windows::aim(router, target.as_ref(), || host.press_keys(combos)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(Ack::Error("keys_failed")),
                Err(code) => Err(Ack::Error(code)),
            }
        }
        Input::Text(text) => {
            if host.focused_is_password().await {
                return Err(Ack::Error("password_field"));
            }
            match windows::aim(router, target.as_ref(), || host.paste(text)).await {
                Ok(Pasted::Done) => Ok(()),
                Ok(Pasted::Off) => Err(Ack::Error("paste_off")),
                Ok(Pasted::Failed) => Err(Ack::Error("paste_failed")),
                Err(code) => Err(Ack::Error(code)),
            }
        }
        Input::Tap {
            at,
            button,
            count,
            confirmed,
        } => {
            if !control_allowed {
                return Err(Ack::Error("not_allowed"));
            }
            let geometry = geometry.ok_or(Ack::Error("target_gone"))?;
            let (x, y) = on_screen(at, geometry);
            // Aimed first, so the control under the point is the watched
            // window's and not whatever covered it.
            windows::aim(router, target.as_ref(), || async {})
                .await
                .map_err(Ack::Error)?;
            if let Some(control) = host.control_at(x, y).await {
                if control.password {
                    return Err(Ack::Error("password_field"));
                }
                if !confirmed
                    && button == Button::Left
                    && needs_confirmation(control.role, &control.label, &UiAction::Invoke)
                {
                    return Err(Ack::Confirm(control.label));
                }
            }
            host.click(x, y, button, count)
                .await
                .map_err(|_| Ack::Error("tap_failed"))
        }
        Input::Scroll { at, dx, dy } => {
            if !control_allowed {
                return Err(Ack::Error("not_allowed"));
            }
            let geometry = geometry.ok_or(Ack::Error("target_gone"))?;
            let (x, y) = on_screen(at, geometry);
            windows::aim(router, target.as_ref(), || async {})
                .await
                .map_err(Ack::Error)?;
            host.scroll(x, y, dx, dy)
                .await
                .map_err(|_| Ack::Error("tap_failed"))
        }
    }
}

/// What the computer shows while a phone watches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewing {
    pub phone: String,
    pub title: String,
}

/// The next packet, or never when there is no sound.
async fn recv_or_wait<T>(receiver: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

/// Stops a capture when the session ends, however it ends.
struct Stop<'a>(Option<&'a dyn super::video::VideoStream>);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        if let Some(video) = self.0.take() {
            video.stop();
        }
    }
}

/// Closes the connection when the session ends.
struct PeerGuard(Peer);

impl Drop for PeerGuard {
    fn drop(&mut self) {
        let connection = self.0.handle();
        tauri::async_runtime::spawn(async move { connection.close().await });
    }
}

impl Reply {
    /// A `progress` with fields of its own beside `stage`.
    pub(in crate::account::remote) fn send_progress(&self, stage: &str, fields: Value) {
        let mut body = Map::new();
        body.insert("kind".into(), "progress".into());
        body.insert("stage".into(), stage.into());
        if let Value::Object(fields) = fields {
            body.extend(fields);
        }
        self.send(body);
    }
}

#[cfg(test)]
mod tests {
    use super::super::video::EncodedFrame;
    use super::*;
    use crate::account::remote::testing::{self, FakeHost, FakeVideo};
    use crate::command_mode::effects::WindowEntry;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use webrtc::data_channel::{DataChannel, DataChannelEvent};
    use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};
    use webrtc::peer_connection::{
        register_default_interceptors, MediaEngine, PeerConnection, PeerConnectionBuilder,
        PeerConnectionEventHandler, RTCIceCandidateInit, RTCPeerConnectionIceEvent,
        RTCSessionDescription, Registry,
    };

    /// The phone's side of a live view, in-process: a WebRTC peer that
    /// answers, counts the video packets it receives, and keeps the
    /// `control` channel the computer opens.
    struct Phone {
        candidates: mpsc::UnboundedSender<RTCIceCandidateInit>,
        packets: Arc<AtomicUsize>,
        control: mpsc::UnboundedSender<Arc<dyn DataChannel>>,
    }

    #[async_trait::async_trait]
    impl PeerConnectionEventHandler for Phone {
        async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
            if let Ok(candidate) = event.candidate.to_json() {
                let _ = self.candidates.send(candidate);
            }
        }
        async fn on_track(&self, track: Arc<dyn TrackRemote>) {
            let packets = self.packets.clone();
            tokio::spawn(async move {
                while let Some(event) = track.poll().await {
                    if let TrackRemoteEvent::OnRtpPacket(_) = event {
                        packets.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
        }
        async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
            let _ = self.control.send(channel);
        }
    }

    async fn next(frames: &mut mpsc::Receiver<String>) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(10), frames.recv())
            .await
            .expect("a reply")
            .expect("open outbox");
        serde_json::from_str::<Value>(&frame).unwrap()["body"].clone()
    }

    async fn next_ack(acks: &mut mpsc::UnboundedReceiver<Value>) -> Value {
        tokio::time::timeout(Duration::from_secs(10), acks.recv())
            .await
            .expect("an ack")
            .expect("open channel")
    }

    /// An access unit with an SPS, a PPS and an IDR slice, as the encoder
    /// sends a keyframe.
    fn keyframe(n: u64) -> EncodedFrame {
        let mut data = vec![0, 0, 0, 1, 0x67, 0x42, 0xe0, 0x1f, 0x8c, 0x8d, 0x40];
        data.extend([0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
        data.extend([0, 0, 0, 1, 0x65]);
        data.extend(std::iter::repeat_n(0x88, 3000));
        EncodedFrame {
            data,
            keyframe: true,
            width: 1280,
            height: 720,
            timestamp: Duration::from_millis(33 * n),
        }
    }

    /// The whole view against a real WebRTC peer: offer, answer and ICE over
    /// the relay, frames arriving as RTP, keys and taps over the data channel
    /// (a tap on a Send button asks first), and a cancel that ends it cleanly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_phone_watches_and_works_in_a_window() {
        let (frames_in, frames_out) = mpsc::channel(16);
        let feed = tokio::spawn(async move {
            for n in 0..5 {
                frames_in.send(keyframe(n)).await.unwrap();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            frames_in
        });
        watch_and_work(frames_out).await;
        drop(feed);
    }

    /// The same view of the real primary screen, captured and encoded on this
    /// machine: the frames the phone receives are real H.264.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "captures the screen of the machine it runs on"]
    async fn a_phone_watches_the_real_screen() {
        let screen = super::super::video::list_screens()
            .into_iter()
            .next()
            .expect("a screen");
        let (stream, frames) =
            super::super::video::start(CaptureTarget::Screen(screen.id), VideoSettings::default())
                .expect("capture starts");
        watch_and_work(frames).await;
        stream.stop();
    }

    async fn watch_and_work(frames_out: mpsc::Receiver<EncodedFrame>) {
        let keyframes = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let host = FakeHost::default();
        *host.windows.lock().unwrap() = vec![WindowEntry {
            id: 2,
            title: "Claude Code".into(),
            app: "WindowsTerminal".into(),
        }];
        *host.front.lock().unwrap() = Some(2);
        *host.video.lock().unwrap() = Some(FakeVideo {
            frames: Some(frames_out),
            geometry: Some(Geometry {
                left: 0,
                top: 0,
                width: 1281,
                height: 721,
            }),
            keyframes: keyframes.clone(),
            stopped: stopped.clone(),
        });
        let host = Arc::new(host);
        let (router, mut relay) = testing::router(host.clone());

        router.handle(
            "phone",
            json!({ "kind": "windows.list", "requestId": "list" }),
        );
        let listed = next(&mut relay).await;
        let target = listed["windows"][0]["id"].as_str().unwrap().to_string();

        router.handle(
            "phone",
            json!({ "kind": "screen.watch", "requestId": "w1", "target": target, "control": true }),
        );
        let offer = next(&mut relay).await;
        assert_eq!(offer["stage"], "rtc.offer", "{offer}");
        assert_eq!(host.notices.lock().unwrap().len(), 1);

        // The phone answers.
        let (candidates, mut phone_candidates) = mpsc::unbounded_channel();
        let (controls, mut control) = mpsc::unbounded_channel();
        let packets = Arc::new(AtomicUsize::new(0));
        let mut media = MediaEngine::default();
        media.register_default_codecs().unwrap();
        let registry = register_default_interceptors(Registry::new(), &mut media).unwrap();
        let phone = PeerConnectionBuilder::new()
            .with_media_engine(media)
            .with_interceptor_registry(registry)
            .with_handler(Arc::new(Phone {
                candidates,
                packets: packets.clone(),
                control: controls,
            }))
            .with_udp_addrs(vec!["0.0.0.0:0"])
            .build()
            .await
            .unwrap();
        let offer_sdp = offer["sdp"].as_str().unwrap().to_string();
        phone
            .set_remote_description(RTCSessionDescription::offer(offer_sdp).unwrap())
            .await
            .unwrap();
        let answer = phone.create_answer(None).await.unwrap();
        let sdp = answer.sdp.clone();
        phone.set_local_description(answer).await.unwrap();
        router.handle(
            "phone",
            json!({ "kind": "rtc.answer", "requestId": "w1", "sdp": sdp }),
        );

        // Candidates both ways until the view is live.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < deadline, "never went live");
            tokio::select! {
                body = next(&mut relay) => {
                    match body["stage"].as_str() {
                        Some("rtc.ice") => {
                            let candidate =
                                serde_json::from_value(body["candidate"].clone()).unwrap();
                            phone.add_ice_candidate(candidate).await.unwrap();
                        }
                        Some("live") => break,
                        _ => panic!("unexpected {body}"),
                    }
                }
                Some(candidate) = phone_candidates.recv() => {
                    router.handle(
                        "phone",
                        json!({ "kind": "rtc.ice", "requestId": "w1", "candidate": candidate }),
                    );
                }
            }
        }
        // Going live asks the encoder for a picture the phone can start from.
        assert!(keyframes.load(Ordering::SeqCst) >= 1);

        // Frames arrive as RTP.
        let deadline = Instant::now() + Duration::from_secs(10);
        while packets.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "no video reached the phone");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Keys and taps over the control channel, each answered by an ack.
        let channel = tokio::time::timeout(Duration::from_secs(10), control.recv())
            .await
            .unwrap()
            .unwrap();
        let (acks_in, mut acks) = mpsc::unbounded_channel::<Value>();
        {
            let channel = channel.clone();
            tokio::spawn(async move {
                while let Some(event) = channel.poll().await {
                    if let DataChannelEvent::OnMessage(message) = event {
                        let value: Value = serde_json::from_slice(&message.data).unwrap();
                        if value["t"] == "ack" {
                            let _ = acks_in.send(value);
                        }
                    }
                }
            });
        }
        // The channel may still be opening on the phone's side.
        let deadline = Instant::now() + Duration::from_secs(10);
        while channel
            .send_text(r#"{"t":"keys","id":"k1","keys":["enter"]}"#)
            .await
            .is_err()
        {
            assert!(
                Instant::now() < deadline,
                "the control channel never opened"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            next_ack(&mut acks).await,
            json!({ "t": "ack", "id": "k1", "ok": true })
        );
        assert_eq!(*host.pressed.lock().unwrap(), ["Enter"]);

        *host.control.lock().unwrap() = Some(super::super::input::ControlInfo {
            role: "button",
            label: "Send".into(),
            password: false,
        });
        channel
            .send_text(r#"{"t":"tap","id":"k2","x":0.5,"y":0.5}"#)
            .await
            .unwrap();
        assert_eq!(
            next_ack(&mut acks).await,
            json!({ "t": "ack", "id": "k2", "ok": false, "error": "confirm", "label": "Send" })
        );
        assert!(host.clicks.lock().unwrap().is_empty());
        channel
            .send_text(r#"{"t":"tap","id":"k3","x":0.5,"y":0.5,"confirmed":true}"#)
            .await
            .unwrap();
        assert_eq!(next_ack(&mut acks).await["ok"], true);
        assert_eq!(*host.clicks.lock().unwrap(), [(640, 360, Button::Left, 1)]);

        // The phone stops watching: the one ending that is not an error.
        router.handle("phone", json!({ "kind": "cancel", "requestId": "w1" }));
        let mut result = next(&mut relay).await;
        while result["kind"] != "result" {
            result = next(&mut relay).await;
        }
        assert_eq!(
            result,
            json!({ "kind": "result", "requestId": "w1", "ok": true })
        );
        assert!(stopped.load(Ordering::SeqCst));
        assert_eq!(host.notices.lock().unwrap().last(), Some(&None));
        assert!(!router.watching.load(Ordering::SeqCst));
        let _ = phone.close().await;
    }

    fn geometry() -> Geometry {
        Geometry {
            left: 100,
            top: 50,
            width: 1001,
            height: 501,
        }
    }

    #[test]
    fn picture_points_land_on_the_screen() {
        let at = |x, y| control::Point { x, y };
        assert_eq!(on_screen(at(0.0, 0.0), geometry()), (100, 50));
        assert_eq!(on_screen(at(1.0, 1.0), geometry()), (1100, 550));
        assert_eq!(on_screen(at(0.5, 0.5), geometry()), (600, 300));
    }

    #[test]
    fn the_caret_is_sent_as_fractions_of_the_picture() {
        let caret = Rect {
            left: 600,
            top: 300,
            width: 2,
            height: 20,
        };
        assert_eq!(
            across(Some(caret), Some(geometry())),
            Some((0.4995, 0.499, 0.002, 0.0399))
        );
        // Outside the picture (another window's caret) is no caret.
        let outside = Rect { left: 10, ..caret };
        assert_eq!(across(Some(outside), Some(geometry())), None);
        assert_eq!(across(None, Some(geometry())), None);
        assert_eq!(across(Some(caret), None), None);
    }
}
