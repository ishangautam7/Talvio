//! `assistant.ask`: a phone asking this computer's assistant (`PROTOCOL.md`,
//! "Assistant"). The question is text, or a recording this computer
//! transcribes; the answer is the assistant's, as for a quick ask here, with
//! whatever it did on the computer ([`crate::assistant::phone`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, info};
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::{dictate, Reply, Router};
use crate::assistant::phone::{PhoneAnswer, PhoneAsk, PhoneAskError, PhoneProgress};

const MAX_TEXT_CHARS: usize = 4_000;
const MAX_CONVERSATION_CHARS: usize = 64;
/// At most four `answer` progress messages a second.
const ANSWER_EVERY: Duration = Duration::from_millis(250);
/// How often a running question looks at its cancel flag and the switch.
const POLL: Duration = Duration::from_millis(200);

/// Lets at most one `answer` progress through per [`ANSWER_EVERY`].
#[derive(Default)]
struct Throttle {
    last: Option<Instant>,
}

impl Throttle {
    fn allows(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < ANSWER_EVERY)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// This phone's question is being answered; let go when dropped.
struct Asking {
    router: Arc<Router>,
    phone: String,
}

impl Drop for Asking {
    fn drop(&mut self) {
        self.router
            .asking
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.phone);
    }
}

impl Router {
    /// Mark `phone` as asking, unless it already is.
    fn begin_asking(self: &Arc<Self>, phone: &str) -> Option<Asking> {
        let mut asking = self.asking.lock().unwrap_or_else(|e| e.into_inner());
        asking.insert(phone.to_string()).then(|| Asking {
            router: self.clone(),
            phone: phone.to_string(),
        })
    }
}

impl Reply {
    /// A `progress` message with fields of its own.
    fn progress_with(&self, stage: &str, fields: Value) {
        let mut body = Map::new();
        body.insert("kind".into(), "progress".into());
        body.insert("stage".into(), stage.into());
        if let Value::Object(fields) = fields {
            body.extend(fields);
        }
        self.send(body);
    }

    /// `failed`, with why in English.
    fn fail_with_message(mut self, message: &str) {
        let mut body = Map::new();
        body.insert("kind".into(), "result".into());
        body.insert("ok".into(), false.into());
        body.insert("error".into(), "failed".into());
        body.insert("message".into(), message.into());
        self.release();
        self.send(body);
    }
}

/// `assistant.ask`.
pub(super) async fn ask(router: Arc<Router>, body: Value, reply: Reply) {
    let text = match body.get("text") {
        Some(Value::String(text)) => Some(text.trim().to_string()),
        Some(_) => return reply.fail("unsupported"),
        None => None,
    };
    let audio = match body.get("audio") {
        Some(Value::String(audio)) => Some(audio.as_str()),
        Some(_) => return reply.fail("unsupported"),
        None => None,
    };
    let conversation = match body.get("conversation") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if (1..=MAX_CONVERSATION_CHARS).contains(&id.chars().count()) => {
            Some(id.clone())
        }
        Some(_) => return reply.fail("unsupported"),
    };
    let well_formed = match (&text, audio) {
        (Some(text), None) => (1..=MAX_TEXT_CHARS).contains(&text.chars().count()),
        (None, Some(_)) => true,
        _ => false,
    };
    if !well_formed {
        return reply.fail("unsupported");
    }

    let host = router.host();
    if !router.input_allowed() {
        return reply.fail("remote_input_off");
    }
    if !host.assistant_enabled() {
        return reply.fail("assistant_off");
    }
    // One question per phone at a time. Let go before the result is sent,
    // so the phone's next question, sent the moment it arrives, is not `busy`.
    let Some(asking) = router.begin_asking(&reply.to) else {
        return reply.fail("busy");
    };

    let text = match (text, audio) {
        (Some(text), _) => text,
        (None, Some(audio)) => match transcribe(&router, &reply, audio).await {
            Ok(text) => text,
            Err(code) => {
                drop(asking);
                return reply.fail(code);
            }
        },
        (None, None) => return reply.fail("unsupported"),
    };
    reply.progress_with("thinking", json!({ "transcript": text }));

    let (progress, mut updates) = mpsc::unbounded_channel();
    let answer = host.assistant_ask(PhoneAsk {
        phone: reply.to.clone(),
        conversation,
        text: text.clone(),
        progress,
    });
    tokio::pin!(answer);
    let mut throttle = Throttle::default();
    let mut updates_open = true;
    // An interval rather than a fresh sleep per pass, so a stream of progress
    // cannot keep the cancel check from ever running.
    let mut tick = tokio::time::interval(POLL);
    let outcome = loop {
        tokio::select! {
            // The checks first, so a stream of progress cannot starve them;
            // then progress, so what the answer reported before it finished
            // goes out ahead of its result.
            biased;
            _ = tick.tick() => {
                // Dropping the answer stops it: an LLM request in flight is
                // abandoned and no further tool starts.
                if reply.is_cancelled() {
                    drop(asking);
                    return reply.fail("cancelled");
                }
                if !router.input_allowed() {
                    drop(asking);
                    return reply.fail("remote_input_off");
                }
            }
            update = updates.recv(), if updates_open => match update {
                Some(update) => forward(&reply, &mut throttle, update),
                None => updates_open = false,
            },
            outcome = &mut answer => break outcome,
        }
    };
    // What the answer reported in the poll that finished it.
    while let Ok(update) = updates.try_recv() {
        forward(&reply, &mut throttle, update);
    }
    drop(asking);
    match outcome {
        Ok(answer) => {
            info!("Phone question answered");
            reply.ok(fields(text, answer));
        }
        Err(PhoneAskError::NotConfigured) => reply.fail("not_configured"),
        Err(PhoneAskError::Failed(message)) => reply.fail_with_message(&message),
    }
}

