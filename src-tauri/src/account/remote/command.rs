//! `command`, `command.confirm` and `command.answer`: a phone telling this
//! computer what to do (`PROTOCOL.md`, "Commands"). The phone only says what it
//! wants, as text or as a recording this computer transcribes; command mode
//! picks the tools and they run here, as for a command spoken at the computer.

use std::sync::Arc;

use futures_util::future::BoxFuture;
use log::{debug, info};
use serde_json::{json, Map, Value};

use super::{dictate, Reply, Router};

const MAX_TEXT_CHARS: usize = 2_000;

/// What a command came to, as the phone is told it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    pub transcript: String,
    /// `done`, `awaiting_confirmation`, `needs_answer`, `cancelled`, `failed`
    /// or `needs_computer_use`.
    pub status: String,
    /// What happened, in English.
    pub feedback: String,
    /// What a confirmation or an answer refers to.
    pub id: Option<u32>,
    pub pending: Option<String>,
    pub question: Option<String>,
    pub choices: Vec<String>,
    /// Command mode is on but has no model it can use.
    pub not_configured: bool,
}

/// What the phone asks for.
pub enum Ask {
    Run(String),
    Confirm { id: u32, accept: bool },
    Answer { id: u32, answer: String },
}

/// Command mode, as phone requests reach it (`Host::command`).
pub type CommandFuture = BoxFuture<'static, CommandOutcome>;

/// `command`: run one, from text or from a recording.
pub(super) async fn run(router: Arc<Router>, body: Value, reply: Reply) {
    let text = match body.get("text") {
        Some(Value::String(text)) => Some(text.trim().to_string()),
        Some(_) => return reply.fail("unsupported"),
        None => None,
    };
    let audio = body.get("audio").and_then(Value::as_str);
    let text = match (text, audio) {
        (Some(text), None) if (1..=MAX_TEXT_CHARS).contains(&text.chars().count()) => text,
        (None, Some(audio)) => match transcribe(&router, &reply, audio).await {
            Ok(text) => text,
            Err(code) => return reply.fail(code),
        },
        _ => return reply.fail("unsupported"),
    };
    perform(&router, reply, Ask::Run(text)).await;
}

/// `command.confirm`: Do it or Cancel on what a command left waiting.
pub(super) async fn confirm(router: Arc<Router>, body: Value, reply: Reply) {
    let (Some(id), Some(accept)) = (
        body.get("id")
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok()),
        body.get("accept").and_then(Value::as_bool),
    ) else {
        return reply.fail("unsupported");
    };
    perform(&router, reply, Ask::Confirm { id, accept }).await;
}

/// `command.answer`: the answer to a command's question.
pub(super) async fn answer(router: Arc<Router>, body: Value, reply: Reply) {
    let (Some(id), Some(answer)) = (
        body.get("id")
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok()),
        body.get("answer")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|a| (1..=MAX_TEXT_CHARS).contains(&a.chars().count())),
    ) else {
        return reply.fail("unsupported");
    };
    let answer = answer.to_string();
    perform(&router, reply, Ask::Answer { id, answer }).await;
}

/// The checks every command request passes, then the command itself.
async fn perform(router: &Router, mut reply: Reply, ask: Ask) {
    let host = router.host();
    if !router.input_allowed() {
        return reply.fail("remote_input_off");
    }
    if !host.command_enabled() {
        return reply.fail("command_mode_off");
    }
    // One command at a time: a second would replace what the first left
    // waiting while the first was still working.
    if !reply.take_input() {
        return reply.fail("busy");
    }
    if reply.is_cancelled() {
        return reply.fail("cancelled");
    }
    reply.progress("working");
    let outcome = host.command(ask).await;
    if outcome.not_configured {
        return reply.fail("not_configured");
    }
    info!("Phone command: {}", outcome.status);
    reply.ok(fields(outcome));
}

