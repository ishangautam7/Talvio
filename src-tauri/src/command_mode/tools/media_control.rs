//! `media_control`: pause, resume and skip whatever is playing.
//!
//! Pause and resume go to the player itself where the system lists players
//! ([`Effects::pause_media`], Windows' media sessions): the play/pause key
//! only toggles, so "pause" said while nothing played used to start
//! something. Skipping, and pausing where there is no session list, are the
//! system media keys ([`Effects::media`]), which reach Spotify, a YouTube tab
//! or a podcast app alike without knowing which one is playing, and work
//! without focus, which is why the tool is offered on Wayland too.
//!
//! [`Effects::media`]: crate::command_mode::effects::Effects::media
//! [`Effects::pause_media`]: crate::command_mode::effects::Effects::pause_media

use serde_json::{json, Value};

use crate::command_mode::effects::MediaAction;
use crate::command_mode::registry::{
    arg_str, Feedback, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS,
};

pub static SPEC: ToolSpec = ToolSpec {
    name: "media_control",
    description: "Control whatever music or video is already playing: pause it, resume it, \
                  skip to the next or previous track, or stop. Works on any player, without \
                  switching to it. To start something new by name, use play_media instead.",
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
                "enum": ["pause", "resume", "play_pause", "next", "previous", "stop"],
                "description": "pause or resume what is playing; play_pause only when the user \
                                says to toggle it."
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
    match requested {
        "pause" => return pause(ctx),
        "resume" => return resume(ctx),
        _ => {}
    }
    let Some((action, feedback)) = action_named(requested) else {
        return ToolOutcome::failed(
            "The media action has to be pause, resume, play_pause, next, previous or stop.",
            false,
        );
    };
    press(ctx, action, feedback)
}

/// Pause what plays, by the player's own pause. Nothing playing is said as
/// much, rather than toggled into playing. Without a session list, the key.
fn pause(ctx: &ToolCtx) -> ToolOutcome {
    match ctx.effects.pause_media() {
        Ok(0) => ToolOutcome::done("mediaNothingPlaying", "Nothing is playing.", &[]),
        Ok(_) => ToolOutcome::done("mediaPaused", "Paused.", &[]),
        Err(e) => {
            log::debug!("command mode: pausing by session failed ({e}); pressing the media key");
            press(
                ctx,
                MediaAction::PlayPause,
                Feedback::new("mediaPlayPause", "Play/pause pressed.", &[]),
            )
        }
    }
}

/// Resume the player by its own play. Something already playing is left
/// alone; a player that cannot be asked gets the key.
fn resume(ctx: &ToolCtx) -> ToolOutcome {
    if ctx.effects.media_playing() == Some(true) {
        return ToolOutcome::done("mediaAlreadyPlaying", "It's already playing.", &[]);
    }
    match ctx.effects.resume_media() {
        Ok(true) => ToolOutcome::done("mediaResumed", "Resumed.", &[]),
        Ok(false) | Err(_) => press(
            ctx,
            MediaAction::PlayPause,
            Feedback::new("mediaPlayPause", "Play/pause pressed.", &[]),
        ),
    }
}

fn press(ctx: &ToolCtx, action: MediaAction, feedback: Feedback) -> ToolOutcome {
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

    fn with_sessions(playing: Option<bool>) -> Arc<FakeEffects> {
        let fake = FakeEffects::default();
        *fake.playing.lock().unwrap() = playing;
        Arc::new(fake)
    }

    fn key_of(outcome: ToolOutcome) -> String {
        match outcome {
            ToolOutcome::Done { feedback } => feedback.key,
            other => panic!("expected Done, got {other:?}"),
        }
    }

    /// "Pause" pauses the player that plays, and never toggles silence into
    /// playing.
    #[test]
    fn pause_pauses_what_plays_and_never_starts_anything() {
        let fake = with_sessions(Some(true));
        let key = key_of(run(&ctx(fake.clone()), json!({"action": "pause"})));
        assert_eq!(key, "commandMode.feedback.mediaPaused");
        assert_eq!(fake.actions(), vec!["pause_media".to_string()]);

        let fake = with_sessions(Some(false));
        let key = key_of(run(&ctx(fake.clone()), json!({"action": "pause"})));
        assert_eq!(key, "commandMode.feedback.mediaNothingPlaying");
        assert!(!fake.actions().iter().any(|a| a.starts_with("media ")));
    }

    /// Where the system lists no players, pause is the media key.
    #[test]
    fn without_sessions_pause_and_resume_press_the_key() {
        for action in ["pause", "resume"] {
            let fake = with_sessions(None);
            let key = key_of(run(&ctx(fake.clone()), json!({ "action": action })));
            assert_eq!(key, "commandMode.feedback.mediaPlayPause", "{action}");
            assert_eq!(fake.actions(), vec!["media PlayPause".to_string()]);
        }
    }

    #[test]
    fn resume_leaves_something_playing_alone() {
        let fake = with_sessions(Some(true));
        let key = key_of(run(&ctx(fake.clone()), json!({"action": "resume"})));
        assert_eq!(key, "commandMode.feedback.mediaAlreadyPlaying");
        assert!(fake.actions().is_empty());

        let fake = with_sessions(Some(false));
        let key = key_of(run(&ctx(fake.clone()), json!({"action": "resume"})));
        assert_eq!(key, "commandMode.feedback.mediaResumed");
        assert_eq!(fake.actions(), vec!["resume_media".to_string()]);
    }

    /// Media keys need no focus, so Wayland keeps them.
    #[test]
    fn media_control_is_offered_on_wayland() {
        assert!(SPEC.supports(Platform::LinuxWayland));
    }
}
