//! Windows: Windows.Graphics.Capture, Direct3D 11 and Media Foundation H.264.
//!
//! One thread per stream owns everything: COM (MTA) and Media Foundation are
//! started on it, and the Direct3D device, the capture, the colour converter
//! and the encoder all live and die there, because none of their interfaces
//! may be used from another thread without care, and a stream that owns its
//! own thread can always be ended by ending that thread. The handle the
//! session holds ([`WindowsStream`]) only sends it commands.
//!
//! The thread sleeps until something happens: the capture saying a new frame
//! is ready (Windows only composes one when the picture changed), a command,
//! or the next pacing deadline (a frame held back by the rate limit, or the
//! keepalive). So a still window costs one encode every keepalive.
//!
//! What a frame goes through:
//!
//! 1. The newest captured frame is copied into a texture of our own
//!    ([`d3d::Source`]), so the capture's buffer goes straight back and the
//!    picture can be encoded again later (keepalive, a keyframe request).
//! 2. The video processor scales it and converts it to NV12 at the output
//!    size ([`d3d::Converter`]).
//! 3. The encoder turns it into one access unit ([`encoder::Encoder`]);
//!    [`annexb::ParamSets`] makes sure a keyframe carries its SPS and PPS.
//! 4. It goes into the frame channel with `try_send`: a consumer that is
//!    behind loses the frame, and the next one is a keyframe so the phone's
//!    decoder recovers.
//!
//! A window that changes size gets a new capture buffer at once, but a new
//! encoder only once its size has held still for [`RESIZE_SETTLE`]: dragging
//! a window's edge changes the size on every frame, and an encoder costs tens
//! of milliseconds to make. Meanwhile the picture is letterboxed into the old
//! size.

mod annexb;
mod capture;
mod d3d;
mod encoder;
#[cfg(test)]
mod live_tests;
mod pacing;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self as std_mpsc, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use tokio::sync::mpsc;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_LITE, MF_VERSION};
use windows::Win32::System::Com::{
    CoIncrementMTAUsage, CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow, MONITORINFOF_PRIMARY};

use self::annexb::{is_keyframe, ParamSets};
use self::capture::Capture;
use self::d3d::{Converter, Gpu, Source};
use self::encoder::{Config, Encoder, Preference};
use self::pacing::{output_size, Pacer, Step};
use super::{
    CaptureTarget, EncodedFrame, Ended, Geometry, ScreenInfo, StartError, VideoSettings,
    VideoStream,
};

/// How long a window's new size must hold before the encoder is made again
/// at it.
const RESIZE_SETTLE: Duration = Duration::from_millis(300);
/// Frames waiting for the consumer. A few, so a short stall on the network
/// side does not cost a keyframe, but not so many that the phone sees old
/// pictures.
const CHANNEL_FRAMES: usize = 4;
/// How long `start` waits for the thread to have the capture and the encoder
/// running.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// How long dropping a stream waits for its thread to release everything.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
/// How often the captured window or monitor is checked for still existing.
/// The capture's `Closed` event did not arrive for a destroyed window in
/// testing, so this is what ends the stream when the window closes.
const LIVENESS_CHECK: Duration = Duration::from_millis(250);
/// A requested keyframe that has not come after this many frames gets a new
/// encoder, which always starts with one.
const KEYFRAME_RETRIES: u32 = 3;

pub fn list_screens() -> Vec<ScreenInfo> {
    let mut monitors = monitors();
    // Primary first, then left to right, top to bottom, as a person numbers
    // them.
    monitors.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
    monitors
        .into_iter()
        .enumerate()
        .map(|(index, m)| ScreenInfo {
            id: m.device,
            name: format!("Display {}", index + 1),
            width: (m.rect.right - m.rect.left).max(0) as u32,
            height: (m.rect.bottom - m.rect.top).max(0) as u32,
            primary: m.primary,
        })
        .collect()
}

pub fn start(
    target: CaptureTarget,
    settings: VideoSettings,
) -> Result<(Box<dyn VideoStream>, mpsc::Receiver<EncodedFrame>), StartError> {
    start_with(target, settings, Preference::Auto)
}

/// What the stream captures, as the geometry query needs it. Both handles are
/// kept as integers: they are only ever passed back to the system, which
/// checks them.
#[derive(Clone, Copy, Debug)]
enum Place {
    Window(isize),
    Monitor(isize),
}

impl Place {
    fn hwnd(raw: isize) -> HWND {
        HWND(raw as *mut _)
    }
}

