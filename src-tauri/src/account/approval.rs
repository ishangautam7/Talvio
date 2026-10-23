//! The question a phone's first request asks on this computer: may it dictate
//! and type here? (`server/PROTOCOL.md`, "Approval"; the gate itself is
//! `remote::Router::admit`, which calls [`ask`] and [`close`] through `Host`.)
//!
//! A small always-on-top card in the corner, built like the reminder popup and
//! for the same reasons: it has to reach someone whose main window is hidden in
//! the tray, and it must never take the keyboard from the app they are in,
//! because Allow lets a request paste at their cursor straight away. So it
//! never arrives focused, and where the platform allows it a click cannot focus
//! it either: `WS_EX_NOACTIVATE` on Windows, a layer surface without keyboard on
//! Wayland, no input focus on X11. On macOS a non-activating window makes
//! WebView buttons unreliable (see `reminders.rs`), so there a click on either
//! button hands activation back to the app that had it ([`hand_back_focus`]),
//! so the next paste lands there. Several phones asking at once stack in one
//! window, below a reminder popup that shares the corner. All window work runs
//! on the main thread.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use log::{debug, error, warn};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindowBuilder};

use super::remote::AccountPhone;

pub const POPUP_LABEL: &str = "account_approval";
/// The phones being asked about, sent to the popup whenever they change.
const CHANGED_EVENT: &str = "account-approval-changed";
const WIDTH: f64 = 360.0;
const FALLBACK_HEIGHT: f64 = 160.0;
const MIN_HEIGHT: f64 = 72.0;
const MAX_HEIGHT: f64 = 720.0;
/// Between this card and a reminder above it.
const GAP: f64 = 8.0;
#[cfg(target_os = "linux")]
const MARGIN: i32 = 18;

/// The phones being asked about, oldest first: what the popup shows.
static ASKING: Mutex<Vec<AccountPhone>> = Mutex::new(Vec::new());
/// The height the popup measured for its cards; 0 until it has.
static HEIGHT: AtomicU32 = AtomicU32::new(0);
/// Whether Talvio was the active app the last time it was looked at before
/// a click on the popup (the click itself activates it).
#[cfg(target_os = "macos")]
static ACTIVE_WHEN_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether a task keeps [`ACTIVE_WHEN_ASKED`] current.
#[cfg(target_os = "macos")]
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn asking_list() -> MutexGuard<'static, Vec<AccountPhone>> {
    ASKING.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the popup shows, for a webview that just loaded.
pub fn asking() -> Vec<AccountPhone> {
    asking_list().clone()
}

/// Ask about `phone`, showing the popup.
pub fn ask(app: &AppHandle, phone: AccountPhone) {
    {
        let mut asking = asking_list();
        if asking.iter().any(|p| p.id == phone.id) {
            return;
        }
        asking.push(phone);
    }
    refresh(app);
}

/// Take the question about `phone` down, and the popup with its last one.
pub fn close(app: &AppHandle, phone: &str) {
    let changed = {
        let mut asking = asking_list();
        let before = asking.len();
        asking.retain(|p| p.id != phone);
        asking.len() != before
    };
    if changed {
        refresh(app);
    }
}

/// Show, update or hide the popup to match [`ASKING`]. Queued onto the main
/// thread, because building a WebView window off it deadlocks the event loop.
fn refresh(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        let asking = asking();
        if asking.is_empty() {
            HEIGHT.store(0, Ordering::SeqCst);
            if let Some(window) = handle.get_webview_window(POPUP_LABEL) {
                let _ = window.hide();
            }
        } else {
            build(&handle);
            if let Some(window) = handle.get_webview_window(POPUP_LABEL) {
                #[cfg(target_os = "macos")]
                {
                    if !window.is_visible().unwrap_or(false) {
                        ACTIVE_WHEN_ASKED.store(macos::app_is_active(), Ordering::SeqCst);
                    }
                    watch_activation(&handle);
                }
                place(&handle, &window);
                let _ = window.show();
                // Another process can take topmost at any time.
                let _ = window.set_always_on_top(true);
            }
        }
        // After the window exists; a webview still loading asks on mount.
        let _ = handle.emit_to(POPUP_LABEL, CHANGED_EVENT, &asking);
    }) {
        warn!("Could not queue the phone approval popup onto the main thread: {e}");
    }
}

