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
    MissingApiKey {
        provider: String,
    },
    /// The provider or model is not set, or cannot route (no HTTP endpoint).
    NotConfigured(String),
    Http {
        status: u16,
        detail: String,
    },
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
    fn route<'a>(
        &'a self,
        request: &'a RouteRequest,
    ) -> BoxFuture<'a, Result<RouteResponse, LlmError>>;

    /// "provider/model", for logs and the cost table.
    fn label(&self) -> String;

    /// The model id, for the price table.
    fn model(&self) -> &str;
}

/// Build the router the settings ask for: native Anthropic when the provider
/// is `anthropic`, the OpenAI-compatible client for any other HTTP provider.
pub fn router_for(settings: &AppSettings) -> Result<Box<dyn RouterLlm>, LlmError> {
    let model = settings.command_router_model.trim();
    if model.is_empty() {
        return Err(LlmError::NotConfigured(
            "no router model is selected".into(),
        ));
    }
    let (provider_id, model) = effective(settings, model);
    llm_for(settings, &provider_id, &model)
}

/// The provider and model a command actually runs on: the ones chosen for
/// command mode while that provider can be used (it has a key, or needs
/// none), otherwise the assistant's own provider and model. Someone who set
/// the assistant up with one provider should not have to find a second key
/// before "open YouTube" works; the default router is Anthropic, which many
/// people have no key for.
pub(crate) fn effective(settings: &AppSettings, model: &str) -> (String, String) {
    effective_with(
        settings,
        model,
        crate::free_credit::signed_in(),
        crate::free_credit::usable(),
    )
}

/// [`effective`] with the free credit's state given: `free_signed_in` lets a
/// router the user pointed at Talvio free run, and `free_usable` makes Talvio
/// free the last resort after every provider the user set up a key for.
pub(crate) fn effective_with(
    settings: &AppSettings,
    model: &str,
    free_signed_in: bool,
    free_usable: bool,
) -> (String, String) {
    let free = crate::free_credit::PROVIDER_ID;
    let usable = |provider: &str| {
        if provider == free {
            free_signed_in
        } else {
            usable(settings, provider)
        }
    };
    let chosen = settings.command_router_provider_id.trim();
    if usable(chosen) {
        let model = if chosen == free {
            crate::free_credit::CHAT_MODEL
        } else {
            model
        };
        return (chosen.to_string(), model.to_string());
    }
    // Another provider set up with a key: the assistant's, then AI cleanup's.
    let elsewhere = [
        (
            "assistant",
            settings.assistant_provider_id.trim(),
            &settings.assistant_models,
        ),
        (
            "cleanup",
            settings.post_process_provider_id.trim(),
            &settings.post_process_models,
        ),
    ];
    for (feature, provider, models) in elsewhere {
        let other_model = models.get(provider).map(|m| m.trim()).unwrap_or_default();
        if provider != chosen && usable(provider) && !other_model.is_empty() {
            log::info!(
                "Command mode has no key for `{chosen}`; using the {feature}'s `{provider}/{other_model}`"
            );
            return (provider.to_string(), other_model.to_string());
        }
    }
    // Nothing the user set up can route: the account's free credit can.
    if free_usable {
        log::info!("Command mode has no key for `{chosen}`; using Talvio free");
        return (free.to_string(), crate::free_credit::CHAT_MODEL.to_string());
    }
    (chosen.to_string(), model.to_string())
}

/// Whether `provider_id` can take a tool call: an HTTP provider with a key,
/// or one that needs none.
pub(crate) fn usable(settings: &AppSettings, provider_id: &str) -> bool {
    let Some(provider) = settings.post_process_provider(provider_id) else {
        return false;
    };
    if provider.id == "apple_intelligence" || !provider.base_url.starts_with("http") {
        return false;
    }
    if provider.id == crate::free_credit::PROVIDER_ID {
        return crate::free_credit::signed_in();
    }
    let has_key = settings
        .post_process_api_keys
        .get(provider_id)
        .is_some_and(|key| !key.trim().is_empty());
    has_key || !crate::settings::post_process_provider_requires_api_key(&provider.id)
}

/// Build the model the tier-2 agent runs on: the router's provider and key,
/// with `command_agent_model` (a stronger model than the router's, since it
/// plans over many steps). An empty agent model means the default.
pub fn agent_llm_for(settings: &AppSettings) -> Result<Box<dyn RouterLlm>, LlmError> {
    let (provider_id, model) = agent_choice(settings);
    llm_for(settings, &provider_id, &model)
}

/// The provider and model the agent runs on. The default agent model is an
/// Anthropic id, which means nothing to any other provider: on Azure it was
/// asked for as a deployment that does not exist (HTTP 404
/// `DeploymentNotFound`), so every command that fell through to the agent
/// failed. Off Anthropic, an unset or default agent model therefore means the
/// router's own model, which that provider is known to serve.
pub(crate) fn agent_choice(settings: &AppSettings) -> (String, String) {
    let configured = settings.command_agent_model.trim();
    let default = crate::settings::DEFAULT_COMMAND_AGENT_MODEL;
    let wanted = if configured.is_empty() {
        default
    } else {
        configured
    };
    let (provider_id, model) = effective(settings, wanted);
    if provider_id == "anthropic" || (wanted != default && model == wanted) {
        return (provider_id, model);
    }
    // Not Anthropic, and the agent model is the Anthropic default (or the
    // assistant's model already replaced it): run on the router's model.
    effective(settings, settings.command_router_model.trim())
}

