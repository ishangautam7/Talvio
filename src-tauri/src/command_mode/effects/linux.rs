//! Command-mode effects on Linux, for X11 and Wayland.
//!
//! Linux has no single API for "which window is in front" or "launch this
//! app", so this module leans on the tools a desktop already has, the same way
//! the rest of the app does (`xdotool` in `clipboard.rs`, `wpctl` / `pactl` in
//! `audio_toolkit::ducking`): every tool is looked up on `PATH` first, run with
//! a deadline, and a missing tool is a sentence for the user, never a crash.
//!
//! - **Windows** (the foreground window, listing, switching) need X11:
//!   `wmctrl` when it is installed, `xdotool` otherwise. Wayland deliberately
//!   lets no client see or raise another client's windows, so there the reads
//!   come back empty and the actions say so. A [`WindowEntry::id`] is the X11
//!   window id as a number, whichever tool printed it (`xdotool` in decimal,
//!   `wmctrl` and `xprop` in hex), so the foreground window and a listed one
//!   compare equal. Panels, docks and the desktop are not windows a person
//!   switches to and are left out of the list (`_NET_WM_WINDOW_TYPE`, read
//!   with `xprop`).
//! - **Apps** come from `.desktop` files, the freedesktop.org registry every
//!   launcher reads, and are started by `gtk-launch` or `gio launch`, which
//!   honour the entry's `Exec=`, `Path=`, `Terminal=` and D-Bus activation.
//!   Only when neither exists is `Exec=` run here, split into an argument
//!   vector by hand and never through a shell: an `Exec=` line is data from a
//!   file anyone can drop into `~/.local/share/applications`.
//! - **Keys** go through the shared `Enigo` (XTest), which reaches X11 only.
//! - **Media** prefers `playerctl` (MPRIS over D-Bus, so it works on Wayland
//!   too) and falls back to media keys on X11.
//!
//! Every program started here has the AppImage's library directories scrubbed
//! from its environment (`helpers::appimage`), or a system tool can load a
//! bundled library built for another version and fail to start. Tools whose
//! output is parsed also get `LC_ALL=C`; launched apps keep the user's locale.

use std::collections::HashSet;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use enigo::{Direction, Key, Keyboard};
use log::{debug, info, warn};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use super::{
    is_known_key, AppEntry, Effects, KeyCombo, MediaAction, SystemEffects, VolumeAction,
    WindowEntry,
};
use crate::command_mode::registry::Modifier;

/// How long a tool whose answer is parsed (`xdotool`, `wmctrl`, `playerctl`,
/// `wpctl`, `pactl`) may take. They answer in milliseconds; one stuck on a
/// hung X server or D-Bus must not hold a command for longer than this.
const TOOL_DEADLINE: Duration = Duration::from_secs(2);

/// How long `gtk-launch` / `gio launch` may take to report whether they found
/// the app. Past this the launcher is assumed to be starting it and is left
/// running: it is never killed, since killing it could take the app with it.
const LAUNCHER_DEADLINE: Duration = Duration::from_secs(5);

/// How long typing may take on the main thread before the command gives up
/// waiting for it. The paste path itself waits for the clipboard and the
/// user's paste delay, which together stay far below this.
const TYPE_DEADLINE: Duration = Duration::from_secs(30);

/// How long the scanned list of installed apps is reused. Scanning reads a few
/// hundred small files, cheap but not free, and apps are rarely installed
/// between two commands; ten minutes means a new install shows up soon.
const APP_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// Directory depth scanned below each `applications` directory. Vendors nest
/// entries one level deep (`kde4/`, `wine/Programs/...`); deeper is unusual.
const MAX_SCAN_DEPTH: usize = 4;

/// Files looked at per `applications` directory, so a pathological directory
/// (a symlink farm, a network mount) cannot stall a command.
const MAX_SCAN_ENTRIES: usize = 5_000;

/// Windows listed by the `xdotool` fallback, which costs two processes per
/// window. `wmctrl` lists every window in one process and is not capped as
/// tightly.
const XDOTOOL_WINDOW_CAP: usize = 50;
const WMCTRL_WINDOW_CAP: usize = 200;

/// Recent files returned, most recent first.
const MAX_RECENT_FILES: usize = 200;

/// The percentage one volume "step" moves, like a desktop's volume keys.
const VOLUME_STEP_PERCENT: u32 = 5;
/// Steps taken at most for one "turn it up", so a misheard number cannot blast
/// the speakers from quiet to full.
const MAX_VOLUME_STEPS: u8 = 20;

const WAYLAND_NO_WINDOWS: &str =
    "Wayland does not let apps see or switch other apps' windows, so I can't do that here.";
const WAYLAND_NO_KEYS: &str =
    "Wayland does not let apps press keys in other apps, so I can't do that here.";

// ── The focused window ─────────────────────────────────────────────────────

/// The focused top-level window, on X11 with `xdotool` installed. `None` on
/// Wayland, without `xdotool`, or when one of our own windows is in front.
pub fn foreground_window() -> Option<WindowEntry> {
    if crate::utils::is_wayland() || !on_path("xdotool") {
        return None;
    }
    let id = active_window_id()?;
    let window = xdotool_window(id)?;
    if window.is_none() {
        // Ours: the overlay or the assistant panel can be in front while a
        // command's context is gathered, and it is not what the user means.
        debug!("command mode: the focused window is Talvio's own");
    }
    window
}

/// The X11 id of the active window, per `xdotool getactivewindow`.
fn active_window_id() -> Option<i64> {
    parse_x11_id(&tool_output("xdotool", &["getactivewindow"])?)
}

/// An X11 window id as printed by any of the tools: decimal (`xdotool`) or
/// `0x` hex (`wmctrl`, `xprop`). One parser for both, so the same window
/// gets the same [`WindowEntry::id`] however it was found.
fn parse_x11_id(text: &str) -> Option<i64> {
    let text = text.trim();
    let id = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok()?,
        None => text.parse().ok()?,
    };
    (id > 0).then_some(id)
}

/// One window's title and app through `xdotool`. The outer `None` means the
/// window could not be read; `Some(None)` means it belongs to this process.
fn xdotool_window(id: i64) -> Option<Option<WindowEntry>> {
    let id_arg = id.to_string();
    let title = tool_output("xdotool", &["getwindowname", &id_arg])?
        .trim_end_matches(['\n', '\r'])
        .to_string();
    // A window without `_NET_WM_PID` has no pid; its app then reads as "".
    let pid = tool_output("xdotool", &["getwindowpid", &id_arg])
        .and_then(|out| out.trim().parse::<u32>().ok());
    if pid == Some(std::process::id()) {
        return Some(None);
    }
    Some(Some(WindowEntry {
        id,
        title,
        app: pid.and_then(process_name).unwrap_or_default(),
    }))
}

/// A process's short name ("firefox", "code") from `/proc/<pid>/comm`.
fn process_name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let comm = comm.trim();
    (!comm.is_empty()).then(|| comm.to_string())
}

// ── Listing windows ────────────────────────────────────────────────────────

/// One line of `wmctrl -lp`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WmctrlWindow {
    id: i64,
    /// `-1` for a window shown on every desktop.
    desktop: i64,
    /// `0` when the window has no `_NET_WM_PID`.
    pid: u32,
    title: String,
}

/// Parse one line of `wmctrl -lp`: `0x03a00007  0 12345  host  Title text`.
///
/// The title is everything after the host, kept as written (it may contain
/// runs of spaces); a window with no title has nothing after the host.
fn parse_wmctrl_line(line: &str) -> Option<WmctrlWindow> {
    let (id, rest) = next_field(line)?;
    let (desktop, rest) = next_field(rest)?;
    let (pid, rest) = next_field(rest)?;
    // The host is required; the title after it is not.
    let (_host, rest) = next_field(rest).or_else(|| {
        let host = rest.trim();
        (!host.is_empty()).then_some((host, ""))
    })?;
    // wmctrl always prints hex; a bare number here is not a window line.
    if !id.starts_with("0x") && !id.starts_with("0X") {
        return None;
    }
    let id = parse_x11_id(id)?;
    Some(WmctrlWindow {
        id,
        desktop: desktop.parse().ok()?,
        pid: pid.parse().ok()?,
        title: rest.trim_end_matches(['\n', '\r']).to_string(),
    })
}

