//! macOS side effects for command mode.
//!
//! This file cannot be compiled on the Windows machine it was written on, so it
//! leans on what the rest of the crate already compiles on a Mac and keeps the
//! new Objective-C surface as small as it can be:
//!
//! * **Apps and windows come from `NSWorkspace`.** macOS activates apps, not
//!   windows, so a [`WindowEntry`] here is a running app: its id is the pid
//!   and, in [`Effects::open_windows`], its title the app's name. The
//!   foreground entry carries the focused window's real title instead (see
//!   [`foreground_window`]); both use the pid as the id, so the two compare
//!   equal. Only four typed AppKit calls are used
//!   (`sharedWorkspace`, `frontmostApplication`, `runningApplications`,
//!   `localizedName`); everything else goes through `msg_send!`, whose
//!   selectors are the stable Objective-C names rather than generated Rust
//!   method names that could differ between objc2 releases.
//! * **The focused window's title comes from Accessibility**, the same calls
//!   smart spacing makes (`smart_spacing/macos.rs`), on a short-lived thread
//!   with 0.2 s messaging timeouts: an AX call is answered by the target app's
//!   main thread and a hung app must cost a missing title, not a stuck command.
//!   Without the Accessibility permission the title falls back to the app's
//!   name.
//! * **A window is only as identifiable as its title.** The id is a pid, so
//!   two windows (or two tabs) of one browser share it; a step that must act
//!   on one particular window, like sending a drafted email, can only tell it
//!   from its siblings by the focused window's title read just before acting.
//! * **Launching and activating go through `/usr/bin/open`.** LaunchServices
//!   activates a running app reliably, where `-[NSRunningApplication
//!   activateWithOptions:]` from a background process is refused by macOS 14's
//!   cooperative activation more often than not.
//! * **Every keystroke runs on the main thread.** enigo reads the keyboard
//!   layout through Text Input Sources and posts media keys as AppKit events,
//!   and both assert or crash off the main thread on recent macOS; the paste
//!   path's pasteboard promise must be published there too (`paste_tx/macos.rs`).
//! * **Absolute volume and mute go through the ducking module's helpers**,
//!   which open the default output afresh; the volume keys only step.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use enigo::{Direction, Enigo, Key, Keyboard};
use log::{debug, info, warn};
use objc2::msg_send;
use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement};
use objc2_core_foundation::{CFRetained, CFString, CFType};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use super::{
    is_known_key, AppEntry, Effects, KeyCombo, MediaAction, SystemEffects, VolumeAction,
    WindowEntry,
};
use crate::command_mode::registry::Modifier;

/// Messaging timeout for each AX call, in seconds (smart spacing's value).
const AX_TIMEOUT_SECS: f32 = 0.2;

/// How long the caller waits for the title read as a whole. A few AX round
/// trips at most; past this the context line goes without a title.
const TITLE_BUDGET: Duration = Duration::from_millis(500);

/// How long a keystroke or paste may wait for the main thread. The main
/// thread is only ever busy for moments, so this only fires when something is
/// badly wrong, and then the command fails instead of hanging.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Folder levels searched below each app folder: its own entries, then one
/// vendor folder down ("/Applications/Adobe Photoshop 2025/Adobe Photoshop
/// 2025.app"), the way Adobe and other suites install. Deeper is not a layout
/// anyone installs to, and bundles are never searched inside.
const MAX_SCAN_DEPTH: usize = 2;

/// Directory entries looked at across one whole scan, so a pathological folder
/// (thousands of files, a slow network mount) cannot stall a command.
const MAX_SCAN_ENTRIES: usize = 5_000;

/// Apps that live outside the app folders but are still asked for by name.
/// Named one by one: the folder they sit in (CoreServices) is full of system
/// agents nobody opens by name.
const EXTRA_BUNDLES: &[&str] = &["/System/Library/CoreServices/Finder.app"];

/// How long the installed-app list is reused. Scanning five folders is cheap,
/// but a command often reads the list twice (resolve, then confirm), and an app
/// installed a minute ago is rare enough to wait for.
const INSTALLED_APPS_TTL: Duration = Duration::from_secs(10 * 60);

/// Most volume-key presses one command sends. macOS has 16 steps from silent
/// to full, so more than that only queues indicator animations.
const MAX_VOLUME_STEPS: u8 = 16;

/// `NSApplicationActivationPolicyRegular`: an app with a Dock icon and menu
/// bar, the only kind a person switches to.
const ACTIVATION_POLICY_REGULAR: isize = 0;

/// The `open` tool, by absolute path so nothing on `PATH` can stand in for it.
const OPEN_TOOL: &str = "/usr/bin/open";

static INSTALLED_APPS: Mutex<Option<(Instant, Vec<AppEntry>)>> = Mutex::new(None);

/* ───────────────────────────── foreground ───────────────────────────── */

