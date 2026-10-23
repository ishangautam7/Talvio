//! `set_volume`: turn the computer's output volume up, down, off or to a level.
//!
//! Goes through [`Effects::volume`], which on the real system reuses the
//! output-volume code the audio ducking already has (the default output
//! device's endpoint on Windows), so command mode and ducking agree on which
//! device "the volume" is.
//!
//! Relative changes are in steps rather than percent because that is how
//! people say it ("a bit louder", "turn it down a couple") and how the volume
//! keys move; an absolute level is `set` with a percent. `set` without a
//! percent is refused rather than asked about: it means the model misread the
//! request, and the repair round that follows a failure fixes it faster than a
//! question to the user would.
//!
//! [`Effects::volume`]: crate::command_mode::effects::Effects::volume

use serde_json::{json, Value};

use crate::command_mode::effects::VolumeAction;
use crate::command_mode::registry::{
    arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS,
};

/// Steps for a bare "turn it up": noticeable, not startling.
const DEFAULT_STEPS: i64 = 2;
const MAX_STEPS: i64 = 10;

pub static SPEC: ToolSpec = ToolSpec {
    name: "set_volume",
    description: "Change the computer's output volume. \"Louder\" / \"quieter\" are up / down by \
                  steps (each step is one press of a volume key; default 2). \"Set it to 30\" or \
                  \"volume 30 percent\" is action set with percent 30. Mute and unmute toggle \
                  the sound without changing the level.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &[
        "volume", "louder", "quieter", "softer", "mute", "unmute", "sound", "turn up",
        "turn down", "loud", "quiet", "silence",
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
                "enum": ["up", "down", "mute", "unmute", "set"],
                "description": "up/down change it by steps; set goes to an exact percent."
            },
            "percent": {
                "type": "integer",
                "minimum": 0,
                "maximum": 100,
                "description": "The level for action set, 0-100. Required for set, ignored otherwise."
            },
            "steps": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_STEPS,
                "description": "How many volume-key steps for up/down. Defaults to 2."
            }
        },
        "required": ["action"],
        "additionalProperties": false
    })
}

/// An integer argument, accepting a whole-number float ("30.0") the way the
/// schema validator does; `arg_i64` alone would read that as absent.
fn whole_number(args: &Value, key: &str) -> Option<i64> {
    let value = args.get(key)?;
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.is_finite() && f.fract() == 0.0)
            .map(|f| f as i64)
    })
}