/// The first whitespace-separated field of `text` and what follows it, with
/// the separating whitespace removed. `None` when there is no field, or no
/// whitespace after it (so the field cannot be followed by another).
fn next_field(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace)?;
    let (field, rest) = text.split_at(end);
    Some((field, rest.trim_start()))
}

/// `wmctrl`'s windows, without our own, untitled ones, or shell surfaces.
///
/// Panels, docks and the desktop window are in `_NET_CLIENT_LIST` like any
/// app, so `wmctrl` lists them ("xfce4-panel", xfdesktop's "Desktop"), and a
/// fuzzy "switch to desktop" would then focus one. They are told apart by
/// their window type, which costs one `xprop` per window; only sticky windows
/// (desktop -1) are checked, since that is where every shell surface sits and
/// an ordinary window almost never does, so the common list costs nothing
/// extra.
fn wmctrl_windows() -> Vec<WindowEntry> {
    let Some(out) = tool_output("wmctrl", &["-lp"]) else {
        return Vec::new();
    };
    let own = std::process::id();
    let has_xprop = on_path("xprop");
    out.lines()
        .filter_map(parse_wmctrl_line)
        .filter(|w| w.pid != own && !w.title.trim().is_empty())
        .map(|w| {
            let app = if w.pid == 0 {
                String::new()
            } else {
                process_name(w.pid).unwrap_or_default()
            };
            (w, app)
        })
        .filter(|(w, app)| w.desktop != -1 || !is_shell_surface(w.id, app, has_xprop))
        .take(WMCTRL_WINDOW_CAP)
        .map(|(w, app)| WindowEntry {
            id: w.id,
            app,
            title: w.title,
        })
        .collect()
}

/// `xdotool`'s visible named windows, for when `wmctrl` is missing. Its
/// search also finds child windows that `windowactivate` cannot raise, so the
/// ids are kept to the window manager's own client list when `xprop` can read
/// it; and since `xdotool` says nothing of desktops, every window's type is
/// checked (one `xprop` each, within [`XDOTOOL_WINDOW_CAP`]).
fn xdotool_windows() -> Vec<WindowEntry> {
    let Some(out) = tool_output("xdotool", &["search", "--onlyvisible", "--name", "."]) else {
        return Vec::new();
    };
    let has_xprop = on_path("xprop");
    let clients = if has_xprop {
        tool_output("xprop", &["-root", "_NET_CLIENT_LIST"])
            .as_deref()
            .and_then(parse_client_list)
    } else {
        None
    };
    let mut seen = HashSet::new();
    out.lines()
        .filter_map(parse_x11_id)
        .filter(|id| seen.insert(*id))
        .filter(|id| match &clients {
            Some(clients) => clients.contains(id),
            None => true,
        })
        .take(XDOTOOL_WINDOW_CAP)
        .filter_map(|id| xdotool_window(id).flatten())
        .filter(|w| !w.title.trim().is_empty())
        .filter(|w| !is_shell_surface(w.id, &w.app, has_xprop))
        .collect()
}

/// `_NET_WM_WINDOW_TYPE`s (without that prefix) that belong to the desktop
/// shell or are transient popups, never a window a person switches to.
/// NORMAL, DIALOG and UTILITY (a tool palette) stay.
const SHELL_WINDOW_TYPES: &[&str] = &[
    "DESKTOP",
    "DOCK",
    "TOOLBAR",
    "MENU",
    "SPLASH",
    "NOTIFICATION",
    "DROPDOWN_MENU",
    "POPUP_MENU",
    "TOOLTIP",
    "COMBO",
    "DND",
];

/// Processes that draw panels, docks or the desktop, for when `xprop` cannot
/// say what a window is. Only names that never own an ordinary window: a file
/// manager that also draws the desktop (nautilus, caja, pcmanfm) is not here,
/// since dropping it would hide its real windows.
const SHELL_PROCESSES: &[&str] = &[
    "xfce4-panel",
    "xfdesktop",
    "xfce4-notifyd",
    "plank",
    "nautilus-desktop",
    "plasmashell",
    "gnome-shell",
    "lxpanel",
    "lxqt-panel",
    "mate-panel",
    "budgie-panel",
    "tint2",
    "polybar",
    "cairo-dock",
    "latte-dock",
    "docky",
    "conky",
];

/// Whether window `id` (owned by process `app`) is a panel, dock, desktop or
/// popup. Read from its window type when `xprop` is installed and answers,
/// else guessed from the owning process.
fn is_shell_surface(id: i64, app: &str, has_xprop: bool) -> bool {
    let types = if has_xprop {
        tool_output(
            "xprop",
            &["-id", &format!("0x{id:x}"), "_NET_WM_WINDOW_TYPE"],
        )
        .map(|out| parse_window_types(&out))
    } else {
        None
    };
    looks_like_shell_surface(types.as_deref(), app)
}

/// The decision behind [`is_shell_surface`]. `types` is `None` when the type
/// could not be read at all, and empty when the window has no type, which
/// EWMH says means NORMAL. The first type listed is the one that applies (the
/// list is in order of preference, and only EWMH's own types are kept by
/// [`parse_window_types`]).
fn looks_like_shell_surface(types: Option<&[String]>, app: &str) -> bool {
    match types {
        Some(types) => types
            .first()
            .is_some_and(|first| SHELL_WINDOW_TYPES.contains(&first.as_str())),
        None => {
            // `/proc/<pid>/comm` keeps only the first 15 bytes of a name.
            !app.is_empty()
                && SHELL_PROCESSES
                    .iter()
                    .any(|&name| name.get(..15).unwrap_or(name) == app)
        }
    }
}

/// The EWMH window types in `xprop -id <id> _NET_WM_WINDOW_TYPE`'s answer,
/// without the `_NET_WM_WINDOW_TYPE_` prefix: `_NET_WM_WINDOW_TYPE(ATOM) =
/// _NET_WM_WINDOW_TYPE_DOCK` gives `["DOCK"]`. Vendor types such as KDE's
/// `_KDE_NET_WM_WINDOW_TYPE_OVERRIDE` are skipped. Empty when the property is
/// not set (`_NET_WM_WINDOW_TYPE:  not found.`).
fn parse_window_types(out: &str) -> Vec<String> {
    let Some((_, values)) = out.split_once('=') else {
        return Vec::new();
    };
    values
        .split(',')
        .filter_map(|value| value.trim().strip_prefix("_NET_WM_WINDOW_TYPE_"))
        .map(str::to_string)
        .collect()
}

/// The ids in `xprop -root _NET_CLIENT_LIST`'s answer:
/// `_NET_CLIENT_LIST(WINDOW): window id # 0x1e00003, 0x2200001`. `None` when
/// the property is missing, so the caller does not filter by an empty list a
/// window manager without EWMH would give.
fn parse_client_list(out: &str) -> Option<HashSet<i64>> {
    let (_, ids) = out.split_once('#')?;
    let ids: HashSet<i64> = ids.split(',').filter_map(parse_x11_id).collect();
    (!ids.is_empty()).then_some(ids)
}

/// Put the active window first: neither tool lists windows front to back,
/// and the one in front is the likeliest to be meant.
fn active_first(windows: &mut [WindowEntry], active: Option<i64>) {
    if let Some(index) = active.and_then(|id| windows.iter().position(|w| w.id == id)) {
        windows[..=index].rotate_right(1);
    }
}

// ── Installed apps (.desktop files) ────────────────────────────────────────

/// An application found in a `.desktop` file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopApp {
    /// The desktop file id: the path below `applications/` with `/` turned
    /// into `-` ("firefox.desktop", "kde4-kate.desktop"). What `gtk-launch`
    /// takes and what [`AppEntry::launch`] carries.
    id: String,
    name: String,
    path: PathBuf,
    exec: Option<String>,
}

