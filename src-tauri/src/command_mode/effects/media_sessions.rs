//! The system's media sessions: what is playing, and pausing or resuming it
//! by name rather than with the play/pause key.
//!
//! The media key only toggles, so "pause" pressed while nothing plays starts
//! something, and a song started from YouTube while Spotify plays leaves both
//! playing. Windows lists every app's player (Spotify, a browser tab, a
//! podcast app) as a session of the Global System Media Transport Controls,
//! with its status and its own pause and play. macOS and Linux answer `None`
//! here, and the tools fall back to the key.
//!
//! Each call runs on its own short-lived thread in the multithreaded COM
//! apartment and is waited for at most [`WAIT`], so a hung session manager
//! costs a command a second, never the command.

use std::time::Duration;

/// How long a read or a pause may take before it is given up on.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WAIT: Duration = Duration::from_millis(1500);

/// What to do to the sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
enum Act {
    /// Only look.
    Read,
    /// Pause every playing session.
    PauseAll,
    /// Play the current session (the one Windows would resume with the key).
    PlayCurrent,
}

/// Whether any media session is playing. `None` when this system has no
/// session list or it could not be read.
pub fn playing() -> Option<bool> {
    run(Act::Read).map(|state| state.playing > 0)
}

/// Pause every playing session. `Ok(n)`: how many were paused (0 when nothing
/// played). `Err` when there is no session list to work with.
pub fn pause_all() -> Result<usize, String> {
    run(Act::PauseAll)
        .map(|state| state.acted)
        .ok_or_else(|| "media sessions are not available".to_string())
}

/// Resume the current session. `Ok(true)` when it was asked to play.
pub fn play_current() -> Result<bool, String> {
    run(Act::PlayCurrent)
        .map(|state| state.acted > 0)
        .ok_or_else(|| "media sessions are not available".to_string())
}

#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
struct State {
    /// Sessions playing when they were read.
    playing: usize,
    /// Sessions paused or played.
    acted: usize,
}

#[cfg(target_os = "windows")]
fn run(act: Act) -> Option<State> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("media-sessions".into())
        .spawn(move || {
            let _ = sender.send(windows_impl::run(act));
        })
        .ok()?;
    match receiver.recv_timeout(WAIT) {
        Ok(Ok(state)) => Some(state),
        Ok(Err(e)) => {
            log::debug!("media sessions: {e}");
            None
        }
        Err(_) => {
            log::warn!(
                "media sessions did not answer within {} ms",
                WAIT.as_millis()
            );
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn run(_act: Act) -> Option<State> {
    None
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{Act, State};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    pub(super) fn run(act: Act) -> windows::core::Result<State> {
        // SAFETY: this thread is ours and lives only for this call; every
        // successful initialisation is paired with the uninitialise below.
        let initialised = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let result = sessions(act);
        if initialised {
            // SAFETY: paired with the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        result
    }

    fn sessions(act: Act) -> windows::core::Result<State> {
        let manager = Manager::RequestAsync()?.get()?;
        let mut state = State::default();
        if act == Act::PlayCurrent {
            if let Ok(current) = manager.GetCurrentSession() {
                if current.TryPlayAsync()?.get()? {
                    state.acted = 1;
                }
            }
            return Ok(state);
        }
        let sessions = manager.GetSessions()?;
        for index in 0..sessions.Size()? {
            let session = sessions.GetAt(index)?;
            let playing = session
                .GetPlaybackInfo()
                .and_then(|info| info.PlaybackStatus())
                .is_ok_and(|status| status == Status::Playing);
            if !playing {
                continue;
            }
            state.playing += 1;
            if act == Act::PauseAll && session.TryPauseAsync()?.get()? {
                state.acted += 1;
            }
        }
        Ok(state)
    }

    /// Run by hand to see this machine's sessions:
    /// `cargo test --lib media_sessions -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads this machine's media sessions"]
    fn live_media_sessions_are_read() {
        println!("playing: {:?}", super::playing());
    }
}
