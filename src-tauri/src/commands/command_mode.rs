//! Tauri commands for command mode (`crate::command_mode`): the typed entry
//! in Settings → Debug, the answers and confirmations a command waits for,
//! the contacts file, and the command-mode settings.
//!
//! Everything here is a thin edge. The executor never fails a command with an
//! `Err` (a failure is a report with `status: failed` and a message the UI can
//! show), so the run commands return a `CommandReport` rather than a
//! `Result`. They are `async` because a command is an LLM call plus whatever
//! its tools do, and a synchronous Tauri command runs on the main thread.
//!
//! The setters normalise through the pure functions at the bottom, so what a
//! stray space or an emptied field turns into is decided in one tested place
//! rather than by whichever surface wrote the value.

use log::{info, warn};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::command_mode::contacts::ContactBook;
use crate::command_mode::{CommandOrigin, CommandReport};
use crate::settings::{
    clamp_command_agent_max_cost_usd, clamp_command_agent_max_steps, get_settings, write_settings,
    COMMAND_SEARCH_QUERY_PLACEHOLDER, DEFAULT_COMMAND_AGENT_MODEL, DEFAULT_COMMAND_ROUTER_MODEL,
    DEFAULT_COMMAND_ROUTER_PROVIDER_ID, DEFAULT_COMMAND_SEARCH_URL,
};

/// What a new contacts file holds: one example the user edits into their own,
/// which also shows the shape (name, aliases, emails). JSON has no comments,
/// so the example is the documentation.
const CONTACTS_TEMPLATE: &str = r#"[
  {
    "name": "Ron Weasley",
    "aliases": ["Ron"],
    "emails": ["ron@example.com"]
  }
]
"#;

/// Run a command typed into Settings → Debug. `dry_run` overrides the saved
/// `command_dry_run` setting for this one command; `null` uses the setting.
///
/// Not gated on `command_mode_enabled`: this box is the developer surface
/// (the Debug tab is itself hidden behind debug mode), and testing a command
/// must not require switching the feature on for the hotkey too.
#[tauri::command]
#[specta::specta]
pub async fn command_run_text(
    app: AppHandle,
    text: String,
    dry_run: Option<bool>,
) -> CommandReport {
    crate::command_mode::run_command(&app, &text, CommandOrigin::Typed, dry_run, None).await
}

/// Answer the question a command asked (`status: needs_answer`), by the
/// report's `id`. The command carries on from where it stopped.
#[tauri::command]
#[specta::specta]
pub async fn command_answer(app: AppHandle, id: u32, answer: String) -> CommandReport {
    crate::command_mode::answer_command(&app, id, &answer).await
}

/// Accept (`true`, "send") or decline what a command left waiting for
/// confirmation (`status: awaiting_confirmation`), by the report's `id`.
#[tauri::command]
#[specta::specta]
pub async fn command_confirm(app: AppHandle, id: u32, accept: bool) -> CommandReport {
    crate::command_mode::confirm_command(&app, id, accept).await
}

/// Stop the command that is running, if any: an agent task stops before its
/// next request or action and leaves the screen as it is. Esc and `--cancel`
/// do the same through `utils::cancel_current_operation`.
#[tauri::command]
#[specta::specta]
pub async fn command_cancel() {
    crate::command_mode::cancel_running();
}

/// Open the contacts file in the user's editor, creating it with one example
/// contact first if there is none. A file that exists is never touched, even
/// an empty or broken one: it is the user's, and the loader already says
/// what is wrong with it.
#[tauri::command]
#[specta::specta]
pub async fn command_open_contacts_file(app: AppHandle) -> Result<(), String> {
    let path = ContactBook::path(&app)?;
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, CONTACTS_TEMPLATE)
            .map_err(|e| format!("Could not create {}: {e}", path.display()))?;
        info!(
            "command mode: created the contacts file at {}",
            path.display()
        );
    }

    let shown = path.to_string_lossy().to_string();
    if let Err(e) = app.opener().open_path(shown, None::<String>) {
        // Windows has no default app for `.json` on a fresh install, and the
        // open fails rather than asking; showing the file in its folder still
        // gets the user to it.
        warn!("command mode: could not open the contacts file ({e}); revealing it instead");
        app.opener()
            .reveal_item_in_dir(&path)
            .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Turn command mode on or off.
#[tauri::command]
#[specta::specta]
pub async fn change_command_mode_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_mode_enabled = enabled;
    write_settings(&app, settings);
    Ok(())
}

