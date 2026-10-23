//! A phone asking this computer's assistant (`assistant.ask`, `PROTOCOL.md`,
//! "Assistant").
//!
//! The same assistant as the quick ask: provider, model, profile, memory, web
//! search and tools, `do_on_computer` included while command mode is on. It
//! runs headless: nothing here touches the panel window, its conversation, the
//! voice, or the screen (there is no `capture_screen`, since nobody at the
//! computer asked for a look). Each phone keeps its own conversation
//! ([`PhoneConversations`]): one per phone, forgotten 30 minutes after its last
//! question, never mixed with the quick ask's or a call's.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use log::{debug, info, warn};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::UnboundedSender;

use super::turn::{self, TurnModel, TurnTools};
use super::{act, PromptPlan};
use crate::command_mode::{CommandOrigin, CommandStatus};
use crate::llm_client::{self, ChatMessage, ToolCall};
use crate::settings::{get_settings, AppSettings};

/// How long a phone's conversation is kept after its last question.
pub const CONVERSATION_TTL: Duration = Duration::from_secs(30 * 60);
/// The most messages a phone's conversation keeps; older ones are dropped.
const MAX_KEPT_MESSAGES: usize = 40;
/// How often the answer so far may be reported.
const ANSWER_PROGRESS_EVERY: Duration = Duration::from_millis(100);
/// The longest label a tool's progress carries.
const MAX_LABEL_CHARS: usize = 80;

/// What a phone's question is doing, for its `progress` messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhoneProgress {
    /// A tool is running: "Searching the web for …", "Play believer on youtube".
    Tool(String),
    /// The whole answer so far.
    Answer(String),
}

/// One thing `do_on_computer` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneAction {
    pub label: String,
    /// The command's status: `done`, `failed`, `awaiting_confirmation`, …
    pub status: String,
}

/// A command waiting for Do it, confirmed with `command.confirm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonePending {
    pub id: u32,
    pub summary: String,
}

/// The answer to a phone's question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneAnswer {
    /// Markdown.
    pub answer: String,
    /// The conversation it belongs to, for the next question.
    pub conversation: String,
    pub actions: Vec<PhoneAction>,
    pub pending: Option<PhonePending>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhoneAskError {
    /// No provider or model.
    NotConfigured,
    /// The model call failed; why, in English.
    Failed(String),
}

/// One phone's question.
pub struct PhoneAsk {
    pub phone: String,
    pub conversation: Option<String>,
    pub text: String,
    pub progress: UnboundedSender<PhoneProgress>,
}

/// One phone's conversation.
struct PhoneConversation {
    id: String,
    messages: Vec<ChatMessage>,
    last: Instant,
}

/// Every phone's conversation, by phone id. Pure apart from the clock, which
/// is a parameter.
#[derive(Default)]
pub struct PhoneConversations {
    by_phone: HashMap<String, PhoneConversation>,
}

static CONVERSATIONS: Mutex<Option<PhoneConversations>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A conversation id: short, opaque and not reused within a run.
fn new_conversation_id() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("c{n:x}{nanos:08x}")
}

impl PhoneConversations {
    /// Forget conversations whose last question is older than the TTL.
    fn prune(&mut self, now: Instant) {
        self.by_phone
            .retain(|_, c| now.saturating_duration_since(c.last) < CONVERSATION_TTL);
    }

    /// The conversation a question joins, and what was said in it so far: the
    /// phone's own when `requested` names it and it is still kept, otherwise a
    /// new one (with a new id). A new one replaces the phone's old one only
    /// once it has an answer ([`Self::record`]).
    pub fn open(
        &mut self,
        phone: &str,
        requested: Option<&str>,
        now: Instant,
    ) -> (String, Vec<ChatMessage>) {
        self.prune(now);
        match (self.by_phone.get(phone), requested) {
            (Some(kept), Some(requested)) if kept.id == requested => {
                (kept.id.clone(), kept.messages.clone())
            }
            _ => (new_conversation_id(), Vec::new()),
        }
    }

