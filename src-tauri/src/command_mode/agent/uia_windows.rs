//! Windows: a window's controls through UI Automation, for command mode's agent.
//!
//! The platform half of [`super::tree`]. One dedicated thread owns the UI
//! Automation client and every element it hands out, for the same reasons as
//! `smart_spacing/windows.rs`: COM interface pointers are neither `Send` nor
//! `Sync`, and a cross-process call into a busy or hung app blocks the thread
//! that makes it, so callers only ever wait on a channel with a deadline. A
//! call that misses its deadline abandons that thread (it exits once its call
//! returns and it finds the channel closed) and the next call starts a fresh
//! one, so one hung app cannot hold every later read behind it. The elements of
//! the latest snapshot live on that thread, indexed by id, which is what lets
//! `act` find them; a fresh thread has none, and the agent is told to read the
//! window again.
//!
//! What the read does, and why:
//!
//! * **One cross-process call.** A cache request with every property the
//!   rendering needs, scoped to the whole subtree under the control view, is
//!   built in a single `ElementFromHandleBuildCache`. Everything after that
//!   (properties, children, nesting) is answered from the local cache, so a big
//!   Chromium or Electron page costs one round trip rather than one per
//!   property per element. Walking the cached children instead of a flat
//!   `FindAllBuildCache(TreeScope_Descendants)` gives each element its real
//!   nesting for free, at the same cost.
//! * **Pruned to what a person could act on or read as a label**: offscreen
//!   elements, scroll bars and title bars are dropped; an element with neither
//!   a name nor a value is dropped unless it is a field; text is kept only when
//!   it says something its parent does not, and at most [`MAX_TEXTS`] of it.
//!   At most [`MAX_ELEMENTS`] are kept, in document order.
//! * **A password field's value is never read**: its cached value is not even
//!   fetched out of the cache, and it reports the state "password".
//! * **The foreground window is read where the input goes.** With no window
//!   given, the window in front is read (a Save As dialog, say), but it is
//!   reported under its root owner's id and title, exactly as
//!   `effects::windows::foreground_window` and `open_windows` name it.
//! * **Chromium answers its first accessibility request with nothing**, so an
//!   empty first read is tried once more a moment later.
//!
//! Nothing here clicks or types: every action is a UI Automation pattern call.
//! No element name or value is ever logged; counts and timings are.

use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use log::{debug, warn};
use windows::core::BSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, S_OK};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, ExpandCollapseState_Collapsed, ExpandCollapseState_Expanded,
    ExpandCollapseState_LeafNode, ExpandCollapseState_PartiallyExpanded, IUIAutomation2,
    IUIAutomationCacheRequest, IUIAutomationElement, IUIAutomationExpandCollapsePattern,
    IUIAutomationInvokePattern, IUIAutomationScrollItemPattern, IUIAutomationSelectionItemPattern,
    IUIAutomationTogglePattern, IUIAutomationValuePattern, ToggleState_Off, ToggleState_On,
    TreeScope_Subtree, UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId,
    UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId, UIA_ControlTypePropertyId,
    UIA_CustomControlTypeId, UIA_DataItemControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_ExpandCollapseExpandCollapseStatePropertyId,
    UIA_ExpandCollapsePatternId, UIA_GroupControlTypeId, UIA_HasKeyboardFocusPropertyId,
    UIA_HyperlinkControlTypeId, UIA_InvokePatternId, UIA_IsEnabledPropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId, UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId, UIA_IsPasswordPropertyId,
    UIA_IsScrollItemPatternAvailablePropertyId, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsTogglePatternAvailablePropertyId, UIA_IsValuePatternAvailablePropertyId,
    UIA_ListItemControlTypeId, UIA_MenuItemControlTypeId, UIA_NamePropertyId,
    UIA_PaneControlTypeId, UIA_RadioButtonControlTypeId, UIA_ScrollBarControlTypeId,
    UIA_ScrollItemPatternId, UIA_SelectionItemIsSelectedPropertyId, UIA_SelectionItemPatternId,
    UIA_SliderControlTypeId, UIA_SplitButtonControlTypeId, UIA_TabItemControlTypeId,
    UIA_TextControlTypeId, UIA_ThumbControlTypeId, UIA_TitleBarControlTypeId, UIA_TogglePatternId,
    UIA_ToggleToggleStatePropertyId, UIA_ToolTipControlTypeId, UIA_TreeItemControlTypeId,
    UIA_ValueIsReadOnlyPropertyId, UIA_ValuePatternId, UIA_ValueValuePropertyId,
    UIA_E_ELEMENTNOTAVAILABLE, UIA_E_ELEMENTNOTENABLED, UIA_E_NOTSUPPORTED, UIA_E_TIMEOUT,
    UIA_PROPERTY_ID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsHungAppWindow, IsWindow, IsWindowVisible, GA_ROOTOWNER,
};

use super::tree::{AccessibilityTree, TreeError, UiAction, UiElement, UiSnapshot};

/// The most elements one snapshot keeps. About 3K tokens rendered.
const MAX_ELEMENTS: usize = 300;
/// The most text elements one snapshot keeps. Text labels nearby fields, but a
/// page of prose would otherwise fill the whole budget.
const MAX_TEXTS: usize = 80;
/// The most cached elements one walk looks at, so a pathological page (a
/// spreadsheet's grid, an endless feed) bounds the local work too.
const MAX_VISITED: usize = 20_000;
/// The longest value kept from a field. The rendering cuts it further; this
/// only stops a document's whole text from being held.
const MAX_VALUE_CHARS: usize = 500;
/// How long a caller waits for the worker.
const DEADLINE: Duration = Duration::from_secs(6);
/// UI Automation's own per-call timeouts. Building a big page's cache is one
/// long transaction, so it gets more than smart spacing's 200 ms.
const UIA_CONNECTION_TIMEOUT_MS: u32 = 2_000;
const UIA_TRANSACTION_TIMEOUT_MS: u32 = 5_000;
/// The pause before an empty read is tried again (Chromium turns its
/// accessibility on at the first request and answers that one with nothing).
const EMPTY_RETRY_AFTER: Duration = Duration::from_millis(150);