/// The provider (a `post_process_providers` id) that routes commands. Empty
/// goes back to the default.
#[tauri::command]
#[specta::specta]
pub async fn change_command_router_provider_id_setting(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_router_provider_id = normalize_router_provider_id(&provider_id);
    write_settings(&app, settings);
    Ok(())
}

/// The model that routes commands. Empty goes back to the default.
#[tauri::command]
#[specta::specta]
pub async fn change_command_router_model_setting(
    app: AppHandle,
    model: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_router_model = normalize_router_model(&model);
    write_settings(&app, settings);
    Ok(())
}

/// Whether commands show what they would do instead of doing it.
#[tauri::command]
#[specta::specta]
pub async fn change_command_dry_run_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_dry_run = enabled;
    write_settings(&app, settings);
    Ok(())
}

/// The Google account (`authuser=`) Gmail and Calendar links open in. Empty
/// means the browser's default account.
#[tauri::command]
#[specta::specta]
pub async fn change_command_google_account_setting(
    app: AppHandle,
    account: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_google_account = normalize_google_account(&account);
    write_settings(&app, settings);
    Ok(())
}

/// The search page a web search opens. It must contain `{query}`; anything
/// else goes back to the default rather than opening a page that ignores
/// what was asked.
#[tauri::command]
#[specta::specta]
pub async fn change_command_search_url_setting(app: AppHandle, url: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_search_url = normalize_search_url(&url);
    write_settings(&app, settings);
    Ok(())
}

/// Whether a command no tool fits is handed to the screen agent.
#[tauri::command]
#[specta::specta]
pub async fn change_command_auto_escalate_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_auto_escalate = enabled;
    write_settings(&app, settings);
    Ok(())
}

/// The model the screen agent runs on. Empty goes back to the default.
#[tauri::command]
#[specta::specta]
pub async fn change_command_agent_model_setting(
    app: AppHandle,
    model: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_agent_model = non_empty_or(&model, DEFAULT_COMMAND_AGENT_MODEL);
    write_settings(&app, settings);
    Ok(())
}

/// The most LLM rounds one agent task may use, kept within 1–60.
#[tauri::command]
#[specta::specta]
pub async fn change_command_agent_max_steps_setting(
    app: AppHandle,
    steps: u32,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_agent_max_steps = clamp_command_agent_max_steps(steps);
    write_settings(&app, settings);
    Ok(())
}

/// What one agent task may cost, in US dollars, kept within $0.01–$20.
#[tauri::command]
#[specta::specta]
pub async fn change_command_agent_max_cost_usd_setting(
    app: AppHandle,
    usd: f64,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.command_agent_max_cost_usd = clamp_command_agent_max_cost_usd(usd);
    write_settings(&app, settings);
    Ok(())
}

/// Trimmed; empty means the default provider.
fn normalize_router_provider_id(value: &str) -> String {
    non_empty_or(value, DEFAULT_COMMAND_ROUTER_PROVIDER_ID)
}

/// Trimmed; empty means the default model.
fn normalize_router_model(value: &str) -> String {
    non_empty_or(value, DEFAULT_COMMAND_ROUTER_MODEL)
}

/// Trimmed and nothing else: an email address, an account index ("1") and
/// empty (the browser's default account) are all meaningful to `authuser=`.
fn normalize_google_account(value: &str) -> String {
    value.trim().to_string()
}

/// Trimmed; a URL without `{query}`, or one that is not http(s), means the
/// default search page. A page that cannot receive the query would open on
/// every search and show nothing the user asked for.
fn normalize_search_url(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    let is_web = lower.starts_with("https://") || lower.starts_with("http://");
    if is_web && trimmed.contains(COMMAND_SEARCH_QUERY_PLACEHOLDER) {
        trimmed.to_string()
    } else {
        DEFAULT_COMMAND_SEARCH_URL.to_string()
    }
}

