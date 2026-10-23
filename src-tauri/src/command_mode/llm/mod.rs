//! The LLM behind command mode, as a provider interface.
//!
//! Tier 1 needs one thing from a model: given a system prompt, a tool list and
//! a short conversation, return tool calls. [`RouterLlm`] is that, with two
//! implementations:
//!
//! - [`anthropic::AnthropicRouter`] speaks the native Messages API
//!   (`POST /v1/messages`). It exists because the OpenAI-compatible layer the
//!   rest of the app uses for Anthropic has no prompt caching, no forced
//!   `tool_choice` and no computer-use toolset, and reports no usage.
//! - [`openai_compat::OpenAiCompatRouter`] wraps `crate::llm_client` for every
//!   other configured provider.
//!
//! [`router_for`] picks one from the settings. The conversation is kept in a
//! provider-neutral shape ([`Turn`]) and each transport renders it its own way.

pub mod anthropic;
pub mod openai_compat;

use futures_util::future::BoxFuture;
use serde::Serialize;
use serde_json::Value;
use specta::Type;
use std::fmt;

use crate::settings::AppSettings;

/// A tool as the model sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub input_schema: Value,
}

/// One tool call the model made.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoutedCall {
    /// The provider's id for this call, echoed back with its result.
    pub id: String,
    pub name: String,
    /// The arguments, parsed. A call whose arguments were not valid JSON
    /// arrives as `Value::Null`, so validation reports it rather than the
    /// transport dropping it.
    pub input: Value,
}

/// A step in the router conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    User(String),
    Assistant {
        text: String,
        calls: Vec<RoutedCall>,
    },
    /// Results for the calls of the preceding [`Turn::Assistant`], by call id.
    ToolResults(Vec<ToolResult>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
}

/// One router request.
#[derive(Debug, Clone)]
pub struct RouteRequest {
    /// Byte-stable across requests: the cached prefix starts here.
    pub system: String,
    /// In a deterministic order, for the same reason.
    pub tools: Vec<ToolDef>,
    pub turns: Vec<Turn>,
    /// Require at least one tool call. A transport whose model refuses forced
    /// tool choice falls back to "auto" (the system prompt still says to call
    /// a tool) rather than failing.
    pub force_tool: bool,
    pub max_tokens: u32,
}

/// Token counts for one request. Integers so outcomes can derive `Eq`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
    }
}

/// What the model answered.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteResponse {
    pub calls: Vec<RoutedCall>,
    /// Any text alongside the calls (or instead of them).
    pub text: String,
    pub usage: Usage,
    /// The provider's stop reason (`tool_use`, `end_turn`, `max_tokens`,
    /// `refusal`, …) when it gave one.
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmError {
    /// No key saved for a provider that needs one.
    MissingApiKey { provider: String },
    /// The provider or model is not set, or cannot route (no HTTP endpoint).
    NotConfigured(String),
    Http { status: u16, detail: String },
    Transport(String),
    Decode(String),
    /// The model declined (Anthropic `stop_reason: "refusal"`).
    Refused(String),
    Timeout,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::MissingApiKey { provider } => {
                write!(f, "No API key is saved for {provider}")
            }
            LlmError::NotConfigured(why) => write!(f, "Command mode is not set up: {why}"),
            LlmError::Http { status, detail } => {
                write!(f, "The provider answered HTTP {status}: {detail}")
            }
            LlmError::Transport(why) => write!(f, "Could not reach the provider: {why}"),
            LlmError::Decode(why) => write!(f, "Could not read the provider's answer: {why}"),
            LlmError::Refused(why) => write!(f, "The model declined: {why}"),
            LlmError::Timeout => write!(f, "The provider took too long to answer"),
        }
    }
}

/// A model that can route a command to tools.
pub trait RouterLlm: Send + Sync {
    fn route<'a>(&'a self, request: &'a RouteRequest)
        -> BoxFuture<'a, Result<RouteResponse, LlmError>>;

    /// "provider/model", for logs and the cost table.
    fn label(&self) -> String;

    /// The model id, for the price table.
    fn model(&self) -> &str;
}

/// Build the router the settings ask for: native Anthropic when the provider
/// is `anthropic`, the OpenAI-compatible client for any other HTTP provider.
pub fn router_for(settings: &AppSettings) -> Result<Box<dyn RouterLlm>, LlmError> {
    let provider_id = settings.command_router_provider_id.trim();
    let model = settings.command_router_model.trim();
    if provider_id.is_empty() {
        return Err(LlmError::NotConfigured("no router provider is selected".into()));
    }
    if model.is_empty() {
        return Err(LlmError::NotConfigured("no router model is selected".into()));
    }
    let provider = settings
        .post_process_provider(provider_id)
        .cloned()
        .ok_or_else(|| LlmError::NotConfigured(format!("unknown provider `{provider_id}`")))?;
    let api_key = settings
        .post_process_api_keys
        .get(provider_id)
        .cloned()
        .unwrap_or_default();

    if provider.id == "anthropic" {
        if api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: provider.label.clone(),
            });
        }
        return Ok(Box::new(anthropic::AnthropicRouter::new(
            provider.base_url.clone(),
            api_key,
            model.to_string(),
        )));
    }
    if provider.id == "apple_intelligence" || !provider.base_url.starts_with("http") {
        return Err(LlmError::NotConfigured(format!(
            "{} cannot route commands (it has no tool-calling endpoint)",
            provider.label
        )));
    }
    if api_key.trim().is_empty()
        && crate::settings::post_process_provider_requires_api_key(&provider.id)
    {
        return Err(LlmError::MissingApiKey {
            provider: provider.label.clone(),
        });
    }
    Ok(Box::new(openai_compat::OpenAiCompatRouter::new(
        provider,
        api_key,
        model.to_string(),
    )))
}