fn start_with(
    target: CaptureTarget,
    settings: VideoSettings,
    preference: Preference,
) -> Result<(Box<dyn VideoStream>, mpsc::Receiver<EncodedFrame>), StartError> {
    let place = match &target {
        CaptureTarget::Window(id) => {
            let raw = *id as isize;
            // SAFETY: IsWindow accepts any value and answers false for one
            // that is not a live window.
            if raw == 0 || !unsafe { IsWindow(Some(Place::hwnd(raw))) }.as_bool() {
                return Err(StartError::TargetGone);
            }
            Place::Window(raw)
        }
        CaptureTarget::Screen(name) => match monitors().into_iter().find(|m| &m.device == name) {
            Some(m) => Place::Monitor(m.handle),
            None => return Err(StartError::TargetGone),
        },
    };

    let (commands, inbox) = std_mpsc::channel::<Command>();
    let (frames, frames_rx) = mpsc::channel(CHANNEL_FRAMES);
    let (ready_tx, ready_rx) = std_mpsc::sync_channel::<Result<(), StartError>>(1);
    let (done_tx, done_rx) = std_mpsc::channel::<()>();
    let ended = Arc::new(Mutex::new(None));
    let stopped = Arc::new(AtomicBool::new(false));

    let thread_ended = Arc::clone(&ended);
    let thread_commands = commands.clone();
    std::thread::Builder::new()
        .name("live-view-video".into())
        .spawn(move || {
            run(
                place,
                settings,
                preference,
                inbox,
                thread_commands,
                frames,
                thread_ended,
                ready_tx,
            );
            drop(done_tx);
        })
        .map_err(|err| StartError::Failed(format!("no thread: {err}")))?;

    match ready_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            let _ = commands.send(Command::Stop);
            return Err(StartError::Failed(
                "the capture did not start in time".into(),
            ));
        }
    }
    let stream = WindowsStream {
        commands: Mutex::new(commands),
        place,
        ended,
        stopped,
        done: Mutex::new(Some(done_rx)),
    };
    Ok((Box::new(stream), frames_rx))
}

/// What the stream's thread is told.
enum Command {
    /// The capture has a new frame (from the capture's own thread).
    Frame,
    /// The window closed or the monitor went away (likewise).
    Closed,
    Keyframe,
    #[allow(dead_code)] // sent by `set_bitrate`, which nothing calls yet
    Bitrate(u32),
    Stop,
}

/// The session's handle on a running stream.
struct WindowsStream {
    // `Sender` is `Sync`, but the mutex keeps that from depending on the
    // toolchain.
    commands: Mutex<Sender<Command>>,
    place: Place,
    ended: Arc<Mutex<Option<Ended>>>,
    stopped: Arc<AtomicBool>,
    done: Mutex<Option<Receiver<()>>>,
}

impl WindowsStream {
    fn send(&self, command: Command) {
        if let Ok(commands) = self.commands.lock() {
            let _ = commands.send(command);
        }
    }
}

impl VideoStream for WindowsStream {
    fn request_keyframe(&self) {
        self.send(Command::Keyframe);
    }

    fn set_bitrate(&self, bps: u32) {
        self.send(Command::Bitrate(bps));
    }

    fn geometry(&self) -> Option<Geometry> {
        match self.place {
            Place::Window(raw) => window_geometry(Place::hwnd(raw)),
            Place::Monitor(raw) => monitor_rect(HMONITOR(raw as *mut _)).map(rect_geometry),
        }
    }

    fn ended(&self) -> Option<Ended> {
        self.ended.lock().ok().and_then(|ended| ended.clone())
    }

    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            self.send(Command::Stop);
        }
    }
}

impl Drop for WindowsStream {
    fn drop(&mut self) {
        self.stop();
        // Wait (bounded) for the thread to release the capture, so the border
        // is gone by the time the stream is: a session ending and starting
        // another must not briefly show two.
        if let Some(done) = self.done.lock().ok().and_then(|mut done| done.take()) {
            if let Err(RecvTimeoutError::Timeout) = done.recv_timeout(STOP_TIMEOUT) {
                warn!("live view: the capture thread did not stop in time");
            }
        }
    }
}

fn rect_geometry(rect: RECT) -> Geometry {
    Geometry {
        left: rect.left,
        top: rect.top,
        width: (rect.right - rect.left).max(0) as u32,
        height: (rect.bottom - rect.top).max(0) as u32,
    }
}