/// [`router_for`] and [`agent_llm_for`] with the model already chosen.
fn llm_for(
    settings: &AppSettings,
    provider_id: &str,
    model: &str,
) -> Result<Box<dyn RouterLlm>, LlmError> {
    if provider_id.is_empty() {
        return Err(LlmError::NotConfigured(
            "no router provider is selected".into(),
        ));
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
    if provider.id == crate::free_credit::PROVIDER_ID && !crate::free_credit::signed_in() {
        return Err(LlmError::NotConfigured(
            crate::free_credit::SIGN_IN_MESSAGE.to_string(),
        ));
    }
    Ok(Box::new(openai_compat::OpenAiCompatRouter::new(
        provider,
        api_key,
        model.to_string(),
    )))
}

#[cfg(test)]
mod agent_choice_tests {
    use super::agent_choice;
    use crate::settings::{get_default_settings, AppSettings};

    fn with_key(provider: &str, router_model: &str) -> AppSettings {
        let mut settings = get_default_settings();
        settings.command_router_provider_id = provider.into();
        settings.command_router_model = router_model.into();
        settings
            .post_process_api_keys
            .insert(provider.into(), "key".into());
        settings
    }

    /// The Anthropic default agent model means nothing on Azure: the agent
    /// runs on the router's model there (the real failure was HTTP 404
    /// DeploymentNotFound for `claude-sonnet-5-5`).
    #[test]
    fn off_anthropic_the_default_agent_model_becomes_the_routers() {
        let mut settings = with_key("azure_openai", "gpt-5.6-luna");
        if settings.post_process_provider("azure_openai").is_none() {
            return; // no Azure in this build's provider list
        }
        settings.command_agent_model = String::new();
        assert_eq!(
            agent_choice(&settings),
            ("azure_openai".into(), "gpt-5.6-luna".into())
        );
        settings.command_agent_model = crate::settings::DEFAULT_COMMAND_AGENT_MODEL.into();
        assert_eq!(agent_choice(&settings).1, "gpt-5.6-luna");
        // A model the user picked for that provider is kept.
        settings.command_agent_model = "gpt-5.6".into();
        assert_eq!(agent_choice(&settings).1, "gpt-5.6");
    }

    #[test]
    fn on_anthropic_the_default_agent_model_stays() {
        let settings = with_key("anthropic", "claude-haiku-5-5");
        let (provider, model) = agent_choice(&settings);
        assert_eq!(provider, "anthropic");
        assert_eq!(model, crate::settings::DEFAULT_COMMAND_AGENT_MODEL);
    }
}

#[cfg(test)]
mod free_credit_tests {
    use super::effective_with;
    use crate::free_credit::{CHAT_MODEL, PROVIDER_ID};
    use crate::settings::get_default_settings;

    #[test]
    fn talvio_free_is_the_last_resort() {
        let mut settings = get_default_settings();
        settings.command_router_provider_id = "anthropic".into();
        // Nothing set up, free credit available: Talvio free routes.
        assert_eq!(
            effective_with(&settings, "claude-haiku-5-5", true, true),
            (PROVIDER_ID.into(), CHAT_MODEL.into())
        );
        // Signed out, or out of credit: the user's choice stays (and says
        // what it needs).
        assert_eq!(
            effective_with(&settings, "claude-haiku-5-5", false, false).0,
            "anthropic"
        );
        assert_eq!(
            effective_with(&settings, "claude-haiku-5-5", true, false).0,
            "anthropic"
        );
        // A provider the user has a key for always wins over the free credit.
        settings.post_process_provider_id = "openai".into();
        settings
            .post_process_models
            .insert("openai".into(), "gpt-5-mini".into());
        settings
            .post_process_api_keys
            .insert("openai".into(), "key".into());
        assert_eq!(
            effective_with(&settings, "claude-haiku-5-5", true, true),
            ("openai".into(), "gpt-5-mini".into())
        );
        settings
            .post_process_api_keys
            .insert("anthropic".into(), "key".into());
        assert_eq!(
            effective_with(&settings, "claude-haiku-5-5", true, true).0,
            "anthropic"
        );
    }

    #[test]
    fn a_router_pointed_at_it_runs_while_signed_in() {
        let mut settings = get_default_settings();
        settings.command_router_provider_id = PROVIDER_ID.into();
        assert_eq!(
            effective_with(&settings, "claude-haiku-5-5", true, false),
            (PROVIDER_ID.into(), CHAT_MODEL.into())
        );
    }
}

#[cfg(test)]
mod effective_tests {
    use super::effective;
    use crate::settings::get_default_settings;

    #[test]
    fn a_keyless_choice_runs_on_the_assistant_then_on_cleanup() {
        let mut settings = get_default_settings();
        settings.command_router_provider_id = "anthropic".into();
        settings.assistant_provider_id = "azure_openai".into();
        settings.post_process_provider_id = "openai".into();
        settings
            .assistant_models
            .insert("azure_openai".into(), "gpt-5.6-luna".into());
        settings
            .post_process_models
            .insert("openai".into(), "gpt-5-mini".into());
        settings
            .post_process_api_keys
            .insert("openai".into(), "key".into());
        assert_eq!(
            effective(&settings, "claude-haiku-5-5"),
            ("openai".into(), "gpt-5-mini".into())
        );

        settings
            .post_process_api_keys
            .insert("azure_openai".into(), "key".into());
        assert_eq!(
            effective(&settings, "claude-haiku-5-5"),
            ("azure_openai".into(), "gpt-5.6-luna".into())
        );

        settings
            .post_process_api_keys
            .insert("anthropic".into(), "key".into());
        assert_eq!(
            effective(&settings, "claude-haiku-5-5"),
            ("anthropic".into(), "claude-haiku-5-5".into())
        );
    }
}
