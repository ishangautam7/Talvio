//! Command routing over any OpenAI-compatible provider.
//!
//! Everything but Anthropic goes through here, and it deliberately reuses
//! [`crate::llm_client::send_chat_stream_with_tools`] rather than a second HTTP
//! client: that function already knows every provider quirk the app has met
//! (Azure base URLs, OpenRouter headers, Gemini's `models/` prefix and
//! thought signatures, the reasoning-effort ladder, in-stream errors, strict
//! role alternation). A router that re-implemented the wire format would have
//! to rediscover each of those the hard way.
//!
//! What this file adds is the translation between the router's
//! provider-neutral conversation ([`Turn`]) and OpenAI's message list, plus
//! one negotiation `llm_client` does not do: forced tool choice. The router
//! wants `tool_choice: "required"`, since a command that comes back as prose
//! does nothing, but some gateways and local servers refuse that value. Such a
//! refusal is retried once with `"auto"` (the system prompt still says to call
//! a tool) and remembered per provider and model, so later commands do not pay
//! for the failed request again.
//!
//! Gemini thinking models attach an opaque `thought_signature` to each call
//! and refuse a later request that replays the call without it ("Function
//! call is missing a thought_signature", HTTP 400). The router's repair round
//! is exactly such a request, so each router keeps the signatures of the
//! calls it was answered with, by call id, and puts them back as
//! `extra_content.google.thought_signature` when those calls are replayed (the
//! same shape `assistant.rs` sends). They stay out of [`RoutedCall`], which is
//! serialized into reports, because they are opaque provider state, not part
//! of what the model decided.
//!
//! Known limitations, each a property of `llm_client` today:
//! - No token usage: the streaming client does not parse `usage`, so every
//!   response reports [`Usage::default`]. Phase 4 adds usage parsing.
//! - No stop reason: the stream's `finish_reason` is not surfaced either, so
//!   the router's guard against an answer cut off at the output limit only
//!   fires on the Anthropic transport. `ToolStreamOutcome` would have to carry
//!   the `finish_reason` its accumulator already reads.
//! - No output cap: `send_chat_stream_with_tools` takes no `max_tokens`, so
//!   [`RouteRequest::max_tokens`] is not sent. A router answer is a tool call
//!   or a sentence, so the cap is a cost guard rather than a correctness one.

use futures_util::future::BoxFuture;
use log::{debug, warn};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use super::{LlmError, RouteRequest, RouteResponse, RoutedCall, RouterLlm, ToolDef, Turn, Usage};
use crate::llm_client::{self, ToolCall};
use crate::settings::{PostProcessProvider, ThinkingLevel};

/// How long one routing request may take, retries included. A router answer
/// is short; anything slower than this is a stalled provider, and the person
/// who spoke the command is waiting on it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// `(provider id, model)` pairs that refused `tool_choice: "required"`.
/// Process-wide so the lesson outlives one router instance (one is built per
/// command), and not persisted so a provider that adds support is tried again
/// after a restart.
static REQUIRED_REFUSED: Lazy<Mutex<HashSet<(String, String)>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

/// Tier 1 router for every non-Anthropic HTTP provider.
pub struct OpenAiCompatRouter {
    provider: PostProcessProvider,
    api_key: String,
    model: String,
    /// Gemini thought signatures by call id, from every answer this router
    /// received (see the module docs). Per router rather than process-wide:
    /// one router serves one command, and ids such as `call_0` repeat from
    /// command to command.
    signatures: Mutex<HashMap<String, String>>,
}

impl OpenAiCompatRouter {
    pub fn new(provider: PostProcessProvider, api_key: String, model: String) -> Self {
        Self {
            provider,
            api_key,
            model,
            signatures: Mutex::new(HashMap::new()),
        }
    }

    /// Keep the signature of every call that came with one.
    fn remember_signatures(&self, calls: &[ToolCall]) {
        let mut signatures = self.signatures.lock().unwrap_or_else(|e| e.into_inner());
        for call in calls {
            if let Some(signature) = &call.thought_signature {
                signatures.insert(call.id.clone(), signature.clone());
            }
        }
    }

