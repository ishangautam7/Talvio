//! Windows: clicks and wheel turns through `SendInput`, and UI Automation for
//! the control under a point and the text cursor.
//!
//! UI Automation calls go to one worker thread of their own (an MTA with short
//! connection and transaction timeouts, as smart spacing's reader has), so a
//! hung app costs one skipped answer, never a stuck session.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;
use std::time::Duration;

use log::{debug, warn};
use windows::core::BOOL;
use windows::Win32::Foundation::{POINT, RECT, S_OK};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextPattern2, IUIAutomationTextRange, TextUnit_Character, UIA_TextPattern2Id,
    UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetSystemMetrics,
    GetWindowThreadProcessId, SetCursorPos, GUITHREADINFO, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WHEEL_DELTA,
};

use super::{Button, ControlInfo, Rect};

/// How long a UI Automation call may take inside another process.
const UIA_TIMEOUT_MS: u32 = 300;
/// How long the session waits for the worker's answer.
const ANSWER_WAIT: Duration = Duration::from_millis(500);
/// A label longer than this is someone's document, not a button's name.
const MAX_LABEL_CHARS: usize = 120;
/// The gap between the two clicks of a double click, well inside the
/// system's double-click time.
const DOUBLE_CLICK_GAP: Duration = Duration::from_millis(40);

/// Click `count` times with `button` at (`x`, `y`), then put the pointer back
/// where it was, so someone at the computer finds it where they left it.
pub fn click(x: i32, y: i32, button: Button, count: u8) -> Result<(), String> {
    let (down, up) = match button {
        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
    };
    let _injected = handy_keys::ignore_injected_input();
    let back = cursor();
    send(&[move_to(x, y)])?;
    for press in 0..count.max(1) {
        if press > 0 {
            std::thread::sleep(DOUBLE_CLICK_GAP);
        }
        send(&[mouse(down, 0, 0, 0), mouse(up, 0, 0, 0)])?;
    }
    restore(back);
    Ok(())
}

/// Turn the wheel `dy` notches (up is negative, as on a phone a drag up
/// scrolls down) and `dx` sideways, over (`x`, `y`).
pub fn scroll(x: i32, y: i32, dx: i32, dy: i32) -> Result<(), String> {
    let _injected = handy_keys::ignore_injected_input();
    let back = cursor();
    send(&[move_to(x, y)])?;
    let notch = WHEEL_DELTA as i32;
    if dy != 0 {
        send(&[mouse(MOUSEEVENTF_WHEEL, 0, 0, -dy * notch)])?;
    }
    if dx != 0 {
        send(&[mouse(MOUSEEVENTF_HWHEEL, 0, 0, dx * notch)])?;
    }
    restore(back);
    Ok(())
}

fn cursor() -> Option<POINT> {
    let mut point = POINT::default();
    // SAFETY: writes the position into a local.
    unsafe { GetCursorPos(&mut point) }.ok().map(|_| point)
}

fn restore(back: Option<POINT>) {
    if let Some(point) = back {
        // SAFETY: plain user32 call with values read from GetCursorPos.
        let _ = unsafe { SetCursorPos(point.x, point.y) };
    }
}

/// An absolute move to a physical screen point, in the 0..=65535 space
/// `SendInput` maps across the whole virtual desktop.
fn move_to(x: i32, y: i32) -> INPUT {
    // SAFETY: metric reads with no arguments that can be wrong.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let (nx, ny) = normalize(x, y, left, top, width, height);
    mouse(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        nx,
        ny,
        0,
    )
}

/// A point on a virtual desktop at (`left`, `top`) sized `width` by `height`,
/// in `SendInput`'s absolute coordinates.
fn normalize(x: i32, y: i32, left: i32, top: i32, width: i32, height: i32) -> (i32, i32) {
    let scale = |value: i32, origin: i32, size: i32| {
        let span = (size - 1).max(1) as i64;
        let offset = (value - origin).clamp(0, size.max(1) - 1) as i64;
        ((offset * 65_535 + span / 2) / span) as i32
    };
    (scale(x, left, width), scale(y, top, height))
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    // SAFETY: a slice of fully initialised INPUT structures and their size.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        // Blocked by UIPI: the window under the point runs elevated.
        Err("Windows did not take the click (the app may be running as administrator)".into())
    }
}

