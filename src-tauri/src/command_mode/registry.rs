//! The tool registry: every predefined action command mode can take.
//!
//! One file per tool under `tools/`, each exporting `pub static SPEC: ToolSpec`.
//! `build.rs` (`generate_command_tools`) turns the directory listing into
//! `tools::ALL`, so adding a tool is adding a file. Everything else about a tool
//! is derived from its spec: what the router model is offered
//! ([`tool_defs`]), how its arguments are checked ([`validate_args`]), whether
//! it may run on this machine ([`Platform`]), and whether it needs the user's
//! go-ahead first ([`Risk`]).
//!
//! Handlers are synchronous on purpose. Every starter tool does local work
//! (build a URL, open it, press a key, look up a contact), the executor runs
//! them on a blocking thread, and keeping them sync is what lets a spec be a
//! `static` with plain function pointers in it.

use serde::Serialize;
use serde_json::Value;
use specta::Type;
use std::sync::Arc;

use super::contacts::ContactBook;
use super::context::CommandContext;
use super::effects::Effects;
use super::llm::ToolDef;
use crate::settings::AppSettings;

/// Whether a tool can act on its own or must wait for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Reversible or harmless: opening a page, a draft, an app.
    Safe,
    /// The tool does its reversible half (open the draft) and returns
    /// [`ToolOutcome::NeedsConfirmation`]; the irreversible half (send) runs
    /// only from [`ToolSpec::on_confirm`] after the user says so.
    NeedsConfirmation,
}

/// Where a tool can run. Linux is split because Wayland forbids the input
/// injection and window activation several tools depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    MacOs,
    LinuxX11,
    LinuxWayland,
}

impl Platform {
    /// The platform this process is running on, decided at run time on Linux.
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOs
        }
        #[cfg(target_os = "linux")]
        {
            if crate::utils::is_wayland() {
                Platform::LinuxWayland
            } else {
                Platform::LinuxX11
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            Platform::LinuxX11
        }
    }

    /// The modifier a shortcut like "send" uses here: Cmd on a Mac, Ctrl elsewhere.
    pub fn primary_modifier(self) -> Modifier {
        match self {
            Platform::MacOs => Modifier::Meta,
            _ => Modifier::Ctrl,
        }
    }

    /// A short name for the router's context line.
    pub fn label(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::MacOs => "macOS",
            Platform::LinuxX11 => "Linux (X11)",
            Platform::LinuxWayland => "Linux (Wayland)",
        }
    }
}

/// Every platform. Most tools use this.
pub const ALL_PLATFORMS: &[Platform] = &[
    Platform::Windows,
    Platform::MacOs,
    Platform::LinuxX11,
    Platform::LinuxWayland,
];

/// Every platform except Wayland, for tools that inject input or activate
/// windows.
pub const NOT_WAYLAND: &[Platform] = &[Platform::Windows, Platform::MacOs, Platform::LinuxX11];

/// A keyboard modifier, as tools and [`KeyCombo`] name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    /// Windows key, Cmd on a Mac, Super on Linux.
    Meta,
}

/// A tool's handler. Runs on a blocking thread; never on the main thread.
pub type Handler = fn(&ToolCtx, Value) -> ToolOutcome;

/// Everything about one tool, in one place.
pub struct ToolSpec {
    /// Snake-case name the model calls. Unique across the registry.
    pub name: &'static str,
    /// One or two sentences written for the router model: what the tool does
    /// and when to pick it over the others.
    pub description: &'static str,
    /// JSON Schema of the arguments (the subset [`super::schema`] checks).
    /// A function rather than a `Value` because a `static` cannot allocate.
    pub schema: fn() -> Value,
    pub risk: Risk,
    pub platforms: &'static [Platform],
    /// Words that make this tool relevant, for pre-filtering a large registry
    /// before the router call. Matched case-insensitively against the
    /// transcript, alongside the name and description.
    pub keywords: &'static [&'static str],
    pub handler: Handler,
    /// Runs the irreversible half of a [`Risk::NeedsConfirmation`] tool with the
    /// [`PendingAction::payload`] the handler left. `None` for safe tools.
    pub on_confirm: Option<Handler>,
}

impl ToolSpec {
    pub fn supports(&self, platform: Platform) -> bool {
        self.platforms.contains(&platform)
    }

    pub fn def(&self) -> ToolDef {
        ToolDef {
            name: self.name.to_string(),
            description: self.description.to_string(),
            input_schema: (self.schema)(),
        }
    }
}

