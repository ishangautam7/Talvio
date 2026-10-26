//! **Talvio free**: every signed-in Talvio account gets a small AI allowance
//! (one US dollar) served by the account server, shared by every cloud job:
//! cloud transcription, the assistant, AI cleanup, command mode and spoken
//! answers. It exists so someone can try every cloud feature before they have
//! an API key of their own.
//!
//! It is one more provider (`talvio_free`) in each job's registry, speaking
//! the OpenAI-compatible shapes the app already has (`llm_client`, the
//! `OpenAiCompatible` cloud STT kind, the OpenAI-compatible voice engine). What
//! is different is where its address and credential come from:
//!
//! - The **address** is the account server discovery found
//!   (`account::server_url`) plus [`API_PATH`]; the registry entries carry only
//!   [`PLACEHOLDER_BASE_URL`], an `.invalid` host no request can reach. It is
//!   swapped in at request time.
//! - The **credential** is the account session token, held here in memory only
//!   while a session runs ([`set_session`], called by the account manager). It
//!   is never written into the settings (its key slots stay empty), never sent
//!   to the webview, and never logged: [`FreeAccess`] has a redacting `Debug`.
//!
//! The server meters every call and answers `402 credit_exhausted` once the
//! allowance is spent. That is mapped to one friendly sentence
//! ([`EXHAUSTED_MESSAGE`]) and marks the credit exhausted, so the app stops
//! choosing Talvio free on its own; it never retries or falls back to another
//! route that would also cost the owner. Metered answers carry
//! `X-Talvio-Credit-Remaining`, which keeps the balance the UI shows current
//! without asking `/balance` after every call.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use log::{debug, info, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::settings::AppSettings;

/// The provider id in every job's registry.
pub const PROVIDER_ID: &str = "talvio_free";
/// What the provider is called wherever a provider name is shown.
pub const LABEL: &str = "Talvio free";
/// Model ids the server serves (`GET /v1/free/models`).
pub const CHAT_MODEL: &str = "talvio-free-chat";
pub const TRANSCRIBE_MODEL: &str = "talvio-free-transcribe";
pub const SPEECH_MODEL: &str = "talvio-free-speech";
/// A voice every OpenAI-compatible speech route knows.
pub const DEFAULT_VOICE: &str = "alloy";
/// Where the OpenAI-compatible API lives on the account server.
pub const API_PATH: &str = "/v1/free";
/// What the registries store as the base URL. `.invalid` never resolves
/// (RFC 2606), so a request that somehow skipped the swap goes nowhere.
pub const PLACEHOLDER_BASE_URL: &str = "https://talvio-free.invalid/v1/free";
/// The header a metered answer carries.
pub const CREDIT_HEADER: &str = "x-talvio-credit-remaining";
/// Sent to every window with the whole [`FreeCreditStatus`] when it changes.
pub const EVENT: &str = "free-credit-changed";

/// What the user is told when the allowance is spent. The frontend has its
/// own translated copy; this one reaches error toasts and the assistant.
pub const EXHAUSTED_MESSAGE: &str = "Your Talvio free credit is used up. Add your own API key on the Models page to keep using this feature.";
/// What the user is told when the server has switched the free credit off.
pub const DISABLED_MESSAGE: &str =
    "Talvio free is not available right now. Add your own API key on the Models page to keep using this feature.";
/// What the user is told when the free credit is chosen but nobody is signed in.
pub const SIGN_IN_MESSAGE: &str =
    "Talvio free needs a Talvio account. Sign in under Settings → Account, or choose another provider.";

/// The free allowance as the server reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct FreeCreditBalance {
    pub limit_usd: f64,
    pub used_usd: f64,
    pub remaining_usd: f64,
    pub enabled: bool,
}

/// Which jobs run on Talvio free right now, chosen or as the fallback.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Type)]
pub struct FreeCreditJobs {
    pub assistant: bool,
    pub cleanup: bool,
    pub command: bool,
    pub stt: bool,
    pub voice: bool,
}

/// Everything the UI shows about the free credit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Type)]
pub struct FreeCreditStatus {
    pub signed_in: bool,
    /// `None` until the server has answered `/v1/free/balance`.
    pub balance: Option<FreeCreditBalance>,
    /// The server answered `402 credit_exhausted` (or reported nothing left).
    pub exhausted: bool,
    pub serving: FreeCreditJobs,
}