async fn transcribe(router: &Router, reply: &Reply, audio: &str) -> Result<String, &'static str> {
    let host = router.host();
    if !router.input_allowed() {
        return Err("remote_input_off");
    }
    if !host.command_enabled() {
        return Err("command_mode_off");
    }
    if !host.speech_ready() {
        return Err("no_speech_model");
    }
    let (_, samples) = dictate::decode(audio).map_err(|why| {
        debug!("A phone command's recording was refused: {why}");
        "bad_audio"
    })?;
    reply.progress("transcribing");
    let text = host
        .transcribe(samples)
        .await
        .ok_or("transcription_failed")?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("no_speech");
    }
    if reply.is_cancelled() {
        return Err("cancelled");
    }
    Ok(text)
}

/// The result's fields beside `ok`, as `PROTOCOL.md` names them.
fn fields(outcome: CommandOutcome) -> Value {
    let mut fields = Map::new();
    fields.insert("transcript".into(), outcome.transcript.into());
    fields.insert("status".into(), outcome.status.into());
    fields.insert("feedback".into(), outcome.feedback.into());
    if let Some(id) = outcome.id {
        fields.insert("id".into(), id.into());
    }
    if let Some(pending) = outcome.pending {
        fields.insert("pending".into(), pending.into());
    }
    if let Some(question) = outcome.question {
        fields.insert("question".into(), question.into());
        fields.insert("choices".into(), json!(outcome.choices));
    }
    Value::Object(fields)
}

