//! `do_on_computer`: the assistant doing something on the computer rather than
//! explaining how to.
//!
//! "Play Believer on YouTube", "open Gmail", "turn it down", "pause" are
//! requests command mode already carries out: its router model picks a tool
//! and the tool runs visibly in the user's own apps. This tool hands the
//! request to it ([`crate::command_mode::run_command`]) and gives the model the
//! report in a sentence it can relay. It is offered only while command mode
//! ("Voice commands") is on, because that switch is the user's consent to
//! having things done on their computer.
//!
//! A request is carried out whole, sending included (`command_mode::review`):
//! asking the assistant to email someone is asking for the email to go. It
//! waits only when the user asked to check it first, which the model passes on
//! in the request. The assistant can never confirm anything itself: there is
//! no confirm tool. A command that waits for "send" shows on the computer's command card
//! (`command_mode::voice::show_card`) with Do it and Cancel, and the model is
//! told to say it is waiting.

use log::{info, warn};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::command_mode::{CommandOrigin, CommandReport, CommandStatus};
use crate::settings::AppSettings;

pub const TOOL_NAME: &str = "do_on_computer";

/// Whether a turn offers `do_on_computer`: command mode is on. Never for the
/// Cat persona, which calls no model at all.
pub fn offered(settings: &AppSettings) -> bool {
    settings.command_mode_enabled && !settings.active_character_is_cat()
}

/// The tool, as the model sees it.
pub fn tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": "Carry out an action on the user's computer: open an app or website, play music or a video, pause, resume or skip media, change the volume, switch windows, type text, start an email or a calendar event, or search the web in the browser. Call it whenever the user asks for something to be done on their computer instead of explaining how to do it. Pass the request in plain words, with every detail they gave (names, titles, times). It carries the whole request out, sending an email or a message included. When the user asked to check it first (\"let me see it before you send\", \"just draft it\", \"ask before sending\"), say so in the request and it waits for them instead.",
            "parameters": {
                "type": "object",
                "properties": {
                    "request": {
                        "type": "string",
                        "description": "What to do, in plain words, as the user would say it to someone at the computer, e.g. 'play Believer by Imagine Dragons on YouTube' or 'open Gmail and start an email to Ron about the meeting at 9'."
                    }
                },
                "required": ["request"]
            }
        }
    })
}

/// When to use it, for the Tools section of the system prompt. Fixed text, so
/// the prompt prefix stays cache-stable.
pub const GUIDANCE: &str = "• do_on_computer(request): do something on the user's computer. When the user asks for an action (\"play X\", \"open Y\", \"turn it down\", \"pause\", \"skip this song\", \"email Ron about the meeting\"), call it in this same reply instead of explaining how they could do it, and pass the whole request with every detail they gave. Questions, writing, explanations and advice are still answered normally, without it. It carries the whole request out, sending included, so do not ask the user to confirm first; only when they asked to check or approve it first (\"let me see it before you send\", \"draft it\", \"napathau\") say that in the request, and it then waits for them on a card. Say something was sent or done only when the tool's result says so. Afterwards tell the user in one short sentence what happened, from the tool's result; on a call, one short spoken sentence. Write the request in English, but keep names, titles and any words to be typed or sent exactly as the user said them. Reply in the language and script the user wrote in: Nepali written in Latin letters (\"gaana bajau\", \"gmail khola\") is Nepali, so answer in Nepali, never in Hindi.\n";

/// The `request` argument, tolerating the other names models use for it.
pub fn parse_request(arguments: &str) -> String {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    ["request", "task", "command", "text"]
        .iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .map(|request| request.trim().to_string())
        .unwrap_or_default()
}

/// The command's status as the protocol and the phone name it.
pub fn status_code(status: CommandStatus) -> &'static str {
    match status {
        CommandStatus::Done => "done",
        CommandStatus::AwaitingConfirmation => "awaiting_confirmation",
        CommandStatus::NeedsAnswer => "needs_answer",
        CommandStatus::NeedsComputerUse => "needs_computer_use",
        CommandStatus::Cancelled => "cancelled",
        CommandStatus::Failed => "failed",
    }
}

/// Command mode is on but has no model it can use.
pub fn not_configured(report: &CommandReport) -> bool {
    report.status == CommandStatus::Failed && report.feedback.iter().any(|f| f.is("notConfigured"))
}

/// What the model is told about a command it ran: what happened, and how to
/// put it to the user. Pure, so every case is a test.
pub fn result_for_model(report: &CommandReport) -> String {
    let summary = report.summary();
    let dry = if report.dry_run {
        let actions = if report.dry_run_actions.is_empty() {
            "nothing".to_string()
        } else {
            report.dry_run_actions.join("; ")
        };
        format!(
            " This was a dry run (the Dry run setting under Commands is on), so nothing really \
             happened on the computer; say so. It would have done: {actions}."
        )
    } else {
        String::new()
    };
    match report.status {
        CommandStatus::Done => format!(
            "Done. What happened: {summary}{dry} Tell the user in one short sentence what you did."
        ),
        CommandStatus::AwaitingConfirmation => {
            let pending = report
                .pending_summary
                .clone()
                .unwrap_or_else(|| summary.clone());
            format!(
                "Prepared, and waiting for the user: {pending}. Nothing has been sent or done yet. \
                 A card on the computer shows it with Do it and Cancel. Tell the user in one short \
                 sentence that it is ready and waits for them to press Do it. Never say it was \
                 sent or done.{dry}"
            )
        }
        CommandStatus::NeedsAnswer => {
            let question = report.question.clone().unwrap_or_else(|| summary.clone());
            let choices = if report.choices.is_empty() {
                String::new()
            } else {
                format!(" Choices: {}.", report.choices.join(", "))
            };
            format!(
                "Not done yet: the computer needs an answer first: {question}{choices} Ask the user \
                 this in one short sentence. When they answer, call {TOOL_NAME} again with the \
                 whole request and their answer."
            )
        }
        CommandStatus::NeedsComputerUse => format!(
            "Not done: this cannot be done automatically on the computer yet. {summary} Tell the \
             user in one short sentence, and say how they can do it themselves if that is simple."
        ),
        CommandStatus::Cancelled => format!("Not done: {summary} Tell the user briefly."),
        CommandStatus::Failed if not_configured(report) => format!(
            "Not done: actions on the computer are not set up ({summary}). Tell the user to choose \
             a model for Commands in Talvio."
        ),
        CommandStatus::Failed => format!(
            "It did not work: {summary} Tell the user in one short sentence what went wrong. Do not \
             call {TOOL_NAME} again with the same request."
        ),
    }
}