// Control types as plain integers, so the pure rules below (and their tests)
// need no COM types.
const BUTTON: i32 = UIA_ButtonControlTypeId.0;
const SPLIT_BUTTON: i32 = UIA_SplitButtonControlTypeId.0;
const EDIT: i32 = UIA_EditControlTypeId.0;
const HYPERLINK: i32 = UIA_HyperlinkControlTypeId.0;
const CHECK_BOX: i32 = UIA_CheckBoxControlTypeId.0;
const RADIO_BUTTON: i32 = UIA_RadioButtonControlTypeId.0;
const COMBO_BOX: i32 = UIA_ComboBoxControlTypeId.0;
const LIST_ITEM: i32 = UIA_ListItemControlTypeId.0;
const DATA_ITEM: i32 = UIA_DataItemControlTypeId.0;
const MENU_ITEM: i32 = UIA_MenuItemControlTypeId.0;
const TAB_ITEM: i32 = UIA_TabItemControlTypeId.0;
const TREE_ITEM: i32 = UIA_TreeItemControlTypeId.0;
const TEXT: i32 = UIA_TextControlTypeId.0;
const DOCUMENT: i32 = UIA_DocumentControlTypeId.0;
const SLIDER: i32 = UIA_SliderControlTypeId.0;
const GROUP: i32 = UIA_GroupControlTypeId.0;
const PANE: i32 = UIA_PaneControlTypeId.0;
const CUSTOM: i32 = UIA_CustomControlTypeId.0;
const SCROLL_BAR: i32 = UIA_ScrollBarControlTypeId.0;
const THUMB: i32 = UIA_ThumbControlTypeId.0;
const TITLE_BAR: i32 = UIA_TitleBarControlTypeId.0;
const TOOL_TIP: i32 = UIA_ToolTipControlTypeId.0;

const TOGGLE_ON: i32 = ToggleState_On.0;
const TOGGLE_OFF: i32 = ToggleState_Off.0;
const EXPANDED: i32 = ExpandCollapseState_Expanded.0;
const PARTIALLY_EXPANDED: i32 = ExpandCollapseState_PartiallyExpanded.0;
const COLLAPSED: i32 = ExpandCollapseState_Collapsed.0;
const LEAF: i32 = ExpandCollapseState_LeafNode.0;

// ── The reader ─────────────────────────────────────────────────────────────

/// UI Automation behind [`AccessibilityTree`]. Cheap to create; the worker
/// thread starts on first use and is restarted if it dies or hangs.
pub struct UiaTree {
    jobs: Mutex<Option<Sender<Request>>>,
}

enum Request {
    Snapshot {
        window: Option<i64>,
        reply: SyncSender<Result<UiSnapshot, TreeError>>,
    },
    Act {
        element: u32,
        action: UiAction,
        reply: SyncSender<Result<(), TreeError>>,
    },
}

impl UiaTree {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(None),
        }
    }

    fn jobs(&self) -> MutexGuard<'_, Option<Sender<Request>>> {
        // A poisoned lock only means another caller panicked while holding a
        // sender; the sender itself is still fine to use or replace.
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Hand `request` to the worker, starting one if there is none or the
    /// last one died.
    fn send(&self, request: Request) -> Result<(), TreeError> {
        let mut jobs = self.jobs();
        let request = match jobs.as_ref() {
            Some(sender) => match sender.send(request) {
                Ok(()) => return Ok(()),
                Err(mpsc::SendError(request)) => {
                    debug!("Command mode: the UI Automation worker had stopped; starting another");
                    request
                }
            },
            None => request,
        };
        let sender = spawn_worker()?;
        sender
            .send(request)
            .map_err(|_| TreeError::Failed("the accessibility reader could not start".into()))?;
        *jobs = Some(sender);
        Ok(())
    }

    fn call<T>(
        &self,
        what: &str,
        make: impl FnOnce(SyncSender<Result<T, TreeError>>) -> Request,
    ) -> Result<T, TreeError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.send(make(reply))?;
        match answer.recv_timeout(DEADLINE) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // The worker is stuck in a call into the target app. Leave it
                // to finish on its own and let the next call start fresh.
                warn!(
                    "Command mode: {what} did not finish within {} s; restarting the UI Automation reader",
                    DEADLINE.as_secs()
                );
                *self.jobs() = None;
                Err(TreeError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                warn!("Command mode: the UI Automation worker stopped during {what}");
                *self.jobs() = None;
                Err(TreeError::Failed(
                    "the accessibility reader stopped unexpectedly".into(),
                ))
            }
        }
    }
}

impl Default for UiaTree {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityTree for UiaTree {
    fn snapshot(&self, window: Option<i64>) -> Result<UiSnapshot, TreeError> {
        self.call("a window read", |reply| Request::Snapshot { window, reply })
    }

    fn act(&self, element: u32, action: &UiAction) -> Result<(), TreeError> {
        let action = action.clone();
        self.call("an element action", |reply| Request::Act {
            element,
            action,
            reply,
        })
    }
}

fn spawn_worker() -> Result<Sender<Request>, TreeError> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("command-mode-uia".into())
        .spawn(move || worker(receiver))
        .map_err(|err| {
            warn!("Command mode: could not start the UI Automation thread: {err}");
            TreeError::Failed("the accessibility reader could not start".into())
        })?;
    Ok(sender)
}

// ── The worker thread ──────────────────────────────────────────────────────

/// Owns this thread's COM apartment. `S_OK` means we initialised COM and owe
/// the matching `CoUninitialize`; `S_FALSE` means somebody else did.
struct Apartment {
    owned: bool,
}

impl Apartment {
    fn new() -> Self {
        // SAFETY: called once, on the thread that owns the apartment; the
        // matching uninitialise happens in Drop on the same thread.
        let owned = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) } == S_OK;
        Self { owned }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances the successful CoInitializeEx in `new`, on the
            // same thread, after every COM object of this thread is released.
            unsafe { CoUninitialize() };
        }
    }
}

/// The worker's state. Field order is drop order: the elements go before the
/// client, and both before the apartment (a local declared earlier).
struct Worker {
    held: Vec<(IUIAutomationElement, Held)>,
    client: Option<IUIAutomation2>,
}

fn worker(jobs: Receiver<Request>) {
    let _apartment = Apartment::new();
    // SAFETY: COM is initialised on this thread; the client never leaves it.
    let client = unsafe {
        CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
    }
    .ok()
    .map(|client| {
        // SAFETY: plain setters on a live client; a failure keeps the defaults.
        unsafe {
            let _ = client.SetConnectionTimeout(UIA_CONNECTION_TIMEOUT_MS);
            let _ = client.SetTransactionTimeout(UIA_TRANSACTION_TIMEOUT_MS);
        }
        client
    });
    if client.is_none() {
        warn!("Command mode: UI Automation is unavailable; window controls cannot be read");
    }
    let mut state = Worker {
        held: Vec::new(),
        client,
    };
    while let Ok(request) = jobs.recv() {
        match request {
            Request::Snapshot { window, reply } => {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.snapshot(window)
                }))
                .unwrap_or_else(|_| {
                    warn!("Command mode: reading a window's controls panicked");
                    Err(TreeError::Failed("reading the window failed".into()))
                });
                let _ = reply.send(result);
            }
            Request::Act {
                element,
                action,
                reply,
            } => {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.act(element, &action)
                }))
                .unwrap_or_else(|_| {
                    warn!("Command mode: acting on an element panicked");
                    Err(TreeError::Failed("that action failed".into()))
                });
                let _ = reply.send(result);
            }
        }
    }
    // `state` (elements, then client) drops here, before the apartment.
}