/// A command report as the phone hears about it.
pub fn outcome_of(report: &crate::command_mode::CommandReport) -> CommandOutcome {
    use crate::command_mode::CommandStatus;
    let status = match report.status {
        CommandStatus::Done => "done",
        CommandStatus::AwaitingConfirmation => "awaiting_confirmation",
        CommandStatus::NeedsAnswer => "needs_answer",
        CommandStatus::NeedsComputerUse => "needs_computer_use",
        CommandStatus::Cancelled => "cancelled",
        CommandStatus::Failed => "failed",
    };
    let waiting = matches!(
        report.status,
        CommandStatus::AwaitingConfirmation | CommandStatus::NeedsAnswer
    );
    CommandOutcome {
        transcript: report.transcript.clone(),
        status: status.to_string(),
        feedback: report.summary(),
        id: waiting.then_some(report.id),
        pending: report.pending_summary.clone(),
        question: report.question.clone(),
        choices: report.choices.clone(),
        not_configured: crate::assistant::act::not_configured(report),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{self, FakeHost};
    use super::*;
    use std::time::Duration;
    use tokio::sync::mpsc;

    async fn next(frames: &mut mpsc::Receiver<String>) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .expect("a reply")
            .expect("open outbox");
        serde_json::from_str::<Value>(&frame).unwrap()["body"].clone()
    }

    fn router(host: FakeHost) -> (Arc<Router>, Arc<FakeHost>, mpsc::Receiver<String>) {
        let host = Arc::new(host);
        let (router, frames) = testing::router(host.clone());
        (router, host, frames)
    }

    #[tokio::test]
    async fn a_typed_command_runs_and_reports() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle(
            "phone",
            json!({ "kind": "command", "requestId": "c1", "text": " open youtube " }),
        );
        assert_eq!(next(&mut frames).await["stage"], "working");
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "c1", "ok": true,
                    "transcript": "open youtube", "status": "done",
                    "feedback": "Did: open youtube" })
        );
        assert_eq!(*host.commands.lock().unwrap(), ["run open youtube"]);
    }

    #[tokio::test]
    async fn a_spoken_command_is_transcribed_first() {
        let (router, host, mut frames) = router(FakeHost::default());
        let audio = dictate::tests::wav_base64(16_000);
        router.handle(
            "phone",
            json!({ "kind": "command", "requestId": "c1", "audio": audio }),
        );
        assert_eq!(next(&mut frames).await["stage"], "transcribing");
        assert_eq!(next(&mut frames).await["stage"], "working");
        assert_eq!(next(&mut frames).await["transcript"], "hello there");
        assert_eq!(*host.commands.lock().unwrap(), ["run hello there"]);
        // Nothing is pasted or kept in History: it was a command.
        assert!(host.pasted.lock().unwrap().is_empty());
        assert!(host.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn waiting_commands_carry_their_id_and_are_confirmed_by_it() {
        let host = FakeHost::default();
        *host.command_waits.lock().unwrap() = true;
        let (router, host, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "command", "requestId": "c1", "text": "email ron" }),
        );
        next(&mut frames).await;
        let result = next(&mut frames).await;
        assert_eq!(result["status"], "awaiting_confirmation");
        assert_eq!(result["id"], 7);
        assert_eq!(result["pending"], "Send the email");

        router.handle(
            "phone",
            json!({ "kind": "command.confirm", "requestId": "c2", "id": 7, "accept": true }),
        );
        next(&mut frames).await;
        next(&mut frames).await;
        router.handle(
            "phone",
            json!({ "kind": "command.answer", "requestId": "c3", "id": 7, "answer": "the work one" }),
        );
        next(&mut frames).await;
        next(&mut frames).await;
        assert_eq!(
            *host.commands.lock().unwrap(),
            ["run email ron", "confirm 7 true", "answer 7 the work one"]
        );
    }

    #[tokio::test]
    async fn refusals_come_before_anything_runs() {
        let host = FakeHost::default();
        *host.command_on.lock().unwrap() = false;
        let (router, host, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "command", "requestId": "c1", "text": "open youtube" }),
        );
        assert_eq!(next(&mut frames).await["error"], "command_mode_off");

        *host.command_on.lock().unwrap() = true;
        for (id, body) in [
            (
                "c2",
                json!({ "kind": "command", "requestId": "c2", "text": "" }),
            ),
            ("c3", json!({ "kind": "command", "requestId": "c3" })),
            (
                "c4",
                json!({ "kind": "command", "requestId": "c4", "text": "x", "audio": "y" }),
            ),
            (
                "c5",
                json!({ "kind": "command.confirm", "requestId": "c5", "id": "7", "accept": true }),
            ),
            (
                "c6",
                json!({ "kind": "command.answer", "requestId": "c6", "id": 7 }),
            ),
        ] {
            router.handle("phone", body);
            assert_eq!(next(&mut frames).await["error"], "unsupported", "{id}");
        }
        assert!(host.commands.lock().unwrap().is_empty());
    }

    /// The executor's own "not set up" report, namespaced key and all, is the
    /// phone's `not_configured`; any other failure is not.
    #[test]
    fn a_report_with_no_model_reads_as_not_configured() {
        use crate::command_mode::registry::Feedback;
        use crate::command_mode::{CommandOrigin, CommandReport, CommandStatus};
        let report = |key: &str| CommandReport {
            id: 1,
            origin: CommandOrigin::Phone,
            transcript: "open youtube".into(),
            status: CommandStatus::Failed,
            feedback: vec![Feedback::new(key, "Command mode isn't set up: no key", &[])],
            steps: Vec::new(),
            pending_summary: None,
            question: None,
            choices: Vec::new(),
            escalation: None,
            dry_run: false,
            dry_run_actions: Vec::new(),
            model: String::new(),
            llm_calls: 0,
            usage: Default::default(),
            latency_ms: 0,
            error: None,
        };
        assert!(outcome_of(&report("notConfigured")).not_configured);
        assert!(!outcome_of(&report("routeFailed")).not_configured);
    }

    #[tokio::test]
    async fn no_model_is_its_own_error() {
        let host = FakeHost::default();
        *host.command_unconfigured.lock().unwrap() = true;
        let (router, _, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "command", "requestId": "c1", "text": "open youtube" }),
        );
        assert_eq!(next(&mut frames).await["stage"], "working");
        assert_eq!(next(&mut frames).await["error"], "not_configured");
    }
}