/// Run one `do_on_computer` call: the request through command mode, a card on
/// the computer when it waits for the user, and the report for the windows
/// that show reports (the Commands page). `None` for a call with no request.
pub async fn perform(
    app: &AppHandle,
    arguments: &str,
    origin: CommandOrigin,
) -> (String, Option<CommandReport>) {
    let request = parse_request(arguments);
    if request.is_empty() {
        return (
            format!("No request was given to {TOOL_NAME}. Say what to do in plain words."),
            None,
        );
    }
    info!("Assistant asked the computer to act");
    let report = crate::command_mode::run_command(app, &request, origin, None, None).await;
    if let Err(e) = app.emit(crate::signal_handle::COMMAND_REPORT_EVENT, &report) {
        warn!("Could not send the command report to the windows: {e}");
    }
    if matches!(
        report.status,
        CommandStatus::AwaitingConfirmation | CommandStatus::NeedsAnswer
    ) {
        crate::command_mode::voice::show_card(app, report.clone());
    }
    (result_for_model(&report), Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::llm::Usage;
    use crate::command_mode::registry::Feedback;

    pub(crate) fn report(status: CommandStatus, feedback: &str) -> CommandReport {
        CommandReport {
            id: 7,
            origin: CommandOrigin::Assistant,
            transcript: "play believer on youtube".into(),
            status,
            feedback: vec![Feedback::new("done", feedback, &[])],
            steps: Vec::new(),
            pending_summary: None,
            question: None,
            choices: Vec::new(),
            escalation: None,
            dry_run: false,
            dry_run_actions: Vec::new(),
            model: String::new(),
            llm_calls: 1,
            usage: Usage::default(),
            latency_ms: 0,
            error: None,
        }
    }

    #[test]
    fn it_is_offered_only_with_command_mode_on() {
        let mut settings = crate::settings::get_default_settings();
        settings.command_mode_enabled = false;
        assert!(!offered(&settings));
        settings.command_mode_enabled = true;
        assert!(offered(&settings));
    }

    #[test]
    fn the_request_is_read_under_the_names_models_use() {
        assert_eq!(
            parse_request(r#"{"request":" open youtube "}"#),
            "open youtube"
        );
        assert_eq!(parse_request(r#"{"task":"pause"}"#), "pause");
        assert_eq!(parse_request("not json"), "");
        assert_eq!(parse_request("{}"), "");
    }

    #[test]
    fn a_done_command_is_reported_as_done() {
        let text = result_for_model(&report(CommandStatus::Done, "Playing Believer on YouTube."));
        assert!(text.starts_with("Done."));
        assert!(text.contains("Playing Believer on YouTube."));
        assert!(!text.contains("dry run"));
    }

    #[test]
    fn a_waiting_command_is_never_called_sent() {
        let mut waiting = report(CommandStatus::AwaitingConfirmation, "Draft ready.");
        waiting.pending_summary = Some("Send the email to Ron".into());
        let text = result_for_model(&waiting);
        assert!(text.contains("Send the email to Ron"));
        assert!(text.contains("Do it"));
        assert!(text.contains("Never say it was sent"));
    }

    #[test]
    fn a_question_is_passed_on_with_its_choices() {
        let mut asking = report(CommandStatus::NeedsAnswer, "Which Ron?");
        asking.question = Some("Which Ron?".into());
        asking.choices = vec!["Ron Weasley".into(), "Ron Swanson".into()];
        let text = result_for_model(&asking);
        assert!(text.contains("Which Ron?"));
        assert!(text.contains("Ron Weasley, Ron Swanson"));
        assert!(text.contains(TOOL_NAME));
    }

    #[test]
    fn failures_and_dry_runs_say_so() {
        let text = result_for_model(&report(CommandStatus::Failed, "No app called Spotfy."));
        assert!(text.starts_with("It did not work"));
        let mut setup = report(CommandStatus::Failed, "Command mode isn't set up: no key");
        // Built the way the executor builds it, namespace and all.
        setup.feedback = vec![Feedback::new(
            "notConfigured",
            "Command mode isn't set up: no key",
            &[],
        )];
        assert!(not_configured(&setup));
        assert!(result_for_model(&setup).contains("not set up"));

        let mut dry = report(CommandStatus::Done, "Played it.");
        dry.dry_run = true;
        dry.dry_run_actions = vec!["open https://youtube.com".into()];
        let text = result_for_model(&dry);
        assert!(text.contains("dry run"));
        assert!(text.contains("open https://youtube.com"));
    }

    #[test]
    fn the_guidance_has_no_em_dashes_and_names_the_tool() {
        assert!(GUIDANCE.contains(TOOL_NAME));
        assert!(!GUIDANCE.contains('\u{2014}'));
        assert!(GUIDANCE.contains("instead of explaining"));
        let description = tool_definition()["function"]["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!description.contains('\u{2014}'));
    }
}