/// What a `.desktop` file's `[Desktop Entry]` group says, when it is an app
/// that should be offered: its untranslated `Name=` and its `Exec=`.
///
/// `None` for anything else: a `Link` or `Directory`, an entry hidden from
/// menus (`NoDisplay=true`, the helpers and handlers every desktop installs),
/// a deleted one (`Hidden=true`), or one with no name. Localized names
/// (`Name[de]=`) are ignored: the untranslated name is what fuzzy matching and
/// the other platforms use, and a localized one is a second, competing name.
/// Keys in other groups (`[Desktop Action new-window]` has its own `Name=`) do
/// not count.
fn parse_desktop_entry(content: &str) -> Option<(String, Option<String>)> {
    let mut in_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut kind = None;
    let mut hidden = false;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" => name = Some(unescape_value(value)),
            "Exec" => exec = Some(value.to_string()),
            "Type" => kind = Some(value.to_string()),
            "NoDisplay" | "Hidden" if value.eq_ignore_ascii_case("true") => hidden = true,
            _ => {}
        }
    }
    if hidden || kind.as_deref() != Some("Application") {
        return None;
    }
    let name = name.filter(|n| !n.trim().is_empty())?;
    Some((name.trim().to_string(), exec))
}

/// The desktop file id for a file at `relative` below an `applications`
/// directory: path separators become `-`, per the Desktop Entry spec.
fn desktop_file_id(relative: &Path) -> String {
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Every `applications` directory, most important first: the user's own,
/// then each system data directory, then Flatpak's exports (often already in
/// `XDG_DATA_DIRS`; duplicates are dropped). An empty or relative
/// `XDG_DATA_HOME` / `XDG_DATA_DIRS` entry is ignored, as the spec says.
fn application_dirs_from(
    data_home: Option<&str>,
    home: Option<&Path>,
    data_dirs: Option<&str>,
) -> Vec<PathBuf> {
    let data_home = data_home
        .filter(|d| Path::new(d).is_absolute())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let data_dirs: Vec<PathBuf> = data_dirs
        .map(|dirs| {
            dirs.split(':')
                .filter(|d| Path::new(d).is_absolute())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });

    let mut dirs = Vec::new();
    if let Some(data_home) = &data_home {
        dirs.push(data_home.join("applications"));
        dirs.push(data_home.join("flatpak/exports/share/applications"));
    }
    dirs.extend(data_dirs.iter().map(|d| d.join("applications")));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    let mut seen = HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    application_dirs_from(
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        home.as_deref(),
        std::env::var("XDG_DATA_DIRS").ok().as_deref(),
    )
}

/// The apps in `dirs`, in order of precedence. The first directory holding an
/// id decides it, even when that file hides the app: a user's copy with
/// `Hidden=true` or `NoDisplay=true` is how a system entry is removed from
/// menus, so it must remove it here too. Sorted by name for a stable list.
fn scan_desktop_dirs(dirs: &[PathBuf]) -> Vec<DesktopApp> {
    let mut claimed = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let mut files = Vec::new();
        collect_desktop_files(dir, dir, 0, &mut files);
        for (id, path) in files {
            if !claimed.insert(id.clone()) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some((name, exec)) = parse_desktop_entry(&content) {
                apps.push(DesktopApp {
                    id,
                    name,
                    path,
                    exec,
                });
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// `(desktop file id, path)` of every `*.desktop` file below `dir`, bounded by
/// [`MAX_SCAN_DEPTH`] and [`MAX_SCAN_ENTRIES`]. Symlinked directories are not
/// followed, so a loop cannot recurse forever; symlinked files are read.
fn collect_desktop_files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().take(MAX_SCAN_ENTRIES).collect();
    // A stable order, so which of two same-id files wins does not depend on
    // the filesystem's.
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= MAX_SCAN_ENTRIES {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth + 1 < MAX_SCAN_DEPTH {
                collect_desktop_files(root, &path, depth + 1, out);
            }
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(root) {
            out.push((desktop_file_id(relative), path.clone()));
        }
    }
}

static APP_CACHE: Mutex<Option<(Instant, Vec<DesktopApp>)>> = Mutex::new(None);

/// The installed apps, from the cache when it is younger than
/// [`APP_CACHE_TTL`] and `refresh` is not set.
fn desktop_apps(refresh: bool) -> Vec<DesktopApp> {
    let mut cache = APP_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !refresh {
        if let Some((at, apps)) = cache.as_ref() {
            if at.elapsed() < APP_CACHE_TTL {
                return apps.clone();
            }
        }
    }
    let started = Instant::now();
    let apps = scan_desktop_dirs(&application_dirs());
    debug!(
        "command mode: found {} installed apps in {:?}",
        apps.len(),
        started.elapsed()
    );
    *cache = Some((Instant::now(), apps.clone()));
    apps
}

// ── Exec= lines ────────────────────────────────────────────────────────────

/// The Desktop Entry spec's string escapes (`\s`, `\n`, `\t`, `\r`, `\\`),
/// which apply to every string value before anything else reads it.
fn unescape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Field codes `Exec=` may carry. None of them has a value when an app is
/// launched by name with no file: `%f`/`%F`/`%u`/`%U` are the files or URLs to
/// open, `%i` the icon, `%c` the name, `%k` the entry's path, and the rest are
/// deprecated and must be dropped.
const FIELD_CODES: &[char] = &[
    'f', 'F', 'u', 'U', 'i', 'c', 'k', 'd', 'D', 'n', 'N', 'v', 'm',
];

/// An `Exec=` value as an argument vector, with field codes removed, or `None`
/// when it is empty or has an unterminated quote.
///
/// Follows the spec's quoting: arguments are separated by spaces, an argument
/// may be wrapped in double quotes, and inside quotes a backslash escapes
/// `"`, `` ` ``, `$` and `\`. This is what lets the result be started directly
/// rather than through `sh -c`, which would run whatever shell syntax the file
/// held.
fn exec_argv(exec: &str) -> Option<Vec<String>> {
    let exec = unescape_value(exec);
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' => quoted = false,
                '\\' => match chars.next() {
                    Some(next @ ('"' | '`' | '$' | '\\')) => current.push(next),
                    Some(next) => {
                        current.push('\\');
                        current.push(next);
                    }
                    None => return None,
                },
                _ => current.push(c),
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                in_arg = true;
            }
            c if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            _ => {
                current.push(c);
                in_arg = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if in_arg {
        args.push(current);
    }
    let argv: Vec<String> = args
        .into_iter()
        .filter_map(|arg| strip_field_codes(&arg))
        .collect();
    (!argv.is_empty()).then_some(argv)
}

/// `arg` without its field codes, `%%` read as `%`. `None` when the argument
/// was only field codes (`%U`), so it disappears instead of becoming an empty
/// argument the app would read as a file name.
fn strip_field_codes(arg: &str) -> Option<String> {
    let mut out = String::with_capacity(arg.len());
    let mut had_code = false;
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some(code) if FIELD_CODES.contains(&code) => {
                chars.next();
                had_code = true;
            }
            _ => out.push('%'),
        }
    }
    if had_code && out.is_empty() {
        None
    } else {
        Some(out)
    }
}

// ── Launching ──────────────────────────────────────────────────────────────

/// What a launcher (`gtk-launch`, `gio launch`) reported.
enum Launched {
    Yes,
    No,
    /// Still running past [`LAUNCHER_DEADLINE`]; taken as starting the app.
    Pending,
}

/// A command for a program that is started, not parsed: the user's locale and
/// environment are kept (an app should start in the user's language), only the
/// AppImage's libraries are taken out. It gets its own process group, so a
/// Ctrl+C in the terminal Talvio was started from does not reach it.
fn launch_command(program: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(home) = std::env::var_os("HOME") {
        cmd.current_dir(home);
    }
    crate::helpers::appimage::scrub_appimage_library_path(&mut cmd);
    cmd
}

/// Reap `child` on a thread of its own, so a launched app that exits later
/// does not stay behind as a zombie. Nothing ever waits for it otherwise: an
/// app is not part of the command, and it must outlive Talvio.
fn reap_in_background(mut child: std::process::Child) {
    let _ = thread::Builder::new()
        .name("command-launch-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

fn run_launcher(program: &str, args: &[&str]) -> Launched {
    let mut child = match launch_command(program, args).spawn() {
        Ok(child) => child,
        Err(e) => {
            warn!("command mode: could not start {program}: {e}");
            return Launched::No;
        }
    };
    match wait_until(&mut child, Instant::now() + LAUNCHER_DEADLINE) {
        Some(status) if status.success() => Launched::Yes,
        Some(status) => {
            debug!("command mode: {program} exited with {status}");
            Launched::No
        }
        None => {
            reap_in_background(child);
            Launched::Pending
        }
    }
}

/// Start `argv` directly, without a shell, and leave it running.
fn spawn_detached(argv: &[String]) -> Result<(), String> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| "The app's launcher entry has no command.".to_string())?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let child = launch_command(program, &args)
        .spawn()
        .map_err(|e| format!("Could not start the app: {e}"))?;
    reap_in_background(child);
    Ok(())
}

fn launch_desktop_app(app: &DesktopApp) -> Result<(), String> {
    if on_path("gtk-launch") {
        match run_launcher("gtk-launch", &[&app.id]) {
            Launched::Yes | Launched::Pending => return Ok(()),
            Launched::No => debug!("command mode: gtk-launch could not start {}", app.id),
        }
    }
    if on_path("gio") {
        let path = app.path.to_string_lossy();
        match run_launcher("gio", &["launch", &path]) {
            Launched::Yes | Launched::Pending => return Ok(()),
            Launched::No => debug!("command mode: gio launch could not start {}", app.id),
        }
    }
    let argv = app
        .exec
        .as_deref()
        .and_then(exec_argv)
        .ok_or_else(|| format!("I couldn't work out how to start {}.", app.name))?;
    spawn_detached(&argv)
}

// ── Keys ───────────────────────────────────────────────────────────────────

/// The enigo key for a [`KeyCombo`] key name, `None` for a name it does not
/// accept. Punctuation is sent as the character, which XTest maps to whatever
/// key produces it on the user's layout.
fn enigo_key(name: &str) -> Option<Key> {
    let name = name.to_ascii_lowercase();
    if !is_known_key(&name) {
        return None;
    }
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(Key::Unicode(c));
    }
    Some(match name.as_str() {
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
        "minus" => Key::Unicode('-'),
        "equal" => Key::Unicode('='),
        "comma" => Key::Unicode(','),
        "period" => Key::Unicode('.'),
        "slash" => Key::Unicode('/'),
        "backslash" => Key::Unicode('\\'),
        "semicolon" => Key::Unicode(';'),
        "quote" => Key::Unicode('\''),
        "bracketleft" => Key::Unicode('['),
        "bracketright" => Key::Unicode(']'),
        "backquote" => Key::Unicode('`'),
        _ => return None,
    })
}

