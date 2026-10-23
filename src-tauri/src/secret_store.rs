//! Secure storage for provider API keys, backed by the OS keychain.
//!
//! Wraps the [`keyring`] crate so secrets live in the platform credential
//! store (Windows Credential Manager / macOS Keychain / Linux Secret Service)
//! instead of in plaintext inside `settings_store.json`.
//!
//! Three things keep this both correct and fast:
//!
//! 1. **In-memory cache.** `settings::get_settings()` runs on every action, so
//!    we must never block on the OS keychain in the hot path. Each account is
//!    read from the keychain at most once and then served from a process-wide
//!    cache (`None` is cached too, so an unset key stays a cache hit).
//! 2. **Off-thread access.** The Linux `async-secret-service` backend can
//!    deadlock if it is called on the app's main/runtime thread (see keyring
//!    issue #132). Every keyring call is therefore run on a short-lived worker
//!    thread. Joining that thread also turns any backend panic into a normal
//!    error, so a misbehaving keychain can never crash the app.
//! 3. **Graceful fallback.** If the platform store is unavailable (for example
//!    a headless Linux box with no Secret Service), every call degrades to a
//!    no-op and the caller keeps the secret in the settings file exactly as the
//!    app did before this module existed.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use log::{debug, warn};

/// Keyring *service* name. Matches the app's bundle identifier so all secrets
/// are grouped under the app in the OS credential viewer.
const SERVICE: &str = "com.talvio.desktop";

/// The service secrets were stored under while the app was called Zynveliq.
/// A secret missing under [`SERVICE`] is looked for here, moved to
/// [`SERVICE`] and deleted here (see [`read_with_carry_over`]), so an update
/// from Zynveliq keeps every API key without the user typing it again.
const LEGACY_SERVICE: &str = "com.zynveliq.desktop";

/// Account used only to detect whether the keychain backend works at all. It is
/// never written, so the probe is non-destructive.
const PROBE_ACCOUNT: &str = "__talvio_keychain_probe__";

/// Account name for the single assistant TTS API key.
pub const ACCOUNT_ASSISTANT_TTS: &str = "assistant_tts";

/// Account name for a per-engine assistant TTS key, keyed by engine id
/// (e.g. `assistant_tts:openai`). Each remote TTS engine (openai / elevenlabs /
/// azure) keeps its own key instead of sharing one slot.
pub fn account_assistant_tts(engine: &str) -> String {
    format!("assistant_tts:{engine}")
}

/// Account name for a post-processing/assistant provider key, keyed by provider
/// id (e.g. `post_process:openai`).
pub fn account_post_process(provider_id: &str) -> String {
    format!("post_process:{provider_id}")
}

/// Account name for a web-search provider key, keyed by provider id
/// (e.g. `web_search:brave`).
pub fn account_web_search(provider_id: &str) -> String {
    format!("web_search:{provider_id}")
}

/// Account name for a cloud speech-to-text provider key, keyed by provider id
/// (e.g. `cloud_stt:elevenlabs`). Namespaced separately from the LLM and
/// web-search accounts on purpose: the same vendor can appear in more than one
/// list (ElevenLabs does STT and TTS; Groq does LLM and STT) and the keys are
/// not necessarily the same one.
pub fn account_cloud_stt(provider_id: &str) -> String {
    format!("cloud_stt:{provider_id}")
}

/// account -> cached value. `Some(None)` means "known to be absent" so repeated
/// lookups of an unset key stay cache hits instead of re-hitting the keychain.
fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run a keyring operation on a dedicated OS thread and return its result.
///
/// Returns `None` if the thread could not be spawned or panicked — callers
/// treat that as "the keychain failed" and fall back accordingly.
fn run_on_thread<T, F>(label: &'static str, f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match std::thread::Builder::new()
        .name(format!("keychain-{label}"))
        .spawn(f)
    {
        Ok(handle) => match handle.join() {
            Ok(value) => Some(value),
            Err(_) => {
                warn!("secret_store: keychain '{label}' thread panicked");
                None
            }
        },
        Err(e) => {
            warn!("secret_store: could not spawn keychain thread: {e}");
            None
        }
    }
}

/// Read one credential of one service. Runs on the caller's thread.
fn read_entry(service: &str, account: &str) -> Result<Option<String>, ()> {
    let entry = keyring::Entry::new(service, account).map_err(|e| {
        debug!("secret_store: entry('{service}', '{account}') error: {e}");
    })?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            debug!("secret_store: get('{service}', '{account}') failed: {e}");
            Err(())
        }
    }
}

