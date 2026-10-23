//! `open_app`: launch an installed application by the name the user said.
//!
//! The installed list comes from the platform ([`Effects::installed_apps`]:
//! Start-menu shortcuts and App Paths on Windows, `.app` bundles on macOS,
//! `.desktop` files on Linux), and the spoken name is matched against it with
//! the shared fuzzy rule: one clear winner launches, several close ones become
//! a question, nothing close is a failure the computer-use fallback may still
//! solve (the app could be a portable one no list knows about).
//!
//! A well-known site said by name ("open YouTube", "open Gmail") is not
//! matched fuzzily at all: the site's own desktop app is launched when it is
//! installed under exactly that name (Spotify, WhatsApp), and otherwise the
//! site opens in the browser ([`crate::command_mode::sites`]). Without this,
//! "open Google" launched Google Chrome and "open YouTube" failed.
//!
//! The app matching lives here and is `pub(crate)` because `switch_to_app`
//! falls back to it when no window matches: "switch to Spotify" with Spotify
//! closed should open it rather than fail.
//!
//! [`Effects::installed_apps`]: crate::command_mode::effects::Effects::installed_apps

use serde_json::{json, Value};
use std::collections::HashSet;

use crate::command_mode::effects::AppEntry;
use crate::command_mode::fuzzy::{self, Pick, DEFAULT_LEAD, DEFAULT_MIN_SCORE};
use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};
use crate::command_mode::sites;

use super::open_url::open_site;

