//! Requests from the account's other devices, relayed by the server
//! (`server/PROTOCOL.md`, "Phone ↔ computer messages").
//!
//! Everything that is the same for every kind lives here: [`CAPABILITIES`]
//! (what `hello` announces), the dispatch table in [`handler`], the approval
//! gate every request passes before its handler runs ([`Router::admit`]), a
//! [`Reply`] per request that sends any number of `progress` messages and then
//! exactly one `result`, and `cancel`. Each kind is a file with an
//! `async fn handle(router: Arc<Router>, body: Value, reply: Reply)`.
//!
//! **Adding a kind** (the phone-to-computer agent feature, whose kinds and
//! capability names all start with `agent.`): add `remote/agent.rs`, one arm in
//! [`handler`] (`kind if kind.starts_with("agent.") =>
//! Some(|r, b, rep| agent::handle(r, b, rep).boxed())`) and its capability
//! names in [`CAPABILITIES`]. Nothing else here changes: the kind passes the
//! approval gate like every other. A request's cancel flag is
//! [`Reply::is_cancelled`]; a progress message with fields of its own is a
//! `Reply::send` from an `impl Reply` block in that file. If it needs more of
//! the app than [`Host`] offers, that is a new `Host` method, implemented for
//! the app in `host.rs`. The one-request-at-a-time rule ([`Reply::take_input`])
//! is for phone input; a new kind decides for itself whether it applies.
//! Losing access stops a running request through the same cancel flag.
//!
//! **Approval** (`PROTOCOL.md`, "Approval"): a request runs only when Phone
//! dictation is on and its sender is one of this account's phones that this
//! computer's user allowed. A phone not yet allowed gets `progress: approval`
//! while the user is asked ([`Host::ask_approval`]); up to [`MAX_WAITING`] of
//! its requests wait on that one question, outside the input slot and off the
//! socket's task. The question stays up until it is answered or its two
//! minutes run out, even when nothing waits on it any more, so a phone cannot
//! flash it on and off with a request and a cancel.

mod dictate;
mod host;
pub(super) mod screen;
mod text;
mod windows;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::FutureExt as _;
use log::{debug, warn};
use serde::Serialize;
use serde_json::{json, Map, Value};
use specta::Type;
use tokio::sync::{mpsc, watch};

use super::api;
pub use host::{AppHost, Host, Pasted};
pub use screen::session::Viewing;

/// The kinds this computer handles, announced in `hello`. `screen` (live view)
/// only where a window can be captured.
#[cfg(target_os = "windows")]
pub const CAPABILITIES: &[&str] = &["dictate", "text", "windows", "screen"];
#[cfg(not(target_os = "windows"))]
pub const CAPABILITIES: &[&str] = &["dictate", "text", "windows"];

/// How long the question about a new phone waits for an answer.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// How often a request waiting for that answer looks again at what else ends
/// its wait (a cancel, Phone dictation switched off, the session ending):
/// none of them signals, and a look cannot miss one.
const APPROVAL_POLL: Duration = Duration::from_millis(200);
/// How many requests a phone not yet allowed may have waiting for its
/// question; more are `busy`. Each holds its whole body (up to a 16 MiB
/// recording), so a phone nobody allowed cannot pile them up.
const MAX_WAITING: usize = 4;

/// A kind's handler, from its `async fn handle(router, body, reply)`.
type Handler = fn(Arc<Router>, Value, Reply) -> BoxFuture<'static, ()>;

/// The dispatch table. A kind not here answers `unsupported` without asking
/// anyone; a kind here runs once its request passes the approval gate.
fn handler(kind: &str) -> Option<Handler> {
    match kind {
        "dictate" => Some(|router, body, reply| dictate::handle(router, body, reply).boxed()),
        "text" => Some(|router, body, reply| text::handle(router, body, reply).boxed()),
        "windows.list" => Some(|router, body, reply| windows::list(router, body, reply).boxed()),
        "windows.keys" => Some(|router, body, reply| windows::keys(router, body, reply).boxed()),
        "screen.watch" if windows::CAN_WATCH => {
            Some(|router, body, reply| screen::session::watch(router, body, reply).boxed())
        }
        // A kind with no input slot, like the agent feature's, so the tests show
        // the gate holds for any kind. Outside `agent.`, which that feature owns.
        #[cfg(test)]
        "test.echo" => Some(|_, _, reply| async move { reply.ok(json!({})) }.boxed()),
        _ => None,
    }
}

/// One of this account's phones, as the latest `devices` snapshot lists it:
/// the one the approval question names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct AccountPhone {
    pub id: String,
    pub name: String,
    pub platform: String,
}

/// How the question about a phone ended for the requests waiting on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Allowed,
    NotApproved,
}

impl Answer {
    fn of(allowed: bool) -> Self {
        if allowed {
            Self::Allowed
        } else {
            Self::NotApproved
        }
    }
}

/// The open question about one phone, shared by all of its waiting requests.
struct Question {
    /// Tells apart a question from the next one about the same phone.
    serial: u64,
    answer: watch::Sender<Option<Answer>>,
    waiting: usize,
}

/// A request's place in a question: its serial, and where its answer comes.
type Waiter = (u64, watch::Receiver<Option<Answer>>);

/// Replies waiting for the socket. Bounded so a stalled socket cannot grow
/// memory; one request at a time sends a handful.
const OUTBOX_CAPACITY: usize = 256;

/// One request in flight, keyed by its `requestId`.
struct InFlight {
    from: String,
    cancelled: AtomicBool,
    /// Its phone lost access while it ran: it stops as a cancel does, and
    /// answers `not_approved`.
    revoked: AtomicBool,
}

/// Turns relayed bodies into handled requests and their replies into frames
/// for the socket.
pub struct Router {
    host: Arc<dyn Host>,
    outbox: mpsc::Sender<String>,
    requests: Mutex<HashMap<String, Arc<InFlight>>>,
    input_busy: AtomicBool,
    /// This account's phones, by id, from the latest `devices` snapshot.
    phones: Mutex<HashMap<String, AccountPhone>>,
    /// Open approval questions, by phone id.
    questions: Mutex<HashMap<String, Question>>,
    question_serial: AtomicU64,
    /// The windows each phone was last offered (`windows.list`), by phone id.
    offered: Mutex<HashMap<String, windows::Offered>>,
    /// A live view is running (one at a time).
    watching: AtomicBool,
    /// Stop was pressed on this computer's "Phone is viewing" notice.
    stop_watch: AtomicBool,
    /// The phone watching asked to watch again: its running view ends.
    replace_watch: AtomicBool,
    /// This account's phones that are connected now, from the latest
    /// `devices` snapshot.
    online: Mutex<Option<std::collections::HashSet<String>>>,
    /// Where a running live view hears its phone's `rtc.*` messages, by the
    /// watch's `requestId`: the phone that asked, and the session's inbox.
    watches: Mutex<HashMap<String, (String, mpsc::Sender<Value>)>>,
}

