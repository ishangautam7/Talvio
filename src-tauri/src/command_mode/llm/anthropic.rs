//! The router on Anthropic's native Messages API (`POST /v1/messages`).
//!
//! There is no Rust SDK, so this is raw HTTP over `reqwest`. It exists instead
//! of reusing `crate::llm_client` because that client speaks Anthropic's
//! OpenAI-compatible layer, which has none of the three things a router needs
//! to be cheap and dependable:
//!
//! - **Prompt caching.** The system prompt is sent as one text block carrying
//!   `cache_control: {type: "ephemeral"}`. The API renders `tools`, then
//!   `system`, then `messages`, so that single breakpoint caches the whole tool
//!   list and the system prompt; everything that changes per command (the
//!   context, the words spoken) lives in the user message after it.
//! - **Forced tool choice.** `tool_choice: {type: "any"}` guarantees a call on
//!   models that accept it, and on Claude Haiku 5.5 a forced call also skips
//!   thinking, which is exactly what a router wants. Newer large models reject
//!   it with a 400, so [`accepts_forced_tool_choice`] falls back to `auto` for
//!   them (the system prompt still says to call a tool).
//! - **Usage.** Token counts, including cache reads and writes, come back on
//!   every response and feed the cost log.
//!
//! The request is pinned to API version `2026-10-21`, on which `temperature`,
//! `top_p` and `top_k` are a 400 on every model, so none of them is ever sent.
//! The key goes in `x-api-key` and is never logged.

use std::sync::OnceLock;
use std::time::Duration;

use futures_util::future::BoxFuture;
use log::{debug, info, warn};
use serde_json::{json, Map, Value};

use super::{LlmError, RouteRequest, RouteResponse, RoutedCall, RouterLlm, Turn, Usage};

/// The API version this request shape is written against.
const ANTHROPIC_VERSION: &str = "2026-10-21";

/// Connection setup gets its own short bound so an unreachable host fails
/// fast instead of eating the whole request budget.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A router call is a short, non-streaming request; anything slower than this
/// is a stall the user should hear about rather than wait through.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How much of an unparseable error body is quoted back, so a provider that
/// answers with an HTML page does not flood the log or the toast.
const MAX_ERROR_DETAIL_CHARS: usize = 300;

/// One client for the whole process, so the TLS connection to the API is
/// pooled across commands instead of being set up again for each one.
static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

fn http_client() -> Result<&'static reqwest::Client, LlmError> {
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .map_err(|e| format!("Failed to build HTTP client: {e}"))
        })
        .as_ref()
        .map_err(|e| LlmError::Transport(e.clone()))
}

/// Command mode's router on the native Messages API.
pub struct AnthropicRouter {
    base_url: String,
    api_key: String,
    model: String,
    /// Per-request deadline. A field rather than the constant so tests can
    /// exercise the timeout path without waiting thirty seconds.
    timeout: Duration,
}

impl AnthropicRouter {
    /// `base_url` is the provider's configured base, normally
    /// `https://api.anthropic.com/v1`; `/messages` is appended to it.
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            timeout: REQUEST_TIMEOUT,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/messages", self.base_url.trim_end_matches('/'))
    }

    async fn send(&self, request: &RouteRequest) -> Result<RouteResponse, LlmError> {
        let body = build_request_body(&self.model, request);
        let client = http_client()?;
        debug!(
            "command router: anthropic/{} with {} tools, {} turns, forced={}",
            self.model,
            request.tools.len(),
            request.turns.len(),
            body.get("tool_choice")
                .and_then(|choice| choice.get("type"))
                .and_then(Value::as_str)
                == Some("any")
        );

        let response = client
            .post(self.endpoint())
            .timeout(self.timeout)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status().as_u16();
        let text = response.text().await.map_err(map_reqwest_error)?;

        if !(200..300).contains(&status) {
            let error = http_error(status, &text);
            warn!("command router: anthropic/{} failed: {error}", self.model);
            return Err(error);
        }

        let parsed = parse_response(&text)?;
        info!(
            "command router: anthropic/{} answered {} call(s), stop={:?}, tokens in={} out={} cache_read={} cache_write={}",
            self.model,
            parsed.calls.len(),
            parsed.stop_reason,
            parsed.usage.input_tokens,
            parsed.usage.output_tokens,
            parsed.usage.cache_read_tokens,
            parsed.usage.cache_write_tokens
        );
        Ok(parsed)
    }
}