/// What a tool needs to run. Built once per command and shared by every call
/// in it.
pub struct ToolCtx {
    /// A snapshot of the settings when the command started.
    pub settings: AppSettings,
    /// Time, place and focus when the command started.
    pub context: CommandContext,
    /// Every side effect goes through here, so dry run and tests can see it.
    pub effects: Arc<dyn Effects>,
    pub contacts: Arc<ContactBook>,
    pub platform: Platform,
    pub dry_run: bool,
}

/// What a user-facing message says, as a translation key plus parameters.
///
/// Feedback is built from templates rather than a second LLM call. The key is
/// looked up in the frontend's translations (`commandMode.feedback.*`), and
/// [`Feedback::render_en`] gives the English text for logs, the CLI and
/// anywhere without the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Feedback {
    pub key: String,
    pub params: Vec<(String, String)>,
    /// The English rendering, so a surface without translations still has words.
    pub text: String,
}

impl Feedback {
    /// A message from a template key, its parameters and its English text.
    /// The English template uses `{{name}}` placeholders, as i18next does.
    pub fn new(key: &str, english: &str, params: &[(&str, &str)]) -> Self {
        let mut text = english.to_string();
        for (name, value) in params {
            text = text.replace(&format!("{{{{{name}}}}}"), value);
        }
        Self {
            key: format!("commandMode.feedback.{key}"),
            params: params
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            text,
        }
    }

    pub fn render_en(&self) -> &str {
        &self.text
    }
}

/// The irreversible half of a tool, waiting for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAction {
    /// The tool whose `on_confirm` will run.
    pub tool: &'static str,
    /// What will happen, shown on the confirmation surface:
    /// "Send email to Ron Weasley · Meeting at 9:00".
    pub summary: String,
    /// Whatever `on_confirm` needs (window hint, recipient), opaque to the
    /// executor.
    pub payload: Value,
}

/// How one tool call ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Done {
        feedback: Feedback,
    },
    NeedsConfirmation {
        pending: PendingAction,
        feedback: Feedback,
    },
    /// The tool cannot go on without an answer: several contacts match, none
    /// does, a file is ambiguous.
    AskUser {
        question: String,
        choices: Vec<String>,
    },
    /// The tool could not do it. `escalate` says whether the computer-use
    /// fallback could plausibly succeed where the tool failed (an app the tool
    /// could not find: yes; a missing API key: no).
    Failed {
        reason: String,
        escalate: bool,
    },
}

impl ToolOutcome {
    pub fn done(key: &str, english: &str, params: &[(&str, &str)]) -> Self {
        ToolOutcome::Done {
            feedback: Feedback::new(key, english, params),
        }
    }

    pub fn failed(reason: impl Into<String>, escalate: bool) -> Self {
        ToolOutcome::Failed {
            reason: reason.into(),
            escalate,
        }
    }

    pub fn ask(question: impl Into<String>, choices: Vec<String>) -> Self {
        ToolOutcome::AskUser {
            question: question.into(),
            choices,
        }
    }
}

/// Every registered tool, in file-name order.
pub fn all() -> &'static [&'static ToolSpec] {
    super::tools::ALL
}

/// Look a tool up by name.
pub fn find(name: &str) -> Option<&'static ToolSpec> {
    all().iter().copied().find(|spec| spec.name == name)
}

/// The tools that can run on `platform`, in registry order.
pub fn available(platform: Platform) -> Vec<&'static ToolSpec> {
    all()
        .iter()
        .copied()
        .filter(|spec| spec.supports(platform))
        .collect()
}

/// The model-facing definitions of `tools`, in the order given. Deterministic
/// for a given input, which the provider's prompt cache depends on.
pub fn tool_defs(tools: &[&ToolSpec]) -> Vec<ToolDef> {
    tools.iter().map(|spec| spec.def()).collect()
}

/// Check a call's arguments against the tool's schema. The error lists every
/// problem, phrased so it can be handed back to the model for a repair.
pub fn validate_args(spec: &ToolSpec, args: &Value) -> Result<(), String> {
    super::schema::validate(&(spec.schema)(), args).map_err(|problems| problems.join("; "))
}

/// Read a string argument, trimmed; `None` when absent or blank.
pub fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Read an integer argument.
pub fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

/// Read a boolean argument.
pub fn arg_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