pub static SPEC: ToolSpec = ToolSpec {
    name: "open_app",
    description: "Launch an installed application by name (\"open Spotify\", \"start VS Code\"). \
        Use switch_to_app instead when the user wants a window that is already open.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &["open", "launch", "start", "run", "app", "application", "program"],
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
                "description": "The app's name as the user said it, e.g. \"Spotify\" or \"Visual Studio Code\"."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn handler(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(name) = arg_str(&args, "name") else {
        return ToolOutcome::failed("Which app should I open?", false);
    };
    let apps = ctx.effects.installed_apps();
    log::debug!("open_app: matching against {} installed apps", apps.len());

    if let Some(site) = sites::named(&clean_spoken_name(name)) {
        return match apps.iter().find(|app| sites::is_own_app(site, &app.name)) {
            Some(app) => launch(ctx, app),
            None => open_site(ctx, site),
        };
    }

    match pick_app(name, &apps) {
        Pick::Unique(app) => launch(ctx, &app),
        Pick::Ambiguous(options) => ToolOutcome::ask("Which one?", app_choices(&options)),
        Pick::None => ToolOutcome::failed(format!("I couldn't find an app called {name}."), true),
    }
}

fn launch(ctx: &ToolCtx, app: &AppEntry) -> ToolOutcome {
    match ctx.effects.launch_app(app) {
        Ok(()) => ToolOutcome::done("openingApp", "Opening {{app}}.", &[("app", &app.name)]),
        Err(e) => {
            log::warn!("open_app: launching an app failed: {e}");
            // The computer-use fallback can still go through the Start menu or
            // Launchpad when the direct launch is refused.
            ToolOutcome::failed(e, true)
        }
    }
}

/// Choose the installed app `spoken` means.
///
/// Apps are deduplicated by name first: Windows lists the same app once per
/// user and once for all users, and a question offering "Slack" twice cannot
/// be answered. Each app is known by its full name and, when it ends in a
/// qualifier, by the name without it ("Microsoft Teams (work or school)" is
/// also "Microsoft Teams").
pub(crate) fn pick_app(spoken: &str, apps: &[AppEntry]) -> Pick<AppEntry> {
    let query = clean_spoken_name(spoken);
    let mut seen = HashSet::new();
    let unique: Vec<AppEntry> = apps
        .iter()
        .filter(|app| !app.name.trim().is_empty())
        .filter(|app| seen.insert(fuzzy::normalize(&app.name)))
        .cloned()
        .collect();
    fuzzy::pick(&query, &unique, app_names, DEFAULT_MIN_SCORE, DEFAULT_LEAD)
}

/// The names an installed app answers to.
fn app_names(app: &AppEntry) -> Vec<String> {
    let mut names = vec![app.name.clone()];
    if let Some(bare) = strip_qualifier(&app.name) {
        names.push(bare.to_string());
    }
    names
}

/// The choices of a "Which one?" question, one per app.
pub(crate) fn app_choices(apps: &[AppEntry]) -> Vec<String> {
    let mut seen = HashSet::new();
    apps.iter()
        .map(|app| app.name.clone())
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

/// "Microsoft Teams (work or school)" → "Microsoft Teams". `None` when there is
/// no trailing parenthesised qualifier, or nothing would be left without it.
pub(crate) fn strip_qualifier(name: &str) -> Option<&str> {
    let trimmed = name.trim_end();
    if !trimmed.ends_with(')') {
        return None;
    }
    let open = trimmed.rfind(" (")?;
    let bare = trimmed[..open].trim_end();
    (!bare.is_empty()).then_some(bare)
}

/// Words the router or the user wrap around a name that are not part of it:
/// "the Spotify app", "my Slack window", "the Gmail tab". Dropping them matters
/// because the fuzzy match wants every query word in the candidate, and no app
/// is called "the". A name made only of such words is left as it was.
pub(crate) fn clean_spoken_name(spoken: &str) -> String {
    const LEADING: &[&str] = &["the", "my", "a", "an", "that", "this"];
    const TRAILING: &[&str] = &["app", "application", "program", "window", "tab"];

    let is_one_of = |word: &str, list: &[&str]| list.iter().any(|w| w.eq_ignore_ascii_case(word));
    let words: Vec<&str> = spoken.split_whitespace().collect();
    let mut start = 0;
    let mut end = words.len();
    // `end - start > 1`: never strip the last word left.
    while end - start > 1 && is_one_of(words[start], LEADING) {
        start += 1;
    }
    while end - start > 1 && is_one_of(words[end - 1], TRAILING) {
        end -= 1;
    }
    words[start..end].join(" ")
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

    fn run(apps: Vec<AppEntry>, name: &str) -> (ToolOutcome, Vec<String>) {
        let fake = Arc::new(FakeEffects {
            apps,
            ..Default::default()
        });
        let outcome = handler(&ctx_with(fake.clone()), json!({ "name": name }));
        (outcome, fake.actions())
    }

    #[test]
    fn one_clear_match_is_launched_and_announced() {
        let (outcome, actions) = run(vec![app("Spotify"), app("Slack"), app("Calculator")], "spotify");
        assert_eq!(actions, vec!["launch_app Spotify.lnk".to_string()]);
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done, got {outcome:?}");
        };
        assert_eq!(feedback.key, "commandMode.feedback.openingApp");
        assert_eq!(feedback.render_en(), "Opening Spotify.");
    }

    /// Two equally good apps must become a question, never a coin toss.
    #[test]
    fn two_close_matches_are_a_question_and_launch_nothing() {
        let (outcome, actions) = run(
            vec![
                app("Microsoft Teams (work or school)"),
                app("Microsoft Teams classic"),
                app("Notepad"),
            ],
            "teams",
        );
        assert!(actions.is_empty());
        let ToolOutcome::AskUser { question, choices } = outcome else {
            panic!("expected a question, got {outcome:?}");
        };
        assert_eq!(question, "Which one?");
        assert_eq!(
            choices,
            vec![
                "Microsoft Teams (work or school)".to_string(),
                "Microsoft Teams classic".to_string()
            ]
        );
    }

    /// An app nobody lists may still be reachable by clicking around, so the
    /// failure invites the computer-use fallback.
    #[test]
    fn an_unknown_app_fails_and_may_escalate() {
        let (outcome, actions) = run(vec![app("Notepad")], "Photoshop");
        assert!(actions.is_empty());
        assert_eq!(
            outcome,
            ToolOutcome::failed("I couldn't find an app called Photoshop.", true)
        );
    }

    /// Answering the question with one of its choices must settle it.
    #[test]
    fn answering_with_a_choice_picks_that_app() {
        let (_, actions) = run(
            vec![app("Microsoft Teams (work or school)"), app("Microsoft Teams classic")],
            "Microsoft Teams (work or school)",
        );
        assert_eq!(
            actions,
            vec!["launch_app Microsoft Teams (work or school).lnk".to_string()]
        );
    }

    #[test]
    fn a_trailing_qualifier_is_optional() {
        assert_eq!(
            strip_qualifier("Microsoft Teams (work or school)"),
            Some("Microsoft Teams")
        );
        assert_eq!(strip_qualifier("Spotify"), None);
        assert_eq!(strip_qualifier("(x)"), None);
        // The bare name scores as an exact match, so it beats a longer namesake.
        let (_, actions) = run(vec![app("Paint (Preview)"), app("Paint 3D")], "Paint");
        assert_eq!(actions, vec!["launch_app Paint (Preview).lnk".to_string()]);
    }

    /// Windows lists many apps twice (per user and for all users); that is
    /// one app, not a question with two identical answers.
    #[test]
    fn duplicate_entries_of_one_app_are_not_ambiguous() {
        let fake = Arc::new(FakeEffects {
            apps: vec![
                AppEntry {
                    name: "Slack".into(),
                    launch: "user\\Slack.lnk".into(),
                },
                AppEntry {
                    name: "Slack".into(),
                    launch: "all\\Slack.lnk".into(),
                },
            ],
            ..Default::default()
        });
        let outcome = handler(&ctx_with(fake.clone()), json!({ "name": "slack" }));
        assert!(matches!(outcome, ToolOutcome::Done { .. }), "{outcome:?}");
        assert_eq!(fake.actions(), vec!["launch_app user\\Slack.lnk".to_string()]);
    }

    #[test]
    fn filler_words_around_the_name_are_ignored() {
        assert_eq!(clean_spoken_name("the Spotify app"), "Spotify");
        assert_eq!(clean_spoken_name("my Slack window"), "Slack");
        assert_eq!(clean_spoken_name("the Gmail tab"), "Gmail");
        // Nothing is stripped when it would leave nothing.
        assert_eq!(clean_spoken_name("the"), "the");
        assert_eq!(clean_spoken_name("app"), "app");
        let (_, actions) = run(vec![app("Spotify"), app("Slack")], "the Spotify app");
        assert_eq!(actions, vec!["launch_app Spotify.lnk".to_string()]);
    }

    /// A launch the OS refuses is reported with the platform's own sentence.
    #[test]
    fn a_refused_launch_is_a_failure_with_the_reason() {
        let fake = Arc::new(FakeEffects {
            apps: vec![app("Spotify")],
            fail_with: Some("Windows could not start Spotify.".into()),
            ..Default::default()
        });
        let outcome = handler(&ctx_with(fake), json!({ "name": "Spotify" }));
        assert_eq!(
            outcome,
            ToolOutcome::failed("Windows could not start Spotify.", true)
        );
    }

    /// "open YouTube" with no YouTube app opens the site, never a fuzzy
    /// near-miss and never a failure.
    #[test]
    fn a_site_without_its_app_opens_in_the_browser() {
        let (outcome, actions) = run(
            vec![app("YouTube Music"), app("Google Chrome"), app("Notepad")],
            "YouTube",
        );
        assert_eq!(actions, vec!["open_url https://www.youtube.com/".to_string()]);
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done, got {outcome:?}");
        };
        assert_eq!(feedback.key, "commandMode.feedback.urlOpened");
        assert_eq!(feedback.render_en(), "Opened YouTube.");

        // "open Google" means the search page, not Google Chrome.
        let (_, actions) = run(vec![app("Google Chrome")], "google");
        assert_eq!(actions, vec!["open_url https://www.google.com/".to_string()]);

        let (_, actions) = run(Vec::new(), "the Gmail app");
        assert_eq!(actions, vec!["open_url https://mail.google.com/".to_string()]);
    }

    /// The site's own desktop app wins when it is installed.
    #[test]
    fn an_installed_app_beats_its_website() {
        let (outcome, actions) = run(vec![app("Spotify"), app("Slack")], "spotify");
        assert_eq!(actions, vec!["launch_app Spotify.lnk".to_string()]);
        assert!(matches!(outcome, ToolOutcome::Done { .. }));
        let (_, actions) = run(vec![app("WhatsApp")], "WhatsApp");
        assert_eq!(actions, vec!["launch_app WhatsApp.lnk".to_string()]);
        let (_, actions) = run(vec![app("WhatsApp")], "netflix");
        assert_eq!(actions, vec!["open_url https://www.netflix.com/".to_string()]);
    }

    #[test]
    fn the_schema_requires_a_name_and_nothing_else() {
        assert!(validate_args(&SPEC, &json!({ "name": "Spotify" })).is_ok());
        assert!(validate_args(&SPEC, &json!({})).is_err());
        assert!(validate_args(&SPEC, &json!({ "name": "x", "path": "y" })).is_err());
    }
}