/// A job the UI can point at Talvio free.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FreeCreditJob {
    Assistant,
    Cleanup,
    Command,
    Stt,
    Voice,
}

/// The address and credential of the free API, for one request.
#[derive(Clone)]
pub struct FreeAccess {
    pub base_url: String,
    pub token: String,
    /// The app may choose Talvio free on its own: the server says it is on and
    /// there is credit left.
    pub usable: bool,
}

impl std::fmt::Debug for FreeAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FreeAccess")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .field("usable", &self.usable)
            .finish()
    }
}

#[derive(Default)]
struct State {
    base_url: Option<String>,
    token: Option<String>,
    balance: Option<FreeCreditBalance>,
    exhausted: bool,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));
static APP: OnceLock<AppHandle> = OnceLock::new();

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keep the app handle for the change events. Called once at startup.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The free API's root on an account server.
pub fn free_base_url(server: &str) -> String {
    format!("{}{API_PATH}", server.trim().trim_end_matches('/'))
}

/// The account session changed: `server` is the account server in use (if
/// this build may reach it) and `token` the session's. `None` for either ends
/// free access. A new session starts with an unknown balance and asks for it.
pub(crate) fn set_session(server: Option<String>, token: Option<String>) {
    let base_url = server.map(|server| free_base_url(&server));
    let changed = {
        let mut state = state();
        let same = state.base_url == base_url && state.token == token;
        if !same {
            state.base_url = base_url;
            state.token = token;
            state.balance = None;
            state.exhausted = false;
        }
        !same
    };
    if changed {
        notify();
        refresh_in_background();
    }
}

/// The free API's address and credential while someone is signed in.
pub fn access() -> Option<FreeAccess> {
    let state = state();
    let base_url = state.base_url.clone()?;
    let token = state.token.clone().filter(|t| !t.trim().is_empty())?;
    Some(FreeAccess {
        base_url,
        token,
        usable: usable_from(state.balance.as_ref(), state.exhausted),
    })
}

/// Whether a session is running that the free API would accept.
pub fn signed_in() -> bool {
    access().is_some()
}

/// Whether the app may route a job to Talvio free on its own.
pub fn usable() -> bool {
    access().is_some_and(|access| access.usable)
}

/// Only a balance the server reported, switched on and with something left,
/// lets the app pick Talvio free by itself. Before the balance is known it
/// does not: a request that would only come back `free_disabled` is worse
/// than the setup notice the user would otherwise see.
pub fn usable_from(balance: Option<&FreeCreditBalance>, exhausted: bool) -> bool {
    !exhausted && balance.is_some_and(|b| b.enabled && b.remaining_usd > 0.0)
}

/// Whether `url` is a request to the free API.
pub fn is_free_url(url: &str) -> bool {
    let state = state();
    state
        .base_url
        .as_deref()
        .is_some_and(|base| url.starts_with(base))
}

/// The base URL and key a request to `provider_id` really uses: the free
/// API's own for Talvio free (or the unreachable placeholder and no key while
/// nobody is signed in), the given ones for everything else.
pub fn route(
    provider_id: &str,
    base_url: &str,
    api_key: &str,
    access: Option<&FreeAccess>,
) -> (String, String) {
    if provider_id != PROVIDER_ID {
        return (base_url.to_string(), api_key.to_string());
    }
    match access {
        Some(access) => (access.base_url.clone(), access.token.clone()),
        None => (PLACEHOLDER_BASE_URL.to_string(), String::new()),
    }
}

/// The free API's answers that need more than the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreeFailure {
    Exhausted,
    Disabled,
    Unauthorized,
    RateLimited,
}

impl FreeFailure {
    /// The sentence the user sees.
    pub fn message(self) -> &'static str {
        match self {
            Self::Exhausted => EXHAUSTED_MESSAGE,
            Self::Disabled => DISABLED_MESSAGE,
            Self::Unauthorized => SIGN_IN_MESSAGE,
            Self::RateLimited => {
                "Talvio free is busy right now. Wait a moment and try again, or add your own API key on the Models page."
            }
        }
    }
}