/// The control at a physical screen point.
pub fn control_at(x: i32, y: i32) -> Option<ControlInfo> {
    ask(Job::ControlAt(POINT { x, y })).and_then(|answer| match answer {
        Answer::Control(info) => info,
        _ => None,
    })
}

/// Whether the control with the keyboard is a password field.
pub fn focused_is_password() -> bool {
    matches!(ask(Job::FocusedPassword), Some(Answer::Password(true)))
}

/// The text cursor of the window in front, in physical screen pixels: the
/// system caret when the app has one (Win32 edit boxes, many classic apps),
/// otherwise UI Automation's caret or selection.
pub fn caret() -> Option<Rect> {
    if let Some(rect) = system_caret() {
        return Some(rect);
    }
    ask(Job::Caret).and_then(|answer| match answer {
        Answer::Caret(rect) => rect,
        _ => None,
    })
}

fn system_caret() -> Option<Rect> {
    // SAFETY: reads the foreground window's GUI thread state into a local
    // structure sized as the API requires, then maps a point.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return None;
        }
        let thread = GetWindowThreadProcessId(foreground, None);
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(thread, &mut info).ok()?;
        if info.hwndCaret.is_invalid() {
            return None;
        }
        let RECT {
            left,
            top,
            right,
            bottom,
        } = info.rcCaret;
        if bottom <= top {
            return None;
        }
        let mut corner = POINT { x: left, y: top };
        if !ClientToScreen(info.hwndCaret, &mut corner).as_bool() {
            return None;
        }
        Some(Rect {
            left: corner.x,
            top: corner.y,
            width: (right - left).max(2),
            height: bottom - top,
        })
    }
}

enum Job {
    ControlAt(POINT),
    FocusedPassword,
    Caret,
}

enum Answer {
    Control(Option<ControlInfo>),
    Password(bool),
    Caret(Option<Rect>),
}

type Request = (Job, SyncSender<Answer>);

/// One question to the worker. `None` when it is busy with an earlier one or
/// does not answer in time.
fn ask(job: Job) -> Option<Answer> {
    static WORKER: OnceLock<Option<SyncSender<Request>>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (requests, jobs) = sync_channel::<Request>(1);
        std::thread::Builder::new()
            .name("live-view-uia".into())
            .spawn(move || worker(jobs))
            .map_err(|e| warn!("Live view: no UI Automation thread: {e}"))
            .ok()
            .map(|_| requests)
    });
    let (answer, answered) = sync_channel(1);
    worker.as_ref()?.try_send((job, answer)).ok()?;
    answered.recv_timeout(ANSWER_WAIT).ok()
}

/// Owns this thread's COM apartment; uninitialises only what it initialised.
struct Apartment {
    owned: bool,
}

impl Apartment {
    fn new() -> Self {
        // SAFETY: initialising COM on this thread, balanced in Drop.
        let owned = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) } == S_OK;
        Self { owned }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances the CoInitializeEx above, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

fn worker(jobs: Receiver<Request>) {
    let _apartment = Apartment::new();
    // SAFETY: COM is initialised on this thread; the client lives and dies here.
    let client = unsafe {
        CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
    }
    .ok()
    .inspect(|client| {
        // SAFETY: setting timeouts on a live client.
        unsafe {
            let _ = client.SetConnectionTimeout(UIA_TIMEOUT_MS);
            let _ = client.SetTransactionTimeout(UIA_TIMEOUT_MS);
        }
    });
    if client.is_none() {
        warn!("Live view: UI Automation is unavailable");
    }
    while let Ok((job, reply)) = jobs.recv() {
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let client = client.as_ref();
            match job {
                Job::ControlAt(point) => Answer::Control(client.and_then(|c| control(c, point))),
                Job::FocusedPassword => Answer::Password(client.is_some_and(focused_password)),
                Job::Caret => Answer::Caret(client.and_then(uia_caret)),
            }
        }));
        match answer {
            Ok(answer) => {
                let _ = reply.try_send(answer);
            }
            Err(_) => debug!("Live view: a UI Automation call panicked"),
        }
    }
    drop(client);
}