impl Worker {
    fn snapshot(&mut self, window: Option<i64>) -> Result<UiSnapshot, TreeError> {
        // Every earlier id is invalid from here on, whatever this read finds.
        self.held.clear();
        let client = self.client.as_ref().ok_or_else(|| {
            TreeError::Failed("UI Automation isn't available on this system".into())
        })?;
        let target = resolve_window(window)?;
        let started = Instant::now();
        let mut walked = read_tree(client, target.read)?;
        if walked.collector.elements.is_empty() && started.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(EMPTY_RETRY_AFTER);
            walked = read_tree(client, target.read)?;
        }
        let Walked {
            collector,
            nodes,
            visited,
        } = walked;
        debug!(
            "Command mode: read {} controls ({} text, {} looked at{}) in {} ms",
            collector.elements.len(),
            collector.texts,
            visited,
            if collector.truncated { ", cut" } else { "" },
            started.elapsed().as_millis()
        );
        self.held = nodes.into_iter().zip(collector.held).collect();
        Ok(UiSnapshot {
            window_id: target.id,
            window_title: target.title,
            app: target.app,
            elements: collector.elements,
            truncated: collector.truncated,
        })
    }

    fn act(&self, id: u32, action: &UiAction) -> Result<(), TreeError> {
        let (element, held) = id
            .checked_sub(1)
            .and_then(|index| self.held.get(index as usize))
            .ok_or_else(|| TreeError::NotFound(format!("element {id}")))?;
        if let Some(refusal) = refusal(id, held, action) {
            return Err(TreeError::NotSupported(refusal));
        }
        let started = Instant::now();
        let done = perform(element, action);
        debug!(
            "Command mode: {} on an element took {} ms ({})",
            action.label(),
            started.elapsed().as_millis(),
            if done.is_ok() { "ok" } else { "failed" }
        );
        done.map_err(|err| action_error(id, held, action, &err))
    }
}

/// Do `action` through its pattern. The pattern is fetched live, not from the
/// cache: a cached pattern object would act on the state at read time.
fn perform(element: &IUIAutomationElement, action: &UiAction) -> windows::core::Result<()> {
    // SAFETY (every block below): `element` is a live element owned by this
    // thread; each pattern is checked by `GetCurrentPatternAs`, which answers
    // an error (never a dangling interface) when the element lacks it.
    match action {
        UiAction::Invoke => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?
                .Invoke()
        },
        UiAction::SetValue(text) => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)?
                .SetValue(&BSTR::from(text.as_str()))
        },
        UiAction::Toggle => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)?
                .Toggle()
        },
        UiAction::Select => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                    UIA_SelectionItemPatternId,
                )?
                .Select()
        },
        UiAction::Expand => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )?
                .Expand()
        },
        UiAction::Collapse => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )?
                .Collapse()
        },
        UiAction::ScrollIntoView => unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationScrollItemPattern>(UIA_ScrollItemPatternId)?
                .ScrollIntoView()
        },
        UiAction::Focus => unsafe { element.SetFocus() },
    }
}

/// A failed pattern call, as the agent should hear it.
fn action_error(id: u32, held: &Held, action: &UiAction, err: &windows::core::Error) -> TreeError {
    let code = err.code().0 as u32;
    match code {
        UIA_E_ELEMENTNOTAVAILABLE => TreeError::NotFound(format!("element {id}")),
        UIA_E_ELEMENTNOTENABLED => TreeError::NotSupported(format!("element {id} is disabled")),
        UIA_E_TIMEOUT => TreeError::Timeout,
        // 0 is a null pattern pointer: the element stopped offering it.
        0 | UIA_E_NOTSUPPORTED => TreeError::NotSupported(cannot(id, held, action)),
        _ => {
            debug!("Command mode: {} failed with {code:#010x}", action.label());
            TreeError::Failed(format!("element {id} could not {}", verb(action)))
        }
    }
}

// ── Windows ────────────────────────────────────────────────────────────────

/// The window to read, and how it is reported.
struct WindowTarget {
    /// The HWND whose tree is read.
    read: HWND,
    id: i64,
    title: String,
    app: String,
}

fn resolve_window(window: Option<i64>) -> Result<WindowTarget, TreeError> {
    let gone = || TreeError::NotFound("that window".into());
    let read = match window {
        Some(id) => HWND(id as isize as *mut c_void),
        // SAFETY: no arguments; a null handle is checked below.
        None => unsafe { GetForegroundWindow() },
    };
    // SAFETY: IsWindow accepts any value and only answers whether it names a
    // window right now.
    if read.is_invalid() || !unsafe { IsWindow(Some(read)) }.as_bool() {
        return Err(gone());
    }
    let own = own_process();
    let pid = window_process(read);
    if pid == 0 || pid == own {
        return Err(gone());
    }
    // SAFETY: a stale handle only makes this answer false.
    if unsafe { IsHungAppWindow(read) }.as_bool() {
        return Err(TreeError::Timeout);
    }
    let shown = match window {
        Some(_) => read,
        None => reported_window(read),
    };
    let shown = if window_process(shown) == own {
        read
    } else {
        shown
    };
    let title = window_title(shown);
    let exe = process_exe_name(window_process(shown))
        .map(|name| strip_exe(&name).to_string())
        .unwrap_or_default();
    Ok(WindowTarget {
        read,
        id: shown.0 as isize as i64,
        app: app_label(&exe, &title),
        title,
    })
}

/// The foreground window's visible root owner, the id `open_windows` lists
/// (same rule as `effects::windows::reported_window`).
fn reported_window(foreground: HWND) -> HWND {
    // SAFETY: read-only queries; a stale handle answers null or false.
    let (root, visible) = unsafe {
        let root = GetAncestor(foreground, GA_ROOTOWNER);
        let visible = !root.is_invalid() && IsWindowVisible(root).as_bool();
        (root, visible)
    };
    if visible && root != foreground {
        root
    } else {
        foreground
    }
}

