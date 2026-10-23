//! What a phone request may do on this computer, behind a trait so the
//! request logic runs in tests without an app.

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use log::{error, warn};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use super::{AccountPhone, APPROVAL_TIMEOUT};
use crate::assistant::phone::{PhoneAnswer, PhoneAsk, PhoneAskError};
use crate::clipboard::PasteBehavior;
use crate::command_mode::effects::{Effects as _, KeyCombo, SystemEffects, WindowEntry};

use super::screen::audio::{AudioStream, OpusPacket};
use super::screen::input::{self, Button, ControlInfo, Rect};
use super::screen::rtc::IceServer;
use super::screen::session::Viewing;
use super::screen::video::{CaptureTarget, EncodedFrame, ScreenInfo, VideoSettings, VideoStream};
use super::{Ask, CommandFuture};
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, PasteMethod};
use crate::speech_readiness::SpeechReadiness;

/// The paste waits on the main thread and a clipboard transaction; anything
/// past this is a paste that is not coming.
const PASTE_TIMEOUT: Duration = Duration::from_secs(15);

/// How a paste at the cursor ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pasted {
    Done,
    /// The paste method is None: this computer pastes nothing anywhere.
    Off,
    Failed,
}

/// Another device's text never gets the Enter that Auto-submit adds to a
/// dictation made here, and goes where the cursor is now, never back to the
/// window a local dictation or ask started in.
const PHONE_PASTE: PasteBehavior = PasteBehavior {
    allow_trailing_space: true,
    allow_auto_submit: false,
    smart_spacing: None,
    restore_target: false,
};

pub trait Host: Send + Sync + 'static {
    /// The "Phone dictation" switch.
    fn remote_input_enabled(&self) -> bool;
    /// Whether speech can be turned into text right now (a downloaded,
    /// current model, or cloud transcription).
    fn speech_ready(&self) -> bool;
    /// 16 kHz mono PCM to final text, `""` when nothing was said, `None` when
    /// transcription failed.
    fn transcribe(&self, samples: Vec<f32>) -> BoxFuture<'static, Option<String>>;
    /// Paste at the cursor, as a dictation does, but never followed by Enter.
    fn paste(&self, text: String) -> BoxFuture<'static, Pasted>;
    /// Keep a dictation (its WAV file and transcript) in local History.
    fn save_history(&self, wav: Vec<u8>, text: String);
    /// Whether this computer's user allowed `phone` (by device id) to use it.
    fn approved(&self, phone: &str) -> bool;
    /// Whether they said Don't allow to it in this run of the app.
    fn refused(&self, phone: &str) -> bool;
    /// Keep their answer: Allow is saved, Don't allow lasts for this run.
    fn remember(&self, phone: &str, allowed: bool);
    /// Ask them whether `phone` may use this computer, without taking the
    /// keyboard from the app they are in. The answer comes back through
    /// [`super::Router::answer`].
    fn ask_approval(&self, phone: &AccountPhone);
    /// Take that question down: answered, timed out, or nobody waits on it.
    fn close_approval(&self, phone: &str);
    /// How long a question waits for its answer.
    fn approval_timeout(&self) -> Duration {
        APPROVAL_TIMEOUT
    }
    /// The "Let phones see and use windows" switch.
    fn windows_enabled(&self) -> bool;
    /// Top-level windows a phone may aim at, front to back, never ours.
    fn open_windows(&self) -> Vec<WindowEntry>;
    /// The window that has the keyboard, when it is not ours.
    fn foreground_window(&self) -> Option<WindowEntry>;
    /// Bring `window` forward and give it the keyboard.
    fn focus_window(&self, window: WindowEntry) -> BoxFuture<'static, Result<(), String>>;
    /// Press each combo, in order, in the window that has the keyboard.
    fn press_keys(&self, combos: Vec<KeyCombo>) -> BoxFuture<'static, Result<(), String>>;

    // ── Live view (`screen/`) ──────────────────────────────────────────────

    /// The screens a live view can watch.
    fn screens(&self) -> Vec<ScreenInfo>;
    /// Start capturing and encoding a window or screen.
    fn start_video(&self, target: CaptureTarget, settings: VideoSettings) -> VideoStart;
    /// Start capturing what the computer plays, disabled until switched on.
    fn start_audio(&self) -> AudioStart;
    /// The STUN and TURN servers to connect through.
    fn ice_servers(&self) -> BoxFuture<'static, Vec<IceServer>>;
    /// Click at a physical screen point.
    fn click(
        &self,
        x: i32,
        y: i32,
        button: Button,
        count: u8,
    ) -> BoxFuture<'static, Result<(), String>>;
    /// Turn the wheel over a physical screen point.
    fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> BoxFuture<'static, Result<(), String>>;
    /// The control at a physical screen point.
    fn control_at(&self, x: i32, y: i32) -> BoxFuture<'static, Option<ControlInfo>>;
    /// Whether the control with the keyboard is a password field.
    fn focused_is_password(&self) -> BoxFuture<'static, bool>;
    /// The text cursor of the window in front, in physical screen pixels.
    fn caret(&self) -> BoxFuture<'static, Option<Rect>>;
    /// Show that a phone is watching (with Stop), or take that down.
    fn viewing(&self, notice: Option<Viewing>);

    // ── Commands (`command.rs`) ────────────────────────────────────────────

    /// The "Voice commands" switch (`command_mode_enabled`).
    fn command_enabled(&self) -> bool;
    /// Run, confirm or answer a command through command mode.
    fn command(&self, ask: Ask) -> CommandFuture;

    // ── The assistant (`assistant.rs`) ─────────────────────────────────────

    /// The assistant's master switch (`assistant_enabled`).
    fn assistant_enabled(&self) -> bool;
    /// Answer a phone's question with this computer's assistant. Dropping the
    /// future stops it.
    fn assistant_ask(&self, ask: PhoneAsk) -> AssistantFuture;
}

