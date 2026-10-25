//! Command mode by voice: the `command` shortcut (the ask keys plus Shift).
//!
//! The shortcut records through the transcription coordinator like the ask
//! (`actions::CommandAction`), so Hold or Tap, Esc and the processing cap all
//! work as they do there. The recording is transcribed like an ask (local or
//! cloud, never cleaned up, never pasted) and the transcript runs as a command
//! ([`super::run_command`] with [`CommandOrigin::Voice`]).
//!
//! What the user sees:
//!
//! - while it records, the recording pill; while it is transcribed and
//!   carried out, the pill's working state ("Working on it…");
//! - when it is done, a small card in the corner: what was heard and what
//!   happened. A command waiting for "send" shows what will be sent with Send
//!   and Cancel; one asking a question shows the question with its choices.
//!
//! The card lives in the phone-approval window (`account::approval`), which
//! already sits in the corner, stays on top and never takes the keyboard from
//! the user's app, which matters here as much as there: a confirmed draft is
//! sent with Ctrl+Enter into the window the user is in. A card that only
//! reports closes itself (the window does that, so hovering keeps it); a card
//! that waits stays until it is answered or what it waits for expires
//! ([`card_lifetime`]).
//!
//! Esc stops a spoken command at any point: while it records (the recording
//! is dropped), while it is transcribed, and while it is carried out (the run
//! is abandoned before its next step, see [`until_cancelled`]).

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

use log::{debug, info, warn};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter};

use super::context::CommandContext;
use super::{CommandOrigin, CommandReport, CommandStatus};

/// How long a question card waits for an answer. The executor keeps a
/// question until a newer command replaces it; the card does not need to sit
/// in the corner that long.
const QUESTION_CARD_TTL: Duration = Duration::from_secs(120);

/// The longest a card that only reports stays up. The window closes it after
/// a few seconds itself (and keeps it while the pointer is on it); this is
/// the backstop for a window that never loaded.
const REPORT_CARD_BACKSTOP: Duration = Duration::from_secs(60);

/// How long the run waits for the context captured at recording start before
/// it captures one itself.
const CONTEXT_WAIT: Duration = Duration::from_millis(1500);

/// How often a running command looks at the cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// A spoken command is being transcribed or carried out.
static BUSY: AtomicBool = AtomicBool::new(false);

/// The context captured when the command's recording started.
static CONTEXT: Mutex<Option<Receiver<CommandContext>>> = Mutex::new(None);

/// Numbers every card shown, so an action or a timer aimed at one card never
/// touches the card that replaced it.
static CARD_SEQ: AtomicU32 = AtomicU32::new(0);

/// Whether a spoken command is being transcribed or carried out, so Esc has
/// something to stop after the recording has ended.
pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

/// Marks a spoken command as running for as long as it is held.
pub struct Busy(());

impl Busy {
    pub fn begin() -> Self {
        BUSY.store(true, Ordering::SeqCst);
        Busy(())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// A spoken command's recording has started.
///
/// An Esc from before this command (a dictation cancelled an hour ago) must
/// not stop it, so the cancel flag starts lowered here rather than when the
/// command runs: an Esc pressed while it is transcribed is then still seen.
/// The context (time, the app in front and its window title) is captured now,
/// on its own thread, while the user's app is still the one in front.
pub fn begin() {
    super::reset_cancel();
    let (sender, receiver) = mpsc::channel();
    *CONTEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(receiver);
    std::thread::spawn(move || {
        let _ = sender.send(CommandContext::capture());
    });
}

/// Whether Esc (or `--cancel`) stopped the spoken command.
pub fn cancelled() -> bool {
    super::CANCEL.load(Ordering::SeqCst)
}

/// The context [`begin`] captured, if it arrives in time.
async fn captured_context() -> Option<CommandContext> {
    let receiver = CONTEXT.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(CONTEXT_WAIT).ok())
        .await
        .ok()
        .flatten()
}

/// Run `run` until it finishes, or until `stopped` answers true, checked every
/// [`CANCEL_POLL`]. A run that is stopped is dropped: an LLM request in flight
/// is abandoned, and no further step starts (one already running on the
/// blocking pool finishes on its own, as a timed-out step does).
async fn until_cancelled<F: Future>(run: F, stopped: impl Fn() -> bool) -> Option<F::Output> {
    tokio::pin!(run);
    loop {
        tokio::select! {
            biased;
            output = &mut run => return Some(output),
            _ = tokio::time::sleep(CANCEL_POLL) => {
                if stopped() {
                    return None;
                }
            }
        }
    }
}

/// Carry out a spoken command and show how it went.
///
/// Called by the command shortcut's pipeline once the recording is
/// transcribed, with the coordinator still holding it, so no dictation can
/// start while the command types into the user's app.
pub async fn run_spoken(app: &AppHandle, transcript: String) {
    if cancelled() {
        debug!("Spoken command cancelled before it ran");
        return;
    }
    crate::overlay::show_command_overlay(app);
    // A spoken command while the card asks a question is the answer to it:
    // the card says to say the answer, and run as a command of its own
    // ("the work one") it would mean nothing.
    let question = question_on_card(current_card());
    let context = match question {
        Some(_) => None,
        None => captured_context().await,
    };
    // `run_command` and `answer_command` lower the cancel flag as they start,
    // so an Esc pressed while the context was awaited is honoured here.
    if cancelled() {
        info!("Spoken command stopped by the user");
        crate::utils::hide_recording_overlay(app);
        return;
    }
    let run = async {
        match question {
            Some((seq, id)) => {
                info!("Spoken command answers the question on card {seq}");
                super::answer_command(app, id, &transcript).await
            }
            None => super::run_command(app, &transcript, CommandOrigin::Voice, None, context).await,
        }
    };
    let Some(report) = until_cancelled(run, cancelled).await else {
        info!("Spoken command stopped by the user");
        crate::utils::hide_recording_overlay(app);
        return;
    };
    crate::utils::hide_recording_overlay(app);
    if let Err(e) = app.emit(crate::signal_handle::COMMAND_REPORT_EVENT, &report) {
        warn!("Could not send the command report to the windows: {e}");
    }
    show_card(app, report);
}

/// One command's report, as the corner card shows it. `seq` changes every
/// time the card does, so an answer or a timer for an earlier card is ignored.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct CommandCard {
    pub seq: u32,
    pub report: CommandReport,
}