/// Turn the arguments into a [`VolumeAction`], or say what is wrong with them.
pub(crate) fn volume_action(args: &Value) -> Result<VolumeAction, String> {
    let steps = || -> Result<u8, String> {
        match whole_number(args, "steps") {
            None => Ok(DEFAULT_STEPS as u8),
            Some(n) if (1..=MAX_STEPS).contains(&n) => Ok(n as u8),
            Some(n) => Err(format!(
                "Can't change the volume by {n} steps; use 1 to {MAX_STEPS}."
            )),
        }
    };
    match arg_str(args, "action").unwrap_or_default() {
        "up" => steps().map(VolumeAction::Up),
        "down" => steps().map(VolumeAction::Down),
        "mute" => Ok(VolumeAction::Mute),
        "unmute" => Ok(VolumeAction::Unmute),
        "set" => match whole_number(args, "percent") {
            Some(p) if (0..=100).contains(&p) => Ok(VolumeAction::Set(p as u8)),
            Some(p) => Err(format!(
                "{p}% is not a volume level; use a percent from 0 to 100."
            )),
            None => Err("Setting the volume needs a percent from 0 to 100.".to_string()),
        },
        _ => Err("The volume action has to be up, down, mute, unmute or set.".to_string()),
    }
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let action = match volume_action(&args) {
        Ok(action) => action,
        Err(e) => return ToolOutcome::failed(e, false),
    };
    if let Err(e) = ctx.effects.volume(action) {
        log::warn!("command mode: volume {action:?} failed: {e}");
        return ToolOutcome::failed(e, false);
    }
    match action {
        VolumeAction::Up(_) => ToolOutcome::done("volumeUp", "Turned the volume up.", &[]),
        VolumeAction::Down(_) => ToolOutcome::done("volumeDown", "Turned the volume down.", &[]),
        VolumeAction::Mute => ToolOutcome::done("volumeMute", "Sound muted.", &[]),
        VolumeAction::Unmute => ToolOutcome::done("volumeUnmute", "Sound back on.", &[]),
        VolumeAction::Set(level) => {
            let percent = level.to_string();
            ToolOutcome::done(
                "volumeSet",
                "Volume set to {{percent}}%.",
                &[("percent", percent.as_str())],
            )
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

    /// The argument shapes a model sends land on the right action.
    #[test]
    fn arguments_map_to_volume_actions() {
        let cases = [
            (json!({"action": "up"}), VolumeAction::Up(2)),
            (json!({"action": "down", "steps": 5}), VolumeAction::Down(5)),
            (json!({"action": "up", "steps": 10.0}), VolumeAction::Up(10)),
            (json!({"action": "mute"}), VolumeAction::Mute),
            (json!({"action": "unmute"}), VolumeAction::Unmute),
            (json!({"action": "set", "percent": 30}), VolumeAction::Set(30)),
            (json!({"action": "set", "percent": 0}), VolumeAction::Set(0)),
            (json!({"action": "set", "percent": 100}), VolumeAction::Set(100)),
            // A percent on a relative change is ignored, not an error.
            (json!({"action": "up", "percent": 20}), VolumeAction::Up(2)),
        ];
        for (args, expected) in cases {
            assert!(validate_args(&SPEC, &args).is_ok(), "{args}");
            assert_eq!(volume_action(&args), Ok(expected), "{args}");
        }
    }

    /// "Set it" with no level is a misread request, refused with a reason the
    /// model can repair from.
    #[test]
    fn set_without_a_percent_is_refused() {
        let args = json!({"action": "set"});
        // The schema cannot say "required when set", so the handler does.
        assert!(validate_args(&SPEC, &args).is_ok());
        let fake = Arc::new(FakeEffects::default());
        match run(&ctx(fake.clone()), args) {
            ToolOutcome::Failed { reason, escalate } => {
                assert!(reason.contains("percent"), "{reason}");
                assert!(!escalate);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(fake.actions().is_empty());
    }

    /// Out-of-range values are refused by the schema and again by the handler.
    #[test]
    fn out_of_range_values_are_refused() {
        for args in [
            json!({"action": "set", "percent": 101}),
            json!({"action": "set", "percent": -5}),
            json!({"action": "up", "steps": 0}),
            json!({"action": "down", "steps": 11}),
        ] {
            assert!(validate_args(&SPEC, &args).is_err(), "schema took {args}");
            assert!(volume_action(&args).is_err(), "handler took {args}");
        }
        assert!(validate_args(&SPEC, &json!({"action": "louder"})).is_err());
        assert!(volume_action(&json!({"action": "louder"})).is_err());
    }

    #[test]
    fn each_action_reaches_the_system_and_says_what_it_did() {
        let cases = [
            (json!({"action": "up", "steps": 3}), "volume Up(3)", "Turned the volume up."),
            (json!({"action": "down"}), "volume Down(2)", "Turned the volume down."),
            (json!({"action": "mute"}), "volume Mute", "Sound muted."),
            (json!({"action": "unmute"}), "volume Unmute", "Sound back on."),
            (
                json!({"action": "set", "percent": 30}),
                "volume Set(30)",
                "Volume set to 30%.",
            ),
        ];
        for (args, line, text) in cases {
            let fake = Arc::new(FakeEffects::default());
            match run(&ctx(fake.clone()), args) {
                ToolOutcome::Done { feedback } => assert_eq!(feedback.render_en(), text),
                other => panic!("expected Done, got {other:?}"),
            }
            assert_eq!(fake.actions(), vec![line.to_string()]);
        }
    }

    #[test]
    fn a_volume_failure_is_reported() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("No output device.".into()),
            ..Default::default()
        });
        assert_eq!(
            run(&ctx(fake), json!({"action": "mute"})),
            ToolOutcome::failed("No output device.", false)
        );
    }

    #[test]
    fn volume_is_offered_on_every_platform() {
        assert!(SPEC.supports(Platform::LinuxWayland));
        assert!(SPEC.supports(Platform::MacOs));
    }
}
