//! "Sync API keys": every computer signed in to the same account shares the
//! user's provider keys (`/v1/secrets` on the account server).
//!
//! Values live only in the OS keychain here and, encrypted, on the server. The
//! small state file beside the settings ([`STATE_FILE`]) records, per key
//! name, when this computer last changed or received it and whether that
//! change was a removal; never a value. Conflicts go to the last writer by
//! that time (`updatedAt`, milliseconds), the server's rule too.
//!
//! The decision is [`plan`], a pure function over this computer's view of
//! each key ([`LocalKey`]) and the server's records; everything else here
//! only carries its [`Step`]s out. A sync runs, while signed in with the
//! setting on and a usable keychain:
//!
//! - right after sign-in and at launch with a saved session ([`sync_now`]);
//! - every [`SYNC_INTERVAL`];
//! - [`DEBOUNCE`] after a key is added, changed or cleared on this computer,
//!   which `secret_store` reports through its change listener ([`init`]).
//!
//! **The first sync** of a computer has keys with no time yet (added before
//! sync existed, or before this file did). Such a key loses to a live server
//! copy of the same name, which is pulled over it, and is uploaded when the
//! server has no live copy, a tombstone included: a computer joining the
//! account never has a key deleted by a removal it never saw.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use super::api::{valid_secret_name, SecretRecord, SecretWrite, MAX_SECRET_LEN};
use crate::settings::{get_settings, write_settings, AppSettings};

/// How often keys are synced with nothing changing here.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// How long after a local change the sync waits for more changes.
pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// The sync state, in the app data dir.
const STATE_FILE: &str = "account_secrets_sync.json";

/// What this computer knows about one key name: when it was last changed or
/// received here, and whether that left it removed. Never its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEntry {
    pub updated_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct SyncState {
    #[serde(default)]
    entries: BTreeMap<String, StateEntry>,
}

/// This computer's view of one key.
///
/// - a value with a time: a key changed or received at that time;
/// - a value without one: a key from before this computer synced;
/// - no value with a time: removed here at that time (a tombstone);
/// - neither: nothing known.
#[derive(Clone, PartialEq, Eq)]
pub struct LocalKey {
    pub value: Option<String>,
    pub updated_at: Option<i64>,
}

impl LocalKey {
    /// The view from the keychain's `value` (empty = none) and the state
    /// file's `entry`. An entry that disagrees with the keychain (a live entry
    /// with no key, a tombstone with one) gives no time: the keychain changed
    /// without this app, so when it did is not known.
    pub fn from_parts(value: Option<&str>, entry: Option<StateEntry>) -> Self {
        let value = value.filter(|v| !v.is_empty()).map(str::to_string);
        let updated_at = entry
            .filter(|e| e.deleted == value.is_none())
            .map(|e| e.updated_at);
        Self { value, updated_at }
    }
}

impl std::fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalKey")
            .field("has_value", &self.value.is_some())
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// One thing a sync does about one key.
#[derive(Clone, PartialEq, Eq)]
pub enum Step {
    /// Take the server's key into the keychain.
    Pull {
        name: String,
        value: String,
        updated_at: i64,
    },
    /// Remove the key here; the server's removal is newer.
    RemoveLocal { name: String, updated_at: i64 },
    /// Send this computer's key.
    Push {
        name: String,
        value: String,
        updated_at: i64,
    },
    /// Send this computer's removal.
    PushDelete { name: String, updated_at: i64 },
    /// Nothing moves; only the time (and removal) is remembered.
    Record {
        name: String,
        updated_at: i64,
        deleted: bool,
    },
}