fn non_empty_or(value: &str, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider or model field cleared in the UI must not leave command
    /// mode with nothing to route through; it goes back to the default.
    #[test]
    fn an_empty_provider_or_model_goes_back_to_the_default() {
        assert_eq!(normalize_router_provider_id(""), "anthropic");
        assert_eq!(normalize_router_provider_id("   "), "anthropic");
        assert_eq!(normalize_router_model(""), "claude-haiku-5-5");
        assert_eq!(normalize_router_model(" \t\n"), "claude-haiku-5-5");
    }

    /// An emptied agent model goes back to the default rather than leaving
    /// the agent nothing to run on.
    #[test]
    fn an_empty_agent_model_goes_back_to_the_default() {
        assert_eq!(
            non_empty_or("  ", DEFAULT_COMMAND_AGENT_MODEL),
            "claude-sonnet-5-5"
        );
        assert_eq!(
            non_empty_or(" claude-opus-5-5 ", DEFAULT_COMMAND_AGENT_MODEL),
            "claude-opus-5-5"
        );
    }

    /// A pasted id with spaces around it is the id, not an unknown provider.
    #[test]
    fn provider_and_model_are_trimmed_and_otherwise_kept() {
        assert_eq!(normalize_router_provider_id("  openrouter "), "openrouter");
        assert_eq!(
            normalize_router_model(" openai/gpt-5-mini\n"),
            "openai/gpt-5-mini"
        );
    }

    /// Every form `authuser=` takes survives, including empty (the default
    /// account), which must not be replaced with anything.
    #[test]
    fn a_google_account_is_only_trimmed() {
        assert_eq!(normalize_google_account(""), "");
        assert_eq!(normalize_google_account("  "), "");
        assert_eq!(
            normalize_google_account(" me@example.com "),
            "me@example.com"
        );
        assert_eq!(normalize_google_account("1"), "1");
    }

    /// A search page that cannot receive the query is no search page.
    #[test]
    fn a_search_url_without_the_query_placeholder_falls_back_to_google() {
        assert_eq!(normalize_search_url(""), DEFAULT_COMMAND_SEARCH_URL);
        assert_eq!(
            normalize_search_url("https://duckduckgo.com/"),
            DEFAULT_COMMAND_SEARCH_URL
        );
        assert_eq!(
            normalize_search_url("https://duckduckgo.com/?q={q}"),
            DEFAULT_COMMAND_SEARCH_URL
        );
    }

    /// Only web pages: a `file:` or `javascript:` "search page" is refused,
    /// since the URL is opened with the system's default handler.
    #[test]
    fn a_search_url_must_be_a_web_page() {
        assert_eq!(
            normalize_search_url("file:///C:/x?q={query}"),
            DEFAULT_COMMAND_SEARCH_URL
        );
        assert_eq!(
            normalize_search_url("javascript:alert('{query}')"),
            DEFAULT_COMMAND_SEARCH_URL
        );
    }

    /// A good search URL is kept as typed, minus surrounding spaces.
    #[test]
    fn a_search_url_with_the_placeholder_is_kept() {
        assert_eq!(
            normalize_search_url("  https://duckduckgo.com/?q={query} "),
            "https://duckduckgo.com/?q={query}"
        );
        assert_eq!(
            normalize_search_url("HTTPS://www.bing.com/search?q={query}"),
            "HTTPS://www.bing.com/search?q={query}"
        );
    }

    /// The template a new contacts file starts from must load as one contact,
    /// or the first thing the user sees of the feature is a parse error.
    #[test]
    fn the_contacts_template_loads_as_one_contact() {
        let book = ContactBook::parse(CONTACTS_TEMPLATE);
        assert!(book.load_error.is_none(), "{:?}", book.load_error);
        assert_eq!(book.contacts().len(), 1);
        assert_eq!(book.contacts()[0].name, "Ron Weasley");
        assert_eq!(book.contacts()[0].primary_email(), Some("ron@example.com"));
    }
}