    /// Keep a question and its answer in conversation `id`, which becomes the
    /// phone's one conversation.
    pub fn record(&mut self, phone: &str, id: &str, question: &str, answer: &str, now: Instant) {
        let conversation =
            self.by_phone
                .entry(phone.to_string())
                .or_insert_with(|| PhoneConversation {
                    id: id.to_string(),
                    messages: Vec::new(),
                    last: now,
                });
        if conversation.id != id {
            conversation.id = id.to_string();
            conversation.messages.clear();
        }
        for (role, content) in [("user", question), ("assistant", answer)] {
            conversation.messages.push(ChatMessage {
                role: role.to_string(),
                content: content.to_string(),
                images: Vec::new(),
            });
        }
        let excess = conversation
            .messages
            .len()
            .saturating_sub(MAX_KEPT_MESSAGES);
        conversation.messages.drain(..excess);
        conversation.last = now;
    }
}

fn with_conversations<T>(f: impl FnOnce(&mut PhoneConversations) -> T) -> T {
    let mut guard = CONVERSATIONS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(PhoneConversations::default))
}

/// Forget every phone's conversation (sign-out).
pub fn forget_all() {
    *CONVERSATIONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// `s` with its first letter upper-cased, cut to `max` characters.
fn short_label(s: &str, max: usize) -> String {
    let s = s.trim();
    let mut chars = s.chars();
    let mut label: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if label.chars().count() > max {
        label = label
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>()
            + "…";
    }
    label
}

/// What a tool's progress says, in English.
pub fn tool_label(call: &ToolCall) -> String {
    match call.name.as_str() {
        "web_search" => {
            let (query, _, _) = super::parse_web_search_args(&call.arguments);
            if query.is_empty() {
                "Searching the web".to_string()
            } else {
                short_label(
                    &format!("Searching the web for \u{201c}{query}\u{201d}"),
                    MAX_LABEL_CHARS,
                )
            }
        }
        "get_current_datetime" => "Checking the time".to_string(),
        "set_reminder" => "Setting a reminder".to_string(),
        "list_reminders" => "Checking your reminders".to_string(),
        "cancel_reminder" => "Cancelling a reminder".to_string(),
        name if name == act::TOOL_NAME => {
            let request = act::parse_request(&call.arguments);
            if request.is_empty() {
                "Working on the computer".to_string()
            } else {
                short_label(&request, MAX_LABEL_CHARS)
            }
        }
        _ => "Working on it".to_string(),
    }
}

/// A phone's tools: progress to the phone, commands as the phone's, and the
/// actions they took collected for the result.
struct PhoneTools {
    app: AppHandle,
    settings: AppSettings,
    progress: UnboundedSender<PhoneProgress>,
    actions: Mutex<Vec<PhoneAction>>,
    pending: Mutex<Option<PhonePending>>,
}

impl TurnTools for PhoneTools {
    fn round_started(&self, calls: &[ToolCall]) {
        if let Some(first) = calls.first() {
            let _ = self.progress.send(PhoneProgress::Tool(tool_label(first)));
        }
    }

    fn run<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, String> {
        Box::pin(async move {
            if call.name == act::TOOL_NAME {
                if !act::offered(&get_settings(&self.app)) {
                    return "Actions on the computer are turned off. Tell the user.".into();
                }
                let _ = self.progress.send(PhoneProgress::Tool(tool_label(call)));
                let (result, report) =
                    act::perform(&self.app, &call.arguments, CommandOrigin::Phone).await;
                if let Some(report) = report {
                    record_action(&self.actions, &self.pending, &report);
                }
                return result;
            }
            super::run_text_tool(
                &self.app,
                &self.settings,
                &call.name,
                &call.arguments,
                None,
                false,
            )
            .await
        })
    }

    fn capture_screen(&self) -> BoxFuture<'_, Result<String, String>> {
        // Never offered; a model that calls it anyway is told why not.
        Box::pin(async { Err("the screen cannot be seen when asked from the phone".to_string()) })
    }
}

/// Note what a command did, and what it left waiting.
fn record_action(
    actions: &Mutex<Vec<PhoneAction>>,
    pending: &Mutex<Option<PhonePending>>,
    report: &crate::command_mode::CommandReport,
) {
    let label = match report.status {
        CommandStatus::AwaitingConfirmation => report
            .pending_summary
            .clone()
            .unwrap_or_else(|| report.summary()),
        _ => report.summary(),
    };
    actions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(PhoneAction {
            label,
            status: act::status_code(report.status).to_string(),
        });
    // A newer command replaces whatever an earlier one left waiting.
    let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
    *pending = match report.status {
        CommandStatus::AwaitingConfirmation => Some(PhonePending {
            id: report.id,
            summary: report
                .pending_summary
                .clone()
                .unwrap_or_else(|| report.summary()),
        }),
        _ => None,
    };
}