impl Router {
    /// The router and the receiving end of its replies, which the socket
    /// drains. Replies wait there while the socket reconnects.
    pub fn new(host: Arc<dyn Host>) -> (Arc<Self>, mpsc::Receiver<String>) {
        let (outbox, frames) = mpsc::channel(OUTBOX_CAPACITY);
        let router = Arc::new(Self {
            host,
            outbox,
            requests: Mutex::new(HashMap::new()),
            input_busy: AtomicBool::new(false),
            phones: Mutex::default(),
            questions: Mutex::default(),
            question_serial: AtomicU64::new(0),
            offered: Mutex::default(),
            watching: AtomicBool::new(false),
            stop_watch: AtomicBool::new(false),
            replace_watch: AtomicBool::new(false),
            online: Mutex::default(),
            watches: Mutex::default(),
        });
        (router, frames)
    }

    pub fn host(&self) -> &dyn Host {
        self.host.as_ref()
    }

    /// Whether phone input may act now: "Phone dictation" is on and the
    /// session the request came in on is still running (its socket owns the
    /// outbox's receiving end, dropped when the session ends). Checked when a
    /// request arrives and again before anything is pasted after a wait.
    pub fn input_allowed(&self) -> bool {
        !self.outbox.is_closed() && self.host.remote_input_enabled()
    }

    /// Handle a relay `body` from device `from`. Never blocks: each request
    /// runs on its own task.
    pub fn handle(self: &Arc<Self>, from: &str, body: Value) {
        let kind = body.get("kind").and_then(Value::as_str).unwrap_or_default();
        let request_id = body
            .get("requestId")
            .and_then(Value::as_str)
            .filter(|id| is_request_id(id));
        match (kind, request_id) {
            ("cancel", Some(id)) => return self.cancel(from, id),
            // A live view's handshake: never a request of its own.
            (kind, Some(id)) if kind.starts_with("rtc.") => {
                let id = id.to_string();
                return self.signal(from, &id, body.clone());
            }
            // Replies are never requests; answering one could start a loop
            // between two devices.
            ("progress" | "result", _) => return,
            (_, None) => {
                debug!("Ignoring a relayed '{kind}' message without a request id");
                return;
            }
            _ => {}
        }
        let Some(reply) = request_id.and_then(|id| self.begin(from, id)) else {
            return;
        };
        debug!("Phone request '{kind}' received");
        let Some(handler) = handler(kind) else {
            return reply.fail("unsupported");
        };
        let router = self.clone();
        tauri::async_runtime::spawn(async move {
            match router.admit(&reply).await {
                Ok(()) => handler(router, body, reply).await,
                Err(code) => reply.fail(code),
            }
        });
    }

    /// Track a new request. `None` for an id already in flight: a second set of
    /// replies under one id would end the first request early.
    fn begin(self: &Arc<Self>, from: &str, request_id: &str) -> Option<Reply> {
        let state = Arc::new(InFlight {
            from: from.to_string(),
            cancelled: AtomicBool::new(false),
            revoked: AtomicBool::new(false),
        });
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        if requests.contains_key(request_id) {
            debug!("Ignoring a request whose id is already in flight");
            return None;
        }
        requests.insert(request_id.to_string(), state.clone());
        Some(Reply {
            router: self.clone(),
            to: from.to_string(),
            request_id: request_id.to_string(),
            state,
            input: None,
        })
    }

    /// Best effort: the request answers `cancelled` itself if it stops. Only
    /// the device that asked can cancel; a finished or unknown id is ignored.
    fn cancel(&self, from: &str, request_id: &str) {
        let requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = requests.get(request_id).filter(|s| s.from == from) {
            state.cancelled.store(true, Ordering::Release);
        }
    }

    /// Hand a relayed `rtc.*` body to the live view it belongs to: the
    /// running watch with that `requestId`, from the same device. Anything
    /// else is ignored without a reply.
    fn signal(&self, from: &str, request_id: &str, body: Value) {
        let watches = self.watches();
        let Some((phone, inbox)) = watches.get(request_id) else {
            return;
        };
        if phone == from && inbox.try_send(body).is_err() {
            debug!("A live view's handshake message was dropped");
        }
    }

    fn watches(&self) -> MutexGuard<'_, HashMap<String, (String, mpsc::Sender<Value>)>> {
        self.watches.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stop on this computer's "Phone is viewing" notice: the running live
    /// view ends with `stopped`.
    pub fn stop_watching(&self) {
        if self.watching.load(Ordering::Acquire) {
            self.stop_watch.store(true, Ordering::Release);
        }
    }

    /// Whether `phone` is connected to the account server, as far as the
    /// latest devices snapshot says. Before any snapshot it is taken as
    /// connected, so a first moment never ends a view.
    fn phone_online(&self, phone: &str) -> bool {
        let online = self.online.lock().unwrap_or_else(|e| e.into_inner());
        online.as_ref().is_none_or(|online| online.contains(phone))
    }

    /// What the approval card calls a phone, for the viewing notice.
    fn phone_name(&self, id: &str) -> String {
        self.phone(id).map(|phone| phone.name).unwrap_or_default()
    }

    /// Stop the requests already past the gate whose sender `lost_access`.
    fn revoke_running(&self, lost_access: impl Fn(&str) -> bool) {
        let requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        for state in requests.values().filter(|s| lost_access(&s.from)) {
            state.revoked.store(true, Ordering::Release);
        }
    }

    /// The latest `devices` snapshot, taken by the socket before the frames
    /// after it are handled, so a request is judged by the list sent before it.
    /// A phone that left the account is answered `not_approved`, waiting or
    /// running.
    pub fn devices_changed(&self, devices: &[api::Device]) {
        *self.online.lock().unwrap_or_else(|e| e.into_inner()) = Some(
            devices
                .iter()
                .filter(|device| device.kind == "mobile" && device.online)
                .map(|device| device.id.clone())
                .collect(),
        );
        self.set_phones(
            devices
                .iter()
                .filter(|device| device.kind == "mobile")
                .map(|device| AccountPhone {
                    id: device.id.clone(),
                    name: device.name.clone(),
                    platform: device.platform.clone(),
                })
                .collect(),
        );
    }