fn control(client: &IUIAutomation2, point: POINT) -> Option<ControlInfo> {
    // SAFETY: COM calls on a live client from its own apartment.
    unsafe {
        let element = client.ElementFromPoint(point).ok()?;
        let label = element
            .CurrentName()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let label: String = label.trim().chars().take(MAX_LABEL_CHARS).collect();
        let password = element
            .CurrentIsPassword()
            .map(BOOL::as_bool)
            .unwrap_or(false);
        let role = element
            .CurrentControlType()
            .map(|kind| crate::command_mode::agent::uia_windows::role_for(kind.0))
            .unwrap_or("other");
        Some(ControlInfo {
            role,
            label,
            password,
        })
    }
}

fn focused_password(client: &IUIAutomation2) -> bool {
    // SAFETY: COM calls on a live client from its own apartment.
    unsafe {
        client
            .GetFocusedElement()
            .and_then(|element| element.CurrentIsPassword())
            .map(BOOL::as_bool)
            .unwrap_or(false)
    }
}

fn uia_caret(client: &IUIAutomation2) -> Option<Rect> {
    // SAFETY: COM calls on a live client from its own apartment.
    unsafe {
        let element = client.GetFocusedElement().ok()?;
        if element
            .CurrentIsPassword()
            .map(BOOL::as_bool)
            .unwrap_or(false)
        {
            // Where someone types a password is nobody else's business.
            return None;
        }
        let range = caret_range(&element)?;
        if let Some(rect) = bounds(&range) {
            return Some(rect);
        }
        // An empty range often has no rectangle; the character after it does.
        let wider = range.Clone().ok()?;
        wider.ExpandToEnclosingUnit(TextUnit_Character).ok()?;
        bounds(&wider).map(|rect| Rect { width: 2, ..rect })
    }
}

/// The caret as a text range: TextPattern2's caret when it is active, else
/// the end of the first selection.
unsafe fn caret_range(element: &IUIAutomationElement) -> Option<IUIAutomationTextRange> {
    if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
    {
        let mut active = BOOL(0);
        if let Ok(range) = pattern.GetCaretRange(&mut active) {
            if active.as_bool() {
                return Some(range);
            }
        }
    }
    let pattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let selection = pattern.GetSelection().ok()?;
    (selection.Length().ok()? > 0)
        .then(|| selection.GetElement(0).ok())
        .flatten()
}

/// The last line rectangle of a range (where its end is), in screen pixels.
unsafe fn bounds(range: &IUIAutomationTextRange) -> Option<Rect> {
    let array = range.GetBoundingRectangles().ok()?;
    let values = safe_array_doubles(array)?;
    let last = values.chunks_exact(4).last()?;
    let [left, top, width, height] = [last[0], last[1], last[2], last[3]];
    (height > 0.0).then(|| Rect {
        left: (left + width).round() as i32,
        top: top.round() as i32,
        width: 2,
        height: height.round() as i32,
    })
}

/// The doubles in a one-dimensional SAFEARRAY, which the array is freed after.
unsafe fn safe_array_doubles(
    array: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Option<Vec<f64>> {
    use windows::Win32::System::Ole::{
        SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
    };
    if array.is_null() {
        return None;
    }
    let read = || -> Option<Vec<f64>> {
        let lower = SafeArrayGetLBound(array, 1).ok()?;
        let upper = SafeArrayGetUBound(array, 1).ok()?;
        let mut values = Vec::with_capacity((upper - lower + 1).max(0) as usize);
        for index in lower..=upper {
            let mut value = 0f64;
            SafeArrayGetElement(array, &index, &mut value as *mut f64 as *mut _).ok()?;
            values.push(value);
        }
        Some(values)
    };
    let values = read();
    let _ = SafeArrayDestroy(array);
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_map_across_the_virtual_desktop() {
        // One 1920x1080 screen.
        assert_eq!(normalize(0, 0, 0, 0, 1920, 1080), (0, 0));
        assert_eq!(normalize(1919, 1079, 0, 0, 1920, 1080), (65_535, 65_535));
        assert_eq!(normalize(960, 540, 0, 0, 1920, 1080).0, 32_785);
        // A second screen to the left makes the desktop start below zero.
        assert_eq!(normalize(-1920, 0, -1920, 0, 3840, 1080), (0, 0));
        assert_eq!(normalize(3839, 0, -1920, 0, 3840, 1080).0, 65_535);
        // Outside the desktop is clamped to its edge.
        assert_eq!(normalize(-5000, 9000, 0, 0, 1920, 1080), (0, 65_535));
    }
}