impl Step {
    pub fn name(&self) -> &str {
        match self {
            Step::Pull { name, .. }
            | Step::RemoveLocal { name, .. }
            | Step::Push { name, .. }
            | Step::PushDelete { name, .. }
            | Step::Record { name, .. } => name,
        }
    }
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, updated_at) = match self {
            Step::Pull { updated_at, .. } => ("Pull", updated_at),
            Step::RemoveLocal { updated_at, .. } => ("RemoveLocal", updated_at),
            Step::Push { updated_at, .. } => ("Push", updated_at),
            Step::PushDelete { updated_at, .. } => ("PushDelete", updated_at),
            Step::Record {
                updated_at,
                deleted: true,
                ..
            } => ("RecordDeleted", updated_at),
            Step::Record { updated_at, .. } => ("Record", updated_at),
        };
        write!(f, "{kind}({} @ {updated_at})", self.name())
    }
}

/// What to do about the key `name`, given this computer's view and the
/// server's record of it. `now` is the time a never-timed key is sent with.
pub fn decide(
    name: &str,
    local: &LocalKey,
    server: Option<&SecretRecord>,
    now: i64,
) -> Option<Step> {
    let name = name.to_string();
    let pull = |r: &SecretRecord| Step::Pull {
        name: name.clone(),
        value: r.value.clone(),
        updated_at: r.updated_at,
    };
    let push = |value: &str, updated_at: i64| Step::Push {
        name: name.clone(),
        value: value.to_string(),
        updated_at,
    };
    let record = |updated_at: i64, deleted: bool| Step::Record {
        name: name.clone(),
        updated_at,
        deleted,
    };
    // A record with no value is a removal whatever its flag says.
    let server = server.map(|r| (r, r.deleted || r.value.is_empty()));
    match (&local.value, local.updated_at, server) {
        // The server has never heard of it.
        (Some(value), at, None) => Some(push(value, at.unwrap_or(now))),
        (None, _, None) => None,

        // The server has a live key.
        (Some(value), Some(at), Some((r, false))) => {
            if at > r.updated_at {
                Some(push(value, at))
            } else if *value != r.value {
                // Newer on the server, or a tie: the server's copy wins, so
                // every computer ends on the same key.
                Some(pull(r))
            } else if r.updated_at > at {
                Some(record(r.updated_at, false))
            } else {
                None
            }
        }
        (Some(value), None, Some((r, false))) => Some(if *value == r.value {
            record(r.updated_at, false)
        } else {
            pull(r)
        }),
        (None, Some(at), Some((r, false))) if at > r.updated_at => Some(Step::PushDelete {
            name: name.clone(),
            updated_at: at,
        }),
        (None, _, Some((r, false))) => Some(pull(r)),

        // The server has a removal.
        (Some(value), Some(at), Some((r, true))) => Some(if at > r.updated_at {
            push(value, at)
        } else {
            Step::RemoveLocal {
                name: name.clone(),
                updated_at: r.updated_at,
            }
        }),
        // Never delete a key this computer has not synced before.
        (Some(value), None, Some((_, true))) => Some(push(value, now)),
        (None, Some(at), Some((r, true))) => {
            (r.updated_at > at).then(|| record(r.updated_at, true))
        }
        (None, None, Some((r, true))) => Some(record(r.updated_at, true)),
    }
}