/// Read an array of strings argument, skipping blanks.
pub fn arg_strings(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Two tools with one name would make the model's choice ambiguous and the
    /// dispatch pick whichever came first.
    #[test]
    fn tool_names_are_unique() {
        let mut seen = HashSet::new();
        for spec in all() {
            assert!(seen.insert(spec.name), "duplicate tool name {}", spec.name);
        }
    }

    /// Tool names are what the model writes back; providers accept
    /// `^[a-zA-Z0-9_-]{1,64}$` and the special tools must not be shadowed.
    #[test]
    fn tool_names_are_valid_and_do_not_shadow_the_special_tools() {
        for spec in all() {
            assert!(
                !spec.name.is_empty()
                    && spec.name.len() <= 64
                    && spec
                        .name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "bad tool name {}",
                spec.name
            );
            assert!(
                !super::super::router::SPECIAL_TOOLS.contains(&spec.name),
                "{} shadows a special tool",
                spec.name
            );
        }
    }

    /// Every schema must be an object schema the validator understands, or
    /// every call to that tool fails validation.
    #[test]
    fn every_schema_is_a_well_formed_object_schema() {
        for spec in all() {
            let schema = (spec.schema)();
            assert_eq!(
                schema.get("type").and_then(Value::as_str),
                Some("object"),
                "{} schema is not an object",
                spec.name
            );
            assert!(
                super::super::schema::check_schema(&schema).is_ok(),
                "{} schema uses unsupported keywords: {:?}",
                spec.name,
                super::super::schema::check_schema(&schema)
            );
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                let properties = schema.get("properties").and_then(Value::as_object);
                for key in required {
                    let key = key.as_str().unwrap();
                    assert!(
                        properties.is_some_and(|p| p.contains_key(key)),
                        "{} requires {key} but does not define it",
                        spec.name
                    );
                }
            }
        }
    }

    /// A tool marked as needing confirmation must have something to confirm,
    /// and a safe tool must not carry a confirmation step nothing will ask for.
    #[test]
    fn confirmation_handlers_match_the_risk() {
        for spec in all() {
            match spec.risk {
                Risk::NeedsConfirmation => assert!(
                    spec.on_confirm.is_some(),
                    "{} needs confirmation but has no on_confirm",
                    spec.name
                ),
                Risk::Safe => assert!(
                    spec.on_confirm.is_none(),
                    "{} is safe but has on_confirm",
                    spec.name
                ),
            }
        }
    }

    #[test]
    fn every_tool_runs_somewhere_and_has_words_for_the_model() {
        for spec in all() {
            assert!(!spec.platforms.is_empty(), "{} runs nowhere", spec.name);
            assert!(
                spec.description.len() >= 20,
                "{} description too short",
                spec.name
            );
        }
    }

    /// The generated list is sorted, which keeps the cached prompt prefix
    /// identical across builds.
    #[test]
    fn the_registry_is_in_a_stable_order() {
        let names: Vec<&str> = all().iter().map(|spec| spec.name).collect();
        let defs_a: Vec<String> = tool_defs(all()).into_iter().map(|d| d.name).collect();
        let defs_b: Vec<String> = tool_defs(all()).into_iter().map(|d| d.name).collect();
        assert_eq!(defs_a, defs_b);
        assert_eq!(names.len(), defs_a.len());
    }

    #[test]
    fn the_starter_tools_are_all_registered() {
        for name in [
            "compose_email",
            "create_calendar_event",
            "open_url",
            "web_search",
            "open_app",
            "switch_to_app",
            "type_text",
            "press_keys",
            "media_control",
            "set_volume",
            "open_file",
            "lookup_contact",
        ] {
            assert!(find(name).is_some(), "{name} is not registered");
        }
    }

    #[test]
    fn wayland_drops_the_tools_that_inject_input() {
        let wayland: Vec<&str> = available(Platform::LinuxWayland)
            .iter()
            .map(|spec| spec.name)
            .collect();
        assert!(!wayland.contains(&"press_keys"));
        assert!(!wayland.contains(&"switch_to_app"));
        assert!(wayland.contains(&"open_url"));
    }

    #[test]
    fn feedback_fills_its_placeholders() {
        let feedback = Feedback::new(
            "emailDraft",
            "Draft to {{name}} ready. Say send or cancel.",
            &[("name", "Ron Weasley")],
        );
        assert_eq!(feedback.key, "commandMode.feedback.emailDraft");
        assert_eq!(
            feedback.render_en(),
            "Draft to Ron Weasley ready. Say send or cancel."
        );
    }

    #[test]
    fn argument_readers_trim_and_skip_blanks() {
        let args = serde_json::json!({"a": "  x ", "b": "   ", "list": ["", " y ", 3]});
        assert_eq!(arg_str(&args, "a"), Some("x"));
        assert_eq!(arg_str(&args, "b"), None);
        assert_eq!(arg_str(&args, "missing"), None);
        assert_eq!(arg_strings(&args, "list"), vec!["y".to_string()]);
    }
}
