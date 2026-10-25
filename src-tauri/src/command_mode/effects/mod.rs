//! Every side effect a command-mode tool can have, behind one trait.
//!
//! Tools never open a URL, press a key or launch an app themselves: they ask
//! [`Effects`]. That gives three implementations of the same contract:
//!
//! - [`SystemEffects`]: the real thing, one platform module per OS
//!   (`windows.rs`, `macos.rs`, `linux.rs`).
//! - [`DryRunEffects`]: wraps the real one, lets the *reads* through (so a dry
//!   run still finds the app or window it would have used) and records every
//!   *action* as a line of text instead of doing it.
//! - A fake in tests, so every tool's logic is testable without an `AppHandle`.
//!
//! Reads are infallible and return empty lists when the platform cannot answer;
//! actions return `Err` with a sentence fit to show the user.

pub mod http;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod media_sessions;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(all(test, target_os = "windows"))]
pub(crate) use windows::live::LiveEffects;

use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;

use super::registry::Modifier;

/// An installed application the user can launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AppEntry {
    /// What the user would call it: "Spotify", "Visual Studio Code".
    pub name: String,
    /// How to launch it, platform-specific: a `.lnk` path or
    /// `shell:AppsFolder\<AUMID>` on Windows, an `.app` bundle path on macOS,
    /// a `.desktop` file id on Linux.
    pub launch: String,
}

/// A top-level window on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct WindowEntry {
    /// Platform handle: HWND on Windows, pid on macOS (apps are activated, not
    /// windows), X11 window id on Linux. Opaque to tools.
    pub id: i64,
    pub title: String,
    /// The owning app's name ("chrome", "Slack"), lower-level than [`AppEntry::name`].
    pub app: String,
}

/// A key combination, e.g. Ctrl+Shift+T.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct KeyCombo {
    pub modifiers: Vec<Modifier>,
    /// A key name from [`KEY_NAMES`], lowercase ("t", "enter", "f5", "tab").
    pub key: String,
}

impl KeyCombo {
    /// "Ctrl+Shift+T", for feedback and dry-run lines.
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = self
            .modifiers
            .iter()
            .map(|m| match m {
                Modifier::Ctrl => "Ctrl".to_string(),
                Modifier::Alt => "Alt".to_string(),
                Modifier::Shift => "Shift".to_string(),
                Modifier::Meta => {
                    if cfg!(target_os = "macos") {
                        "Cmd".to_string()
                    } else {
                        "Win".to_string()
                    }
                }
            })
            .collect();
        let key = if self.key.chars().count() == 1 {
            self.key.to_uppercase()
        } else {
            let mut chars = self.key.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        };
        parts.push(key);
        parts.join("+")
    }
}

/// Key names a [`KeyCombo`] may use besides a single letter or digit. The
/// platform modules map each of these; `press_keys` validates against it.
pub const KEY_NAMES: &[&str] = &[
    "enter",
    "tab",
    "escape",
    "space",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "minus",
    "equal",
    "comma",
    "period",
    "slash",
    "backslash",
    "semicolon",
    "quote",
    "bracketleft",
    "bracketright",
    "backquote",
];

