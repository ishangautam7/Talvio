use crate::TranscriptionCoordinator;
#[cfg(unix)]
use log::debug;
use log::{info, warn};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "macos")]
use signal_hook::consts::SIGUSR1;
#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;
#[cfg(unix)]
use std::thread;

/// Send a transcription input to the coordinator.
/// Used by signal handlers, CLI flags, and any other external trigger.
pub fn send_transcription_input(app: &AppHandle, binding_id: &str, source: &str) {
    // External triggers reach the coordinator directly, so they skip the gate in
    // `shortcut::handler`. Without this, `--toggle-assistant` would still record
    // and run a whole assistant turn — model, screen capture, spoken reply —
    // with the assistant switched off and no window to show any of it in.
    if crate::assistant::is_assistant_binding(binding_id)
        && !crate::settings::get_settings(app).assistant_enabled
    {
        warn!("Ignoring '{binding_id}' from {source}: the assistant is switched off");
        return;
    }
    // Same rule as the hotkey path (see `shortcut::handler`): an assistant
    // trigger must not hang up a live voice conversation that is already
    // listening — it surfaces the panel instead. Dictation leaves the call up
    // too: it holds the call's microphone while it records.
    if crate::voice_conversation::is_active(app)
        && crate::assistant::is_assistant_binding(binding_id)
    {
        crate::assistant::show_assistant_panel(app);
        return;
    }
    if let Some(c) = app.try_state::<TranscriptionCoordinator>() {
        // External triggers can't "hold", so they always run hands-free (lock).
        c.send_input(
            binding_id,
            source,
            true,
            crate::transcription_coordinator::RecordingMode::Lock,
        );
    } else {
        warn!("TranscriptionCoordinator not initialized");
    }
}

/// Start a conversation, or end the one in progress, from outside the app
/// (`--toggle-call`). A conversation is not a recording, so this goes to its
/// shortcut action, exactly as the call key does, rather than to the
/// transcription coordinator.
pub fn toggle_call(app: &AppHandle, source: &str) {
    if !crate::settings::get_settings(app).assistant_enabled {
        warn!("Ignoring the conversation toggle from {source}: the assistant is switched off");
        return;
    }
    match crate::actions::ACTION_MAP.get("assistant_call") {
        Some(action) => action.start(app, "assistant_call", source),
        None => warn!("No action for 'assistant_call'"),
    }
}

/// The event a command run from outside the main window reports on, so the
/// Settings → Debug command box can show it. Carries a `CommandReport`.
pub const COMMAND_REPORT_EVENT: &str = "command-mode-report";

/// Run a typed command-mode request from outside the app (`--run-command`).
///
/// Never blocks the caller: on Windows the single-instance callback runs in
/// the plugin's window procedure on the main thread, with the second instance
/// waiting in `SendMessageW`, and a command is an LLM call plus whatever the
/// tools do. Gated on the command-mode switch the way `--toggle-call` is on
/// the assistant's, because a script must not act in the user's apps with
/// the feature off. `--dry-run` forces a dry run; without it the saved
/// setting decides, since CLI flags never write settings.
///
/// A real run first waits for keyboard control (`input::EnigoState`), which
/// only exists once the main window has mounted and called
/// `initialize_enigo`. On a cold start (`talvio --run-command ...` with no
/// instance running) this function is called from setup, a second or more
/// before that, and a command that types or presses keys would otherwise fail
/// with "Keyboard control isn't ready yet" right after its router call. Rust
/// does not create it itself: `initialize_core_logic` leaves that to the
/// frontend so macOS asks for Accessibility only once onboarding is done. The
/// wait is bounded by `KEYBOARD_WAIT`; past it the command runs anyway and a
/// keyboard tool reports its own error, which is better than silently
/// dropping a command nothing will ever unblock (onboarding not finished).
pub fn run_command(app: &AppHandle, text: String, dry_run: bool, source: &str) {
    let text = text.trim().to_string();
    if text.is_empty() {
        warn!("Ignoring an empty command from {source}");
        return;
    }
    let settings = crate::settings::get_settings(app);
    if !settings.command_mode_enabled {
        warn!("Ignoring a command from {source}: command mode is switched off");
        return;
    }
    let app = app.clone();
    let dry_run_override = dry_run.then_some(true);
    // A dry run sends no keystrokes, so it has nothing to wait for.
    let needs_keyboard = !(dry_run || settings.command_dry_run);
    let source = source.to_string();
    tauri::async_runtime::spawn(async move {
        if needs_keyboard && app.try_state::<crate::input::EnigoState>().is_none() {
            let started = std::time::Instant::now();
            let ready = wait_until(KEYBOARD_WAIT, KEYBOARD_POLL, || {
                app.try_state::<crate::input::EnigoState>().is_some()
            })
            .await;
            if ready {
                info!(
                    "Command from {source} waited {} ms for keyboard control",
                    started.elapsed().as_millis()
                );
            } else {
                warn!(
                    "Keyboard control still not ready after {} s; running the command from {source} anyway",
                    KEYBOARD_WAIT.as_secs()
                );
            }
        }
        let report = crate::command_mode::run_command(
            &app,
            &text,
            crate::command_mode::CommandOrigin::Cli,
            dry_run_override,
            None,
        )
        .await;
        // `run_command` has already logged its one-line summary (status,
        // tools, tokens); the feedback text is not logged again, since it can
        // quote the user's own words.
        if let Err(e) = app.emit(COMMAND_REPORT_EVENT, &report) {
            warn!("Could not send the command report to the windows: {e}");
        }
    });
}

