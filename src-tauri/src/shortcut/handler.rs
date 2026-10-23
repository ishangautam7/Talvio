//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::warn;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::{command_binding_active, get_settings, AppSettings, COMMAND_BINDING_ID};
use crate::transcription_coordinator::{is_transcribe_binding, recording_mode};
use crate::TranscriptionCoordinator;

/// Handle a shortcut event from either implementation.
///
/// This function contains the shared logic for:
/// - Looking up the action in ACTION_MAP
/// - Handling the cancel binding (only fires when recording)
/// - Routing transcribe/assistant bindings to the coordinator, resolving the
///   recording mode (push-to-talk hold vs tap to toggle) from the setting
///
/// # Arguments
/// * `app` - The Tauri app handle
/// * `binding_id` - The ID of the binding (e.g., "transcribe", "cancel")
/// * `hotkey_string` - The string representation of the hotkey
/// * `is_pressed` - Whether this is a key press (true) or release (false)
pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    let base_id = binding_id;

    // The hands-free key on top of the dictation keys (`hands_free`). Only
    // the press means anything: the release of the dictation keys is what
    // matters to a held dictation, and they are released after it either way.
    if let Some(dictation) = crate::hands_free::dictation_of(base_id) {
        if is_pressed {
            if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
                coordinator.send_hands_free(dictation);
            }
        }
        return;
    }

    // The assistant or command mode is switched off: its hotkeys do nothing.
    // They are also unregistered at the OS level when the setting changes, so
    // this is the belt-and-braces path for a shortcut that was already in
    // flight.
    if switched_off(&get_settings(app), base_id) {
        return;
    }

    // Transcribe/assistant bindings are handled by the coordinator.
    if is_transcribe_binding(base_id) {
        // The quick ask ends a running call, since it takes the window over, but
        // it does so itself, once its press has settled (`AssistantAction`):
        // ending it here, on the press, meant the call key could never hang up
        // a call, because its first two keys are the ask's. Dictation does not
        // end a call at all: it holds the call's microphone while it records
        // (see `voice_conversation::DictationTracker`).
        if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
            // Dictation (with or without cleanup) and the ask are each held or
            // tapped on their own setting (`settings::holds_to_record`):
            //   • Hold → hold the shortcut to record, release to stop.
            //   • Tap  → tap once to start, tap again to stop.
            let holds = crate::settings::holds_to_record(&get_settings(app), base_id);
            let mode = recording_mode(holds);
            coordinator.send_input(base_id, hotkey_string, is_pressed, mode);
        } else {
            warn!("TranscriptionCoordinator is not initialized");
        }
        return;
    }

    let Some(action) = ACTION_MAP.get(base_id) else {
        warn!(
            "No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', Pressed: {}",
            base_id, hotkey_string, is_pressed
        );
        return;
    };

    // Cancel binding: fires while recording, while the assistant is generating
    // an answer, OR while Flow is starting/generating, so Esc can stop every
    // long-running voice operation after recording ends. Only on key-press.
    if base_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        let assistant_busy = app
            .try_state::<crate::assistant::AssistantConversation>()
            .map_or(false, |c| c.is_busy());
        let flow_busy = crate::flow::is_generation_active();
        // A spoken command being transcribed or carried out: Esc stops it.
        let command_busy = crate::command_mode::voice::is_busy();
        if is_pressed {
            // Esc during a call: stop the reply if there is one, hang up if
            // there isn't — unless a dictation is running beside the call, in
            // which case Esc is the dictation's and the call is left alone.
            //
            // It used to hang up unconditionally, which is the worst of both.
            // Mid-answer it threw away the thing the user was listening to, and
            // either way it ended the session while leaving the window on
            // screen — so the panel re-rendered as the quick-ask card and the
            // call had silently died behind it. A hang-up now goes through
            // `hide_assistant_panel` (which ends the session itself), so the
            // surface always leaves with the call.
            if crate::voice_conversation::esc_hangs_up(
                crate::voice_conversation::is_active(app),
                assistant_busy,
                crate::voice_conversation::dictation_in_flight(),
            ) {
                crate::assistant::hide_assistant_panel(app);
            }
            if cancel_has_work(
                audio_manager.is_recording(),
                assistant_busy,
                flow_busy,
                command_busy,
            ) {
                action.start(app, base_id, hotkey_string);
            }
        }
        return;
    }

    // Remaining bindings (e.g. "test") use simple start/stop on press/release.
    if is_pressed {
        action.start(app, base_id, hotkey_string);
    } else {
        action.stop(app, base_id, hotkey_string);
    }
}

/// Whether `binding_id` belongs to a feature that is switched off, so its
/// press does nothing: the assistant's two shortcuts while the assistant is
/// off, and the command shortcut while command mode is off.
fn switched_off(settings: &AppSettings, binding_id: &str) -> bool {
    (crate::assistant::is_assistant_binding(binding_id) && !settings.assistant_enabled)
        || (binding_id == COMMAND_BINDING_ID && !command_binding_active(settings))
}

/// Whether Esc has something to stop: a recording, an assistant reply, a
/// Flow generation, or a spoken command being transcribed or carried out.
fn cancel_has_work(
    recording: bool,
    assistant_busy: bool,
    flow_busy: bool,
    command_busy: bool,
) -> bool {
    recording || assistant_busy || flow_busy || command_busy
}

/// A modifier-only shortcut that fired a moment ago was withdrawn by the
/// hotkey engine: another key followed, so the modifiers were the start of a
/// different shortcut (`Ctrl+Win+→` to switch desktops while dictation is
/// `Ctrl+Win`). Throw away the recording that press started.
///
/// Only recording shortcuts can be withdrawn this way in practice — every other
/// action ships on a combo with a main key, which the engine never withdraws —
/// so anything else has nothing to undo.
pub fn handle_shortcut_cancelled(app: &AppHandle, binding_id: &str) {
    if !is_transcribe_binding(binding_id) {
        return;
    }
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_abort(binding_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    /// The command shortcut does nothing while command mode is off, and the
    /// assistant's switch does not decide it.
    #[test]
    fn the_command_key_is_off_with_command_mode() {
        let mut settings = get_default_settings();
        settings.assistant_enabled = true;
        settings.command_mode_enabled = false;
        assert!(switched_off(&settings, COMMAND_BINDING_ID));
        settings.command_mode_enabled = true;
        assert!(!switched_off(&settings, COMMAND_BINDING_ID));
        settings.assistant_enabled = false;
        assert!(!switched_off(&settings, COMMAND_BINDING_ID));
        assert!(switched_off(&settings, "assistant"));
        assert!(switched_off(&settings, "assistant_call"));
        // Dictation is never switched off here.
        assert!(!switched_off(&settings, "transcribe"));
    }

    /// Esc stops a spoken command after its recording has ended, while it is
    /// transcribed or carried out, and nothing at all when nothing runs.
    #[test]
    fn esc_has_a_running_command_to_stop() {
        assert!(cancel_has_work(false, false, false, true));
        assert!(cancel_has_work(true, false, false, false));
        assert!(!cancel_has_work(false, false, false, false));
    }

    /// The command shortcut goes to the coordinator, so it records like the
    /// ask and is never handed to `ACTION_MAP` directly on a press.
    #[test]
    fn the_command_key_goes_to_the_coordinator() {
        assert!(is_transcribe_binding(COMMAND_BINDING_ID));
        assert!(ACTION_MAP.contains_key(COMMAND_BINDING_ID));
    }
}