/// How long a card stays before it is taken down from here. A confirmation
/// lasts as long as the executor would still accept it
/// ([`CommandOrigin::confirm_ttl`]); after that Send could only fail. A
/// question gets [`QUESTION_CARD_TTL`]. A card that only reports is closed by
/// the window after a few seconds, hover permitting, and by
/// [`REPORT_CARD_BACKSTOP`] if it never is.
fn card_lifetime(report: &CommandReport) -> Duration {
    match report.status {
        CommandStatus::AwaitingConfirmation => report.origin.confirm_ttl(),
        CommandStatus::NeedsAnswer => QUESTION_CARD_TTL,
        CommandStatus::Done
        | CommandStatus::NeedsComputerUse
        | CommandStatus::Cancelled
        | CommandStatus::Failed => REPORT_CARD_BACKSTOP,
    }
}

/// Show `report` on the corner card, replacing whatever card was there.
pub fn show_card(app: &AppHandle, report: CommandReport) {
    let seq = CARD_SEQ.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    let lifetime = card_lifetime(&report);
    crate::account::approval::set_command_card(app, Some(CommandCard { seq, report }));
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(lifetime);
        if close_card(&app, seq) {
            debug!("Command card {seq} expired");
        }
    });
}

/// The card on screen now, if any.
pub fn current_card() -> Option<CommandCard> {
    crate::account::approval::command_card()
}

/// Take card `seq` down. Answers whether it was the one on screen.
pub fn close_card(app: &AppHandle, seq: u32) -> bool {
    crate::account::approval::close_command_card(app, seq)
}

/// The card's seq and its command's id, when the card on screen asks a
/// question of a command spoken at this computer. A question a phone or the
/// assistant asked is answered there, not by the next spoken command.
fn question_on_card(card: Option<CommandCard>) -> Option<(u32, u32)> {
    card.filter(|card| {
        card.report.status == CommandStatus::NeedsAnswer
            && card.report.origin == CommandOrigin::Voice
    })
    .map(|card| (card.seq, card.report.id))
}

/// What a click on the card is aimed at: the report of card `seq`, while it
/// is the one on screen and waits for `status`.
fn aimed_at(card: Option<CommandCard>, seq: u32, status: CommandStatus) -> Option<CommandReport> {
    card.filter(|card| card.seq == seq && card.report.status == status)
        .map(|card| card.report)
}

/// Send (or Cancel) on a card that waits for confirmation. A declined
/// command's card goes away; an accepted one shows how the send went.
pub async fn confirm_from_card(app: &AppHandle, seq: u32, accept: bool) {
    let Some(report) = aimed_at(current_card(), seq, CommandStatus::AwaitingConfirmation) else {
        debug!("Ignoring a confirmation for command card {seq}: it is not waiting");
        return;
    };
    // On macOS a click on the card made Talvio the active app; the send's key
    // press belongs to the user's window.
    crate::account::approval::hand_back_focus(app).await;
    let result = super::confirm_command(app, report.id, accept).await;
    let _ = app.emit(crate::signal_handle::COMMAND_REPORT_EVENT, &result);
    if accept {
        show_card(app, result);
    } else {
        close_card(app, seq);
    }
}