    fn set_phones(&self, phones: Vec<AccountPhone>) {
        let phones: HashMap<_, _> = phones.into_iter().map(|p| (p.id.clone(), p)).collect();
        let gone: Vec<String> = self
            .questions()
            .keys()
            .filter(|id| !phones.contains_key(*id))
            .cloned()
            .collect();
        self.revoke_running(|from| !phones.contains_key(from));
        *self.phones.lock().unwrap_or_else(|e| e.into_inner()) = phones;
        for phone in gone {
            self.settle(&phone, None, Answer::NotApproved);
        }
    }

    /// The approval gate (`PROTOCOL.md`, "Approval"), in its order: `Ok` when
    /// the request may run, else the code it is answered with. A phone not
    /// yet asked about is asked, and this waits for the answer.
    async fn admit(self: &Arc<Self>, reply: &Reply) -> Result<(), &'static str> {
        if !self.input_allowed() {
            return Err("remote_input_off");
        }
        let phone = reply.to.as_str();
        let Some((serial, mut answer)) = self.join_question(phone)? else {
            return Ok(());
        };
        let _waiting = Waiting {
            router: self.as_ref(),
            phone,
            serial,
        };
        reply.progress("approval");
        let outcome = |answer: Option<Answer>| match answer {
            Some(Answer::Allowed) => Ok(()),
            // No answer: the question went away without one.
            _ => Err("not_approved"),
        };
        loop {
            if let Some(answered) = *answer.borrow_and_update() {
                return outcome(Some(answered));
            }
            if reply.is_cancelled() {
                return Err("cancelled");
            }
            if !self.input_allowed() {
                return Err("remote_input_off");
            }
            tokio::select! {
                changed = answer.changed() => {
                    if changed.is_err() {
                        return outcome(*answer.borrow());
                    }
                }
                _ = tokio::time::sleep(APPROVAL_POLL) => {}
            }
        }
    }

    fn phone(&self, id: &str) -> Option<AccountPhone> {
        self.phones
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn offered(&self) -> MutexGuard<'_, HashMap<String, windows::Offered>> {
        self.offered.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn questions(&self) -> MutexGuard<'_, HashMap<String, Question>> {
        self.questions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The rest of the gate, under the questions lock, so a request is never
    /// judged by an answer that is being saved at that moment: `Ok(None)`
    /// runs now, `Err` is its code, and otherwise it waits on the open
    /// question about `phone`, asked here if there is none. The host is told
    /// under the lock, so it never sees a question closed before it was asked.
    fn join_question(self: &Arc<Self>, phone: &str) -> Result<Option<Waiter>, &'static str> {
        let mut questions = self.questions();
        let Some(asking) = self.phone(phone) else {
            // Not a phone on this account: a computer, or a stale sender.
            return Err("not_approved");
        };
        if self.host.approved(phone) {
            return Ok(None);
        }
        if self.host.refused(phone) {
            return Err("not_approved");
        }
        if let Some(question) = questions.get_mut(phone) {
            if question.waiting >= MAX_WAITING {
                return Err("busy");
            }
            question.waiting += 1;
            return Ok(Some((question.serial, question.answer.subscribe())));
        }
        self.host.ask_approval(&asking);
        let serial = self.question_serial.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = watch::channel(None);
        questions.insert(
            phone.to_string(),
            Question {
                serial,
                answer,
                waiting: 1,
            },
        );
        // Its own deadline, which holds when nothing waits on it any more.
        let router = Arc::downgrade(self);
        let phone = phone.to_string();
        let timeout = self.host.approval_timeout();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(timeout).await;
            if let Some(router) = router.upgrade() {
                if router.settle(&phone, Some(serial), Answer::NotApproved) {
                    debug!("A phone's approval question timed out");
                }
            }
        });
        Ok(Some((serial, answered)))
    }

    /// The user's answer to the question about `phone` (the popup's buttons):
    /// Allow is saved and its waiting requests run; Don't allow refuses it for
    /// this run of the app and they end as `not_approved`. An answer to a
    /// question that is no longer open changes nothing.
    pub fn answer(&self, phone: &str, allow: bool) {
        let mut questions = self.questions();
        if let Some(question) = questions.remove(phone) {
            self.host.remember(phone, allow);
            self.close(phone, question, Answer::of(allow));
        }
    }

    /// Settings → Account allowed `phone` ahead of time, or took that away: an
    /// open question about it ends the same way, and taking it away also
    /// stops its requests already running.
    pub fn approval_changed(&self, phone: &str, approved: bool) {
        if !approved {
            self.revoke_running(|from| from == phone);
        }
        self.settle(phone, None, Answer::of(approved));
    }

    /// End the question about `phone` (only the one numbered `serial`, if
    /// given) with `answer`. False when there was no such question.
    fn settle(&self, phone: &str, serial: Option<u64>, answer: Answer) -> bool {
        let mut questions = self.questions();
        let open = questions
            .get(phone)
            .is_some_and(|q| serial.is_none_or(|serial| q.serial == serial));
        if !open {
            return false;
        }
        if let Some(question) = questions.remove(phone) {
            self.close(phone, question, answer);
        }
        true
    }

    /// Called with the question already out of the table, under its lock.
    fn close(&self, phone: &str, question: Question, answer: Answer) {
        question.answer.send_replace(Some(answer));
        self.host.close_approval(phone);
    }
}

impl Drop for Router {
    /// The session ended: its questions go with it.
    fn drop(&mut self) {
        for phone in self.questions().keys() {
            self.host.close_approval(phone);
        }
    }
}

/// The phone-input slot, held by one request at a time ([`Reply::take_input`]).
struct InputSlot(Arc<Router>);

impl Drop for InputSlot {
    fn drop(&mut self) {
        self.0.input_busy.store(false, Ordering::Release);
    }
}

