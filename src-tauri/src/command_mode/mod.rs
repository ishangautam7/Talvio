//! Command mode: say what you want done, and Talvio does it on screen.
//!
//! "Send an email to Ron about a meeting at 9:00" becomes one request to a
//! small, fast model that picks a predefined tool and fills in its arguments
//! (tier 1, [`router`]); the tool then runs locally ([`tools`]) and does its
//! work visibly, in the user's own apps: the Gmail compose window opens in the
//! default browser, already logged in, already filled in. Anything
//! irreversible (the send) waits for the user ([`registry::Risk`]).
//!
//! The pieces:
//!
//! - [`registry`]: what a tool is. [`tools`]: the tools, one file each.
//! - [`agent`]: tier 2, for what no tool covers: an agent that works the
//!   app's controls through the accessibility tree, step by step.
//! - [`router`]: the one LLM call, its prompt, its tool list, validation and
//!   the single repair retry. [`llm`]: the provider interface behind it.
//! - [`effects`]: every side effect, so dry run records instead of acting.
//! - [`context`]: time, place and focus, sent with the command.
//! - [`contacts`], [`fuzzy`]: turning spoken names into people, apps, files.
//! - this module: the executor that runs a plan's calls in order and keeps
//!   the one pending confirmation or question per app.
//!
//! The executor's rules for what waits on the user:
//!
//! - One item waits at a time, and it belongs to the newest command. A new
//!   command clears it when it starts, and a command that finishes after a
//!   newer one started cannot put its own back: its report says it was
//!   replaced instead of offering a Send that would not work.
//! - An answer only takes a question, and a confirm only a confirmation; a
//!   reply of the wrong kind leaves the item where it is.
//! - A confirmation expires: an accept that comes after
//!   [`CommandOrigin::confirm_ttl`] runs nothing, because the draft and the
//!   windows around it may have changed since.
//! - Answering a question routes the command again with every question and
//!   answer so far and the steps already done, so nothing runs twice; after
//!   three answers a further question fails the command.
//!
//! When the router escalates (or a tool fails in a way another approach could
//! fix) and `command_auto_escalate` is on where the agent can run, the task
//! goes to [`agent::run`] on `command_agent_model`, and its steps, cost and
//! ending join the same report. An element action the agent held for the
//! user waits like any other confirmation, under the reserved tool name
//! [`agent::CONFIRM_TOOL`]. Esc ([`cancel_running`]) stops it between steps.
//!
//! A typed or CLI command forgets the window the last dictation remembered,
//! so its typing can never be pasted into that window
//! ([`CommandOrigin::keeps_dictation_target`]). Every tool step runs on the
//! blocking pool with a time limit.
//!
//! See `docs/command-mode-plan.md` for the design and the phases.

pub mod agent;
pub mod contacts;
pub mod context;
pub mod effects;
pub mod fuzzy;
pub mod llm;
pub mod registry;
pub mod review;
pub mod router;
pub mod schema;
pub mod sites;
pub mod tools;
pub mod voice;

#[cfg(test)]
pub(crate) mod live_tests;

use log::{debug, info, warn};
use serde::Serialize;
use serde_json::Value;
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use contacts::ContactBook;
use context::CommandContext;
use effects::{DryRunEffects, Effects, RecordsActions, SystemEffects};
use llm::Usage;
use registry::{Feedback, PendingAction, Platform, ToolCtx, ToolOutcome};
use router::{Followup, PlannedCall, RouteError};

/// Upper bound on routing, including its one repair retry. It is a deadline
/// the router checks around each request ([`router::route`]), rather than a
/// timeout around the whole call, so a repair round that runs out of time
/// still reports what the first request cost.
const ROUTE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long one tool step (or a confirmed step) may run before the command
/// stops waiting for it. Every effect bounds its own waits, so this is the
/// backstop for one that does not: without it a hung step left the command,
/// and the Debug box's busy state, waiting forever. A blocking thread cannot
/// be cancelled, so the step may still finish later, and the report says so.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// How many questions one command may ask before it gives up. A model that
/// keeps asking after three answers is not converging, and every round costs
/// the user a sentence and a router call.
const MAX_CLARIFICATIONS: usize = 3;

/// How long a confirmation waits when the command was typed or came from the
/// command line. Long enough to read the draft; short enough that "send"
/// cannot reach a draft the user has forgotten, in a browser that has since
/// moved on.
const CONFIRM_TTL: Duration = Duration::from_secs(120);

/// How long a spoken command's confirmation waits. Phase 2 listens for the
/// answer for about eight seconds; until then a voice confirmation gets this.
const VOICE_CONFIRM_TTL: Duration = Duration::from_secs(30);

/// Raised by Esc, `--cancel` and the Debug box's Stop to stop the running
/// command, and lowered when a new command (or an answer or confirmation)
/// starts. One flag for the process because one command runs at a time; the
/// agent checks it before every request and action, and while a request is
/// in flight.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Stop the running command, if there is one. Harmless when nothing runs:
/// the next command lowers the flag before it starts.
pub fn cancel_running() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// A new command starts un-cancelled.
fn reset_cancel() {
    CANCEL.store(false, Ordering::SeqCst);
}

/// Where a command came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CommandOrigin {
    /// Settings → Debug text box.
    Typed,
    /// `--run-command` on the command line.
    Cli,
    /// The command-mode hotkey (Phase 2).
    Voice,
    /// The assistant's `do_on_computer` tool, from the quick ask or a call.
    Assistant,
    /// A phone: its `command` request, or its assistant's `do_on_computer`.
    Phone,
}

impl CommandOrigin {
    /// Whether this command may paste into the window a dictation remembered
    /// (`input::remember_paste_target`). A spoken command and an ask remember
    /// their own target when they start (the ask does, for its Insert), so they
    /// may. A typed, CLI or phone command must not: the target is left over
    /// from the last dictation, possibly hours ago, and `type_text` would bring
    /// that window forward and paste into it.
    fn keeps_dictation_target(self) -> bool {
        matches!(self, CommandOrigin::Voice | CommandOrigin::Assistant)
    }

    /// How long a confirmation from this origin stays valid.
    fn confirm_ttl(self) -> Duration {
        match self {
            CommandOrigin::Voice => VOICE_CONFIRM_TTL,
            CommandOrigin::Typed
            | CommandOrigin::Cli
            | CommandOrigin::Assistant
            | CommandOrigin::Phone => CONFIRM_TTL,
        }
    }
}

/// Clear the target an earlier dictation left behind, unless this command is
/// one that keeps it. See [`CommandOrigin::keeps_dictation_target`].
fn release_dictation_target(origin: CommandOrigin) {
    if !origin.keeps_dictation_target() {
        crate::input::forget_paste_target();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    /// Every call ran.
    Done,
    /// A tool did its reversible half and waits for "send" / "cancel".
    AwaitingConfirmation,
    /// The model or a tool needs an answer before going on.
    NeedsAnswer,
    /// No tool fits, and the agent that would take over is switched off,
    /// not set up, or not available on this system.
    NeedsComputerUse,
    /// Declined, expired, or replaced by a newer command.
    Cancelled,
    Failed,
}

/// One tool call, as the report shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct StepReport {
    pub tool: String,
    /// The arguments as JSON text (the frontend only displays them).
    pub args_json: String,
    /// "done", "awaiting_confirmation", "asked", "failed", "skipped".
    pub result: String,
    pub detail: String,
}

/// Everything about one command, for the UI, the CLI and the log.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct CommandReport {
    /// Identifies the pending confirmation or question, if any.
    pub id: u32,
    pub origin: CommandOrigin,
    pub transcript: String,
    pub status: CommandStatus,
    /// What to show and say, in order.
    pub feedback: Vec<Feedback>,
    pub steps: Vec<StepReport>,
    /// What "send" will do, while awaiting confirmation.
    pub pending_summary: Option<String>,
    pub question: Option<String>,
    pub choices: Vec<String>,
    /// The task handed to computer use, when the router escalated.
    pub escalation: Option<String>,
    pub dry_run: bool,
    /// In a dry run, every action that would have happened.
    pub dry_run_actions: Vec<String>,
    pub model: String,
    pub llm_calls: u32,
    pub usage: Usage,
    pub latency_ms: u32,
    pub error: Option<String>,
}

impl CommandReport {
    fn new(id: u32, origin: CommandOrigin, transcript: &str, dry_run: bool) -> Self {
        Self {
            id,
            origin,
            transcript: transcript.to_string(),
            status: CommandStatus::Done,
            feedback: Vec::new(),
            steps: Vec::new(),
            pending_summary: None,
            question: None,
            choices: Vec::new(),
            escalation: None,
            dry_run,
            dry_run_actions: Vec::new(),
            model: String::new(),
            llm_calls: 0,
            usage: Usage::default(),
            latency_ms: 0,
            error: None,
        }
    }

    fn fail(mut self, feedback: Feedback, error: String) -> Self {
        self.status = CommandStatus::Failed;
        self.feedback.push(feedback);
        self.error = Some(error);
        self
    }

    /// A newer command took the waiting slot while this one ran, so what
    /// this one would have left waiting was dropped. The report must not
    /// offer a Send or an answer box that can no longer work.
    fn superseded(&mut self) {
        self.status = CommandStatus::Cancelled;
        self.pending_summary = None;
        self.question = None;
        self.choices.clear();
        self.feedback.push(Feedback::new(
            "superseded",
            "A newer command replaced this one, so it isn't waiting any more.",
            &[],
        ));
    }