/// The frontmost app, as a [`WindowEntry`] whose id is its pid, the same id
/// [`Effects::open_windows`] gives it (see the module docs for why an app and
/// not a window). `None` when it is Talvio itself, AppKit has no answer, or
/// the front app kept changing while it was read.
///
/// The title is the focused window's `AXTitle` when Accessibility is granted
/// and the app answers within [`TITLE_BUDGET`], else the app's name. A send
/// step compares this title with the window it means to act on just before
/// pressing its key, so the read must never pair one app's pid with another's
/// title: the frontmost pid is read again afterwards, and the whole read is
/// retried once when it moved.
pub fn foreground_window() -> Option<WindowEntry> {
    for _ in 0..2 {
        let (pid, name) = frontmost_app()?;
        if pid == own_pid() {
            return None;
        }
        let title = focused_window_title(pid);
        if frontmost_app().map(|(now, _)| now) != Some(pid) {
            debug!("Command mode: the front app changed while its title was read");
            continue;
        }
        return Some(foreground_entry(pid, name, title));
    }
    None
}

/// The entry for the front app `pid`: its focused window's title when one was
/// read, else its name. A check that needs a particular draft's or tab's
/// title therefore fails closed when Accessibility could not provide it,
/// as long as it does not accept a bare app name as that title.
fn foreground_entry(pid: i32, name: String, title: Option<String>) -> WindowEntry {
    let title = title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| name.clone());
    WindowEntry {
        id: i64::from(pid),
        title,
        app: name,
    }
}

#[allow(unused_unsafe)]
fn frontmost_app() -> Option<(i32, String)> {
    autoreleasepool(|_| {
        let workspace = unsafe { NSWorkspace::sharedWorkspace() };
        let app = unsafe { workspace.frontmostApplication() }?;
        let pid = pid_of(&app);
        if pid <= 0 {
            return None;
        }
        Some((pid, app_name(&app, pid)))
    })
}

/// Regular running apps, the frontmost first, never Talvio.
#[allow(unused_unsafe)]
fn running_apps() -> Vec<WindowEntry> {
    let own = own_pid();
    let front = frontmost_app().map(|(pid, _)| pid);
    let mut entries: Vec<WindowEntry> = autoreleasepool(|_| {
        let workspace = unsafe { NSWorkspace::sharedWorkspace() };
        let apps = unsafe { workspace.runningApplications() };
        let count: usize = unsafe { msg_send![&*apps, count] };
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let app: Retained<NSRunningApplication> =
                unsafe { msg_send![&*apps, objectAtIndex: index] };
            let policy: isize = unsafe { msg_send![&*app, activationPolicy] };
            if policy != ACTIVATION_POLICY_REGULAR {
                continue;
            }
            let pid = pid_of(&app);
            if pid <= 0 || pid == own {
                continue;
            }
            let name = app_name(&app, pid);
            if name.is_empty() {
                continue;
            }
            entries.push(WindowEntry {
                id: i64::from(pid),
                title: name.clone(),
                app: name,
            });
        }
        entries
    });
    front_first(&mut entries, front);
    entries
}

/// Move the frontmost app's entry to the front, keeping the rest in order.
fn front_first(entries: &mut [WindowEntry], front: Option<i32>) {
    let Some(front) = front else { return };
    if let Some(position) = entries.iter().position(|e| e.id == i64::from(front)) {
        entries[..=position].rotate_right(1);
    }
}

fn pid_of(app: &NSRunningApplication) -> i32 {
    // `processIdentifier` is typed `pid_t` in objc2-app-kit only behind its
    // `libc` feature; the selector itself is fixed, so ask for it directly.
    unsafe { msg_send![app, processIdentifier] }
}

#[allow(unused_unsafe)]
fn app_name(app: &NSRunningApplication, pid: i32) -> String {
    let localized = unsafe { app.localizedName() }
        .map(|name| name.to_string())
        .filter(|name| !name.trim().is_empty());
    localized
        .or_else(|| app_bundle_path(pid).and_then(|path| bundle_stem(Path::new(&path))))
        .unwrap_or_default()
}

fn own_pid() -> i32 {
    std::process::id() as i32
}

/* ───────────────────────────── AX title ───────────────────────────── */

/// The focused window's title of the app `pid`, read on its own thread so a
/// hung app costs [`TITLE_BUDGET`] at most.
fn focused_window_title(pid: i32) -> Option<String> {
    if !unsafe { AXIsProcessTrusted() } {
        return None;
    }
    let (reply, answer) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("command-mode-ax".into())
        .spawn(move || {
            let title = std::panic::catch_unwind(|| read_focused_title(pid))
                .ok()
                .flatten();
            let _ = reply.try_send(title);
        });
    if spawned.is_err() {
        return None;
    }
    answer.recv_timeout(TITLE_BUDGET).ok().flatten()
}

fn read_focused_title(pid: i32) -> Option<String> {
    let system = unsafe { AXUIElement::new_system_wide() };
    set_timeout(&system);
    let app = attr_element(&system, "AXFocusedApplication")?;
    set_timeout(&app);
    // Focus can move between the AppKit read and this one; a title from a
    // different app than the one named would mislabel the context.
    if element_pid(&app)? != pid {
        return None;
    }
    let window = attr_element(&app, "AXFocusedWindow")?;
    set_timeout(&window);
    attr_string(&window, "AXTitle").filter(|title| !title.trim().is_empty())
}

// The AX helpers below are copies of smart_spacing/macos.rs's private ones.

fn attr(element: &AXUIElement, name: &str) -> Result<CFRetained<CFType>, AXError> {
    let name = CFString::from_str(name);
    let mut value: *const CFType = std::ptr::null();
    let error = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut value)) };
    if error != AXError::Success {
        return Err(error);
    }
    NonNull::new(value as *mut CFType)
        .map(|ptr| unsafe { CFRetained::from_raw(ptr) })
        .ok_or(AXError::NoValue)
}