/// Read a failed answer from the free API. 402 is always the allowance
/// running out, whatever its body says; 503 only with `free_disabled`.
pub fn classify_failure(status: u16, body: &str) -> Option<FreeFailure> {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/code")
                .and_then(|c| c.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    match status {
        402 => Some(FreeFailure::Exhausted),
        _ if code == "credit_exhausted" => Some(FreeFailure::Exhausted),
        503 if code == "free_disabled" => Some(FreeFailure::Disabled),
        _ if code == "free_disabled" => Some(FreeFailure::Disabled),
        401 => Some(FreeFailure::Unauthorized),
        429 => Some(FreeFailure::RateLimited),
        _ => None,
    }
}

/// The remaining credit a metered answer reports, in US dollars.
pub fn remaining_from_header(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v.max(0.0))
}

/// The balance after a metered answer said `remaining` is left.
pub fn balance_after(
    previous: Option<&FreeCreditBalance>,
    remaining: f64,
) -> Option<FreeCreditBalance> {
    let previous = previous?;
    let limit = previous.limit_usd.max(remaining);
    Some(FreeCreditBalance {
        limit_usd: limit,
        used_usd: (limit - remaining).max(0.0),
        remaining_usd: remaining,
        enabled: previous.enabled,
    })
}

/// A successful answer from `url`: keep the balance its header reports.
pub fn note_success(url: &str, headers: &reqwest::header::HeaderMap) {
    if !is_free_url(url) {
        return;
    }
    let Some(remaining) = headers
        .get(CREDIT_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(remaining_from_header)
    else {
        return;
    };
    let changed = {
        let mut state = state();
        let next = balance_after(state.balance.as_ref(), remaining);
        let exhausted = remaining <= 0.0;
        let changed = next.is_some() && next != state.balance || exhausted != state.exhausted;
        if next.is_some() {
            state.balance = next;
        }
        state.exhausted = exhausted;
        changed
    };
    if changed {
        notify();
    }
    if !has_balance() {
        refresh_in_background();
    }
}

fn has_balance() -> bool {
    state().balance.is_some()
}

/// A failed answer from `url`: the message to show instead of the raw one,
/// when it is the free API's own. Running out marks the credit exhausted.
pub fn note_failure(url: &str, status: u16, body: &str) -> Option<String> {
    if !is_free_url(url) {
        return None;
    }
    let failure = classify_failure(status, body)?;
    match failure {
        FreeFailure::Exhausted => {
            info!("Talvio free: the credit is used up");
            mark(|state| {
                state.exhausted = true;
                if let Some(balance) = state.balance.as_mut() {
                    balance.used_usd = balance.limit_usd;
                    balance.remaining_usd = 0.0;
                }
            });
        }
        FreeFailure::Disabled => {
            info!("Talvio free: switched off on the server");
            mark(|state| {
                if let Some(balance) = state.balance.as_mut() {
                    balance.enabled = false;
                }
            });
        }
        FreeFailure::Unauthorized | FreeFailure::RateLimited => {}
    }
    Some(failure.message().to_string())
}

fn mark(change: impl FnOnce(&mut State)) {
    change(&mut state());
    notify();
}

static HTTP: Lazy<Option<reqwest::Client>> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .ok()
});

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BalanceAnswer {
    #[serde(default)]
    limit_usd: f64,
    #[serde(default)]
    used_usd: f64,
    #[serde(default)]
    remaining_usd: f64,
    #[serde(default)]
    enabled: bool,
}

/// Parse `GET /v1/free/balance`.
pub fn parse_balance(body: &str) -> Option<FreeCreditBalance> {
    let answer: BalanceAnswer = serde_json::from_str(body).ok()?;
    let finite = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
    Some(FreeCreditBalance {
        limit_usd: finite(answer.limit_usd),
        used_usd: finite(answer.used_usd),
        remaining_usd: finite(answer.remaining_usd),
        enabled: answer.enabled,
    })
}