    /// The English feedback, joined: what the CLI prints and the log records.
    pub fn summary(&self) -> String {
        self.feedback
            .iter()
            .map(|f| f.render_en().to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A tool's reversible half is done and its irreversible half waits for
/// "send".
struct PendingConfirmation {
    id: u32,
    action: PendingAction,
    ctx: Arc<ToolCtx>,
    dry: Option<Arc<DryRunEffects<SystemEffects>>>,
    origin: CommandOrigin,
    transcript: String,
    /// The questions answered and steps done before it, in case the
    /// confirmed step asks something in turn.
    followup: Followup,
    /// When it started waiting; an accept after the origin's
    /// [`CommandOrigin::confirm_ttl`] runs nothing.
    created: Instant,
}

/// A question waits for the user's answer.
struct PendingAnswer {
    id: u32,
    origin: CommandOrigin,
    transcript: String,
    question: String,
    /// Every earlier question and answer, and every step already done, so
    /// the answer's re-route neither asks again nor repeats a step.
    followup: Followup,
    dry_run: bool,
    context: CommandContext,
}

/// A confirmation or question waiting on the user. One at a time, and it
/// always belongs to the newest command: a new command replaces whatever was
/// waiting, the way a new dictation replaces an Undo offer, and an older
/// command that finishes late cannot put its own back
/// ([`CommandModeState::offer`]).
enum Waiting {
    Confirmation(PendingConfirmation),
    Answer(PendingAnswer),
}

impl Waiting {
    fn id(&self) -> u32 {
        match self {
            Waiting::Confirmation(pending) => pending.id,
            Waiting::Answer(pending) => pending.id,
        }
    }
}

/// Why nothing was taken for an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotWaiting {
    /// Nothing waits under that id.
    Nothing,
    /// Something waits under that id, but of the other kind; it was left in
    /// place.
    OtherKind,
}

/// Managed state: the id counter and what is waiting.
#[derive(Default)]
pub struct CommandModeState {
    inner: Mutex<StateInner>,
}

#[derive(Default)]
struct StateInner {
    /// The newest id issued. Only the command holding it may leave
    /// something waiting.
    next_id: u32,
    waiting: Option<Waiting>,
}

impl CommandModeState {
    fn lock(&self) -> std::sync::MutexGuard<'_, StateInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start a command: issue its id and drop whatever was waiting, under one
    /// lock, so nothing can be offered between the two.
    fn begin(&self) -> u32 {
        let mut inner = self.lock();
        inner.next_id = inner.next_id.wrapping_add(1).max(1);
        inner.waiting = None;
        inner.next_id
    }

    /// Leave `waiting` for the user, if its command is still the newest.
    /// Returns `false`, and drops it, when a newer command has started since:
    /// that one owns the slot, and a late finisher must not overwrite its
    /// confirmation. Equality rather than `<`, so the id wrapping around
    /// changes nothing.
    fn offer(&self, waiting: Waiting) -> bool {
        let mut inner = self.lock();
        if waiting.id() != inner.next_id {
            debug!(
                "command mode: command {} finished after command {} started; not leaving it waiting",
                waiting.id(),
                inner.next_id
            );
            return false;
        }
        inner.waiting = Some(waiting);
        true
    }

    /// Take the waiting question with this id. Anything else (another id, or
    /// a confirmation under this id) stays where it is, so a reply of the
    /// wrong kind cannot destroy a pending send.
    fn take_answer(&self, id: u32) -> Result<PendingAnswer, NotWaiting> {
        let mut inner = self.lock();
        match inner.waiting.take() {
            Some(Waiting::Answer(pending)) if pending.id == id => Ok(pending),
            other => {
                let why = not_waiting(&other, id);
                inner.waiting = other;
                Err(why)
            }
        }
    }

    /// Take the waiting confirmation with this id; see [`Self::take_answer`].
    fn take_confirmation(&self, id: u32) -> Result<PendingConfirmation, NotWaiting> {
        let mut inner = self.lock();
        match inner.waiting.take() {
            Some(Waiting::Confirmation(pending)) if pending.id == id => Ok(pending),
            other => {
                let why = not_waiting(&other, id);
                inner.waiting = other;
                Err(why)
            }
        }
    }
}

/// Why `left` (what was waiting, already known not to be the wanted kind)
/// does not answer for `id`.
fn not_waiting(left: &Option<Waiting>, id: u32) -> NotWaiting {
    match left {
        Some(waiting) if waiting.id() == id => NotWaiting::OtherKind,
        _ => NotWaiting::Nothing,
    }
}

/// The effects for one command: real, or real-wrapped-in-dry-run.
fn effects_for(
    app: &AppHandle,
    dry_run: bool,
) -> (Arc<dyn Effects>, Option<Arc<DryRunEffects<SystemEffects>>>) {
    if dry_run {
        let dry = Arc::new(DryRunEffects::new(SystemEffects::new(app.clone())));
        (dry.clone() as Arc<dyn Effects>, Some(dry))
    } else {
        (Arc::new(SystemEffects::new(app.clone())), None)
    }
}

fn load_contacts(app: &AppHandle) -> ContactBook {
    match ContactBook::path(app) {
        Ok(path) => ContactBook::load(&path),
        Err(e) => {
            warn!("command mode: {e}");
            ContactBook::default()
        }
    }
}

/// The deadline for one command's routing.
fn route_deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + ROUTE_TIMEOUT
}

/// Everything one run of the executor works with. Kept together so the steps
/// that can leave something waiting carry the command's whole story (origin,
/// transcript, earlier answers, steps already done) into it.
struct Scope<'a> {
    state: &'a CommandModeState,
    ctx: Arc<ToolCtx>,
    dry: Option<Arc<DryRunEffects<SystemEffects>>>,
    origin: CommandOrigin,
    transcript: String,
    /// The questions answered and steps done before this run.
    followup: Followup,
}

impl Scope<'_> {
    /// This run's followup plus the steps the report shows as done, for
    /// whatever is left waiting next.
    fn followup_after(&self, report: &CommandReport) -> Followup {
        let mut followup = self.followup.clone();
        followup.already_done.extend(completed_steps(&report.steps));
        followup
    }
}

/// Whether this command waits for the user before anything irreversible:
/// the "Ask before irreversible actions" switch is on, or the user's words
/// ask to review (`review::asks_to_review`). A call can ask for itself too
/// ([`review::call_wants_review`]); see [`waits_for_review`].
fn reviews_first(scope: &Scope<'_>) -> bool {
    scope.ctx.settings.command_confirm_irreversible || review::asks_to_review(&scope.transcript)
}

/// Whether one call's irreversible step waits for the user: the command
/// does, or the router marked this call ([`review::CONFIRM_FIRST`]).
fn waits_for_review(scope: &Scope<'_>, args: &Value) -> bool {
    reviews_first(scope) || review::call_wants_review(args)
}

/// Carry out the irreversible half a tool held ([`ToolOutcome::NeedsConfirmation`])
/// right away, because nobody asked to review it: the tool's `on_confirm`,
/// or for a control the agent held, [`agent::confirm_element`]. The outcome
/// is what the user hears ("Sent to Ron."), or an honest failure.
pub(crate) async fn carry_out_pending(ctx: &Arc<ToolCtx>, pending: &PendingAction) -> ToolOutcome {
    info!(
        "command mode: carrying out {} without waiting (nobody asked to review it)",
        pending.tool
    );
    if pending.tool == agent::CONFIRM_TOOL {
        let payload = pending.payload.clone();
        if ctx.dry_run {
            return agent::confirm_element(ctx, &agent::tree::Unsupported, &payload);
        }
        let run_ctx = ctx.clone();
        return run_blocking(TOOL_TIMEOUT, pending.tool, move || {
            agent::confirm_element(&run_ctx, agent::tree().as_ref(), &payload)
        })
        .await;
    }
    let Some(on_confirm) = registry::find(pending.tool).and_then(|spec| spec.on_confirm) else {
        return ToolOutcome::failed("That can no longer be done.", false);
    };
    let payload = pending.payload.clone();
    let run_ctx = ctx.clone();
    run_blocking(TOOL_TIMEOUT, pending.tool, move || {
        on_confirm(&run_ctx, payload)
    })
    .await
}

/// The steps that changed something, as `tool(args)`: done, or done up to
/// their confirmation. The special tools change nothing and are left out.
fn completed_steps(steps: &[StepReport]) -> Vec<String> {
    steps
        .iter()
        .filter(|step| matches!(step.result.as_str(), "done" | "awaiting_confirmation"))
        .filter(|step| !router::SPECIAL_TOOLS.contains(&step.tool.as_str()))
        .map(|step| format!("{}({})", step.tool, step.args_json))
        .collect()
}

/// Run a command from its transcript. Never panics and never returns an
/// error: every failure is a report with `status: Failed` and a message.
///
/// `context` is the context captured when the command started (Phase 2
/// captures it at recording start); `None` captures it now.
pub async fn run_command(
    app: &AppHandle,
    transcript: &str,
    origin: CommandOrigin,
    dry_run_override: Option<bool>,
    context: Option<CommandContext>,
) -> CommandReport {
    let started = Instant::now();
    reset_cancel();
    release_dictation_target(origin);
    let settings = crate::settings::get_settings(app);
    let dry_run = dry_run_override.unwrap_or(settings.command_dry_run);
    let context = match context {
        Some(context) => context,
        None => tauri::async_runtime::spawn_blocking(CommandContext::capture)
            .await
            .unwrap_or_else(|_| {
                CommandContext::at(chrono::Local::now(), None, Platform::current(), None, None)
            }),
    };

    let state = app.state_or_default();
    // A new command replaces whatever was waiting.
    let id = state.begin();

    let transcript = transcript.trim();
    let report = CommandReport::new(id, origin, transcript, dry_run);
    if transcript.is_empty() {
        return report.fail(
            Feedback::new("empty", "I didn't catch a command.", &[]),
            "empty transcript".into(),
        );
    }

    let llm = match llm::router_for(&settings) {
        Ok(llm) => llm,
        Err(e) => {
            return report.fail(
                Feedback::new(
                    "notConfigured",
                    "Command mode isn't set up: {{why}}",
                    &[("why", &e.to_string())],
                ),
                e.to_string(),
            )
        }
    };

    let (effects, dry) = effects_for(app, dry_run);
    let scope = Scope {
        state,
        ctx: Arc::new(ToolCtx {
            settings,
            context: context.clone(),
            effects,
            contacts: Arc::new(load_contacts(app)),
            platform: Platform::current(),
            dry_run,
        }),
        dry,
        origin,
        transcript: transcript.to_string(),
        followup: Followup::default(),
    };

    let routed = router::route(
        llm.as_ref(),
        scope.ctx.platform,
        transcript,
        &context,
        &scope.followup,
        route_deadline(),
    )
    .await;

    let report = finish(
        &scope,
        report,
        Some(llm.as_ref()),
        llm.label(),
        routed,
        started,
    )
    .await;
    log_report(&report);
    report
}