/// The longest a command waits for keyboard control before it runs anyway. A
/// cold start reaches it in a few seconds (webview load, then the frontend's
/// `initialize_enigo`); fifteen leaves room for a slow first launch without
/// leaving a script hanging when keyboard control will never come.
const KEYBOARD_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// How often that wait checks. Short enough to add no noticeable delay once
/// keyboard control appears; the check is a map lookup.
const KEYBOARD_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Wait until `ready` answers true, checking every `poll`, for at most
/// `limit`. Answers whether it became ready. Checked once more at the limit,
/// so something that arrived during the last sleep still counts.
async fn wait_until(
    limit: std::time::Duration,
    poll: std::time::Duration,
    mut ready: impl FnMut() -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + limit;
    loop {
        if ready() {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        tokio::time::sleep(poll.min(deadline - now)).await;
    }
}

/// Listen for Unix signals that remotely toggle transcription.
///
/// SIGUSR2 toggles plain transcription on every Unix platform. SIGUSR1
/// (transcription with post-processing) is only handled on macOS: on Linux,
/// WebKitGTK's JavaScriptCore garbage collector sends SIGUSR1 to its own
/// threads to suspend them, so handling it started or stopped a recording on
/// its own every few minutes (Handy #1660). Linux users trigger that binding
/// with `--toggle-post-process` instead. Backport of Handy #1824.
///
/// Registration failure is logged rather than fatal: the signals are a
/// convenience for scripts, and the app must still start without them.
#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle) {
    #[cfg(target_os = "macos")]
    let registered = Signals::new([SIGUSR1, SIGUSR2]);
    #[cfg(not(target_os = "macos"))]
    let registered = Signals::new([SIGUSR2]);
    let mut signals = match registered {
        Ok(signals) => signals,
        Err(e) => {
            warn!("Could not register transcription signal handlers: {e}");
            return;
        }
    };
    #[cfg(target_os = "macos")]
    debug!("Signal handlers registered (SIGUSR1, SIGUSR2)");
    #[cfg(not(target_os = "macos"))]
    debug!("Signal handler registered (SIGUSR2; SIGUSR1 is left to WebKitGTK)");
    thread::spawn(move || {
        // On macOS the transcription worker passes these signals on to the
        // app (it is also named `talvio`, so `pkill` reaches it), and a
        // `pkill` without `-n` signals both: the same toggle then arrives
        // twice within a millisecond and would start and stop at once. Nobody
        // toggles a recording twice inside this window on purpose.
        const DUPLICATE_WINDOW: std::time::Duration = std::time::Duration::from_millis(250);
        let mut last: Option<(i32, std::time::Instant)> = None;
        for sig in signals.forever() {
            let now = std::time::Instant::now();
            if last
                .is_some_and(|(seen, at)| seen == sig && now.duration_since(at) < DUPLICATE_WINDOW)
            {
                debug!("Ignoring a repeated signal {sig} (delivered to the app and its worker)");
                continue;
            }
            last = Some((sig, now));
            let (binding_id, signal_name) = match sig {
                #[cfg(target_os = "macos")]
                SIGUSR1 => ("transcribe_with_post_process", "SIGUSR1"),
                SIGUSR2 => ("transcribe", "SIGUSR2"),
                _ => continue,
            };
            debug!("Received {signal_name}");
            send_transcription_input(&app_handle, binding_id, signal_name);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::wait_until;
    use std::time::Duration;

    /// Something already ready costs no wait at all: the single-instance path
    /// on a running app must not be slowed by the cold-start wait.
    #[tokio::test]
    async fn something_already_ready_is_not_waited_for() {
        let started = std::time::Instant::now();
        assert!(wait_until(Duration::from_secs(5), Duration::from_millis(50), || true).await);
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    /// Keyboard control that appears during the wait ends it: the command
    /// runs as soon as it can type, not after the whole limit.
    #[tokio::test]
    async fn readiness_that_arrives_mid_wait_ends_the_wait() {
        let mut checks = 0;
        let started = std::time::Instant::now();
        let ready = wait_until(Duration::from_secs(5), Duration::from_millis(5), || {
            checks += 1;
            checks >= 3
        })
        .await;
        assert!(ready);
        assert_eq!(checks, 3);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// The wait is bounded: when keyboard control never comes (onboarding not
    /// finished), it gives up so the command still runs and reports why.
    #[tokio::test]
    async fn the_wait_gives_up_at_the_limit() {
        let started = std::time::Instant::now();
        assert!(
            !wait_until(Duration::from_millis(40), Duration::from_millis(10), || {
                false
            })
            .await
        );
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(40));
        assert!(waited < Duration::from_secs(1));
    }
}