impl RouterLlm for AnthropicRouter {
    fn route<'a>(
        &'a self,
        request: &'a RouteRequest,
    ) -> BoxFuture<'a, Result<RouteResponse, LlmError>> {
        Box::pin(self.send(request))
    }

    fn label(&self) -> String {
        format!("anthropic/{}", self.model)
    }

    fn model(&self) -> &str {
        &self.model
    }
}

/// The Claude line a model id belongs to, and its version, read from the id.
///
/// Ids come as `claude-haiku-5-5`, dated (`claude-opus-4-5-20251101`), with a
/// platform prefix (`us.anthropic.claude-…`, `anthropic/claude-…`) or in the
/// old order (`claude-3-5-sonnet-20241022`). A segment of eight digits or more
/// is a date, never part of the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Opus,
    Sonnet,
    Haiku,
    Fable,
    Mythos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelId {
    family: Family,
    /// `(major, minor)`; `(0, 0)` when the id carries no version at all.
    version: (u32, u32),
}

fn parse_model_id(model: &str) -> Option<ModelId> {
    let lower = model.trim().to_ascii_lowercase();
    let start = lower.find("claude-")?;
    let rest = &lower[start + "claude-".len()..];
    let segments: Vec<&str> = rest.split(['-', '@', ':']).collect();

    let (index, family) = segments.iter().enumerate().find_map(|(i, segment)| {
        let family = match *segment {
            "opus" => Family::Opus,
            "sonnet" => Family::Sonnet,
            "haiku" => Family::Haiku,
            "fable" => Family::Fable,
            "mythos" => Family::Mythos,
            _ => return None,
        };
        Some((i, family))
    })?;

    let version_from = |parts: &[&str]| -> Option<(u32, u32)> {
        let numbers: Vec<u32> = parts
            .iter()
            .take_while(|part| !part.is_empty() && part.len() < 8)
            .map_while(|part| part.parse::<u32>().ok())
            .take(2)
            .collect();
        match numbers.as_slice() {
            [] => None,
            [major] => Some((*major, 0)),
            [major, minor, ..] => Some((*major, *minor)),
        }
    };

    // New order: the version follows the family. Old order: it precedes it.
    let version = version_from(&segments[index + 1..])
        .or_else(|| version_from(&segments[..index]))
        .unwrap_or((0, 0));
    Some(ModelId { family, version })
}

/// Whether the model accepts `tool_choice` `any`/`tool`.
///
/// A denylist, because forcing works on every older model and on Claude Haiku
/// 5.5, and it is the newer large models that reject it with a 400: Claude
/// Opus 5.5, Claude Sonnet 5.5, Claude Fable 5.1 and Claude Mythos 5.1, and
/// any later model of those lines. An id we cannot read is assumed to accept
/// it, which matches every model that existed before the restriction.
fn accepts_forced_tool_choice(model: &str) -> bool {
    let Some(id) = parse_model_id(model) else {
        return true;
    };
    let rejecting_from = match id.family {
        Family::Opus | Family::Sonnet => (5, 5),
        Family::Fable | Family::Mythos => (5, 1),
        Family::Haiku => return true,
    };
    id.version < rejecting_from
}

/// Whether the model takes `output_config.effort`.
///
/// Effort is an error on models that predate it (Claude Haiku 4.5, Claude
/// Sonnet 4.5 and older), so it is sent only where it is known to work:
/// Claude Opus 4.5 and later, Claude Sonnet 4.6 and later, Claude Haiku 5.5
/// and later, and every Fable and Mythos model. An unreadable id gets none.
fn supports_effort(model: &str) -> bool {
    let Some(id) = parse_model_id(model) else {
        return false;
    };
    match id.family {
        Family::Opus => id.version >= (4, 5),
        Family::Sonnet => id.version >= (4, 6),
        Family::Haiku => id.version >= (5, 5),
        Family::Fable | Family::Mythos => true,
    }
}