/// Answer the question a command asked, and carry on. The original command,
/// every question asked so far with its answer, and the steps that already
/// ran go back to the router together, so the new plan neither asks again
/// nor repeats what is done.
pub async fn answer_command(app: &AppHandle, id: u32, answer: &str) -> CommandReport {
    let started = Instant::now();
    reset_cancel();
    let state = app.state_or_default();
    let PendingAnswer {
        origin,
        transcript,
        question,
        mut followup,
        dry_run,
        context,
        ..
    } = match state.take_answer(id) {
        Ok(pending) => pending,
        Err(why) => {
            let feedback = match why {
                NotWaiting::OtherKind => Feedback::new(
                    "waitingForConfirmation",
                    "That command is waiting for you to confirm, not for an answer.",
                    &[],
                ),
                NotWaiting::Nothing => Feedback::new(
                    "nothingWaitingAnswer",
                    "Nothing is waiting for an answer.",
                    &[],
                ),
            };
            return CommandReport::new(id, CommandOrigin::Typed, "", false)
                .fail(feedback, "no matching question".into());
        }
    };
    release_dictation_target(origin);
    followup
        .exchanges
        .push((question, answer.trim().to_string()));

    let settings = crate::settings::get_settings(app);
    let new_id = state.begin();
    let report = CommandReport::new(new_id, origin, &transcript, dry_run);
    let llm = match llm::router_for(&settings) {
        Ok(llm) => llm,
        Err(e) => {
            return report.fail(
                Feedback::new(
                    "notConfigured",
                    "Command mode isn't set up: {{why}}",
                    &[("why", &e.to_string())],
                ),
                e.to_string(),
            )
        }
    };
    let (effects, dry) = effects_for(app, dry_run);
    let scope = Scope {
        state,
        ctx: Arc::new(ToolCtx {
            settings,
            context: context.clone(),
            effects,
            contacts: Arc::new(load_contacts(app)),
            platform: Platform::current(),
            dry_run,
        }),
        dry,
        origin,
        transcript: transcript.clone(),
        followup,
    };
    let routed = router::route(
        llm.as_ref(),
        scope.ctx.platform,
        &transcript,
        &context,
        &scope.followup,
        route_deadline(),
    )
    .await;

    let report = finish(
        &scope,
        report,
        Some(llm.as_ref()),
        llm.label(),
        routed,
        started,
    )
    .await;
    log_report(&report);
    report
}

/// Accept or decline what a command left waiting for confirmation.
pub async fn confirm_command(app: &AppHandle, id: u32, accept: bool) -> CommandReport {
    let started = Instant::now();
    reset_cancel();
    let state = app.state_or_default();
    let pending = match state.take_confirmation(id) {
        Ok(pending) => pending,
        Err(why) => {
            let feedback = match why {
                NotWaiting::OtherKind => Feedback::new(
                    "waitingForAnswer",
                    "That command is waiting for an answer, not a confirmation.",
                    &[],
                ),
                NotWaiting::Nothing => Feedback::new(
                    "nothingWaitingConfirmation",
                    "Nothing is waiting for confirmation.",
                    &[],
                ),
            };
            return CommandReport::new(id, CommandOrigin::Typed, "", false)
                .fail(feedback, "no matching confirmation".into());
        }
    };
    release_dictation_target(pending.origin);
    let report = confirm_taken(state, pending, accept, started).await;
    log_report(&report);
    report
}

/// The rest of [`confirm_command`], once the confirmation is taken. Separate
/// so the decline and expiry rules can be tested without an `AppHandle`.
async fn confirm_taken(
    state: &CommandModeState,
    pending: PendingConfirmation,
    accept: bool,
    started: Instant,
) -> CommandReport {
    confirm_taken_with(state, pending, accept, started, None).await
}

/// [`confirm_taken`] with the accessibility tree an agent confirmation acts
/// through; `None` is the platform's own ([`agent::tree`]), fetched only when
/// the confirmation is an agent's.
async fn confirm_taken_with(
    state: &CommandModeState,
    pending: PendingConfirmation,
    accept: bool,
    started: Instant,
    tree: Option<Arc<dyn agent::tree::AccessibilityTree>>,
) -> CommandReport {
    let PendingConfirmation {
        id,
        action,
        ctx,
        dry,
        origin,
        transcript,
        followup,
        created,
    } = pending;
    let mut report = CommandReport::new(id, origin, &transcript, ctx.dry_run);
    if !accept {
        report.status = CommandStatus::Cancelled;
        report.feedback.push(Feedback::new(
            "cancelled",
            "Cancelled. Nothing was sent.",
            &[],
        ));
        report.latency_ms = started.elapsed().as_millis() as u32;
        return report;
    }
    if created.elapsed() > origin.confirm_ttl() {
        // The draft, the window and the user's attention have all had time
        // to move on; a send now could land somewhere else.
        report.status = CommandStatus::Cancelled;
        report.feedback.push(Feedback::new(
            "confirmExpired",
            "That waited too long, so I didn't do it. Ask me again.",
            &[],
        ));
        report.error = Some("the confirmation expired".into());
        report.latency_ms = started.elapsed().as_millis() as u32;
        return report;
    }

    if action.tool == agent::CONFIRM_TOOL {
        let scope = Scope {
            state,
            ctx,
            dry,
            origin,
            transcript,
            followup,
        };
        return confirm_agent_action(&scope, report, action, tree, started).await;
    }

    let Some(on_confirm) = registry::find(action.tool).and_then(|spec| spec.on_confirm) else {
        return report.fail(
            Feedback::new("confirmFailed", "That can no longer be done.", &[]),
            format!("{} has no confirmation step", action.tool),
        );
    };

    // The original command recorded into the same dry run; the report shows
    // only what the confirmation did.
    let before = dry.as_ref().map(|d| d.recorded().len()).unwrap_or(0);
    let payload = action.payload.clone();
    let run_ctx = ctx.clone();
    let outcome = run_blocking(TOOL_TIMEOUT, action.tool, move || {
        on_confirm(&run_ctx, payload)
    })
    .await;

    report.steps.push(StepReport {
        tool: action.tool.to_string(),
        args_json: action.payload.to_string(),
        result: String::new(),
        detail: String::new(),
    });
    let scope = Scope {
        state,
        ctx,
        dry: dry.clone(),
        origin,
        transcript,
        followup,
    };
    apply_outcome(&scope, &mut report, outcome);
    if let Some(dry) = dry {
        report.dry_run_actions = dry.recorded().into_iter().skip(before).collect();
    }
    report.latency_ms = started.elapsed().as_millis() as u32;
    report
}

/// Carry out an element action the agent held, now that the user said yes.
/// It has no registry entry: [`agent::confirm_element`] reads the window
/// again and acts on the one control with the same role and name, or does
/// nothing. A dry run records the line instead and reads nothing.
async fn confirm_agent_action(
    scope: &Scope<'_>,
    mut report: CommandReport,
    action: PendingAction,
    tree: Option<Arc<dyn agent::tree::AccessibilityTree>>,
    started: Instant,
) -> CommandReport {
    let payload = action.payload.clone();
    report.steps.push(StepReport {
        tool: action.tool.to_string(),
        args_json: payload.to_string(),
        result: String::new(),
        detail: String::new(),
    });
    let outcome = if scope.ctx.dry_run {
        report
            .dry_run_actions
            .push(agent::confirm_dry_run_line(&payload));
        // `confirm_element` answers a dry run before touching the tree.
        agent::confirm_element(&scope.ctx, &agent::tree::Unsupported, &payload)
    } else {
        let tree = tree.unwrap_or_else(agent::tree);
        let run_ctx = scope.ctx.clone();
        run_blocking(TOOL_TIMEOUT, action.tool, move || {
            agent::confirm_element(&run_ctx, tree.as_ref(), &payload)
        })
        .await
    };
    apply_outcome(scope, &mut report, outcome);
    report.latency_ms = started.elapsed().as_millis() as u32;
    report
}

/// Run a tool's blocking step on the blocking pool, for at most `limit`.
/// Past it the command stops waiting; the thread itself cannot be cancelled
/// and may still finish its effect later, which the failure says.
async fn run_blocking<F>(limit: Duration, tool: &str, step: F) -> ToolOutcome
where
    F: FnOnce() -> ToolOutcome + Send + 'static,
{
    match tokio::time::timeout(limit, tauri::async_runtime::spawn_blocking(step)).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => ToolOutcome::failed(format!("The tool crashed: {e}"), false),
        Err(_) => {
            warn!(
                "command mode: {tool} ran past {} s; stopped waiting, it may still finish",
                limit.as_secs()
            );
            ToolOutcome::failed(
                "That step took too long, so I stopped waiting for it. It may still finish on its own.",
                false,
            )
        }
    }
}

/// Run a routed plan's calls in order and fill in the report. With `llm`, a
/// multi-part command whose plan stopped after opening something is routed
/// once more for what is left ([`continue_plan`]).
async fn finish(
    scope: &Scope<'_>,
    mut report: CommandReport,
    llm: Option<&dyn llm::RouterLlm>,
    model: String,
    routed: Result<router::RoutePlan, RouteError>,
    started: Instant,
) -> CommandReport {
    report.model = model;
    let plan = match routed {
        Ok(plan) => plan,
        Err(e) => {
            report.latency_ms = started.elapsed().as_millis() as u32;
            let (llm_calls, usage) = e.spent();
            report.llm_calls = llm_calls;
            report.usage = usage;
            return report.fail(
                Feedback::new(
                    "routeFailed",
                    "I couldn't work out what to do: {{why}}",
                    &[("why", &e.to_string())],
                ),
                e.to_string(),
            );
        }
    };
    report.llm_calls = plan.llm_calls;
    report.usage = plan.usage;

    let planned: Vec<String> = plan.calls.iter().map(|call| call.name.clone()).collect();
    // What was on screen before an app was opened, while a later step may
    // still have to type into it.
    let mut opened: Option<WindowsBefore> = None;
    let stopped = run_plan(scope, &mut report, plan.calls, &mut opened).await;
    report_dropped(&mut report, plan.dropped);
    if let Some(llm) = llm {
        if !stopped
            && report.status == CommandStatus::Done
            && wants_continuation(&scope.transcript, &planned)
        {
            continue_plan(scope, &mut report, llm, &mut opened).await;
        }
    }

    if let Some(dry) = &scope.dry {
        // What the tools' effects recorded, then the agent's element actions
        // (which go through the tree, not the effects, and were collected on
        // the report as they happened).
        let mut actions = dry.recorded();
        actions.append(&mut report.dry_run_actions);
        report.dry_run_actions = actions;
    }
    report.latency_ms = started.elapsed().as_millis() as u32;
    report
}

