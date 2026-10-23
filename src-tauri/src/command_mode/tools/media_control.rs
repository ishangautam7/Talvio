//! `media_control`: play, pause and skip whatever is playing.
//!
//! Sent as the system media keys ([`Effects::media`]), so it reaches Spotify,
//! a YouTube tab or a podcast app alike without knowing which one is playing,
//! and works without focus, which is why it is offered on Wayland too.
//!
//! [`Effects::media`]: crate::command_mode::effects::Effects::media

use serde_json::{json, Value};

use crate::command_mode::effects::MediaAction;
use crate::command_mode::registry::{
    arg_str, Feedback, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS,
};

pub static SPEC: ToolSpec = ToolSpec {
    name: "media_control",
    description: "Control whatever music or video is playing, through the system media keys: \
                  play or pause, next track, previous track, or stop. Works on any player, \
                  without switching to it.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &[
        "play", "pause", "resume", "next", "skip", "previous", "back", "stop", "music", "song",
        "track", "spotify", "video", "podcast",
    ],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["play_pause", "next", "previous", "stop"],
                "description": "play_pause toggles (use it for play, pause and resume)."
            }
        },
        "required": ["action"],
        "additionalProperties": false
    })
}

/// The media key for an `action` argument, and what to tell the user after.
fn action_named(action: &str) -> Option<(MediaAction, Feedback)> {
    let (media, key, english) = match action {
        "play_pause" => (MediaAction::PlayPause, "mediaPlayPause", "Play/pause pressed."),
        "next" => (MediaAction::Next, "mediaNext", "Skipped to the next track."),
        "previous" => (
            MediaAction::Previous,
            "mediaPrevious",
            "Back to the previous track.",
        ),
        "stop" => (MediaAction::Stop, "mediaStop", "Stopped playback."),
        _ => return None,
    };
    Some((media, Feedback::new(key, english, &[])))
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let requested = arg_str(&args, "action").unwrap_or_default();
    let Some((action, feedback)) = action_named(requested) else {
        return ToolOutcome::failed(
            "The media action has to be play_pause, next, previous or stop.",
            false,
        );
    };
    match ctx.effects.media(action) {
        Ok(()) => ToolOutcome::Done { feedback },
        Err(e) => {
            log::warn!("command mode: media {action:?} failed: {e}");
            ToolOutcome::failed(e, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::registry::{validate_args, Platform};
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    fn ctx(effects: Arc<FakeEffects>) -> ToolCtx {
        let platform = Platform::Windows;
        ToolCtx {
            settings: crate::settings::get_default_settings(),
            context: CommandContext::at(
                Utc.with_ymd_and_hms(2026, 10, 22, 9, 0, 0).unwrap(),
                None,
                platform,
                None,
                None,
            ),
            effects,
            contacts: Arc::new(ContactBook::new(Vec::new())),
            platform,
            dry_run: false,
        }
    }

    /// Each schema value maps to its own media key and its own message.
    #[test]
    fn every_action_presses_its_media_key() {
        let cases = [
            ("play_pause", "media PlayPause", "commandMode.feedback.mediaPlayPause"),
            ("next", "media Next", "commandMode.feedback.mediaNext"),
            ("previous", "media Previous", "commandMode.feedback.mediaPrevious"),
            ("stop", "media Stop", "commandMode.feedback.mediaStop"),
        ];
        for (action, line, key) in cases {
            let fake = Arc::new(FakeEffects::default());
            let args = json!({ "action": action });
            assert!(validate_args(&SPEC, &args).is_ok(), "{action}");
            match run(&ctx(fake.clone()), args) {
                ToolOutcome::Done { feedback } => assert_eq!(feedback.key, key),
                other => panic!("{action}: expected Done, got {other:?}"),
            }
            assert_eq!(fake.actions(), vec![line.to_string()]);
        }
    }

    /// The schema and the handler agree on what an action can be.
    #[test]
    fn an_unknown_action_is_refused() {
        let args = json!({"action": "rewind"});
        assert!(validate_args(&SPEC, &args).is_err());
        let fake = Arc::new(FakeEffects::default());
        assert!(matches!(
            run(&ctx(fake.clone()), args),
            ToolOutcome::Failed { escalate: false, .. }
        ));
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn a_media_key_failure_is_reported() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("No media keys here.".into()),
            ..Default::default()
        });
        assert_eq!(
            run(&ctx(fake), json!({"action": "next"})),
            ToolOutcome::failed("No media keys here.", false)
        );
    }

    /// Media keys need no focus, so Wayland keeps them.
    #[test]
    fn media_control_is_offered_on_wayland() {
        assert!(SPEC.supports(Platform::LinuxWayland));
    }
}