    fn signatures(&self) -> HashMap<String, String> {
        self.signatures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn refusal_key(&self) -> (String, String) {
        (self.provider.id.clone(), self.model.clone())
    }

    fn required_refused(&self) -> bool {
        REQUIRED_REFUSED
            .lock()
            .map(|set| set.contains(&self.refusal_key()))
            .unwrap_or(false)
    }

    fn remember_required_refused(&self) {
        if let Ok(mut set) = REQUIRED_REFUSED.lock() {
            set.insert(self.refusal_key());
        }
    }

    /// One request, with the `"required"` → `"auto"` fallback. Errors are the
    /// raw `llm_client` strings; [`map_error`] classifies them once, at the end.
    async fn send(&self, request: &RouteRequest) -> Result<llm_client::ToolStreamOutcome, String> {
        let messages = render_messages(request, &self.signatures());
        let tools = render_tools(&request.tools);
        let (reasoning_effort, reasoning) =
            llm_client::reasoning_options_for(&self.provider.id, &self.model, ThinkingLevel::Off);

        let force = request.force_tool && !self.required_refused();
        let choice = if force { json!("required") } else { json!("auto") };
        let first = llm_client::send_chat_stream_with_tools(
            &self.provider,
            self.api_key.clone(),
            &self.model,
            messages.clone(),
            tools.clone(),
            choice,
            reasoning_effort.clone(),
            reasoning.clone(),
            |_| {},
        )
        .await;

        match first {
            Err(error) if force && tool_choice_rejected(&error) => {
                warn!(
                    "Command router {}/{} seems to refuse tool_choice \"required\"; retrying with \"auto\"",
                    self.provider.id, self.model
                );
                // Remembered only once "auto" has worked: an error that merely
                // looks like a refusal (a schema problem that also says
                // "required") fails the retry the same way, and must not turn
                // forced tool choice off for the rest of the session.
                let retry = llm_client::send_chat_stream_with_tools(
                    &self.provider,
                    self.api_key.clone(),
                    &self.model,
                    messages,
                    tools,
                    json!("auto"),
                    reasoning_effort,
                    reasoning,
                    |_| {},
                )
                .await;
                if retry.is_ok() {
                    self.remember_required_refused();
                }
                retry
            }
            other => other,
        }
    }
}

impl RouterLlm for OpenAiCompatRouter {
    fn route<'a>(
        &'a self,
        request: &'a RouteRequest,
    ) -> BoxFuture<'a, Result<RouteResponse, LlmError>> {
        Box::pin(async move {
            // The built-in engine has to be started and held busy (an activity
            // guard, so idle-unload cannot stop it mid-request) through
            // `LocalLlmManager`, which lives in Tauri state. `RouterLlm` has no
            // AppHandle, and sending to its port without that would race the
            // engine's own lifecycle, so it is refused plainly instead.
            if self.provider.id == "builtin" {
                return Err(LlmError::NotConfigured(
                    "the built-in engine cannot route commands yet; pick a cloud provider".into(),
                ));
            }
            debug!(
                "Routing a command through {} ({} tools, {} turns, forced: {})",
                self.label(),
                request.tools.len(),
                request.turns.len(),
                request.force_tool
            );
            let outcome = tokio::time::timeout(REQUEST_TIMEOUT, self.send(request))
                .await
                .map_err(|_| LlmError::Timeout)?
                .map_err(map_error)?;
            self.remember_signatures(&outcome.tool_calls);
            Ok(RouteResponse {
                calls: routed_calls(outcome.tool_calls),
                text: outcome.text,
                usage: Usage::default(),
                stop_reason: None,
            })
        })
    }

    fn label(&self) -> String {
        format!("{}/{}", self.provider.id, self.model)
    }

    fn model(&self) -> &str {
        &self.model
    }
}