/// Words that join the parts of a spoken command, English and Nepali.
const PART_JOINERS: &[&str] = &["and", "then", "after", "ani", "ra", "pachi"];

/// Whether a command looks like it has a part the plan left out: it joins
/// parts ("open Notepad and type hello", "Notepad khola ani hello lekha"), and
/// its plan was a single step that only opened or switched to something. The
/// router model reliably plans "open X" and leaves what is done in X for a
/// later turn, because it cannot see the app come up; measured on the live
/// sweep, it dropped the typing in 7 of 8 runs.
fn wants_continuation(transcript: &str, planned: &[String]) -> bool {
    let [only] = planned else {
        return false;
    };
    let opener = opens_window(only) || matches!(only.as_str(), "open_url" | "switch_to_app");
    let lower = transcript.to_lowercase();
    let joined = lower.contains(',')
        || lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| PART_JOINERS.contains(&word));
    opener && joined
}

/// Route the command once more, with what already ran, and run what is left.
/// A plan that only answers (`say`) or repeats a step that ran adds nothing.
/// A failure here leaves the report as it was: the part that ran did run.
async fn continue_plan(
    scope: &Scope<'_>,
    report: &mut CommandReport,
    llm: &dyn llm::RouterLlm,
    opened: &mut Option<WindowsBefore>,
) {
    let followup = scope.followup_after(report);
    let plan = match router::route(
        llm,
        scope.ctx.platform,
        &scope.transcript,
        &scope.ctx.context,
        &followup,
        route_deadline(),
    )
    .await
    {
        Ok(plan) => plan,
        Err(e) => {
            debug!("command mode: no continuation for the rest of the command: {e}");
            return;
        }
    };
    report.llm_calls += plan.llm_calls;
    report.usage.add(plan.usage);
    let done: Vec<String> = followup
        .already_done
        .iter()
        .map(|step| step.to_lowercase())
        .collect();
    let calls: Vec<PlannedCall> = plan
        .calls
        .into_iter()
        .filter(|call| call.name != router::SAY)
        .filter(|call| !done.contains(&format!("{}({})", call.name, call.args).to_lowercase()))
        .filter(|call| {
            !(opens_window(&call.name) && report.steps.iter().any(|s| s.tool == call.name))
        })
        .collect();
    if calls.is_empty() {
        return;
    }
    info!(
        "command mode: carrying out {} more step(s) of the command",
        calls.len()
    );
    run_plan(scope, report, calls, opened).await;
    report_dropped(report, plan.dropped);
}

/// Run `calls` in order. `true` when the plan stopped early: a step waits for
/// the user or failed, and the rest are listed as skipped.
async fn run_plan(
    scope: &Scope<'_>,
    report: &mut CommandReport,
    calls: Vec<PlannedCall>,
    opened: &mut Option<WindowsBefore>,
) -> bool {
    let mut calls = calls.into_iter();
    while let Some(call) = calls.next() {
        if types_into_front(&call.name) {
            if let Some(before) = opened.take() {
                if !scope.ctx.dry_run
                    && !wait_for_new_window(scope.ctx.effects.as_ref(), &before, NEW_WINDOW_WAIT)
                        .await
                {
                    window_not_ready(report, &call);
                    skip_rest(report, calls);
                    return true;
                }
            }
        }
        let before =
            opens_window(&call.name).then(|| WindowsBefore::now(scope.ctx.effects.as_ref(), &call));
        let stop = run_call(scope, report, call).await;
        if let Some(before) = before {
            if report
                .steps
                .last()
                .is_some_and(|last| last.result == "done")
            {
                *opened = Some(before);
            }
        }
        if stop {
            skip_rest(report, calls);
            return true;
        }
    }
    false
}

/// List the calls left after a step that stopped the plan as skipped.
fn skip_rest(report: &mut CommandReport, rest: impl Iterator<Item = PlannedCall>) {
    for skipped in rest {
        report.steps.push(StepReport {
            tool: skipped.name,
            args_json: skipped.args.to_string(),
            result: "skipped".into(),
            detail: "An earlier step is waiting or failed.".into(),
        });
    }
}

// How long typing waits for the window of an app an earlier step opened.
const NEW_WINDOW_WAIT: Duration = Duration::from_secs(10);
/// How often it looks.
const NEW_WINDOW_POLL: Duration = Duration::from_millis(150);
/// Once the window is in front, before the first key: its text field takes
/// the keyboard a moment after the window itself does.
const NEW_WINDOW_SETTLE: Duration = Duration::from_millis(600);

/// Steps that open a new window: launching an app or a file.
fn opens_window(tool: &str) -> bool {
    matches!(tool, "open_app" | "open_file")
}

/// Steps that act on whatever window is in front.
fn types_into_front(tool: &str) -> bool {
    matches!(tool, "type_text" | "press_keys")
}

/// The screen before an app was opened: the window in front and every
/// window open.
struct WindowsBefore {
    front: Option<i64>,
    open: std::collections::HashSet<i64>,
    /// The name the app was opened by ("notepad"), lowercase.
    opened: String,
}

impl WindowsBefore {
    fn now(effects: &dyn Effects, call: &PlannedCall) -> Self {
        let opened = registry::arg_str(&call.args, "name")
            .unwrap_or_default()
            .to_lowercase();
        Self {
            front: effects.foreground_window().map(|window| window.id),
            open: effects
                .open_windows()
                .iter()
                .map(|window| window.id)
                .collect(),
            opened,
        }
    }

    /// Whether `window` belongs to the app that was opened: opening an app
    /// that was already in front (Notepad opens a new tab in its own window)
    /// changes no window.
    fn is_opened_app(&self, window: &effects::WindowEntry) -> bool {
        let first = self
            .opened
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase();
        !first.is_empty()
            && (window.app.to_lowercase().contains(&first)
                || window.title.to_lowercase().contains(&first))
    }
}

/// How long the app already in front gets to open its new tab or document
/// before typing goes into it.
const SAME_APP_GRACE: Duration = Duration::from_millis(1500);

/// Wait until a window other than the one in front before the app was
/// opened has the keyboard. A launch returns before the app's window exists,
/// and typing straight after it went into whatever the user had in front. A
/// new window that opened behind (Windows does not always let a launched app
/// take the foreground) is brought forward. `false` when nothing came up in
/// `timeout`: the typing is then not done at all.
async fn wait_for_new_window(
    effects: &dyn Effects,
    before: &WindowsBefore,
    timeout: Duration,
) -> bool {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut focused: Option<i64> = None;
    loop {
        if let Some(front) = effects.foreground_window() {
            let same_app_settled =
                started.elapsed() >= SAME_APP_GRACE.min(timeout) && before.is_opened_app(&front);
            if Some(front.id) != before.front || same_app_settled {
                tokio::time::sleep(NEW_WINDOW_SETTLE).await;
                return true;
            }
        }
        if focused.is_none() {
            if let Some(new) = effects
                .open_windows()
                .into_iter()
                .find(|window| !before.open.contains(&window.id))
            {
                debug!("command mode: the opened app's window is behind; bringing it forward");
                focused = Some(new.id);
                if let Err(e) = effects.focus_window(&new) {
                    warn!("command mode: could not bring the new window forward: {e}");
                }
            }
        }
        if Instant::now() >= deadline {
            warn!(
                "command mode: no new window came up within {} s; not typing",
                timeout.as_secs()
            );
            return false;
        }
        tokio::time::sleep(NEW_WINDOW_POLL).await;
    }
}

/// The typing step after an app that never came up: refused, and said so,
/// rather than typed into the window the user had in front.
fn window_not_ready(report: &mut CommandReport, call: &PlannedCall) {
    report.steps.push(step(
        &call.name,
        &call.args.to_string(),
        "failed",
        "the app's window did not come up",
    ));
    report.status = CommandStatus::Failed;
    let reason = "The app didn't come up in time, so I didn't type anything.";
    report.error = Some(reason.to_string());
    report
        .feedback
        .push(Feedback::new("windowNotReady", reason, &[]));
}

/// List the calls the plan cap cut off as skipped, and say so: a command
/// that was only partly carried out must not read as done.
fn report_dropped(report: &mut CommandReport, dropped: Vec<PlannedCall>) {
    if dropped.is_empty() {
        return;
    }
    let count = dropped.len();
    for call in dropped {
        report.steps.push(StepReport {
            tool: call.name,
            args_json: call.args.to_string(),
            result: "skipped".into(),
            detail: format!("Over the {} step limit.", router::MAX_PLAN_CALLS),
        });
    }
    report.feedback.push(Feedback::new(
        "planCapped",
        "That was too many steps at once, so I left out the last {{count}}.",
        &[("count", &count.to_string())],
    ));
}