/// Where a window is: its extended frame bounds, which is what
/// Windows.Graphics.Capture captures (the visible frame, without the
/// invisible resize borders `GetWindowRect` includes).
fn window_geometry(hwnd: HWND) -> Option<Geometry> {
    // SAFETY: both answer false for a handle that is not a live window.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let mut rect = RECT::default();
    // SAFETY: the out-pointer is a live RECT and its size is passed with it.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .ok()?;
    let geometry = rect_geometry(rect);
    (geometry.width > 0 && geometry.height > 0).then_some(geometry)
}

fn monitor_rect(monitor: HMONITOR) -> Option<RECT> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `cbSize` is set; a stale handle fails the call.
    unsafe { GetMonitorInfoW(monitor, &mut info) }
        .as_bool()
        .then_some(info.rcMonitor)
}

struct Monitor {
    handle: isize,
    device: String,
    rect: RECT,
    primary: bool,
}

fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the Vec passed to EnumDisplayMonitors below,
        // alive for the whole call that invokes this.
        let found = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: the extended struct is passed with its own size, which
        // tells the call it may write the device name.
        let info_ptr = &mut info as *mut MONITORINFOEXW as *mut MONITORINFO;
        if unsafe { GetMonitorInfoW(monitor, info_ptr) }.as_bool() {
            let len = info
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.szDevice.len());
            found.push(Monitor {
                handle: monitor.0 as isize,
                device: String::from_utf16_lossy(&info.szDevice[..len]),
                rect: info.monitorInfo.rcMonitor,
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        BOOL(1)
    }
    let mut found: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only runs during this call, and the pointer it
    // gets is to `found`, which outlives it.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut found as *mut Vec<Monitor> as isize),
        );
    }
    found
}

/// COM and Media Foundation for the stream's thread, undone when it ends.
struct ThreadSetup {
    com: bool,
    mf: bool,
}

/// Keeps the process's multithreaded apartment alive for good, from the first
/// stream on.
///
/// `windows` caches WinRT activation factories process-wide (the capture
/// session's, the frame pool's). When the last thread in the MTA calls
/// `CoUninitialize`, which is this thread when a stream ends and nothing else
/// in the app happens to be in the MTA, COM tears the apartment down and those
/// cached factories dangle: the next stream crashed in
/// `GraphicsCaptureSession::IsSupported`. The cookie is never released.
fn keep_mta_alive() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: takes a process-wide MTA reference; the cookie is
        // deliberately leaked.
        if let Err(err) = unsafe { CoIncrementMTAUsage() } {
            warn!("live view: could not pin the MTA: {err}");
        }
    });
}

impl ThreadSetup {
    fn new() -> Self {
        keep_mta_alive();
        // SAFETY: initialises COM for this thread only; balanced in Drop.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        // SAFETY: as above, for Media Foundation.
        let mf = unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.is_ok();
        Self { com, mf }
    }
}