/// An answer to a card's question, from one of its choices. The command
/// carries on, and its card shows what happened next.
pub async fn answer_from_card(app: &AppHandle, seq: u32, answer: &str) {
    let Some(report) = aimed_at(current_card(), seq, CommandStatus::NeedsAnswer) else {
        debug!("Ignoring an answer for command card {seq}: it is not asking");
        return;
    };
    crate::account::approval::hand_back_focus(app).await;
    let result = super::answer_command(app, report.id, answer).await;
    let _ = app.emit(crate::signal_handle::COMMAND_REPORT_EVENT, &result);
    show_card(app, result);
}

/// The card was closed (its own timer in the window, Cancel on a question,
/// or the window's close). A confirmation it held is declined, so nothing is
/// sent after its card is gone.
pub async fn dismiss_card(app: &AppHandle, seq: u32) {
    let waiting = aimed_at(current_card(), seq, CommandStatus::AwaitingConfirmation);
    if !close_card(app, seq) {
        return;
    }
    if let Some(report) = waiting {
        let _ = super::confirm_command(app, report.id, false).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(status: CommandStatus, origin: CommandOrigin) -> CommandReport {
        let mut report = CommandReport::new(7, origin, "open YouTube", false);
        report.status = status;
        report
    }

    #[test]
    fn a_waiting_card_lasts_as_long_as_its_confirmation() {
        let voice = report(CommandStatus::AwaitingConfirmation, CommandOrigin::Voice);
        assert_eq!(card_lifetime(&voice), CommandOrigin::Voice.confirm_ttl());
        let typed = report(CommandStatus::AwaitingConfirmation, CommandOrigin::Typed);
        assert_eq!(card_lifetime(&typed), CommandOrigin::Typed.confirm_ttl());
        let question = report(CommandStatus::NeedsAnswer, CommandOrigin::Voice);
        assert_eq!(card_lifetime(&question), QUESTION_CARD_TTL);
    }

    #[test]
    fn a_reporting_card_only_has_the_backstop() {
        for status in [
            CommandStatus::Done,
            CommandStatus::Failed,
            CommandStatus::Cancelled,
            CommandStatus::NeedsComputerUse,
        ] {
            let card = report(status, CommandOrigin::Voice);
            assert_eq!(card_lifetime(&card), REPORT_CARD_BACKSTOP, "{status:?}");
        }
    }

    /// A click acts only on the card it was made on, and only when that card
    /// waits for what the click answers.
    #[test]
    fn a_click_only_reaches_the_card_it_was_aimed_at() {
        let card = Some(CommandCard {
            seq: 3,
            report: report(CommandStatus::AwaitingConfirmation, CommandOrigin::Voice),
        });
        assert!(aimed_at(card.clone(), 3, CommandStatus::AwaitingConfirmation).is_some());
        // A newer card has replaced it.
        assert!(aimed_at(card.clone(), 2, CommandStatus::AwaitingConfirmation).is_none());
        // An answer to a card that waits for a confirmation.
        assert!(aimed_at(card, 3, CommandStatus::NeedsAnswer).is_none());
        assert!(aimed_at(None, 3, CommandStatus::NeedsAnswer).is_none());
    }

    #[test]
    fn only_a_spoken_question_takes_the_next_spoken_command_as_its_answer() {
        let card = |status, origin| {
            Some(CommandCard {
                seq: 4,
                report: report(status, origin),
            })
        };
        assert_eq!(
            question_on_card(card(CommandStatus::NeedsAnswer, CommandOrigin::Voice)),
            Some((4, 7))
        );
        assert_eq!(
            question_on_card(card(CommandStatus::NeedsAnswer, CommandOrigin::Assistant)),
            None
        );
        assert_eq!(
            question_on_card(card(
                CommandStatus::AwaitingConfirmation,
                CommandOrigin::Voice
            )),
            None
        );
        assert_eq!(question_on_card(None), None);
    }

    #[test]
    fn busy_lasts_as_long_as_its_guard() {
        {
            let _busy = Busy::begin();
            assert!(is_busy());
        }
        assert!(!is_busy());
    }

    #[tokio::test]
    async fn a_run_that_finishes_is_returned() {
        let out = until_cancelled(async { 5 }, || false).await;
        assert_eq!(out, Some(5));
    }

    #[tokio::test]
    async fn a_stopped_run_is_abandoned() {
        let out = until_cancelled(std::future::pending::<u8>(), || true).await;
        assert_eq!(out, None);
    }
}