/// Read `account` under the current service and, when it is not there, under
/// the service of the app's previous name.
///
/// A secret found only under the old service is written under the new one and
/// then deleted from the old one, so the move happens once per secret. The old
/// copy is deleted only after the write succeeded: a failed write still
/// returns the value (this session works) and leaves the old copy for the next
/// launch to try again. A failed read of the new service is reported as a
/// failure without looking further, because "absent" there is not known.
///
/// The keychain steps are parameters so the decision is testable without one.
fn read_with_carry_over(
    read_new: impl FnOnce() -> Result<Option<String>, ()>,
    read_old: impl FnOnce() -> Result<Option<String>, ()>,
    write_new: impl FnOnce(&str) -> bool,
    delete_old: impl FnOnce() -> bool,
) -> Result<Option<String>, ()> {
    match read_new()? {
        Some(value) => Ok(Some(value)),
        None => match read_old() {
            Ok(Some(value)) => {
                if write_new(&value) {
                    if !delete_old() {
                        warn!(
                            "secret_store: moved a secret to the new keychain service \
                             but could not delete the old copy"
                        );
                    }
                } else {
                    warn!(
                        "secret_store: could not move a secret from the old keychain \
                         service; it will be tried again next launch"
                    );
                }
                Ok(Some(value))
            }
            // Nothing under the old name either, or the old service cannot be
            // read: the new service's answer stands.
            Ok(None) | Err(()) => Ok(None),
        },
    }
}

/// Low-level read straight from the keychain (no cache).
///
/// * `Ok(Some(v))` — a credential exists.
/// * `Ok(None)` — the backend works but there is no such credential.
/// * `Err(())` — the backend itself failed (treated as "unavailable").
fn keyring_get(account: &str) -> Result<Option<String>, ()> {
    let account = account.to_string();
    run_on_thread("get", move || {
        read_with_carry_over(
            || read_entry(SERVICE, &account),
            || read_entry(LEGACY_SERVICE, &account),
            |value| {
                let moved = keyring::Entry::new(SERVICE, &account)
                    .and_then(|entry| entry.set_password(value))
                    .is_ok();
                if moved {
                    debug!("secret_store: moved '{account}' to {SERVICE}");
                }
                moved
            },
            || {
                matches!(
                    keyring::Entry::new(LEGACY_SERVICE, &account)
                        .and_then(|entry| entry.delete_credential()),
                    Ok(()) | Err(keyring::Error::NoEntry)
                )
            },
        )
    })
    .unwrap_or(Err(()))
}

/// Whether the OS keychain backend is usable. Probed once per process; the
/// result is cached for the lifetime of the app.
pub fn is_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    let probe = || run_on_thread("probe", || read_entry(SERVICE, PROBE_ACCOUNT)).unwrap_or(Err(()));
    *AVAILABLE.get_or_init(|| match probe() {
        Ok(_) => {
            debug!("secret_store: OS keychain is available");
            true
        }
        Err(()) => {
            warn!(
                "secret_store: OS keychain unavailable; API keys will be kept \
                 in the settings file as a fallback"
            );
            false
        }
    })
}

/// Read a secret, preferring the in-memory cache. Returns `None` if the account
/// is unset or the keychain is unavailable.
pub fn get(account: &str) -> Option<String> {
    if let Some(cached) = cache().lock().unwrap().get(account).cloned() {
        return cached;
    }
    if !is_available() {
        return None;
    }
    // Only cache a definitive answer (`Ok`). A transient backend failure returns
    // `None` for this call but is left uncached so a later read can retry,
    // rather than masking a real key for the rest of the session.
    match keyring_get(account) {
        Ok(value) => {
            cache()
                .lock()
                .unwrap()
                .insert(account.to_string(), value.clone());
            value
        }
        Err(()) => None,
    }
}

/// Write a secret to the keychain and update the cache. Returns `false` if the
/// keychain is unavailable or the write failed.
pub fn set(account: &str, value: &str) -> bool {
    if !is_available() {
        return false;
    }
    let account_owned = account.to_string();
    let value_owned = value.to_string();
    let ok = run_on_thread("set", move || {
        let entry = match keyring::Entry::new(SERVICE, &account_owned) {
            Ok(entry) => entry,
            Err(e) => {
                warn!("secret_store: set('{account_owned}') entry error: {e}");
                return false;
            }
        };
        match entry.set_password(&value_owned) {
            Ok(()) => true,
            Err(e) => {
                warn!("secret_store: set('{account_owned}') failed: {e}");
                false
            }
        }
    })
    .unwrap_or(false);
    if ok {
        cache()
            .lock()
            .unwrap()
            .insert(account.to_string(), Some(value.to_string()));
    }
    ok
}

/// Delete a secret from the keychain and update the cache. A missing credential
/// counts as success. Returns `false` only if the keychain is unavailable or
/// the delete failed for another reason.
pub fn delete(account: &str) -> bool {
    if !is_available() {
        return false;
    }
    let account_owned = account.to_string();
    let ok = run_on_thread("delete", move || {
        let entry = match keyring::Entry::new(SERVICE, &account_owned) {
            Ok(entry) => entry,
            Err(e) => {
                warn!("secret_store: delete('{account_owned}') entry error: {e}");
                return false;
            }
        };
        match entry.delete_credential() {
            Ok(()) => true,
            Err(keyring::Error::NoEntry) => true,
            Err(e) => {
                warn!("secret_store: delete('{account_owned}') failed: {e}");
                false
            }
        }
    })
    .unwrap_or(false);
    if ok {
        cache().lock().unwrap().insert(account.to_string(), None);
    }
    ok
}