/// Whether `key` is a name a [`KeyCombo`] accepts.
pub fn is_known_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    (key.chars().count() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()))
        || KEY_NAMES.contains(&key.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    Next,
    Previous,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VolumeAction {
    /// Step up by the OS increment, `n` times.
    Up(u8),
    Down(u8),
    Mute,
    Unmute,
    /// Absolute level, 0–100.
    Set(u8),
}

/// The side effects tools may have. See the module docs.
pub trait Effects: Send + Sync {
    // ── Reads: run in dry run too ──────────────────────────────────────────

    /// Installed applications the user can launch by name.
    fn installed_apps(&self) -> Vec<AppEntry>;
    /// Top-level, visible windows, front to back where the platform knows.
    /// Never includes Talvio's own windows.
    fn open_windows(&self) -> Vec<WindowEntry>;
    /// Recently used files, most recent first, where the platform keeps such
    /// a list.
    fn recent_files(&self) -> Vec<PathBuf>;
    /// Folders worth searching by name: Documents, Downloads, Desktop, home.
    fn known_folders(&self) -> Vec<PathBuf>;
    /// The focused top-level window right now, with its real title where the
    /// platform can read one (on macOS through Accessibility). A send step
    /// checks this immediately before pressing its key, so a window that took
    /// the foreground in between is never typed into.
    fn foreground_window(&self) -> Option<WindowEntry> {
        foreground_window()
    }
    /// Whether the window that has the keyboard right now is exactly window
    /// `id`, not a dialog or popup it owns. [`Effects::foreground_window`]
    /// may report a popup under its owner's id and title (that is what makes
    /// a browser's permission bubble read as the browser), which is right for
    /// knowing where the user is and wrong for deciding where a send key
    /// lands. A send step checks this immediately before pressing.
    fn foreground_is_exactly(&self, id: i64) -> bool {
        self.foreground_window().is_some_and(|front| front.id == id)
    }
    /// Fetch a web page (http or https) as text, as a desktop browser would
    /// ask for it, cut at [`http::MAX_BODY_BYTES`]. A read, so a dry run
    /// fetches too. Unlike the other reads it can fail, because a tool that
    /// fetches has a fallback to choose (`play_media` opens the results page).
    fn fetch_text(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<String, String> {
        http::fetch_text(url, headers, timeout)
    }
    /// Whether any app is playing media right now. `None` when this system
    /// cannot tell (only Windows lists media sessions), and the tools then
    /// fall back to the play/pause key.
    fn media_playing(&self) -> Option<bool> {
        media_sessions::playing()
    }

    // ── Actions: recorded, not run, in dry run ─────────────────────────────

    /// Pause every app that is playing, by its own pause rather than the
    /// toggle key. `Ok(n)`: how many were paused. `Err` where the system
    /// has no media sessions.
    fn pause_media(&self) -> Result<usize, String> {
        media_sessions::pause_all()
    }
    /// Resume the current player by its own play. `Ok(false)`: nothing to
    /// resume. `Err` where the system has no media sessions.
    fn resume_media(&self) -> Result<bool, String> {
        media_sessions::play_current()
    }

    /// Open a URL in the user's default browser.
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// Open a file or folder with its default application.
    fn open_path(&self, path: &Path) -> Result<(), String>;
    fn launch_app(&self, app: &AppEntry) -> Result<(), String>;
    /// Bring a window to the front and give it the keyboard.
    fn focus_window(&self, window: &WindowEntry) -> Result<(), String>;
    /// Put text into the focused field.
    fn type_text(&self, text: &str) -> Result<(), String>;
    fn press_keys(&self, combo: &KeyCombo) -> Result<(), String>;
    fn media(&self, action: MediaAction) -> Result<(), String>;
    fn volume(&self, action: VolumeAction) -> Result<(), String>;
}

/// The focused top-level window right now, when it is not one of ours and the
/// platform can tell. Used for the router's context line.
pub fn foreground_window() -> Option<WindowEntry> {
    #[cfg(target_os = "windows")]
    {
        windows::foreground_window()
    }
    #[cfg(target_os = "macos")]
    {
        macos::foreground_window()
    }
    #[cfg(target_os = "linux")]
    {
        linux::foreground_window()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// The real effects, on this OS. Holds the app handle the opener plugin, the
/// shared `Enigo` and the paste path need. `impl Effects for SystemEffects`
/// lives in the platform module (`windows.rs`, `macos.rs`, `linux.rs`).
pub struct SystemEffects {
    pub app: AppHandle,
}

impl SystemEffects {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

/// The real effects wrapped so that actions are recorded instead of run.
pub struct DryRunEffects<E: Effects> {
    inner: E,
    actions: Mutex<Vec<String>>,
}

impl<E: Effects> DryRunEffects<E> {
    pub fn new(inner: E) -> Self {
        Self {
            inner,
            actions: Mutex::new(Vec::new()),
        }
    }

    /// Every action that would have run, in order.
    pub fn actions(&self) -> Vec<String> {
        self.actions.lock().map(|a| a.clone()).unwrap_or_default()
    }

    fn record(&self, line: String) -> Result<(), String> {
        // The line can hold an email body or typed text: it goes to the
        // report the user is looking at, never to the log.
        log::debug!("command mode dry run: recorded an action");
        if let Ok(mut actions) = self.actions.lock() {
            actions.push(line);
        }
        Ok(())
    }
}

/// Lets a `DryRunEffects` hand out its recorded lines through `dyn Effects`.
pub trait RecordsActions {
    fn recorded(&self) -> Vec<String>;
}

impl<E: Effects> RecordsActions for DryRunEffects<E> {
    fn recorded(&self) -> Vec<String> {
        self.actions()
    }
}

impl<E: Effects> Effects for DryRunEffects<E> {
    fn installed_apps(&self) -> Vec<AppEntry> {
        self.inner.installed_apps()
    }
    fn open_windows(&self) -> Vec<WindowEntry> {
        self.inner.open_windows()
    }
    fn recent_files(&self) -> Vec<PathBuf> {
        self.inner.recent_files()
    }
    fn known_folders(&self) -> Vec<PathBuf> {
        self.inner.known_folders()
    }
    fn foreground_window(&self) -> Option<WindowEntry> {
        self.inner.foreground_window()
    }
    fn foreground_is_exactly(&self, id: i64) -> bool {
        self.inner.foreground_is_exactly(id)
    }
    fn fetch_text(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<String, String> {
        self.inner.fetch_text(url, headers, timeout)
    }
    fn media_playing(&self) -> Option<bool> {
        self.inner.media_playing()
    }

    fn pause_media(&self) -> Result<usize, String> {
        let playing = usize::from(self.inner.media_playing() == Some(true));
        self.record("pause what is playing".to_string())?;
        Ok(playing)
    }
    fn resume_media(&self) -> Result<bool, String> {
        self.record("resume the player".to_string())?;
        Ok(true)
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.record(format!("open URL {url}"))
    }
    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.record(format!("open {}", path.display()))
    }
    fn launch_app(&self, app: &AppEntry) -> Result<(), String> {
        self.record(format!("launch {} ({})", app.name, app.launch))
    }
    fn focus_window(&self, window: &WindowEntry) -> Result<(), String> {
        self.record(format!(
            "focus window \"{}\" ({})",
            window.title, window.app
        ))
    }
    fn type_text(&self, text: &str) -> Result<(), String> {
        self.record(format!(
            "type {} characters: {text:?}",
            text.chars().count()
        ))
    }
    fn press_keys(&self, combo: &KeyCombo) -> Result<(), String> {
        self.record(format!("press {}", combo.label()))
    }
    fn media(&self, action: MediaAction) -> Result<(), String> {
        self.record(format!("media {action:?}"))
    }
    fn volume(&self, action: VolumeAction) -> Result<(), String> {
        self.record(format!("volume {action:?}"))
    }
}

/// A scripted [`Effects`] for tests: fixed reads, recorded actions, and
/// optional failures.
#[cfg(test)]
pub mod fake {
    use super::*;

    #[derive(Default)]
    pub struct FakeEffects {
        pub apps: Vec<AppEntry>,
        pub windows: Vec<WindowEntry>,
        pub recent: Vec<PathBuf>,
        pub folders: Vec<PathBuf>,
        /// What `foreground_window` answers. `None` = no window in front.
        /// A test of a focus-then-act step sets this to the window it expects
        /// to be focused; [`FakeEffects::focus_follows`] makes focusing a
        /// window move it here, the way the OS would.
        pub foreground: Mutex<Option<WindowEntry>>,
        /// When true, `focus_window` makes the focused window the foreground.
        pub focus_follows: bool,
        /// When true, a popup the foreground window owns has the keyboard:
        /// `foreground_window` still reports the owner, `foreground_is_exactly`
        /// answers false.
        pub popup_in_front: bool,
        /// When set, every action fails with this message.
        pub fail_with: Option<String>,
        pub actions: Mutex<Vec<String>>,
        /// Canned pages for `fetch_text`, by URL prefix: the first entry whose
        /// prefix the URL starts with answers. A URL nothing matches fails as
        /// if offline, so a test never reaches the network.
        pub pages: Vec<(String, Result<String, String>)>,
        /// Every URL `fetch_text` was asked for, in order. Fetches are reads,
        /// so they are kept apart from `actions`.
        pub fetched: Mutex<Vec<String>>,
        /// What `media_playing` answers; `None` = a system without media
        /// sessions, where `pause_media` and `resume_media` fail.
        pub playing: Mutex<Option<bool>>,
    }

    impl FakeEffects {
        pub fn actions(&self) -> Vec<String> {
            self.actions.lock().unwrap().clone()
        }

        pub fn fetched(&self) -> Vec<String> {
            self.fetched.lock().unwrap().clone()
        }

        fn act(&self, line: String) -> Result<(), String> {
            if let Some(error) = &self.fail_with {
                return Err(error.clone());
            }
            self.actions.lock().unwrap().push(line);
            Ok(())
        }
    }

    impl Effects for FakeEffects {
        fn installed_apps(&self) -> Vec<AppEntry> {
            self.apps.clone()
        }
        fn open_windows(&self) -> Vec<WindowEntry> {
            self.windows.clone()
        }
        fn recent_files(&self) -> Vec<PathBuf> {
            self.recent.clone()
        }
        fn known_folders(&self) -> Vec<PathBuf> {
            self.folders.clone()
        }
        fn foreground_window(&self) -> Option<WindowEntry> {
            self.foreground.lock().unwrap().clone()
        }
        fn foreground_is_exactly(&self, id: i64) -> bool {
            !self.popup_in_front && self.foreground_window().is_some_and(|front| front.id == id)
        }
        fn fetch_text(
            &self,
            url: &str,
            _headers: &[(&str, &str)],
            _timeout: Duration,
        ) -> Result<String, String> {
            self.fetched.lock().unwrap().push(url.to_string());
            self.pages
                .iter()
                .find(|(prefix, _)| url.starts_with(prefix.as_str()))
                .map(|(_, page)| page.clone())
                .unwrap_or_else(|| Err("offline".to_string()))
        }
        fn media_playing(&self) -> Option<bool> {
            *self.playing.lock().unwrap()
        }
        fn pause_media(&self) -> Result<usize, String> {
            let mut playing = self.playing.lock().unwrap();
            match *playing {
                None => Err("media sessions are not available".into()),
                Some(was) => {
                    self.act("pause_media".into())?;
                    *playing = Some(false);
                    Ok(usize::from(was))
                }
            }
        }
        fn resume_media(&self) -> Result<bool, String> {
            let mut playing = self.playing.lock().unwrap();
            match *playing {
                None => Err("media sessions are not available".into()),
                Some(_) => {
                    self.act("resume_media".into())?;
                    *playing = Some(true);
                    Ok(true)
                }
            }
        }
        fn open_url(&self, url: &str) -> Result<(), String> {
            self.act(format!("open_url {url}"))
        }
        fn open_path(&self, path: &Path) -> Result<(), String> {
            self.act(format!("open_path {}", path.display()))
        }
        fn launch_app(&self, app: &AppEntry) -> Result<(), String> {
            self.act(format!("launch_app {}", app.launch))
        }
        fn focus_window(&self, window: &WindowEntry) -> Result<(), String> {
            self.act(format!("focus_window {}", window.id))?;
            if self.focus_follows {
                *self.foreground.lock().unwrap() = Some(window.clone());
            }
            Ok(())
        }
        fn type_text(&self, text: &str) -> Result<(), String> {
            self.act(format!("type_text {text}"))
        }
        fn press_keys(&self, combo: &KeyCombo) -> Result<(), String> {
            self.act(format!("press_keys {}", combo.label()))
        }
        fn media(&self, action: MediaAction) -> Result<(), String> {
            self.act(format!("media {action:?}"))
        }
        fn volume(&self, action: VolumeAction) -> Result<(), String> {
            self.act(format!("volume {action:?}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeEffects;
    use super::*;

    #[test]
    fn a_dry_run_lets_reads_through_and_records_actions_instead_of_running_them() {
        let fake = FakeEffects {
            apps: vec![AppEntry {
                name: "Spotify".into(),
                launch: "spotify.lnk".into(),
            }],
            fail_with: Some("would have run".into()),
            pages: vec![("https://example.com/".into(), Ok("page".into()))],
            ..Default::default()
        };
        let dry = DryRunEffects::new(fake);
        assert_eq!(dry.installed_apps().len(), 1);
        // A fetch is a read: it reaches the inner effects and is not recorded.
        assert_eq!(
            dry.fetch_text("https://example.com/a", &[], Duration::from_secs(1)),
            Ok("page".to_string())
        );
        // The inner fake fails every action, so success proves nothing ran.
        assert!(dry.open_url("https://example.com").is_ok());
        assert!(dry
            .press_keys(&KeyCombo {
                modifiers: vec![Modifier::Ctrl],
                key: "enter".into()
            })
            .is_ok());
        assert_eq!(
            dry.actions(),
            vec![
                "open URL https://example.com".to_string(),
                "press Ctrl+Enter".to_string()
            ]
        );
    }

    #[test]
    fn key_names_accept_letters_digits_and_named_keys_only() {
        assert!(is_known_key("t"));
        assert!(is_known_key("7"));
        assert!(is_known_key("Enter"));
        assert!(is_known_key("f5"));
        assert!(!is_known_key("ctrl"));
        assert!(!is_known_key("é"));
        assert!(!is_known_key("hyper"));
    }

    #[test]
    fn a_combo_reads_the_way_a_person_writes_it() {
        let combo = KeyCombo {
            modifiers: vec![Modifier::Ctrl, Modifier::Shift],
            key: "t".into(),
        };
        assert_eq!(combo.label(), "Ctrl+Shift+T");
        let combo = KeyCombo {
            modifiers: vec![],
            key: "pagedown".into(),
        };
        assert_eq!(combo.label(), "Pagedown");
    }
}