/// Ask the server for the balance now. Without a session it only says so.
pub async fn refresh() {
    let Some(current) = access() else {
        notify();
        return;
    };
    let Some(client) = HTTP.as_ref() else { return };
    let url = format!("{}/balance", current.base_url);
    let answer = client.get(&url).bearer_auth(&current.token).send().await;
    let response = match answer {
        Ok(response) => response,
        Err(e) => {
            debug!("Talvio free: could not ask for the balance: {e}");
            return;
        }
    };
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    // A session that ended or moved while this waited says nothing now.
    if access().map(|now| now.token) != Some(current.token.clone()) {
        return;
    }
    if !(200..300).contains(&status) {
        if note_failure(&url, status, &body).is_none() {
            debug!("Talvio free: the balance request answered {status}");
        }
        return;
    }
    let Some(balance) = parse_balance(&body) else {
        warn!("Talvio free: the balance answer was unreadable");
        return;
    };
    mark(|state| {
        state.exhausted = balance.enabled && balance.remaining_usd <= 0.0;
        state.balance = Some(balance);
    });
}

fn refresh_in_background() {
    if APP.get().is_none() {
        return;
    }
    tauri::async_runtime::spawn(refresh());
}

/// The whole status for the UI.
pub fn status(settings: &AppSettings) -> FreeCreditStatus {
    let (signed_in, balance, exhausted) = {
        let state = state();
        (
            state.base_url.is_some() && state.token.as_ref().is_some_and(|t| !t.is_empty()),
            state.balance.clone(),
            state.exhausted,
        )
    };
    FreeCreditStatus {
        signed_in,
        balance,
        exhausted,
        serving: serving(settings, signed_in, usable()),
    }
}

/// Which jobs run on Talvio free with these settings. `signed_in` decides a
/// job the user pointed at it; `usable` decides the fallbacks.
pub fn serving(settings: &AppSettings, signed_in: bool, usable: bool) -> FreeCreditJobs {
    if !signed_in {
        return FreeCreditJobs::default();
    }
    let assistant = settings
        .assistant_provider_with(usable)
        .is_some_and(|p| p.id == PROVIDER_ID);
    let cleanup = crate::settings::resolve_post_process_brain_with(settings, usable)
        .is_ok_and(|(provider, ..)| provider.id == PROVIDER_ID);
    let command = crate::command_mode::llm::effective_with(
        settings,
        settings.command_router_model.trim(),
        signed_in,
        usable,
    )
    .0 == PROVIDER_ID;
    let stt = settings.stt_engine_mode == crate::settings::SttEngineMode::Cloud
        && crate::stt_cloud::effective_stt_provider(settings, usable) == PROVIDER_ID;
    let voice = crate::tts::effective_engine(settings, usable) == PROVIDER_ID;
    FreeCreditJobs {
        assistant,
        cleanup,
        command,
        stt,
        voice,
    }
}

/// Tell every window the status changed.
pub fn notify() {
    let Some(app) = APP.get() else { return };
    let app = app.clone();
    // Off the caller's thread: callers may hold the account manager's lock,
    // and reading the settings must not wait on it.
    std::thread::spawn(move || {
        let settings = crate::settings::get_settings(&app);
        if let Err(e) = app.emit(EVENT, status(&settings)) {
            warn!("Could not send the free credit status: {e}");
        }
    });
}