/// Every step of one sync. Nothing when syncing is off; names the server
/// would refuse, and keys too long for it, are left out.
pub fn plan(
    enabled: bool,
    local: &BTreeMap<String, LocalKey>,
    server: &[SecretRecord],
    now: i64,
) -> Vec<Step> {
    if !enabled {
        return Vec::new();
    }
    let server: BTreeMap<&str, &SecretRecord> = server
        .iter()
        .filter(|r| valid_secret_name(&r.name))
        .map(|r| (r.name.as_str(), r))
        .collect();
    let names: BTreeSet<&str> = local
        .keys()
        .map(String::as_str)
        .chain(server.keys().copied())
        .filter(|name| valid_secret_name(name))
        .collect();
    let nothing = LocalKey {
        value: None,
        updated_at: None,
    };
    names
        .into_iter()
        .filter_map(|name| {
            let mine = local.get(name).unwrap_or(&nothing);
            decide(name, mine, server.get(name).copied(), now)
        })
        .filter(|step| match step {
            Step::Push { value, .. } if value.len() > MAX_SECRET_LEN => {
                warn!("Not syncing the API key {}: it is too long", step.name());
                false
            }
            _ => true,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Keys in the settings
// ---------------------------------------------------------------------------

const KINDS: [&str; 4] = ["post_process", "cloud_stt", "assistant_tts", "web_search"];

fn key_map<'a>(
    settings: &'a mut AppSettings,
    kind: &str,
) -> Option<&'a mut crate::settings::SecretMap> {
    match kind {
        "post_process" => Some(&mut settings.post_process_api_keys),
        "cloud_stt" => Some(&mut settings.cloud_stt_api_keys),
        "assistant_tts" => Some(&mut settings.assistant_tts_api_keys),
        "web_search" => Some(&mut settings.web_search_api_keys),
        _ => None,
    }
}

/// Every key the (hydrated) settings hold, by record name.
fn local_values(settings: &mut AppSettings) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for kind in KINDS {
        if let Some(map) = key_map(settings, kind) {
            for (id, value) in map.iter() {
                values.insert(format!("{kind}:{id}"), value.clone());
            }
        }
    }
    values
}

/// Put `value` (empty = removed) into the settings slot of the key `name`.
fn set_slot(settings: &mut AppSettings, name: &str, value: &str) {
    let Some((kind, id)) = name.split_once(':') else {
        return;
    };
    if let Some(map) = key_map(settings, kind) {
        map.insert(id.to_string(), value.to_string());
    }
}

// ---------------------------------------------------------------------------
// The state file
// ---------------------------------------------------------------------------

fn state_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    crate::portable::resolve_app_data(app, STATE_FILE).ok()
}

/// The state, or an empty one when there is none or it cannot be read (every
/// key then counts as never synced, which loses nothing).
fn load_state(path: &std::path::Path) -> SyncState {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            warn!("The API key sync state is unreadable ({e}); starting over");
            SyncState::default()
        }),
        Err(_) => SyncState::default(),
    }
}

fn save_state(path: &std::path::Path, state: &SyncState) {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(state)?)?;
        std::fs::rename(&temp, path)
    })();
    if let Err(e) = result {
        warn!("Could not save the API key sync state: {e}");
    }
}

/// Change the state under its lock.
fn update_state<T>(app: &AppHandle, change: impl FnOnce(&mut SyncState) -> T) -> Option<T> {
    let path = state_path(app)?;
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load_state(&path);
    let before = state.clone();
    let result = change(&mut state);
    if state != before {
        save_state(&path, &state);
    }
    Some(result)
}

fn read_state(app: &AppHandle) -> SyncState {
    let Some(path) = state_path(app) else {
        return SyncState::default();
    };
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    load_state(&path)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------------
// When it runs
// ---------------------------------------------------------------------------

enum Trigger {
    /// Sign-in, launch, the setting turned on.
    Now,
    /// A key changed here; more may follow.
    Soon,
}

fn trigger() -> &'static OnceLock<mpsc::UnboundedSender<Trigger>> {
    static TRIGGER: OnceLock<mpsc::UnboundedSender<Trigger>> = OnceLock::new();
    &TRIGGER
}

/// Sync as soon as possible (sign-in, launch, the setting turned on).
pub fn sync_now() {
    if let Some(sender) = trigger().get() {
        let _ = sender.send(Trigger::Now);
    }
}

/// Start the background loop and listen for key changes made on this
/// computer. Once per process.
pub fn init(app: &AppHandle) {
    let (sender, mut received) = mpsc::unbounded_channel();
    if trigger().set(sender).is_err() {
        return;
    }
    let listener_app = app.clone();
    crate::secret_store::set_change_listener(move |account, present| {
        if !valid_secret_name(account) {
            return;
        }
        let entry = StateEntry {
            updated_at: now_ms(),
            deleted: !present,
        };
        update_state(&listener_app, |state| {
            state.entries.insert(account.to_string(), entry);
        });
        if let Some(sender) = trigger().get() {
            let _ = sender.send(Trigger::Soon);
        }
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match tokio::time::timeout(SYNC_INTERVAL, received.recv()).await {
                Ok(None) => return,
                Ok(Some(Trigger::Soon)) => tokio::time::sleep(DEBOUNCE).await,
                Ok(Some(Trigger::Now)) | Err(_) => {}
            }
            // Whatever else arrived meanwhile is covered by this run.
            while received.try_recv().is_ok() {}
            run(&app).await;
        }
    });
}