/// One request's place in the wait for a question's answer. When the last
/// request waiting on it stops waiting because phone input ended (switched
/// off, signed out), the question is taken down; after a cancel it stays up.
struct Waiting<'a> {
    router: &'a Router,
    phone: &'a str,
    serial: u64,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let mut questions = self.router.questions();
        let Some(question) = questions
            .get_mut(self.phone)
            .filter(|q| q.serial == self.serial)
        else {
            return;
        };
        question.waiting -= 1;
        if question.waiting == 0 && !self.router.input_allowed() {
            questions.remove(self.phone);
            self.router.host.close_approval(self.phone);
        }
    }
}

/// A `requestId` worth tracking: a uuid in practice, bounded so a sender
/// cannot make the table hold large keys.
fn is_request_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.chars().all(|c| c.is_ascii_graphic())
}

/// The replies to one request. Consumed by its `result`.
pub struct Reply {
    router: Arc<Router>,
    to: String,
    request_id: String,
    state: Arc<InFlight>,
    input: Option<InputSlot>,
}

impl Reply {
    /// Its phone lost access (rather than cancelling).
    pub fn revoked(&self) -> bool {
        self.state.revoked.load(Ordering::Acquire)
    }

    /// Cancelled by its phone, or its phone lost access.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire) || self.state.revoked.load(Ordering::Acquire)
    }

    /// Take the phone-input slot (`dictate`, `text`): one request at a time,
    /// else false and the request answers `busy`. Held until its result.
    pub fn take_input(&mut self) -> bool {
        let router = &self.router;
        if router
            .input_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.input = Some(InputSlot(router.clone()));
        }
        self.input.is_some()
    }

    pub fn progress(&self, stage: &str) {
        let mut body = Map::new();
        body.insert("kind".into(), "progress".into());
        body.insert("stage".into(), stage.into());
        self.send(body);
    }

    /// `ok: true`, with `fields` (an object) beside it.
    pub fn ok(self, fields: Value) {
        let mut body = Map::new();
        body.insert("kind".into(), "result".into());
        body.insert("ok".into(), true.into());
        if let Value::Object(fields) = fields {
            body.extend(fields);
        }
        self.finish(body);
    }

    pub fn fail(self, error: &str) {
        // A request stopped because its phone lost access was not cancelled.
        let error = match error {
            "cancelled" if self.state.revoked.load(Ordering::Acquire) => "not_approved",
            error => error,
        };
        debug!("Phone request answered: {error}");
        let mut body = Map::new();
        body.insert("kind".into(), "result".into());
        body.insert("ok".into(), false.into());
        body.insert("error".into(), error.into());
        self.finish(body);
    }

    /// Send the result. What the request held is let go first, so the phone's
    /// next request, sent the moment this arrives, finds the input slot free.
    fn finish(mut self, body: Map<String, Value>) {
        self.release();
        self.send(body);
    }

    fn release(&mut self) {
        self.input = None;
        let mut requests = self
            .router
            .requests
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if requests
            .get(&self.request_id)
            .is_some_and(|s| Arc::ptr_eq(s, &self.state))
        {
            requests.remove(&self.request_id);
        }
    }

    /// Send a reply body to the requester, under this request's id.
    fn send(&self, mut body: Map<String, Value>) {
        body.insert("requestId".into(), self.request_id.clone().into());
        let frame = json!({ "type": "relay", "to": self.to, "body": body }).to_string();
        if self.router.outbox.try_send(frame).is_err() {
            warn!("A reply to a phone request was dropped: the connection is backed up or gone");
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::host::{AudioStart, VideoStart};
    use super::screen::audio::StartError as AudioStartError;
    use super::screen::input::{Button, ControlInfo, Rect};
    use super::screen::rtc::IceServer;
    use super::screen::session::Viewing;
    use super::screen::video::{
        CaptureTarget, EncodedFrame, Ended, Geometry, ScreenInfo, StartError, VideoSettings,
        VideoStream,
    };
    use super::{AccountPhone, Host, Pasted, Router, APPROVAL_TIMEOUT};
    use crate::command_mode::effects::{KeyCombo, WindowEntry};
    use futures_util::future::BoxFuture;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;

    /// A [`Host`] without an app: switches, a canned transcript, a record of
    /// what was pasted and saved, and approval answered by the test.
    pub struct FakeHost {
        pub enabled: AtomicBool,
        pub ready: bool,
        /// `None` fails the transcription.
        pub transcript: Option<String>,
        pub paste: Pasted,
        pub pasted: Mutex<Vec<String>>,
        pub saved: Mutex<Vec<String>>,
        /// Held by `transcribe` until released, to test busy and cancel.
        pub gate: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        /// Allowed phones: "phone", so its requests run at once.
        pub approved: Mutex<HashSet<String>>,
        pub refused: Mutex<HashSet<String>>,
        /// Every approval question asked, and the ids of those still open.
        pub asked: Mutex<Vec<AccountPhone>>,
        pub open: Mutex<Vec<String>>,
        pub approval_timeout: Duration,
        /// Run once by `refused`, after it has read its answer: what happens
        /// while a request is being judged.
        pub while_judging: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        /// "Let phones see and use windows".
        pub windows_on: AtomicBool,
        /// Every phone counts as allowed (a live test serving a real phone).
        pub approve_all: bool,
        /// Views capture this machine for real (a live test).
        pub capture_for_real: bool,
        /// The open windows, and the id of the one in front.
        pub windows: Mutex<Vec<WindowEntry>>,
        pub front: Mutex<Option<i64>>,
        /// Every window brought forward, in order.
        pub focused: Mutex<Vec<i64>>,
        pub focus_fails: Mutex<bool>,
        /// Every combo pressed, as its label.
        pub pressed: Mutex<Vec<String>>,
        /// The window in front at each paste.
        pub pasted_into: Mutex<Vec<Option<i64>>>,
        /// Live view: the screens offered, the capture `start_video` hands
        /// out (taken once), every click and scroll, the control under a
        /// point, and every viewing notice shown (`None` = taken down).
        pub screens: Vec<ScreenInfo>,
        pub video: Mutex<Option<FakeVideo>>,
        pub clicks: Mutex<Vec<(i32, i32, Button, u8)>>,
        pub scrolls: Mutex<Vec<(i32, i32, i32, i32)>>,
        pub control: Mutex<Option<ControlInfo>>,
        pub notices: Mutex<Vec<Option<Viewing>>>,
    }

    /// A capture the test drives: frames go in through `frames`.
    pub struct FakeVideo {
        pub frames: Option<mpsc::Receiver<EncodedFrame>>,
        pub geometry: Option<Geometry>,
        pub keyframes: Arc<AtomicUsize>,
        pub stopped: Arc<AtomicBool>,
    }

    struct FakeStream {
        geometry: Option<Geometry>,
        keyframes: Arc<AtomicUsize>,
        stopped: Arc<AtomicBool>,
    }

    impl VideoStream for FakeStream {
        fn request_keyframe(&self) {
            self.keyframes.fetch_add(1, Ordering::SeqCst);
        }
        fn set_bitrate(&self, _bps: u32) {}
        fn geometry(&self) -> Option<Geometry> {
            self.geometry
        }
        fn ended(&self) -> Option<Ended> {
            Some(Ended::TargetGone)
        }
        fn stop(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
    }

    impl Default for FakeHost {
        fn default() -> Self {
            Self {
                enabled: AtomicBool::new(true),
                ready: true,
                transcript: Some("hello there".into()),
                paste: Pasted::Done,
                pasted: Mutex::default(),
                saved: Mutex::default(),
                gate: Mutex::default(),
                approved: Mutex::new(HashSet::from(["phone".to_string()])),
                refused: Mutex::default(),
                asked: Mutex::default(),
                open: Mutex::default(),
                approval_timeout: APPROVAL_TIMEOUT,
                while_judging: Mutex::default(),
                windows_on: AtomicBool::new(true),
                approve_all: false,
                capture_for_real: false,
                windows: Mutex::default(),
                front: Mutex::default(),
                focused: Mutex::default(),
                focus_fails: Mutex::default(),
                pressed: Mutex::default(),
                pasted_into: Mutex::default(),
                screens: Vec::new(),
                video: Mutex::default(),
                clicks: Mutex::default(),
                scrolls: Mutex::default(),
                control: Mutex::default(),
                notices: Mutex::default(),
            }
        }
    }

    impl Host for FakeHost {
        fn remote_input_enabled(&self) -> bool {
            self.enabled.load(Ordering::SeqCst)
        }
        fn speech_ready(&self) -> bool {
            self.ready
        }
        fn transcribe(&self, _samples: Vec<f32>) -> BoxFuture<'static, Option<String>> {
            let transcript = self.transcript.clone();
            let gate = self.gate.lock().unwrap().take();
            Box::pin(async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                transcript
            })
        }
        fn paste(&self, text: String) -> BoxFuture<'static, Pasted> {
            self.pasted.lock().unwrap().push(text);
            let front = *self.front.lock().unwrap();
            self.pasted_into.lock().unwrap().push(front);
            let pasted = self.paste;
            Box::pin(async move { pasted })
        }
        fn save_history(&self, _wav: Vec<u8>, text: String) {
            self.saved.lock().unwrap().push(text);
        }
        fn approved(&self, phone: &str) -> bool {
            self.approve_all || self.approved.lock().unwrap().contains(phone)
        }
        fn refused(&self, phone: &str) -> bool {
            let refused = self.refused.lock().unwrap().contains(phone);
            let judging = self.while_judging.lock().unwrap().take();
            if let Some(judging) = judging {
                judging();
            }
            refused
        }
        fn remember(&self, phone: &str, allowed: bool) {
            if allowed {
                self.approved.lock().unwrap().insert(phone.to_string());
                self.refused.lock().unwrap().remove(phone);
            } else {
                self.refused.lock().unwrap().insert(phone.to_string());
            }
        }
        fn ask_approval(&self, phone: &AccountPhone) {
            self.asked.lock().unwrap().push(phone.clone());
            self.open.lock().unwrap().push(phone.id.clone());
        }
        fn close_approval(&self, phone: &str) {
            self.open.lock().unwrap().retain(|id| id != phone);
        }
        fn approval_timeout(&self) -> Duration {
            self.approval_timeout
        }
        fn windows_enabled(&self) -> bool {
            self.windows_on.load(Ordering::SeqCst)
        }
        fn open_windows(&self) -> Vec<WindowEntry> {
            self.windows.lock().unwrap().clone()
        }
        fn foreground_window(&self) -> Option<WindowEntry> {
            let front = (*self.front.lock().unwrap())?;
            self.open_windows().into_iter().find(|w| w.id == front)
        }
        fn focus_window(&self, window: WindowEntry) -> BoxFuture<'static, Result<(), String>> {
            let result = if *self.focus_fails.lock().unwrap() {
                Err("refused".to_string())
            } else {
                self.focused.lock().unwrap().push(window.id);
                *self.front.lock().unwrap() = Some(window.id);
                Ok(())
            };
            Box::pin(async move { result })
        }
        fn press_keys(&self, combos: Vec<KeyCombo>) -> BoxFuture<'static, Result<(), String>> {
            self.pressed
                .lock()
                .unwrap()
                .extend(combos.iter().map(KeyCombo::label));
            Box::pin(async { Ok(()) })
        }
        fn screens(&self) -> Vec<ScreenInfo> {
            self.screens.clone()
        }
        fn start_video(&self, target: CaptureTarget, settings: VideoSettings) -> VideoStart {
            if self.capture_for_real {
                let target = match target {
                    CaptureTarget::Window(_) => CaptureTarget::Screen(
                        super::screen::video::list_screens()
                            .into_iter()
                            .next()
                            .map(|screen| screen.id)
                            .unwrap_or_default(),
                    ),
                    screen => screen,
                };
                return super::screen::video::start(target, settings);
            }
            let Some(mut video) = self.video.lock().unwrap().take() else {
                return Err(StartError::Unsupported("no fake capture".into()));
            };
            let frames = video.frames.take().expect("frames");
            Ok((
                Box::new(FakeStream {
                    geometry: video.geometry,
                    keyframes: video.keyframes,
                    stopped: video.stopped,
                }),
                frames,
            ))
        }
        fn start_audio(&self) -> AudioStart {
            Err(AudioStartError::Unsupported("no sound in tests".into()))
        }
        fn ice_servers(&self) -> BoxFuture<'static, Vec<IceServer>> {
            Box::pin(async { Vec::new() })
        }
        fn click(
            &self,
            x: i32,
            y: i32,
            button: Button,
            count: u8,
        ) -> BoxFuture<'static, Result<(), String>> {
            self.clicks.lock().unwrap().push((x, y, button, count));
            Box::pin(async { Ok(()) })
        }
        fn scroll(
            &self,
            x: i32,
            y: i32,
            dx: i32,
            dy: i32,
        ) -> BoxFuture<'static, Result<(), String>> {
            self.scrolls.lock().unwrap().push((x, y, dx, dy));
            Box::pin(async { Ok(()) })
        }
        fn control_at(&self, _x: i32, _y: i32) -> BoxFuture<'static, Option<ControlInfo>> {
            let control = self.control.lock().unwrap().clone();
            Box::pin(async move { control })
        }
        fn focused_is_password(&self) -> BoxFuture<'static, bool> {
            Box::pin(async { false })
        }
        fn caret(&self) -> BoxFuture<'static, Option<Rect>> {
            Box::pin(async { None })
        }
        fn viewing(&self, notice: Option<Viewing>) {
            self.notices.lock().unwrap().push(notice);
        }
    }

    pub fn phone(id: &str) -> AccountPhone {
        AccountPhone {
            id: id.to_string(),
            name: format!("{id} name"),
            platform: "android".to_string(),
        }
    }

    /// A router whose account has two phones: "phone" (allowed by the
    /// default host) and "phone2".
    pub fn router(host: Arc<FakeHost>) -> (Arc<Router>, mpsc::Receiver<String>) {
        let (router, frames) = Router::new(host);
        router.set_phones(vec![phone("phone"), phone("phone2")]);
        (router, frames)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{self, FakeHost};
    use super::*;
    use std::time::Duration;

    fn router(host: FakeHost) -> (Arc<Router>, Arc<FakeHost>, mpsc::Receiver<String>) {
        let host = Arc::new(host);
        let (router, frames) = testing::router(host.clone());
        (router, host, frames)
    }

    async fn next(frames: &mut mpsc::Receiver<String>) -> Value {
        next_to(frames, "phone").await
    }

    async fn next_to(frames: &mut mpsc::Receiver<String>, to: &str) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .expect("a reply")
            .expect("open outbox");
        let frame: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(frame["type"], "relay");
        assert_eq!(frame["to"], to);
        frame["body"].clone()
    }

    async fn nothing(frames: &mut mpsc::Receiver<String>) {
        assert!(
            tokio::time::timeout(Duration::from_millis(200), frames.recv())
                .await
                .is_err(),
            "expected no reply"
        );
    }

    #[tokio::test]
    async fn an_unknown_kind_with_a_request_id_is_unsupported() {
        let (router, _, mut frames) = router(FakeHost::default());
        router.handle("phone", json!({ "kind": "nope.kind", "requestId": "r1" }));
        let body = next(&mut frames).await;
        assert_eq!(
            body,
            json!({ "kind": "result", "requestId": "r1", "ok": false, "error": "unsupported" })
        );
        assert!(router.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn messages_without_a_request_id_and_replies_are_ignored() {
        let (router, _, mut frames) = router(FakeHost::default());
        router.handle("phone", json!({ "kind": "agent.run" }));
        router.handle("phone", json!({ "kind": "text", "text": "hi" }));
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "", "text": "hi" }),
        );
        router.handle(
            "phone",
            json!({ "kind": "result", "requestId": "r1", "ok": true }),
        );
        router.handle("phone", json!({ "kind": "progress", "requestId": "r1" }));
        router.handle("phone", json!({ "kind": "cancel", "requestId": "nope" }));
        router.handle("phone", json!("not an object"));
        nothing(&mut frames).await;
    }

    #[tokio::test]
    async fn text_is_pasted() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r1", "text": "hi" }),
        );
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "progress", "requestId": "r1", "stage": "pasting" })
        );
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "r1", "ok": true, "pasted": true })
        );
        assert_eq!(*host.pasted.lock().unwrap(), vec!["hi".to_string()]);
    }

    #[tokio::test]
    async fn text_is_refused_when_remote_input_is_off() {
        let (router, host, mut frames) = router(FakeHost {
            enabled: false.into(),
            ..FakeHost::default()
        });
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r1", "text": "hi" }),
        );
        assert_eq!(next(&mut frames).await["error"], "remote_input_off");
        assert!(host.pasted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn text_outside_the_limits_is_unsupported() {
        let (router, host, mut frames) = router(FakeHost::default());
        for (id, text) in [
            ("a", json!("")),
            ("b", json!("x".repeat(10_001))),
            ("c", json!(5)),
        ] {
            router.handle(
                "phone",
                json!({ "kind": "text", "requestId": id, "text": text }),
            );
            assert_eq!(next(&mut frames).await["error"], "unsupported");
        }
        assert!(host.pasted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_paste_says_so() {
        for (pasted, code) in [(Pasted::Failed, "paste_failed"), (Pasted::Off, "paste_off")] {
            let (router, _, mut frames) = router(FakeHost {
                paste: pasted,
                ..FakeHost::default()
            });
            router.handle(
                "phone",
                json!({ "kind": "text", "requestId": "r1", "text": "hi" }),
            );
            assert_eq!(next(&mut frames).await["stage"], "pasting");
            assert_eq!(next(&mut frames).await["error"], code);
        }
    }

    #[tokio::test]
    async fn one_phone_request_at_a_time_and_cancel() {
        let (release, gate) = tokio::sync::oneshot::channel();
        let host = FakeHost::default();
        *host.gate.lock().unwrap() = Some(gate);
        let (router, host, mut frames) = router(host);

        let audio = dictate::tests::wav_base64(16_000);
        router.handle(
            "phone",
            json!({ "kind": "dictate", "requestId": "r1", "deliver": "paste", "audio": audio }),
        );
        assert_eq!(next(&mut frames).await["stage"], "transcribing");

        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r2", "text": "hi" }),
        );
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "r2", "ok": false, "error": "busy" })
        );

        // Another device cannot cancel it; the one that asked can.
        router.handle("laptop", json!({ "kind": "cancel", "requestId": "r1" }));
        router.handle("phone", json!({ "kind": "cancel", "requestId": "r1" }));
        release.send(()).unwrap();
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "r1", "ok": false, "error": "cancelled" })
        );
        assert!(host.pasted.lock().unwrap().is_empty());
        assert!(host.saved.lock().unwrap().is_empty());

        // The slot is free again.
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r3", "text": "hi" }),
        );
        assert_eq!(next(&mut frames).await["stage"], "pasting");
        assert_eq!(next(&mut frames).await["ok"], true);
    }

    fn text(id: &str) -> Value {
        json!({ "kind": "text", "requestId": id, "text": "hi" })
    }

    fn approval(id: &str) -> Value {
        json!({ "kind": "progress", "requestId": id, "stage": "approval" })
    }

    fn failed(id: &str, error: &str) -> Value {
        json!({ "kind": "result", "requestId": id, "ok": false, "error": error })
    }

    /// The next `count` replies to phone2, in `requestId` order.
    async fn replies(frames: &mut mpsc::Receiver<String>, count: usize) -> Vec<Value> {
        let mut bodies = Vec::new();
        for _ in 0..count {
            bodies.push(next_to(frames, "phone2").await);
        }
        bodies.sort_by_key(|b| b["requestId"].as_str().unwrap().to_string());
        bodies
    }

    #[tokio::test]
    async fn a_new_phone_is_asked_and_runs_once_allowed() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        assert_eq!(*host.asked.lock().unwrap(), [testing::phone("phone2")]);
        assert_eq!(*host.open.lock().unwrap(), ["phone2"]);
        assert!(host.pasted.lock().unwrap().is_empty());

        router.answer("phone2", true);
        assert_eq!(next_to(&mut frames, "phone2").await["stage"], "pasting");
        assert_eq!(next_to(&mut frames, "phone2").await["ok"], true);
        assert!(host.approved.lock().unwrap().contains("phone2"));
        assert!(host.open.lock().unwrap().is_empty());

        // Allowed is remembered: the next request runs without a question.
        router.handle("phone2", text("r2"));
        assert_eq!(next_to(&mut frames, "phone2").await["stage"], "pasting");
        assert_eq!(next_to(&mut frames, "phone2").await["ok"], true);
        assert_eq!(host.asked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn dont_allow_refuses_now_and_later_without_asking() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        router.answer("phone2", false);
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r1", "not_approved")
        );
        assert!(host.refused.lock().unwrap().contains("phone2"));
        assert!(host.open.lock().unwrap().is_empty());

        router.handle("phone2", text("r2"));
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r2", "not_approved")
        );
        assert_eq!(host.asked.lock().unwrap().len(), 1);
        assert!(host.pasted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unanswered_question_times_out_without_refusing() {
        let (router, host, mut frames) = router(FakeHost {
            approval_timeout: Duration::from_millis(300),
            ..FakeHost::default()
        });
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r1", "not_approved")
        );
        assert!(host.open.lock().unwrap().is_empty());
        assert!(!host.refused.lock().unwrap().contains("phone2"));

        // An answer after the question is gone changes nothing.
        router.answer("phone2", true);
        assert!(!host.approved.lock().unwrap().contains("phone2"));

        // No answer is not Don't allow: the next request asks again.
        router.handle("phone2", text("r2"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r2"));
        assert_eq!(host.asked.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn requests_from_one_phone_share_one_question() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone2", json!({ "kind": "test.echo", "requestId": "r1" }));
        router.handle("phone2", json!({ "kind": "test.echo", "requestId": "r2" }));
        assert_eq!(
            replies(&mut frames, 2).await,
            [approval("r1"), approval("r2")]
        );
        assert_eq!(host.asked.lock().unwrap().len(), 1);

        router.answer("phone2", true);
        let done = replies(&mut frames, 2).await;
        assert_eq!(
            done,
            [
                json!({ "kind": "result", "requestId": "r1", "ok": true }),
                json!({ "kind": "result", "requestId": "r2", "ok": true }),
            ]
        );
        assert!(host.open.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn every_handled_kind_passes_the_gate() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone", json!({ "kind": "test.echo", "requestId": "r1" }));
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "r1", "ok": true })
        );
        host.enabled.store(false, Ordering::SeqCst);
        router.handle("phone", json!({ "kind": "test.echo", "requestId": "r2" }));
        assert_eq!(next(&mut frames).await, failed("r2", "remote_input_off"));
        router.handle("laptop", json!({ "kind": "test.echo", "requestId": "r3" }));
        assert_eq!(
            next_to(&mut frames, "laptop").await,
            failed("r3", "remote_input_off")
        );
        host.enabled.store(true, Ordering::SeqCst);
        router.handle("phone2", json!({ "kind": "test.echo", "requestId": "r4" }));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r4"));
        router.answer("phone2", false);
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r4", "not_approved")
        );
    }

    #[tokio::test]
    async fn a_cancel_ends_a_waiting_request_but_not_the_question() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone2", text("r1"));
        router.handle("phone2", text("r2"));
        assert_eq!(
            replies(&mut frames, 2).await,
            [approval("r1"), approval("r2")]
        );

        // Cancel is never gated: a phone not yet allowed can cancel its own.
        router.handle("phone2", json!({ "kind": "cancel", "requestId": "r1" }));
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r1", "cancelled")
        );
        assert_eq!(*host.open.lock().unwrap(), ["phone2"]);

        router.handle("phone2", json!({ "kind": "cancel", "requestId": "r2" }));
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r2", "cancelled")
        );
        assert!(router.requests.lock().unwrap().is_empty());
        // Still asked, so a phone cannot flash the question on and off: the
        // next request waits on it, and the answer still counts.
        assert_eq!(*host.open.lock().unwrap(), ["phone2"]);
        router.handle("phone2", text("r3"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r3"));
        assert_eq!(host.asked.lock().unwrap().len(), 1);
        router.answer("phone2", true);
        assert_eq!(next_to(&mut frames, "phone2").await["stage"], "pasting");
        assert_eq!(next_to(&mut frames, "phone2").await["ok"], true);
        assert!(host.open.lock().unwrap().is_empty());

        // A refused or unknown sender's cancel is not answered either.
        host.refused.lock().unwrap().insert("phone2".into());
        router.handle("phone2", json!({ "kind": "cancel", "requestId": "r9" }));
        router.handle("laptop", json!({ "kind": "cancel", "requestId": "r9" }));
        nothing(&mut frames).await;
    }

    /// Wait until `done` holds, for at most five seconds.
    async fn until(done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "never happened");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn cancel_waiting(router: &Arc<Router>, frames: &mut mpsc::Receiver<String>, id: &str) {
        router.handle("phone2", text(id));
        assert_eq!(next_to(frames, "phone2").await, approval(id));
        router.handle("phone2", json!({ "kind": "cancel", "requestId": id }));
        assert_eq!(next_to(frames, "phone2").await, failed(id, "cancelled"));
    }

    #[tokio::test]
    async fn a_question_nobody_waits_on_ends_with_its_time_or_its_session() {
        let (router, host, mut frames) = router(FakeHost {
            approval_timeout: Duration::from_millis(300),
            ..FakeHost::default()
        });
        cancel_waiting(&router, &mut frames, "r1").await;
        assert_eq!(*host.open.lock().unwrap(), ["phone2"]);
        until(|| host.open.lock().unwrap().is_empty()).await;
        assert!(!host.refused.lock().unwrap().contains("phone2"));

        let (router, host, mut frames) = self::router(FakeHost::default());
        cancel_waiting(&router, &mut frames, "r1").await;
        assert_eq!(*host.open.lock().unwrap(), ["phone2"]);
        drop(router);
        until(|| host.open.lock().unwrap().is_empty()).await;
    }

    #[tokio::test]
    async fn a_phone_not_yet_allowed_has_only_a_few_requests_waiting() {
        let (router, host, mut frames) = router(FakeHost::default());
        for i in 0..=MAX_WAITING {
            router.handle(
                "phone2",
                json!({ "kind": "test.echo", "requestId": format!("r{i}") }),
            );
        }
        let bodies = replies(&mut frames, MAX_WAITING + 1).await;
        let busy: Vec<_> = bodies.iter().filter(|b| b["error"] == "busy").collect();
        assert_eq!(busy.len(), 1, "{bodies:?}");
        assert_eq!(
            bodies.iter().filter(|b| b["stage"] == "approval").count(),
            MAX_WAITING
        );
        // The busy one is gone, body and all.
        assert_eq!(router.requests.lock().unwrap().len(), MAX_WAITING);
        assert_eq!(host.asked.lock().unwrap().len(), 1);

        router.answer("phone2", true);
        let done = replies(&mut frames, MAX_WAITING).await;
        assert!(done.iter().all(|b| b["ok"] == true), "{done:?}");
    }

    #[tokio::test]
    async fn an_answer_saved_while_a_request_is_judged_is_not_asked_again() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        // The user clicks Don't allow just after r2 was found not refused.
        let answering = router.clone();
        *host.while_judging.lock().unwrap() = Some(Box::new(move || {
            std::thread::spawn(move || answering.answer("phone2", false));
            std::thread::sleep(Duration::from_millis(100));
        }));
        router.handle("phone2", text("r2"));
        // r2 waits on r1's question, and that answer ends both.
        assert_eq!(
            replies(&mut frames, 3).await,
            [
                failed("r1", "not_approved"),
                approval("r2"),
                failed("r2", "not_approved")
            ]
        );
        assert_eq!(host.asked.lock().unwrap().len(), 1);
        assert!(host.open.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_the_accounts_phones_are_asked() {
        let host = Arc::new(FakeHost::default());
        // Even an id that was allowed, if it is a computer.
        host.approved.lock().unwrap().insert("laptop".into());
        let (router, mut frames) = Router::new(host.clone());
        let devices: Vec<api::Device> = serde_json::from_value(json!([
            { "id": "laptop", "name": "PC", "platform": "windows", "kind": "desktop",
              "online": true, "lastSeenAt": 1, "current": false },
            { "id": "phone2", "name": "Pixel", "platform": "android", "kind": "mobile",
              "online": true, "lastSeenAt": 1, "current": false },
        ]))
        .unwrap();
        router.devices_changed(&devices);

        for (from, id) in [("laptop", "r1"), ("stranger", "r2")] {
            router.handle(from, text(id));
            assert_eq!(next_to(&mut frames, from).await, failed(id, "not_approved"));
        }
        assert!(host.asked.lock().unwrap().is_empty());
        assert!(host.pasted.lock().unwrap().is_empty());

        router.handle("phone2", text("r3"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r3"));
        assert_eq!(
            *host.asked.lock().unwrap(),
            [AccountPhone {
                id: "phone2".into(),
                name: "Pixel".into(),
                platform: "android".into(),
            }]
        );
    }

    #[tokio::test]
    async fn phone_dictation_off_is_checked_before_asking() {
        let (router, host, mut frames) = router(FakeHost {
            enabled: false.into(),
            ..FakeHost::default()
        });
        router.handle("phone2", text("r1"));
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r1", "remote_input_off")
        );
        assert!(host.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn switching_off_or_signing_out_while_waiting_ends_the_question() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        host.enabled.store(false, Ordering::SeqCst);
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r1", "remote_input_off")
        );
        assert!(host.open.lock().unwrap().is_empty());

        host.enabled.store(true, Ordering::SeqCst);
        router.handle("phone2", text("r2"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r2"));
        // Ending the session drops the outbox's receiver.
        drop(frames);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !router.requests.lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the request never ended"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(host.open.lock().unwrap().is_empty());
        assert!(host.pasted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn settings_and_the_device_list_answer_a_waiting_question() {
        let (router, host, mut frames) = router(FakeHost::default());
        // Access removed in Settings while it waits.
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        router.approval_changed("phone2", false);
        assert_eq!(
            next_to(&mut frames, "phone2").await,
            failed("r1", "not_approved")
        );
        assert!(host.open.lock().unwrap().is_empty());

        // Allowed in Settings while it waits: it runs.
        router.handle("phone2", text("r2"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r2"));
        router.approval_changed("phone2", true);
        assert_eq!(next_to(&mut frames, "phone2").await["stage"], "pasting");
        assert_eq!(next_to(&mut frames, "phone2").await["ok"], true);

        // The phone leaves the account while it waits.
        router.handle("phone3", text("r3"));
        assert_eq!(
            next_to(&mut frames, "phone3").await,
            failed("r3", "not_approved")
        );
        router.set_phones(vec![testing::phone("phone"), testing::phone("phone3")]);
        router.handle("phone3", text("r4"));
        assert_eq!(next_to(&mut frames, "phone3").await, approval("r4"));
        router.set_phones(vec![testing::phone("phone")]);
        assert_eq!(
            next_to(&mut frames, "phone3").await,
            failed("r4", "not_approved")
        );
        assert!(host.open.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn waiting_does_not_hold_the_input_slot() {
        let (router, _, mut frames) = router(FakeHost::default());
        router.handle("phone2", text("r1"));
        assert_eq!(next_to(&mut frames, "phone2").await, approval("r1"));
        router.handle("phone", text("r2"));
        assert_eq!(next(&mut frames).await["stage"], "pasting");
        assert_eq!(next(&mut frames).await["ok"], true);
    }
}