/// Reconcile the keychain with the desired `value` for `account` (an empty
/// `value` means the credential should not exist), skipping the keychain
/// round-trip when the cache already shows the desired state.
///
/// Returns `true` when the keychain is now in the desired state — i.e. it is
/// safe for the caller to drop its plaintext copy. Returns `false` only when the
/// keychain is unavailable or the write/delete failed, in which case the caller
/// MUST keep the value on disk so a key is never lost.
pub fn sync(account: &str, value: &str) -> bool {
    if !is_available() {
        return false;
    }
    {
        let cache = cache().lock().unwrap();
        match cache.get(account) {
            // Keychain already holds exactly this value.
            Some(Some(current)) if current == value => return true,
            // Keychain already known to be absent and we want it gone.
            Some(None) if value.is_empty() => return true,
            _ => {}
        }
    }
    // `set`/`delete` return true when the keychain reaches the desired state
    // (delete treats a missing credential as success).
    if value.is_empty() {
        // An empty value with no cached answer is ambiguous: every hydrated
        // account has a cache entry, so a miss here means the read failed
        // (`get` never caches an `Err`). A D-Bus timeout or a dismissed unlock
        // prompt hydrates the slot as "" and the very next settings write would
        // then delete a key that was never cleared. Re-read before deleting.
        let known = cache().lock().unwrap().contains_key(account);
        if !known {
            return match keyring_get(account) {
                Ok(None) => {
                    cache().lock().unwrap().insert(account.to_string(), None);
                    true
                }
                Ok(Some(existing)) => {
                    warn!(
                        "secret_store: '{account}' was not hydrated but exists in the \
                         keychain; keeping it instead of deleting it"
                    );
                    cache()
                        .lock()
                        .unwrap()
                        .insert(account.to_string(), Some(existing));
                    // Not the desired state; the caller keeps its (empty) copy.
                    false
                }
                Err(()) => false,
            };
        }
        delete(account)
    } else {
        set(account, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_are_namespaced_by_kind() {
        assert_eq!(account_post_process("openai"), "post_process:openai");
        assert_eq!(account_web_search("brave"), "web_search:brave");
        assert_eq!(ACCOUNT_ASSISTANT_TTS, "assistant_tts");
        // Different kinds with the same provider id must not collide.
        assert_ne!(account_post_process("groq"), account_web_search("groq"));
    }

    #[test]
    fn services_follow_the_rename() {
        assert_eq!(SERVICE, "com.talvio.desktop");
        assert_eq!(LEGACY_SERVICE, "com.zynveliq.desktop");
    }

    use std::cell::RefCell;

    /// What [`read_with_carry_over`] returned, what it wrote under the new
    /// service, and whether it deleted the old copy.
    type Carried = (Result<Option<String>, ()>, Option<String>, bool);

    fn carry(
        new: Result<Option<&str>, ()>,
        old: Result<Option<&str>, ()>,
        write_ok: bool,
    ) -> Carried {
        let written = RefCell::new(None);
        let deleted = RefCell::new(false);
        let result = read_with_carry_over(
            || new.map(|v| v.map(str::to_string)),
            || old.map(|v| v.map(str::to_string)),
            |value| {
                *written.borrow_mut() = Some(value.to_string());
                write_ok
            },
            || {
                *deleted.borrow_mut() = true;
                true
            },
        );
        (result, written.into_inner(), deleted.into_inner())
    }

    #[test]
    fn a_secret_under_the_new_service_is_used_as_is() {
        let (result, written, deleted) = carry(Ok(Some("new")), Ok(Some("old")), true);
        assert_eq!(result, Ok(Some("new".to_string())));
        assert_eq!(written, None);
        assert!(!deleted, "the old copy is only touched when it is moved");
    }

    #[test]
    fn a_secret_only_under_the_old_service_moves_once() {
        let (result, written, deleted) = carry(Ok(None), Ok(Some("sk-1")), true);
        assert_eq!(result, Ok(Some("sk-1".to_string())));
        assert_eq!(written.as_deref(), Some("sk-1"));
        assert!(deleted);
    }

    #[test]
    fn a_failed_move_keeps_the_old_copy_and_still_answers() {
        let (result, written, deleted) = carry(Ok(None), Ok(Some("sk-1")), false);
        assert_eq!(result, Ok(Some("sk-1".to_string())));
        assert_eq!(written.as_deref(), Some("sk-1"));
        assert!(!deleted, "never delete the only copy");
    }

    #[test]
    fn absent_everywhere_is_absent() {
        assert_eq!(carry(Ok(None), Ok(None), true), (Ok(None), None, false));
        assert_eq!(carry(Ok(None), Err(()), true), (Ok(None), None, false));
    }

    #[test]
    fn a_failed_read_of_the_new_service_is_a_failure() {
        assert_eq!(
            carry(Err(()), Ok(Some("old")), true),
            (Err(()), None, false)
        );
    }
}