fn modifier_key(modifier: Modifier) -> Key {
    match modifier {
        Modifier::Ctrl => Key::Control,
        Modifier::Alt => Key::Alt,
        Modifier::Shift => Key::Shift,
        Modifier::Meta => Key::Meta,
    }
}

impl SystemEffects {
    /// Run `f` with the shared `Enigo`. It only exists once the main window
    /// has initialised it, so a command that arrives before that fails with a
    /// sentence instead of a panic.
    fn with_enigo<T>(
        &self,
        f: impl FnOnce(&mut enigo::Enigo) -> Result<T, String>,
    ) -> Result<T, String> {
        let state = self
            .app
            .try_state::<crate::input::EnigoState>()
            .ok_or_else(|| {
                "Keyboard control isn't ready yet. Try again in a moment.".to_string()
            })?;
        let mut enigo = state
            .0
            .lock()
            .map_err(|e| format!("Keyboard control is unavailable: {e}"))?;
        f(&mut enigo)
    }

    /// Click one key `times` times through XTest.
    fn click_key(&self, key: Key, times: u8) -> Result<(), String> {
        self.with_enigo(|enigo| {
            let _injected = handy_keys::ignore_injected_input();
            for _ in 0..times.max(1) {
                enigo
                    .key(key, Direction::Click)
                    .map_err(|e| format!("Could not press the key: {e}"))?;
            }
            Ok(())
        })
    }
}

// ── Volume ─────────────────────────────────────────────────────────────────

/// PulseAudio's 100%, as a raw volume (`PA_VOLUME_NORM`).
const PA_VOLUME_NORM: u32 = 0x10000;

/// The arguments for WirePlumber's `wpctl` (PipeWire), the first tool tried
/// to step the default output. Raising is capped at 100% by `wpctl` itself
/// (`-l 1.0`).
fn wpctl_step_args(up: bool, percent: u32) -> Vec<String> {
    let sign = if up { '+' } else { '-' };
    let mut args = vec!["set-volume".to_string()];
    if up {
        args.extend(["-l".to_string(), "1.0".to_string()]);
    }
    args.extend([
        "@DEFAULT_AUDIO_SINK@".to_string(),
        format!("{percent}%{sign}"),
    ]);
    args
}

/// What `pactl` (PulseAudio, or PipeWire's Pulse server) is asked to do for
/// one step, the fallback when `wpctl` is missing or fails.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PactlStep {
    /// Run `pactl` with these arguments.
    Run(Vec<String>),
    /// Already at (or above) 100%: raising would only amplify past it.
    AtFull,
    /// The current level could not be read, so raising is not attempted:
    /// `pactl` has no ceiling of its own.
    Unread,
}

/// The `pactl` step for `percent`, given the loudest channel's raw volume
/// (read only when raising). `pactl` happily goes past 100%, into digital
/// amplification that distorts, so a raise is cut to the headroom below 100%,
/// rounded down; it stays relative so the channels keep their balance.
/// Lowering needs no cap.
fn pactl_step(up: bool, percent: u32, loudest_raw: Option<u32>) -> PactlStep {
    let run = |change: String| {
        PactlStep::Run(vec![
            "set-sink-volume".to_string(),
            "@DEFAULT_SINK@".to_string(),
            change,
        ])
    };
    if !up {
        return run(format!("-{percent}%"));
    }
    let Some(loudest) = loudest_raw else {
        return PactlStep::Unread;
    };
    let headroom = PA_VOLUME_NORM.saturating_sub(loudest) * 100 / PA_VOLUME_NORM;
    if headroom == 0 {
        return PactlStep::AtFull;
    }
    run(format!("+{}%", percent.min(headroom)))
}

/// The loudest channel's raw volume in `pactl get-sink-volume`'s first line:
/// `Volume: front-left: 26214 /  40% / -23.88 dB,   front-right: ...`. The raw
/// value before each channel's first `/` is exact, where the percentage is
/// rounded (the same reading `audio_toolkit::ducking` makes).
fn pactl_loudest_raw(out: &str) -> Option<u32> {
    out.lines()
        .next()?
        .split(',')
        .filter_map(|channel| {
            channel
                .split('/')
                .next()?
                .split_whitespace()
                .last()?
                .parse::<u32>()
                .ok()
        })
        .max()
}

// ── Recent files (recently-used.xbel) ──────────────────────────────────────