/// One sync, if there is anything to sync with.
async fn run(app: &AppHandle) {
    let manager = super::manager(app);
    let Some((session, generation)) = manager.session() else {
        return;
    };
    let mut settings = get_settings(app);
    if !settings.account_sync_secrets {
        return;
    }
    if !crate::secret_store::is_available() {
        debug!("API key sync skipped: no OS keychain");
        return;
    }
    let Ok(api) = super::server_api(app) else {
        return;
    };
    let server = match api.list_secrets(&session.token).await {
        Ok(server) => server,
        Err(code) => {
            info!("Could not sync API keys ({code}); will try again");
            manager.checked(code);
            return;
        }
    };

    let state = read_state(app);
    let values = local_values(&mut settings);
    let names: BTreeSet<&String> = values.keys().chain(state.entries.keys()).collect();
    let local: BTreeMap<String, LocalKey> = names
        .into_iter()
        .map(|name| {
            let value = values.get(name).map(String::as_str);
            let entry = state.entries.get(name).copied();
            (name.clone(), LocalKey::from_parts(value, entry))
        })
        .collect();
    let steps = plan(true, &local, &server, now_ms());
    if steps.is_empty() {
        debug!("API keys are in sync ({} on the account)", server.len());
        return;
    }

    let mut applied = Applied::default();
    let mut sent = 0usize;
    for step in steps {
        let planned = state.entries.get(step.name()).copied();
        // Signed out (or in again) meanwhile: this run's session is over.
        if manager.session().map(|(_, g)| g) != Some(generation) {
            debug!("API key sync stopped: the session changed");
            break;
        }
        let answer = match &step {
            Step::Push {
                name,
                value,
                updated_at,
            } => {
                api.put_secret(&session.token, name, value, *updated_at)
                    .await
            }
            Step::PushDelete { name, updated_at } => {
                api.delete_secret(&session.token, name, *updated_at).await
            }
            local_step => {
                applied.apply(app, local_step, planned);
                continue;
            }
        };
        match answer {
            Ok(SecretWrite::Saved(record)) => {
                sent += 1;
                applied.record(app, &record.name, planned, entry_of(&record));
            }
            Ok(SecretWrite::Stale(record)) => {
                debug!("The account has a newer {}; taking it", record.name);
                applied.apply(app, &server_step(&record), planned);
            }
            Err(code) if code == "network" || code == "unauthorized" => {
                info!("Could not sync API keys ({code}); will try again");
                manager.checked(code);
                break;
            }
            Err(code) => warn!("The account refused the API key {} ({code})", step.name()),
        }
    }
    if sent > 0 {
        info!("Sent {sent} API key change(s) to the account");
    }
    applied.finish(app);
}

/// What a server record says, as a state entry.
fn entry_of(record: &SecretRecord) -> StateEntry {
    StateEntry {
        updated_at: record.updated_at,
        deleted: record.deleted || record.value.is_empty(),
    }
}

/// The local step that takes a server record as it is.
fn server_step(record: &SecretRecord) -> Step {
    if record.deleted || record.value.is_empty() {
        Step::RemoveLocal {
            name: record.name.clone(),
            updated_at: record.updated_at,
        }
    } else {
        Step::Pull {
            name: record.name.clone(),
            value: record.value.clone(),
            updated_at: record.updated_at,
        }
    }
}

/// Local changes of one run, gathered so the settings are written once.
#[derive(Default)]
struct Applied {
    /// name -> new value (empty = removed).
    slots: BTreeMap<String, String>,
}

