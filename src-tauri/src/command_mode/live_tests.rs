//! `#[ignore]`d checks of the router against the model this machine's own
//! settings would use (the same fallback to the assistant's provider), so a
//! prompt or tool change can be tried on the real model before a release. Most
//! only ask for the plan; `command_mode_live_carry_out` runs the executor for
//! real (`TALVIO_LIVE_RUN`), and refuses any email not addressed to
//! `TALVIO_LIVE_OWN_EMAIL` alone.
//!
//! ```text
//! cargo test --lib command_mode::live_tests -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use super::context::CommandContext;
use super::llm;
use super::registry::Platform;
use super::router::{self, Followup};
use crate::settings::AppSettings;

/// The settings the installed app would load, with its keys from the keychain.
pub(crate) fn installed_settings() -> AppSettings {
    let dir = std::env::var("APPDATA").expect("APPDATA");
    let path = std::path::Path::new(&dir).join("com.talvio.desktop/settings_store.json");
    let text = std::fs::read_to_string(&path).expect("the installed app's settings");
    let text = text.trim_start_matches('\u{feff}');
    let store: serde_json::Value = serde_json::from_str(text).expect("settings JSON");
    let mut settings: AppSettings =
        serde_json::from_value(store["settings"].clone()).expect("settings shape");
    crate::settings::hydrate_secrets(&mut settings);
    settings
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the model this machine's settings name"]
async fn command_mode_live_routes_everyday_commands() {
    let settings = installed_settings();
    let llm = llm::router_for(&settings).expect("a usable router model");
    println!("router: {}", llm.label());
    let context = CommandContext::capture();
    for (command, tool) in [
        ("open youtube", None),
        ("play believer by imagine dragons", Some("play_media")),
        ("turn the volume down", Some("set_volume")),
        ("pause the music", Some("media_control")),
        ("open gmail", None),
    ] {
        let started = Instant::now();
        let plan = router::route(
            llm.as_ref(),
            Platform::current(),
            command,
            &context,
            &Followup::default(),
            tokio::time::Instant::now() + Duration::from_secs(40),
        )
        .await
        .unwrap_or_else(|e| panic!("{command}: {e:?}"));
        let calls: Vec<String> = plan
            .calls
            .iter()
            .map(|call| format!("{} {}", call.name, call.args))
            .collect();
        println!("{command:?} -> {calls:?} in {:?}", started.elapsed());
        if let Some(tool) = tool {
            assert!(
                plan.calls.iter().any(|call| call.name == tool),
                "{command}: expected {tool}, got {calls:?}"
            );
        }
    }
}

/// The tier-2 agent on a real window: it opens Calculator, asks the agent to
/// work out a sum by pressing its buttons, prints every step, and closes that
/// window. Calculator because it holds nothing of the user's: an earlier
/// version used Notepad, which restores the user's last session on launch,
/// and the agent wrote into one of the user's own tabs. Skipped when
/// Calculator is already open.
#[cfg(target_os = "windows")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "opens Calculator and lets the agent work in it"]
async fn command_mode_live_agent_works_calculator() {
    use super::agent;
    use super::contacts::ContactBook;
    use super::effects::{Effects, LiveEffects};
    use super::registry::ToolCtx;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

    let effects = LiveEffects;
    let is_calculator =
        |app: &str, title: &str| app.to_lowercase().contains("calculator") || title == "Calculator";
    if effects
        .open_windows()
        .iter()
        .any(|w| is_calculator(&w.app, &w.title))
    {
        println!("Calculator is already open; skipping so the user's window is left alone");
        return;
    }
    // calc.exe only starts the Store app and exits.
    std::process::Command::new("calc.exe")
        .status()
        .expect("calc");
    let deadline = Instant::now() + Duration::from_secs(15);
    let calculator = loop {
        if let Some(front) = effects.foreground_window() {
            if is_calculator(&front.app, &front.title) {
                break front;
            }
        }
        // Windows does not always let a launched app take the foreground.
        if let Some(window) = effects
            .open_windows()
            .into_iter()
            .find(|w| is_calculator(&w.app, &w.title))
        {
            let _ = effects.focus_window(&window);
        }
        assert!(
            Instant::now() < deadline,
            "Calculator never came to the front"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    tokio::time::sleep(Duration::from_millis(800)).await;

    let settings = installed_settings();
    let llm = llm::agent_llm_for(&settings).expect("an agent model");
    println!("agent: {}", llm.label());
    let limits = agent::Limits::from_settings(&settings);
    let ctx = Arc::new(ToolCtx {
        settings,
        context: CommandContext::capture(),
        effects: Arc::new(LiveEffects),
        contacts: Arc::new(ContactBook::default()),
        platform: Platform::current(),
        dry_run: false,
    });
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let outcome = agent::run(
        "In the Calculator window, work out 12 times 7 by pressing its buttons",
        ctx,
        llm.as_ref(),
        agent::tree(),
        limits,
        &cancel,
    )
    .await;
    for step in &outcome.steps {
        println!(
            "STEP {} {} -> {} {}",
            step.tool, step.args_json, step.result, step.detail
        );
    }
    println!(
        "END {:?} in {:.1}s, {} requests, {} in / {} out tokens",
        outcome.end,
        started.elapsed().as_secs_f64(),
        outcome.llm_calls,
        outcome.usage.input_tokens,
        outcome.usage.output_tokens
    );

    // SAFETY: posting WM_CLOSE to the window this test opened; a handle that
    // has closed meanwhile only makes the call fail.
    let _ = unsafe {
        PostMessageW(
            Some(HWND(calculator.id as isize as *mut std::ffi::c_void)),
            WM_CLOSE,
            WPARAM(0),
            LPARAM(0),
        )
    };
    assert!(
        matches!(outcome.end, agent::AgentEnd::Done { .. }),
        "the agent did not finish: {:?}",
        outcome.end
    );
}

/// Route each command in `TALVIO_LIVE_COMMANDS` (separated by `|`) several
/// times (`TALVIO_LIVE_REPEAT`, default 5) and print every plan, to tell a
/// flaky route from a wrong one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the model this machine's settings name"]
async fn command_mode_live_repeat() {
    let commands = std::env::var("TALVIO_LIVE_COMMANDS").unwrap_or_default();
    let repeat: usize = std::env::var("TALVIO_LIVE_REPEAT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(5);
    let settings = installed_settings();
    let llm = llm::router_for(&settings).expect("a usable router model");
    let context = CommandContext::capture();
    for command in commands.split('|').map(str::trim).filter(|c| !c.is_empty()) {
        for round in 1..=repeat {
            let plan = router::route(
                llm.as_ref(),
                Platform::current(),
                command,
                &context,
                &Followup::default(),
                tokio::time::Instant::now() + Duration::from_secs(40),
            )
            .await;
            let shown = match plan {
                Ok(plan) => plan
                    .calls
                    .iter()
                    .map(|call| format!("{} {}", call.name, call.args))
                    .collect::<Vec<_>>()
                    .join(" ; "),
                Err(e) => format!("ERROR {e:?}"),
            };
            println!("REPEAT {round} | {command} | {shown}");
        }
    }
}

/// A broad sweep of everyday commands, English and Nepali-English mixed, with
/// the tools an acceptable plan contains. Prints one row per command (the
/// plan, whether it matched, the latency) and fails if any command missed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the model this machine's settings name"]
async fn command_mode_live_routing_sweep() {
    let settings = installed_settings();
    let llm = llm::router_for(&settings).expect("a usable router model");
    println!("router: {}", llm.label());
    let context = CommandContext::capture();
    // Each entry: the command, then groups of tools of which the plan must
    // contain at least one per group.
    let cases: &[(&str, &[&[&str]])] = &[
        ("open youtube", &[&["open_app", "open_url"]]),
        ("open youtube and play believer", &[&["play_media"]]),
        ("play believer by imagine dragons", &[&["play_media"]]),
        ("play some lo-fi music", &[&["play_media"]]),
        ("play despacito on spotify", &[&["play_media"]]),
        ("turn the volume down", &[&["set_volume"]]),
        ("mute the sound", &[&["set_volume", "media_control"]]),
        ("set the volume to 30 percent", &[&["set_volume"]]),
        ("pause the music", &[&["media_control"]]),
        ("skip to the next song", &[&["media_control"]]),
        ("switch to chrome", &[&["switch_to_app"]]),
        ("go to github.com", &[&["open_url"]]),
        ("search for the weather in kathmandu", &[&["web_search"]]),
        ("google how to make momo", &[&["web_search"]]),
        (
            "send an email to Ron about the meeting at 9",
            &[&["compose_email"]],
        ),
        ("email ram that I will be late today", &[&["compose_email"]]),
        (
            "open notepad and type hello",
            &[&["open_app"], &["type_text"]],
        ),
        ("type thank you very much", &[&["type_text"]]),
        ("save this file", &[&["press_keys"]]),
        ("undo that", &[&["press_keys"]]),
        ("open my downloads folder", &[&["open_file", "open_app"]]),
        (
            "schedule a meeting with ram tomorrow at 3pm",
            &[&["create_calendar_event"]],
        ),
        ("what time is it", &[&["say"]]),
        ("what is ram's email address", &[&["lookup_contact", "say"]]),
        // Alt+F4 is refused by press_keys, so saying so is a fair answer.
        (
            "close this window",
            &[&["press_keys", "escalate_to_computer_use", "say"]],
        ),
        ("youtube ma believer bajau", &[&["play_media"]]),
        ("volume ali ghatau", &[&["set_volume"]]),
        ("chrome ma jau", &[&["switch_to_app"]]),
        ("gmail khola", &[&["open_app", "open_url"]]),
        ("kathmandu ko weather search gara", &[&["web_search"]]),
        (
            "notepad khola ani hello lekha",
            &[&["open_app"], &["type_text"]],
        ),
        ("gaana roka", &[&["media_control"]]),
        (
            "ram lai email pathau ki ma bholi aauchhu",
            &[&["compose_email"]],
        ),
        ("open spotify and play arijit singh", &[&["play_media"]]),
    ];
    let mut misses = Vec::new();
    let mut total = Duration::ZERO;
    for (command, groups) in cases {
        let started = Instant::now();
        let result = router::route(
            llm.as_ref(),
            Platform::current(),
            command,
            &context,
            &Followup::default(),
            tokio::time::Instant::now() + Duration::from_secs(40),
        )
        .await;
        // As the executor does: a joined command that stopped at an opener
        // is routed once more, told the opener ran.
        let result = match result {
            Ok(mut plan) => {
                let names: Vec<String> = plan.calls.iter().map(|c| c.name.clone()).collect();
                if super::wants_continuation(command, &names) {
                    let followup = Followup {
                        exchanges: Vec::new(),
                        already_done: plan
                            .calls
                            .iter()
                            .map(|c| format!("{}({})", c.name, c.args))
                            .collect(),
                    };
                    if let Ok(more) = router::route(
                        llm.as_ref(),
                        Platform::current(),
                        command,
                        &context,
                        &followup,
                        tokio::time::Instant::now() + Duration::from_secs(40),
                    )
                    .await
                    {
                        plan.calls.extend(
                            more.calls
                                .into_iter()
                                .filter(|c| c.name != router::SAY && c.name != names[0]),
                        );
                    }
                }
                Ok(plan)
            }
            Err(e) => Err(e),
        };
        let took = started.elapsed();
        total += took;
        let (calls, ok) = match result {
            Ok(plan) => {
                let names: Vec<&str> = plan.calls.iter().map(|c| c.name.as_str()).collect();
                let ok = groups
                    .iter()
                    .all(|group| group.iter().any(|tool| names.contains(tool)));
                let shown: Vec<String> = plan
                    .calls
                    .iter()
                    .map(|call| format!("{} {}", call.name, call.args))
                    .collect();
                if !ok && !plan.text.trim().is_empty() {
                    println!("  model said: {}", plan.text.trim());
                }
                (shown.join(" ; "), ok)
            }
            Err(e) => (format!("ERROR {e:?}"), false),
        };
        println!(
            "ROW | {command} | {calls} | {} | {:.2}s",
            if ok { "ok" } else { "MISS" },
            took.as_secs_f64()
        );
        if !ok {
            misses.push(*command);
        }
    }
    println!(
        "SUMMARY {}/{} ok, mean {:.2}s",
        cases.len() - misses.len(),
        cases.len(),
        total.as_secs_f64() / cases.len() as f64
    );
    assert!(misses.is_empty(), "missed: {misses:?}");
}

/// Print the buttons and fields the accessibility tree reads in the first
/// window whose title contains `TALVIO_LIVE_WINDOW`, three reads a second
/// apart (a Chromium browser fills its tree in after the first request).
#[cfg(target_os = "windows")]
#[test]
#[ignore = "reads a window on this computer"]
fn command_mode_live_dump_window() {
    use super::agent;
    use super::effects::{Effects, LiveEffects};
    let wanted = std::env::var("TALVIO_LIVE_WINDOW").expect("TALVIO_LIVE_WINDOW");
    let window = LiveEffects
        .open_windows()
        .into_iter()
        .find(|w| w.title.contains(&wanted))
        .expect("a window with that title");
    let tree = agent::tree();
    for read in 1..=3 {
        match tree.snapshot(Some(window.id)) {
            Ok(snapshot) => {
                println!("READ {read}: {} elements", snapshot.elements.len());
                for element in snapshot
                    .elements
                    .iter()
                    .filter(|e| matches!(e.role.as_str(), "button" | "edit" | "document"))
                {
                    println!(
                        "  {} {} {:?} {:?}",
                        element.id, element.role, element.name, element.states
                    );
                }
            }
            Err(e) => println!("READ {read}: {e}"),
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Try the ways of sending the Gmail draft open in the window whose title
/// contains `TALVIO_LIVE_WINDOW`, one at a time, printing the title after
/// each: the keyboard into the message body then Ctrl+Enter, then the Send
/// button. For a draft addressed to the tester only.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "sends the Gmail draft open on this computer"]
fn command_mode_live_try_send() {
    use super::agent::{self, tree::UiAction};
    use super::effects::{Effects, KeyCombo, LiveEffects};
    use super::registry::Modifier;
    let wanted = std::env::var("TALVIO_LIVE_WINDOW").expect("TALVIO_LIVE_WINDOW");
    let effects = LiveEffects;
    let window = effects
        .open_windows()
        .into_iter()
        .find(|w| w.title.contains(&wanted))
        .expect("a window with that title");
    let title = |label: &str| {
        let now = effects
            .open_windows()
            .into_iter()
            .find(|w| w.id == window.id)
            .map(|w| w.title)
            .unwrap_or_else(|| "(closed)".into());
        println!("{label}: {now}");
        now
    };
    effects.focus_window(&window).expect("focus");
    std::thread::sleep(Duration::from_millis(600));
    let tree = agent::tree();
    let snapshot = tree.snapshot(Some(window.id)).expect("tree");
    let body = snapshot
        .elements
        .iter()
        .find(|e| e.role == "edit" && e.name.to_lowercase().starts_with("message body"))
        .expect("message body");
    println!("focus body: {:?}", tree.act(body.id, &UiAction::Focus));
    std::thread::sleep(Duration::from_millis(400));
    let ctrl_enter = KeyCombo {
        modifiers: vec![Modifier::Ctrl],
        key: "enter".into(),
    };
    println!("ctrl+enter: {:?}", effects.press_keys(&ctrl_enter));
    for _ in 0..8 {
        std::thread::sleep(Duration::from_secs(1));
        if !title("after ctrl+enter").starts_with("Compose Mail") {
            return;
        }
    }
    println!(
        "send button: {:?}",
        super::effects::press_button_in_tree(tree.as_ref(), window.id, "send")
    );
    for _ in 0..8 {
        std::thread::sleep(Duration::from_secs(1));
        if !title("after the button").starts_with("Compose Mail") {
            return;
        }
    }
}

/// Whether a command's irreversible step waits: the router's flag on the
/// email, or the wording backstop, against what the phrasing asks for. Only
/// the plan is asked for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the model this machine's settings name"]
async fn command_mode_live_review_phrasing() {
    use super::review;
    let settings = installed_settings();
    let llm = llm::router_for(&settings).expect("a usable router model");
    let context = CommandContext::capture();
    let cases: &[(&str, bool)] = &[
        ("email ron that I'm running late", false),
        ("send an email to ron about the budget meeting", false),
        ("ram lai email pathau ki ma bholi aauchhu", false),
        (
            "write an email to ron about the budget but let me check it before you send it",
            true,
        ),
        ("draft an email to ron about lunch tomorrow", true),
        ("ram lai email lekha tara napathau", true),
        (
            "email sita that the report is ready, ask me before sending",
            true,
        ),
        (
            "email ron about the trip and wait for me before sending",
            true,
        ),
    ];
    let mut wrong = Vec::new();
    for (command, waits) in cases {
        let plan = router::route(
            llm.as_ref(),
            Platform::current(),
            command,
            &context,
            &Followup::default(),
            tokio::time::Instant::now() + Duration::from_secs(40),
        )
        .await
        .unwrap_or_else(|e| panic!("{command}: {e:?}"));
        let email = plan.calls.iter().find(|c| c.name == "compose_email");
        let flagged = email.is_some_and(|c| review::call_wants_review(&c.args));
        let worded = review::asks_to_review(command);
        let decided = flagged || worded;
        println!(
            "REVIEW {command:?}: router={flagged} words={worded} -> waits={decided} (want {waits})"
        );
        if email.is_none() || decided != *waits {
            wrong.push(*command);
        }
    }
    assert!(wrong.is_empty(), "wrong: {wrong:?}");
}

/// Carry out each command in `TALVIO_LIVE_RUN` (separated by `|`) on this
/// computer, through the real executor and the real effects, and print what it
/// did. An email is refused unless every recipient is `TALVIO_LIVE_OWN_EMAIL`,
/// and an email command handed to the screen agent is refused too, so nothing
/// can reach anyone else.
#[cfg(target_os = "windows")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "carries the commands out on this computer"]
async fn command_mode_live_carry_out() {
    use super::contacts::ContactBook;
    use super::effects::LiveEffects;
    use super::registry::{arg_str, arg_strings, ToolCtx};
    use super::{CommandModeState, CommandOrigin, CommandReport, Scope};
    use std::sync::Arc;

    let commands = std::env::var("TALVIO_LIVE_RUN").expect("TALVIO_LIVE_RUN");
    let own = std::env::var("TALVIO_LIVE_OWN_EMAIL")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let settings = installed_settings();
    let llm = llm::router_for(&settings).expect("a usable router model");
    println!("router: {}", llm.label());
    let state = CommandModeState::default();
    for command in commands.split('|').map(str::trim).filter(|c| !c.is_empty()) {
        let started = Instant::now();
        let id = state.begin();
        let context = CommandContext::capture();
        let followup = Followup::default();
        let routed = router::route(
            llm.as_ref(),
            Platform::current(),
            command,
            &context,
            &followup,
            tokio::time::Instant::now() + Duration::from_secs(40),
        )
        .await;
        if let Ok(plan) = &routed {
            let unsafe_email = plan.calls.iter().any(|call| {
                let emailing = command.to_lowercase().contains("mail");
                match call.name.as_str() {
                    "compose_email" => {
                        let mut to: Vec<String> = arg_str(&call.args, "to")
                            .unwrap_or("")
                            .split([',', ';'])
                            .map(|s| s.trim().to_lowercase())
                            .collect();
                        to.extend(
                            arg_strings(&call.args, "cc")
                                .iter()
                                .map(|s| s.to_lowercase()),
                        );
                        own.is_empty() || to.iter().any(|addr| *addr != own)
                    }
                    router::ESCALATE => emailing,
                    _ => false,
                }
            });
            if unsafe_email {
                println!("RUN {command:?}: REFUSED (email not only to the test address)");
                continue;
            }
        }
        let scope = Scope {
            state: &state,
            ctx: Arc::new(ToolCtx {
                settings: settings.clone(),
                context,
                effects: Arc::new(LiveEffects),
                contacts: Arc::new(ContactBook::default()),
                platform: Platform::current(),
                dry_run: false,
            }),
            dry: None,
            origin: CommandOrigin::Typed,
            transcript: command.to_string(),
            followup,
        };
        let report = CommandReport::new(id, CommandOrigin::Typed, command, false);
        let report = super::finish(
            &scope,
            report,
            Some(llm.as_ref()),
            llm.label(),
            routed,
            started,
        )
        .await;
        for step in &report.steps {
            println!(
                "  STEP {} {} -> {} {}",
                step.tool, step.args_json, step.result, step.detail
            );
        }
        let said: Vec<String> = report
            .feedback
            .iter()
            .map(|f| f.render_en().to_string())
            .collect();
        println!(
            "RUN {command:?}: {:?} in {:.1}s, {} call(s) | {}",
            report.status,
            report.latency_ms as f64 / 1000.0,
            report.llm_calls,
            said.join(" / ")
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