/// The JSON body for one router request.
fn build_request_body(model: &str, request: &RouteRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("max_tokens".into(), json!(request.max_tokens));
    body.insert(
        "system".into(),
        json!([{
            "type": "text",
            "text": request.system,
            "cache_control": {"type": "ephemeral"},
        }]),
    );

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));

        // `any` with no tools is a 400, so tool_choice only goes with tools.
        let choice = if request.force_tool && accepts_forced_tool_choice(model) {
            "any"
        } else {
            "auto"
        };
        body.insert("tool_choice".into(), json!({"type": choice}));
    }

    body.insert(
        "messages".into(),
        Value::Array(request.turns.iter().filter_map(render_turn).collect()),
    );

    if supports_effort(model) {
        body.insert("output_config".into(), json!({"effort": "low"}));
    }

    Value::Object(body)
}

/// One [`Turn`] as a Messages API message. An assistant turn with neither
/// text nor calls is dropped: an empty `content` array is a 400, and such a
/// turn says nothing the model needs.
fn render_turn(turn: &Turn) -> Option<Value> {
    match turn {
        Turn::User(text) => Some(json!({"role": "user", "content": text})),
        Turn::Assistant { text, calls } => {
            let mut content = Vec::with_capacity(calls.len() + 1);
            if !text.is_empty() {
                content.push(json!({"type": "text", "text": text}));
            }
            for call in calls {
                // A call whose arguments were unparseable arrives as Null;
                // `input` must be an object, so it is echoed back as `{}`.
                let input = if call.input.is_object() {
                    call.input.clone()
                } else {
                    json!({})
                };
                content.push(json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": input,
                }));
            }
            if content.is_empty() {
                return None;
            }
            Some(json!({"role": "assistant", "content": content}))
        }
        Turn::ToolResults(results) => {
            if results.is_empty() {
                return None;
            }
            let content: Vec<Value> = results
                .iter()
                .map(|result| {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": result.call_id,
                        "content": result.content,
                        "is_error": result.is_error,
                    })
                })
                .collect();
            Some(json!({"role": "user", "content": content}))
        }
    }
}

/// Read a successful (2xx) Messages response.
fn parse_response(text: &str) -> Result<RouteResponse, LlmError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| LlmError::Decode(format!("the response was not JSON: {e}")))?;

    let stop_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(str::to_string);

    if stop_reason.as_deref() == Some("refusal") {
        let details = value.get("stop_details");
        let field = |name: &str| {
            details
                .and_then(|d| d.get(name))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let why = field("explanation")
            .or_else(|| field("category"))
            .unwrap_or_else(|| "refused".to_string());
        return Err(LlmError::Refused(why));
    }

    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| LlmError::Decode("the response had no content array".into()))?;

    let mut text_out = String::new();
    let mut calls = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(part) = block.get("text").and_then(Value::as_str) {
                    text_out.push_str(part);
                }
            }
            Some("tool_use") => {
                let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                calls.push(RoutedCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: block.get("input").cloned().unwrap_or(Value::Null),
                });
            }
            // Thinking (summarized, omitted or redacted) is not part of the
            // answer, and a router never replays it.
            _ => {}
        }
    }

    let usage = value.get("usage");
    let count = |name: &str| {
        usage
            .and_then(|u| u.get(name))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let usage = Usage {
        input_tokens: count("input_tokens"),
        output_tokens: count("output_tokens"),
        cache_read_tokens: count("cache_read_input_tokens"),
        cache_write_tokens: count("cache_creation_input_tokens"),
    };

    Ok(RouteResponse {
        calls,
        text: text_out,
        usage,
        stop_reason,
    })
}

/// A non-2xx answer as an [`LlmError::Http`], with the API's own message
/// (`{"type":"error","error":{"type":…,"message":…}}`) when it sent one.
fn http_error(status: u16, body: &str) -> LlmError {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.chars().count() > MAX_ERROR_DETAIL_CHARS {
                let cut: String = trimmed.chars().take(MAX_ERROR_DETAIL_CHARS).collect();
                format!("{cut}…")
            } else {
                trimmed.to_string()
            }
        });

    let detail = if status == 401 {
        if message.is_empty() {
            "the API key was rejected".to_string()
        } else {
            format!("the API key was rejected ({message})")
        }
    } else if message.is_empty() {
        "no details were given".to_string()
    } else {
        message
    };
    LlmError::Http { status, detail }
}

