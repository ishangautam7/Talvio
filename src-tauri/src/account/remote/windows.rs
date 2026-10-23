//! `windows.list` and `windows.keys`, and the `target` that aims `text` and
//! `dictate` at one window (`PROTOCOL.md`, "Windows").
//!
//! A phone never names a window handle. `windows.list` makes up an id for each
//! window it offers and keeps them per phone ([`Offered`]); only those ids are
//! accepted, for [`ID_LIFETIME`] after the phone's latest list. A window keeps
//! its id across that phone's lists while it stays open, so a phone holding an
//! id can list again without losing its choice. An id is checked against the
//! windows open right now before anything is sent to it, so a closed window (or
//! a handle Windows has since reused) is `target_gone`.
//!
//! Aiming ([`aim`]) brings the target forward, delivers, and gives the
//! foreground back to the window that had it, so someone working at the
//! computer is interrupted for a moment, not moved. The keys a phone may press
//! are command mode's: the same parser and the same refused list
//! (`tools/press_keys.rs`), except that Enter is pressed without asking, since
//! a person pressed it on purpose.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::debug;
use serde_json::{json, Value};

use super::{Reply, Router};
use crate::command_mode::effects::{KeyCombo, WindowEntry};
use crate::command_mode::registry::Platform;
use crate::command_mode::tools::press_keys::{deny_reason, parse_combo};

/// The most windows one `windows.list` offers.
const MAX_LISTED: usize = 60;
/// How long the ids of one `windows.list` stay good.
const ID_LIFETIME: Duration = Duration::from_secs(30 * 60);
/// The most combos one `windows.keys` presses.
const MAX_KEYS: usize = 20;
/// A combo longer than this is not one anybody types.
const MAX_COMBO_CHARS: usize = 64;
/// Between delivering to the target and handing the foreground back: a
/// synthetic key is processed by the window that has the keyboard when the
/// system reads it, so it must not move before then.
const RESTORE_DELAY: Duration = Duration::from_millis(150);

/// Ids are never reused, so an id from an earlier list cannot come to mean
/// another window.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The windows one phone was last offered, by id.
pub(super) struct Offered {
    issued: Instant,
    windows: HashMap<String, WindowEntry>,
}

/// Where a request's input goes.
pub(super) struct Target {
    window: WindowEntry,
    /// Leave it in front afterwards.
    stay: bool,
}

/// `windows.list`: the open windows, the one in front first.
pub(super) async fn list(router: Arc<Router>, _body: Value, reply: Reply) {
    if let Err(code) = allowed(&router) {
        return reply.fail(code);
    }
    let host = router.host();
    let front = host.foreground_window().map(|w| w.id);
    let mut windows = host.open_windows();
    // Stable, so the rest keep the platform's front-to-back order.
    windows.sort_by_key(|w| Some(w.id) != front);
    windows.truncate(MAX_LISTED);

    let mut table = router.offered();
    let earlier = table.remove(&reply.to);
    let mut offered = HashMap::new();
    let listed: Vec<Value> = windows
        .into_iter()
        .map(|window| {
            // The same window (same handle, same app) keeps its id.
            let id = earlier
                .as_ref()
                .and_then(|earlier| {
                    earlier
                        .windows
                        .iter()
                        .find(|(_, seen)| seen.id == window.id && seen.app == window.app)
                })
                .map(|(id, _)| id.clone())
                .unwrap_or_else(|| format!("w{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
            let item = json!({
                "id": id,
                "title": window.title,
                "app": window.app,
                "front": Some(window.id) == front,
            });
            offered.insert(id, window);
            item
        })
        .collect();
    table.insert(
        reply.to.clone(),
        Offered {
            issued: Instant::now(),
            windows: offered,
        },
    );
    drop(table);
    reply.ok(json!({ "windows": listed }))
}

/// `windows.keys`: press combos in the target, or in the window in front.
pub(super) async fn keys(router: Arc<Router>, body: Value, mut reply: Reply) {
    if let Err(code) = allowed(&router) {
        return reply.fail(code);
    }
    if !reply.take_input() {
        return reply.fail("busy");
    }
    let combos = match combos(&body) {
        Ok(combos) => combos,
        Err(code) => return reply.fail(code),
    };
    let target = match target(&router, &reply, &body) {
        Ok(target) => target,
        Err(code) => return reply.fail(code),
    };
    if reply.is_cancelled() {
        return reply.fail("cancelled");
    }
    let host = router.host();
    let count = combos.len();
    match aim(&router, target.as_ref(), || host.press_keys(combos)).await {
        Ok(Ok(())) => reply.ok(json!({ "pressed": count })),
        Ok(Err(e)) => {
            debug!("A phone's keys were not pressed: {e}");
            reply.fail("keys_failed")
        }
        Err(code) => reply.fail(code),
    }
}

/// Phone input is on, and so is "Let phones choose windows".
fn allowed(router: &Router) -> Result<(), &'static str> {
    if !router.input_allowed() {
        return Err("remote_input_off");
    }
    if !router.host().windows_enabled() {
        return Err("remote_windows_off");
    }
    Ok(())
}

/// The request's `keys`, parsed and checked: `unsupported` for one that cannot
/// be read, `key_refused` for one on command mode's refused list. Nothing is
/// pressed unless every combo passes.
fn combos(body: &Value) -> Result<Vec<KeyCombo>, &'static str> {
    let keys = body
        .get("keys")
        .and_then(Value::as_array)
        .filter(|keys| (1..=MAX_KEYS).contains(&keys.len()))
        .ok_or("unsupported")?;
    let platform = Platform::current();
    keys.iter()
        .map(|key| {
            let key = key
                .as_str()
                .filter(|key| key.chars().count() <= MAX_COMBO_CHARS)
                .ok_or("unsupported")?;
            let combo = parse_combo(key, platform).map_err(|_| "unsupported")?;
            match deny_reason(&combo, 1, platform) {
                Some(reason) => {
                    debug!("A phone's {} was refused: {reason}", combo.label());
                    Err("key_refused")
                }
                None => Ok(combo),
            }
        })
        .collect()
}