/// Point one job at Talvio free. The user's own provider choices for the
/// other jobs, and their keys, are left alone.
pub fn choose(settings: &mut AppSettings, job: FreeCreditJob) {
    let id = PROVIDER_ID.to_string();
    match job {
        FreeCreditJob::Assistant => {
            settings.assistant_provider_id = id.clone();
            settings.assistant_last_cloud_provider_id = Some(id.clone());
            settings.assistant_models.insert(id, CHAT_MODEL.to_string());
        }
        FreeCreditJob::Cleanup => {
            settings.post_process_provider_id = id.clone();
            settings.post_process_last_cloud_provider_id = Some(id.clone());
            settings
                .post_process_models
                .insert(id, CHAT_MODEL.to_string());
        }
        FreeCreditJob::Command => {
            settings.command_router_provider_id = id;
            settings.command_router_model = CHAT_MODEL.to_string();
        }
        FreeCreditJob::Stt => {
            settings.stt_engine_mode = crate::settings::SttEngineMode::Cloud;
            settings.cloud_stt_provider_id = id.clone();
            settings
                .cloud_stt_models
                .insert(id, TRANSCRIBE_MODEL.to_string());
        }
        FreeCreditJob::Voice => {
            settings.assistant_tts_engine = id;
            // Its own saved model and voice (none, so the engine's defaults).
            settings.sync_active_tts_fields();
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_free_credit_status(app: AppHandle) -> FreeCreditStatus {
    status(&crate::settings::get_settings(&app))
}

/// Ask the server for the balance again (the main window regaining focus).
#[tauri::command]
#[specta::specta]
pub async fn refresh_free_credit(app: AppHandle) -> FreeCreditStatus {
    refresh().await;
    status(&crate::settings::get_settings(&app))
}

/// "Use Talvio free" for one job.
#[tauri::command]
#[specta::specta]
pub fn choose_free_credit(app: AppHandle, job: FreeCreditJob) -> Result<FreeCreditStatus, String> {
    if !signed_in() {
        return Err("unauthorized".to_string());
    }
    let mut settings = crate::settings::get_settings(&app);
    choose(&mut settings, job);
    crate::settings::write_settings(&app, settings);
    let _ = app.emit("assistant-settings-changed", ());
    notify();
    Ok(status(&crate::settings::get_settings(&app)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(usable: bool) -> FreeAccess {
        FreeAccess {
            base_url: "https://sync.example.com/v1/free".into(),
            token: "zv1_secret_token".into(),
            usable,
        }
    }

    #[test]
    fn the_base_url_follows_the_account_server() {
        assert_eq!(
            free_base_url("https://sync.example.com/"),
            "https://sync.example.com/v1/free"
        );
        assert_eq!(
            free_base_url("http://192.168.1.5:8787"),
            "http://192.168.1.5:8787/v1/free"
        );
    }

    #[test]
    fn only_talvio_free_gets_the_session_token() {
        let free = access(true);
        assert_eq!(
            route(PROVIDER_ID, PLACEHOLDER_BASE_URL, "", Some(&free)),
            (free.base_url.clone(), free.token.clone())
        );
        // Another provider keeps its own address and key, signed in or not.
        assert_eq!(
            route("openai", "https://api.openai.com/v1", "sk-x", Some(&free)),
            ("https://api.openai.com/v1".into(), "sk-x".into())
        );
        // Signed out: nowhere, with no credential.
        assert_eq!(
            route(PROVIDER_ID, PLACEHOLDER_BASE_URL, "stale", None),
            (PLACEHOLDER_BASE_URL.into(), String::new())
        );
    }

    #[test]
    fn the_token_never_shows_in_debug_output() {
        let text = format!("{:?}", access(true));
        assert!(!text.contains("zv1_secret_token"), "{text}");
        assert!(text.contains("redacted"));
    }

    #[test]
    fn running_out_is_402_whatever_the_body() {
        assert_eq!(
            classify_failure(
                402,
                r#"{"error":{"code":"credit_exhausted","message":"Out"}}"#
            ),
            Some(FreeFailure::Exhausted)
        );
        assert_eq!(classify_failure(402, "nope"), Some(FreeFailure::Exhausted));
        assert_eq!(
            classify_failure(503, r#"{"error":{"code":"free_disabled"}}"#),
            Some(FreeFailure::Disabled)
        );
        assert_eq!(classify_failure(503, "busy"), None);
        assert_eq!(classify_failure(401, ""), Some(FreeFailure::Unauthorized));
        assert_eq!(classify_failure(429, ""), Some(FreeFailure::RateLimited));
        assert_eq!(classify_failure(500, ""), None);
        assert!(FreeFailure::Exhausted.message().contains("API key"));
    }

    #[test]
    fn the_header_keeps_the_balance_current() {
        assert_eq!(remaining_from_header(" 0.73 "), Some(0.73));
        assert_eq!(remaining_from_header("-1"), Some(0.0));
        assert_eq!(remaining_from_header("NaN"), None);
        assert_eq!(remaining_from_header("abc"), None);
        let before = FreeCreditBalance {
            limit_usd: 1.0,
            used_usd: 0.2,
            remaining_usd: 0.8,
            enabled: true,
        };
        let after = balance_after(Some(&before), 0.73).unwrap();
        assert!((after.used_usd - 0.27).abs() < 1e-9);
        assert_eq!(after.remaining_usd, 0.73);
        assert_eq!(after.limit_usd, 1.0);
        // Nothing to update until the full balance is known.
        assert_eq!(balance_after(None, 0.5), None);
    }

    #[test]
    fn the_balance_parses_from_the_server() {
        let parsed =
            parse_balance(r#"{"limitUsd":1,"usedUsd":0.27,"remainingUsd":0.73,"enabled":true}"#)
                .unwrap();
        assert_eq!(parsed.limit_usd, 1.0);
        assert_eq!(parsed.remaining_usd, 0.73);
        assert!(parsed.enabled);
        assert!(parse_balance("not json").is_none());
    }

    #[test]
    fn the_app_picks_it_only_with_credit_left() {
        let mut balance = FreeCreditBalance {
            limit_usd: 1.0,
            used_usd: 0.0,
            remaining_usd: 1.0,
            enabled: true,
        };
        assert!(usable_from(Some(&balance), false));
        assert!(!usable_from(Some(&balance), true), "after a 402");
        assert!(!usable_from(None, false), "balance not known yet");
        balance.remaining_usd = 0.0;
        assert!(!usable_from(Some(&balance), false));
        balance.remaining_usd = 0.5;
        balance.enabled = false;
        assert!(!usable_from(Some(&balance), false));
    }

    #[test]
    fn choosing_it_touches_one_job_and_no_key() {
        let mut settings = crate::settings::get_default_settings();
        settings
            .post_process_api_keys
            .insert("openai".into(), "sk-mine".into());
        let assistant_before = settings.assistant_provider_id.clone();
        choose(&mut settings, FreeCreditJob::Cleanup);
        assert_eq!(settings.post_process_provider_id, PROVIDER_ID);
        assert_eq!(settings.post_process_models[PROVIDER_ID], CHAT_MODEL);
        assert_eq!(settings.assistant_provider_id, assistant_before);
        assert_eq!(settings.post_process_api_keys["openai"], "sk-mine");
        assert!(settings
            .post_process_api_keys
            .get(PROVIDER_ID)
            .is_none_or(|key| key.is_empty()));

        choose(&mut settings, FreeCreditJob::Stt);
        assert_eq!(
            settings.stt_engine_mode,
            crate::settings::SttEngineMode::Cloud
        );
        assert_eq!(settings.cloud_stt_provider_id, PROVIDER_ID);
    }

    #[test]
    fn the_settings_never_hold_the_token() {
        let mut settings = crate::settings::get_default_settings();
        for job in [
            FreeCreditJob::Assistant,
            FreeCreditJob::Cleanup,
            FreeCreditJob::Command,
            FreeCreditJob::Stt,
            FreeCreditJob::Voice,
        ] {
            choose(&mut settings, job);
        }
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("zv1_secret_token"));
        // The registries carry only the unreachable placeholder.
        let provider = settings.post_process_provider(PROVIDER_ID).unwrap();
        assert_eq!(provider.base_url, PLACEHOLDER_BASE_URL);
    }

    #[test]
    fn nothing_runs_on_it_signed_out() {
        let mut settings = crate::settings::get_default_settings();
        choose(&mut settings, FreeCreditJob::Assistant);
        assert_eq!(serving(&settings, false, false), FreeCreditJobs::default());
        assert!(serving(&settings, true, false).assistant);
    }

    fn configured(provider: &str, model: &str) -> AppSettings {
        let mut settings = crate::settings::get_default_settings();
        settings.assistant_provider_id = provider.into();
        settings
            .assistant_models
            .insert(provider.into(), model.into());
        settings
    }

    #[test]
    fn the_assistant_falls_back_only_when_its_choice_cannot_run() {
        // The default selection has no model: Talvio free stands in.
        let settings = crate::settings::get_default_settings();
        assert_eq!(
            settings
                .assistant_provider_with(true)
                .map(|p| p.id.as_str()),
            Some(PROVIDER_ID)
        );
        assert_ne!(
            settings
                .assistant_provider_with(false)
                .map(|p| p.id.as_str()),
            Some(PROVIDER_ID)
        );
        // A provider that needs a key it does not have: free stands in.
        let mut settings = configured("openai", "gpt-5-mini");
        assert_eq!(
            settings.assistant_provider_with(true).unwrap().id,
            PROVIDER_ID
        );
        // With its key it is the user's, and stays.
        settings
            .post_process_api_keys
            .insert("openai".into(), "sk-mine".into());
        assert_eq!(settings.assistant_provider_with(true).unwrap().id, "openai");
        // A keyless local server with a model is set up too.
        let settings = configured("local", "llama3");
        assert_eq!(settings.assistant_provider_with(true).unwrap().id, "local");
    }

    #[test]
    fn cleanup_uses_it_after_both_of_the_users_selections() {
        let mut settings = crate::settings::get_default_settings();
        settings.post_process_provider_id = "openai".into();
        settings
            .post_process_models
            .insert("openai".into(), "gpt-5-mini".into());
        let (provider, model, key, source) =
            crate::settings::resolve_post_process_brain_with(&settings, true).unwrap();
        assert_eq!(provider.id, PROVIDER_ID);
        assert_eq!(model, CHAT_MODEL);
        assert!(key.is_empty(), "the token is never in the resolved config");
        assert_eq!(source, crate::settings::PostProcessConfigSource::FreeCredit);
        assert!(crate::settings::resolve_post_process_brain_with(&settings, false).is_err());
        settings
            .post_process_api_keys
            .insert("openai".into(), "sk-mine".into());
        let (provider, ..) =
            crate::settings::resolve_post_process_brain_with(&settings, true).unwrap();
        assert_eq!(provider.id, "openai");
    }

    #[test]
    fn cloud_transcription_takes_the_token_at_request_time() {
        let mut settings = crate::settings::get_default_settings();
        choose(&mut settings, FreeCreditJob::Stt);
        let free = access(true);
        let resolved = crate::stt_cloud::resolve_cloud_stt_with(&settings, Some(&free))
            .ok()
            .unwrap();
        assert_eq!(resolved.base_url, free.base_url);
        assert_eq!(resolved.api_key, free.token);
        assert_eq!(resolved.model, TRANSCRIBE_MODEL);
        // Signed out: it says why instead of sending anything.
        let refused = crate::stt_cloud::resolve_cloud_stt_with(&settings, None)
            .err()
            .unwrap();
        assert_eq!(
            refused.reason,
            crate::settings::CloudSttUnavailableReason::SignInRequired
        );
        // The settings never held the token.
        assert!(settings
            .cloud_stt_api_keys
            .get(PROVIDER_ID)
            .is_none_or(|key| key.is_empty()));
    }

    #[test]
    fn a_cloud_provider_without_its_key_falls_back_to_free() {
        let mut settings = crate::settings::get_default_settings();
        settings.stt_engine_mode = crate::settings::SttEngineMode::Cloud;
        settings.cloud_stt_provider_id = "openai".into();
        assert_eq!(
            crate::stt_cloud::effective_stt_provider(&settings, true),
            PROVIDER_ID
        );
        assert_eq!(
            crate::stt_cloud::effective_stt_provider(&settings, false),
            "openai"
        );
        settings
            .cloud_stt_api_keys
            .insert("openai".into(), "sk-mine".into());
        assert_eq!(
            crate::stt_cloud::effective_stt_provider(&settings, true),
            "openai"
        );
        // A keyless self-hosted server is the user's own setup.
        settings.cloud_stt_provider_id = "custom".into();
        assert_eq!(
            crate::stt_cloud::effective_stt_provider(&settings, true),
            "custom"
        );
    }

    #[test]
    fn a_voice_waiting_for_its_key_speaks_with_free() {
        let mut settings = crate::settings::get_default_settings();
        settings.assistant_tts_engine = "kokoro".into();
        assert_eq!(crate::tts::effective_engine(&settings, true), "kokoro");
        settings.assistant_tts_engine = "openai".into();
        settings.assistant_tts_api_key = crate::settings::SecretString(String::new());
        assert_eq!(crate::tts::effective_engine(&settings, true), PROVIDER_ID);
        assert_eq!(crate::tts::effective_engine(&settings, false), "openai");
        settings.assistant_tts_api_key = crate::settings::SecretString("sk".into());
        assert_eq!(crate::tts::effective_engine(&settings, true), "openai");
    }
}
