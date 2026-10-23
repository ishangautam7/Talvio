//! `type_text`: put literal text into whatever field has the keyboard.
//!
//! For the part of a command that is itself dictation ("search for flights to
//! Pokhara" after a `press_keys` that focused the search box, "type my address
//! is 12 Lakeside Road"). It goes through [`Effects::type_text`], which on the
//! real system is the existing paste path, so it works wherever dictation
//! works, Wayland included, and inherits dictation's clipboard care.
//!
//! The text is passed through untouched: no trimming beyond refusing a blank
//! call, because a leading space or a trailing newline may be exactly what the
//! user asked for. It is never logged, only its length, for the same reason a
//! dictation's text is not.
//!
//! [`Effects::type_text`]: crate::command_mode::effects::Effects::type_text

use serde_json::{json, Value};

use crate::command_mode::registry::{Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};

/// The most a single call may type. Long enough for a paragraph or an
/// address block, short enough that a runaway model cannot paste a novel.
const MAX_CHARS: u64 = 5000;

pub static SPEC: ToolSpec = ToolSpec {
    name: "type_text",
    description: "Type text into the field that currently has the keyboard focus, as if the user \
                  had typed it. Use it for literal words the user dictated as part of the command \
                  (\"type hello team, running late\"), not for composing emails or events, which \
                  have their own tools. The text is typed exactly as given.",
    schema,
    risk: Risk::Safe,
    // The paste path works on Wayland through the existing typing tools.
    platforms: ALL_PLATFORMS,
    keywords: &["type", "write", "enter text", "insert", "paste", "fill in"],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text": {
                "type": "string",
                "description": "The exact text to type, with the punctuation and capitalisation it should have.",
                "minLength": 1,
                "maxLength": MAX_CHARS
            }
        },
        "required": ["text"],
        "additionalProperties": false
    })
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    // Read raw, not through `arg_str`, which trims: whitespace at either end
    // can be part of what the user meant to type.
    let Some(text) = args.get("text").and_then(Value::as_str) else {
        return ToolOutcome::failed("There was no text to type.", false);
    };
    if text.trim().is_empty() {
        return ToolOutcome::failed("There was no text to type.", false);
    }
    let count = text.chars().count();
    if count as u64 > MAX_CHARS {
        return ToolOutcome::failed(
            format!("That is too much text to type at once (the limit is {MAX_CHARS} characters)."),
            false,
        );
    }

    match ctx.effects.type_text(text) {
        Ok(()) => {
            log::debug!("command mode: typed {count} characters");
            let count = count.to_string();
            ToolOutcome::done("typedText", "Typed it.", &[("count", count.as_str())])
        }
        Err(e) => {
            log::warn!("command mode: typing {count} characters failed: {e}");
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

    /// The text reaches the focused field exactly, spaces and all.
    #[test]
    fn the_text_is_typed_exactly_as_given() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(&ctx(fake.clone()), json!({"text": " Hello team, running late.\n"}));
        match outcome {
            ToolOutcome::Done { feedback } => {
                assert_eq!(feedback.render_en(), "Typed it.");
                assert_eq!(feedback.key, "commandMode.feedback.typedText");
                assert!(feedback
                    .params
                    .contains(&("count".to_string(), "27".to_string())));
            }
            other => panic!("expected Done, got {other:?}"),
        }
        assert_eq!(
            fake.actions(),
            vec!["type_text  Hello team, running late.\n".to_string()]
        );
    }

    /// A blank call would paste nothing and report success, which reads as a bug.
    #[test]
    fn blank_or_missing_text_is_refused_without_typing() {
        let fake = Arc::new(FakeEffects::default());
        for args in [json!({"text": "   "}), json!({})] {
            assert!(matches!(
                run(&ctx(fake.clone()), args),
                ToolOutcome::Failed { escalate: false, .. }
            ));
        }
        assert!(fake.actions().is_empty());
    }

    /// The handler holds the limit even if called without schema validation.
    #[test]
    fn text_over_the_limit_is_refused() {
        let fake = Arc::new(FakeEffects::default());
        let long = "x".repeat(MAX_CHARS as usize + 1);
        assert!(matches!(
            run(&ctx(fake.clone()), json!({"text": long})),
            ToolOutcome::Failed { .. }
        ));
        assert!(fake.actions().is_empty());
        assert!(validate_args(&SPEC, &json!({"text": "x".repeat(MAX_CHARS as usize + 1)})).is_err());
        assert!(validate_args(&SPEC, &json!({"text": "ok"})).is_ok());
    }

    /// The platform's own message is what the user sees when typing fails.
    #[test]
    fn a_typing_failure_is_reported_and_not_escalated() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("Could not type into that window.".into()),
            ..Default::default()
        });
        match run(&ctx(fake), json!({"text": "hi"})) {
            ToolOutcome::Failed { reason, escalate } => {
                assert_eq!(reason, "Could not type into that window.");
                assert!(!escalate);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// Typing is the one input tool that must survive Wayland.
    #[test]
    fn typing_is_offered_on_every_platform_including_wayland() {
        assert!(SPEC.supports(Platform::LinuxWayland));
        assert!(SPEC.supports(Platform::MacOs));
    }
}
