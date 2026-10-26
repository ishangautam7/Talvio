//! The model-and-tools loop of one assistant turn, shared by the panel (the
//! quick ask and calls, `run_assistant_turn_inner`) and a phone's question
//! (`phone.rs`). What differs between them (where tokens go, what the user sees
//! while a tool runs, how a screenshot is taken, which surface a command
//! belongs to) is a [`TurnTools`]; the rounds, the tool-call bookkeeping the
//! providers require and the last-round answer are here once.

use futures_util::future::BoxFuture;
use log::debug;
use serde_json::{json, Value};
use std::time::Instant;

use super::{act, tool_round_policy, ToolRoundPolicy, MAX_ASSISTANT_TOOL_ROUNDS};
use crate::llm_client::{self, ReasoningConfig, ToolCall, ToolStreamOutcome};
use crate::settings::PostProcessProvider;

/// Who answers a turn: the provider, its key, the model and the thinking dial.
#[derive(Clone)]
pub(crate) struct TurnModel {
    pub provider: PostProcessProvider,
    pub api_key: String,
    pub model: String,
    pub thinking: (Option<String>, Option<ReasoningConfig>),
}

/// What a turn's tools do on the surface that asked.
pub(crate) trait TurnTools: Send + Sync {
    /// The model asked for `calls`; they are about to run.
    fn round_started(&self, calls: &[ToolCall]);
    /// They ran; the model is asked again.
    fn round_finished(&self) {}
    /// Run one tool whose result is text (every tool but `capture_screen`).
    fn run<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, String>;
    /// Take a screenshot for `capture_screen`, as a data URL.
    fn capture_screen(&self) -> BoxFuture<'_, Result<String, String>>;
}

/// Tools that run one at a time, in the model's order, after the others: a
/// screenshot, whose result is a picture, and an action on the computer,
/// since two at once would race for the same windows (and command mode keeps
/// one waiting confirmation, which the second would replace).
fn runs_alone(name: &str) -> bool {
    name == "capture_screen" || name == act::TOOL_NAME
}

/// The assistant message that carries a round's tool calls, as the provider
/// needs it echoed before the results.
fn tool_call_message(round: &ToolStreamOutcome) -> Value {
    let calls: Vec<Value> = round
        .tool_calls
        .iter()
        .map(|tc| {
            let mut call = json!({
                "id": tc.id,
                "type": "function",
                "function": { "name": tc.name, "arguments": tc.arguments }
            });
            // Gemini thinking models reject the follow-up request
            // ("Function call is missing a thought_signature in functionCall
            // parts", HTTP 400) unless the opaque signature that came back with
            // the call is echoed verbatim, in exactly this shape. Added only
            // when the provider sent one, so requests to every other
            // OpenAI-compatible provider stay byte-identical.
            if let Some(signature) = &tc.thought_signature {
                call["extra_content"] = json!({ "google": { "thought_signature": signature } });
            }
            call
        })
        .collect();
    json!({
        "role": "assistant",
        "content": round.text,
        "tool_calls": calls
    })
}