impl Applied {
    /// Remember `entry` for `name`, unless a change on this computer
    /// replaced the entry this run planned with since.
    fn record(
        &self,
        app: &AppHandle,
        name: &str,
        planned: Option<StateEntry>,
        entry: StateEntry,
    ) -> bool {
        update_state(app, |state| {
            if state.entries.get(name).copied() != planned {
                return false;
            }
            state.entries.insert(name.to_string(), entry);
            true
        })
        .unwrap_or(false)
    }

    fn apply(&mut self, app: &AppHandle, step: &Step, planned: Option<StateEntry>) {
        match step {
            Step::Pull {
                name,
                value,
                updated_at,
            } => {
                // Recorded first, so a change made here meanwhile wins.
                let entry = StateEntry {
                    updated_at: *updated_at,
                    deleted: false,
                };
                if self.record(app, name, planned, entry) {
                    if crate::secret_store::set_from_sync(name, value) {
                        self.slots.insert(name.clone(), value.clone());
                    } else {
                        // Try again next time.
                        update_state(app, |state| match planned {
                            Some(planned) => state.entries.insert(name.clone(), planned),
                            None => state.entries.remove(name),
                        });
                    }
                }
            }
            Step::RemoveLocal { name, updated_at } => {
                let entry = StateEntry {
                    updated_at: *updated_at,
                    deleted: true,
                };
                if self.record(app, name, planned, entry) {
                    if crate::secret_store::delete_from_sync(name) {
                        self.slots.insert(name.clone(), String::new());
                    } else {
                        update_state(app, |state| match planned {
                            Some(planned) => state.entries.insert(name.clone(), planned),
                            None => state.entries.remove(name),
                        });
                    }
                }
            }
            Step::Record {
                name,
                updated_at,
                deleted,
            } => {
                let entry = StateEntry {
                    updated_at: *updated_at,
                    deleted: *deleted,
                };
                self.record(app, name, planned, entry);
            }
            Step::Push { .. } | Step::PushDelete { .. } => {}
        }
    }