/// The token sink for one round: the answer so far, reasoning removed, sent
/// to the phone at most every [`ANSWER_PROGRESS_EVERY`].
fn phone_token_sink(
    progress: UnboundedSender<PhoneProgress>,
    partial: Arc<Mutex<String>>,
    streamed: Arc<AtomicBool>,
) -> impl FnMut(&str) + Send {
    let mut reasoning = crate::flow::ReasoningStreamFilter::default();
    let mut last_sent: Option<Instant> = None;
    move |token: &str| {
        streamed.store(true, Ordering::SeqCst);
        let token = reasoning.push(token);
        if token.is_empty() {
            return;
        }
        let so_far = {
            let mut buffer = partial.lock().unwrap_or_else(|e| e.into_inner());
            buffer.push_str(&token);
            buffer.clone()
        };
        if last_sent.is_none_or(|at| at.elapsed() >= ANSWER_PROGRESS_EVERY) {
            let _ = progress.send(PhoneProgress::Answer(so_far));
            last_sent = Some(Instant::now());
        }
    }
}

/// Answer a phone's question with this computer's assistant. The caller has
/// checked that the assistant is on and that this phone has no other question
/// running; it drops the future to cancel.
pub async fn answer_for_phone(
    app: &AppHandle,
    ask: PhoneAsk,
) -> Result<PhoneAnswer, PhoneAskError> {
    let PhoneAsk {
        phone,
        conversation,
        text,
        progress,
    } = ask;
    let text = text.trim().to_string();
    let mut settings = get_settings(app);
    // Nothing is spoken: the answer is read on the phone.
    settings.assistant_tts_enabled = false;
    let (conversation_id, history) =
        with_conversations(|c| c.open(&phone, conversation.as_deref(), Instant::now()));

    if settings.active_character_is_cat() {
        let answer = super::random_meow();
        with_conversations(|c| c.record(&phone, &conversation_id, &text, &answer, Instant::now()));
        crate::insights::record_phone_question(app);
        return Ok(PhoneAnswer {
            answer,
            conversation: conversation_id,
            actions: Vec::new(),
            pending: None,
        });
    }

    let turn_model = super::resolve_turn_model(&settings).map_err(|problem| {
        debug!("A phone's question has no model: {problem:?}");
        PhoneAskError::NotConfigured
    })?;
    let messages = request_messages(&settings, &turn_model, &history, &text);
    let (tools, without_tools) = (messages.tools, messages.without_tools);

    // The built-in engine: running and serving the model, and kept loaded
    // while this answer streams.
    let _llm_activity_guard = if turn_model.provider.id == "builtin" {
        let manager = app.state::<Arc<crate::managers::local_llm::LocalLlmManager>>();
        manager
            .ensure_running(&turn_model.model)
            .await
            .map_err(|e| PhoneAskError::Failed(e.to_string()))?;
        Some(manager.begin_request())
    } else {
        None
    };

    let hooks = PhoneTools {
        app: app.clone(),
        settings: settings.clone(),
        progress: progress.clone(),
        actions: Mutex::default(),
        pending: Mutex::default(),
    };
    let partial = Arc::new(Mutex::new(String::new()));
    let streamed = Arc::new(AtomicBool::new(false));
    let sink = || {
        // Each round's text replaces the last one's.
        partial.lock().unwrap_or_else(|e| e.into_inner()).clear();
        phone_token_sink(progress.clone(), partial.clone(), streamed.clone())
    };
    let mut outcome =
        turn::run_tool_loop(&turn_model, messages.with_tools, &tools, &hooks, &sink).await;
    if let Err(e) = &outcome {
        // A provider that refuses function calling: once more with no tools,
        // as the panel does, while nothing has been shown yet.
        if super::is_tools_unsupported_error(e) && !streamed.load(Ordering::SeqCst) {
            warn!(
                "The assistant's provider rejected tool calling; answering the phone without tools"
            );
            outcome = llm_client::send_chat_stream(
                &turn_model.provider,
                turn_model.api_key.clone(),
                &turn_model.model,
                without_tools,
                turn_model.thinking.0.clone(),
                turn_model.thinking.1.clone(),
                sink(),
            )
            .await;
        }
    }
    let answer = outcome.map_err(|e| {
        warn!("A phone's question could not be answered: {e}");
        PhoneAskError::Failed(e)
    })?;
    let answer = crate::flow::strip_reasoning_blocks(&answer)
        .trim()
        .to_string();
    if answer.is_empty() {
        return Err(PhoneAskError::Failed(
            "The model gave an empty answer.".to_string(),
        ));
    }
    with_conversations(|c| c.record(&phone, &conversation_id, &text, &answer, Instant::now()));
    crate::insights::record_phone_question(app);
    info!("Answered a phone's question");
    let actions = std::mem::take(&mut *hooks.actions.lock().unwrap_or_else(|e| e.into_inner()));
    let pending = hooks
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    Ok(PhoneAnswer {
        answer,
        conversation: conversation_id,
        actions,
        pending,
    })
}