/// Run the turn: up to [`MAX_ASSISTANT_TOOL_ROUNDS`] model rounds, running
/// the tools each asks for, and resolve to the final answer text (raw, as the
/// client returned it). `sink` makes each round's token sink.
pub(crate) async fn run_tool_loop<S: FnMut(&str)>(
    model: &TurnModel,
    mut msgs: Vec<Value>,
    tools: &Value,
    hooks: &dyn TurnTools,
    sink: impl Fn() -> S,
) -> Result<String, String> {
    let started = Instant::now();
    let mut answer = String::new();
    // At most one screenshot per user message.
    let mut screen_captured = false;
    for round_index in 0..MAX_ASSISTANT_TOOL_ROUNDS {
        // The model alone decides whether to call a tool; we never force one.
        let round_out = llm_client::send_chat_stream_with_tools(
            &model.provider,
            model.api_key.clone(),
            &model.model,
            msgs.clone(),
            tools.clone(),
            json!("auto"),
            model.thinking.0.clone(),
            model.thinking.1.clone(),
            sink(),
        )
        .await?;
        let round_policy = tool_round_policy(&round_out, round_index);
        if round_policy == ToolRoundPolicy::FinalResponse {
            answer = round_out.text;
            break;
        }
        hooks.round_started(&round_out.tool_calls);
        msgs.push(tool_call_message(&round_out));
        let tool_names: Vec<&str> = round_out
            .tool_calls
            .iter()
            .map(|tc| tc.name.as_str())
            .collect();

        // Independent tools run concurrently: two searches in one round should
        // cost one wait, not two.
        let dispatched = Instant::now();
        let concurrent: Vec<usize> = round_out
            .tool_calls
            .iter()
            .enumerate()
            .filter(|(_, tc)| !runs_alone(&tc.name))
            .map(|(index, _)| index)
            .collect();
        let mut text_results: Vec<Option<String>> = vec![None; round_out.tool_calls.len()];
        let completed = futures_util::future::join_all(
            concurrent
                .iter()
                .map(|&index| hooks.run(&round_out.tool_calls[index])),
        )
        .await;
        for (&index, content) in concurrent.iter().zip(completed) {
            text_results[index] = Some(content);
        }

        for (index, tc) in round_out.tool_calls.iter().enumerate() {
            // capture_screen returns an image, not text: the tool message is a
            // short receipt and the frame rides in a follow-up user message
            // (image parts aren't valid in tool results on most
            // OpenAI-compatible providers).
            if tc.name == "capture_screen" {
                if screen_captured {
                    msgs.push(json!({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": "A screenshot was already captured for this message. Answer now using it."
                    }));
                    continue;
                }
                match hooks.capture_screen().await {
                    Ok(data_url) => {
                        screen_captured = true;
                        msgs.push(json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": "Screenshot captured. It is attached in the next message."
                        }));
                        msgs.push(json!({
                            "role": "user",
                            "content": [
                                {"type": "text", "text": "[Screenshot of the user's current screen, from the capture_screen tool. Use it to answer the previous question.]"},
                                {"type": "image_url", "image_url": {"url": data_url}}
                            ]
                        }));
                    }
                    Err(e) => {
                        log::warn!("Agent screen capture failed: {}", e);
                        msgs.push(json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": format!("Screen capture is unavailable ({}). Answer without it.", e)
                        }));
                    }
                }
                continue;
            }
            let content = match text_results[index].take() {
                Some(content) => content,
                None => hooks.run(tc).await,
            };
            msgs.push(json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": content
            }));
        }
        debug!(
            "Assistant tools [{}] took {}ms ({}ms into the turn)",
            tool_names.join(", "),
            dispatched.elapsed().as_millis(),
            started.elapsed().as_millis()
        );
        hooks.round_finished();
        answer = round_out.text;
        if round_policy == ToolRoundPolicy::RunToolsThenStop {
            // Out of tool rounds. A round that ends in tool calls usually
            // writes no prose, so stopping now would end the turn with no
            // reply at all. Ask once more with no tools attached, so the model
            // has to answer from what it already has.
            if answer.trim().is_empty() {
                debug!("Tool rounds exhausted with no answer text; asking once more without tools");
                answer = llm_client::send_chat_stream(
                    &model.provider,
                    model.api_key.clone(),
                    &model.model,
                    msgs.clone(),
                    model.thinking.0.clone(),
                    model.thinking.1.clone(),
                    sink(),
                )
                .await?;
            }
            break;
        }
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshots_and_actions_run_alone() {
        assert!(runs_alone("capture_screen"));
        assert!(runs_alone(act::TOOL_NAME));
        assert!(!runs_alone("web_search"));
        assert!(!runs_alone("get_current_datetime"));
    }

    #[test]
    fn a_thought_signature_is_echoed_and_only_then() {
        let round = ToolStreamOutcome {
            text: String::new(),
            tool_calls: vec![
                ToolCall {
                    id: "a".into(),
                    name: "web_search".into(),
                    arguments: "{}".into(),
                    thought_signature: Some("sig".into()),
                },
                ToolCall {
                    id: "b".into(),
                    name: act::TOOL_NAME.into(),
                    arguments: "{}".into(),
                    thought_signature: None,
                },
            ],
        };
        let message = tool_call_message(&round);
        assert_eq!(
            message["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
            "sig"
        );
        assert!(message["tool_calls"][1].get("extra_content").is_none());
        assert_eq!(message["tool_calls"][1]["function"]["name"], act::TOOL_NAME);
    }
}