    /// Show what arrived: the settings take the new keys, and open windows
    /// read them again.
    fn finish(self, app: &AppHandle) {
        if self.slots.is_empty() {
            return;
        }
        let mut settings = get_settings(app);
        for (name, value) in &self.slots {
            set_slot(&mut settings, name, value);
        }
        // Mirror the active voice engine's key into the field the UI reads.
        settings.sync_active_tts_fields();
        write_settings(app, settings);
        info!(
            "Received {} API key change(s) from the account",
            self.slots.len()
        );
        let _ = app.emit(
            "settings-changed",
            serde_json::json!({ "setting": "api_keys" }),
        );
        let _ = app.emit("assistant-settings-changed", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "post_process:openai";
    const NOW: i64 = 10_000;

    fn local(value: Option<&str>, at: Option<i64>) -> LocalKey {
        LocalKey {
            value: value.map(str::to_string),
            updated_at: at,
        }
    }

    fn live(value: &str, at: i64) -> SecretRecord {
        SecretRecord {
            name: NAME.into(),
            value: value.into(),
            updated_at: at,
            deleted: false,
        }
    }

    fn gone(at: i64) -> SecretRecord {
        SecretRecord {
            name: NAME.into(),
            value: String::new(),
            updated_at: at,
            deleted: true,
        }
    }

    fn pull(value: &str, at: i64) -> Option<Step> {
        Some(Step::Pull {
            name: NAME.into(),
            value: value.into(),
            updated_at: at,
        })
    }

    fn push(value: &str, at: i64) -> Option<Step> {
        Some(Step::Push {
            name: NAME.into(),
            value: value.into(),
            updated_at: at,
        })
    }

    fn record(at: i64, deleted: bool) -> Option<Step> {
        Some(Step::Record {
            name: NAME.into(),
            updated_at: at,
            deleted,
        })
    }

    fn d(l: LocalKey, s: Option<SecretRecord>) -> Option<Step> {
        decide(NAME, &l, s.as_ref(), NOW)
    }

    #[test]
    fn a_key_only_here_is_uploaded() {
        assert_eq!(d(local(Some("a"), Some(5)), None), push("a", 5));
        // Never timed: sent as of now.
        assert_eq!(d(local(Some("a"), None), None), push("a", NOW));
    }

    #[test]
    fn nothing_anywhere_is_nothing() {
        assert_eq!(d(local(None, None), None), None);
        // A removal the server never had a key for needs no message.
        assert_eq!(d(local(None, Some(5)), None), None);
    }

    #[test]
    fn a_key_only_on_the_server_is_downloaded() {
        assert_eq!(d(local(None, None), Some(live("s", 5))), pull("s", 5));
    }

    #[test]
    fn equal_keys_only_take_the_newer_time() {
        assert_eq!(d(local(Some("a"), Some(5)), Some(live("a", 5))), None);
        assert_eq!(
            d(local(Some("a"), Some(5)), Some(live("a", 7))),
            record(7, false)
        );
        // Same key, newer here: re-sent so the server's time catches up.
        assert_eq!(
            d(local(Some("a"), Some(9)), Some(live("a", 7))),
            push("a", 9)
        );
        assert_eq!(
            d(local(Some("a"), None), Some(live("a", 7))),
            record(7, false)
        );
    }

    #[test]
    fn different_keys_go_to_the_newer() {
        assert_eq!(
            d(local(Some("a"), Some(9)), Some(live("s", 7))),
            push("a", 9)
        );
        assert_eq!(
            d(local(Some("a"), Some(5)), Some(live("s", 7))),
            pull("s", 7)
        );
        // A tie goes to the server, so every computer ends on one key.
        assert_eq!(
            d(local(Some("a"), Some(7)), Some(live("s", 7))),
            pull("s", 7)
        );
    }

    #[test]
    fn first_sync_a_never_timed_key_loses_to_a_server_copy() {
        assert_eq!(d(local(Some("a"), None), Some(live("s", 1))), pull("s", 1));
    }

    #[test]
    fn first_sync_a_never_timed_key_survives_a_server_removal() {
        assert_eq!(d(local(Some("a"), None), Some(gone(9_999))), push("a", NOW));
    }

    #[test]
    fn a_removal_here_reaches_the_server_when_newer() {
        assert_eq!(
            d(local(None, Some(9)), Some(live("s", 7))),
            Some(Step::PushDelete {
                name: NAME.into(),
                updated_at: 9
            })
        );
        // Older (or tied) removal: the server's key comes back.
        assert_eq!(d(local(None, Some(5)), Some(live("s", 7))), pull("s", 7));
        assert_eq!(d(local(None, Some(7)), Some(live("s", 7))), pull("s", 7));
    }

    #[test]
    fn a_removal_on_the_server_reaches_here_when_newer() {
        assert_eq!(
            d(local(Some("a"), Some(5)), Some(gone(7))),
            Some(Step::RemoveLocal {
                name: NAME.into(),
                updated_at: 7
            })
        );
        assert_eq!(d(local(Some("a"), Some(9)), Some(gone(7))), push("a", 9));
    }

    #[test]
    fn removals_on_both_sides_only_take_the_newer_time() {
        assert_eq!(d(local(None, Some(5)), Some(gone(7))), record(7, true));
        assert_eq!(d(local(None, Some(9)), Some(gone(7))), None);
        assert_eq!(d(local(None, None), Some(gone(7))), record(7, true));
    }

    #[test]
    fn a_live_record_without_a_value_is_a_removal() {
        let mut empty = live("", 7);
        empty.deleted = false;
        assert_eq!(d(local(None, None), Some(empty)), record(7, true));
    }

    #[test]
    fn the_local_view_trusts_the_keychain_over_the_state() {
        let entry = |at, deleted| {
            Some(StateEntry {
                updated_at: at,
                deleted,
            })
        };
        assert_eq!(
            LocalKey::from_parts(Some("a"), entry(5, false)),
            local(Some("a"), Some(5))
        );
        assert_eq!(
            LocalKey::from_parts(None, entry(5, true)),
            local(None, Some(5))
        );
        assert_eq!(
            LocalKey::from_parts(Some(""), entry(5, true)),
            local(None, Some(5))
        );
        assert_eq!(
            LocalKey::from_parts(Some("a"), None),
            local(Some("a"), None)
        );
        // The state and the keychain disagree: the time is not known.
        assert_eq!(
            LocalKey::from_parts(Some("a"), entry(5, true)),
            local(Some("a"), None)
        );
        assert_eq!(
            LocalKey::from_parts(None, entry(5, false)),
            local(None, None)
        );
    }

    #[test]
    fn syncing_off_plans_nothing() {
        let here = BTreeMap::from([(NAME.to_string(), local(Some("a"), Some(5)))]);
        assert!(plan(false, &here, &[live("s", 9)], NOW).is_empty());
        assert_eq!(
            plan(true, &here, &[live("s", 9)], NOW),
            vec![pull("s", 9).unwrap()]
        );
    }

    #[test]
    fn a_first_sync_plans_every_direction() {
        let here = BTreeMap::from([
            ("post_process:openai".to_string(), local(Some("mine"), None)),
            ("cloud_stt:deepgram".to_string(), local(Some("dg"), None)),
            ("web_search:brave".to_string(), local(Some("same"), None)),
            // An empty slot in the settings.
            ("assistant_tts:azure".to_string(), local(None, None)),
        ]);
        let rec = |name: &str, value: &str, at: i64, deleted: bool| SecretRecord {
            name: name.into(),
            value: value.into(),
            updated_at: at,
            deleted,
        };
        let server = [
            rec("post_process:openai", "theirs", 3, false),
            rec("web_search:brave", "same", 4, false),
            rec("assistant_tts:elevenlabs", "el", 6, false),
            rec("cloud_stt:deepgram", "", 8, true),
        ];
        let steps = plan(true, &here, &server, NOW);
        assert_eq!(
            steps,
            vec![
                Step::Pull {
                    name: "assistant_tts:elevenlabs".into(),
                    value: "el".into(),
                    updated_at: 6
                },
                Step::Push {
                    name: "cloud_stt:deepgram".into(),
                    value: "dg".into(),
                    updated_at: NOW
                },
                Step::Pull {
                    name: "post_process:openai".into(),
                    value: "theirs".into(),
                    updated_at: 3
                },
                Step::Record {
                    name: "web_search:brave".into(),
                    updated_at: 4,
                    deleted: false
                },
            ]
        );
    }

    #[test]
    fn names_and_keys_the_server_refuses_are_left_out() {
        let here = BTreeMap::from([
            (
                "post_process:My Provider".to_string(),
                local(Some("a"), None),
            ),
            ("account:session".to_string(), local(Some("t"), None)),
            (
                "post_process:big".to_string(),
                local(Some(&"x".repeat(MAX_SECRET_LEN + 1)), None),
            ),
        ]);
        let mut odd = live("s", 1);
        odd.name = "other:thing".into();
        assert!(plan(true, &here, &[odd], NOW).is_empty());
    }

    #[test]
    fn the_state_file_holds_no_values() {
        let mut state = SyncState::default();
        state.entries.insert(
            NAME.into(),
            StateEntry {
                updated_at: 5,
                deleted: false,
            },
        );
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            json,
            r#"{"entries":{"post_process:openai":{"updatedAt":5,"deleted":false}}}"#
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        save_state(&path, &state);
        assert_eq!(load_state(&path), state);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load_state(&path), SyncState::default());
    }

    #[test]
    fn debug_output_never_shows_a_key() {
        let step = push("sk-secret", 5).unwrap();
        assert!(!format!("{step:?}").contains("sk-secret"));
        assert!(!format!("{:?}", local(Some("sk-secret"), None)).contains("sk-secret"));
        assert!(!format!("{:?}", live("sk-secret", 1)).contains("sk-secret"));
    }
}