/// Run one call. Returns `true` when the plan must stop here.
async fn run_call(scope: &Scope<'_>, report: &mut CommandReport, call: PlannedCall) -> bool {
    let args_json = call.args.to_string();
    match call.name.as_str() {
        router::SAY => {
            let text = registry::arg_str(&call.args, "text")
                .unwrap_or_default()
                .to_string();
            report.steps.push(step(&call.name, &args_json, "done", ""));
            report
                .feedback
                .push(Feedback::new("say", "{{text}}", &[("text", &text)]));
            false
        }
        router::ASK_USER => {
            let question = registry::arg_str(&call.args, "question")
                .unwrap_or("Can you say that differently?")
                .to_string();
            let choices = registry::arg_strings(&call.args, "choices");
            report
                .steps
                .push(step(&call.name, &args_json, "asked", &question));
            ask(scope, report, question, choices);
            true
        }
        router::ESCALATE => {
            let task = registry::arg_str(&call.args, "task")
                .unwrap_or(scope.transcript.as_str())
                .to_string();
            if agent_enabled(&scope.ctx.settings) {
                report.steps.push(step(
                    &call.name,
                    &args_json,
                    "done",
                    "handed to the screen agent",
                ));
                let review = waits_for_review(scope, &call.args);
                escalate(scope, report, task, review).await;
                return true;
            }
            report.steps.push(step(
                &call.name,
                &args_json,
                "skipped",
                "computer use is not available",
            ));
            report.status = CommandStatus::NeedsComputerUse;
            report.escalation = Some(task);
            report
                .feedback
                .push(computer_use_unavailable(&scope.ctx.settings));
            true
        }
        name => {
            let Some(spec) = registry::find(name) else {
                // The router validates names, so this is a bug, not a model error.
                report
                    .steps
                    .push(step(name, &args_json, "failed", "unknown tool"));
                report.status = CommandStatus::Failed;
                report.error = Some(format!("unknown tool {name}"));
                return true;
            };
            if !spec.supports(scope.ctx.platform) {
                report.steps.push(step(
                    name,
                    &args_json,
                    "failed",
                    "not available on this system",
                ));
                report.status = CommandStatus::Failed;
                report.feedback.push(Feedback::new(
                    "unsupported",
                    "That isn't possible on {{os}}.",
                    &[("os", scope.ctx.platform.label())],
                ));
                return true;
            }
            let handler = spec.handler;
            let run_ctx = scope.ctx.clone();
            let args = call.args.clone();
            let outcome = run_blocking(TOOL_TIMEOUT, name, move || handler(&run_ctx, args)).await;
            // What the tool held for a Do it runs now, unless someone asked
            // to review it (`review`).
            let outcome = match outcome {
                ToolOutcome::NeedsConfirmation { pending, .. }
                    if !waits_for_review(scope, &call.args) =>
                {
                    carry_out_pending(&scope.ctx, &pending).await
                }
                other => other,
            };
            report.steps.push(step(name, &args_json, "", ""));
            let stop = apply_outcome(scope, report, outcome);
            // A failure another approach could fix goes to the agent, with
            // the whole command as its task.
            if report.status == CommandStatus::NeedsComputerUse
                && agent_enabled(&scope.ctx.settings)
            {
                let task = report
                    .escalation
                    .clone()
                    .unwrap_or_else(|| scope.transcript.clone());
                let task = with_steps_done(task, &completed_steps(&report.steps));
                escalate(scope, report, task, reviews_first(scope)).await;
            }
            stop
        }
    }
}

/// The agent's task after a tool failed partway through a plan: the whole
/// command, plus the steps that already ran, so it does not open the app or
/// type the text a second time.
fn with_steps_done(task: String, done: &[String]) -> String {
    if done.is_empty() {
        return task;
    }
    format!("{task}\nAlready done, do not repeat: {}", done.join("; "))
}

/// Whether an escalation goes to the agent: the user allows it and this
/// platform has an accessibility reader.
fn agent_enabled(settings: &crate::settings::AppSettings) -> bool {
    settings.command_auto_escalate && agent::supported()
}

/// Why an escalation stops at `NeedsComputerUse`: the switch is off, or this
/// system has no accessibility reader yet.
fn computer_use_unavailable(settings: &crate::settings::AppSettings) -> Feedback {
    if agent::supported() && !settings.command_auto_escalate {
        Feedback::new(
            "agentOff",
            "None of my tools can do that. Turn on \u{201c}Control the screen when needed\u{201d} to let me work the app for you.",
            &[],
        )
    } else {
        Feedback::new(
            "needsComputerUse",
            "None of my tools can do that yet. Controlling the screen for it comes in a later version.",
            &[],
        )
    }
}

/// Hand `task` to the agent on `command_agent_model`. A model that cannot be
/// built (no key, no provider) leaves the command at `NeedsComputerUse` with
/// the reason.
async fn escalate(scope: &Scope<'_>, report: &mut CommandReport, task: String, review: bool) {
    report.escalation = Some(task.clone());
    let llm = match llm::agent_llm_for(&scope.ctx.settings) {
        Ok(llm) => llm,
        Err(e) => {
            report.status = CommandStatus::NeedsComputerUse;
            report.error = Some(e.to_string());
            report.feedback.push(Feedback::new(
                "agentNotConfigured",
                "That needs me to control the screen, but the agent isn't set up: {{why}}",
                &[("why", &e.to_string())],
            ));
            return;
        }
    };
    run_agent_into(
        scope,
        report,
        &task,
        llm.as_ref(),
        agent::tree(),
        &CANCEL,
        review,
    )
    .await;
}

/// Run the agent and fold what it did into the report: its steps, its
/// requests and tokens, its dry-run lines, and a status and feedback for how
/// it ended. Separate from [`escalate`] so tests can pass a scripted model,
/// a fake tree and their own stop flag.
async fn run_agent_into(
    scope: &Scope<'_>,
    report: &mut CommandReport,
    task: &str,
    llm: &dyn llm::RouterLlm,
    tree: Arc<dyn agent::tree::AccessibilityTree>,
    cancel: &AtomicBool,
    review: bool,
) {
    let mut limits = agent::Limits::from_settings(&scope.ctx.settings);
    limits.review_first = review;
    let outcome = agent::run(task, scope.ctx.clone(), llm, tree, limits, cancel).await;

    report.feedback.push(match outcome.app.as_deref() {
        Some(app) => Feedback::new(
            "agentWorking",
            "Working on it in {{app}}\u{2026}",
            &[("app", app)],
        ),
        None => Feedback::new("agentWorkingNoApp", "Working on it\u{2026}", &[]),
    });
    report.steps.extend(outcome.steps);
    report.llm_calls += outcome.llm_calls;
    report.usage.add(outcome.usage);
    report.model = if report.model.is_empty() {
        outcome.model
    } else {
        format!("{} + {}", report.model, outcome.model)
    };
    report.dry_run_actions.extend(outcome.dry_run_actions);

    match outcome.end {
        agent::AgentEnd::Done { summary } => {
            report.status = CommandStatus::Done;
            // A tool failure that sent the command here is history now.
            report.error = None;
            report.feedback.push(Feedback::new(
                "agentDone",
                "{{summary}}",
                &[("summary", &summary)],
            ));
        }
        agent::AgentEnd::GaveUp { reason } => {
            report.status = CommandStatus::Failed;
            report.error = Some(reason.clone());
            report.feedback.push(Feedback::new(
                "agentGaveUp",
                "I couldn't finish that: {{reason}}",
                &[("reason", &reason)],
            ));
        }
        agent::AgentEnd::NeedsConfirmation { pending, feedback } => {
            report.error = None;
            apply_outcome(
                scope,
                report,
                ToolOutcome::NeedsConfirmation { pending, feedback },
            );
        }
        agent::AgentEnd::LimitHit { which } => {
            report.status = CommandStatus::Failed;
            let feedback = match which {
                agent::Limit::Steps => Feedback::new(
                    "agentStepLimit",
                    "That took more than {{steps}} steps, so I stopped and left everything as it is.",
                    &[("steps", &limits.max_steps.to_string())],
                ),
                agent::Limit::Cost => Feedback::new(
                    "agentCostLimit",
                    "That was about to cost more than ${{limit}}, so I stopped and left everything as it is.",
                    &[("limit", &format!("{:.2}", limits.max_cost_usd))],
                ),
            };
            report.error = Some(feedback.render_en().to_string());
            report.feedback.push(feedback);
        }
        agent::AgentEnd::Cancelled => {
            report.status = CommandStatus::Cancelled;
            report.error = None;
            report.feedback.push(Feedback::new(
                "agentCancelled",
                "Stopped. I left everything as it is.",
                &[],
            ));
        }
        agent::AgentEnd::Failed { reason } => {
            report.status = CommandStatus::Failed;
            report.error = Some(reason.clone());
            report.feedback.push(Feedback::new(
                "agentFailed",
                "I couldn't control the screen: {{reason}}",
                &[("reason", &reason)],
            ));
        }
    }
}

fn step(tool: &str, args_json: &str, result: &str, detail: &str) -> StepReport {
    StepReport {
        tool: tool.to_string(),
        args_json: args_json.to_string(),
        result: result.to_string(),
        detail: detail.to_string(),
    }
}

/// Record a tool's outcome on the report (its last step) and the state.
/// Returns `true` when the plan must stop.
fn apply_outcome(scope: &Scope<'_>, report: &mut CommandReport, outcome: ToolOutcome) -> bool {
    match outcome {
        ToolOutcome::Done { feedback } => {
            if let Some(last) = report.steps.last_mut() {
                last.result = "done".into();
                last.detail = feedback.render_en().to_string();
            }
            report.feedback.push(feedback);
            false
        }
        ToolOutcome::NeedsConfirmation { pending, feedback } => {
            if let Some(last) = report.steps.last_mut() {
                last.result = "awaiting_confirmation".into();
                last.detail = pending.summary.clone();
            }
            report.feedback.push(feedback);
            let summary = pending.summary.clone();
            let waiting = Waiting::Confirmation(PendingConfirmation {
                id: report.id,
                action: pending,
                ctx: scope.ctx.clone(),
                dry: scope.dry.clone(),
                origin: scope.origin,
                transcript: scope.transcript.clone(),
                followup: scope.followup_after(report),
                created: Instant::now(),
            });
            if scope.state.offer(waiting) {
                report.status = CommandStatus::AwaitingConfirmation;
                report.pending_summary = Some(summary);
            } else {
                report.superseded();
            }
            true
        }
        ToolOutcome::AskUser { question, choices } => {
            if let Some(last) = report.steps.last_mut() {
                last.result = "asked".into();
                last.detail = question.clone();
            }
            ask(scope, report, question, choices);
            true
        }
        ToolOutcome::Failed { reason, escalate } => {
            if let Some(last) = report.steps.last_mut() {
                last.result = "failed".into();
                last.detail = reason.clone();
            }
            report.status = if escalate {
                CommandStatus::NeedsComputerUse
            } else {
                CommandStatus::Failed
            };
            if escalate {
                report.escalation = Some(scope.transcript.clone());
            }
            report.error = Some(reason.clone());
            report.feedback.push(Feedback::new(
                "failed",
                "{{reason}}",
                &[("reason", &reason)],
            ));
            true
        }
    }
}

