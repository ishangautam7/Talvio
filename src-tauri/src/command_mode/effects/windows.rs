//! Command mode's effects on Windows.
//!
//! Most of this is plain Win32, and three things are less obvious than they
//! look:
//!
//! - **Bringing a window forward needs the foreground lock lifted.** Windows
//!   only lets the process that owns the foreground hand it on, and after a
//!   voice command that is the user's app, not us, so `SetForegroundWindow` on
//!   its own only flashes the taskbar button. Joining the foreground thread's
//!   input queue for the moment of the switch (`AttachThreadInput`) is the
//!   accepted way round it, and the result is checked rather than trusted.
//! - **Installed apps come from two places.** Desktop programs leave a `.lnk`
//!   in a Start Menu folder; Store apps (Calculator, Settings) leave nothing on
//!   disk and are only reachable by their AppUserModelID, which `Get-StartApps`
//!   lists. One PowerShell run is slow (about a second), so the list is cached
//!   for [`APPS_FRESH_FOR`].
//! - **Keys are sent by virtual-key code, not by character**, wherever a key has
//!   one: Ctrl+T must be Ctrl+T on a Russian or AZERTY layout too, the way the
//!   paste's Ctrl+V already is (`input::send_paste_ctrl_v`).
//! - **A window is named by its root owner.** [`foreground_window`] and
//!   `open_windows` both answer with the HWND of the unowned top-level window,
//!   so a Save As dialog or a browser prompt in front reports the window it
//!   belongs to and the two lists compare by id. The other platforms keep the
//!   same rule with their own ids (a pid on macOS, an X11 id on Linux).
//! - **Nothing here waits on another process's window.** A hung app answers
//!   no messages, so every call that would send it one is either skipped
//!   (`IsHungAppWindow`) or posted (`ShowWindowAsync`, `SWP_ASYNCWINDOWPOS`).

use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use enigo::{Direction, Enigo, Key, Keyboard};
use log::{debug, info, warn};
use serde::Deserialize;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetForegroundWindow, GetLastActivePopup, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsGUIThread,
    IsHungAppWindow, IsIconic, IsWindow, IsWindowVisible, SetForegroundWindow, SetWindowPos,
    ShowWindowAsync, GA_ROOTOWNER, GWL_EXSTYLE, GW_OWNER, HWND_TOP, SWP_ASYNCWINDOWPOS,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, WS_EX_TOOLWINDOW,
};

use super::{
    is_known_key, AppEntry, Effects, KeyCombo, MediaAction, SystemEffects, VolumeAction,
    WindowEntry,
};
use crate::command_mode::registry::Modifier;

/// Console programs (PowerShell) open no terminal window with this.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// How long the installed-app list is reused. Long enough that a burst of
/// commands pays for PowerShell once, short enough that an app installed a
/// few minutes ago is found.
const APPS_FRESH_FOR: Duration = Duration::from_secs(10 * 60);
/// How long a list read while `Get-StartApps` failed is reused. Short, so a
/// cold PowerShell start under a virus scan costs Calculator and Settings
/// for one command rather than ten minutes; long enough that a machine where
/// it always fails does not make every command wait for it.
const STORE_APPS_RETRY_AFTER: Duration = Duration::from_secs(30);
/// The longest `Get-StartApps` may run before its answer is given up on. It
/// usually takes about a second; a machine where it hangs still gets the
/// Start Menu shortcuts.
const START_APPS_DEADLINE: Duration = Duration::from_secs(8);
/// How deep the Start Menu walk goes, and how many entries it looks at, so a
/// folder link loop or a huge tree cannot stall a command.
const MAX_START_MENU_DEPTH: usize = 6;
const MAX_START_MENU_ENTRIES: usize = 5_000;
/// The most recent files offered, newest first.
const MAX_RECENT_FILES: usize = 200;
/// The most windows listed. Nobody has more real windows open than this.
const MAX_WINDOWS: usize = 100;
/// How long `type_text` waits for the main thread to take up the paste (and,
/// usually, finish it). A paste that has not started by then never will.
const PASTE_WAIT: Duration = Duration::from_secs(10);
/// How much longer a paste that has already started is waited for. It is
/// bounded by the paste transaction's own restore timeout; the margin covers
/// the keystroke and the clipboard work around it.
const PASTE_FINISH_WAIT: Duration =
    Duration::from_secs(crate::paste_tx::RESTORE_TIMEOUT.as_secs() + 2);
/// How long a focus change has to show up before it counts as refused.
const FOCUS_SETTLE: Duration = Duration::from_millis(300);
/// The same for a minimized window: its restore is posted, not waited for,
/// so it lands a little later.
const FOCUS_SETTLE_RESTORED: Duration = Duration::from_millis(800);
/// The most volume-key steps one command sends (each is 2% on Windows).
const MAX_VOLUME_STEPS: u8 = 50;
/// Start Menu shortcuts whose name contains one of these are not apps.
const NOT_APPS: &[&str] = &["uninstall", "readme", "help", "website"];

// ── Foreground and windows ─────────────────────────────────────────────────

/// The focused top-level window, named by its root owner (see the module
/// doc), unless it is ours, hung, cloaked, or there is none.
///
/// The id is the one `open_windows` lists for the same window, so a tool can
/// compare the two. A dialog in front therefore reads as its owner: a caller
/// about to send keys to a window should know that the keys may land on a
/// dialog of it.
pub fn foreground_window() -> Option<WindowEntry> {
    // SAFETY: no arguments; a null handle is checked below.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let own = own_process();
    // A dialog of ours in front is ours, whoever owns it.
    if window_process(hwnd) == own {
        return None;
    }
    // SAFETY: read-only queries on a handle from the system; a stale one
    // answers null or false, which the helper treats as "no root".
    let (root, root_visible) = unsafe {
        let root = GetAncestor(hwnd, GA_ROOTOWNER);
        let visible = !root.is_invalid() && IsWindowVisible(root).as_bool();
        (root, visible)
    };
    let shown = HWND(reported_window(handle_id(hwnd), handle_id(root), root_visible) as *mut _);
    if is_cloaked(shown) {
        return None;
    }
    describe(shown, own)
}

fn own_process() -> u32 {
    // SAFETY: no arguments, cannot fail.
    unsafe { GetCurrentProcessId() }
}