/// The files in a `recently-used.xbel`, most recently modified first, each
/// once. Entries that are not `file://` URLs (a `recent://`, an `sftp://`) are
/// left out, and an entry with no readable time sorts last.
///
/// The format is GLib's bookmark file, written by GTK and KDE alike; reading it
/// by scanning `<bookmark ...>` tags avoids an XML dependency for one element.
fn parse_xbel(content: &str) -> Vec<PathBuf> {
    let mut found: Vec<(Option<i64>, usize, PathBuf)> = Vec::new();
    let mut rest = content;
    let mut order = 0;
    while let Some(start) = rest.find("<bookmark") {
        let after = &rest[start + "<bookmark".len()..];
        // `<bookmark:applications>` and the like are other elements.
        if !after.starts_with(|c: char| c.is_whitespace()) {
            rest = after;
            continue;
        }
        let Some(end) = after.find('>') else {
            break;
        };
        let tag = &after[..end];
        rest = &after[end..];

        let attrs = parse_attributes(tag);
        let attr = |name: &str| {
            attrs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        let Some(path) = attr("href").and_then(file_url_path) else {
            continue;
        };
        let time = ["modified", "visited", "added"]
            .iter()
            .find_map(|key| attr(key))
            .and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
            .map(|stamp| stamp.timestamp_micros());
        found.push((time, order, path));
        order += 1;
    }
    // Newest first; undated last, in file order.
    found.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => y.cmp(&x).then(a.1.cmp(&b.1)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.1.cmp(&b.1),
    });
    let mut seen = HashSet::new();
    found
        .into_iter()
        .map(|(_, _, path)| path)
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

/// `name="value"` pairs of a tag's attributes, values XML-unescaped. Single
/// and double quotes are both accepted; anything malformed ends the scan.
fn parse_attributes(tag: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = tag;
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else {
            break;
        };
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let value_start = &after[1..];
        let Some(close) = value_start.find(quote) else {
            break;
        };
        attrs.push((name, xml_unescape(&value_start[..close])));
        rest = &value_start[close + 1..];
    }
    attrs
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// The local path of a `file://` URL, percent-decoded.
fn file_url_path(href: &str) -> Option<PathBuf> {
    let url = reqwest::Url::parse(href).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

fn data_home() -> Option<PathBuf> {
    std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|d| Path::new(d).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

// ── Running tools ──────────────────────────────────────────────────────────

/// Whether `program` is an executable file on `PATH` (as `clipboard.rs`'s
/// private `linux_tool_on_path` checks).
fn on_path(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                std::fs::metadata(dir.join(program))
                    .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Wait for `child` until `deadline`. `None` when it is still running.
fn wait_until(child: &mut std::process::Child, deadline: Instant) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => return None,
        }
    }
}

/// Stdout of `program args` when it is on `PATH` and succeeds within
/// [`TOOL_DEADLINE`]. A tool that runs past it is killed and reaped.
fn tool_output(program: &str, args: &[&str]) -> Option<String> {
    if !on_path(program) {
        return None;
    }
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::helpers::appimage::scrub_appimage_library_path(&mut cmd);

    let deadline = Instant::now() + TOOL_DEADLINE;
    let mut child = cmd.spawn().ok()?;
    // Read on another thread, so a tool that writes more than a pipe holds
    // (a long window list) cannot stall on it.
    let stdout = child.stdout.take().map(|mut pipe| {
        let (sent, received) = mpsc::channel();
        thread::spawn(move || {
            let mut out = Vec::new();
            let _ = pipe.read_to_end(&mut out);
            let _ = sent.send(out);
        });
        received
    });
    let Some(status) = wait_until(&mut child, deadline) else {
        debug!("command mode: {program} ran past its deadline");
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    if !status.success() {
        return None;
    }
    let out = match stdout {
        Some(received) => received
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()?,
        None => Vec::new(),
    };
    Some(String::from_utf8_lossy(&out).into_owned())
}

fn tool_succeeds(program: &str, args: &[&str]) -> bool {
    tool_output(program, args).is_some()
}

// ── Main-thread jobs ───────────────────────────────────────────────────────

/// A queued main-thread job that has neither started nor been given up on.
const JOB_PENDING: u8 = 0;
/// The job started: its effect is under way and will finish.
const JOB_RUNNING: u8 = 1;
/// The waiter gave up first: the job must never act.
const JOB_ABANDONED: u8 = 2;

/// Called by a queued job before it acts: true when it may run. Exactly one
/// of this and [`abandon_job`] wins, so a paste either runs while its caller
/// still waits for it or never runs at all; a plain "abandoned" flag would
/// race with a job that starts just as the deadline passes.
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

// ── The trait ──────────────────────────────────────────────────────────────

impl Effects for SystemEffects {
    fn installed_apps(&self) -> Vec<AppEntry> {
        desktop_apps(false)
            .into_iter()
            .map(|app| AppEntry {
                name: app.name,
                launch: app.id,
            })
            .collect()
    }

    fn open_windows(&self) -> Vec<WindowEntry> {
        if crate::utils::is_wayland() {
            return Vec::new();
        }
        let mut windows = if on_path("wmctrl") {
            wmctrl_windows()
        } else if on_path("xdotool") {
            xdotool_windows()
        } else {
            debug!("command mode: neither wmctrl nor xdotool is installed; no window list");
            Vec::new()
        };
        if on_path("xdotool") {
            active_first(&mut windows, active_window_id());
        }
        windows
    }

    fn recent_files(&self) -> Vec<PathBuf> {
        let Some(path) = data_home().map(|d| d.join("recently-used.xbel")) else {
            return Vec::new();
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Vec::new();
        };
        parse_xbel(&content)
            .into_iter()
            .filter(|p| p.exists())
            .take(MAX_RECENT_FILES)
            .collect()
    }

    fn known_folders(&self) -> Vec<PathBuf> {
        let paths = self.app.path();
        let mut seen = HashSet::new();
        [
            paths.document_dir(),
            paths.download_dir(),
            paths.desktop_dir(),
            paths.home_dir(),
        ]
        .into_iter()
        .filter_map(Result::ok)
        .filter(|dir| dir.is_dir() && seen.insert(dir.clone()))
        .collect()
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("Could not open the link: {e}"))
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| format!("Could not open {}: {e}", path.display()))
    }

    fn launch_app(&self, app: &AppEntry) -> Result<(), String> {
        let found = desktop_apps(false)
            .into_iter()
            .find(|candidate| candidate.id == app.launch)
            // Installed since the cache was filled.
            .or_else(|| {
                desktop_apps(true)
                    .into_iter()
                    .find(|candidate| candidate.id == app.launch)
            });
        info!("command mode: launching {}", app.launch);
        match found {
            Some(desktop) => launch_desktop_app(&desktop),
            // Not in any directory scanned here, but gtk-launch searches the
            // same places and may still know it.
            None if on_path("gtk-launch") => match run_launcher("gtk-launch", &[&app.launch]) {
                Launched::Yes | Launched::Pending => Ok(()),
                Launched::No => Err(format!(
                    "{} doesn't seem to be installed any more.",
                    app.name
                )),
            },
            None => Err(format!(
                "{} doesn't seem to be installed any more.",
                app.name
            )),
        }
    }

    fn focus_window(&self, window: &WindowEntry) -> Result<(), String> {
        if crate::utils::is_wayland() {
            return Err(WAYLAND_NO_WINDOWS.to_string());
        }
        if on_path("wmctrl") {
            let id = format!("0x{:08x}", window.id);
            if tool_succeeds("wmctrl", &["-ia", &id]) {
                return Ok(());
            }
            debug!("command mode: wmctrl could not activate window {id}");
        }
        if on_path("xdotool") {
            if tool_succeeds("xdotool", &["windowactivate", &window.id.to_string()]) {
                return Ok(());
            }
            return Err(format!("Could not switch to \"{}\".", window.title));
        }
        Err("Switching windows needs wmctrl or xdotool, and neither is installed.".to_string())
    }

    fn type_text(&self, text: &str) -> Result<(), String> {
        // The paste path writes the clipboard and sends the paste keys, and
        // the dictation pipeline runs it on the main thread; so does this. It
        // already picks a Wayland typing tool (wtype, dotool, ydotool) itself.
        //
        // When the deadline passes before the paste has started, the queued
        // paste is cancelled rather than left to run later into whatever
        // window is in front by then (see [`start_job`]).
        let (sent, received) = mpsc::channel();
        let app = self.app.clone();
        let text = text.to_string();
        let state = Arc::new(AtomicU8::new(JOB_PENDING));
        let job_state = Arc::clone(&state);
        self.app
            .run_on_main_thread(move || {
                if !start_job(&job_state) {
                    debug!("command mode: dropped a paste its caller gave up on");
                    return;
                }
                let result = crate::clipboard::paste_with_behavior(
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
                );
                let _ = sent.send(result);
            })
            .map_err(|e| format!("Could not type the text: {e}"))?;
        match received.recv_timeout(TYPE_DEADLINE) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) if abandon_job(&state) => {
                warn!("command mode: the main thread stayed busy, so the paste was cancelled");
                Err("Typing the text took too long, so nothing was typed.".to_string())
            }
            // The paste started just before the deadline: it is happening, so
            // wait for its real result instead of reporting a failure.
            Err(mpsc::RecvTimeoutError::Timeout) => received
                .recv_timeout(TYPE_DEADLINE)
                .map_err(|_| "Typing the text took too long.".to_string())?,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Could not type the text.".to_string())
            }
        }
    }

    fn press_keys(&self, combo: &KeyCombo) -> Result<(), String> {
        if crate::utils::is_wayland() {
            return Err(WAYLAND_NO_KEYS.to_string());
        }
        let key = enigo_key(&combo.key)
            .ok_or_else(|| format!("\"{}\" isn't a key I can press.", combo.key))?;
        self.with_enigo(|enigo| {
            let _injected = handy_keys::ignore_injected_input();
            let mut pressed = Vec::new();
            let mut result = Ok(());
            for modifier in &combo.modifiers {
                let modifier = modifier_key(*modifier);
                match enigo.key(modifier, Direction::Press) {
                    Ok(()) => pressed.push(modifier),
                    Err(e) => {
                        result = Err(format!("Could not press {}: {e}", combo.label()));
                        break;
                    }
                }
            }
            if result.is_ok() {
                result = enigo
                    .key(key, Direction::Click)
                    .map_err(|e| format!("Could not press {}: {e}", combo.label()));
            }
            // Every modifier that went down comes back up, whatever failed,
            // or it stays held at the OS level.
            for modifier in pressed.into_iter().rev() {
                let _ = enigo.key(modifier, Direction::Release);
            }
            crate::input::release_all_modifiers(enigo);
            result
        })
    }

    fn media(&self, action: MediaAction) -> Result<(), String> {
        let verb = match action {
            MediaAction::PlayPause => "play-pause",
            MediaAction::Next => "next",
            MediaAction::Previous => "previous",
            MediaAction::Stop => "stop",
        };
        let has_playerctl = on_path("playerctl");
        if has_playerctl {
            if tool_succeeds("playerctl", &[verb]) {
                return Ok(());
            }
            // Usually "No players found"; on X11 the desktop may still act on
            // a media key, so try that before giving up.
            debug!("command mode: playerctl {verb} failed");
        }
        if crate::utils::is_wayland() {
            return Err(if has_playerctl {
                "No media player is running.".to_string()
            } else {
                "Controlling media on Wayland needs playerctl, which isn't installed.".to_string()
            });
        }
        let key = match action {
            MediaAction::PlayPause => Key::MediaPlayPause,
            MediaAction::Next => Key::MediaNextTrack,
            MediaAction::Previous => Key::MediaPrevTrack,
            MediaAction::Stop => Key::MediaStop,
        };
        self.click_key(key, 1)
    }

    fn volume(&self, action: VolumeAction) -> Result<(), String> {
        match action {
            VolumeAction::Up(steps) | VolumeAction::Down(steps) => {
                let up = matches!(action, VolumeAction::Up(_));
                let steps = steps.clamp(1, MAX_VOLUME_STEPS);
                let percent = u32::from(steps) * VOLUME_STEP_PERCENT;
                if on_path("wpctl") {
                    let args = wpctl_step_args(up, percent);
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    if tool_succeeds("wpctl", &args) {
                        return Ok(());
                    }
                    debug!("command mode: wpctl could not change the volume");
                }
                if on_path("pactl") {
                    let loudest = if up {
                        tool_output("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])
                            .as_deref()
                            .and_then(pactl_loudest_raw)
                    } else {
                        None
                    };
                    match pactl_step(up, percent, loudest) {
                        PactlStep::AtFull => return Ok(()),
                        PactlStep::Run(args) => {
                            let args: Vec<&str> = args.iter().map(String::as_str).collect();
                            if tool_succeeds("pactl", &args) {
                                return Ok(());
                            }
                            debug!("command mode: pactl could not change the volume");
                        }
                        // The volume keys below are handled by the desktop,
                        // which stops at 100% unless the user allowed more.
                        PactlStep::Unread => {
                            debug!("command mode: pactl could not read the volume to raise it")
                        }
                    }
                }
                if crate::utils::is_wayland() {
                    return Err(
                        "Changing the volume needs wpctl or pactl, and neither worked.".to_string(),
                    );
                }
                let key = if up { Key::VolumeUp } else { Key::VolumeDown };
                self.click_key(key, steps)
            }
            VolumeAction::Mute => crate::audio_toolkit::ducking::set_output_muted(true),
            VolumeAction::Unmute => crate::audio_toolkit::ducking::set_output_muted(false),
            VolumeAction::Set(level) => {
                let level = level.min(100);
                crate::audio_toolkit::ducking::set_output_volume(f32::from(level) / 100.0)?;
                // "Set the volume to 40" on a muted output means "let me hear
                // it at 40"; leaving it muted would look like nothing happened.
                if level > 0 {
                    if let Err(e) = crate::audio_toolkit::ducking::set_output_muted(false) {
                        debug!("command mode: could not unmute after setting the volume: {e}");
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// The common case: a plain application entry gives its name and command.
    #[test]
    fn an_application_entry_gives_its_name_and_command() {
        let content = "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Firefox".to_string(), Some("firefox %u".to_string())))
        );
    }

    /// A localized name is a second, competing name; only `Name=` counts,
    /// wherever it sits relative to the translations.
    #[test]
    fn localized_names_are_ignored() {
        let content = "[Desktop Entry]\nName[de]=Dateien\nName[fr]=Fichiers\nName=Files\n\
                       Type=Application\nExec=nautilus\n";
        assert_eq!(
            parse_desktop_entry(content).map(|e| e.0),
            Some("Files".into())
        );
        let only_localized = "[Desktop Entry]\nType=Application\nName[de]=Dateien\nExec=x\n";
        assert_eq!(parse_desktop_entry(only_localized), None);
    }

    /// Helpers and handlers every desktop installs are hidden from menus, and
    /// a deleted entry is hidden for good: neither should be launchable by name.
    #[test]
    fn hidden_and_no_display_entries_are_skipped() {
        let no_display = "[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\n";
        let hidden = "[Desktop Entry]\nType=Application\nName=Gone\nHidden=True\n";
        let shown = "[Desktop Entry]\nType=Application\nName=Shown\nNoDisplay=false\n";
        assert_eq!(parse_desktop_entry(no_display), None);
        assert_eq!(parse_desktop_entry(hidden), None);
        assert!(parse_desktop_entry(shown).is_some());
    }

    /// Links and directories are not apps, and an entry with no type at all is
    /// malformed.
    #[test]
    fn only_application_entries_count() {
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nType=Link\nName=Site\nURL=https://x\n"),
            None
        );
        assert_eq!(parse_desktop_entry("[Desktop Entry]\nName=Untyped\n"), None);
    }

    /// `[Desktop Action ...]` groups have their own `Name=`; it must not
    /// replace the app's, and their `NoDisplay` must not hide it.
    #[test]
    fn keys_outside_the_desktop_entry_group_are_ignored() {
        let content = "# comment\n[Desktop Entry]\nType=Application\nName=Firefox\n\
                       Exec=firefox\n\n[Desktop Action new-window]\nName=New Window\n\
                       Exec=firefox --new-window\nNoDisplay=true\n";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Firefox".to_string(), Some("firefox".to_string())))
        );
    }

    /// Ids follow the spec, so the one handed to `gtk-launch` is the one it
    /// knows the entry by.
    #[test]
    fn a_nested_entry_gets_a_dashed_desktop_file_id() {
        assert_eq!(
            desktop_file_id(Path::new("firefox.desktop")),
            "firefox.desktop"
        );
        assert_eq!(
            desktop_file_id(Path::new("kde4/kate.desktop")),
            "kde4-kate.desktop"
        );
    }

    /// The user's directory comes first so it can override a system entry,
    /// and the spec's defaults apply when the variables are unset or invalid.
    #[test]
    fn application_dirs_put_the_users_own_first_and_fall_back_to_the_defaults() {
        let dirs = application_dirs_from(None, Some(Path::new("/home/u")), None);
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/u/.local/share/applications"),
                PathBuf::from("/home/u/.local/share/flatpak/exports/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("/usr/share/applications"),
                PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            ]
        );
        let dirs = application_dirs_from(
            Some("relative/ignored"),
            Some(Path::new("/home/u")),
            Some("/opt/share::/var/lib/flatpak/exports/share:bad"),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/u/.local/share/applications"),
                PathBuf::from("/home/u/.local/share/flatpak/exports/share/applications"),
                PathBuf::from("/opt/share/applications"),
                PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            ]
        );
    }

    /// A user's `Hidden=true` copy is how a system entry is removed from
    /// menus; it must remove it here too, and nested entries are found.
    #[test]
    fn a_users_hidden_copy_removes_the_system_entry() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        std::fs::write(
            user.path().join("vlc.desktop"),
            "[Desktop Entry]\nType=Application\nName=VLC\nHidden=true\n",
        )
        .unwrap();
        std::fs::write(
            system.path().join("vlc.desktop"),
            "[Desktop Entry]\nType=Application\nName=VLC\nExec=vlc\n",
        )
        .unwrap();
        std::fs::create_dir(system.path().join("kde4")).unwrap();
        std::fs::write(
            system.path().join("kde4/kate.desktop"),
            "[Desktop Entry]\nType=Application\nName=Kate\nExec=kate %U\n",
        )
        .unwrap();
        std::fs::write(system.path().join("notes.txt"), "not an entry").unwrap();

        let apps = scan_desktop_dirs(&[user.path().to_path_buf(), system.path().to_path_buf()]);
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["kde4-kate.desktop"]);
        assert_eq!(apps[0].name, "Kate");
    }

    /// The usual `wmctrl -lp` shape, with the title kept exactly as written.
    #[test]
    fn a_wmctrl_line_gives_id_desktop_pid_and_title() {
        let line = "0x03a00007  0 12345  laptop Inbox -  Mail  (2)";
        assert_eq!(
            parse_wmctrl_line(line),
            Some(WmctrlWindow {
                id: 0x03a0_0007,
                desktop: 0,
                pid: 12345,
                title: "Inbox -  Mail  (2)".to_string(),
            })
        );
    }

    /// Sticky windows report desktop -1, windows without `_NET_WM_PID` report
    /// pid 0, and an untitled window has nothing after the host.
    #[test]
    fn wmctrl_lines_with_sticky_untitled_or_pidless_windows_still_parse() {
        let sticky = parse_wmctrl_line("0x01e00003 -1 0      laptop Desktop").unwrap();
        assert_eq!((sticky.desktop, sticky.pid), (-1, 0));
        let untitled = parse_wmctrl_line("0x02200001  1 4242   laptop").unwrap();
        assert_eq!(untitled.title, "");
        let trailing = parse_wmctrl_line("0x02200001  1 4242   laptop \n").unwrap();
        assert_eq!(trailing.title, "");
    }

    /// Anything that is not a window line (an error message, a blank line)
    /// is skipped rather than turned into a bogus window.
    #[test]
    fn malformed_wmctrl_lines_are_rejected() {
        assert_eq!(parse_wmctrl_line(""), None);
        assert_eq!(parse_wmctrl_line("Cannot open display."), None);
        assert_eq!(parse_wmctrl_line("12345 0 1 host title"), None);
        assert_eq!(parse_wmctrl_line("0xzz 0 1 host title"), None);
    }

    /// Field codes have no value when an app is launched by name; an argument
    /// made only of them disappears instead of becoming an empty file name.
    #[test]
    fn exec_field_codes_are_stripped() {
        assert_eq!(exec_argv("firefox %u"), Some(vec!["firefox".to_string()]));
        assert_eq!(
            exec_argv("app %F --flag %i %c %k"),
            Some(vec!["app".to_string(), "--flag".to_string()])
        );
        assert_eq!(
            exec_argv("app --file=%f 100%%"),
            Some(vec![
                "app".to_string(),
                "--file=".to_string(),
                "100%".to_string()
            ])
        );
    }

    /// Quoted arguments keep their spaces and escapes, so the command can be
    /// started directly and never through a shell.
    #[test]
    fn exec_quotes_and_escapes_split_like_the_spec() {
        assert_eq!(
            exec_argv(r#""/opt/My App/bin/app" --name "say \"hi\"" plain"#),
            Some(vec![
                "/opt/My App/bin/app".to_string(),
                "--name".to_string(),
                "say \"hi\"".to_string(),
                "plain".to_string(),
            ])
        );
        // `\\\\` in the file is `\\` after the string escapes, `\` after quoting.
        assert_eq!(
            exec_argv(r#"app "C:\\\\dir""#),
            Some(vec!["app".to_string(), "C:\\dir".to_string()])
        );
        // `\s` is a space at the string level, which then separates arguments.
        assert_eq!(
            exec_argv(r"app\s--x"),
            Some(vec!["app".to_string(), "--x".to_string()])
        );
        // Shell syntax is just text: no expansion, no command separator.
        assert_eq!(
            exec_argv("app ; rm -rf ~"),
            Some(vec![
                "app".to_string(),
                ";".to_string(),
                "rm".to_string(),
                "-rf".to_string(),
                "~".to_string(),
            ])
        );
    }

    /// A broken or empty `Exec=` starts nothing.
    #[test]
    fn an_unterminated_quote_or_empty_exec_gives_nothing() {
        assert_eq!(exec_argv(r#"app "unterminated"#), None);
        assert_eq!(exec_argv("   "), None);
        assert_eq!(exec_argv("%U"), None);
    }

    /// Newest first by `modified`, percent-encoding and XML entities decoded,
    /// non-file URLs and duplicates dropped, undated entries last.
    #[test]
    fn xbel_bookmarks_are_listed_newest_first() {
        let xbel = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/u/old.txt" added="2024-01-01T10:00:00Z" modified="2024-01-01T10:00:00.000000Z" visited="2024-01-01T10:00:00Z">
    <info><metadata owner="http://freedesktop.org"><bookmark:applications>
      <bookmark:application name="gedit" exec="&apos;gedit %u&apos;" modified="2030-01-01T00:00:00Z" count="1"/>
    </bookmark:applications></metadata></info>
  </bookmark>
  <bookmark href="file:///home/u/My%20Report%20&amp;%20Notes.pdf" added="2024-03-01T09:00:00Z" modified="2024-03-02T09:00:00.123456Z" visited="2024-03-01T09:00:00Z"/>
  <bookmark href="sftp://host/remote.txt" modified="2025-01-01T00:00:00Z"/>
  <bookmark href="file:///home/u/undated.txt"/>
  <bookmark href="file:///home/u/old.txt" modified="2023-01-01T00:00:00Z"/>
</xbel>"#;
        assert_eq!(
            parse_xbel(xbel),
            vec![
                PathBuf::from("/home/u/My Report & Notes.pdf"),
                PathBuf::from("/home/u/old.txt"),
                PathBuf::from("/home/u/undated.txt"),
            ]
        );
    }

    /// An unreadable file yields nothing rather than an error.
    #[test]
    fn an_empty_or_garbled_xbel_lists_nothing() {
        assert!(parse_xbel("").is_empty());
        assert!(parse_xbel("<xbel><bookmark href=").is_empty());
    }

    /// Every name a combo may use maps to a key, and nothing else does.
    #[test]
    fn every_known_key_name_maps_to_an_enigo_key() {
        for name in super::super::KEY_NAMES {
            assert!(enigo_key(name).is_some(), "{name} has no key");
        }
        assert_eq!(enigo_key("Enter"), Some(Key::Return));
        assert_eq!(enigo_key("t"), Some(Key::Unicode('t')));
        assert_eq!(enigo_key("T"), Some(Key::Unicode('t')));
        assert_eq!(enigo_key("f5"), Some(Key::F5));
        assert_eq!(enigo_key("bracketleft"), Some(Key::Unicode('[')));
        assert_eq!(enigo_key("hyper"), None);
        assert_eq!(enigo_key("ctrl"), None);
    }

    /// The window in front leads the list; the rest keep their order.
    #[test]
    fn the_active_window_is_moved_to_the_front() {
        let window = |id: i64| WindowEntry {
            id,
            title: format!("w{id}"),
            app: "app".into(),
        };
        let mut windows = vec![window(1), window(2), window(3)];
        active_first(&mut windows, Some(3));
        assert_eq!(
            windows.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
        active_first(&mut windows, Some(99));
        assert_eq!(
            windows.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
        active_first(&mut windows, None);
        assert_eq!(
            windows.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
    }

    /// Raising the volume is capped at 100% by wpctl itself; lowering needs
    /// no cap.
    #[test]
    fn wpctl_steps_have_the_right_signs_and_a_ceiling_when_raising() {
        assert_eq!(
            wpctl_step_args(true, 10),
            ["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "10%+"]
        );
        assert_eq!(
            wpctl_step_args(false, 5),
            ["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"]
        );
    }

    /// pactl has no ceiling, so a raise is cut to the headroom below 100%
    /// (80% + 50% lands at 100%, not 130%), nothing is raised at or past
    /// 100%, and nothing is raised when the level could not be read. A
    /// lowering is sent as asked.
    #[test]
    fn pactl_raises_never_go_past_full_volume() {
        let run = |change: &str| {
            PactlStep::Run(vec![
                "set-sink-volume".to_string(),
                "@DEFAULT_SINK@".to_string(),
                change.to_string(),
            ])
        };
        let percent_raw = |percent: u32| PA_VOLUME_NORM * percent / 100;
        assert_eq!(pactl_step(true, 10, Some(percent_raw(40))), run("+10%"));
        assert_eq!(pactl_step(true, 50, Some(percent_raw(80))), run("+20%"));
        // 99.5% leaves less than one whole percent: rounding down keeps it
        // at or under 100%.
        assert_eq!(pactl_step(true, 5, Some(65_200)), PactlStep::AtFull);
        assert_eq!(pactl_step(true, 5, Some(PA_VOLUME_NORM)), PactlStep::AtFull);
        assert_eq!(
            pactl_step(true, 5, Some(percent_raw(150))),
            PactlStep::AtFull
        );
        assert_eq!(pactl_step(true, 5, None), PactlStep::Unread);
        assert_eq!(pactl_step(false, 100, None), run("-100%"));
        assert_eq!(pactl_step(false, 5, Some(percent_raw(150))), run("-5%"));
    }

    /// The loudest channel decides the headroom, so an unbalanced output
    /// never has its louder side pushed past 100%.
    #[test]
    fn the_loudest_pactl_channel_is_read_from_the_raw_values() {
        let out = "Volume: front-left: 26214 /  40% / -23.88 dB,   front-right: 52429 /  80% / -5.81 dB\n        balance 0.50\n";
        assert_eq!(pactl_loudest_raw(out), Some(52_429));
        let mono = "Volume: mono: 65536 / 100% / 0.00 dB\n";
        assert_eq!(pactl_loudest_raw(mono), Some(65_536));
        assert_eq!(pactl_loudest_raw(""), None);
        assert_eq!(pactl_loudest_raw("Failed to get sink volume"), None);
    }

    /// xdotool prints ids in decimal and wmctrl in hex; both must give the
    /// same number, or the window in front never matches its listed entry.
    #[test]
    fn x11_ids_parse_the_same_from_decimal_and_hex() {
        assert_eq!(parse_x11_id("60817415\n"), Some(0x03a0_0007));
        assert_eq!(parse_x11_id("0x03a00007"), Some(0x03a0_0007));
        assert_eq!(
            parse_wmctrl_line("0x03a00007  0 12345  laptop Mail").map(|w| w.id),
            parse_x11_id("60817415")
        );
        assert_eq!(parse_x11_id("0"), None);
        assert_eq!(parse_x11_id("not a window"), None);
    }

    /// Panels, docks and the desktop are dropped by their window type; a
    /// window with no type is NORMAL, and dialogs and tool palettes stay.
    #[test]
    fn shell_surfaces_are_recognised_by_window_type() {
        let dock = parse_window_types("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DOCK\n");
        assert_eq!(dock, ["DOCK"]);
        assert!(looks_like_shell_surface(Some(dock.as_slice()), "anything"));
        let desktop =
            parse_window_types("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DESKTOP\n");
        assert!(looks_like_shell_surface(
            Some(desktop.as_slice()),
            "nautilus"
        ));
        // KDE puts its own type first; the first EWMH type is what counts.
        let kde = parse_window_types(
            "_NET_WM_WINDOW_TYPE(ATOM) = _KDE_NET_WM_WINDOW_TYPE_OVERRIDE, _NET_WM_WINDOW_TYPE_NORMAL\n",
        );
        assert_eq!(kde, ["NORMAL"]);
        assert!(!looks_like_shell_surface(
            Some(kde.as_slice()),
            "plasmashell"
        ));
        let untyped = parse_window_types("_NET_WM_WINDOW_TYPE:  not found.\n");
        assert!(untyped.is_empty());
        assert!(!looks_like_shell_surface(
            Some(untyped.as_slice()),
            "xfce4-panel"
        ));
        for kind in ["DIALOG", "UTILITY"] {
            let line = format!("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_{kind}");
            assert!(!looks_like_shell_surface(
                Some(parse_window_types(&line).as_slice()),
                ""
            ));
        }
    }

    /// Without xprop, only processes that never own an ordinary window are
    /// taken for shell surfaces, matched as `/proc/<pid>/comm` truncates them.
    #[test]
    fn without_a_window_type_shell_surfaces_are_guessed_from_the_process() {
        assert!(looks_like_shell_surface(None, "xfce4-panel"));
        assert!(looks_like_shell_surface(None, "plank"));
        // "nautilus-desktop" is 16 bytes; comm keeps 15.
        assert!(looks_like_shell_surface(None, "nautilus-deskto"));
        assert!(!looks_like_shell_surface(None, "nautilus"));
        assert!(!looks_like_shell_surface(None, "firefox"));
        assert!(!looks_like_shell_surface(None, ""));
    }

    /// The client list limits the xdotool fallback to real top-level
    /// windows; a missing list filters nothing.
    #[test]
    fn the_client_list_parses_to_window_ids() {
        let out = "_NET_CLIENT_LIST(WINDOW): window id # 0x1e00003, 0x2200001\n";
        let ids = parse_client_list(out).unwrap();
        assert!(ids.contains(&0x01e0_0003) && ids.contains(&0x0220_0001));
        assert_eq!(ids.len(), 2);
        assert_eq!(parse_client_list("_NET_CLIENT_LIST:  not found.\n"), None);
        assert_eq!(
            parse_client_list("_NET_CLIENT_LIST(WINDOW): window id # \n"),
            None
        );
    }

    /// A queued paste either runs while its caller still waits or never:
    /// once the waiter gives up the job may not start, and once it has
    /// started the waiter cannot give up on it.
    #[test]
    fn a_main_thread_job_and_its_waiter_agree_on_who_wins() {
        let state = AtomicU8::new(JOB_PENDING);
        assert!(abandon_job(&state));
        assert!(!start_job(&state), "an abandoned paste must not run");

        let state = AtomicU8::new(JOB_PENDING);
        assert!(start_job(&state));
        assert!(!abandon_job(&state), "a running paste cannot be abandoned");
        assert!(!start_job(&state), "a paste runs once");
    }
}