fn build(app: &AppHandle) {
    if app.get_webview_window(POPUP_LABEL).is_some() {
        return;
    }
    let builder = WebviewWindowBuilder::new(
        app,
        POPUP_LABEL,
        tauri::WebviewUrl::App("src/approval/index.html".into()),
    )
    .additional_browser_args(crate::WEBVIEW2_BROWSER_ARGS)
    .title("Allow phone")
    .inner_size(WIDTH, FALLBACK_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    // A click lands on the button it was aimed at.
    .accept_first_mouse(true)
    .focused(false)
    .visible(false);

    // A click cannot take the foreground: Windows' `WS_EX_NOACTIVATE`, and on
    // X11 a window that does not accept input focus.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let builder = builder.focusable(false);

    let mut builder = builder;
    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    match builder.build() {
        Ok(window) => {
            #[cfg(target_os = "linux")]
            {
                if init_layer_shell(&window) {
                    debug!("GTK layer shell initialized for the phone approval popup");
                }
                // After layer shell, which must be set up before realization.
                crate::overlay::realize_gtk_window(&window);
            }
            let app_handle = app.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    // Closing it is Don't allow, for every phone it shows.
                    api.prevent_close();
                    let app = app_handle.clone();
                    let phones = asking().into_iter().map(|phone| phone.id).collect();
                    tauri::async_runtime::spawn(async move {
                        super::answer_from_popup(&app, phones, false).await;
                    });
                }
            });
            debug!("Phone approval popup window created (hidden)");
        }
        Err(e) => error!("Failed to create the phone approval popup window: {e}"),
    }
}

/// A layer surface in the top-right corner that never takes the keyboard, as
/// the recording overlay is on Wayland. False where the compositor has none.
#[cfg(target_os = "linux")]
fn init_layer_shell(window: &tauri::WebviewWindow) -> bool {
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
    if crate::overlay::env_flag_enabled("NO_GTK_LAYER_SHELL") || !gtk_layer_shell::is_supported() {
        return false;
    }
    let Ok(gtk_window) = window.gtk_window() else {
        return false;
    };
    gtk_window.init_layer_shell();
    gtk_window.set_layer(Layer::Overlay);
    gtk_window.set_keyboard_mode(KeyboardMode::None);
    gtk_window.set_exclusive_zone(0);
    for edge in [Edge::Top, Edge::Right] {
        gtk_window.set_anchor(edge, true);
        gtk_window.set_layer_shell_margin(edge, MARGIN);
    }
    true
}

fn place(app: &AppHandle, window: &tauri::WebviewWindow) {
    let height = match HEIGHT.load(Ordering::SeqCst) {
        0 => FALLBACK_HEIGHT,
        measured => measured as f64,
    };
    crate::reminders::place_top_right(app, window, WIDTH, height);
    clear_reminder(app, window);
}

/// A reminder popup was placed: keep this card clear of it.
pub fn make_room(app: &AppHandle) {
    if let Some(window) = app
        .get_webview_window(POPUP_LABEL)
        .filter(|window| window.is_visible().unwrap_or(false))
    {
        clear_reminder(app, &window);
    }
}

/// The reminder popup uses the same corner, and whichever was shown last
/// would cover the other: this card moves below a reminder it overlaps.
fn clear_reminder(app: &AppHandle, window: &tauri::WebviewWindow) {
    let Some(reminder) = app
        .get_webview_window(crate::reminders::POPUP_LABEL)
        .filter(|reminder| reminder.is_visible().unwrap_or(false))
    else {
        return;
    };
    let (Ok(at), Ok(size), Ok(reminder_at), Ok(reminder_size)) = (
        window.outer_position(),
        window.outer_size(),
        reminder.outer_position(),
        reminder.outer_size(),
    ) else {
        return;
    };
    let gap = (GAP * window.scale_factor().unwrap_or(1.0)).round() as i32;
    if let Some(y) = below((at, size), (reminder_at, reminder_size), gap) {
        let _ = window.set_position(PhysicalPosition::new(at.x, y));
    }
}

type Frame = (PhysicalPosition<i32>, PhysicalSize<u32>);

/// The `y` that puts `ours` `gap` below `theirs`, when the two overlap.
fn below((at, size): Frame, (their_at, their_size): Frame, gap: i32) -> Option<i32> {
    let overlaps = at.x < their_at.x + their_size.width as i32
        && their_at.x < at.x + size.width as i32
        && at.y < their_at.y + their_size.height as i32
        && their_at.y < at.y + size.height as i32;
    overlaps.then(|| their_at.y + their_size.height as i32 + gap)
}