/// The router conversation as OpenAI chat messages.
///
/// An assistant turn without calls carries no `tool_calls` key at all, since
/// OpenAI refuses an empty array there. Tool results become one `tool`
/// message each, in order, as the API requires. A replayed call whose id has
/// a Gemini thought signature in `signatures` carries it back.
pub(crate) fn render_messages(
    request: &RouteRequest,
    signatures: &HashMap<String, String>,
) -> Vec<Value> {
    let mut messages = Vec::with_capacity(request.turns.len() + 1);
    if !request.system.is_empty() {
        messages.push(json!({"role": "system", "content": request.system}));
    }
    for turn in &request.turns {
        match turn {
            Turn::User(text) => messages.push(json!({"role": "user", "content": text})),
            Turn::Assistant { text, calls } => {
                let mut message = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    // `arguments` is a JSON *string* on this API, and it must
                    // hold an object: a call whose arguments did not parse
                    // (`Null`) is replayed as `{}`, because gateways that
                    // decode it into an object (Gemini's) refuse `"null"`, and
                    // the repair round is exactly when such a call is replayed.
                    // The tool result after it says what was wrong.
                    let tool_calls: Vec<Value> = calls
                        .iter()
                        .map(|call| {
                            let arguments = if call.input.is_object() {
                                call.input.to_string()
                            } else {
                                "{}".to_string()
                            };
                            let mut rendered = json!({
                                "id": call.id,
                                "type": "function",
                                "function": {
                                    "name": call.name,
                                    "arguments": arguments,
                                },
                            });
                            // Only when the provider sent one, so every other
                            // provider's request stays byte-identical.
                            if let Some(signature) = signatures.get(&call.id) {
                                rendered["extra_content"] =
                                    json!({"google": {"thought_signature": signature}});
                            }
                            rendered
                        })
                        .collect();
                    message["tool_calls"] = Value::Array(tool_calls);
                }
                messages.push(message);
            }
            Turn::ToolResults(results) => {
                for result in results {
                    let content = if result.is_error {
                        format!("Error: {}", result.content)
                    } else {
                        result.content.clone()
                    };
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": result.call_id,
                        "content": content,
                    }));
                }
            }
        }
    }
    messages
}

/// The tool list in OpenAI's `function` shape.
pub(crate) fn render_tools(tools: &[ToolDef]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    },
                })
            })
            .collect(),
    )
}

/// The calls as the router sees them. Arguments that are not valid JSON
/// become `Value::Null` rather than dropping the call, so validation can tell
/// the model what was wrong. An empty string (a call with no arguments, which
/// some providers stream that way) is an empty object.
pub(crate) fn routed_calls(calls: Vec<ToolCall>) -> Vec<RoutedCall> {
    calls
        .into_iter()
        .map(|call| {
            let input = if call.arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&call.arguments).unwrap_or(Value::Null)
            };
            RoutedCall {
                id: call.id,
                name: call.name,
                input,
            }
        })
        .collect()
}

/// Whether a failed request was the provider refusing `tool_choice:
/// "required"`, as opposed to anything else (a bad key, a bad model, a full
/// context), which must reach the user unchanged rather than be retried.
///
/// The error has to name the parameter — `tool_choice`, or `required` beside
/// `tool` — and say it is not accepted. Auth and rate-limit answers are never
/// read as one, whatever their text says. Nor is a tool *schema* error
/// ("Invalid schema for function …", `"param": "tools[0]…"`): a JSON schema
/// has its own `required` keyword, so those say "required" and "tool" too.
pub(crate) fn tool_choice_rejected(error: &str) -> bool {
    if let Some((status, _)) = parse_http_error(error) {
        if matches!(status, 401 | 403 | 404 | 429) || status >= 500 {
            return false;
        }
    }
    let error = error.to_ascii_lowercase();
    let about_a_schema = ["tools[", "parameters", "schema"]
        .iter()
        .any(|mark| error.contains(mark));
    let names_it = error.contains("tool_choice")
        || (error.contains("required") && error.contains("tool") && !about_a_schema);
    names_it
        && ["not supported", "unsupported", "invalid", "unknown"]
            .iter()
            .any(|word| error.contains(word))
}

/// `llm_client` reports an HTTP failure as
/// `"API request failed with status 400 Bad Request: <body>"` (the status is
/// reqwest's `StatusCode` display, code then reason). Returns the code and body.
fn parse_http_error(error: &str) -> Option<(u16, &str)> {
    let rest = error.strip_prefix("API request failed with status ")?;
    let (status, detail) = rest.split_once(": ").unwrap_or((rest, ""));
    let code = status.split_whitespace().next()?.parse::<u16>().ok()?;
    Some((code, detail))
}

