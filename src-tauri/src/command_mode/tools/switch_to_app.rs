//! `switch_to_app`: bring a window that is already open to the front.
//!
//! The user names an app ("Slack") or something on a window's title bar ("the
//! Gmail tab", "the budget spreadsheet"), so each window answers to its app,
//! its title, and both together. Three refinements on the shared fuzzy rule:
//!
//! - Several windows of one app, when the user named the app rather than a
//!   window, are not a question: the front-most one is meant. "Switch to
//!   Chrome" asking which of five Chrome windows reads as obtuse.
//! - Nothing open by that name falls back to launching it, because "switch to
//!   Spotify" with Spotify closed means "I want Spotify", not "I want an error".
//! - The failure does not escalate: if neither a window nor an installed app
//!   matches, the computer-use fallback has nothing better to click.
//!
//! Not offered on Wayland, which has no way for one client to activate another
//! client's window.

use serde_json::{json, Value};
use std::collections::HashSet;

use super::open_app::{app_choices, clean_spoken_name, pick_app};
use crate::command_mode::effects::{AppEntry, WindowEntry};
use crate::command_mode::fuzzy::{self, Pick, DEFAULT_LEAD, DEFAULT_MIN_SCORE};
use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, NOT_WAYLAND};

pub static SPEC: ToolSpec = ToolSpec {
    name: "switch_to_app",
    description: "Bring an already-open app or window to the front, by app name or by words in \
        its title (\"switch to Slack\", \"go to the Gmail tab\"). Opens the app if it is not \
        running.",
    schema,
    risk: Risk::Safe,
    platforms: NOT_WAYLAND,
    keywords: &["switch", "go to", "focus", "bring up", "show", "window", "tab", "back to"],
    handler,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "The app or window as the user said it, e.g. \"Slack\" or \"the Gmail tab\"."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn handler(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(name) = arg_str(&args, "name") else {
        return ToolOutcome::failed("Which app should I switch to?", false);
    };
    let windows: Vec<WindowEntry> = ctx
        .effects
        .open_windows()
        .into_iter()
        .filter(|w| !w.title.trim().is_empty() || !w.app.trim().is_empty())
        .collect();
    log::debug!("switch_to_app: matching against {} open windows", windows.len());

    let query = clean_spoken_name(name);
    match fuzzy::pick(&query, &windows, window_names, DEFAULT_MIN_SCORE, DEFAULT_LEAD) {
        Pick::Unique(window) => focus(ctx, &window),
        Pick::Ambiguous(options) => match front_window_of_named_app(&query, &windows, &options) {
            Some(window) => focus(ctx, &window),
            None => ToolOutcome::ask("Which one?", window_choices(&options)),
        },
        Pick::None => open_instead(ctx, name),
    }
}

fn focus(ctx: &ToolCtx, window: &WindowEntry) -> ToolOutcome {
    match ctx.effects.focus_window(window) {
        Ok(()) => {
            let title = if window.title.trim().is_empty() {
                window.app.as_str()
            } else {
                window.title.as_str()
            };
            ToolOutcome::done("switchedTo", "Switched to {{title}}.", &[("title", title)])
        }
        Err(e) => {
            log::warn!("switch_to_app: focusing a window failed: {e}");
            // Clicking the window on the taskbar can work where activation is
            // refused, so this one may escalate.
            ToolOutcome::failed(e, true)
        }
    }
}

/// No window matches: launch the app if one by that name is installed.
fn open_instead(ctx: &ToolCtx, name: &str) -> ToolOutcome {
    let apps = ctx.effects.installed_apps();
    match pick_app(name, &apps) {
        Pick::Unique(app) => launch(ctx, &app),
        Pick::Ambiguous(options) => ToolOutcome::ask("Which one?", app_choices(&options)),
        Pick::None => ToolOutcome::failed(
            format!("I couldn't find a window or an app called {name}."),
            false,
        ),
    }
}

fn launch(ctx: &ToolCtx, app: &AppEntry) -> ToolOutcome {
    match ctx.effects.launch_app(app) {
        Ok(()) => ToolOutcome::done(
            "openedInstead",
            "{{app}} wasn't open, so I opened it.",
            &[("app", &app.name)],
        ),
        Err(e) => {
            log::warn!("switch_to_app: launching an app failed: {e}");
            ToolOutcome::failed(e, true)
        }
    }
}

/// The names a window answers to: its app, its title, and the two together
/// (which is also exactly what a [`window_choices`] answer normalizes to, so
/// answering the question with one of its choices is an exact match).
fn window_names(window: &WindowEntry) -> Vec<String> {
    let app = bare_app(&window.app);
    let mut names = Vec::with_capacity(3);
    if !app.is_empty() {
        names.push(app.to_string());
    }
    if !window.title.trim().is_empty() {
        names.push(window.title.clone());
    }
    if !app.is_empty() && !window.title.trim().is_empty() {
        names.push(format!("{app} {}", window.title));
    }
    names
}

/// The app name without a Windows executable suffix: "chrome.exe" → "chrome",
/// so "chrome" is an exact match rather than a partial one.
fn bare_app(app: &str) -> &str {
    let app = app.trim();
    match app.len().checked_sub(4) {
        Some(cut) if app.is_char_boundary(cut) && app[cut..].eq_ignore_ascii_case(".exe") => {
            &app[..cut]
        }
        _ => app,
    }
}

/// When every close candidate is a window of one app and the user named that
/// app (its name matches at least as well as any of their titles), the
/// front-most of them, which is the first in `windows`' front-to-back order.
fn front_window_of_named_app(
    query: &str,
    windows: &[WindowEntry],
    candidates: &[WindowEntry],
) -> Option<WindowEntry> {
    let first = candidates.first()?;
    let app = fuzzy::normalize(bare_app(&first.app));
    if app.is_empty()
        || !candidates
            .iter()
            .all(|w| fuzzy::normalize(bare_app(&w.app)) == app)
    {
        return None;
    }
    let by_app = fuzzy::score(query, bare_app(&first.app));
    let by_title = candidates
        .iter()
        .map(|w| fuzzy::score(query, &w.title))
        .fold(0.0, f64::max);
    if by_app < by_title {
        return None;
    }
    windows.iter().find(|w| candidates.contains(w)).cloned()
}

/// "Slack — general", one per distinct window, for a "Which one?" question.
fn window_choices(windows: &[WindowEntry]) -> Vec<String> {
    let mut seen = HashSet::new();
    windows
        .iter()
        .map(|w| {
            let app = bare_app(&w.app);
            match (app.is_empty(), w.title.trim().is_empty()) {
                (false, false) => format!("{app} — {}", w.title.trim()),
                (true, _) => w.title.trim().to_string(),
                (false, true) => app.to_string(),
            }
        })
        .filter(|choice| seen.insert(choice.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::registry::{validate_args, Platform};
    use chrono::{FixedOffset, TimeZone};
    use std::sync::Arc;

    fn window(id: i64, app: &str, title: &str) -> WindowEntry {
        WindowEntry {
            id,
            title: title.into(),
            app: app.into(),
        }
    }

    fn app(name: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            launch: format!("{name}.lnk"),
        }
    }

    fn ctx_with(fake: Arc<FakeEffects>) -> ToolCtx {
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 9, 15, 0)
            .unwrap();
        ToolCtx {
            settings: crate::settings::get_default_settings(),
            context: CommandContext::at(now, None, Platform::Windows, None, None),
            effects: fake,
            contacts: Arc::new(ContactBook::new(Vec::new())),
            platform: Platform::Windows,
            dry_run: false,
        }
    }

    fn run(
        windows: Vec<WindowEntry>,
        apps: Vec<AppEntry>,
        name: &str,
    ) -> (ToolOutcome, Vec<String>) {
        let fake = Arc::new(FakeEffects {
            windows,
            apps,
            ..Default::default()
        });
        let outcome = handler(&ctx_with(fake.clone()), json!({ "name": name }));
        (outcome, fake.actions())
    }

    fn desktop() -> Vec<WindowEntry> {
        vec![
            window(1, "chrome.exe", "Inbox (3) - me@example.com - Gmail - Google Chrome"),
            window(2, "Slack", "general - Hogwarts - Slack"),
            window(3, "Code", "main.rs - zynveliq - Visual Studio Code"),
        ]
    }

    #[test]
    fn an_app_name_focuses_its_window() {
        let (outcome, actions) = run(desktop(), vec![], "Slack");
        assert_eq!(actions, vec!["focus_window 2".to_string()]);
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done, got {outcome:?}");
        };
        assert_eq!(feedback.key, "commandMode.feedback.switchedTo");
        assert_eq!(feedback.render_en(), "Switched to general - Hogwarts - Slack.");
    }

    /// "The Gmail tab" names a word of a title, wrapped in filler.
    #[test]
    fn words_of_a_title_find_the_window() {
        let (_, actions) = run(desktop(), vec![], "the Gmail tab");
        assert_eq!(actions, vec!["focus_window 1".to_string()]);
        let (_, actions) = run(desktop(), vec![], "chrome");
        assert_eq!(actions, vec!["focus_window 1".to_string()]);
    }

    /// Windows of different apps whose titles match equally are a question,
    /// with choices that say which app each belongs to and no duplicates.
    #[test]
    fn two_matching_windows_of_different_apps_are_a_question() {
        let windows = vec![
            window(1, "chrome", "Budget"),
            window(2, "EXCEL", "Budget"),
            window(3, "EXCEL", "Budget"),
        ];
        let (outcome, actions) = run(windows, vec![], "budget");
        assert!(actions.is_empty());
        let ToolOutcome::AskUser { question, choices } = outcome else {
            panic!("expected a question, got {outcome:?}");
        };
        assert_eq!(question, "Which one?");
        assert_eq!(
            choices,
            vec!["chrome — Budget".to_string(), "EXCEL — Budget".to_string()]
        );
    }

    /// The answer to that question, echoed back, is an exact match.
    #[test]
    fn answering_with_a_choice_focuses_that_window() {
        let windows = vec![
            window(1, "chrome", "Budget - Google Sheets"),
            window(2, "EXCEL", "Budget"),
        ];
        let (_, actions) = run(windows, vec![], "chrome — Budget - Google Sheets");
        assert_eq!(actions, vec!["focus_window 1".to_string()]);
    }

    /// "Switch to Chrome" with several Chrome windows means the front one.
    #[test]
    fn several_windows_of_the_named_app_pick_the_front_most() {
        let windows = vec![
            window(7, "Slack", "random - Hogwarts - Slack"),
            window(8, "chrome", "Docs"),
            window(9, "chrome", "Calendar"),
        ];
        let (outcome, actions) = run(windows, vec![], "chrome");
        assert_eq!(actions, vec!["focus_window 8".to_string()]);
        assert!(matches!(outcome, ToolOutcome::Done { .. }));
    }

    #[test]
    fn a_closed_app_is_opened_instead() {
        let (outcome, actions) = run(desktop(), vec![app("Spotify"), app("Notepad")], "Spotify");
        assert_eq!(actions, vec!["launch_app Spotify.lnk".to_string()]);
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done, got {outcome:?}");
        };
        assert_eq!(feedback.key, "commandMode.feedback.openedInstead");
        assert_eq!(feedback.render_en(), "Spotify wasn't open, so I opened it.");
    }

    /// Neither open nor installed: computer use has nothing better to try.
    #[test]
    fn nothing_open_or_installed_fails_without_escalating() {
        let (outcome, actions) = run(desktop(), vec![app("Notepad")], "Photoshop");
        assert!(actions.is_empty());
        assert_eq!(
            outcome,
            ToolOutcome::failed("I couldn't find a window or an app called Photoshop.", false)
        );
    }

    #[test]
    fn the_executable_suffix_is_not_part_of_the_app_name() {
        assert_eq!(bare_app("chrome.exe"), "chrome");
        assert_eq!(bare_app("EXCEL.EXE"), "EXCEL");
        assert_eq!(bare_app("Slack"), "Slack");
        assert_eq!(bare_app("exe"), "exe");
        assert_eq!(bare_app("é.exe"), "é");
    }

    #[test]
    fn the_schema_requires_a_name_and_the_tool_is_not_on_wayland() {
        assert!(validate_args(&SPEC, &json!({ "name": "Slack" })).is_ok());
        assert!(validate_args(&SPEC, &json!({})).is_err());
        assert!(!SPEC.supports(Platform::LinuxWayland));
        assert!(SPEC.supports(Platform::LinuxX11));
    }
}