fn attr_string(element: &AXUIElement, name: &str) -> Option<String> {
    let value = attr(element, name).ok()?;
    value.downcast_ref::<CFString>().map(|s| s.to_string())
}

fn attr_element(element: &AXUIElement, name: &str) -> Option<CFRetained<AXUIElement>> {
    attr(element, name).ok()?.downcast::<AXUIElement>().ok()
}

fn element_pid(element: &AXUIElement) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    let error = unsafe { element.pid(NonNull::from(&mut pid)) };
    (error == AXError::Success && pid > 0).then_some(pid)
}

fn set_timeout(element: &AXUIElement) {
    unsafe {
        let _ = element.set_messaging_timeout(AX_TIMEOUT_SECS);
    }
}

/// The `.app` bundle a process runs from (copied from smart spacing).
fn app_bundle_path(pid: i32) -> Option<String> {
    let mut buffer = vec![0u8; 4096];
    let len = unsafe {
        libc::proc_pidpath(
            pid,
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            buffer.len() as u32,
        )
    };
    if len <= 0 {
        return None;
    }
    let path = String::from_utf8_lossy(&buffer[..len as usize]).into_owned();
    bundle_of_executable(&path)
}

/// ".../Foo.app/Contents/MacOS/Foo" -> ".../Foo.app". The innermost bundle,
/// the one whose own `Contents/MacOS` holds the executable: Simulator runs
/// from ".../Xcode.app/Contents/Developer/Applications/Simulator.app/Contents/
/// MacOS/Simulator", and the outermost bundle would switch to (or launch)
/// Xcode instead. A helper process maps to its own helper bundle too, but
/// helpers are never Regular apps, so [`running_apps`] never lists one.
fn bundle_of_executable(path: &str) -> Option<String> {
    let end = path
        .rfind(".app/Contents/MacOS/")
        .or_else(|| path.rfind(".app/"))
        .map(|i| i + ".app".len())?;
    Some(path[..end].to_string())
}

/* ───────────────────────────── installed apps ───────────────────────────── */