/// Leave a question waiting, with everything the answer's re-route needs.
/// Past [`MAX_CLARIFICATIONS`] answered questions the command fails instead.
fn ask(scope: &Scope<'_>, report: &mut CommandReport, question: String, choices: Vec<String>) {
    if scope.followup.exchanges.len() >= MAX_CLARIFICATIONS {
        if let Some(last) = report.steps.last_mut() {
            last.result = "failed".into();
            last.detail = "too many questions".into();
        }
        report.status = CommandStatus::Failed;
        report.error = Some(format!("still asking after {MAX_CLARIFICATIONS} answers"));
        report.feedback.push(Feedback::new(
            "tooManyQuestions",
            "I still couldn't work out what to do. Try saying it differently.",
            &[],
        ));
        return;
    }
    let waiting = Waiting::Answer(PendingAnswer {
        id: report.id,
        origin: scope.origin,
        transcript: scope.transcript.clone(),
        question: question.clone(),
        followup: scope.followup_after(report),
        dry_run: scope.ctx.dry_run,
        context: scope.ctx.context.clone(),
    });
    if !scope.state.offer(waiting) {
        report.superseded();
        return;
    }
    report.status = CommandStatus::NeedsAnswer;
    report.question = Some(question.clone());
    report.choices = choices;
    report.feedback.push(Feedback::new(
        "question",
        "{{question}}",
        &[("question", &question)],
    ));
}

/// One info line per command: what was asked, what ran, what it cost in
/// calls and tokens. Phase 4 adds the JSONL log beside it.
fn log_report(report: &CommandReport) {
    let tools: Vec<String> = report
        .steps
        .iter()
        .map(|s| format!("{}:{}", s.tool, s.result))
        .collect();
    info!(
        "command mode: {:?} {:?} via {} in {} ms, {} LLM call(s), tokens in/out/cache-read {}/{}/{}, tools [{}]{}{}",
        report.origin,
        report.status,
        if report.model.is_empty() { "-" } else { &report.model },
        report.latency_ms,
        report.llm_calls,
        report.usage.input_tokens,
        report.usage.output_tokens,
        report.usage.cache_read_tokens,
        tools.join(", "),
        if report.dry_run { " (dry run)" } else { "" },
        report
            .error
            .as_deref()
            .map(|e| format!(", error: {e}"))
            .unwrap_or_default()
    );
}

/// `app.state::<CommandModeState>()` without panicking when the state was
/// never managed (it is, in `lib.rs`; this only protects a future refactor).
trait StateOrDefault {
    fn state_or_default(&self) -> &CommandModeState;
}

impl StateOrDefault for AppHandle {
    fn state_or_default(&self) -> &CommandModeState {
        use tauri::Manager;
        static FALLBACK: std::sync::OnceLock<CommandModeState> = std::sync::OnceLock::new();
        match self.try_state::<CommandModeState>() {
            Some(state) => state.inner(),
            None => FALLBACK.get_or_init(CommandModeState::default),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::effects::fake::FakeEffects;
    use chrono::{FixedOffset, TimeZone};
    use serde_json::json;

    fn window(id: i64, app: &str) -> effects::WindowEntry {
        effects::WindowEntry {
            id,
            title: format!("{app} window"),
            app: app.into(),
        }
    }

    fn before(front: i64, open: &[i64], opened: &str) -> WindowsBefore {
        WindowsBefore {
            front: Some(front),
            open: open.iter().copied().collect(),
            opened: opened.into(),
        }
    }

    /// Typing after a launch waits for the launched app's window, brings it
    /// forward when it opened behind, and gives up rather than type into the
    /// window the user had in front.
    #[test]
    fn typing_waits_for_the_opened_window() {
        let short = Duration::from_millis(300);
        let wait = |fake: &FakeEffects, before: &WindowsBefore| {
            tauri::async_runtime::block_on(wait_for_new_window(fake, before, short))
        };

        // The new window came to the front.
        let fake = FakeEffects::default();
        *fake.foreground.lock().unwrap() = Some(window(2, "notepad"));
        assert!(wait(&fake, &before(1, &[1], "notepad")));

        // It opened behind: brought forward, then typed into.
        let fake = FakeEffects {
            windows: vec![window(1, "chrome"), window(5, "notepad")],
            focus_follows: true,
            ..Default::default()
        };
        *fake.foreground.lock().unwrap() = Some(window(1, "chrome"));
        assert!(wait(&fake, &before(1, &[1], "notepad")));
        assert_eq!(fake.actions(), vec!["focus_window 5".to_string()]);

        // Nothing came up: no typing.
        let fake = FakeEffects::default();
        *fake.foreground.lock().unwrap() = Some(window(1, "chrome"));
        assert!(!wait(&fake, &before(1, &[1], "notepad")));

        // The app was already in front and opened its new tab in place.
        let fake = FakeEffects::default();
        *fake.foreground.lock().unwrap() = Some(window(1, "notepad"));
        assert!(wait(&fake, &before(1, &[1], "Notepad")));
    }

    #[test]
    fn only_launches_are_waited_for_and_only_typing_waits() {
        assert!(opens_window("open_app") && opens_window("open_file"));
        assert!(!opens_window("open_url") && !opens_window("switch_to_app"));
        assert!(types_into_front("type_text") && types_into_front("press_keys"));
        assert!(!types_into_front("play_media"));
    }

    #[test]
    fn an_escalated_task_names_the_steps_already_done() {
        assert_eq!(
            with_steps_done("open notepad and type hi".into(), &[]),
            "open notepad and type hi"
        );
        let steps = vec![
            step("open_app", r#"{"name":"notepad"}"#, "done", ""),
            step("type_text", r#"{"text":"hi"}"#, "failed", "no field"),
        ];
        let task = with_steps_done("open notepad and type hi".into(), &completed_steps(&steps));
        assert!(task.starts_with("open notepad and type hi\n"));
        assert!(task.contains(r#"open_app({"name":"notepad"})"#));
        assert!(!task.contains("type_text"));
    }

    fn tool_ctx(fake: &Arc<FakeEffects>) -> Arc<ToolCtx> {
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 9, 0, 0)
            .unwrap();
        Arc::new(ToolCtx {
            settings: crate::settings::get_default_settings(),
            context: CommandContext::at(now, None, Platform::Windows, None, None),
            effects: fake.clone(),
            contacts: Arc::new(ContactBook::default()),
            platform: Platform::Windows,
            dry_run: false,
        })
    }

    fn scope<'a>(
        state: &'a CommandModeState,
        fake: &Arc<FakeEffects>,
        followup: Followup,
    ) -> Scope<'a> {
        Scope {
            state,
            ctx: tool_ctx(fake),
            dry: None,
            origin: CommandOrigin::Typed,
            transcript: "email Ron".into(),
            followup,
        }
    }

    /// Runs one `press_keys enter` plan for `transcript` with `args`, in a
    /// chat window that stays in front; the report and what was pressed.
    fn run_enter(transcript: &str, args: Value, ask_setting: bool) -> (CommandReport, Vec<String>) {
        let chat = effects::WindowEntry {
            id: 5,
            app: "Slack".into(),
            title: "general | Slack".into(),
        };
        let fake = Arc::new(FakeEffects {
            windows: vec![chat.clone()],
            foreground: std::sync::Mutex::new(Some(chat)),
            ..Default::default()
        });
        let state = CommandModeState::default();
        let id = state.begin();
        let mut scope = scope(&state, &fake, Followup::default());
        scope.transcript = transcript.into();
        if ask_setting {
            let mut settings = crate::settings::get_default_settings();
            settings.command_confirm_irreversible = true;
            scope.ctx = Arc::new(ToolCtx {
                settings,
                context: scope.ctx.context.clone(),
                effects: fake.clone(),
                contacts: Arc::new(ContactBook::default()),
                platform: Platform::Windows,
                dry_run: false,
            });
        }
        let plan = router::RoutePlan {
            calls: vec![PlannedCall {
                name: "press_keys".into(),
                args,
            }],
            dropped: Vec::new(),
            text: String::new(),
            usage: Usage::default(),
            llm_calls: 1,
        };
        let report = CommandReport::new(id, CommandOrigin::Typed, transcript, false);
        let report = tauri::async_runtime::block_on(finish(
            &scope,
            report,
            None,
            "mock/router".into(),
            Ok(plan),
            Instant::now(),
        ));
        (report, fake.actions())
    }

    /// What the user asked for is carried out, the step that sends included:
    /// no card, no Do it.
    #[test]
    fn a_held_step_is_carried_out_when_nobody_asked_to_review() {
        let (report, pressed) = run_enter("press enter", json!({"keys": "enter"}), false);
        assert_eq!(report.status, CommandStatus::Done, "{:?}", report.feedback);
        assert_eq!(report.pending_summary, None);
        assert_eq!(pressed, vec!["press_keys Enter".to_string()]);
        assert_eq!(report.steps[0].result, "done");
    }

    /// It waits when the user's words ask, when the router marks the call, or
    /// when "Ask before sending or deleting" is on.
    #[test]
    fn a_held_step_waits_when_review_was_asked_for() {
        for (transcript, args, setting) in [
            (
                "press enter but ask before sending",
                json!({"keys": "enter"}),
                false,
            ),
            (
                "press enter",
                json!({"keys": "enter", "confirm_first": true}),
                false,
            ),
            ("press enter", json!({"keys": "enter"}), true),
        ] {
            let (report, pressed) = run_enter(transcript, args, setting);
            assert_eq!(
                report.status,
                CommandStatus::AwaitingConfirmation,
                "{transcript}: {:?}",
                report.feedback
            );
            assert!(pressed.is_empty(), "{transcript}: {pressed:?}");
        }
    }

    #[test]
    fn only_a_joined_command_that_stopped_at_an_opener_is_continued() {
        let one = |name: &str| vec![name.to_string()];
        assert!(wants_continuation(
            "open notepad and type hello",
            &one("open_app")
        ));
        assert!(wants_continuation(
            "notepad khola ani hello lekha",
            &one("open_app")
        ));
        assert!(wants_continuation(
            "go to gmail, then search for invoices",
            &one("open_url")
        ));
        // One part, a plan of the whole thing, or no opener.
        assert!(!wants_continuation("open notepad", &one("open_app")));
        assert!(!wants_continuation(
            "open youtube and play believer",
            &one("play_media")
        ));
        assert!(!wants_continuation(
            "open notepad and type hello",
            &["open_app".to_string(), "type_text".to_string()]
        ));
        // "ra" is a word of its own, not part of one ("random").
        assert!(!wants_continuation("open random app", &one("open_app")));
    }

    /// "Open Notepad and type hello" planned as the open alone: the rest is
    /// routed again, told what ran, and only the new step runs.
    #[test]
    fn the_rest_of_a_joined_command_is_routed_and_run() {
        use agent::testing::{answer, ScriptedLlm};
        let fake = Arc::new(FakeEffects {
            apps: vec![effects::AppEntry {
                name: "Notepad".into(),
                launch: "notepad.lnk".into(),
            }],
            ..Default::default()
        });
        let state = CommandModeState::default();
        let id = state.begin();
        let mut scope = scope(&state, &fake, Followup::default());
        scope.transcript = "open notepad and type hello".into();
        let plan = router::RoutePlan {
            calls: vec![PlannedCall {
                name: "open_app".into(),
                args: json!({"name": "notepad"}),
            }],
            dropped: Vec::new(),
            text: String::new(),
            usage: Usage::default(),
            llm_calls: 1,
        };
        let llm = ScriptedLlm::new(vec![answer(vec![
            ("open_app", json!({"name": "notepad"})),
            ("type_text", json!({"text": "hello"})),
        ])]);
        let report = CommandReport::new(id, CommandOrigin::Typed, &scope.transcript, false);
        let report = tauri::async_runtime::block_on(finish(
            &scope,
            report,
            Some(&llm),
            "mock/router".into(),
            Ok(plan),
            Instant::now(),
        ));
        // The second request listed the open as done.
        let requests = llm.requests();
        assert_eq!(requests.len(), 1);
        assert!(format!("{:?}", requests[0].turns).contains("Already done"));
        // The open ran once; the typing came next and, since no window came
        // up in the fake, was refused rather than typed elsewhere.
        let tools: Vec<&str> = report.steps.iter().map(|s| s.tool.as_str()).collect();
        assert_eq!(tools, vec!["open_app", "type_text"]);
        assert_eq!(report.llm_calls, 2);
        assert_eq!(keys(&report).last(), Some(&"windowNotReady"));
        assert!(!fake.actions().iter().any(|a| a.starts_with("type_text")));
    }

    fn pending_action() -> PendingAction {
        PendingAction {
            tool: "compose_email",
            summary: "Send email to Ron".into(),
            payload: json!({}),
        }
    }

    fn confirmation(id: u32, fake: &Arc<FakeEffects>, created: Instant) -> PendingConfirmation {
        PendingConfirmation {
            id,
            action: pending_action(),
            ctx: tool_ctx(fake),
            dry: None,
            origin: CommandOrigin::Typed,
            transcript: "email Ron".into(),
            followup: Followup::default(),
            created,
        }
    }

    fn answer(id: u32, fake: &Arc<FakeEffects>) -> PendingAnswer {
        PendingAnswer {
            id,
            origin: CommandOrigin::Typed,
            transcript: "email Ron".into(),
            question: "Which Ron?".into(),
            followup: Followup::default(),
            dry_run: false,
            context: tool_ctx(fake).context.clone(),
        }
    }

    /// The feedback keys, without the `commandMode.feedback.` namespace every
    /// one of them carries, so assertions read like the `Feedback::new` calls.
    fn keys(report: &CommandReport) -> Vec<&str> {
        report
            .feedback
            .iter()
            .map(|f| {
                f.key
                    .strip_prefix("commandMode.feedback.")
                    .unwrap_or(&f.key)
            })
            .collect()
    }

    /// A new command takes the slot: what waited is gone, and the older
    /// command can no longer leave anything there.
    #[test]
    fn a_new_command_clears_what_was_waiting_and_owns_the_slot() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let first = state.begin();
        assert!(state.offer(Waiting::Answer(answer(first, &fake))));
        let second = state.begin();
        assert_ne!(first, second);
        assert_eq!(state.take_answer(first).err(), Some(NotWaiting::Nothing));
        assert!(!state.offer(Waiting::Answer(answer(first, &fake))));
        assert!(state.offer(Waiting::Answer(answer(second, &fake))));
        assert!(state.take_answer(second).is_ok());
    }

    /// Command A starts, then B; B finishes first and leaves a confirmation.
    /// A finishing late must not replace it, or B's Send stops working.
    #[test]
    fn an_older_command_finishing_late_cannot_replace_a_newer_ones_confirmation() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let a = state.begin();
        let b = state.begin();
        assert!(state.offer(Waiting::Confirmation(confirmation(
            b,
            &fake,
            Instant::now()
        ))));
        assert!(!state.offer(Waiting::Confirmation(confirmation(
            a,
            &fake,
            Instant::now()
        ))));
        assert!(!state.offer(Waiting::Answer(answer(a, &fake))));
        assert_eq!(state.take_confirmation(a).err(), Some(NotWaiting::Nothing));
        assert!(state.take_confirmation(b).is_ok());
    }

    /// A reply of the wrong kind (an answer to a pending send, a confirm of
    /// a pending question) says so and leaves the item where it was.
    #[test]
    fn a_reply_of_the_wrong_kind_leaves_the_pending_item_in_place() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        assert!(state.offer(Waiting::Confirmation(confirmation(
            id,
            &fake,
            Instant::now()
        ))));
        assert_eq!(state.take_answer(id).err(), Some(NotWaiting::OtherKind));
        assert_eq!(state.take_answer(id + 1).err(), Some(NotWaiting::Nothing));
        assert!(state.take_confirmation(id).is_ok());

        let id = state.begin();
        assert!(state.offer(Waiting::Answer(answer(id, &fake))));
        assert_eq!(
            state.take_confirmation(id).err(),
            Some(NotWaiting::OtherKind)
        );
        assert!(state.take_answer(id).is_ok());
        assert_eq!(state.take_answer(id).err(), Some(NotWaiting::Nothing));
    }