/// The webview reports the height its cards need, and the window takes it.
pub fn fit(app: &AppHandle, requested: f64) {
    if !requested.is_finite() || requested <= 0.0 {
        return;
    }
    let height = requested.clamp(MIN_HEIGHT, MAX_HEIGHT).round();
    if HEIGHT.swap(height as u32, Ordering::SeqCst) == height as u32 {
        return;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(POPUP_LABEL) {
            place(&handle, &window);
        }
    });
}

/// After a click on the popup: on macOS it made Talvio the active app, so
/// the next paste (the one Allow lets through, or any later one) would land
/// here. Nothing to do elsewhere, where the popup cannot take the foreground.
#[cfg(not(target_os = "macos"))]
pub async fn hand_back_focus(_app: &AppHandle) {}

/// `hide:` gives activation back to the app that had it (as the assistant's
/// Insert does), then the windows it hid return without taking it again.
/// Skipped when Talvio was active just before the click: the user was in
/// one of its windows, and the paste belongs there.
#[cfg(target_os = "macos")]
pub async fn hand_back_focus(app: &AppHandle) {
    use std::time::Duration;
    if ACTIVE_WHEN_ASKED.load(Ordering::SeqCst) {
        return;
    }
    let (hid, hidden) = tokio::sync::oneshot::channel();
    if app
        .run_on_main_thread(move || {
            let _ = hid.send(macos::hide_if_active());
        })
        .is_err()
    {
        return;
    }
    if matches!(
        tokio::time::timeout(Duration::from_millis(500), hidden).await,
        Ok(Ok(true))
    ) {
        // Activation moves in the window server, not in that call.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let _ = app.run_on_main_thread(macos::unhide_without_activation);
    }
}

/// Keep [`ACTIVE_WHEN_ASKED`] current while anything is asked: the user may
/// move to another app between the card appearing and their click on it.
/// Called on the main thread, where each look runs too.
// shortcut: looks every 250 ms, so a click within that of switching apps is
// judged by the app before; an NSApplicationDidResignActive observer is exact.
#[cfg(target_os = "macos")]
fn watch_activation(app: &AppHandle) {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let (looked, still_asking) = tokio::sync::oneshot::channel();
            let handle = app.clone();
            let queued = app.run_on_main_thread(move || {
                let _ = looked.send(look_at_activation(&handle));
            });
            if queued.is_err() || !still_asking.await.unwrap_or(false) {
                return;
            }
        }
    });
}

/// False, and the watch ends, once nothing is asked.
#[cfg(target_os = "macos")]
fn look_at_activation(app: &AppHandle) -> bool {
    if asking_list().is_empty() {
        WATCHING.store(false, Ordering::SeqCst);
        return false;
    }
    // A focused popup was clicked, and the click is why the app is active.
    if let Some(popup) = app.get_webview_window(POPUP_LABEL) {
        if popup.is_visible().unwrap_or(false) && !popup.is_focused().unwrap_or(false) {
            ACTIVE_WHEN_ASKED.store(macos::app_is_active(), Ordering::SeqCst);
        }
    }
    true
}

#[cfg(target_os = "macos")]
mod macos {
    use tauri_nspanel::objc2::MainThreadMarker;
    use tauri_nspanel::objc2_app_kit::NSApplication;

    pub fn app_is_active() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive())
    }

    pub fn hide_if_active() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| {
            let ns_app = NSApplication::sharedApplication(mtm);
            let active = ns_app.isActive();
            if active {
                ns_app.hide(None);
            }
            active
        })
    }

    pub fn unhide_without_activation() {
        if let Some(mtm) = MainThreadMarker::new() {
            let ns_app = NSApplication::sharedApplication(mtm);
            if ns_app.isHidden() {
                ns_app.unhideWithoutActivation();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: i32, y: i32, width: u32, height: u32) -> Frame {
        (
            PhysicalPosition::new(x, y),
            PhysicalSize::new(width, height),
        )
    }

    #[test]
    fn the_card_goes_below_a_reminder_it_overlaps() {
        // Both right-aligned in the same corner; the reminder is wider.
        let reminder = frame(1502, 18, 400, 92);
        assert_eq!(below(frame(1542, 18, 360, 160), reminder, 8), Some(118));
        // Already below it, or on another display: left where it is.
        assert_eq!(below(frame(1542, 118, 360, 160), reminder, 8), None);
        assert_eq!(below(frame(3462, 18, 360, 160), reminder, 8), None);
    }
}