/// The id a window is listed under: its HWND as an integer, the same in
/// `open_windows`, `foreground_window` and `focus_window`.
fn handle_id(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// The process that owns `hwnd`, or 0 when it is gone.
fn window_process(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: a stale handle only leaves `pid` at 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Which window `foreground_window` reports: the foreground's root owner
/// when there is a visible one, since that is the window `open_windows` lists
/// and Alt+Tab shows. An owner that is hidden (some frameworks own every
/// window by an invisible one) is no window anyone sees, so the foreground
/// window itself is reported then.
fn reported_window(foreground: isize, root: isize, root_visible: bool) -> isize {
    if root != 0 && root != foreground && root_visible {
        root
    } else {
        foreground
    }
}

/// Did a switch to `target` land? Either `target` itself is in front, or a
/// window it owns is (a modal dialog takes the activation its disabled owner
/// is given, exactly as with Alt+Tab).
fn landed_on(target: isize, foreground: isize, foreground_root: isize) -> bool {
    target != 0 && (foreground == target || foreground_root == target)
}

/// The window to activate for `owner`: its last active popup when the owner
/// is disabled behind a modal dialog (activating a disabled window leaves
/// nothing able to take input), else the owner itself. A modeless popup of an
/// enabled owner is not preferred, so keys meant for the main window do not
/// go to a find bar or a tool palette.
fn activation_target(owner: isize, owner_enabled: bool, popup: isize, popup_usable: bool) -> isize {
    if !owner_enabled && popup != 0 && popup != owner && popup_usable {
        popup
    } else {
        owner
    }
}

/// Is `hwnd` cloaked by DWM? A cloaked window is "visible" to `IsWindowVisible`
/// yet shown nowhere: a Store app prelaunched or suspended in the background
/// (Settings, Calculator) keeps such a frame, as do windows on another virtual
/// desktop. When DWM cannot say, the window counts as not cloaked.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    // SAFETY: the out-pointer is a live u32 and its size is passed with it; a
    // stale handle makes the call fail, which is checked.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    read.is_ok() && cloaked != 0
}

/// Title and app of `hwnd`, or `None` when it belongs to `own_process` or is
/// not answering (a hung window's text is not worth a stall).
fn describe(hwnd: HWND, own_process: u32) -> Option<WindowEntry> {
    let pid = window_process(hwnd);
    if pid == 0 || pid == own_process {
        return None;
    }
    // SAFETY: `hwnd` came from the system a moment ago; a stale handle only
    // makes this answer false.
    if unsafe { IsHungAppWindow(hwnd) }.as_bool() {
        return None;
    }
    let title = window_title(hwnd);
    let exe = process_exe_name(pid).map(|name| strip_exe(&name).to_string());
    Some(WindowEntry {
        id: handle_id(hwnd) as i64,
        app: app_label(exe.as_deref().unwrap_or_default(), &title),
        title,
    })
}

fn window_title(hwnd: HWND) -> String {
    // SAFETY: the buffer is sized from the length the window reports, plus
    // the terminator; GetWindowTextW never writes past the slice it is given.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

/// The executable's file name ("chrome.exe") for a process id.
///
/// Copied from `smart_spacing/windows.rs`, where it is private.
fn process_exe_name(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed on every path; the buffer's length is
    // passed in and the returned size is within it.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}

/// "chrome.exe" → "chrome". Case-insensitive, and only a trailing `.exe`.
fn strip_exe(name: &str) -> &str {
    let len = name.len();
    if len > 4 && name.is_char_boundary(len - 4) && name[len - 4..].eq_ignore_ascii_case(".exe") {
        &name[..len - 4]
    } else {
        name
    }
}

/// The app a window belongs to, as the router should see it. A Store app's
/// window is hosted by `ApplicationFrameHost`, a name that says nothing about
/// it, so its title (which is the app's name, "Calculator") stands in.
fn app_label(exe: &str, title: &str) -> String {
    if exe.eq_ignore_ascii_case("ApplicationFrameHost") && !title.trim().is_empty() {
        title.trim().to_string()
    } else {
        exe.to_string()
    }
}

/// `EnumWindows` callback: collect every top-level handle, in z-order (front
/// to back). Filtering happens afterwards, outside the callback.
unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut Vec<isize>` that `top_level_windows`
    // passed, alive for the whole `EnumWindows` call that invokes this.
    let handles = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    handles.push(hwnd.0 as isize);
    BOOL(1)
}

fn top_level_windows() -> Vec<HWND> {
    let mut handles: Vec<isize> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call.
    let listed = unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM(&mut handles as *mut Vec<isize> as isize),
        )
    };
    if let Err(err) = listed {
        debug!("Listing windows stopped early: {err}");
    }
    handles
        .into_iter()
        .map(|handle| HWND(handle as *mut std::ffi::c_void))
        .collect()
}

/// Would this window show up in Alt+Tab? Visible, not a tool window, not owned
/// by another window, and not cloaked: a suspended Store app's frame is
/// "visible" but shown nowhere, and listing it made "switch to Settings" try
/// to focus a ghost instead of launching the app.
fn is_switchable(hwnd: HWND) -> bool {
    // SAFETY: read-only queries on a handle from `EnumWindows`; a stale one
    // answers false or an error.
    let plain = unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return false;
        }
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        ex_style & WS_EX_TOOLWINDOW.0 == 0
    };
    plain && !is_cloaked(hwnd)
}

/// Windows Explorer's desktop is a visible, unowned window called "Program
/// Manager"; it is never what someone means by a window.
fn is_desktop(entry: &WindowEntry) -> bool {
    entry.app.eq_ignore_ascii_case("explorer") && entry.title == "Program Manager"
}

// ── Installed apps ─────────────────────────────────────────────────────────

/// The app list as last read, and for how long it may be reused.
struct CachedApps {
    read_at: Instant,
    fresh_for: Duration,
    apps: Vec<AppEntry>,
}

/// The cached app list.
fn app_cache() -> &'static Mutex<Option<CachedApps>> {
    static CACHE: OnceLock<Mutex<Option<CachedApps>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// How long a freshly read list is reused: the full time when the Store apps
/// came back, a short retry interval when they did not, so one failed
/// PowerShell run does not hide Calculator and Settings for ten minutes.
fn apps_lifetime(store_ok: bool) -> Duration {
    if store_ok {
        APPS_FRESH_FOR
    } else {
        STORE_APPS_RETRY_AFTER
    }
}

fn installed_apps_cached() -> Vec<AppEntry> {
    // The lock is held while reading, so two commands at once run PowerShell
    // once rather than twice.
    let Ok(mut cache) = app_cache().lock() else {
        return read_installed_apps().0;
    };
    if let Some(cached) = cache.as_ref() {
        if cached.read_at.elapsed() < cached.fresh_for {
            return cached.apps.clone();
        }
    }
    let (apps, store_ok) = read_installed_apps();
    *cache = Some(CachedApps {
        read_at: Instant::now(),
        fresh_for: apps_lifetime(store_ok),
        apps: apps.clone(),
    });
    apps
}

/// Every installed app, and whether the Store apps were read. Without them
/// the Start Menu shortcuts are still returned.
fn read_installed_apps() -> (Vec<AppEntry>, bool) {
    static WARNED: AtomicBool = AtomicBool::new(false);

    let started = Instant::now();
    let mut shortcuts = Vec::new();
    for root in start_menu_roots() {
        let mut budget = MAX_START_MENU_ENTRIES;
        collect_shortcuts(&root, 0, &mut budget, &mut shortcuts);
    }
    let store = start_apps();
    let store_ok = store.is_some();
    if !store_ok {
        // Once per session at warn, so the missing Store apps are explained
        // in the log without a line on every retry.
        if WARNED.swap(true, Ordering::Relaxed) {
            debug!("Store apps could not be listed again; retrying later");
        } else {
            warn!(
                "Store apps could not be listed; only Start Menu shortcuts are offered \
                 until a retry succeeds"
            );
        }
    }
    let apps = merge_apps(shortcuts, store.unwrap_or_default());
    info!(
        "Command mode found {} installed apps in {} ms",
        apps.len(),
        started.elapsed().as_millis()
    );
    (apps, store_ok)
}

/// The machine-wide and the per-user Start Menu program folders.
fn start_menu_roots() -> Vec<PathBuf> {
    ["ProgramData", "APPDATA"]
        .iter()
        .filter_map(|var| std::env::var_os(var))
        .map(|base| {
            PathBuf::from(base)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        })
        .filter(|path| path.is_dir())
        .collect()
}

fn collect_shortcuts(dir: &Path, depth: usize, budget: &mut usize, out: &mut Vec<AppEntry>) {
    if depth > MAX_START_MENU_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // A symlinked folder is not followed: it is how a walk loops.
        if kind.is_dir() {
            collect_shortcuts(&path, depth + 1, budget, out);
        } else if let Some(name) = shortcut_app_name(&path) {
            out.push(AppEntry {
                name,
                launch: path.to_string_lossy().into_owned(),
            });
        }
    }
}

/// The app name a Start Menu shortcut stands for, or `None` when the file is
/// not a shortcut or is one of the helpers installers drop beside the app
/// ("Uninstall Foo", "Foo Readme", "Foo Website").
fn shortcut_app_name(path: &Path) -> Option<String> {
    let is_lnk = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
    if !is_lnk {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().trim().to_string();
    if name.is_empty() || is_helper_shortcut(&name) {
        return None;
    }
    Some(name)
}

fn is_helper_shortcut(name: &str) -> bool {
    let lower = name.to_lowercase();
    NOT_APPS.iter().any(|word| lower.contains(word))
}

/// Shortcuts first (they launch exactly what the Start Menu would), then the
/// Store apps whose names no shortcut already has. One entry per name, compared
/// without case.
fn merge_apps(shortcuts: Vec<AppEntry>, store: Vec<AppEntry>) -> Vec<AppEntry> {
    let mut seen = HashSet::new();
    let mut apps = Vec::with_capacity(shortcuts.len() + store.len());
    for app in shortcuts.into_iter().chain(store) {
        if seen.insert(app.name.to_lowercase()) {
            apps.push(app);
        }
    }
    apps
}

/// One row of `Get-StartApps | ConvertTo-Json`.
#[derive(Debug, Deserialize)]
struct StartApp {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "AppID")]
    app_id: Option<String>,
}

/// `ConvertTo-Json` writes a lone object, not a one-element array, when there
/// is a single result, so both shapes are read.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StartApps {
    Many(Vec<StartApp>),
    One(StartApp),
}

/// Every app `Get-StartApps` lists, as `shell:AppsFolder\<AppID>` launches.
/// `None` when PowerShell is missing, slow or answers with something else,
/// and also when the answer holds no app at all: `Get-StartApps` lists the
/// desktop programs too, so an empty list on a working machine does not
/// happen and is retried like a failure rather than cached.
fn start_apps() -> Option<Vec<AppEntry>> {
    // Output is forced to UTF-8: by default PowerShell writes the console's
    // code page, which garbles every name outside it.
    let script = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
                  Get-StartApps | ConvertTo-Json -Compress";
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let stdout = run_with_deadline(&mut command, START_APPS_DEADLINE)?;
    store_apps_read(parse_start_apps(&String::from_utf8_lossy(&stdout)))
}

/// A parsed `Get-StartApps` answer as a read that worked, or `None` (see
/// [`start_apps`] for why an empty one counts as failed).
fn store_apps_read(apps: Vec<AppEntry>) -> Option<Vec<AppEntry>> {
    (!apps.is_empty()).then_some(apps)
}

/// Run `command` and return its stdout, or `None` if it could not start, ran
/// past `limit` (it is then killed), or failed.
fn run_with_deadline(command: &mut Command, limit: Duration) -> Option<Vec<u8>> {
    use std::io::Read;

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            debug!("Could not start PowerShell to list Store apps: {err}");
            return None;
        }
    };
    let mut pipe = child.stdout.take()?;
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = pipe.read_to_end(&mut out);
        let _ = sent.send(out);
    });
    // The pipe closes when the process exits, so the read finishing is the
    // process finishing.
    match received.recv_timeout(limit) {
        Ok(out) => {
            let status = child.wait().ok()?;
            if !status.success() {
                debug!("Get-StartApps exited with {status}");
                return None;
            }
            Some(out)
        }
        Err(_) => {
            warn!("Get-StartApps did not answer in time; Store apps are left out");
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

fn parse_start_apps(json: &str) -> Vec<AppEntry> {
    let json = json.trim().trim_start_matches('\u{feff}');
    if json.is_empty() {
        return Vec::new();
    }
    let rows = match serde_json::from_str::<StartApps>(json) {
        Ok(StartApps::Many(rows)) => rows,
        Ok(StartApps::One(row)) => vec![row],
        Err(err) => {
            debug!("Could not read the Get-StartApps answer: {err}");
            return Vec::new();
        }
    };
    rows.into_iter()
        .filter_map(|row| {
            let name = row.name?.trim().to_string();
            let app_id = row.app_id?.trim().to_string();
            if name.is_empty() || app_id.is_empty() {
                return None;
            }
            Some(AppEntry {
                name,
                launch: format!("shell:AppsFolder\\{app_id}"),
            })
        })
        .collect()
}

/// The AppID inside a `shell:AppsFolder\…` launch string, if it is one.
fn apps_folder_id(launch: &str) -> Option<&str> {
    const PREFIX: &str = "shell:AppsFolder\\";
    let head = launch.get(..PREFIX.len())?;
    if !head.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let id = &launch[PREFIX.len()..];
    (!id.trim().is_empty()).then_some(id)
}

// ── Typing ─────────────────────────────────────────────────────────────────

/// Where a command's paste stands, shared by the waiter and the main thread
/// (see `type_text`). Exactly one of them moves it off `PASTE_PENDING`.
const PASTE_PENDING: u8 = 0;
const PASTE_STARTED: u8 = 1;
const PASTE_ABANDONED: u8 = 2;

/// The main thread's side: take the paste up, unless the waiter already
/// called it off. True means paste now.
fn claim_paste(state: &AtomicU8) -> bool {
    state
        .compare_exchange(
            PASTE_PENDING,
            PASTE_STARTED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
}

/// The waiter's side: call the paste off. True means it had not started and
/// now never will; false means it is already under way.
fn abandon_paste(state: &AtomicU8) -> bool {
    state
        .compare_exchange(
            PASTE_PENDING,
            PASTE_ABANDONED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
}

/// What the main thread did with a command's text.
enum Typed {
    Done,
    /// Refused: our own window is in front and nothing else is meant.
    OwnWindow,
    Failed(String),
}

/// Is a window of ours in front right now?
fn own_window_in_front() -> bool {
    // SAFETY: no arguments; a null handle is checked.
    let hwnd = unsafe { GetForegroundWindow() };
    !hwnd.is_invalid() && window_process(hwnd) == own_process()
}

/// Should the paste be refused? Only when our own window is in front and no
/// remembered target will take the foreground back from it first: then the
/// text would go into a webview that cannot take it while the paste holds
/// the main thread.
fn refuses_own_window(own_window_in_front: bool, target_elsewhere: bool) -> bool {
    own_window_in_front && !target_elsewhere
}

// ── Keys ───────────────────────────────────────────────────────────────────

/// The enigo key for a [`KeyCombo`] key name: a letter or digit, or one of
/// [`super::KEY_NAMES`].
///
/// Letters and digits are sent as their virtual-key codes (`VK_A` is 0x41,
/// `VK_0` is 0x30), which name a key position whatever the layout; a
/// `Key::Unicode` would be looked up in the current layout and, on one without
/// Latin letters, typed as text with no Ctrl on it. Punctuation uses the OEM
/// keys a US layout has those marks on, which is what a shortcut like Ctrl+/
/// refers to in every app's documentation.
fn key_for(name: &str) -> Option<Key> {
    let name = name.to_ascii_lowercase();
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Some(Key::Other(u32::from(c.to_ascii_uppercase())));
        }
        if c.is_ascii_digit() {
            return Some(Key::Other(u32::from(c)));
        }
    }
    let key = match name.as_str() {
        "enter" => Key::Return,
        "tab" => Key::Tab,
        "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "minus" => Key::OEMMinus,
        "equal" => Key::OEMPlus,
        "comma" => Key::OEMComma,
        "period" => Key::OEMPeriod,
        "slash" => Key::OEM2,
        "backslash" => Key::OEM5,
        "semicolon" => Key::OEM1,
        "quote" => Key::OEM7,
        "bracketleft" => Key::OEM4,
        "bracketright" => Key::OEM6,
        "backquote" => Key::OEM3,
        _ => return None,
    };
    Some(key)
}

fn modifier_key(modifier: Modifier) -> Key {
    match modifier {
        Modifier::Ctrl => Key::Control,
        Modifier::Alt => Key::Alt,
        Modifier::Shift => Key::Shift,
        Modifier::Meta => Key::Meta,
    }
}

fn media_key(action: MediaAction) -> Key {
    match action {
        MediaAction::PlayPause => Key::MediaPlayPause,
        MediaAction::Next => Key::MediaNextTrack,
        MediaAction::Previous => Key::MediaPrevTrack,
        MediaAction::Stop => Key::MediaStop,
    }
}

/// Press `modifiers` in order, click `key`, and release every modifier that
/// went down, in reverse, even when a press or the click failed: a modifier
/// left down reads as a key stuck pressed in every app.
fn press_combo(enigo: &mut Enigo, modifiers: &[Key], key: Key) -> Result<(), String> {
    let mut pressed: Vec<Key> = Vec::with_capacity(modifiers.len());
    let mut result = Ok(());
    for &modifier in modifiers {
        match enigo.key(modifier, Direction::Press) {
            Ok(()) => pressed.push(modifier),
            Err(err) => {
                result = Err(format!("pressing {modifier:?}: {err}"));
                break;
            }
        }
    }
    if result.is_ok() {
        result = enigo
            .key(key, Direction::Click)
            .map_err(|err| format!("clicking {key:?}: {err}"));
    }
    for &modifier in pressed.iter().rev() {
        if let Err(err) = enigo.key(modifier, Direction::Release) {
            if result.is_ok() {
                result = Err(format!("releasing {modifier:?}: {err}"));
            }
        }
    }
    result
}

impl SystemEffects {
    /// Run `work` with the shared `Enigo`, the injected-input guard up (so the
    /// keys sent are never read back as one of our own shortcuts), and, when
    /// `releases_modifiers`, every modifier released afterwards.
    ///
    /// Never call the paste path from inside `work`: it locks the same `Enigo`.
    fn with_keyboard(
        &self,
        releases_modifiers: bool,
        work: impl FnOnce(&mut Enigo) -> Result<(), String>,
    ) -> Result<(), String> {
        let state = self
            .app
            .try_state::<crate::input::EnigoState>()
            .ok_or_else(|| "Keyboard control isn't ready yet.".to_string())?;
        let mut enigo = state.0.lock().map_err(|_| {
            warn!("The shared keyboard controller is poisoned");
            "Keyboard control isn't working right now.".to_string()
        })?;
        let _injected = handy_keys::ignore_injected_input();
        let result = work(&mut enigo);
        if releases_modifiers {
            crate::input::release_all_modifiers(&mut enigo);
        }
        result
    }

    fn click_times(&self, key: Key, times: u8) -> Result<(), String> {
        let times = times.clamp(1, MAX_VOLUME_STEPS);
        self.with_keyboard(false, |enigo| {
            for _ in 0..times {
                enigo.key(key, Direction::Click).map_err(|err| {
                    warn!("Sending {key:?} failed: {err}");
                    "I couldn't change the volume.".to_string()
                })?;
            }
            Ok(())
        })
    }
}

impl Effects for SystemEffects {
    fn foreground_is_exactly(&self, id: i64) -> bool {
        // The raw foreground handle, not the root owner `foreground_window`
        // reports: a "Leave site?" dialog in front of the draft must fail this.
        // SAFETY: no arguments; an invalid handle compares unequal.
        let hwnd = unsafe { GetForegroundWindow() };
        !hwnd.is_invalid() && handle_id(hwnd) as i64 == id
    }
    fn installed_apps(&self) -> Vec<AppEntry> {
        installed_apps_cached()
    }

    fn open_windows(&self) -> Vec<WindowEntry> {
        let own = own_process();
        top_level_windows()
            .into_iter()
            .filter(|&hwnd| is_switchable(hwnd))
            .filter_map(|hwnd| describe(hwnd, own))
            .filter(|entry| !entry.title.trim().is_empty() && !is_desktop(entry))
            .take(MAX_WINDOWS)
            .collect()
    }

    /// Windows keeps recent items as shortcuts in `%APPDATA%\Microsoft\Windows\Recent`,
    /// and these are returned as they are: opening one opens its target, and
    /// the shortcut's file stem is the target's full name ("report.docx"), so a
    /// tool that scores file stems still matches on it. Folders the user
    /// opened are in there too.
    fn recent_files(&self) -> Vec<PathBuf> {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return Vec::new();
        };
        let dir = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Recent");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut dated: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let is_lnk = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
                if !is_lnk {
                    return None;
                }
                let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
                Some((modified, path))
            })
            .collect();
        dated.sort_by(|a, b| b.0.cmp(&a.0));
        dated
            .into_iter()
            .take(MAX_RECENT_FILES)
            .map(|(_, path)| path)
            .collect()
    }

    fn known_folders(&self) -> Vec<PathBuf> {
        let paths = self.app.path();
        let mut folders: Vec<PathBuf> = Vec::new();
        for folder in [
            paths.document_dir(),
            paths.download_dir(),
            paths.desktop_dir(),
            paths.home_dir(),
        ]
        .into_iter()
        .flatten()
        {
            if folder.is_dir() && !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|err| {
                warn!("Opening a link in the browser failed: {err}");
                "I couldn't open that link.".to_string()
            })
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|err| {
                warn!("Opening a file failed: {err}");
                "I couldn't open that.".to_string()
            })
    }

    /// A shortcut or program path goes through the shell, like a double-click.
    /// A Store app goes through Explorer's `shell:AppsFolder`, the only way to
    /// start one by its AppID without COM activation. Neither is put in a
    /// kill-on-close job: an app the user asked for must outlive Talvio.
    fn launch_app(&self, app: &AppEntry) -> Result<(), String> {
        let failed = |err: String| {
            warn!("Launching {} failed: {err}", app.name);
            format!("I couldn't open {}.", app.name)
        };
        if apps_folder_id(&app.launch).is_some() {
            Command::new("explorer.exe")
                .arg(&app.launch)
                .spawn()
                .map(drop)
                .map_err(|err| failed(err.to_string()))
        } else {
            self.app
                .opener()
                .open_path(app.launch.as_str(), None::<&str>)
                .map_err(|err| failed(err.to_string()))
        }
    }

    fn focus_window(&self, window: &WindowEntry) -> Result<(), String> {
        let hwnd = HWND(window.id as isize as *mut std::ffi::c_void);
        // SAFETY: a handle that is no longer a window is detected here; every
        // later call tolerates one that closes in between.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err("That window has closed.".to_string());
        }
        // A window can stop answering after the list was read (between a
        // question and its answer, say). Nothing below would wait for it, but
        // it could not come forward either.
        // SAFETY: a read-only query on a live handle.
        if unsafe { IsHungAppWindow(hwnd) }.as_bool() {
            return Err("That window isn't responding.".to_string());
        }
        // SAFETY: plain user32 calls on a window handle; the attach is undone
        // on the same path that made it. The restore and the raise are posted
        // rather than sent, so a window that hangs from here on cannot hold
        // this thread (a plain ShowWindow or BringWindowToTop on another
        // process's window waits for it to answer).
        let restoring = unsafe {
            // AttachThreadInput needs this thread to have an input queue; a
            // worker that has never touched user32 may not have one yet.
            let _ = IsGUIThread(true);
            let restoring = IsIconic(hwnd).as_bool();
            if restoring {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            }
            // Raised before the attach: only while the two threads' input
            // queues are separate is an async position change posted.
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
            let popup = GetLastActivePopup(hwnd);
            let popup_usable = !popup.is_invalid()
                && IsWindowVisible(popup).as_bool()
                && IsWindowEnabled(popup).as_bool();
            let activate = HWND(activation_target(
                handle_id(hwnd),
                IsWindowEnabled(hwnd).as_bool(),
                handle_id(popup),
                popup_usable,
            ) as *mut _);
            let foreground = GetForegroundWindow();
            let foreground_thread = if foreground.is_invalid() {
                0
            } else {
                GetWindowThreadProcessId(foreground, None)
            };
            let this_thread = GetCurrentThreadId();
            let attached = foreground_thread != 0
                && foreground_thread != this_thread
                && AttachThreadInput(this_thread, foreground_thread, true).as_bool();
            let _ = SetForegroundWindow(activate);
            if attached {
                let _ = AttachThreadInput(this_thread, foreground_thread, false);
            }
            restoring
        };
        let settle = if restoring {
            FOCUS_SETTLE_RESTORED
        } else {
            FOCUS_SETTLE
        };
        let deadline = Instant::now() + settle;
        loop {
            // SAFETY: no arguments, then a read-only query on what it returned.
            let (foreground, root) = unsafe {
                let foreground = GetForegroundWindow();
                let root = if foreground.is_invalid() {
                    foreground
                } else {
                    GetAncestor(foreground, GA_ROOTOWNER)
                };
                (foreground, root)
            };
            if landed_on(handle_id(hwnd), handle_id(foreground), handle_id(root)) {
                if foreground != hwnd {
                    debug!(
                        "The switch to window {} landed on a dialog it owns",
                        window.id
                    );
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                debug!("The foreground switch to window {} was refused", window.id);
                return Err("Windows would not let me bring that window forward.".to_string());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Through the same paste the dictation uses (the user's paste method, the
    /// clipboard put back after), on the main thread like every other caller
    /// of it, with no trailing space and no auto-submit: what was asked for,
    /// exactly. The shared `Enigo` is not held here; the paste locks it itself.
    ///
    /// Nothing here remembers a paste target: the paste only hands the
    /// foreground back to one a dictation remembered (`input::restore_paste_target`),
    /// and the executor forgets that before a command that was not spoken, so
    /// an old dictation's window can never receive a command's text. With no
    /// such target and one of our own windows in front, the text is refused
    /// rather than pasted: a webview cannot take a paste while this holds the
    /// main thread, so it would vanish (the reason `dictation-into-focus`
    /// exists).
    ///
    /// A wait that runs out never leaves a paste queued behind it: one that
    /// has not started is called off and will not run, and the answer says
    /// nothing was typed. One that has started is waited for a bounded while
    /// longer, and if even that runs out the answer is an error that says the
    /// text may still have gone in. An error, not `Ok`, because the step's
    /// outcome is unknown and a plan must not go on to press keys after it as
    /// if the text were there; worded as "may", because claiming nothing was
    /// typed would be the dishonest half.
    fn type_text(&self, text: &str) -> Result<(), String> {
        let (sent, received) = mpsc::channel();
        let app = self.app.clone();
        let text = text.to_string();
        let state = Arc::new(AtomicU8::new(PASTE_PENDING));
        let paste_state = Arc::clone(&state);
        self.app
            .run_on_main_thread(move || {
                if !claim_paste(&paste_state) {
                    // The waiter gave up first; typing now would land in
                    // whatever the user has moved on to.
                    return;
                }
                let typed = if refuses_own_window(
                    own_window_in_front(),
                    crate::input::paste_target_is_elsewhere(),
                ) {
                    Typed::OwnWindow
                } else {
                    match crate::clipboard::paste_with_behavior(
                        text,
                        app,
                        crate::clipboard::PasteBehavior {
                            allow_trailing_space: false,
                            allow_auto_submit: false,
                            smart_spacing: None,
                            // Command mode picks its window itself; never jump back to the
                            // last dictation's target (a typed command would paste there).
                            restore_target: false,
                        },
                    ) {
                        Ok(()) => Typed::Done,
                        Err(err) => Typed::Failed(err),
                    }
                };
                let _ = sent.send(typed);
            })
            .map_err(|err| {
                warn!("Could not reach the main thread to type text: {err}");
                "I couldn't type that.".to_string()
            })?;
        let typed = match received.recv_timeout(PASTE_WAIT) {
            Ok(typed) => typed,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                warn!("The paste for a command was dropped before it ran");
                return Err("I couldn't type that.".to_string());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if abandon_paste(&state) {
                    warn!(
                        "The main thread did not take up a command's paste in time; called it off"
                    );
                    return Err(
                        "Typing that took too long to start, so I didn't type anything."
                            .to_string(),
                    );
                }
                match received.recv_timeout(PASTE_FINISH_WAIT) {
                    Ok(typed) => typed,
                    Err(_) => {
                        warn!("A command's paste started but did not finish in time");
                        return Err("Typing that took too long. It may still have gone through."
                            .to_string());
                    }
                }
            }
        };
        match typed {
            Typed::Done => Ok(()),
            Typed::OwnWindow => Err(
                "Talvio's own window is in front, so there's nowhere to type that. \
                 Switch to the app you want it in first."
                    .to_string(),
            ),
            Typed::Failed(err) => {
                warn!("Typing text failed: {err}");
                Err("I couldn't type that.".to_string())
            }
        }
    }

    fn press_keys(&self, combo: &KeyCombo) -> Result<(), String> {
        if !is_known_key(&combo.key) {
            return Err(format!("I don't know a key called \"{}\".", combo.key));
        }
        let key = key_for(&combo.key)
            .ok_or_else(|| format!("I don't know a key called \"{}\".", combo.key))?;
        let mut modifiers: Vec<Key> = Vec::new();
        for modifier in combo.modifiers.iter().copied().map(modifier_key) {
            if !modifiers.contains(&modifier) {
                modifiers.push(modifier);
            }
        }
        let label = combo.label();
        self.with_keyboard(true, |enigo| {
            press_combo(enigo, &modifiers, key).map_err(|err| {
                warn!("Pressing {label} failed: {err}");
                format!("I couldn't press {label}.")
            })
        })
    }

    fn media(&self, action: MediaAction) -> Result<(), String> {
        let key = media_key(action);
        self.with_keyboard(false, |enigo| {
            enigo.key(key, Direction::Click).map_err(|err| {
                warn!("Sending {key:?} failed: {err}");
                "I couldn't control the media.".to_string()
            })
        })
    }

    fn volume(&self, action: VolumeAction) -> Result<(), String> {
        match action {
            VolumeAction::Up(steps) => self.click_times(Key::VolumeUp, steps),
            VolumeAction::Down(steps) => self.click_times(Key::VolumeDown, steps),
            VolumeAction::Mute => crate::audio_toolkit::ducking::set_output_muted(true),
            VolumeAction::Unmute => crate::audio_toolkit::ducking::set_output_muted(false),
            VolumeAction::Set(percent) => {
                let percent = percent.min(100);
                crate::audio_toolkit::ducking::set_output_volume(f32::from(percent) / 100.0)?;
                // "Set the volume to 40" means hearing it at 40: a muted
                // output is unmuted too. Best effort; the level did change.
                if percent > 0 {
                    if let Err(err) = crate::audio_toolkit::ducking::set_output_muted(false) {
                        debug!("Unmuting after setting the volume failed: {err}");
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `press_keys` refuses only what `is_known_key` refuses, so every name it
    /// accepts must map to a key here, or a valid combo would fail on Windows.
    #[test]
    fn every_named_key_maps_to_a_windows_key() {
        for name in super::super::KEY_NAMES {
            assert!(key_for(name).is_some(), "{name} has no key");
        }
    }

    /// Letters and digits go by virtual-key code, so Ctrl+T is Ctrl+T on any
    /// layout.
    #[test]
    fn letters_and_digits_map_to_their_virtual_key_codes() {
        assert_eq!(key_for("t"), Some(Key::Other(0x54)));
        assert_eq!(key_for("T"), Some(Key::Other(0x54)));
        assert_eq!(key_for("a"), Some(Key::Other(0x41)));
        assert_eq!(key_for("z"), Some(Key::Other(0x5A)));
        assert_eq!(key_for("0"), Some(Key::Other(0x30)));
        assert_eq!(key_for("9"), Some(Key::Other(0x39)));
    }

    #[test]
    fn every_letter_and_digit_maps_to_a_key() {
        for c in ('a'..='z').chain('0'..='9') {
            assert!(key_for(&c.to_string()).is_some(), "{c} has no key");
        }
    }

    /// Names are matched without case, and anything outside the list is
    /// refused rather than guessed at.
    #[test]
    fn named_keys_ignore_case_and_unknown_names_map_to_nothing() {
        assert_eq!(key_for("Enter"), Some(Key::Return));
        assert_eq!(key_for("PAGEDOWN"), Some(Key::PageDown));
        assert_eq!(key_for("ctrl"), None);
        assert_eq!(key_for("hyper"), None);
        assert_eq!(key_for("é"), None);
        assert_eq!(key_for(""), None);
    }

    #[test]
    fn modifiers_and_media_actions_map_to_their_keys() {
        assert_eq!(modifier_key(Modifier::Ctrl), Key::Control);
        assert_eq!(modifier_key(Modifier::Alt), Key::Alt);
        assert_eq!(modifier_key(Modifier::Shift), Key::Shift);
        assert_eq!(modifier_key(Modifier::Meta), Key::Meta);
        assert_eq!(media_key(MediaAction::PlayPause), Key::MediaPlayPause);
        assert_eq!(media_key(MediaAction::Next), Key::MediaNextTrack);
        assert_eq!(media_key(MediaAction::Previous), Key::MediaPrevTrack);
        assert_eq!(media_key(MediaAction::Stop), Key::MediaStop);
    }

    /// The router sees "chrome", not "chrome.exe", and an executable that does
    /// not end in .exe keeps its name.
    #[test]
    fn the_exe_extension_is_stripped_whatever_its_case() {
        assert_eq!(strip_exe("chrome.exe"), "chrome");
        assert_eq!(strip_exe("Code.EXE"), "Code");
        assert_eq!(strip_exe("app.exe.bak"), "app.exe.bak");
        assert_eq!(strip_exe(".exe"), ".exe");
        assert_eq!(strip_exe("notepad"), "notepad");
        assert_eq!(strip_exe("日本語.exe"), "日本語");
    }

    /// A Store app's window belongs to ApplicationFrameHost, which says nothing;
    /// its title is the app's name.
    #[test]
    fn a_store_app_window_is_named_by_its_title() {
        assert_eq!(
            app_label("ApplicationFrameHost", "Calculator"),
            "Calculator"
        );
        assert_eq!(
            app_label("ApplicationFrameHost", "  "),
            "ApplicationFrameHost"
        );
        assert_eq!(app_label("chrome", "Inbox - Gmail"), "chrome");
    }

    #[test]
    fn the_desktop_is_not_a_window_anyone_switches_to() {
        let desktop = WindowEntry {
            id: 1,
            title: "Program Manager".into(),
            app: "explorer".into(),
        };
        assert!(is_desktop(&desktop));
        let folder = WindowEntry {
            id: 2,
            title: "Downloads".into(),
            app: "explorer".into(),
        };
        assert!(!is_desktop(&folder));
    }

    /// `ConvertTo-Json` emits an array for several apps and a bare object for
    /// one; both must read.
    #[test]
    fn start_apps_json_reads_as_an_array_or_a_single_object() {
        let many = r#"[{"Name":"Calculator","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"},{"Name":"Settings","AppID":"windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"}]"#;
        let apps = parse_start_apps(many);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Calculator");
        assert_eq!(
            apps[0].launch,
            "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );

        let one =
            r#"{"Name":"Calculator","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}"#;
        let apps = parse_start_apps(one);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Calculator");
    }

    /// PowerShell can write a byte-order mark, trailing newlines, nothing at
    /// all, or an error; none of it may break the app list.
    #[test]
    fn start_apps_output_that_is_empty_odd_or_broken_gives_no_apps_and_no_panic() {
        assert!(parse_start_apps("").is_empty());
        assert!(parse_start_apps("  \r\n").is_empty());
        assert!(parse_start_apps("not json").is_empty());
        assert!(parse_start_apps("[]").is_empty());
        let with_bom =
            "\u{feff}[{\"Name\":\"Paint\",\"AppID\":\"Microsoft.Paint_8wekyb3d8bbwe!App\"}]\r\n";
        assert_eq!(parse_start_apps(with_bom).len(), 1);
    }

    /// A row with no name or no AppID cannot be offered or launched.
    #[test]
    fn start_apps_rows_missing_a_name_or_an_id_are_skipped() {
        let json = r#"[{"Name":"","AppID":"a!App"},{"Name":"Paint"},{"AppID":"b!App"},{"Name":"Notepad","AppID":null},{"Name":" Photos ","AppID":"Microsoft.Windows.Photos_8wekyb3d8bbwe!App"}]"#;
        let apps = parse_start_apps(json);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Photos");
    }

    /// Installers drop helper shortcuts beside the app; only the app counts.
    #[test]
    fn start_menu_helpers_and_non_shortcuts_are_not_apps() {
        let name = |p: &str| shortcut_app_name(Path::new(p));
        assert_eq!(
            name(r"C:\Start Menu\Programs\Spotify.lnk").as_deref(),
            Some("Spotify")
        );
        assert_eq!(
            name(r"C:\Start Menu\Programs\Visual Studio Code\Visual Studio Code.LNK").as_deref(),
            Some("Visual Studio Code")
        );
        assert_eq!(
            name(r"C:\Start Menu\Programs\7-Zip\Uninstall 7-Zip.lnk"),
            None
        );
        assert_eq!(name(r"C:\Start Menu\Programs\Foo\Foo Readme.lnk"), None);
        assert_eq!(name(r"C:\Start Menu\Programs\Foo\Foo Help.lnk"), None);
        assert_eq!(name(r"C:\Start Menu\Programs\Foo\Foo Website.lnk"), None);
        assert_eq!(name(r"C:\Start Menu\Programs\desktop.ini"), None);
        assert_eq!(name(r"C:\Start Menu\Programs\Foo.url"), None);
    }

    /// A shortcut wins over a Store entry with the same name, compared without
    /// case, and nothing appears twice.
    #[test]
    fn merged_apps_keep_the_shortcut_and_drop_duplicate_names() {
        let entry = |name: &str, launch: &str| AppEntry {
            name: name.into(),
            launch: launch.into(),
        };
        let apps = merge_apps(
            vec![
                entry("Spotify", r"C:\Programs\Spotify.lnk"),
                entry("spotify", r"C:\Other\Spotify.lnk"),
            ],
            vec![
                entry("Spotify", "shell:AppsFolder\\SpotifyAB.Spotify!Spotify"),
                entry("Calculator", "shell:AppsFolder\\Calc!App"),
            ],
        );
        assert_eq!(
            apps,
            vec![
                entry("Spotify", r"C:\Programs\Spotify.lnk"),
                entry("Calculator", "shell:AppsFolder\\Calc!App"),
            ]
        );
    }

    /// A dialog in front reads as the window it belongs to, so its id is the
    /// one `open_windows` lists; an invisible owner is nobody's window, so the
    /// dialog itself is reported then.
    #[test]
    fn the_foreground_is_reported_as_its_visible_root_owner() {
        // An unowned window is its own root.
        assert_eq!(reported_window(10, 10, true), 10);
        // A Save As dialog (20) owned by a visible editor (10).
        assert_eq!(reported_window(20, 10, true), 10);
        // Owned by a hidden framework window.
        assert_eq!(reported_window(20, 10, false), 20);
        // The root could not be read.
        assert_eq!(reported_window(20, 0, false), 20);
    }

    /// A switch counts when the target or a window it owns is in front; a
    /// modal dialog takes the activation of its disabled owner.
    #[test]
    fn a_switch_lands_on_the_window_or_a_dialog_it_owns() {
        assert!(landed_on(10, 10, 10));
        assert!(landed_on(10, 20, 10));
        assert!(!landed_on(10, 30, 30));
        assert!(!landed_on(10, 0, 0));
        // A null target never "lands", even with nothing in front.
        assert!(!landed_on(0, 0, 0));
    }

    /// Only a window disabled behind a modal dialog hands its activation to
    /// that dialog; an enabled one is activated itself, so a find bar or tool
    /// palette never takes keys meant for the main window.
    #[test]
    fn the_last_popup_is_activated_only_for_an_owner_blocked_by_it() {
        assert_eq!(activation_target(10, false, 20, true), 20);
        assert_eq!(activation_target(10, true, 20, true), 10);
        assert_eq!(activation_target(10, false, 20, false), 10);
        assert_eq!(activation_target(10, false, 10, true), 10);
        assert_eq!(activation_target(10, false, 0, true), 10);
    }

    /// Exactly one side wins: a paste the waiter called off never runs, and a
    /// paste already under way cannot be called off (so the waiter knows the
    /// text may land).
    #[test]
    fn a_paste_is_either_taken_up_or_called_off_never_both() {
        let state = AtomicU8::new(PASTE_PENDING);
        assert!(abandon_paste(&state));
        assert!(!claim_paste(&state));

        let state = AtomicU8::new(PASTE_PENDING);
        assert!(claim_paste(&state));
        assert!(!abandon_paste(&state));
        assert!(!claim_paste(&state));
    }

    /// The extra wait for a started paste outlasts the paste's own restore
    /// timeout, so a paste that finishes normally is never reported as stuck.
    #[test]
    fn a_started_paste_is_waited_for_longer_than_it_can_take() {
        assert!(PASTE_FINISH_WAIT > crate::paste_tx::RESTORE_TIMEOUT);
    }

    /// Our own window in front swallows a paste, unless a remembered target
    /// takes the foreground back first.
    #[test]
    fn typing_is_refused_only_into_our_own_window_with_nowhere_else_meant() {
        assert!(refuses_own_window(true, false));
        assert!(!refuses_own_window(true, true));
        assert!(!refuses_own_window(false, false));
        assert!(!refuses_own_window(false, true));
    }

    /// A failed Store-app read is retried soon instead of hiding Calculator
    /// and Settings for the full ten minutes.
    #[test]
    fn a_list_without_store_apps_is_kept_only_briefly() {
        assert_eq!(apps_lifetime(true), APPS_FRESH_FOR);
        assert_eq!(apps_lifetime(false), STORE_APPS_RETRY_AFTER);
        assert!(STORE_APPS_RETRY_AFTER < APPS_FRESH_FOR);
    }

    /// `Get-StartApps` lists desktop programs too, so an answer with no app in
    /// it (broken JSON parses to none) is a failed read, not an empty machine.
    #[test]
    fn an_empty_or_unreadable_start_apps_answer_counts_as_failed() {
        assert!(store_apps_read(parse_start_apps("")).is_none());
        assert!(store_apps_read(parse_start_apps("not json")).is_none());
        assert!(store_apps_read(parse_start_apps("[]")).is_none());
        let one =
            r#"{"Name":"Calculator","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}"#;
        assert_eq!(
            store_apps_read(parse_start_apps(one)).map(|apps| apps.len()),
            Some(1)
        );
    }

    #[test]
    fn a_store_launch_string_is_told_apart_from_a_path() {
        assert_eq!(
            apps_folder_id("shell:AppsFolder\\Calc!App"),
            Some("Calc!App")
        );
        assert_eq!(
            apps_folder_id("SHELL:appsfolder\\Calc!App"),
            Some("Calc!App")
        );
        assert_eq!(apps_folder_id("shell:AppsFolder\\"), None);
        assert_eq!(apps_folder_id(r"C:\Programs\Spotify.lnk"), None);
        assert_eq!(apps_folder_id("shell"), None);
    }
}