impl Drop for ThreadSetup {
    fn drop(&mut self) {
        // SAFETY: each undoes the matching successful call in `new`.
        unsafe {
            if self.mf {
                let _ = MFShutdown();
            }
            if self.com {
                CoUninitialize();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    place: Place,
    settings: VideoSettings,
    preference: Preference,
    inbox: Receiver<Command>,
    commands: Sender<Command>,
    frames: mpsc::Sender<EncodedFrame>,
    ended: Arc<Mutex<Option<Ended>>>,
    ready: std_mpsc::SyncSender<Result<(), StartError>>,
) {
    let setup = ThreadSetup::new();
    if !setup.mf {
        let _ = ready.send(Err(StartError::Unsupported(
            "Media Foundation is not available".into(),
        )));
        return;
    }
    let mut worker = match Worker::new(place, settings, preference, commands, frames) {
        Ok(worker) => worker,
        Err(err) => {
            info!("live view: could not start: {err}");
            let _ = ready.send(Err(err));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let reason = worker.run(&inbox);
    match &reason {
        Some(Ended::TargetGone) => info!("live view: the captured window or screen went away"),
        Some(Ended::Failed(why)) => warn!("live view: capture failed: {why}"),
        None => debug!("live view: capture stopped"),
    }
    if let Ok(mut slot) = ended.lock() {
        *slot = reason;
    }
    // The capture, the encoder and the frame sender go here, before Media
    // Foundation and COM are shut down; the receiver then sees the channel
    // close with `ended` already set.
    drop(worker);
    drop(setup);
}

struct Worker {
    place: Place,
    settings: VideoSettings,
    preference: Preference,
    gpu: Gpu,
    capture: Capture,
    converter: Converter,
    encoder: Encoder,
    params: ParamSets,
    pacer: Pacer,
    frames: mpsc::Sender<EncodedFrame>,
    started: Instant,
    last_time: Option<Duration>,
    last_sent_time: Option<Duration>,
    source: Option<Source>,
    content: (u32, u32),
    content_changed: Instant,
    force_keyframe: bool,
    /// A frame was dropped: only a keyframe may go out next.
    resync: bool,
    keyframe_misses: u32,
    sent: u64,
}

enum Exit {
    Stop,
    Ended(Ended),
}

impl Worker {
    fn new(
        place: Place,
        settings: VideoSettings,
        preference: Preference,
        commands: Sender<Command>,
        frames: mpsc::Sender<EncodedFrame>,
    ) -> Result<Self, StartError> {
        if !capture::is_supported() {
            return Err(StartError::Unsupported(
                "Windows.Graphics.Capture is not available".into(),
            ));
        }
        let gpu = Gpu::new()
            .map_err(|e| StartError::Unsupported(format!("no Direct3D 11 video device: {e}")))?;
        let item = match place {
            Place::Window(raw) => capture::item_for_window(Place::hwnd(raw)).map_err(|e| {
                // SAFETY: IsWindow accepts any value.
                if unsafe { IsWindow(Some(Place::hwnd(raw))) }.as_bool() {
                    StartError::Failed(format!("this window cannot be captured: {e}"))
                } else {
                    StartError::TargetGone
                }
            })?,
            Place::Monitor(raw) => capture::item_for_monitor(HMONITOR(raw as *mut _))
                .map_err(|_| StartError::TargetGone)?,
        };
        let size = item
            .Size()
            .map_err(|e| StartError::Failed(format!("no capture size: {e}")))?;
        let content = (size.Width.max(2) as u32, size.Height.max(2) as u32);
        let (width, height) = output_size(content.0, content.1, settings.max_long_side);
        let encoder = Encoder::create(
            &gpu,
            Config {
                width,
                height,
                fps: settings.max_fps.max(1),
                bitrate: settings.bitrate_bps,
            },
            preference,
        )
        .map_err(StartError::Unsupported)?;
        let on_frame = commands.clone();
        let on_closed = commands;
        let capture = Capture::start(
            &gpu,
            item,
            settings.cursor,
            settings.max_fps,
            move || {
                let _ = on_frame.send(Command::Frame);
            },
            move || {
                let _ = on_closed.send(Command::Closed);
            },
        )
        .map_err(|e| StartError::Failed(format!("capture did not start: {e}")))?;
        let mut params = ParamSets::default();
        if let Some(header) = encoder.sequence_header() {
            params.learn_header(&header);
        }
        let started = Instant::now();
        Ok(Self {
            place,
            pacer: Pacer::new(settings.max_fps, settings.keepalive, started),
            settings,
            preference,
            gpu,
            capture,
            converter: Converter::new(width, height),
            encoder,
            params,
            frames,
            started,
            last_time: None,
            last_sent_time: None,
            source: None,
            content,
            content_changed: started,
            force_keyframe: true,
            resync: false,
            keyframe_misses: 0,
            sent: 0,
        })
    }

    fn run(&mut self, inbox: &Receiver<Command>) -> Option<Ended> {
        loop {
            // Everything already waiting first, so a stop is never behind a
            // frame.
            while let Ok(command) = inbox.try_recv() {
                if let Some(exit) = self.handle(command) {
                    return self.finish(exit);
                }
            }
            let now = Instant::now();
            let step = self.pacer.next(now);
            let result = match step {
                Step::EncodeNew => self.encode_new(now),
                Step::Repeat => self.encode(now, false),
                Step::Wait(until) => {
                    let wait = until.saturating_duration_since(now).min(LIVENESS_CHECK);
                    match inbox.recv_timeout(wait) {
                        Ok(command) => match self.handle(command) {
                            Some(exit) => return self.finish(exit),
                            None => Ok(()),
                        },
                        Err(RecvTimeoutError::Timeout) => Ok(()),
                        // Cannot happen while the capture's handlers hold a
                        // sender; treat it as a stop.
                        Err(RecvTimeoutError::Disconnected) => Err(Exit::Stop),
                    }
                }
            };
            if let Err(exit) = result {
                return self.finish(exit);
            }
            if !self.target_alive() {
                return self.finish(Exit::Ended(Ended::TargetGone));
            }
        }
    }

    fn target_alive(&self) -> bool {
        match self.place {
            // SAFETY: IsWindow accepts any value.
            Place::Window(raw) => unsafe { IsWindow(Some(Place::hwnd(raw))) }.as_bool(),
            Place::Monitor(raw) => monitor_rect(HMONITOR(raw as *mut _)).is_some(),
        }
    }

    fn finish(&self, exit: Exit) -> Option<Ended> {
        debug!(
            "live view: {} frames sent in {:.1} s",
            self.sent,
            self.started.elapsed().as_secs_f64()
        );
        match exit {
            Exit::Stop => None,
            Exit::Ended(ended) => Some(ended),
        }
    }

    fn handle(&mut self, command: Command) -> Option<Exit> {
        match command {
            Command::Frame => self.pacer.frame_arrived(),
            Command::Closed => return Some(Exit::Ended(Ended::TargetGone)),
            Command::Keyframe => {
                self.force_keyframe = true;
                self.pacer.want_repeat();
            }
            Command::Bitrate(bps) => {
                self.settings.bitrate_bps = bps;
                self.encoder.set_bitrate(bps);
            }
            Command::Stop => return Some(Exit::Stop),
        }
        None
    }

    fn encode_new(&mut self, now: Instant) -> Result<(), Exit> {
        let frame = match self.capture.latest(&self.gpu) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                self.pacer.nothing_new();
                return Ok(());
            }
            Err(err) => return Err(failed("reading the captured frame", err)),
        };
        let (width, height) = d3d::texture_size(&frame.texture);
        let reuse = self
            .source
            .as_ref()
            .is_some_and(|s| s.width == width && s.height == height);
        if !reuse {
            self.source = Some(
                Source::new(&self.gpu, width, height).map_err(|e| failed("a frame texture", e))?,
            );
        }
        if let Some(source) = &mut self.source {
            // SAFETY: both textures are live and the same size and format.
            unsafe {
                self.gpu
                    .context
                    .CopyResource(&source.texture, &frame.texture)
            };
            source.content = frame.content;
        }
        drop(frame);
        if self.content != self.source.as_ref().map_or(self.content, |s| s.content) {
            self.content = self.source.as_ref().map_or(self.content, |s| s.content);
            self.content_changed = now;
        }
        self.encode(now, true)
    }

    /// Encode the current picture (`fresh` when it is a new capture).
    fn encode(&mut self, now: Instant, fresh: bool) -> Result<(), Exit> {
        self.follow_size(now)?;
        if self.source.is_none() {
            self.source = Some(Source::black(&self.gpu).map_err(|e| failed("a black picture", e))?);
        }
        let Some(source) = &self.source else {
            return Ok(());
        };
        let texture = self
            .converter
            .convert(&self.gpu, source)
            .map_err(|e| failed("converting the picture", e))?;
        let time = self.next_time(now);
        let keyframe = self.force_keyframe;
        let outputs = match self
            .encoder
            .encode(&self.gpu, &texture, to_100ns(time), keyframe)
        {
            Ok(outputs) => outputs,
            Err(err) => {
                // A hardware encoder that fails mid-stream (a driver reset,
                // a GPU that went to sleep) gets one replacement; the
                // software encoder is the fallback.
                warn!(
                    "live view: {} failed: {err}; starting another encoder",
                    self.encoder.name
                );
                let preference = if self.encoder.is_hardware() {
                    Preference::SoftwareOnly
                } else {
                    self.preference
                };
                self.rebuild_encoder(self.converter.size(), preference)?;
                self.encoder
                    .encode(&self.gpu, &texture, to_100ns(time), true)
                    .map_err(|e| failed("encoding", e))?
            }
        };
        let mut sent_keyframe = false;
        let mut got_keyframe = false;
        let mut dropped = false;
        for output in outputs {
            let data = self.params.complete(output.data);
            let is_key = is_keyframe(&data);
            got_keyframe |= is_key;
            let at = self.frame_time(output.time);
            if self.resync && !is_key {
                // It refers to a picture the phone never got.
                continue;
            }
            let (width, height) = self.converter.size();
            let frame = EncodedFrame {
                data,
                keyframe: is_key,
                width,
                height,
                timestamp: at,
            };
            match self.frames.try_send(frame) {
                Ok(()) => {
                    self.sent += 1;
                    sent_keyframe |= is_key;
                }
                Err(mpsc::error::TrySendError::Full(_)) => dropped = true,
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(Exit::Stop),
            }
        }
        if dropped {
            // Everything after a lost frame is undecodable until a keyframe:
            // ask for one, send nothing else until it is through, and make it
            // as soon as the rate allows rather than at the next change.
            debug!("live view: consumer behind, frame dropped");
            self.resync = true;
            self.force_keyframe = true;
            self.pacer.want_repeat();
        } else if sent_keyframe {
            self.resync = false;
            self.force_keyframe = false;
            self.keyframe_misses = 0;
        } else if keyframe && !got_keyframe {
            self.keyframe_misses += 1;
            if self.keyframe_misses >= KEYFRAME_RETRIES {
                debug!(
                    "live view: {} ignores keyframe requests; new encoder",
                    self.encoder.name
                );
                self.rebuild_encoder(self.converter.size(), self.preference)?;
                self.pacer.want_repeat();
            }
        }
        self.pacer.sent(now, fresh);
        Ok(())
    }

    /// Move the encoder to the picture's size once that size has settled.
    fn follow_size(&mut self, now: Instant) -> Result<(), Exit> {
        let desired = output_size(self.content.0, self.content.1, self.settings.max_long_side);
        if desired == self.converter.size() {
            return Ok(());
        }
        let settled_at = self.content_changed + RESIZE_SETTLE;
        if now < settled_at {
            self.pacer.repeat_at(settled_at);
            return Ok(());
        }
        debug!(
            "live view: picture {}x{} -> {}x{}",
            self.converter.size().0,
            self.converter.size().1,
            desired.0,
            desired.1
        );
        self.rebuild_encoder(desired, self.preference)
    }

    fn rebuild_encoder(&mut self, size: (u32, u32), preference: Preference) -> Result<(), Exit> {
        let config = Config {
            width: size.0,
            height: size.1,
            fps: self.settings.max_fps.max(1),
            bitrate: self.settings.bitrate_bps,
        };
        // The new one is made while the old one still exists, which a GPU
        // with a session limit could refuse; that is rare enough that the
        // software encoder covers it (`Preference::Auto` falls back).
        let encoder = Encoder::create(&self.gpu, config, preference)
            .map_err(|why| Exit::Ended(Ended::Failed(format!("no encoder: {why}"))))?;
        self.encoder = encoder;
        if self.converter.size() != size {
            self.converter = Converter::new(size.0, size.1);
        }
        self.params.clear();
        if let Some(header) = self.encoder.sequence_header() {
            self.params.learn_header(&header);
        }
        self.force_keyframe = true;
        self.keyframe_misses = 0;
        Ok(())
    }

    /// The stream time of a picture taken now: increasing, whatever the clock
    /// does.
    fn next_time(&mut self, now: Instant) -> Duration {
        let mut time = now.saturating_duration_since(self.started);
        if let Some(last) = self.last_time {
            if time <= last {
                time = last + Duration::from_micros(100);
            }
        }
        self.last_time = Some(time);
        time
    }

    /// The timestamp of an access unit: the encoder's sample time (which is
    /// the picture's, since no frame is reordered), never going backwards.
    fn frame_time(&mut self, sample_time: i64) -> Duration {
        let time = u64::try_from(sample_time)
            .map(|t| Duration::from_nanos(t.saturating_mul(100)))
            .unwrap_or_default();
        let time = match self.last_sent_time {
            Some(last) if time <= last => last + Duration::from_micros(100),
            _ => time,
        };
        self.last_sent_time = Some(time);
        time
    }
}

fn to_100ns(time: Duration) -> i64 {
    i64::try_from(time.as_nanos() / 100).unwrap_or(i64::MAX)
}

fn failed(what: &str, err: windows::core::Error) -> Exit {
    Exit::Ended(Ended::Failed(format!("{what}: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_times_convert_to_media_foundation_units() {
        assert_eq!(to_100ns(Duration::from_millis(1)), 10_000);
        assert_eq!(to_100ns(Duration::from_secs(2)), 20_000_000);
    }

    #[test]
    fn a_window_handle_that_is_not_a_window_is_gone() {
        let result = start(CaptureTarget::Window(0x7fff_fff0), VideoSettings::default());
        assert!(matches!(result, Err(StartError::TargetGone)));
        let result = start(
            CaptureTarget::Screen("\\\\.\\NOPE".into()),
            VideoSettings::default(),
        );
        assert!(matches!(result, Err(StartError::TargetGone)));
    }
}