fn own_process() -> u32 {
    // SAFETY: no arguments, cannot fail.
    unsafe { GetCurrentProcessId() }
}

fn window_process(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: a stale handle only leaves `pid` at 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn window_title(hwnd: HWND) -> String {
    // SAFETY: the buffer is sized from the reported length plus the
    // terminator; GetWindowTextW never writes past the slice it is given.
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

/// The executable's file name ("chrome.exe"). Copied from
/// `command_mode/effects/windows.rs`, where it is private.
fn process_exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
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

/// "chrome.exe" → "chrome" (same as `effects::windows::strip_exe`).
fn strip_exe(name: &str) -> &str {
    let len = name.len();
    if len > 4 && name.is_char_boundary(len - 4) && name[len - 4..].eq_ignore_ascii_case(".exe") {
        &name[..len - 4]
    } else {
        name
    }
}

/// A Store app's window belongs to `ApplicationFrameHost`; its title names
/// the app (same as `effects::windows::app_label`).
fn app_label(exe: &str, title: &str) -> String {
    if exe.eq_ignore_ascii_case("ApplicationFrameHost") && !title.trim().is_empty() {
        title.trim().to_string()
    } else {
        exe.to_string()
    }
}

// ── Reading the tree ───────────────────────────────────────────────────────

/// Every property the rendering needs, cached for the whole subtree.
fn cache_request(client: &IUIAutomation2) -> windows::core::Result<IUIAutomationCacheRequest> {
    const PROPERTIES: &[UIA_PROPERTY_ID] = &[
        UIA_NamePropertyId,
        UIA_ControlTypePropertyId,
        UIA_IsEnabledPropertyId,
        UIA_HasKeyboardFocusPropertyId,
        UIA_IsOffscreenPropertyId,
        UIA_IsPasswordPropertyId,
        UIA_AutomationIdPropertyId,
        UIA_IsKeyboardFocusablePropertyId,
        UIA_ValueValuePropertyId,
        UIA_ValueIsReadOnlyPropertyId,
        UIA_ToggleToggleStatePropertyId,
        UIA_ExpandCollapseExpandCollapseStatePropertyId,
        UIA_SelectionItemIsSelectedPropertyId,
        UIA_IsInvokePatternAvailablePropertyId,
        UIA_IsValuePatternAvailablePropertyId,
        UIA_IsTogglePatternAvailablePropertyId,
        UIA_IsSelectionItemPatternAvailablePropertyId,
        UIA_IsExpandCollapsePatternAvailablePropertyId,
        UIA_IsScrollItemPatternAvailablePropertyId,
    ];
    // SAFETY: plain calls on a live client and the request it returns.
    unsafe {
        let request = client.CreateCacheRequest()?;
        for property in PROPERTIES {
            request.AddProperty(*property)?;
        }
        request.SetTreeScope(TreeScope_Subtree)?;
        request.SetTreeFilter(&client.ControlViewCondition()?)?;
        Ok(request)
    }
}

struct Walked {
    collector: Collector,
    nodes: Vec<IUIAutomationElement>,
    visited: usize,
}

/// The one cross-process call, then a walk of the local cache.
fn read_tree(client: &IUIAutomation2, hwnd: HWND) -> Result<Walked, TreeError> {
    let request = cache_request(client).map_err(|err| {
        debug!("Command mode: building the cache request failed: {err}");
        TreeError::Failed("couldn't read that window's controls".into())
    })?;
    // SAFETY: `hwnd` was checked a moment ago; a stale one makes this fail.
    let root = unsafe { client.ElementFromHandleBuildCache(hwnd, &request) }.map_err(|err| {
        match err.code().0 as u32 {
            UIA_E_ELEMENTNOTAVAILABLE => TreeError::NotFound("that window".into()),
            UIA_E_TIMEOUT => TreeError::Timeout,
            code => {
                debug!("Command mode: reading a window's controls failed with {code:#010x}");
                TreeError::Failed("couldn't read that window's controls".into())
            }
        }
    })?;
    let (collector, nodes, visited) = walk(cached_children(&root), cached_children, read_facts);
    Ok(Walked {
        collector,
        nodes,
        visited,
    })
}

/// An element's children from the cache. None (an error, or a null array) is
/// the normal answer for a leaf.
fn cached_children(element: &IUIAutomationElement) -> Vec<IUIAutomationElement> {
    // SAFETY: cache-only reads on a live element; indexes stay within Length.
    unsafe {
        let Ok(children) = element.GetCachedChildren() else {
            return Vec::new();
        };
        let count = children.Length().unwrap_or(0).max(0);
        (0..count)
            .filter_map(|index| children.GetElement(index).ok())
            .collect()
    }
}

fn cached(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Option<VARIANT> {
    // SAFETY: answered from the local cache built by `read_tree`; it does not
    // reach the target app. An empty VARIANT means "not set".
    unsafe { element.GetCachedPropertyValue(property) }
        .ok()
        .filter(|value| !value.is_empty())
}

/// A boolean property; `None` when unset or the reserved "not supported"
/// value (which is not a VT_BOOL and fails the conversion).
fn cached_bool(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Option<bool> {
    cached(element, property).and_then(|value| bool::try_from(&value).ok())
}

fn cached_i32(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Option<i32> {
    cached(element, property).and_then(|value| i32::try_from(&value).ok())
}

fn cached_string(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Option<String> {
    cached(element, property)
        .and_then(|value| BSTR::try_from(&value).ok())
        .map(|text| text.to_string())
}

/// What the cache says about one element.
fn read_facts(element: &IUIAutomationElement) -> Facts {
    let flag = |property| cached_bool(element, property).unwrap_or(false);
    // SAFETY: cache-only reads, as in `cached`.
    let control_type = unsafe { element.CachedControlType() }
        .map(|t| t.0)
        .unwrap_or(0);
    let name = unsafe { element.CachedName() }
        .map(|text| text.to_string())
        .unwrap_or_default();
    let password = flag(UIA_IsPasswordPropertyId);
    let has_value = flag(UIA_IsValuePatternAvailablePropertyId);
    // A password field's value is never taken out of the cache.
    let value = if has_value && !password {
        cached_string(element, UIA_ValueValuePropertyId)
            .map(|text| cap_chars(&text, MAX_VALUE_CHARS))
    } else {
        None
    };
    let has_toggle = flag(UIA_IsTogglePatternAvailablePropertyId);
    let has_expand_collapse = flag(UIA_IsExpandCollapsePatternAvailablePropertyId);
    Facts {
        control_type,
        name,
        automation_id: cached_string(element, UIA_AutomationIdPropertyId).unwrap_or_default(),
        // Unknown counts as enabled: marking everything disabled when a
        // provider omits the property would hide every action.
        enabled: cached_bool(element, UIA_IsEnabledPropertyId).unwrap_or(true),
        focused: flag(UIA_HasKeyboardFocusPropertyId),
        offscreen: flag(UIA_IsOffscreenPropertyId),
        password,
        keyboard_focusable: flag(UIA_IsKeyboardFocusablePropertyId),
        value,
        value_read_only: has_value && flag(UIA_ValueIsReadOnlyPropertyId),
        toggle_state: has_toggle
            .then(|| cached_i32(element, UIA_ToggleToggleStatePropertyId))
            .flatten(),
        expand_state: has_expand_collapse
            .then(|| cached_i32(element, UIA_ExpandCollapseExpandCollapseStatePropertyId))
            .flatten(),
        selected: flag(UIA_SelectionItemIsSelectedPropertyId),
        has_invoke: flag(UIA_IsInvokePatternAvailablePropertyId),
        has_value,
        has_toggle,
        has_selection_item: flag(UIA_IsSelectionItemPatternAvailablePropertyId),
        has_expand_collapse,
        has_scroll_item: flag(UIA_IsScrollItemPatternAvailablePropertyId),
    }
}

fn cap_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text.to_string(),
    }
}

// ── The rules (pure) ───────────────────────────────────────────────────────

/// One element's cached values, as plain data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Facts {
    control_type: i32,
    name: String,
    automation_id: String,
    enabled: bool,
    focused: bool,
    offscreen: bool,
    password: bool,
    keyboard_focusable: bool,
    /// Never set for a password field.
    value: Option<String>,
    value_read_only: bool,
    toggle_state: Option<i32>,
    expand_state: Option<i32>,
    selected: bool,
    has_invoke: bool,
    has_value: bool,
    has_toggle: bool,
    has_selection_item: bool,
    has_expand_collapse: bool,
    has_scroll_item: bool,
}

/// What a kept element can do, kept beside it on the worker so `act` can
/// refuse with a useful sentence without another cross-process call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    role: &'static str,
    can: Can,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Can {
    enabled: bool,
    invoke: bool,
    set_value: bool,
    read_only: bool,
    toggle: bool,
    select: bool,
    expand: bool,
    scroll: bool,
    focus: bool,
}

/// The contract's role word for a UIA control type.
fn role_for(control_type: i32) -> &'static str {
    match control_type {
        BUTTON | SPLIT_BUTTON => "button",
        EDIT => "edit",
        HYPERLINK => "link",
        CHECK_BOX => "checkbox",
        RADIO_BUTTON => "radio",
        COMBO_BOX => "combobox",
        LIST_ITEM | DATA_ITEM => "list_item",
        MENU_ITEM => "menu_item",
        TAB_ITEM => "tab",
        TREE_ITEM => "tree_item",
        TEXT => "text",
        DOCUMENT => "document",
        SLIDER => "slider",
        GROUP | PANE | CUSTOM => "group",
        _ => "other",
    }
}

/// Control types whose whole subtree is chrome nobody acts on through the
/// agent: scroll bars and their thumbs, title bars (minimize, close), tooltips.
fn skips_subtree(control_type: i32) -> bool {
    matches!(control_type, SCROLL_BAR | THUMB | TITLE_BAR | TOOL_TIP)
}

fn is_field(role: &str) -> bool {
    matches!(role, "edit" | "document" | "combobox")
}

fn capabilities(f: &Facts) -> Can {
    Can {
        enabled: f.enabled,
        invoke: f.has_invoke,
        set_value: f.has_value && !f.value_read_only,
        read_only: f.has_value && f.value_read_only,
        toggle: f.has_toggle,
        select: f.has_selection_item,
        expand: f.has_expand_collapse && f.expand_state != Some(LEAF),
        scroll: f.has_scroll_item,
        focus: f.keyboard_focusable,
    }
}

/// The action words shown for an element. Nothing for a disabled one.
/// "scroll" and "focus" are offered by nearly every element of a web page, so
/// they are listed only when an element has nothing better (and "focus" for
/// fields); `act` still accepts them on any element that supports them.
fn actions(role: &str, can: &Can) -> Vec<String> {
    if !can.enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (has, word) in [
        (can.invoke, "invoke"),
        (can.set_value, "set_value"),
        (can.toggle, "toggle"),
        (can.select, "select"),
        (can.expand, "expand"),
    ] {
        if has {
            out.push(word.to_string());
        }
    }
    let primary = !out.is_empty();
    if can.scroll && !primary {
        out.push("scroll".into());
    }
    if can.focus && (!primary || is_field(role)) {
        out.push("focus".into());
    }
    out
}

/// The state words for an element (offscreen elements are never kept, so
/// "offscreen" never appears).
fn states(role: &str, f: &Facts) -> Vec<String> {
    let mut out = Vec::new();
    if !f.enabled {
        out.push("disabled");
    }
    if f.focused {
        out.push("focused");
    }
    if f.has_toggle {
        match f.toggle_state {
            Some(TOGGLE_ON) => out.push("checked"),
            Some(TOGGLE_OFF) => out.push("unchecked"),
            _ => {}
        }
    }
    if f.has_expand_collapse {
        match f.expand_state {
            Some(EXPANDED) | Some(PARTIALLY_EXPANDED) => out.push("expanded"),
            Some(COLLAPSED) => out.push("collapsed"),
            _ => {}
        }
    }
    if f.has_selection_item && f.selected {
        out.push("selected");
    }
    if f.has_value && f.value_read_only && (is_field(role) || role == "slider") {
        out.push("readonly");
    }
    if f.password {
        out.push("password");
    }
    out.into_iter().map(str::to_string).collect()
}

/// The value shown for an element: only for fields and sliders, never for a
/// password field, and never an empty one.
fn visible_value(role: &str, f: &Facts) -> Option<String> {
    if f.password || !(is_field(role) || role == "slider") {
        return None;
    }
    f.value.clone().filter(|value| !value.trim().is_empty())
}

/// The name shown: the accessible name, or, for an unnamed control that can be
/// acted on, an automation id that reads as words ("send_button"). That also
/// lets the confirmation rule see "send" on an unlabelled Send button.
fn display_name(f: &Facts, can: &Can) -> String {
    let name = f.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    let actionable = can.invoke || can.toggle || can.select || can.expand || can.set_value;
    if actionable && reads_as_words(&f.automation_id) {
        return f.automation_id.clone();
    }
    String::new()
}

/// An automation id worth showing: short, plain ASCII, mostly letters, with no
/// long digit run (generated ids are GUIDs and counters, not words).
fn reads_as_words(id: &str) -> bool {
    let len = id.chars().count();
    if !(2..=40).contains(&len) {
        return false;
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return false;
    }
    let letters = id.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let mut run = 0;
    let mut longest_digit_run = 0;
    for c in id.chars() {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        longest_digit_run = longest_digit_run.max(run);
    }
    letters * 2 >= len && longest_digit_run < 4
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// What happened to an offered element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Offer {
    Kept(u32),
    Dropped,
    /// Dropped with everything under it.
    SkipSubtree,
    /// The cap is reached; stop walking.
    Full,
}

/// The kept elements of one snapshot, with the filtering and capping rules.
#[derive(Debug, Default)]
struct Collector {
    elements: Vec<UiElement>,
    held: Vec<Held>,
    texts: usize,
    truncated: bool,
}

impl Collector {
    /// Keep or drop one element. `depth` is its nesting below the window in
    /// kept elements; `parent_name` is the nearest kept ancestor's name.
    fn offer(&mut self, f: &Facts, depth: u8, parent_name: Option<&str>) -> Offer {
        if skips_subtree(f.control_type) {
            return Offer::SkipSubtree;
        }
        if f.offscreen {
            return Offer::Dropped;
        }
        let role = role_for(f.control_type);
        let can = capabilities(f);
        let name = display_name(f, &can);
        let value = visible_value(role, f);
        if name.is_empty() && value.is_none() && !is_field(role) {
            return Offer::Dropped;
        }
        let text = role == "text";
        if text {
            // A link's or button's own label repeated as a text child says
            // nothing new.
            if name.is_empty() || parent_name.is_some_and(|parent| same_text(parent, &name)) {
                return Offer::Dropped;
            }
            if self.texts >= MAX_TEXTS {
                return Offer::Dropped;
            }
        }
        if self.elements.len() >= MAX_ELEMENTS {
            self.truncated = true;
            return Offer::Full;
        }
        if text {
            self.texts += 1;
        }
        let id = self.elements.len() as u32 + 1;
        self.elements.push(UiElement {
            id,
            role: role.to_string(),
            name,
            value,
            states: states(role, f),
            actions: actions(role, &can),
            depth,
        });
        self.held.push(Held { role, can });
        Offer::Kept(id)
    }
}

/// Walk a tree depth-first in document order, offering every node to a
/// [`Collector`]. Generic over the node type so the walk is tested on plain
/// data; the worker passes cached UIA elements. Returns the collector, the
/// kept nodes (parallel to its elements) and how many nodes were looked at.
fn walk<N>(
    roots: Vec<N>,
    mut children: impl FnMut(&N) -> Vec<N>,
    mut facts_of: impl FnMut(&N) -> Facts,
) -> (Collector, Vec<N>, usize) {
    struct Frame<N> {
        node: N,
        depth: u8,
        parent: Option<usize>,
    }
    let mut collector = Collector::default();
    let mut kept = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<Frame<N>> = roots
        .into_iter()
        .rev()
        .map(|node| Frame {
            node,
            depth: 1,
            parent: None,
        })
        .collect();
    while let Some(frame) = stack.pop() {
        if visited >= MAX_VISITED {
            collector.truncated = true;
            break;
        }
        visited += 1;
        let facts = facts_of(&frame.node);
        let parent_name = frame
            .parent
            .and_then(|index| collector.elements.get(index))
            .map(|element| element.name.clone());
        let offer = collector.offer(&facts, frame.depth, parent_name.as_deref());
        let (depth, parent, keep) = match offer {
            Offer::Full => break,
            Offer::SkipSubtree => continue,
            Offer::Dropped => (frame.depth, frame.parent, false),
            Offer::Kept(id) => (frame.depth.saturating_add(1), Some(id as usize - 1), true),
        };
        let below = children(&frame.node);
        if keep {
            kept.push(frame.node);
        }
        stack.extend(below.into_iter().rev().map(|node| Frame {
            node,
            depth,
            parent,
        }));
    }
    (collector, kept, visited)
}

/// Why `action` cannot be done to a held element, before any call is made.
fn refusal(id: u32, held: &Held, action: &UiAction) -> Option<String> {
    let can = &held.can;
    if !can.enabled {
        return Some(format!("element {id} is disabled"));
    }
    let able = match action {
        UiAction::Invoke => can.invoke,
        UiAction::SetValue(_) => {
            if can.read_only {
                return Some(format!(
                    "element {id} is read-only; its text can't be changed"
                ));
            }
            can.set_value
        }
        UiAction::Toggle => can.toggle,
        UiAction::Select => can.select,
        UiAction::Expand | UiAction::Collapse => can.expand,
        UiAction::ScrollIntoView => can.scroll,
        UiAction::Focus => can.focus,
    };
    (!able).then(|| cannot(id, held, action))
}

/// "element 14 (text) can't invoke; it can only be read" and the like.
fn cannot(id: u32, held: &Held, action: &UiAction) -> String {
    let can = &held.can;
    let options: Vec<&str> = [
        (can.invoke, "invoke"),
        (can.set_value, "set_value"),
        (can.toggle, "toggle"),
        (can.select, "select"),
        (can.expand, "expand"),
        (can.scroll, "scroll"),
        (can.focus, "focus"),
    ]
    .into_iter()
    .filter_map(|(has, word)| has.then_some(word))
    .collect();
    let role = held.role;
    let label = action.label();
    if options.is_empty() {
        format!("element {id} ({role}) can't {label}; it can only be read")
    } else {
        format!(
            "element {id} ({role}) can't {label}; it can do: {}",
            options.join(", ")
        )
    }
}

fn verb(action: &UiAction) -> &'static str {
    match action {
        UiAction::Invoke => "be invoked",
        UiAction::SetValue(_) => "take that text",
        UiAction::Toggle => "be toggled",
        UiAction::Select => "be selected",
        UiAction::Expand => "be expanded",
        UiAction::Collapse => "be collapsed",
        UiAction::ScrollIntoView => "be scrolled into view",
        UiAction::Focus => "take focus",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(control_type: i32, name: &str) -> Facts {
        Facts {
            control_type,
            name: name.into(),
            enabled: true,
            ..Facts::default()
        }
    }

    fn button(name: &str) -> Facts {
        Facts {
            has_invoke: true,
            keyboard_focusable: true,
            ..facts(BUTTON, name)
        }
    }

    fn edit(name: &str, value: &str) -> Facts {
        Facts {
            has_value: true,
            value: Some(value.into()),
            keyboard_focusable: true,
            ..facts(EDIT, name)
        }
    }

    /// The model is taught a fixed set of role words; every common UIA
    /// control type must land on one of them.
    #[test]
    fn control_types_map_to_the_contracts_role_words() {
        let cases = [
            (BUTTON, "button"),
            (SPLIT_BUTTON, "button"),
            (EDIT, "edit"),
            (HYPERLINK, "link"),
            (CHECK_BOX, "checkbox"),
            (RADIO_BUTTON, "radio"),
            (COMBO_BOX, "combobox"),
            (LIST_ITEM, "list_item"),
            (DATA_ITEM, "list_item"),
            (MENU_ITEM, "menu_item"),
            (TAB_ITEM, "tab"),
            (TREE_ITEM, "tree_item"),
            (TEXT, "text"),
            (DOCUMENT, "document"),
            (SLIDER, "slider"),
            (GROUP, "group"),
            (PANE, "group"),
            (CUSTOM, "group"),
            (SCROLL_BAR, "other"),
            (0, "other"),
        ];
        for (control_type, role) in cases {
            assert_eq!(role_for(control_type), role, "{control_type}");
        }
    }

    /// A checked checkbox must read as checked and toggleable, so the model
    /// does not toggle it off while trying to turn it on.
    #[test]
    fn a_checkbox_reports_its_state_and_toggle() {
        let f = Facts {
            has_toggle: true,
            toggle_state: Some(TOGGLE_ON),
            focused: true,
            keyboard_focusable: true,
            ..facts(CHECK_BOX, "Remember me")
        };
        let can = capabilities(&f);
        assert_eq!(states("checkbox", &f), vec!["focused", "checked"]);
        assert_eq!(actions("checkbox", &can), vec!["toggle"]);
        let off = Facts {
            toggle_state: Some(TOGGLE_OFF),
            ..f
        };
        assert_eq!(states("checkbox", &off), vec!["focused", "unchecked"]);
    }

    /// Expand state is shown, and a leaf node that reports the pattern is not
    /// offered "expand", since expanding it does nothing.
    #[test]
    fn expand_collapse_states_and_leaf_nodes() {
        let f = Facts {
            has_expand_collapse: true,
            expand_state: Some(COLLAPSED),
            has_selection_item: true,
            selected: true,
            ..facts(TREE_ITEM, "Inbox")
        };
        assert_eq!(states("tree_item", &f), vec!["collapsed", "selected"]);
        assert_eq!(
            actions("tree_item", &capabilities(&f)),
            vec!["select", "expand"]
        );
        let leaf = Facts {
            expand_state: Some(LEAF),
            ..f
        };
        assert!(states("tree_item", &leaf)
            .iter()
            .all(|s| s != "expanded" && s != "collapsed"));
        assert_eq!(actions("tree_item", &capabilities(&leaf)), vec!["select"]);
    }

    /// A disabled control offers nothing, and says why.
    #[test]
    fn a_disabled_control_offers_no_actions() {
        let f = Facts {
            enabled: false,
            ..button("Next")
        };
        let can = capabilities(&f);
        assert!(actions("button", &can).is_empty());
        assert_eq!(states("button", &f), vec!["disabled"]);
        let held = Held {
            role: "button",
            can,
        };
        assert_eq!(
            refusal(3, &held, &UiAction::Invoke),
            Some("element 3 is disabled".into())
        );
    }

    /// Focus and scroll are on nearly every web element; listing them on every
    /// button doubles the tokens for nothing, but fields keep focus.
    #[test]
    fn secondary_actions_are_listed_only_when_nothing_better_exists() {
        let f = Facts {
            has_scroll_item: true,
            ..button("Bold")
        };
        assert_eq!(actions("button", &capabilities(&f)), vec!["invoke"]);
        assert_eq!(
            actions("edit", &capabilities(&edit("To", ""))),
            vec!["set_value", "focus"]
        );
        let bare = Facts {
            keyboard_focusable: true,
            has_scroll_item: true,
            ..facts(GROUP, "Sidebar")
        };
        assert_eq!(
            actions("group", &capabilities(&bare)),
            vec!["scroll", "focus"]
        );
        // Not listed, but still allowed.
        let held = Held {
            role: "button",
            can: capabilities(&f),
        };
        assert_eq!(refusal(1, &held, &UiAction::Focus), None);
    }

    /// A read-only field is marked, and setting its value is refused with a
    /// reason rather than attempted.
    #[test]
    fn a_read_only_field_is_marked_and_refuses_set_value() {
        let f = Facts {
            value_read_only: true,
            ..edit("Account number", "12345")
        };
        let can = capabilities(&f);
        assert!(states("edit", &f).contains(&"readonly".to_string()));
        assert!(!actions("edit", &can).contains(&"set_value".to_string()));
        let held = Held { role: "edit", can };
        let why = refusal(7, &held, &UiAction::SetValue("x".into())).unwrap_or_default();
        assert!(why.contains("read-only"), "{why}");
    }

    /// The refusal names what the element can do instead, so the model can
    /// recover in one step.
    #[test]
    fn a_missing_pattern_is_refused_with_the_alternatives() {
        let held = Held {
            role: "link",
            can: capabilities(&Facts {
                keyboard_focusable: true,
                ..button("Help")
            }),
        };
        let why = refusal(9, &held, &UiAction::Toggle).unwrap_or_default();
        assert_eq!(
            why,
            "element 9 (link) can't toggle; it can do: invoke, focus"
        );
        let label = Held {
            role: "text",
            can: capabilities(&facts(TEXT, "Subject")),
        };
        let why = refusal(2, &label, &UiAction::Invoke).unwrap_or_default();
        assert!(why.ends_with("it can only be read"), "{why}");
    }

    /// A password must never reach the model, the log or a snapshot, even if
    /// a provider were to put it in the cache.
    #[test]
    fn password_values_are_never_surfaced() {
        let f = Facts {
            password: true,
            value: Some("hunter2".into()),
            ..edit("Password", "")
        };
        assert_eq!(visible_value("edit", &f), None);
        assert!(states("edit", &f).contains(&"password".to_string()));
        let mut collector = Collector::default();
        assert_eq!(collector.offer(&f, 1, None), Offer::Kept(1));
        let element = &collector.elements[0];
        assert_eq!(element.value, None);
        assert!(!format!("{element:?}").contains("hunter2"));
    }

    /// Values are shown for fields only; a link's value (its URL in Chromium)
    /// is noise.
    #[test]
    fn values_are_shown_for_fields_only() {
        assert_eq!(
            visible_value("edit", &edit("To", "ron@example.com")),
            Some("ron@example.com".into())
        );
        assert_eq!(visible_value("edit", &edit("To", "  ")), None);
        let link = Facts {
            has_value: true,
            value: Some("https://example.com".into()),
            ..facts(HYPERLINK, "Example")
        };
        assert_eq!(visible_value("link", &link), None);
    }

    /// Offscreen controls, unnamed non-fields and scroll bars are dropped;
    /// an unnamed edit is kept because it is somewhere to type.
    #[test]
    fn filtering_drops_what_cannot_be_used() {
        let mut c = Collector::default();
        let offscreen = Facts {
            offscreen: true,
            ..button("Hidden")
        };
        assert_eq!(c.offer(&offscreen, 1, None), Offer::Dropped);
        assert_eq!(c.offer(&facts(GROUP, ""), 1, None), Offer::Dropped);
        assert_eq!(c.offer(&button(""), 1, None), Offer::Dropped);
        assert_eq!(
            c.offer(&facts(SCROLL_BAR, "Vertical"), 1, None),
            Offer::SkipSubtree
        );
        assert_eq!(c.offer(&edit("", ""), 1, None), Offer::Kept(1));
        assert_eq!(c.offer(&button("Send"), 1, None), Offer::Kept(2));
        assert_eq!(c.elements.len(), 2);
    }

    /// An unnamed button with a readable automation id gets that as its name,
    /// so the confirmation rule can still see "send"; a generated id does not.
    #[test]
    fn readable_automation_ids_name_unlabelled_controls() {
        let mut f = button("");
        f.automation_id = "send_button".into();
        assert_eq!(display_name(&f, &capabilities(&f)), "send_button");
        f.automation_id = "a1b2c3d4-0000-1111-2222-333344445555".into();
        assert_eq!(display_name(&f, &capabilities(&f)), "");
        f.automation_id = "item12345".into();
        assert_eq!(display_name(&f, &capabilities(&f)), "");
    }

    /// Text that repeats its parent's label is dropped, and at most
    /// MAX_TEXTS text elements are kept so prose cannot fill the budget.
    #[test]
    fn text_is_kept_only_when_it_adds_something_and_is_capped() {
        let mut c = Collector::default();
        assert_eq!(c.offer(&facts(TEXT, ""), 2, Some("Inbox")), Offer::Dropped);
        assert_eq!(
            c.offer(&facts(TEXT, "inbox"), 2, Some("Inbox")),
            Offer::Dropped
        );
        for n in 0..MAX_TEXTS {
            assert!(matches!(
                c.offer(&facts(TEXT, &format!("line {n}")), 1, None),
                Offer::Kept(_)
            ));
        }
        assert_eq!(c.offer(&facts(TEXT, "one more"), 1, None), Offer::Dropped);
        assert!(!c.truncated);
        assert!(matches!(c.offer(&button("Send"), 1, None), Offer::Kept(_)));
    }

    /// The element cap stops the walk and marks the snapshot as cut.
    #[test]
    fn the_element_cap_truncates() {
        let mut c = Collector::default();
        for n in 0..MAX_ELEMENTS {
            assert!(matches!(
                c.offer(&button(&format!("b{n}")), 1, None),
                Offer::Kept(_)
            ));
        }
        assert_eq!(c.offer(&button("late"), 1, None), Offer::Full);
        assert!(c.truncated);
        assert_eq!(c.elements.len(), MAX_ELEMENTS);
        assert_eq!(c.elements.last().map(|e| e.id), Some(MAX_ELEMENTS as u32));
    }

    struct Node {
        facts: Facts,
        children: Vec<usize>,
    }

    /// Depth counts kept ancestors only, so an unnamed wrapper pane does not
    /// indent its contents; ids follow document order; kept nodes line up
    /// with their elements.
    #[test]
    fn the_walk_numbers_in_document_order_and_nests_by_kept_ancestors() {
        // 0 pane "" -> [1 group "Compose" -> [2 edit "To", 3 button "Send" -> [4 text "Send"]], 5 link "Help"]
        let tree = vec![
            Node {
                facts: facts(PANE, ""),
                children: vec![1, 5],
            },
            Node {
                facts: facts(GROUP, "Compose"),
                children: vec![2, 3],
            },
            Node {
                facts: edit("To", ""),
                children: vec![],
            },
            Node {
                facts: button("Send"),
                children: vec![4],
            },
            Node {
                facts: facts(TEXT, "Send"),
                children: vec![],
            },
            Node {
                facts: Facts {
                    has_invoke: true,
                    ..facts(HYPERLINK, "Help")
                },
                children: vec![],
            },
        ];
        let (c, kept, visited) = walk(
            vec![0usize],
            |n| tree[*n].children.clone(),
            |n| tree[*n].facts.clone(),
        );
        assert_eq!(visited, 6);
        let summary: Vec<(u32, &str, &str, u8)> = c
            .elements
            .iter()
            .map(|e| (e.id, e.role.as_str(), e.name.as_str(), e.depth))
            .collect();
        assert_eq!(
            summary,
            vec![
                (1, "group", "Compose", 1),
                (2, "edit", "To", 2),
                (3, "button", "Send", 2),
                (4, "link", "Help", 1),
            ]
        );
        assert_eq!(kept, vec![1, 2, 3, 5]);
        assert_eq!(c.held.len(), c.elements.len());
    }

    /// A skipped subtree (a title bar) hides its buttons too.
    #[test]
    fn the_walk_skips_title_bars_entirely() {
        let tree = vec![
            Node {
                facts: facts(TITLE_BAR, "Title"),
                children: vec![1],
            },
            Node {
                facts: button("Close"),
                children: vec![],
            },
            Node {
                facts: button("Save"),
                children: vec![],
            },
        ];
        let (c, kept, _) = walk(
            vec![0usize, 2],
            |n| tree[*n].children.clone(),
            |n| tree[*n].facts.clone(),
        );
        assert_eq!(c.elements.len(), 1);
        assert_eq!(c.elements[0].name, "Save");
        assert_eq!(kept, vec![2]);
    }

    /// Live: snapshot whatever window is in front (the terminal running the
    /// test, usually) and print it as the model would read it.
    #[test]
    #[ignore]
    fn uia_live_reads_the_foreground_window() {
        let tree = UiaTree::new();
        match tree.snapshot(None) {
            Ok(snapshot) => eprintln!("{}", super::super::tree::render(&snapshot)),
            Err(err) => panic!("snapshot failed: {err}"),
        }
    }
}