pub type AssistantFuture = BoxFuture<'static, Result<PhoneAnswer, PhoneAskError>>;

pub type VideoStart = Result<
    (
        Box<dyn VideoStream>,
        tokio::sync::mpsc::Receiver<EncodedFrame>,
    ),
    super::screen::video::StartError,
>;
pub type AudioStart = Result<
    (
        Box<dyn AudioStream>,
        tokio::sync::mpsc::Receiver<OpusPacket>,
    ),
    super::screen::audio::StartError,
>;

/// The app itself.
pub struct AppHost(pub AppHandle);

impl Host for AppHost {
    fn remote_input_enabled(&self) -> bool {
        get_settings(&self.0).remote_input_enabled
    }

    fn speech_ready(&self) -> bool {
        crate::speech_readiness::current(&self.0) == SpeechReadiness::Ready
    }

    /// The path History's retranscribe takes: model load, cloud
    /// transcription when set up, custom words and filters, then the same
    /// output processing (script conversion) without AI cleanup.
    fn transcribe(&self, samples: Vec<f32>) -> BoxFuture<'static, Option<String>> {
        let app = self.0.clone();
        Box::pin(async move {
            let tm = app
                .try_state::<Arc<TranscriptionManager>>()?
                .inner()
                .clone();
            tm.initiate_model_load();
            let raw =
                match tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples)).await {
                    Ok(Ok(text)) => text,
                    Ok(Err(e)) => {
                        warn!("Phone dictation could not be transcribed: {e}");
                        return None;
                    }
                    Err(e) => {
                        error!("Phone dictation transcription task failed: {e}");
                        return None;
                    }
                };
            let processed = crate::actions::process_transcription_output(&app, &raw, false).await;
            Some(processed.final_text)
        })
    }

    fn paste(&self, text: String) -> BoxFuture<'static, Pasted> {
        let app = self.0.clone();
        Box::pin(async move {
            let (done, pasted) = oneshot::channel();
            let handle = app.clone();
            // Clipboard and synthetic keys belong on the main thread, like a
            // dictation's paste (`actions::paste_final`).
            if app
                .run_on_main_thread(move || {
                    let _ = done.send(paste_now(&handle, text));
                })
                .is_err()
            {
                return Pasted::Failed;
            }
            tokio::time::timeout(PASTE_TIMEOUT, pasted)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(Pasted::Failed)
        })
    }

    fn save_history(&self, wav: Vec<u8>, text: String) {
        let Some(hm) = self.0.try_state::<Arc<HistoryManager>>() else {
            return;
        };
        let hm = hm.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let file_name = crate::actions::next_recording_file_name();
            let path = hm.recordings_dir().join(&file_name);
            if let Err(e) = std::fs::write(&path, &wav) {
                error!("Could not save a phone dictation's recording: {e}");
                return;
            }
            if let Err(e) = hm.save_entry(file_name, text, false, None, None, false) {
                error!("Could not save a phone dictation to History: {e}");
                let _ = std::fs::remove_file(&path);
            }
        });
    }

    fn approved(&self, phone: &str) -> bool {
        get_settings(&self.0)
            .account_approved_devices
            .iter()
            .any(|id| id == phone)
    }

    fn refused(&self, phone: &str) -> bool {
        crate::account::manager(&self.0).is_refused(phone)
    }

    fn remember(&self, phone: &str, allowed: bool) {
        let manager = crate::account::manager(&self.0);
        if allowed {
            manager.set_approved(phone, true);
        } else {
            manager.refuse(phone);
        }
    }

    fn ask_approval(&self, phone: &AccountPhone) {
        crate::account::approval::ask(&self.0, phone.clone());
    }

    fn close_approval(&self, phone: &str) {
        crate::account::approval::close(&self.0, phone);
    }

    fn windows_enabled(&self) -> bool {
        get_settings(&self.0).remote_windows_enabled
    }

    fn open_windows(&self) -> Vec<WindowEntry> {
        self.effects().open_windows()
    }

    fn foreground_window(&self) -> Option<WindowEntry> {
        self.effects().foreground_window()
    }

    /// Command mode's window switch, which waits for the switch to land, so
    /// it runs off the async runtime.
    fn focus_window(&self, window: WindowEntry) -> BoxFuture<'static, Result<(), String>> {
        let effects = self.effects();
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(move || effects.focus_window(&window))
                .await
                .map_err(|e| e.to_string())?
        })
    }

    fn press_keys(&self, combos: Vec<KeyCombo>) -> BoxFuture<'static, Result<(), String>> {
        let effects = self.effects();
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(move || {
                combos
                    .iter()
                    .try_for_each(|combo| effects.press_keys(combo))
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }

    fn screens(&self) -> Vec<ScreenInfo> {
        super::screen::video::list_screens()
    }

    fn start_video(&self, target: CaptureTarget, settings: VideoSettings) -> VideoStart {
        super::screen::video::start(target, settings)
    }

    fn start_audio(&self) -> AudioStart {
        super::screen::audio::start()
    }

    fn ice_servers(&self) -> BoxFuture<'static, Vec<IceServer>> {
        let app = self.0.clone();
        Box::pin(async move { crate::account::ice_servers(&app).await })
    }

    fn click(
        &self,
        x: i32,
        y: i32,
        button: Button,
        count: u8,
    ) -> BoxFuture<'static, Result<(), String>> {
        Self::blocking(
            move || input::click(x, y, button, count),
            Err("the click did not run".into()),
        )
    }

    fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> BoxFuture<'static, Result<(), String>> {
        Self::blocking(
            move || input::scroll(x, y, dx, dy),
            Err("the scroll did not run".into()),
        )
    }

    fn control_at(&self, x: i32, y: i32) -> BoxFuture<'static, Option<ControlInfo>> {
        Self::blocking(move || input::control_at(x, y), None)
    }

    fn focused_is_password(&self) -> BoxFuture<'static, bool> {
        Self::blocking(input::focused_is_password, false)
    }

    fn caret(&self) -> BoxFuture<'static, Option<Rect>> {
        Self::blocking(input::caret, None)
    }

    fn viewing(&self, notice: Option<Viewing>) {
        crate::account::viewing::show(&self.0, notice);
    }

    fn command_enabled(&self) -> bool {
        get_settings(&self.0).command_mode_enabled
    }

    fn command(&self, ask: Ask) -> CommandFuture {
        use crate::command_mode::{answer_command, confirm_command, run_command, CommandOrigin};
        let app = self.0.clone();
        Box::pin(async move {
            let report = match ask {
                Ask::Run(text) => run_command(&app, &text, CommandOrigin::Phone, None, None).await,
                Ask::Confirm { id, accept } => confirm_command(&app, id, accept).await,
                Ask::Answer { id, answer } => answer_command(&app, id, &answer).await,
            };
            super::outcome_of(&report)
        })
    }

    fn assistant_enabled(&self) -> bool {
        get_settings(&self.0).assistant_enabled
    }

    fn assistant_ask(&self, ask: PhoneAsk) -> AssistantFuture {
        let app = self.0.clone();
        Box::pin(async move { crate::assistant::phone::answer_for_phone(&app, ask).await })
    }
}