/// The window a request names in `target`, if it names one. Naming one needs
/// "Let phones choose windows" on, and an id this phone was offered that has
/// not expired.
pub(super) fn target(
    router: &Router,
    reply: &Reply,
    body: &Value,
) -> Result<Option<Target>, &'static str> {
    let id = match body.get("target") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(id)) => id,
        Some(_) => return Err("unsupported"),
    };
    if !router.host().windows_enabled() {
        return Err("remote_windows_off");
    }
    let stay = match body.get("stay") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(stay)) => *stay,
        Some(_) => return Err("unsupported"),
    };
    let offered = router.offered();
    let window = offered
        .get(&reply.to)
        .filter(|offered| offered.issued.elapsed() < ID_LIFETIME)
        .and_then(|offered| offered.windows.get(id))
        .ok_or("target_gone")?;
    Ok(Some(Target {
        window: window.clone(),
        stay,
    }))
}

/// Start `deliver` once `target` is in front, then give the foreground back to the
/// window that had it (unless the target was already in front, or `stay`).
/// Without a target it just runs. `Err` is the code the request fails with;
/// nothing was delivered then.
pub(super) async fn aim<T, F: Future<Output = T>>(
    router: &Router,
    target: Option<&Target>,
    deliver: impl FnOnce() -> F,
) -> Result<T, &'static str> {
    let Some(target) = target else {
        return Ok(deliver().await);
    };
    let host = router.host();
    let window = &target.window;
    // Open right now, as the same app: a handle can be reused once its
    // window closes.
    if !host
        .open_windows()
        .iter()
        .any(|open| open.id == window.id && open.app == window.app)
    {
        return Err("target_gone");
    }
    let front = host.foreground_window();
    let in_front = front.as_ref().is_some_and(|front| front.id == window.id);
    let previous = front.filter(|_| !in_front);
    if !in_front {
        if let Err(e) = host.focus_window(window.clone()).await {
            debug!("A phone's target window would not come forward: {e}");
            return Err("focus_failed");
        }
    }
    let delivered = deliver().await;
    if let Some(previous) = previous.filter(|_| !target.stay) {
        tokio::time::sleep(RESTORE_DELAY).await;
        if let Err(e) = host.focus_window(previous).await {
            debug!("The window in front before a phone's input did not come back: {e}");
        }
    }
    Ok(delivered)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{self, FakeHost};
    use super::*;
    use tokio::sync::mpsc;

    fn window(id: i64, title: &str, app: &str) -> WindowEntry {
        WindowEntry {
            id,
            title: title.into(),
            app: app.into(),
        }
    }

    fn host() -> FakeHost {
        let host = FakeHost::default();
        *host.windows.lock().unwrap() = vec![
            window(1, "Inbox", "Outlook"),
            window(2, "Claude Code", "WindowsTerminal"),
            window(3, "notes.txt", "Notepad"),
        ];
        *host.front.lock().unwrap() = Some(2);
        host
    }

    fn router(host: FakeHost) -> (Arc<Router>, Arc<FakeHost>, mpsc::Receiver<String>) {
        let host = Arc::new(host);
        let (router, frames) = testing::router(host.clone());
        (router, host, frames)
    }

    async fn next(frames: &mut mpsc::Receiver<String>) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .expect("a reply")
            .expect("open outbox");
        let frame: Value = serde_json::from_str(&frame).unwrap();
        frame["body"].clone()
    }

    /// The listed windows, and the id of the one titled `title`.
    async fn listed(
        router: &Arc<Router>,
        frames: &mut mpsc::Receiver<String>,
        title: &str,
    ) -> (Vec<Value>, String) {
        router.handle(
            "phone",
            json!({ "kind": "windows.list", "requestId": "list" }),
        );
        let body = next(frames).await;
        assert_eq!(body["ok"], true, "{body}");
        let windows = body["windows"].as_array().unwrap().clone();
        let id = windows
            .iter()
            .find(|w| w["title"] == title)
            .map(|w| w["id"].as_str().unwrap().to_string())
            .unwrap();
        (windows, id)
    }

    #[tokio::test]
    async fn the_list_puts_the_front_window_first() {
        let (router, _, mut frames) = router(host());
        let (windows, _) = listed(&router, &mut frames, "Inbox").await;
        let titles: Vec<_> = windows.iter().map(|w| w["title"].clone()).collect();
        assert_eq!(titles, ["Claude Code", "Inbox", "notes.txt"]);
        assert_eq!(windows[0]["front"], true);
        assert_eq!(windows[1]["front"], false);
        assert_eq!(windows[0]["app"], "WindowsTerminal");
        assert!(windows
            .iter()
            .all(|w| w["id"].as_str().unwrap().len() <= 64));
    }

    #[tokio::test]
    async fn a_window_keeps_its_id_across_lists() {
        let (router, host, mut frames) = router(host());
        let (_, first) = listed(&router, &mut frames, "Inbox").await;
        let (_, again) = listed(&router, &mut frames, "Inbox").await;
        assert_eq!(first, again);
        // A new window under a reused handle is a new id.
        host.windows.lock().unwrap()[0] = window(1, "Inbox", "Thunderbird");
        let (_, reused) = listed(&router, &mut frames, "Inbox").await;
        assert_ne!(first, reused);
    }

    #[tokio::test]
    async fn everything_waits_for_the_switch() {
        let host = host();
        host.windows_on
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let (router, host, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "windows.list", "requestId": "r1" }),
        );
        assert_eq!(next(&mut frames).await["error"], "remote_windows_off");
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r2", "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["error"], "remote_windows_off");
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r3", "text": "hi", "target": "w1" }),
        );
        assert_eq!(next(&mut frames).await["error"], "remote_windows_off");
        assert!(host.pressed.lock().unwrap().is_empty());
        assert!(host.pasted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn keys_go_to_the_target_and_the_front_window_comes_back() {
        let (router, host, mut frames) = router(host());
        let (_, inbox) = listed(&router, &mut frames, "Inbox").await;
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r1", "target": inbox,
                    "keys": ["shift+enter", "Enter"] }),
        );
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "r1", "ok": true, "pressed": 2 })
        );
        assert_eq!(*host.pressed.lock().unwrap(), ["Shift+Enter", "Enter"]);
        // Inbox forward, then the terminal back.
        assert_eq!(*host.focused.lock().unwrap(), [1, 2]);
        assert_eq!(*host.front.lock().unwrap(), Some(2));
    }

    #[tokio::test]
    async fn stay_leaves_the_target_in_front() {
        let (router, host, mut frames) = router(host());
        let (_, inbox) = listed(&router, &mut frames, "Inbox").await;
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r1", "text": "hi", "target": inbox, "stay": true }),
        );
        assert_eq!(next(&mut frames).await["stage"], "pasting");
        assert_eq!(next(&mut frames).await["ok"], true);
        assert_eq!(*host.focused.lock().unwrap(), [1]);
        assert_eq!(*host.pasted_into.lock().unwrap(), [Some(1)]);
    }

    #[tokio::test]
    async fn a_target_already_in_front_is_not_moved() {
        let (router, host, mut frames) = router(host());
        let (_, terminal) = listed(&router, &mut frames, "Claude Code").await;
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r1", "target": terminal, "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["ok"], true);
        assert!(host.focused.lock().unwrap().is_empty());
        assert_eq!(*host.pressed.lock().unwrap(), ["Enter"]);
    }

    #[tokio::test]
    async fn keys_without_a_target_go_to_the_front_window() {
        let (router, host, mut frames) = router(host());
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r1", "keys": ["tab"] }),
        );
        assert_eq!(next(&mut frames).await["ok"], true);
        assert!(host.focused.lock().unwrap().is_empty());
        assert_eq!(*host.pressed.lock().unwrap(), ["Tab"]);
    }

    #[tokio::test]
    async fn refused_or_unreadable_keys_press_nothing() {
        let (router, host, mut frames) = router(host());
        for (id, keys, code) in [
            ("r1", json!(["enter", "alt+f4"]), "key_refused"),
            ("r2", json!(["ctrl+alt+delete"]), "key_refused"),
            ("r3", json!(["hyper+x"]), "unsupported"),
            ("r4", json!([]), "unsupported"),
            ("r5", json!(vec!["enter"; 21]), "unsupported"),
            ("r6", json!("enter"), "unsupported"),
            ("r7", json!([5]), "unsupported"),
        ] {
            router.handle(
                "phone",
                json!({ "kind": "windows.keys", "requestId": id, "keys": keys }),
            );
            assert_eq!(next(&mut frames).await["error"], code, "{id}");
        }
        assert!(host.pressed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_expired_or_closed_target_is_gone() {
        let (router, host, mut frames) = router(host());
        // Never offered.
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r1", "target": "w999999", "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["error"], "target_gone");

        // Offered to another phone only.
        let (_, inbox) = listed(&router, &mut frames, "Inbox").await;
        router.handle(
            "phone2",
            json!({ "kind": "windows.keys", "requestId": "r2", "target": inbox, "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["stage"], "approval");
        router.answer("phone2", true);
        assert_eq!(next(&mut frames).await["error"], "target_gone");

        // Gone from the next list.
        host.windows.lock().unwrap().retain(|w| w.id != 1);
        let _ = listed(&router, &mut frames, "notes.txt").await;
        host.windows
            .lock()
            .unwrap()
            .push(window(1, "Inbox", "Outlook"));
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r3", "target": inbox, "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["error"], "target_gone");

        // Closed since the list.
        let (_, notes) = listed(&router, &mut frames, "notes.txt").await;
        host.windows.lock().unwrap().retain(|w| w.id != 3);
        router.handle(
            "phone",
            json!({ "kind": "text", "requestId": "r4", "text": "hi", "target": notes }),
        );
        assert_eq!(next(&mut frames).await["stage"], "pasting");
        assert_eq!(next(&mut frames).await["error"], "target_gone");

        // Expired.
        let (_, inbox) = listed(&router, &mut frames, "Inbox").await;
        router.offered().get_mut("phone").unwrap().issued -= ID_LIFETIME;
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r5", "target": inbox, "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["error"], "target_gone");

        assert!(host.pressed.lock().unwrap().is_empty());
        assert!(host.pasted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_window_that_will_not_come_forward_gets_nothing() {
        let host = host();
        *host.focus_fails.lock().unwrap() = true;
        let (router, host, mut frames) = router(host);
        let (_, inbox) = listed(&router, &mut frames, "Inbox").await;
        router.handle(
            "phone",
            json!({ "kind": "windows.keys", "requestId": "r1", "target": inbox, "keys": ["enter"] }),
        );
        assert_eq!(next(&mut frames).await["error"], "focus_failed");
        assert!(host.pressed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_dictation_is_pasted_into_its_target() {
        let (router, host, mut frames) = router(host());
        let (_, notes) = listed(&router, &mut frames, "notes.txt").await;
        let audio = super::super::dictate::tests::wav_base64(16_000);
        router.handle(
            "phone",
            json!({ "kind": "dictate", "requestId": "r1", "deliver": "paste",
                    "audio": audio, "target": notes }),
        );
        assert_eq!(next(&mut frames).await["stage"], "transcribing");
        assert_eq!(next(&mut frames).await["stage"], "pasting");
        assert_eq!(next(&mut frames).await["pasted"], true);
        assert_eq!(*host.pasted_into.lock().unwrap(), [Some(3)]);
        assert_eq!(*host.focused.lock().unwrap(), [3, 2]);
    }

    #[tokio::test]
    async fn a_bad_target_fails_a_dictation_before_it_is_transcribed() {
        let (router, host, mut frames) = router(host());
        let audio = super::super::dictate::tests::wav_base64(16_000);
        router.handle(
            "phone",
            json!({ "kind": "dictate", "requestId": "r1", "deliver": "paste",
                    "audio": audio, "target": "w0" }),
        );
        assert_eq!(next(&mut frames).await["error"], "target_gone");
        assert!(host.saved.lock().unwrap().is_empty());
    }
}