/// An `llm_client` error string as an [`LlmError`].
pub(crate) fn map_error(error: String) -> LlmError {
    match parse_http_error(&error) {
        Some((status, detail)) => LlmError::Http {
            status,
            detail: detail.to_string(),
        },
        None => LlmError::Transport(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::llm::ToolResult;
    use std::io::{Read, Write};

    fn provider(id: &str, base: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base.to_string(),
            allow_base_url_edit: true,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: true,
        }
    }

    fn request(turns: Vec<Turn>, force_tool: bool) -> RouteRequest {
        RouteRequest {
            system: "You route commands.".into(),
            tools: vec![ToolDef {
                name: "open_app".into(),
                description: "Open an app.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"name": {"type": "string"}},
                    "required": ["name"],
                    "additionalProperties": false
                }),
            }],
            turns,
            force_tool,
            max_tokens: 256,
        }
    }

    /// Every turn kind must land in the exact shape OpenAI expects, or the
    /// second round of a multi-step command is refused.
    #[test]
    fn every_turn_kind_renders_as_openai_messages() {
        let call = RoutedCall {
            id: "call_1".into(),
            name: "open_app".into(),
            input: json!({"name": "Spotify"}),
        };
        let req = request(
            vec![
                Turn::User("open spotify".into()),
                Turn::Assistant {
                    text: String::new(),
                    calls: vec![call],
                },
                Turn::ToolResults(vec![
                    ToolResult {
                        call_id: "call_1".into(),
                        content: "Opened Spotify".into(),
                        is_error: false,
                    },
                    ToolResult {
                        call_id: "call_2".into(),
                        content: "no such app".into(),
                        is_error: true,
                    },
                ]),
                Turn::Assistant {
                    text: "Done.".into(),
                    calls: vec![],
                },
            ],
            true,
        );
        let messages = render_messages(&req, &HashMap::new());
        assert_eq!(messages.len(), 6);
        assert_eq!(
            messages[0],
            json!({"role": "system", "content": "You route commands."})
        );
        assert_eq!(messages[1], json!({"role": "user", "content": "open spotify"}));
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "");
        let tool_call = &messages[2]["tool_calls"][0];
        assert_eq!(tool_call["id"], "call_1");
        assert_eq!(tool_call["type"], "function");
        assert_eq!(tool_call["function"]["name"], "open_app");
        let arguments = tool_call["function"]["arguments"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(arguments).unwrap(),
            json!({"name": "Spotify"})
        );
        assert_eq!(
            messages[3],
            json!({"role": "tool", "tool_call_id": "call_1", "content": "Opened Spotify"})
        );
        assert_eq!(
            messages[4],
            json!({"role": "tool", "tool_call_id": "call_2", "content": "Error: no such app"})
        );
        assert_eq!(
            messages[5],
            json!({"role": "assistant", "content": "Done."})
        );
        assert!(messages[5].get("tool_calls").is_none());
        // No signature was ever seen, so nothing Gemini-specific is sent.
        assert!(tool_call.get("extra_content").is_none());
    }

    /// A call whose arguments did not parse is replayed in the repair round;
    /// `"null"` there is refused by gateways that decode an object.
    #[test]
    fn a_call_with_unparseable_arguments_is_replayed_as_an_empty_object() {
        let call = RoutedCall {
            id: "call_1".into(),
            name: "open_app".into(),
            input: Value::Null,
        };
        let req = request(
            vec![
                Turn::User("open spotify".into()),
                Turn::Assistant {
                    text: String::new(),
                    calls: vec![call],
                },
            ],
            true,
        );
        let messages = render_messages(&req, &HashMap::new());
        assert_eq!(messages[2]["tool_calls"][0]["function"]["arguments"], "{}");
    }

    /// A Gemini signature goes back on exactly the call it came with, in the
    /// shape `assistant.rs` sends; a call without one gets no `extra_content`.
    #[test]
    fn a_thought_signature_is_rendered_on_its_own_call_only() {
        let calls = vec![
            RoutedCall {
                id: "call_1".into(),
                name: "open_app".into(),
                input: json!({"name": "Spotify"}),
            },
            RoutedCall {
                id: "call_2".into(),
                name: "open_app".into(),
                input: json!({"name": "Mail"}),
            },
        ];
        let req = request(
            vec![
                Turn::User("open spotify and mail".into()),
                Turn::Assistant {
                    text: String::new(),
                    calls,
                },
            ],
            true,
        );
        let signatures = HashMap::from([("call_1".to_string(), "SIG-1".to_string())]);
        let messages = render_messages(&req, &signatures);
        let tool_calls = &messages[2]["tool_calls"];
        assert_eq!(
            tool_calls[0]["extra_content"],
            json!({"google": {"thought_signature": "SIG-1"}})
        );
        assert!(tool_calls[1].get("extra_content").is_none());
    }
    /// An empty system prompt is not sent as an empty system message.
    #[test]
    fn an_empty_system_prompt_is_left_out() {
        let mut req = request(vec![Turn::User("hi".into())], false);
        req.system.clear();
        let messages = render_messages(&req, &HashMap::new());
        assert_eq!(messages, vec![json!({"role": "user", "content": "hi"})]);
    }

    /// The schema must reach the model untouched under `parameters`.
    #[test]
    fn tools_render_in_the_function_shape() {
        let req = request(vec![], false);
        let tools = render_tools(&req.tools);
        assert_eq!(
            tools,
            json!([{
                "type": "function",
                "function": {
                    "name": "open_app",
                    "description": "Open an app.",
                    "parameters": req.tools[0].input_schema,
                }
            }])
        );
    }

    /// Broken arguments must still arrive as a call so validation can report
    /// them; empty ones mean "no arguments".
    #[test]
    fn arguments_parse_to_values_and_bad_json_becomes_null() {
        let call = |arguments: &str| ToolCall {
            id: "c".into(),
            name: "n".into(),
            arguments: arguments.into(),
            thought_signature: None,
        };
        let routed = routed_calls(vec![call(r#"{"a":1}"#), call("{not json"), call("  ")]);
        assert_eq!(routed[0].input, json!({"a": 1}));
        assert_eq!(routed[1].input, Value::Null);
        assert_eq!(routed[2].input, json!({}));
        assert_eq!(routed[0].id, "c");
        assert_eq!(routed[0].name, "n");
    }

    /// Only a refusal of the parameter itself may trigger the "auto" retry;
    /// auth, rate-limit and unrelated errors must reach the user unchanged.
    #[test]
    fn only_a_refused_tool_choice_counts_as_a_rejection() {
        assert!(tool_choice_rejected(
            r#"API request failed with status 400 Bad Request: {"error":{"message":"tool_choice 'required' is not supported"}}"#
        ));
        assert!(tool_choice_rejected(
            "API request failed with status 422 Unprocessable Entity: Invalid value for tool_choice"
        ));
        assert!(tool_choice_rejected(
            "API request failed with status 400 Bad Request: \"required\" tool mode is unsupported by this model"
        ));
        // Arrives in-stream on some gateways, with no HTTP status at all.
        assert!(tool_choice_rejected("Unknown tool_choice value: required"));

        assert!(!tool_choice_rejected(
            "API request failed with status 401 Unauthorized: invalid api key; tool_choice"
        ));
        assert!(!tool_choice_rejected(
            "API request failed with status 429 Too Many Requests: tool_choice unsupported"
        ));
        assert!(!tool_choice_rejected(
            "API request failed with status 400 Bad Request: invalid model id"
        ));
        assert!(!tool_choice_rejected(
            "API request failed with status 400 Bad Request: field 'messages' is required"
        ));
        assert!(!tool_choice_rejected("HTTP request failed: connection refused"));
        // A tool schema error says "required" and "tool" too, but is not about
        // the choice; retrying with "auto" would fail the same way.
        assert!(!tool_choice_rejected(
            r#"API request failed with status 400 Bad Request: {"error":{"message":"Invalid schema for function 'ask_user': 'required' is required to be supplied and to be an array including every key in properties. Unknown key.","param":"tools[0].function.parameters"}}"#
        ));
        assert!(!tool_choice_rejected(
            "API request failed with status 400 Bad Request: Invalid JSON payload received. Unknown name \"required\" at 'tools[0].function_declarations[0].parameters'"
        ));
    }

    /// The HTTP status must survive into `LlmError::Http` so the report can
    /// say "bad key" rather than "could not reach".
    #[test]
    fn error_strings_map_to_typed_errors() {
        assert_eq!(
            map_error("API request failed with status 401 Unauthorized: bad key".into()),
            LlmError::Http {
                status: 401,
                detail: "bad key".into()
            }
        );
        assert_eq!(
            map_error(
                "API request failed with status 400 Bad Request: {\"error\":{\"message\":\"x: y\"}}"
                    .into()
            ),
            LlmError::Http {
                status: 400,
                detail: "{\"error\":{\"message\":\"x: y\"}}".into()
            }
        );
        assert_eq!(
            map_error("API request failed with status 500 Internal Server Error".into()),
            LlmError::Http {
                status: 500,
                detail: String::new()
            }
        );
        assert_eq!(
            map_error("HTTP request failed: dns error".into()),
            LlmError::Transport("HTTP request failed: dns error".into())
        );
    }

    /// The built-in engine needs an AppHandle to start; it is refused before
    /// any request is sent.
    #[tokio::test]
    async fn the_builtin_engine_is_refused_without_a_request() {
        let router = OpenAiCompatRouter::new(
            provider("builtin", "http://127.0.0.1:9/v1"),
            String::new(),
            "local-model".into(),
        );
        let result = router.route(&request(vec![Turn::User("x".into())], true)).await;
        assert!(matches!(result, Err(LlmError::NotConfigured(_))));
        assert_eq!(router.label(), "builtin/local-model");
        assert_eq!(router.model(), "local-model");
    }

    fn read_request_body(stream: &mut std::net::TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&bytes[..end]).to_string();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes[end + 4..end + 4 + length].to_vec()).unwrap();
            }
        }
        String::new()
    }

    /// Serves `replies` in order on a local port and hands back every request
    /// body it received.
    fn mock_server(
        replies: Vec<(u16, String)>,
    ) -> (
        String,
        std::thread::JoinHandle<()>,
        std::sync::mpsc::Receiver<Value>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request_body(&mut stream);
                tx.send(serde_json::from_str::<Value>(&request).unwrap())
                    .unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status} Response\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (base, server, rx)
    }

    /// A tool call streamed in two fragments, the way OpenAI sends one.
    fn tool_call_stream() -> String {
        let first = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_7","function":{"name":"open_app","arguments":"{\"name\":"}}]}}]});
        let second = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Spotify\"}"}}]}}]});
        format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n")
    }

    /// End to end over a real socket: the request carries the rendered
    /// conversation, the tools and a forced choice, and the streamed call
    /// comes back parsed.
    #[tokio::test]
    async fn a_streamed_tool_call_comes_back_as_a_routed_call() {
        let (base, server, rx) = mock_server(vec![(200, tool_call_stream())]);
        let router = OpenAiCompatRouter::new(
            provider("custom", &base),
            "key".into(),
            "router-e2e-model".into(),
        );
        let req = request(vec![Turn::User("open spotify".into())], true);
        let response = router.route(&req).await.unwrap();
        server.join().unwrap();

        assert_eq!(
            response.calls,
            vec![RoutedCall {
                id: "call_7".into(),
                name: "open_app".into(),
                input: json!({"name": "Spotify"}),
            }]
        );
        assert_eq!(response.text, "");
        assert_eq!(response.usage, Usage::default());

        let sent: Vec<Value> = rx.into_iter().collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["model"], "router-e2e-model");
        assert_eq!(sent[0]["tool_choice"], "required");
        assert_eq!(sent[0]["tools"], render_tools(&req.tools));
        assert_eq!(
            sent[0]["messages"],
            json!(render_messages(&req, &HashMap::new()))
        );
    }

    /// End to end: a signature streamed with a call (Gemini sends it in
    /// `extra_content`) goes back with that call when the next request of the
    /// same command replays it, which is what the repair round does.
    #[tokio::test]
    async fn a_streamed_thought_signature_returns_with_the_replayed_call() {
        let signed = json!({"choices":[{"delta":{"tool_calls":[{
            "index": 0,
            "id": "call_7",
            "function": {"name": "open_app", "arguments": "{\"name\":\"Spotify\"}"},
            "extra_content": {"google": {"thought_signature": "SIG-ABC"}}
        }]}}]});
        let stream = format!("data: {signed}\n\ndata: [DONE]\n\n");
        let (base, server, rx) = mock_server(vec![(200, stream), (200, tool_call_stream())]);
        let router = OpenAiCompatRouter::new(
            provider("custom", &base),
            "key".into(),
            "router-signature-model".into(),
        );
        let first_request = request(vec![Turn::User("open spotify".into())], true);
        let first = router.route(&first_request).await.unwrap();
        // The signature is provider state, not part of the call the router sees.
        assert_eq!(first.calls[0].id, "call_7");

        let repair = request(
            vec![
                Turn::User("open spotify".into()),
                Turn::Assistant {
                    text: String::new(),
                    calls: first.calls.clone(),
                },
                Turn::ToolResults(vec![ToolResult {
                    call_id: "call_7".into(),
                    content: "Not run.".into(),
                    is_error: false,
                }]),
            ],
            true,
        );
        router.route(&repair).await.unwrap();
        server.join().unwrap();

        let sent: Vec<Value> = rx.into_iter().collect();
        assert_eq!(sent.len(), 2);
        assert!(sent[0]["messages"][1].get("tool_calls").is_none());
        assert_eq!(
            sent[1]["messages"][2]["tool_calls"][0]["extra_content"],
            json!({"google": {"thought_signature": "SIG-ABC"}})
        );
    }

    /// A provider that refuses "required" gets one retry with "auto", and the
    /// next command for that model goes straight to "auto".
    #[tokio::test]
    async fn a_refused_required_choice_falls_back_to_auto_and_is_remembered() {
        let refusal = json!({"error":{"message":"tool_choice 'required' is not supported"}})
            .to_string();
        let (base, server, rx) = mock_server(vec![
            (400, refusal),
            (200, tool_call_stream()),
            (200, tool_call_stream()),
        ]);
        let router = OpenAiCompatRouter::new(
            provider("custom", &base),
            "key".into(),
            "router-fallback-model".into(),
        );
        let req = request(vec![Turn::User("open spotify".into())], true);
        let first = router.route(&req).await.unwrap();
        assert_eq!(first.calls.len(), 1);
        let second = router.route(&req).await.unwrap();
        assert_eq!(second.calls.len(), 1);
        server.join().unwrap();

        let sent: Vec<Value> = rx.into_iter().collect();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0]["tool_choice"], "required");
        assert_eq!(sent[1]["tool_choice"], "auto");
        assert_eq!(sent[2]["tool_choice"], "auto");
    }

    /// When the "auto" retry fails too, the error was not about the choice,
    /// so nothing is remembered and the next command still asks for
    /// "required".
    #[tokio::test]
    async fn a_refusal_whose_retry_also_fails_is_not_remembered() {
        let refusal = json!({"error":{"message":"tool_choice 'required' is not supported"}})
            .to_string();
        let (base, server, rx) = mock_server(vec![
            (400, refusal.clone()),
            (400, refusal),
            (200, tool_call_stream()),
        ]);
        let router = OpenAiCompatRouter::new(
            provider("custom", &base),
            "key".into(),
            "router-unremembered-model".into(),
        );
        let req = request(vec![Turn::User("open spotify".into())], true);
        assert!(matches!(
            router.route(&req).await,
            Err(LlmError::Http { status: 400, .. })
        ));
        let second = router.route(&req).await.unwrap();
        assert_eq!(second.calls.len(), 1);
        server.join().unwrap();

        let sent: Vec<Value> = rx.into_iter().collect();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0]["tool_choice"], "required");
        assert_eq!(sent[1]["tool_choice"], "auto");
        assert_eq!(sent[2]["tool_choice"], "required");
    }

    /// Any other HTTP failure is reported with its status and not retried.
    #[tokio::test]
    async fn an_auth_failure_is_reported_without_a_retry() {
        let (base, server, rx) = mock_server(vec![(401, "bad key".into())]);
        let router = OpenAiCompatRouter::new(
            provider("custom", &base),
            "key".into(),
            "router-auth-model".into(),
        );
        let result = router
            .route(&request(vec![Turn::User("x".into())], true))
            .await;
        server.join().unwrap();
        assert_eq!(
            result,
            Err(LlmError::Http {
                status: 401,
                detail: "bad key".into()
            })
        );
        assert_eq!(rx.into_iter().count(), 1);
    }
}