/// Where apps live, in the order a duplicate name is resolved: the user's own
/// install first, then Apple's, then the per-user folder.
fn app_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = home {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// "Visual Studio Code" for ".../Visual Studio Code.app"; `None` for anything
/// that is not a visible app bundle.
fn bundle_stem(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    if file_name.starts_with('.') {
        return None;
    }
    let lower = file_name.to_ascii_lowercase();
    if !lower.ends_with(".app") {
        return None;
    }
    let stem = file_name[..file_name.len() - ".app".len()].trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

/// Every app bundle in `dirs` and one vendor folder below them (see
/// [`MAX_SCAN_DEPTH`]), then each of `extra` that exists, one per name (the
/// first found wins), sorted by name so the list is stable from scan to scan.
fn scan_app_dirs(dirs: &[PathBuf], extra: &[PathBuf]) -> Vec<AppEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut apps = Vec::new();
    let mut budget = MAX_SCAN_ENTRIES;
    for dir in dirs {
        let mut found = Vec::new();
        collect_bundles(dir, 1, &mut budget, &mut found);
        for path in found {
            add_bundle(path, &mut seen, &mut apps);
        }
    }
    for bundle in extra {
        if bundle.is_dir() {
            add_bundle(bundle.clone(), &mut seen, &mut apps);
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// Add the bundle at `path` unless an app of the same name came first.
fn add_bundle(
    path: PathBuf,
    seen: &mut std::collections::HashSet<String>,
    apps: &mut Vec<AppEntry>,
) {
    let Some(name) = bundle_stem(&path) else {
        return;
    };
    if !seen.insert(name.to_lowercase()) {
        return;
    }
    apps.push(AppEntry {
        name,
        launch: path.to_string_lossy().into_owned(),
    });
}

/// The `.app` bundles in `dir`, then those in its plain subfolders while
/// `depth` < [`MAX_SCAN_DEPTH`], so a bundle at the top of a folder wins a
/// name over one in a vendor folder below it. A bundle is never searched
/// inside (its helpers are not apps anyone opens), hidden folders are skipped,
/// and a symlinked folder is not followed, so a link cannot loop the scan; a
/// symlinked bundle still counts, since some installers link one into
/// /Applications. `budget` caps the entries read across the whole scan.
fn collect_bundles(dir: &Path, depth: usize, budget: &mut usize, out: &mut Vec<PathBuf>) {
    if *budget == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().take(*budget).collect();
    *budget -= entries.len();
    // A stable order, so which of two same-named bundles wins does not depend
    // on the filesystem's.
    entries.sort_by_key(|entry| entry.file_name());
    let mut subfolders = Vec::new();
    for entry in entries {
        let path = entry.path();
        if bundle_stem(&path).is_some() {
            // `is_dir` follows a symlink: a linked bundle is still a bundle,
            // and a plain file named ".app" is not one.
            if path.is_dir() {
                out.push(path);
            }
            continue;
        }
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        // `file_type` does not follow symlinks, so a linked folder is skipped.
        let plain_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if plain_dir && !hidden && depth < MAX_SCAN_DEPTH {
            subfolders.push(path);
        }
    }
    for folder in subfolders {
        collect_bundles(&folder, depth + 1, budget, out);
    }
}

fn installed_apps_cached() -> Vec<AppEntry> {
    if let Ok(cache) = INSTALLED_APPS.lock() {
        if let Some((at, apps)) = cache.as_ref() {
            if at.elapsed() < INSTALLED_APPS_TTL {
                return apps.clone();
            }
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let extra: Vec<PathBuf> = EXTRA_BUNDLES.iter().map(PathBuf::from).collect();
    let apps = scan_app_dirs(&app_dirs(home.as_deref()), &extra);
    debug!("Command mode: found {} installed apps", apps.len());
    if let Ok(mut cache) = INSTALLED_APPS.lock() {
        *cache = Some((Instant::now(), apps.clone()));
    }
    apps
}

/// A launch target from [`AppEntry::launch`] is only ever an absolute path to
/// an `.app` bundle; anything else (a relative path, something starting with
/// `-` that `open` would read as a flag) is refused rather than handed over.
fn valid_bundle_path(launch: &str) -> bool {
    launch.starts_with('/') && bundle_stem(Path::new(launch.trim_end_matches('/'))).is_some()
}

fn run_open(args: &[&str], what: &str) -> Result<(), String> {
    let output = Command::new(OPEN_TOOL)
        .args(args)
        .output()
        .map_err(|e| format!("Couldn't {what}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    warn!(
        "Command mode: `open` failed to {what} ({}): {}",
        output.status,
        stderr.trim()
    );
    Err(format!("Couldn't {what}."))
}

/* ───────────────────────────── keys ───────────────────────────── */

/// The ANSI virtual keycode of a letter, digit or punctuation key name.
///
/// Physical keycodes, like the paste path's Cmd+V (`input.rs`), because
/// enigo's layout lookup for `Key::Unicode` falls back to keycode 0 (the A key)
/// when the current layout has no such character, so on a Russian or Greek
/// layout Cmd+T would become Cmd+A. macOS reads Command shortcuts on those
/// layouts as QWERTY, which these codes are. The cost is AZERTY and QWERTZ,
/// where the few letters that move (A/Q, Z/W, Y/Z, M) land on the US position.
fn ansi_keycode(name: &str) -> Option<u32> {
    let code = match name {
        "a" => 0x00,
        "s" => 0x01,
        "d" => 0x02,
        "f" => 0x03,
        "h" => 0x04,
        "g" => 0x05,
        "z" => 0x06,
        "x" => 0x07,
        "c" => 0x08,
        "v" => 0x09,
        "b" => 0x0B,
        "q" => 0x0C,
        "w" => 0x0D,
        "e" => 0x0E,
        "r" => 0x0F,
        "y" => 0x10,
        "t" => 0x11,
        "1" => 0x12,
        "2" => 0x13,
        "3" => 0x14,
        "4" => 0x15,
        "6" => 0x16,
        "5" => 0x17,
        "equal" => 0x18,
        "9" => 0x19,
        "7" => 0x1A,
        "minus" => 0x1B,
        "8" => 0x1C,
        "0" => 0x1D,
        "bracketright" => 0x1E,
        "o" => 0x1F,
        "u" => 0x20,
        "bracketleft" => 0x21,
        "i" => 0x22,
        "p" => 0x23,
        "l" => 0x25,
        "j" => 0x26,
        "quote" => 0x27,
        "k" => 0x28,
        "semicolon" => 0x29,
        "backslash" => 0x2A,
        "comma" => 0x2B,
        "slash" => 0x2C,
        "n" => 0x2D,
        "m" => 0x2E,
        "period" => 0x2F,
        "backquote" => 0x32,
        _ => return None,
    };
    Some(code)
}

/// The enigo key for a [`KeyCombo::key`] name.
fn key_for(name: &str) -> Result<Key, String> {
    let name = name.to_ascii_lowercase();
    if !is_known_key(&name) {
        return Err(format!("\"{name}\" isn't a key I can press."));
    }
    if let Some(code) = ansi_keycode(&name) {
        return Ok(Key::Other(code));
    }
    let key = match name.as_str() {
        "enter" => Key::Return,
        "tab" => Key::Tab,
        "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        // Forward delete, the key labelled Delete on a full Windows keyboard.
        "delete" => Key::Delete,
        "insert" => return Err("A Mac keyboard has no Insert key.".to_string()),
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
        other => return Err(format!("\"{other}\" isn't a key I can press.")),
    };
    Ok(key)
}

fn modifier_key(modifier: Modifier) -> Key {
    match modifier {
        Modifier::Ctrl => Key::Control,
        Modifier::Alt => Key::Alt,
        Modifier::Shift => Key::Shift,
        // Command.
        Modifier::Meta => Key::Meta,
    }
}

/// enigo has no Stop media key on macOS (`Key::MediaStop` is compiled out
/// there), and the system has no stop event either: Stop pauses, which is
/// what stopping playback looks like on a Mac.
fn media_key(action: MediaAction) -> Key {
    match action {
        MediaAction::PlayPause | MediaAction::Stop => Key::MediaPlayPause,
        MediaAction::Next => Key::MediaNextTrack,
        MediaAction::Previous => Key::MediaPrevTrack,
    }
}

/// Presses for a step request: at least one (asking for zero steps still
/// means "turn it up"), at most [`MAX_VOLUME_STEPS`].
fn volume_steps(requested: u8) -> u8 {
    requested.clamp(1, MAX_VOLUME_STEPS)
}

/// Press the modifiers, click the key, release the modifiers in reverse. A
/// modifier that went down is always released, even when the click failed.
fn press_combo(enigo: &mut Enigo, modifiers: &[Key], key: Key) -> Result<(), String> {
    let mut pressed: Vec<Key> = Vec::with_capacity(modifiers.len());
    let mut result = Ok(());
    for &modifier in modifiers {
        match enigo.key(modifier, Direction::Press) {
            Ok(()) => pressed.push(modifier),
            Err(e) => {
                result = Err(format!("Couldn't press a modifier key: {e}"));
                break;
            }
        }
    }
    if result.is_ok() {
        result = enigo
            .key(key, Direction::Click)
            .map_err(|e| format!("Couldn't press the key: {e}"));
        if result.is_ok() && !pressed.is_empty() {
            // Same reason as the paste's pause: some apps read the modifier
            // state a moment after the key event arrives.
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    for &modifier in pressed.iter().rev() {
        let _ = enigo.key(modifier, Direction::Release);
    }
    result
}

/// A queued main-thread job that has neither started nor been given up on.
const JOB_PENDING: u8 = 0;
/// The job started: its effect is under way and will finish.
const JOB_RUNNING: u8 = 1;
/// The waiter gave up first: the job must never act.
const JOB_ABANDONED: u8 = 2;

/// Called by the queued job before it acts: true when it may run. Exactly one
/// of this and [`abandon_job`] wins, so the effect either runs while someone
/// still waits for it or never runs at all.
fn start_job(state: &AtomicU8) -> bool {
    state
        .compare_exchange(JOB_PENDING, JOB_RUNNING, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// Called by the waiter when its deadline passes: true when the job had not
/// started and now never will; false when it is already running.
fn abandon_job(state: &AtomicU8) -> bool {
    state
        .compare_exchange(
            JOB_PENDING,
            JOB_ABANDONED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
}

/// Run `f` on the main thread and wait for its answer. Runs it in place when
/// already there, since queueing onto our own thread and waiting would
/// deadlock.
///
/// A job still queued when [`MAIN_THREAD_TIMEOUT`] passes is cancelled, not
/// just no longer waited for: the command reports a failure and the user
/// moves on, so a keystroke or paste that ran later would land in whatever
/// app is in front by then. A job that has already started when the deadline
/// passes is waited for once more, since its effect is happening and
/// reporting a failure for it would be false.
fn on_main_thread<R, F>(app: &AppHandle, what: &str, f: F) -> Result<R, String>
where
    R: Send + 'static,
    F: FnOnce(&AppHandle) -> Result<R, String> + Send + 'static,
{
    if objc2::MainThreadMarker::new().is_some() {
        return f(app);
    }
    let (reply, answer) = mpsc::channel();
    let handle = app.clone();
    let state = Arc::new(AtomicU8::new(JOB_PENDING));
    let job_state = Arc::clone(&state);
    app.run_on_main_thread(move || {
        if !start_job(&job_state) {
            debug!("Command mode: dropped a main-thread job its caller gave up on");
            return;
        }
        let _ = reply.send(f(&handle));
    })
    .map_err(|e| format!("Couldn't {what}: {e}"))?;
    match answer.recv_timeout(MAIN_THREAD_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) if abandon_job(&state) => {
            warn!("Command mode: the main thread stayed busy, so the action was cancelled");
            Err(format!("Couldn't {what}: the app didn't respond in time."))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => answer
            .recv_timeout(MAIN_THREAD_TIMEOUT)
            .map_err(|_| format!("Couldn't {what}: the app didn't finish in time."))?,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("Couldn't {what}: the app didn't respond."))
        }
    }
}

/// Run `f` with the shared enigo, marked as our own input so the hotkey hook
/// does not read it back as a shortcut, and with every modifier released
/// afterwards whatever happened (the paste path's safety net).
fn with_enigo<R>(
    app: &AppHandle,
    f: impl FnOnce(&mut Enigo) -> Result<R, String>,
) -> Result<R, String> {
    let state = app.try_state::<crate::input::EnigoState>().ok_or_else(|| {
        "Keyboard control isn't ready yet. Open the Talvio window once, then try again.".to_string()
    })?;
    let mut enigo = state
        .0
        .lock()
        .map_err(|e| format!("Keyboard control is unavailable: {e}"))?;
    let _injected = handy_keys::ignore_injected_input();
    let result = f(&mut *enigo);
    crate::input::release_all_modifiers(&mut *enigo);
    result
}

/* ───────────────────────────── Effects ───────────────────────────── */

impl Effects for SystemEffects {
    fn installed_apps(&self) -> Vec<AppEntry> {
        installed_apps_cached()
    }

    fn open_windows(&self) -> Vec<WindowEntry> {
        running_apps()
    }

    /// macOS keeps no simple recent-files list: the real one lives in
    /// per-app shared file lists or behind a Spotlight query
    /// (`kMDItemLastUsedDate`), which is Phase 4 work. Empty until then; the
    /// tools fall back to the known folders.
    fn recent_files(&self) -> Vec<PathBuf> {
        Vec::new()
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
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("Couldn't open the link: {e}"))
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| format!("Couldn't open {}: {e}", path.display()))
    }

    fn launch_app(&self, app: &AppEntry) -> Result<(), String> {
        if !valid_bundle_path(&app.launch) {
            warn!("Command mode: refused to launch a target that is not an app bundle");
            return Err(format!("Couldn't open {}.", app.name));
        }
        info!("Command mode: launching {}", app.name);
        run_open(&[&app.launch], &format!("open {}", app.name))
    }

    /// Activates the app the pid belongs to, not one of its windows: which of
    /// its windows comes forward is the app's own key window. A step that
    /// needs a particular window must check [`foreground_window`]'s title
    /// afterwards (see the module docs).
    fn focus_window(&self, window: &WindowEntry) -> Result<(), String> {
        let pid = i32::try_from(window.id).unwrap_or(0);
        if pid == own_pid() {
            return Err("That's Talvio itself.".to_string());
        }
        // The bundle the process runs from names exactly this app; the name
        // is the fallback, and could match another copy of the app.
        let target = (pid > 0)
            .then(|| app_bundle_path(pid))
            .flatten()
            .unwrap_or_else(|| window.app.clone());
        if target.trim().is_empty() || target.starts_with('-') {
            return Err(format!("Couldn't switch to {}.", window.app));
        }
        info!("Command mode: switching to {}", window.app);
        run_open(&["-a", &target], &format!("switch to {}", window.app))
    }

    fn type_text(&self, text: &str) -> Result<(), String> {
        let text = text.to_string();
        debug!(
            "Command mode: typing {} characters into the focused app",
            text.chars().count()
        );
        on_main_thread(&self.app, "type the text", move |app| {
            crate::clipboard::paste_with_behavior(
                text,
                app.clone(),
                crate::clipboard::PasteBehavior {
                    allow_trailing_space: false,
                    allow_auto_submit: false,
                    smart_spacing: None,
                    // Command mode picks its window itself; never jump back to the
                    // last dictation's target (a typed command would paste there).
                    restore_target: false,
                },
            )
        })
    }

    fn press_keys(&self, combo: &KeyCombo) -> Result<(), String> {
        let key = key_for(&combo.key)?;
        let modifiers: Vec<Key> = combo.modifiers.iter().copied().map(modifier_key).collect();
        let label = combo.label();
        debug!("Command mode: pressing {label}");
        on_main_thread(&self.app, &format!("press {label}"), move |app| {
            with_enigo(app, |enigo| press_combo(enigo, &modifiers, key))
        })
    }

    fn media(&self, action: MediaAction) -> Result<(), String> {
        let key = media_key(action);
        debug!("Command mode: media {action:?}");
        on_main_thread(&self.app, "control playback", move |app| {
            with_enigo(app, |enigo| {
                enigo
                    .key(key, Direction::Click)
                    .map_err(|e| format!("Couldn't send the media key: {e}"))
            })
        })
    }

    fn volume(&self, action: VolumeAction) -> Result<(), String> {
        debug!("Command mode: volume {action:?}");
        match action {
            VolumeAction::Up(steps) | VolumeAction::Down(steps) => {
                let key = if matches!(action, VolumeAction::Up(_)) {
                    Key::VolumeUp
                } else {
                    Key::VolumeDown
                };
                let steps = volume_steps(steps);
                on_main_thread(&self.app, "change the volume", move |app| {
                    with_enigo(app, |enigo| {
                        for _ in 0..steps {
                            enigo
                                .key(key, Direction::Click)
                                .map_err(|e| format!("Couldn't change the volume: {e}"))?;
                        }
                        Ok(())
                    })
                })
            }
            VolumeAction::Mute => crate::audio_toolkit::ducking::set_output_muted(true),
            VolumeAction::Unmute => crate::audio_toolkit::ducking::set_output_muted(false),
            VolumeAction::Set(level) => {
                let level = level.min(100);
                crate::audio_toolkit::ducking::set_output_volume(f32::from(level) / 100.0)?;
                // "Set the volume to 40" on muted speakers means "let me hear
                // it at 40"; a level that stays silent would read as failure.
                if level > 0 {
                    if let Err(e) = crate::audio_toolkit::ducking::set_output_muted(false) {
                        warn!("Command mode: set the volume but could not unmute: {e}");
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn window(id: i64, name: &str) -> WindowEntry {
        WindowEntry {
            id,
            title: name.to_string(),
            app: name.to_string(),
        }
    }

    /// The bundle name is what a person calls the app, so it must survive
    /// spaces and case and reject anything that is not a visible bundle.
    #[test]
    fn a_bundle_is_named_by_its_stem_and_only_app_bundles_count() {
        assert_eq!(
            bundle_stem(Path::new("/Applications/Visual Studio Code.app")).as_deref(),
            Some("Visual Studio Code")
        );
        assert_eq!(
            bundle_stem(Path::new("/Applications/Old.APP")).as_deref(),
            Some("Old")
        );
        assert_eq!(bundle_stem(Path::new("/Applications/.hidden.app")), None);
        assert_eq!(bundle_stem(Path::new("/Applications/.app")), None);
        assert_eq!(bundle_stem(Path::new("/Applications/Notes.txt")), None);
        assert_eq!(bundle_stem(Path::new("/Applications/Utilities")), None);
    }

    /// An app nested inside another (Simulator inside Xcode) must activate
    /// itself, not the outer app. A helper maps to its own bundle too, which
    /// is harmless: helpers are never Regular apps, so they are never focused.
    #[test]
    fn an_executable_maps_to_its_innermost_app_bundle() {
        assert_eq!(
            bundle_of_executable("/Applications/Slack.app/Contents/MacOS/Slack").as_deref(),
            Some("/Applications/Slack.app")
        );
        assert_eq!(
            bundle_of_executable(
                "/Applications/Xcode.app/Contents/Developer/Applications/Simulator.app/Contents/MacOS/Simulator"
            )
            .as_deref(),
            Some("/Applications/Xcode.app/Contents/Developer/Applications/Simulator.app")
        );
        assert_eq!(
            bundle_of_executable(
                "/Applications/Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"
            )
            .as_deref(),
            Some("/Applications/Chrome.app/Contents/Frameworks/Helper.app")
        );
        // An executable outside Contents/MacOS still names the bundle it is in.
        assert_eq!(
            bundle_of_executable("/Applications/Tool.app/Contents/Resources/bin/tool").as_deref(),
            Some("/Applications/Tool.app")
        );
        assert_eq!(bundle_of_executable("/usr/bin/python3"), None);
    }

    /// The first folder wins a duplicate name, plain files are ignored, and
    /// the result is sorted so repeated scans agree.
    #[test]
    fn scanning_lists_each_app_once_sorted_with_the_first_folder_winning() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::create_dir(first.path().join("Zed.app")).unwrap();
        std::fs::create_dir(first.path().join("safari.app")).unwrap();
        std::fs::write(first.path().join("Fake.app"), b"not a bundle").unwrap();
        std::fs::create_dir(second.path().join("Safari.app")).unwrap();
        std::fs::create_dir(second.path().join("Calculator.app")).unwrap();

        let dirs = vec![
            first.path().to_path_buf(),
            PathBuf::from("/definitely/not/here"),
            second.path().to_path_buf(),
        ];
        let apps = scan_app_dirs(&dirs, &[]);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Calculator", "safari", "Zed"]);
        let safari = apps.iter().find(|a| a.name == "safari").unwrap();
        assert!(Path::new(&safari.launch).starts_with(first.path()));
    }

    /// Vendors install one folder down ("Adobe Photoshop 2025/Adobe Photoshop
    /// 2025.app"), so that level is searched; deeper folders, hidden folders
    /// and the inside of a bundle are not, and a bundle at the top of a folder
    /// beats a same-named one in a vendor folder.
    #[test]
    fn scanning_finds_apps_in_vendor_folders_but_never_inside_bundles() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        std::fs::create_dir_all(path.join("Adobe Photoshop 2025/Adobe Photoshop 2025.app"))
            .unwrap();
        std::fs::create_dir_all(
            path.join("Xcode.app/Contents/Developer/Applications/Simulator.app"),
        )
        .unwrap();
        std::fs::create_dir_all(path.join("Suite/Tools/TooDeep.app")).unwrap();
        std::fs::create_dir_all(path.join(".hidden/Secret.app")).unwrap();
        std::fs::create_dir_all(path.join("Vendor/Zed.app")).unwrap();
        std::fs::create_dir(path.join("Zed.app")).unwrap();

        let apps = scan_app_dirs(&[path.to_path_buf()], &[]);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Adobe Photoshop 2025", "Xcode", "Zed"]);
        let zed = apps.iter().find(|a| a.name == "Zed").unwrap();
        assert_eq!(Path::new(&zed.launch), path.join("Zed.app"));
    }

    /// A symlinked folder could loop the scan, so it is not followed; a
    /// symlinked bundle is still an app.
    #[test]
    fn scanning_follows_linked_bundles_but_not_linked_folders() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(elsewhere.path().join("Linked.app")).unwrap();
        std::fs::create_dir_all(elsewhere.path().join("Folder/Behind.app")).unwrap();
        std::os::unix::fs::symlink(
            elsewhere.path().join("Linked.app"),
            root.path().join("Linked.app"),
        )
        .unwrap();
        std::os::unix::fs::symlink(elsewhere.path().join("Folder"), root.path().join("Folder"))
            .unwrap();
        // A folder linking to its own parent would recurse if it were followed.
        std::os::unix::fs::symlink(root.path(), root.path().join("Loop")).unwrap();

        let apps = scan_app_dirs(&[root.path().to_path_buf()], &[]);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Linked"]);
    }

    /// Finder lives outside the app folders and is added by name; an extra
    /// bundle that does not exist, or whose name a scanned app already took,
    /// adds nothing.
    #[test]
    fn extra_bundles_are_added_after_the_folders_when_they_exist() {
        let root = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Notes.app")).unwrap();
        std::fs::create_dir(extra.path().join("Finder.app")).unwrap();
        std::fs::create_dir(extra.path().join("Notes.app")).unwrap();
        let apps = scan_app_dirs(
            &[root.path().to_path_buf()],
            &[
                extra.path().join("Finder.app"),
                extra.path().join("Missing.app"),
                extra.path().join("Notes.app"),
            ],
        );
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Finder", "Notes"]);
        let notes = apps.iter().find(|a| a.name == "Notes").unwrap();
        assert!(Path::new(&notes.launch).starts_with(root.path()));
        assert!(EXTRA_BUNDLES.iter().all(|bundle| valid_bundle_path(bundle)));
    }

    /// ~/Applications is searched last, and only when there is a home.
    #[test]
    fn the_per_user_folder_comes_last_and_only_with_a_home() {
        let without = app_dirs(None);
        assert_eq!(without.len(), 4);
        assert_eq!(without[0], PathBuf::from("/Applications"));
        let with = app_dirs(Some(Path::new("/Users/me")));
        assert_eq!(with.last(), Some(&PathBuf::from("/Users/me/Applications")));
    }

    /// `open` must never receive something it reads as a flag or a relative
    /// name.
    #[test]
    fn only_absolute_app_bundle_paths_are_launched() {
        assert!(valid_bundle_path("/Applications/Safari.app"));
        assert!(valid_bundle_path("/Applications/Safari.app/"));
        assert!(!valid_bundle_path("Safari.app"));
        assert!(!valid_bundle_path("-a"));
        assert!(!valid_bundle_path("/usr/bin/python3"));
    }

    /// Letters, digits and punctuation use physical keycodes; named keys use
    /// enigo's own; Insert does not exist on a Mac.
    #[test]
    fn key_names_map_to_mac_keys() {
        assert_eq!(key_for("v"), Ok(Key::Other(0x09)));
        assert_eq!(key_for("T"), Ok(Key::Other(0x11)));
        assert_eq!(key_for("0"), Ok(Key::Other(0x1D)));
        assert_eq!(key_for("slash"), Ok(Key::Other(0x2C)));
        assert_eq!(key_for("enter"), Ok(Key::Return));
        assert_eq!(key_for("pagedown"), Ok(Key::PageDown));
        assert_eq!(key_for("f12"), Ok(Key::F12));
        assert!(key_for("insert").is_err());
        assert!(key_for("hyper").is_err());
    }

    /// Every key name the contract accepts has a mapping here, except Insert,
    /// so a tool never validates a combo this platform then cannot press.
    #[test]
    fn every_contract_key_name_and_alphanumeric_is_mapped() {
        for name in super::super::KEY_NAMES {
            if *name == "insert" {
                continue;
            }
            assert!(key_for(name).is_ok(), "{name}");
        }
        for c in ('a'..='z').chain('0'..='9') {
            assert!(key_for(&c.to_string()).is_ok(), "{c}");
        }
    }

    /// Meta is Command on a Mac; Ctrl stays the Control key.
    #[test]
    fn modifiers_map_to_mac_modifier_keys() {
        assert_eq!(modifier_key(Modifier::Meta), Key::Meta);
        assert_eq!(modifier_key(Modifier::Ctrl), Key::Control);
        assert_eq!(modifier_key(Modifier::Alt), Key::Alt);
        assert_eq!(modifier_key(Modifier::Shift), Key::Shift);
    }

    /// There is no Stop key on macOS, so Stop pauses instead of failing.
    #[test]
    fn stop_pauses_on_a_mac() {
        assert_eq!(media_key(MediaAction::Stop), Key::MediaPlayPause);
        assert_eq!(media_key(MediaAction::Next), Key::MediaNextTrack);
        assert_eq!(media_key(MediaAction::Previous), Key::MediaPrevTrack);
    }

    /// Zero steps still moves the volume, and a runaway count is capped.
    #[test]
    fn volume_steps_are_at_least_one_and_capped() {
        assert_eq!(volume_steps(0), 1);
        assert_eq!(volume_steps(3), 3);
        assert_eq!(volume_steps(200), MAX_VOLUME_STEPS);
    }

    /// The foreground entry uses the pid as its id, as `open_windows` does,
    /// and the window title that was read; without one it falls back to the
    /// app name, which never names a particular draft or tab.
    #[test]
    fn the_foreground_entry_carries_the_pid_and_the_real_title_when_read() {
        let entry = foreground_entry(42, "Google Chrome".into(), Some("Inbox - Gmail".into()));
        assert_eq!(entry.id, 42);
        assert_eq!(entry.title, "Inbox - Gmail");
        assert_eq!(entry.app, "Google Chrome");
        let entry = foreground_entry(42, "Google Chrome".into(), None);
        assert_eq!(entry.title, "Google Chrome");
        let entry = foreground_entry(42, "Safari".into(), Some("  ".into()));
        assert_eq!(entry.title, "Safari");
    }

    /// A queued keystroke either runs while its caller still waits or never:
    /// once the waiter gives up the job may not start, and once the job has
    /// started the waiter cannot give up on it.
    #[test]
    fn a_main_thread_job_and_its_waiter_agree_on_who_wins() {
        let state = AtomicU8::new(JOB_PENDING);
        assert!(abandon_job(&state));
        assert!(!start_job(&state), "an abandoned job must not run");

        let state = AtomicU8::new(JOB_PENDING);
        assert!(start_job(&state));
        assert!(!abandon_job(&state), "a running job cannot be abandoned");
        assert!(!start_job(&state), "a job runs once");
    }

    /// The frontmost app leads the list, the rest keep their order.
    #[test]
    fn the_frontmost_app_is_listed_first() {
        let mut entries = vec![window(1, "Finder"), window(2, "Safari"), window(3, "Mail")];
        front_first(&mut entries, Some(3));
        let ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![3, 1, 2]);
        front_first(&mut entries, Some(99));
        assert_eq!(
            entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
        front_first(&mut entries, None);
        assert_eq!(entries[0].id, 3);
    }
}