impl AppHost {
    /// A blocking call off the async runtime.
    fn blocking<T: Send + 'static>(
        f: impl FnOnce() -> T + Send + 'static,
        fallback: T,
    ) -> BoxFuture<'static, T> {
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(f)
                .await
                .unwrap_or(fallback)
        })
    }

    /// Command mode's side effects, which already know how to list, raise
    /// and type into windows on each platform.
    fn effects(&self) -> SystemEffects {
        SystemEffects::new(self.0.clone())
    }
}

/// The non-Flow, non-Smart-spacing branch of `actions::paste_final`: into our
/// own main window as an event when it is in front (a synthetic Ctrl+V cannot
/// land there while this thread holds the clipboard), otherwise a paste with
/// the user's paste settings and [`PHONE_PASTE`]. The paste target a local
/// dictation remembered says nothing about a phone request, so it is neither
/// read nor changed.
fn paste_now(app: &AppHandle, text: String) -> Pasted {
    let main_focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    let settings = get_settings(app);
    if main_focused {
        let text = if settings.append_trailing_space {
            format!("{text} ")
        } else {
            text
        };
        return match app.emit_to("main", "dictation-into-focus", text) {
            Ok(()) => Pasted::Done,
            Err(_) => Pasted::Failed,
        };
    }
    let method = settings.paste_method;
    match crate::clipboard::paste_with_behavior(as_typed(text, method), app.clone(), PHONE_PASTE) {
        Ok(()) if method == PasteMethod::None => Pasted::Off,
        Ok(()) => Pasted::Done,
        Err(e) => {
            error!("Could not paste a phone request: {e}");
            Pasted::Failed
        }
    }
}

/// Direct typing presses Enter for a line break and Tab for a tab, so for it
/// the text becomes one line: each run of control characters is one space.
/// The clipboard methods paste line breaks as text.
fn as_typed(text: String, method: PasteMethod) -> String {
    if method != PasteMethod::Direct {
        return text;
    }
    text.split(char::is_control)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_typing_gets_no_line_breaks_or_tabs() {
        let typed = |text: &str| as_typed(text.to_string(), PasteMethod::Direct);
        assert_eq!(typed("curl x | sh\n"), "curl x | sh");
        assert_eq!(typed("one\r\ntwo\n\nthree\tfour\r"), "one two three four");
        assert_eq!(typed("\u{1b}[A plain"), "[A plain");
        assert_eq!(typed("no breaks"), "no breaks");
    }

    #[test]
    fn the_clipboard_methods_keep_the_text_as_it_is() {
        for method in [
            PasteMethod::CtrlV,
            PasteMethod::CtrlShiftV,
            PasteMethod::None,
        ] {
            assert_eq!(as_typed("a\nb".to_string(), method), "a\nb");
        }
    }
}