/// A phone question's request: with its tools, and the same without them.
pub(crate) struct RequestMessages {
    pub with_tools: Vec<Value>,
    pub without_tools: Vec<Value>,
    pub tools: Value,
}

/// The request a phone's question sends: the quick ask's system prompt (no
/// screen, never spoken), the conversation so far, and the question.
pub(crate) fn request_messages(
    settings: &AppSettings,
    turn_model: &TurnModel,
    history: &[ChatMessage],
    text: &str,
) -> RequestMessages {
    let (web_via_tools, web_via_online) = super::web_plan(settings, &turn_model.provider, false);
    let act_offered = act::offered(settings);
    let tools = super::build_assistant_tool_capabilities(web_via_tools, false, act_offered)
        .unwrap_or_else(|| json!([]));
    let tools_section = super::tools_system_section(
        web_via_tools,
        false,
        false,
        settings.assistant_search_depth,
        act_offered,
    );
    let system = super::compose_system_prompt(
        settings,
        &PromptPlan {
            call: false,
            meeting_section: None,
            tools_section: &tools_section,
            web_via_online,
        },
        text,
    );
    let max_chars = if super::needs_small_body(&turn_model.provider) {
        24_000
    } else {
        usize::MAX
    };
    let earlier = super::history_window(
        history,
        settings.assistant_max_history_messages as usize,
        max_chars,
    );
    let build = |system: String| {
        let mut messages = vec![json!({ "role": "system", "content": system })];
        messages.extend(earlier.iter().cloned());
        messages.push(json!({ "role": "user", "content": text }));
        messages
    };
    let without_tools = build(system.replacen(&tools_section, super::TOOLS_UNAVAILABLE_SECTION, 1));
    RequestMessages {
        with_tools: build(system),
        without_tools,
        tools,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: name.into(),
            arguments: arguments.into(),
            thought_signature: None,
        }
    }

    #[test]
    fn a_conversation_continues_only_when_named_and_fresh() {
        let start = Instant::now();
        let mut conversations = PhoneConversations::default();
        let (first, history) = conversations.open("phone", None, start);
        assert!(history.is_empty());
        conversations.record("phone", &first, "hi", "hello", start);

        // Named: it continues.
        let (id, history) =
            conversations.open("phone", Some(&first), start + Duration::from_secs(60));
        assert_eq!(id, first);
        assert_eq!(history.len(), 2);

        // Not named, or named wrongly: a new one.
        let (id, history) = conversations.open("phone", None, start);
        assert_ne!(id, first);
        assert!(history.is_empty());
        let (id, _) = conversations.open("phone", Some("c-unknown"), start);
        assert_ne!(id, first);

        // Another phone never sees it.
        let (id, history) = conversations.open("phone2", Some(&first), start);
        assert_ne!(id, first);
        assert!(history.is_empty());

        // Thirty minutes after its last question it is gone.
        let late = start + CONVERSATION_TTL;
        let (id, history) = conversations.open("phone", Some(&first), late);
        assert_ne!(id, first);
        assert!(history.is_empty());
    }

    #[test]
    fn each_phone_keeps_one_conversation() {
        let now = Instant::now();
        let mut conversations = PhoneConversations::default();
        conversations.record("phone", "c1", "a", "b", now);
        conversations.record("phone", "c2", "c", "d", now);
        let (id, history) = conversations.open("phone", Some("c1"), now);
        assert_ne!(id, "c1");
        assert!(history.is_empty());
        let (id, history) = conversations.open("phone", Some("c2"), now);
        assert_eq!(id, "c2");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "c");
    }

    #[test]
    fn a_long_conversation_keeps_its_newest_messages() {
        let now = Instant::now();
        let mut conversations = PhoneConversations::default();
        for i in 0..30 {
            conversations.record("phone", "c1", &format!("q{i}"), &format!("a{i}"), now);
        }
        let (_, history) = conversations.open("phone", Some("c1"), now);
        assert_eq!(history.len(), MAX_KEPT_MESSAGES);
        assert_eq!(history.last().unwrap().content, "a29");
        assert_eq!(history[0].role, "user");
    }

    #[test]
    fn tools_have_short_labels() {
        assert_eq!(
            tool_label(&call("web_search", r#"{"query":"weather in Pokhara"}"#)),
            "Searching the web for \u{201c}weather in Pokhara\u{201d}"
        );
        assert_eq!(
            tool_label(&call(
                act::TOOL_NAME,
                r#"{"request":"play believer on youtube"}"#
            )),
            "Play believer on youtube"
        );
        assert_eq!(
            tool_label(&call("get_current_datetime", "{}")),
            "Checking the time"
        );
        assert_eq!(tool_label(&call("something_new", "{}")), "Working on it");
        let long = format!(r#"{{"request":"{}"}}"#, "x".repeat(200));
        assert_eq!(
            tool_label(&call(act::TOOL_NAME, &long)).chars().count(),
            MAX_LABEL_CHARS
        );
    }

    #[test]
    fn actions_are_collected_and_the_newest_wait_wins() {
        use crate::command_mode::llm::Usage;
        use crate::command_mode::registry::Feedback;
        let report = |status: CommandStatus, id: u32, pending: Option<&str>| {
            crate::command_mode::CommandReport {
                id,
                origin: CommandOrigin::Phone,
                transcript: String::new(),
                status,
                feedback: vec![Feedback::new("x", "Opened YouTube.", &[])],
                steps: Vec::new(),
                pending_summary: pending.map(str::to_string),
                question: None,
                choices: Vec::new(),
                escalation: None,
                dry_run: false,
                dry_run_actions: Vec::new(),
                model: String::new(),
                llm_calls: 0,
                usage: Usage::default(),
                latency_ms: 0,
                error: None,
            }
        };
        let actions = Mutex::default();
        let pending = Mutex::default();
        record_action(
            &actions,
            &pending,
            &report(
                CommandStatus::AwaitingConfirmation,
                7,
                Some("Send the email to Ron"),
            ),
        );
        assert_eq!(
            *pending.lock().unwrap(),
            Some(PhonePending {
                id: 7,
                summary: "Send the email to Ron".into()
            })
        );
        record_action(&actions, &pending, &report(CommandStatus::Done, 8, None));
        assert_eq!(*pending.lock().unwrap(), None);
        assert_eq!(
            *actions.lock().unwrap(),
            vec![
                PhoneAction {
                    label: "Send the email to Ron".into(),
                    status: "awaiting_confirmation".into()
                },
                PhoneAction {
                    label: "Opened YouTube.".into(),
                    status: "done".into()
                },
            ]
        );
    }

    #[test]
    fn a_phone_request_offers_the_computer_only_with_command_mode_and_never_the_screen() {
        let mut settings = crate::settings::get_default_settings();
        settings.assistant_ask_screen_access = true;
        let model = TurnModel {
            provider: settings.post_process_providers[0].clone(),
            api_key: String::new(),
            model: "m".into(),
            thinking: (None, None),
        };
        let names = |messages: &RequestMessages| -> Vec<String> {
            messages
                .tools
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["function"]["name"].as_str().unwrap().to_string())
                .collect()
        };
        settings.command_mode_enabled = false;
        let off = request_messages(&settings, &model, &[], "play believer");
        assert!(!names(&off).contains(&act::TOOL_NAME.to_string()));
        assert!(!names(&off).contains(&"capture_screen".to_string()));

        settings.command_mode_enabled = true;
        let on = request_messages(&settings, &model, &[], "play believer");
        assert!(names(&on).contains(&act::TOOL_NAME.to_string()));
        assert!(!names(&on).contains(&"capture_screen".to_string()));
        let system = on.with_tools[0]["content"].as_str().unwrap();
        assert!(system.contains(act::GUIDANCE.trim_end()));
        let bare = on.without_tools[0]["content"].as_str().unwrap();
        assert!(!bare.contains(act::GUIDANCE.trim_end()));
        assert_eq!(on.with_tools.last().unwrap()["content"], "play believer");
    }
}