/// Send one progress update on to the phone, `answer` throttled.
fn forward(reply: &Reply, throttle: &mut Throttle, update: PhoneProgress) {
    match update {
        PhoneProgress::Tool(label) => reply.progress_with("tool", json!({ "label": label })),
        PhoneProgress::Answer(so_far) => {
            if throttle.allows(Instant::now()) {
                reply.progress_with("answer", json!({ "text": so_far }));
            }
        }
    }
}

async fn transcribe(router: &Router, reply: &Reply, audio: &str) -> Result<String, &'static str> {
    let host = router.host();
    if !host.speech_ready() {
        return Err("no_speech_model");
    }
    let (_, samples) = dictate::decode(audio).map_err(|why| {
        debug!("A phone question's recording was refused: {why}");
        "bad_audio"
    })?;
    reply.progress("transcribing");
    let text = super::with_heartbeat(reply, host.transcribe(samples), || {})
        .await
        .ok_or("transcription_failed")?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("no_speech");
    }
    if reply.is_cancelled() {
        return Err("cancelled");
    }
    if !router.input_allowed() {
        return Err("remote_input_off");
    }
    Ok(text)
}

/// The result's fields beside `ok`, as `PROTOCOL.md` names them.
fn fields(transcript: String, answer: PhoneAnswer) -> Value {
    let actions: Vec<Value> = answer
        .actions
        .into_iter()
        .map(|action| json!({ "label": action.label, "status": action.status }))
        .collect();
    let mut fields = Map::new();
    fields.insert("transcript".into(), transcript.into());
    fields.insert("answer".into(), answer.answer.into());
    fields.insert("conversation".into(), answer.conversation.into());
    fields.insert("actions".into(), actions.into());
    if let Some(pending) = answer.pending {
        fields.insert(
            "pending".into(),
            json!({ "id": pending.id, "summary": pending.summary }),
        );
    }
    Value::Object(fields)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{self, FakeHost};
    use super::*;

    async fn next(frames: &mut mpsc::Receiver<String>) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .expect("a reply")
            .expect("open outbox");
        serde_json::from_str::<Value>(&frame).unwrap()["body"].clone()
    }

    /// Skip progress until request `id`'s result.
    async fn result_of(frames: &mut mpsc::Receiver<String>, id: &str) -> Value {
        loop {
            let body = next(frames).await;
            if body["kind"] == "result" && body["requestId"] == id {
                return body;
            }
        }
    }

    fn router(host: FakeHost) -> (Arc<Router>, Arc<FakeHost>, mpsc::Receiver<String>) {
        let host = Arc::new(host);
        let (router, frames) = testing::router(host.clone());
        (router, host, frames)
    }

    #[tokio::test]
    async fn a_typed_question_is_answered_with_progress() {
        let (router, host, mut frames) = router(FakeHost::default());
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a1", "text": " play believer " }),
        );
        let thinking = next(&mut frames).await;
        assert_eq!(thinking["stage"], "thinking");
        assert_eq!(thinking["transcript"], "play believer");
        let tool = next(&mut frames).await;
        assert_eq!(tool["stage"], "tool");
        assert_eq!(tool["label"], "Play believer");
        let answer = next(&mut frames).await;
        assert_eq!(answer["stage"], "answer");
        assert_eq!(
            next(&mut frames).await,
            json!({ "kind": "result", "requestId": "a1", "ok": true,
                    "transcript": "play believer",
                    "answer": "Answer to: play believer",
                    "conversation": "c1",
                    "actions": [{ "label": "Played it", "status": "done" }] })
        );
        assert_eq!(*host.asks.lock().unwrap(), ["phone: play believer (new)"]);
    }

    #[tokio::test]
    async fn a_conversation_and_a_pending_action_are_passed_through() {
        let host = FakeHost::default();
        *host.assistant_pending.lock().unwrap() = true;
        let (router, host, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a1", "text": "email ron", "conversation": "c9" }),
        );
        let result = result_of(&mut frames, "a1").await;
        assert_eq!(result["conversation"], "c9");
        assert_eq!(
            result["pending"],
            json!({ "id": 7, "summary": "Send the email" })
        );
        assert_eq!(*host.asks.lock().unwrap(), ["phone: email ron (c9)"]);
    }

    #[tokio::test]
    async fn a_spoken_question_is_transcribed_first() {
        let (router, host, mut frames) = router(FakeHost::default());
        let audio = dictate::tests::wav_base64(16_000);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a1", "audio": audio }),
        );
        assert_eq!(next(&mut frames).await["stage"], "transcribing");
        assert_eq!(next(&mut frames).await["transcript"], "hello there");
        assert_eq!(*host.asks.lock().unwrap(), ["phone: hello there (new)"]);
        assert!(host.pasted.lock().unwrap().is_empty());
        assert!(host.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn one_question_per_phone_at_a_time() {
        let host = FakeHost::default();
        let (release, gate) = tokio::sync::oneshot::channel();
        *host.assistant_gate.lock().unwrap() = Some(gate);
        host.approved.lock().unwrap().insert("phone2".into());
        let (router, _, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a1", "text": "first" }),
        );
        assert_eq!(next(&mut frames).await["stage"], "thinking");
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a2", "text": "second" }),
        );
        assert_eq!(result_of(&mut frames, "a2").await["error"], "busy");
        // Another phone is not held up.
        router.handle(
            "phone2",
            json!({ "kind": "assistant.ask", "requestId": "b1", "text": "other" }),
        );
        assert_eq!(result_of(&mut frames, "b1").await["ok"], true);
        let _ = release.send(());
        assert_eq!(result_of(&mut frames, "a1").await["ok"], true);
        // Free again.
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a3", "text": "third" }),
        );
        assert_eq!(next(&mut frames).await["stage"], "thinking");
    }

    #[tokio::test]
    async fn a_cancel_stops_the_question() {
        let host = FakeHost::default();
        let (_release, gate) = tokio::sync::oneshot::channel();
        *host.assistant_gate.lock().unwrap() = Some(gate);
        let (router, _, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a1", "text": "first" }),
        );
        assert_eq!(next(&mut frames).await["stage"], "thinking");
        router.handle("phone", json!({ "kind": "cancel", "requestId": "a1" }));
        assert_eq!(result_of(&mut frames, "a1").await["error"], "cancelled");
    }

    #[tokio::test]
    async fn refusals_and_failures_have_their_codes() {
        let host = FakeHost::default();
        *host.assistant_on.lock().unwrap() = false;
        let (router, host, mut frames) = router(host);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a1", "text": "hi" }),
        );
        assert_eq!(next(&mut frames).await["error"], "assistant_off");
        *host.assistant_on.lock().unwrap() = true;

        for (id, body) in [
            (
                "a2",
                json!({ "kind": "assistant.ask", "requestId": "a2", "text": "" }),
            ),
            ("a3", json!({ "kind": "assistant.ask", "requestId": "a3" })),
            (
                "a4",
                json!({ "kind": "assistant.ask", "requestId": "a4", "text": "x", "audio": "y" }),
            ),
            (
                "a5",
                json!({ "kind": "assistant.ask", "requestId": "a5", "text": "x", "conversation": 3 }),
            ),
            (
                "a6",
                json!({ "kind": "assistant.ask", "requestId": "a6", "text": "x".repeat(4_001) }),
            ),
        ] {
            router.handle("phone", body);
            assert_eq!(next(&mut frames).await["error"], "unsupported", "{id}");
        }

        *host.assistant_error.lock().unwrap() = Some(PhoneAskError::NotConfigured);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a7", "text": "hi" }),
        );
        assert_eq!(
            result_of(&mut frames, "a7").await["error"],
            "not_configured"
        );

        *host.assistant_error.lock().unwrap() =
            Some(PhoneAskError::Failed("HTTP 401: bad key".into()));
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a8", "text": "hi" }),
        );
        let failed = result_of(&mut frames, "a8").await;
        assert_eq!(failed["error"], "failed");
        assert_eq!(failed["message"], "HTTP 401: bad key");

        host.enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        router.handle(
            "phone",
            json!({ "kind": "assistant.ask", "requestId": "a9", "text": "hi" }),
        );
        assert_eq!(next(&mut frames).await["error"], "remote_input_off");
    }

    #[test]
    fn answers_are_throttled_to_four_a_second() {
        let start = Instant::now();
        let mut throttle = Throttle::default();
        assert!(throttle.allows(start));
        assert!(!throttle.allows(start + Duration::from_millis(100)));
        assert!(!throttle.allows(start + Duration::from_millis(249)));
        assert!(throttle.allows(start + Duration::from_millis(250)));
        assert!(!throttle.allows(start + Duration::from_millis(400)));
    }
}