fn map_reqwest_error(error: reqwest::Error) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout
    } else {
        // reqwest's Display never includes request headers, so the key cannot
        // leak through here.
        LlmError::Transport(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::llm::{ToolDef, ToolResult};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;

    /// A request as the mock server saw it.
    struct Captured {
        request_line: String,
        headers: Vec<(String, String)>,
        body: Value,
    }

    impl Captured {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }
    }

    fn read_request(stream: &mut TcpStream) -> (String, Vec<(String, String)>, String) {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut header_end = None;
        let mut expected_len = None;
        loop {
            let read = stream.read(&mut buffer).unwrap_or(0);
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if header_end.is_none() {
                if let Some(position) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = Some(position + 4);
                    let head = String::from_utf8_lossy(&bytes[..position]);
                    expected_len = head.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    });
                }
            }
            if let (Some(start), Some(length)) = (header_end, expected_len) {
                if bytes.len() >= start + length {
                    break;
                }
            }
        }
        let start = header_end.unwrap_or(bytes.len());
        let head = String::from_utf8_lossy(&bytes[..start]).to_string();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default().to_string();
        let headers = lines
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_string(), value.trim().to_string()))
            })
            .collect();
        let body = String::from_utf8_lossy(&bytes[start..]).to_string();
        (request_line, headers, body)
    }

    /// Serve one request with `status` and `reply`, and hand back what came in.
    fn spawn_mock(status: u16, reply: String) -> (String, mpsc::Receiver<Captured>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (request_line, headers, body) = read_request(&mut stream);
            let _ = sender.send(Captured {
                request_line,
                headers,
                body: serde_json::from_str(&body).unwrap_or(Value::Null),
            });
            let response = format!(
                "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (format!("http://{address}/v1/"), receiver)
    }

    fn tool(name: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: format!("The {name} tool."),
            input_schema: json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"],
                "additionalProperties": false,
            }),
        }
    }

    fn two_tool_request(force_tool: bool) -> RouteRequest {
        RouteRequest {
            system: "You route commands.".to_string(),
            tools: vec![tool("open_app"), tool("web_search")],
            turns: vec![Turn::User("open spotify".to_string())],
            force_tool,
            max_tokens: 512,
        }
    }

    fn ok_reply() -> String {
        json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "tool_use", "id": "toolu_1", "name": "open_app", "input": {"query": "spotify"}}],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        })
        .to_string()
    }

    fn contains_key(value: &Value, key: &str) -> bool {
        match value {
            Value::Object(map) => map.iter().any(|(k, v)| k == key || contains_key(v, key)),
            Value::Array(items) => items.iter().any(|v| contains_key(v, key)),
            _ => false,
        }
    }

    /// The wire shape is the contract with the API: one cached system block,
    /// tools in order, forced choice on Haiku 5.5, the pinned version header,
    /// and no sampling parameter anywhere (a 400 on 2026-10-21).
    #[test]
    fn a_haiku_request_carries_the_cached_prefix_forced_choice_and_headers() {
        let (base, captured) = spawn_mock(200, ok_reply());
        let router = AnthropicRouter::new(
            base,
            "sk-test-key".to_string(),
            "claude-haiku-5-5".to_string(),
        );
        let response =
            tauri::async_runtime::block_on(router.route(&two_tool_request(true))).unwrap();
        assert_eq!(response.calls.len(), 1);

        let seen = captured.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(seen.request_line, "POST /v1/messages HTTP/1.1");
        assert_eq!(seen.header("x-api-key"), Some("sk-test-key"));
        assert_eq!(seen.header("anthropic-version"), Some("2026-10-21"));
        assert!(seen
            .header("content-type")
            .is_some_and(|v| v.starts_with("application/json")));

        let body = &seen.body;
        assert_eq!(body["model"], "claude-haiku-5-5");
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(
            body["system"],
            json!([{"type": "text", "text": "You route commands.", "cache_control": {"type": "ephemeral"}}])
        );
        let names: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["open_app", "web_search"]);
        assert_eq!(body["tools"][0]["description"], "The open_app tool.");
        assert_eq!(
            body["tools"][0]["input_schema"]["required"],
            json!(["query"])
        );
        assert_eq!(body["tool_choice"], json!({"type": "any"}));
        assert_eq!(body["output_config"], json!({"effort": "low"}));
        assert_eq!(
            body["messages"],
            json!([{"role": "user", "content": "open spotify"}])
        );
        for key in ["temperature", "top_p", "top_k"] {
            assert!(!contains_key(body, key), "{key} must never be sent");
        }
    }

    /// Sonnet 5.5 answers forced tool choice with a 400, so a forced request
    /// falls back to auto there instead of failing every command.
    #[test]
    fn a_sonnet_5_5_request_asks_for_auto_even_when_forced() {
        let body = build_request_body("claude-sonnet-5-5", &two_tool_request(true));
        assert_eq!(body["tool_choice"], json!({"type": "auto"}));
        assert_eq!(body["output_config"], json!({"effort": "low"}));
        assert!(!contains_key(&body, "temperature"));
    }

    /// An unforced request is auto on any model.
    #[test]
    fn an_unforced_request_is_auto() {
        let body = build_request_body("claude-haiku-5-5", &two_tool_request(false));
        assert_eq!(body["tool_choice"], json!({"type": "auto"}));
    }

    /// Effort errors on models that predate it, so Haiku 4.5 gets none.
    #[test]
    fn a_model_without_effort_gets_no_output_config() {
        let body = build_request_body("claude-haiku-4-5", &two_tool_request(true));
        assert!(body.get("output_config").is_none());
        assert_eq!(body["tool_choice"], json!({"type": "any"}));
    }

    /// `tool_choice: any` with no tools is a 400, so neither is sent.
    #[test]
    fn a_request_without_tools_sends_no_tool_choice() {
        let mut request = two_tool_request(true);
        request.tools.clear();
        let body = build_request_body("claude-haiku-5-5", &request);
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    /// Every turn kind renders as the API expects, including a failed tool
    /// result keeping its `is_error`, which is how the model learns a call
    /// did not work.
    #[test]
    fn turns_render_with_tool_use_and_tool_results() {
        let turns = vec![
            Turn::User("email sam".to_string()),
            Turn::Assistant {
                text: "Looking up Sam.".to_string(),
                calls: vec![
                    RoutedCall {
                        id: "toolu_a".into(),
                        name: "find_contact".into(),
                        input: json!({"name": "sam"}),
                    },
                    RoutedCall {
                        id: "toolu_b".into(),
                        name: "broken".into(),
                        input: Value::Null,
                    },
                ],
            },
            Turn::ToolResults(vec![
                ToolResult {
                    call_id: "toolu_a".into(),
                    content: "sam@example.com".into(),
                    is_error: false,
                },
                ToolResult {
                    call_id: "toolu_b".into(),
                    content: "arguments were not valid".into(),
                    is_error: true,
                },
            ]),
            Turn::Assistant {
                text: String::new(),
                calls: vec![RoutedCall {
                    id: "toolu_c".into(),
                    name: "say".into(),
                    input: json!({"text": "done"}),
                }],
            },
        ];
        let rendered: Vec<Value> = turns.iter().filter_map(render_turn).collect();
        assert_eq!(
            rendered,
            vec![
                json!({"role": "user", "content": "email sam"}),
                json!({"role": "assistant", "content": [
                    {"type": "text", "text": "Looking up Sam."},
                    {"type": "tool_use", "id": "toolu_a", "name": "find_contact", "input": {"name": "sam"}},
                    {"type": "tool_use", "id": "toolu_b", "name": "broken", "input": {}},
                ]}),
                json!({"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_a", "content": "sam@example.com", "is_error": false},
                    {"type": "tool_result", "tool_use_id": "toolu_b", "content": "arguments were not valid", "is_error": true},
                ]}),
                json!({"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_c", "name": "say", "input": {"text": "done"}},
                ]}),
            ]
        );
    }

    /// An empty assistant turn would be an empty content array, a 400.
    #[test]
    fn an_empty_assistant_turn_is_dropped() {
        let turn = Turn::Assistant {
            text: String::new(),
            calls: vec![],
        };
        assert_eq!(render_turn(&turn), None);
    }

    /// Text is concatenated, every tool_use becomes a call in order, thinking
    /// is ignored, and the cache counters map onto Usage.
    #[test]
    fn a_response_with_text_thinking_and_several_calls_parses() {
        let text = json!({
            "content": [
                {"type": "thinking", "thinking": "", "signature": "abc"},
                {"type": "text", "text": "Opening "},
                {"type": "tool_use", "id": "toolu_1", "name": "open_app", "input": {"name": "Spotify"}},
                {"type": "redacted_thinking", "data": "xyz"},
                {"type": "text", "text": "both."},
                {"type": "tool_use", "id": "toolu_2", "name": "media", "input": {"action": "play"}},
            ],
            "stop_reason": "tool_use",
            "stop_details": null,
            "usage": {
                "input_tokens": 42,
                "output_tokens": 17,
                "cache_read_input_tokens": 900,
                "cache_creation_input_tokens": 12,
            },
        })
        .to_string();
        let parsed = parse_response(&text).unwrap();
        assert_eq!(parsed.text, "Opening both.");
        assert_eq!(
            parsed.calls,
            vec![
                RoutedCall {
                    id: "toolu_1".into(),
                    name: "open_app".into(),
                    input: json!({"name": "Spotify"}),
                },
                RoutedCall {
                    id: "toolu_2".into(),
                    name: "media".into(),
                    input: json!({"action": "play"}),
                },
            ]
        );
        assert_eq!(
            parsed.usage,
            Usage {
                input_tokens: 42,
                output_tokens: 17,
                cache_read_tokens: 900,
                cache_write_tokens: 12,
            }
        );
        assert_eq!(parsed.stop_reason.as_deref(), Some("tool_use"));
    }

    /// Cache counters are absent on a request that cached nothing; that is
    /// zero, not an error.
    #[test]
    fn missing_usage_fields_count_as_zero() {
        let parsed = parse_response(r#"{"content":[],"stop_reason":"end_turn"}"#).unwrap();
        assert_eq!(parsed.usage, Usage::default());
        assert!(parsed.calls.is_empty());
    }

    /// A refusal arrives as HTTP 200; it must become an error with the
    /// explanation, falling back to the category, then a plain word.
    #[test]
    fn a_refusal_becomes_refused_with_its_explanation() {
        let with_explanation = json!({
            "content": [],
            "stop_reason": "refusal",
            "stop_details": {"type": "refusal", "category": "cyber", "explanation": "Not able to help with that."},
        });
        assert_eq!(
            parse_response(&with_explanation.to_string()),
            Err(LlmError::Refused("Not able to help with that.".into()))
        );
        let category_only = json!({
            "content": [],
            "stop_reason": "refusal",
            "stop_details": {"type": "refusal", "category": "bio", "explanation": null},
        });
        assert_eq!(
            parse_response(&category_only.to_string()),
            Err(LlmError::Refused("bio".into()))
        );
        let bare = json!({"content": [], "stop_reason": "refusal"});
        assert_eq!(
            parse_response(&bare.to_string()),
            Err(LlmError::Refused("refused".into()))
        );
    }

    /// A 400 quotes the API's own message, which is what says which field
    /// was wrong.
    #[test]
    fn a_400_carries_the_api_error_message() {
        let reply = json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "tool_choice: type \"any\" is not supported for this model."},
        })
        .to_string();
        let (base, _captured) = spawn_mock(400, reply);
        let router = AnthropicRouter::new(base, "k".into(), "claude-haiku-5-5".into());
        let error =
            tauri::async_runtime::block_on(router.route(&two_tool_request(true))).unwrap_err();
        assert_eq!(
            error,
            LlmError::Http {
                status: 400,
                detail: "tool_choice: type \"any\" is not supported for this model.".into(),
            }
        );
    }

    /// A 401 says the key was rejected, so the user knows what to fix.
    #[test]
    fn a_401_says_the_key_was_rejected() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(
            http_error(401, body),
            LlmError::Http {
                status: 401,
                detail: "the API key was rejected (invalid x-api-key)".into(),
            }
        );
    }

    /// A non-JSON error body is quoted, but only so much of it.
    #[test]
    fn a_non_json_error_body_is_truncated() {
        let page = "x".repeat(1000);
        let LlmError::Http { status, detail } = http_error(502, &page) else {
            panic!("expected an HTTP error");
        };
        assert_eq!(status, 502);
        assert_eq!(detail.chars().count(), MAX_ERROR_DETAIL_CHARS + 1);
    }

    /// A 200 that is not JSON is a decode error, not an empty answer.
    #[test]
    fn a_body_that_is_not_json_is_a_decode_error() {
        assert!(matches!(parse_response("<html>"), Err(LlmError::Decode(_))));
    }

    /// A server that never answers ends in Timeout, not a hang.
    #[test]
    fn a_silent_server_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(1500));
            drop(stream);
        });
        let mut router = AnthropicRouter::new(
            format!("http://{address}/v1"),
            "k".into(),
            "claude-haiku-5-5".into(),
        );
        router.timeout = Duration::from_millis(300);
        let error =
            tauri::async_runtime::block_on(router.route(&two_tool_request(true))).unwrap_err();
        assert_eq!(error, LlmError::Timeout);
        let _ = server.join();
    }

    /// Label and model feed the logs and the price table.
    #[test]
    fn label_names_the_provider_and_model() {
        let router = AnthropicRouter::new(
            "https://api.anthropic.com/v1".into(),
            "k".into(),
            "claude-haiku-5-5".into(),
        );
        assert_eq!(router.label(), "anthropic/claude-haiku-5-5");
        assert_eq!(router.model(), "claude-haiku-5-5");
        assert_eq!(router.endpoint(), "https://api.anthropic.com/v1/messages");
    }

    /// The denylist covers the four lines that 400 on forced choice and
    /// anything later in them, dated and prefixed ids included.
    #[test]
    fn forced_tool_choice_follows_the_model_table() {
        let cases = [
            ("claude-haiku-5-5", true),
            ("claude-haiku-4-5", true),
            ("claude-haiku-4-5-20251001", true),
            ("claude-sonnet-5", true),
            ("claude-sonnet-4-6", true),
            ("claude-opus-5", true),
            ("claude-opus-4-8", true),
            ("claude-fable-5", true),
            ("claude-mythos-5", true),
            ("claude-3-5-sonnet-20241022", true),
            ("some-other-model", true),
            ("claude-opus-5-5", false),
            ("claude-opus-5-5-20261001", false),
            ("claude-opus-6", false),
            ("claude-sonnet-5-5", false),
            ("us.anthropic.claude-sonnet-5-5", false),
            ("claude-sonnet-5-6", false),
            ("claude-fable-5-1", false),
            ("claude-fable-6", false),
            ("claude-mythos-5-1", false),
            ("Claude-Opus-5-5", false),
        ];
        for (model, expected) in cases {
            assert_eq!(accepts_forced_tool_choice(model), expected, "{model}");
        }
    }

    /// Effort is sent only where it exists; elsewhere it is an error.
    #[test]
    fn effort_support_follows_the_model_table() {
        let cases = [
            ("claude-haiku-5-5", true),
            ("claude-sonnet-5", true),
            ("claude-sonnet-5-5", true),
            ("claude-opus-5", true),
            ("claude-opus-5-5", true),
            ("claude-fable-5", true),
            ("claude-fable-5-1", true),
            ("claude-mythos-5-1", true),
            ("claude-mythos-preview", true),
            ("claude-opus-4-5", true),
            ("claude-opus-4-5-20251101", true),
            ("claude-opus-4-6", true),
            ("claude-opus-4-7", true),
            ("claude-opus-4-8", true),
            ("claude-sonnet-4-6", true),
            ("claude-haiku-4-5", false),
            ("claude-haiku-4-5-20251001", false),
            ("claude-sonnet-4-5", false),
            ("claude-sonnet-4-20250514", false),
            ("claude-opus-4-1", false),
            ("claude-3-7-sonnet-20250219", false),
            ("gpt-5", false),
        ];
        for (model, expected) in cases {
            assert_eq!(supports_effort(model), expected, "{model}");
        }
    }
}