    /// A late finisher's report must not offer a Send that cannot work.
    #[test]
    fn a_superseded_confirmation_reports_cancelled_instead_of_waiting() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let old = state.begin();
        state.begin();
        let scope = scope(&state, &fake, Followup::default());
        let mut report = CommandReport::new(old, CommandOrigin::Typed, "email Ron", false);
        report.steps.push(step("compose_email", "{}", "", ""));
        let stop = apply_outcome(
            &scope,
            &mut report,
            ToolOutcome::NeedsConfirmation {
                pending: pending_action(),
                feedback: Feedback::new("emailDraft", "Draft ready.", &[]),
            },
        );
        assert!(stop);
        assert_eq!(report.status, CommandStatus::Cancelled);
        assert_eq!(report.pending_summary, None);
        assert_eq!(keys(&report), vec!["emailDraft", "superseded"]);
        assert_eq!(
            state.take_confirmation(old).err(),
            Some(NotWaiting::Nothing)
        );
    }

    /// The question keeps every earlier exchange and adds the steps of this
    /// run that changed something, so the answer's re-route repeats none.
    #[test]
    fn a_question_carries_the_earlier_answers_and_the_steps_already_done() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let earlier = Followup {
            exchanges: vec![("Which Ron?".into(), "Weasley".into())],
            already_done: vec![r#"open_url({"url":"u"})"#.into()],
        };
        let scope = scope(&state, &fake, earlier);
        let mut report = CommandReport::new(id, CommandOrigin::Typed, "email Ron", false);
        report
            .steps
            .push(step("type_text", r#"{"text":"hi team"}"#, "done", ""));
        report
            .steps
            .push(step(router::SAY, r#"{"text":"ok"}"#, "done", ""));
        report
            .steps
            .push(step("open_app", r#"{"name":"x"}"#, "failed", ""));
        report
            .steps
            .push(step("compose_email", r#"{"to":"Ron"}"#, "", ""));
        apply_outcome(&scope, &mut report, ToolOutcome::ask("What time?", vec![]));

        assert_eq!(report.status, CommandStatus::NeedsAnswer);
        assert_eq!(report.question.as_deref(), Some("What time?"));
        let pending = state.take_answer(id).unwrap();
        assert_eq!(pending.question, "What time?");
        assert_eq!(
            pending.followup.exchanges,
            vec![("Which Ron?".to_string(), "Weasley".to_string())]
        );
        assert_eq!(
            pending.followup.already_done,
            vec![
                r#"open_url({"url":"u"})"#.to_string(),
                r#"type_text({"text":"hi team"})"#.to_string(),
            ]
        );
    }

    /// After three answered questions a fourth is a failure, not a loop.
    #[test]
    fn a_fourth_question_fails_instead_of_asking_again() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let exchanges = (0..MAX_CLARIFICATIONS)
            .map(|i| (format!("q{i}"), format!("a{i}")))
            .collect();
        let scope = scope(
            &state,
            &fake,
            Followup {
                exchanges,
                already_done: vec![],
            },
        );
        let mut report = CommandReport::new(id, CommandOrigin::Typed, "email Ron", false);
        report
            .steps
            .push(step(router::ASK_USER, "{}", "asked", "Which Ron?"));
        ask(&scope, &mut report, "Which Ron?".into(), vec![]);
        assert_eq!(report.status, CommandStatus::Failed);
        assert_eq!(report.question, None);
        assert_eq!(keys(&report), vec!["tooManyQuestions"]);
        assert_eq!(report.steps[0].result, "failed");
        assert_eq!(state.take_answer(id).err(), Some(NotWaiting::Nothing));
    }

    /// Only a spoken command may use the window a dictation remembered.
    #[test]
    fn only_a_command_that_remembered_its_own_target_keeps_it() {
        assert!(CommandOrigin::Voice.keeps_dictation_target());
        assert!(CommandOrigin::Assistant.keeps_dictation_target());
        assert!(!CommandOrigin::Typed.keeps_dictation_target());
        assert!(!CommandOrigin::Cli.keeps_dictation_target());
        assert!(!CommandOrigin::Phone.keeps_dictation_target());
    }

    #[test]
    fn confirmations_expire_by_origin() {
        assert_eq!(CommandOrigin::Typed.confirm_ttl(), Duration::from_secs(120));
        assert_eq!(CommandOrigin::Cli.confirm_ttl(), Duration::from_secs(120));
        assert_eq!(CommandOrigin::Voice.confirm_ttl(), Duration::from_secs(30));
        assert_eq!(
            CommandOrigin::Assistant.confirm_ttl(),
            Duration::from_secs(120)
        );
        assert_eq!(CommandOrigin::Phone.confirm_ttl(), Duration::from_secs(120));
    }

    /// "Send" long after the draft opened runs nothing: the draft and the
    /// windows around it may have changed since.
    #[test]
    fn an_expired_confirmation_runs_nothing() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let Some(created) = Instant::now().checked_sub(CONFIRM_TTL + Duration::from_secs(5)) else {
            return;
        };
        let report = tauri::async_runtime::block_on(confirm_taken(
            &state,
            confirmation(7, &fake, created),
            true,
            Instant::now(),
        ));
        assert_eq!(report.status, CommandStatus::Cancelled);
        assert_eq!(keys(&report), vec!["confirmExpired"]);
        assert!(report.steps.is_empty());
        assert!(fake.actions().is_empty());
    }

    /// Declining is always a plain cancel, expired or not.
    #[test]
    fn declining_a_confirmation_cancels_it() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let report = tauri::async_runtime::block_on(confirm_taken(
            &state,
            confirmation(7, &fake, Instant::now()),
            false,
            Instant::now(),
        ));
        assert_eq!(report.status, CommandStatus::Cancelled);
        assert_eq!(keys(&report), vec!["cancelled"]);
    }

    /// A fresh confirmation gets past the expiry check to the tool's own
    /// confirm step (here a tool that has none).
    #[test]
    fn a_fresh_confirmation_reaches_the_confirm_step() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let mut pending = confirmation(7, &fake, Instant::now());
        pending.action.tool = "no_such_tool";
        let report =
            tauri::async_runtime::block_on(confirm_taken(&state, pending, true, Instant::now()));
        assert_eq!(report.status, CommandStatus::Failed);
        assert_eq!(keys(&report), vec!["confirmFailed"]);
    }

    /// A step that never returns ends the command with a clear failure
    /// instead of leaving it, and the Debug box, waiting forever.
    #[test]
    fn a_step_that_runs_too_long_fails_the_command() {
        let outcome = tauri::async_runtime::block_on(run_blocking(
            Duration::from_millis(20),
            "slow_tool",
            || {
                std::thread::sleep(Duration::from_millis(300));
                ToolOutcome::done("ok", "ok", &[])
            },
        ));
        let ToolOutcome::Failed { reason, escalate } = outcome else {
            panic!("not a failure: {outcome:?}");
        };
        assert!(reason.contains("took too long"), "{reason}");
        assert!(!escalate);

        let quick = tauri::async_runtime::block_on(run_blocking(
            Duration::from_secs(5),
            "quick_tool",
            || ToolOutcome::done("ok", "ok", &[]),
        ));
        assert!(matches!(quick, ToolOutcome::Done { .. }));
    }

    /// The calls past the plan cap show as skipped steps with a line saying
    /// part of the command was left out.
    #[test]
    fn calls_over_the_plan_cap_are_reported_as_skipped() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let scope = scope(&state, &fake, Followup::default());
        let plan = router::RoutePlan {
            calls: vec![PlannedCall {
                name: router::SAY.into(),
                args: json!({"text": "On it."}),
            }],
            dropped: vec![
                PlannedCall {
                    name: "press_keys".into(),
                    args: json!({"keys": "tab"}),
                },
                PlannedCall {
                    name: "type_text".into(),
                    args: json!({"text": "yes"}),
                },
            ],
            text: String::new(),
            usage: Usage::default(),
            llm_calls: 1,
        };
        let report = CommandReport::new(id, CommandOrigin::Typed, "fill it in", false);
        let report = tauri::async_runtime::block_on(finish(
            &scope,
            report,
            None,
            "mock/router".into(),
            Ok(plan),
            Instant::now(),
        ));
        let results: Vec<(&str, &str)> = report
            .steps
            .iter()
            .map(|s| (s.tool.as_str(), s.result.as_str()))
            .collect();
        assert_eq!(
            results,
            vec![
                (router::SAY, "done"),
                ("press_keys", "skipped"),
                ("type_text", "skipped")
            ]
        );
        assert!(
            report.steps[1].detail.contains("step limit"),
            "{}",
            report.steps[1].detail
        );
        assert_eq!(keys(&report), vec!["say", "planCapped"]);
        assert_eq!(
            report.feedback[1].render_en(),
            "That was too many steps at once, so I left out the last 2."
        );
        assert!(fake.actions().is_empty());
    }

    /// A routing error keeps its cost and runs nothing.
    #[test]
    fn a_routing_error_fails_the_report_with_its_cost() {
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let scope = scope(&state, &fake, Followup::default());
        let report = CommandReport::new(id, CommandOrigin::Typed, "x", false);
        let spent = Usage {
            input_tokens: 40,
            output_tokens: 4,
            ..Usage::default()
        };
        let report = tauri::async_runtime::block_on(finish(
            &scope,
            report,
            None,
            "mock/router".into(),
            Err(RouteError::OutputLimit {
                llm_calls: 2,
                usage: spent,
            }),
            Instant::now(),
        ));
        assert_eq!(report.status, CommandStatus::Failed);
        assert_eq!(report.llm_calls, 2);
        assert_eq!(report.usage, spent);
        assert_eq!(keys(&report), vec!["routeFailed"]);
    }

    fn agent_tree() -> Arc<agent::testing::FakeTree> {
        Arc::new(agent::testing::FakeTree::with(vec![
            agent::testing::compose_window(),
        ]))
    }

    /// A finished agent task is a done command: its steps, requests and
    /// model join the report, and a tool failure that led there is cleared.
    #[test]
    fn an_agent_that_finishes_makes_the_command_done() {
        use agent::testing::{answer, ScriptedLlm};
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let scope = scope(&state, &fake, Followup::default());
        let mut report = CommandReport::new(id, CommandOrigin::Typed, "email Ron", false);
        report.model = "mock/router".into();
        report.llm_calls = 1;
        report.status = CommandStatus::NeedsComputerUse;
        report.error = Some("no such app".into());
        let llm = ScriptedLlm::new(vec![
            answer(vec![("invoke", json!({"element": 3}))]),
            answer(vec![("done", json!({"summary": "Made it bold."}))]),
        ]);
        let tree = agent_tree();
        tauri::async_runtime::block_on(run_agent_into(
            &scope,
            &mut report,
            "make it bold",
            &llm,
            tree.clone(),
            &AtomicBool::new(false),
            false,
        ));
        assert_eq!(report.status, CommandStatus::Done);
        assert_eq!(report.error, None);
        assert_eq!(report.llm_calls, 3);
        assert_eq!(report.model, "mock/router + mock/mock-agent");
        assert_eq!(keys(&report), vec!["agentWorking", "agentDone"]);
        assert_eq!(
            report.feedback[0].render_en(),
            "Working on it in chrome\u{2026}"
        );
        let tools: Vec<&str> = report.steps.iter().map(|s| s.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec!["agent:read_window", "agent:invoke", "agent:done"]
        );
        assert_eq!(tree.acted().len(), 1);
    }

    /// A Send the agent held waits like a tool's confirmation; saying yes
    /// finds the button again by its label and presses it.
    #[test]
    fn a_held_agent_action_waits_and_runs_on_confirm() {
        use agent::testing::{answer, ScriptedLlm};
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let scope = scope(&state, &fake, Followup::default());
        let mut report = CommandReport::new(id, CommandOrigin::Typed, "email Ron", false);
        let llm = ScriptedLlm::new(vec![answer(vec![("invoke", json!({"element": 14}))])]);
        let tree = agent_tree();
        tauri::async_runtime::block_on(run_agent_into(
            &scope,
            &mut report,
            "send it",
            &llm,
            tree.clone(),
            &AtomicBool::new(false),
            true,
        ));
        assert_eq!(report.status, CommandStatus::AwaitingConfirmation);
        assert!(report.pending_summary.as_deref().unwrap().contains("Send"));
        assert!(tree.acted().is_empty());

        let pending = state.take_confirmation(id).unwrap();
        assert_eq!(pending.action.tool, agent::CONFIRM_TOOL);
        let confirmed = tauri::async_runtime::block_on(confirm_taken_with(
            &state,
            pending,
            true,
            Instant::now(),
            Some(tree.clone() as Arc<dyn agent::tree::AccessibilityTree>),
        ));
        assert_eq!(
            confirmed.status,
            CommandStatus::Done,
            "{:?}",
            confirmed.error
        );
        assert_eq!(keys(&confirmed), vec!["agentConfirmed"]);
        assert_eq!(tree.acted(), vec![(14, agent::tree::UiAction::Invoke)]);
    }

    /// Esc during the agent leaves a cancelled command, not a failure.
    #[test]
    fn a_stopped_agent_cancels_the_command() {
        use agent::testing::ScriptedLlm;
        let fake = Arc::new(FakeEffects::default());
        let state = CommandModeState::default();
        let id = state.begin();
        let scope = scope(&state, &fake, Followup::default());
        let mut report = CommandReport::new(id, CommandOrigin::Typed, "x", false);
        let llm = ScriptedLlm::new(vec![]);
        tauri::async_runtime::block_on(run_agent_into(
            &scope,
            &mut report,
            "x",
            &llm,
            agent_tree(),
            &AtomicBool::new(true),
            false,
        ));
        assert_eq!(report.status, CommandStatus::Cancelled);
        assert_eq!(keys(&report), vec!["agentWorkingNoApp", "agentCancelled"]);
    }

    /// With the switch off, an escalation still stops at "needs computer
    /// use" and says how to turn the agent on.
    #[test]
    fn with_auto_escalate_off_an_escalation_stays_needs_computer_use() {
        let mut settings = crate::settings::get_default_settings();
        settings.command_auto_escalate = false;
        assert!(!agent_enabled(&settings));
        let feedback = computer_use_unavailable(&settings);
        if agent::supported() {
            assert_eq!(feedback.key, "commandMode.feedback.agentOff");
        } else {
            assert_eq!(feedback.key, "commandMode.feedback.needsComputerUse");
        }
    }
}
