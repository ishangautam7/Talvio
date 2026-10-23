//! Live checks against the real capture and encoders. `#[ignore]`d: they need
//! a desktop, put a window on it, and take a few seconds each.
//!
//! `cargo test --lib account::remote::screen::video -- --ignored --nocapture --test-threads=1`
//!
//! Each run writes what it encoded to `<TALVIO_VIDEO_TEST_OUT or temp>/
//! live-view-<name>.h264` and decodes it again with Media Foundation's own
//! H.264 decoder, so a stream that only looks right is caught too.

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, Interface};
use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MSH264DecoderMFT, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFShutdown, MFStartup, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFSTARTUP_LITE, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetTimer, SetWindowPos, TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG,
    SWP_NOMOVE, SWP_NOZORDER, WM_CLOSE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_TOPMOST,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use super::annexb::{nal_units, sps_dimensions, NAL_IDR, NAL_PPS, NAL_SPS};
use super::encoder::Preference;
use super::{list_screens, start_with, CaptureTarget, EncodedFrame, Ended, VideoSettings};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Receiver;

static TICK: AtomicU32 = AtomicU32::new(0);

/// A window of our own that repaints itself ~60 times a second, so the
/// capture always has something new.
struct TestWindow {
    hwnd: isize,
    thread: Option<std::thread::JoinHandle<()>>,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by the system for our own window, with its message.
    unsafe {
        match msg {
            WM_TIMER => {
                TICK.fetch_add(1, Ordering::Relaxed);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let tick = TICK.load(Ordering::Relaxed);
                let background = CreateSolidBrush(windows::Win32::Foundation::COLORREF(
                    (tick * 3 % 256) | ((tick * 7 % 256) << 8) | (0x40 << 16),
                ));
                FillRect(dc, &paint.rcPaint, background);
                let _ = DeleteObject(background.into());
                let bar = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00ff_ffff));
                let x = (tick * 12 % 1200) as i32;
                FillRect(
                    dc,
                    &RECT {
                        left: x,
                        top: 100,
                        right: x + 80,
                        bottom: 400,
                    },
                    bar,
                );
                let _ = DeleteObject(bar.into());
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

impl TestWindow {
    fn open(width: i32, height: i32) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            // SAFETY: a window class and window of our own, and the standard
            // message loop for them on this thread.
            unsafe {
                let instance = GetModuleHandleW(None).expect("module handle");
                let class = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance.into(),
                    lpszClassName: w!("TalvioLiveViewTest"),
                    ..Default::default()
                };
                RegisterClassW(&class);
                let hwnd = CreateWindowExW(
                    // Topmost, so the screen tests see it change whatever
                    // else is open.
                    WS_EX_TOPMOST,
                    w!("TalvioLiveViewTest"),
                    w!("Talvio live view test"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    40,
                    40,
                    width,
                    height,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .expect("test window");
                SetTimer(Some(hwnd), 1, 15, None);
                tx.send(hwnd.0 as isize).expect("send hwnd");
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
        let hwnd = rx.recv().expect("window opened");
        // Let it paint once before it is captured.
        std::thread::sleep(Duration::from_millis(300));
        Self {
            hwnd,
            thread: Some(thread),
        }
    }

    fn resize(&self, width: i32, height: i32) {
        // SAFETY: our own live window.
        unsafe {
            let _ = SetWindowPos(
                HWND(self.hwnd as *mut _),
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER,
            );
        }
    }

    fn close(&mut self) {
        // SAFETY: posting to our own window; a closed one ignores it.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        self.close();
    }
}

fn cpu_time() -> Duration {
    let (mut a, mut b, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: four live out-parameters for our own process.
    unsafe {
        let _ = GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user);
    }
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// Frames that arrive within `time`, stopping early when the channel closes.
fn collect(rx: &mut Receiver<EncodedFrame>, time: Duration) -> (Vec<EncodedFrame>, bool) {
    let until = Instant::now() + time;
    let mut frames = Vec::new();
    while Instant::now() < until {
        match rx.try_recv() {
            Ok(frame) => frames.push(frame),
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(2)),
            Err(TryRecvError::Disconnected) => return (frames, true),
        }
    }
    (frames, false)
}

fn kinds(frame: &EncodedFrame) -> Vec<u8> {
    nal_units(&frame.data).iter().map(|n| n.kind).collect()
}

/// The checks every stream must pass.
fn check_frames(frames: &[EncodedFrame], max_long_side: u32) {
    assert!(!frames.is_empty(), "no frames");
    let first = &frames[0];
    assert!(first.keyframe, "the first frame is not a keyframe");
    let mut last = None;
    for frame in frames {
        assert!(frame.width % 2 == 0 && frame.height % 2 == 0);
        assert!(frame.width.max(frame.height) <= max_long_side);
        if let Some(last) = last {
            assert!(frame.timestamp > last, "timestamps go backwards");
        }
        last = Some(frame.timestamp);
        if frame.keyframe {
            let nals = nal_units(&frame.data);
            let k = kinds(frame);
            assert!(
                k.contains(&NAL_SPS) && k.contains(&NAL_PPS) && k.contains(&NAL_IDR),
                "{k:?}"
            );
            let sps = nals.iter().find(|n| n.kind == NAL_SPS).expect("sps");
            let sps = &frame.data[sps.start..sps.end];
            assert_eq!(sps[1], 66, "Baseline profile");
            assert_eq!(
                sps[2] & 0x40,
                0x40,
                "constraint_set1 (Constrained Baseline)"
            );
            assert_eq!(
                sps_dimensions(sps),
                Some((frame.width, frame.height)),
                "SPS size matches the frame"
            );
        }
    }
}

fn out_dir() -> std::path::PathBuf {
    std::env::var_os("TALVIO_VIDEO_TEST_OUT")
        .map(Into::into)
        .unwrap_or_else(std::env::temp_dir)
}

/// Decode the stream with Media Foundation's H.264 decoder; returns how many
/// pictures came out and the size of the last.
fn decode(frames: &[EncodedFrame]) -> (usize, (u32, u32)) {
    // SAFETY: COM/MF are started and stopped on this thread around the use of
    // a decoder MFT that lives only inside this block.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_LITE).expect("MFStartup");
        let result = (|| -> windows::core::Result<(usize, (u32, u32))> {
            let decoder: IMFTransform =
                CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)?;
            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            decoder.SetInputType(0, &input, 0)?;
            let set_output = |decoder: &IMFTransform| -> windows::core::Result<(u32, u32)> {
                let mut index = 0;
                loop {
                    let t = decoder.GetOutputAvailableType(0, index)?;
                    if t.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                        decoder.SetOutputType(0, &t, 0)?;
                        let size = t.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
                        return Ok(((size >> 32) as u32, size as u32));
                    }
                    index += 1;
                }
            };
            let mut size = set_output(&decoder)?;
            let mut decoded = 0;
            let mut drain =
                |decoder: &IMFTransform, size: &mut (u32, u32)| -> windows::core::Result<()> {
                    loop {
                        let info = decoder.GetOutputStreamInfo(0)?;
                        let sample =
                            if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0 {
                                None
                            } else {
                                let s = MFCreateSample()?;
                                s.AddBuffer(&MFCreateMemoryBuffer(info.cbSize.max(4096))?)?;
                                Some(s)
                            };
                        let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
                            dwStreamID: 0,
                            pSample: ManuallyDrop::new(sample),
                            dwStatus: 0,
                            pEvents: ManuallyDrop::new(None),
                        }];
                        let mut status = 0;
                        let r = decoder.ProcessOutput(0, &mut buffers, &mut status);
                        let _ = ManuallyDrop::take(&mut buffers[0].pSample);
                        let _ = ManuallyDrop::take(&mut buffers[0].pEvents);
                        match r {
                            Ok(()) => decoded += 1,
                            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                                *size = set_output(decoder)?
                            }
                            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                            Err(e) => return Err(e),
                        }
                    }
                };
            for (n, frame) in frames.iter().enumerate() {
                let buffer = MFCreateMemoryBuffer(frame.data.len() as u32)?;
                let mut ptr = std::ptr::null_mut();
                buffer.Lock(&mut ptr, None, None)?;
                std::ptr::copy_nonoverlapping(frame.data.as_ptr(), ptr, frame.data.len());
                buffer.Unlock()?;
                buffer.SetCurrentLength(frame.data.len() as u32)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(n as i64 * 333_333)?;
                decoder.ProcessInput(0, &sample, 0)?;
                drain(&decoder, &mut size)?;
            }
            decoder.ProcessMessage(
                windows::Win32::Media::MediaFoundation::MFT_MESSAGE_COMMAND_DRAIN,
                0,
            )?;
            drain(&decoder, &mut size)?;
            let _ = decoder.cast::<IMFTransform>();
            Ok((decoded, size))
        })();
        let _ = MFShutdown();
        CoUninitialize();
        result.expect("decoding")
    }
}

fn write_and_decode(name: &str, frames: &[EncodedFrame]) {
    let path = out_dir().join(format!("live-view-{name}.h264"));
    let bytes: Vec<u8> = frames.iter().flat_map(|f| f.data.iter().copied()).collect();
    std::fs::write(&path, &bytes).expect("write .h264");
    let (decoded, size) = decode(frames);
    let last = frames.last().expect("frames");
    println!(
        "{name}: wrote {} ({} KiB, {} frames); decoded {decoded} pictures, {}x{}",
        path.display(),
        bytes.len() / 1024,
        frames.len(),
        size.0,
        size.1
    );
    assert!(decoded + 2 >= frames.len(), "decoder lost pictures");
    // The decoder reports its padded size (1088 for 1080) on some versions.
    assert_eq!(size.0, last.width);
    assert!(size.1 >= last.height && size.1 < last.height + 16);
}

fn report(name: &str, frames: &[EncodedFrame], wall: Duration, cpu: Duration) {
    let keys = frames.iter().filter(|f| f.keyframe).count();
    let bytes: usize = frames.iter().map(|f| f.data.len()).sum();
    println!(
        "{name}: {} frames ({keys} keyframes) in {:.2} s = {:.1} fps, {:.0} kbit/s, CPU {:.0}% of one core, size {}x{}",
        frames.len(),
        wall.as_secs_f64(),
        frames.len() as f64 / wall.as_secs_f64(),
        bytes as f64 * 8.0 / wall.as_secs_f64() / 1000.0,
        cpu.as_secs_f64() / wall.as_secs_f64() * 100.0,
        frames.last().map_or(0, |f| f.width),
        frames.last().map_or(0, |f| f.height),
    );
}

fn settings() -> VideoSettings {
    VideoSettings {
        max_long_side: 1920,
        max_fps: 30,
        bitrate_bps: 4_000_000,
        keepalive: Duration::from_millis(500),
        cursor: true,
    }
}

/// The primary screen at 1080p with an animated window on it: frames, rate,
/// CPU, a keyframe on request, a bitrate change, and a clean stop.
fn screen(preference: Preference, name: &str) {
    let _window = TestWindow::open(1280, 720);
    let screens = list_screens();
    println!("screens: {screens:?}");
    let primary = screens
        .iter()
        .find(|s| s.primary)
        .expect("a primary screen");
    assert_eq!(screens[0].id, primary.id, "primary first");
    let started = Instant::now();
    let (stream, mut rx) = start_with(
        CaptureTarget::Screen(primary.id.clone()),
        settings(),
        preference,
    )
    .expect("start");
    println!("{name}: started in {} ms", started.elapsed().as_millis());
    let geometry = stream.geometry().expect("geometry");
    assert_eq!(
        (geometry.width, geometry.height),
        (primary.width, primary.height)
    );

    let cpu = cpu_time();
    let wall = Instant::now();
    let (mut frames, closed) = collect(&mut rx, Duration::from_secs(4));
    report(name, &frames, wall.elapsed(), cpu_time() - cpu);
    assert!(!closed);
    check_frames(&frames, 1920);
    assert!(frames.len() >= 60, "at least ~15 fps with a moving picture");

    stream.request_keyframe();
    let (more, _) = collect(&mut rx, Duration::from_millis(700));
    assert!(more.iter().any(|f| f.keyframe), "keyframe on request");
    frames.extend(more);

    let kbps = |frames: &[EncodedFrame]| {
        let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
            return 0.0;
        };
        let bytes: usize = frames[1..].iter().map(|f| f.data.len()).sum();
        bytes as f64 * 8.0 / (last.timestamp - first.timestamp).as_secs_f64() / 1000.0
    };
    let before = kbps(&frames[frames.len() / 2..]);
    stream.set_bitrate(800_000);
    let (more, _) = collect(&mut rx, Duration::from_millis(2000));
    assert!(more.len() > 10, "frames after a bitrate change");
    // The first half second is the encoder settling.
    let after = kbps(&more[more.len() / 4..]);
    println!("{name}: {before:.0} kbit/s at 4 Mbit/s, {after:.0} kbit/s at 0.8 Mbit/s");
    // The desktop behind the test window is whatever else is on screen, so
    // only a clear drop is asked for, not a number.
    assert!(after < before * 0.8, "the bitrate change took");
    frames.extend(more);
    check_frames(&frames, 1920);
    write_and_decode(name, &frames);

    let stopping = Instant::now();
    stream.stop();
    let (_, closed) = collect(&mut rx, Duration::from_secs(3));
    assert!(closed, "the channel closes on stop");
    println!("{name}: stopped in {} ms", stopping.elapsed().as_millis());
    assert_eq!(stream.ended(), None);
    drop(stream);
}

#[test]
#[ignore]
fn live_screen_hardware() {
    screen(Preference::HardwareOnly, "screen-hardware");
}

#[test]
#[ignore]
fn live_screen_software() {
    screen(Preference::SoftwareOnly, "screen-software");
}

/// A window: a scaled-down stream, a still picture's keepalive, a resize that
/// restarts at the new size with a keyframe, and the window closing.
fn window(preference: Preference, name: &str) {
    let mut window = TestWindow::open(1000, 700);
    let mut settings = settings();
    settings.max_long_side = 640;
    let (stream, mut rx) = start_with(
        CaptureTarget::Window(window.hwnd as i64),
        settings,
        preference,
    )
    .expect("start");
    let geometry = stream.geometry().expect("geometry");
    println!("{name}: window at {geometry:?}");
    let cpu = cpu_time();
    let wall = Instant::now();
    let (mut frames, closed) = collect(&mut rx, Duration::from_secs(2));
    report(name, &frames, wall.elapsed(), cpu_time() - cpu);
    assert!(!closed);
    check_frames(&frames, 640);
    let first_size = (frames[0].width, frames[0].height);
    assert_eq!(first_size.0, 640);

    // Fall behind: the channel fills, frames are dropped, and the first one
    // sent after that is a keyframe so the phone's decoder recovers.
    std::thread::sleep(Duration::from_millis(800));
    let (more, _) = collect(&mut rx, Duration::from_millis(500));
    let queued = 4;
    assert!(more.len() > queued, "frames after the stall");
    assert!(
        more[queued].keyframe,
        "the frame after a drop is a keyframe"
    );
    frames.extend(more);

    window.resize(700, 900);
    let (more, _) = collect(&mut rx, Duration::from_millis(1500));
    let resized = more
        .iter()
        .position(|f| (f.width, f.height) != first_size)
        .expect("frames at the new size");
    assert!(more[resized].keyframe, "a new size starts with a keyframe");
    let last = more.last().expect("frames");
    assert!(
        last.height == 640 && last.width < 640,
        "portrait now: {}x{}",
        last.width,
        last.height
    );
    frames.extend(more);
    check_frames(&frames, 640);
    write_and_decode(
        name,
        &frames[..frames.len()]
            .iter()
            .filter(|f| (f.width, f.height) == first_size)
            .cloned()
            .collect::<Vec<_>>(),
    );

    window.close();
    let (_, closed) = collect(&mut rx, Duration::from_secs(3));
    assert!(closed, "the channel closes when the window does");
    assert_eq!(stream.ended(), Some(Ended::TargetGone));
    assert_eq!(stream.geometry(), None);
}

#[test]
#[ignore]
fn live_window_hardware() {
    window(Preference::HardwareOnly, "window-hardware");
}

#[test]
#[ignore]
fn live_window_software() {
    window(Preference::SoftwareOnly, "window-software");
}

/// A window that does not change: only keepalives, so the thread is idle.
#[test]
#[ignore]
fn live_still_window_idles() {
    // Notepad-like still content: our window with its timer never firing is
    // not possible, so capture a minimized window instead, which Windows
    // stops composing.
    let window = TestWindow::open(800, 600);
    let mut settings = settings();
    settings.keepalive = Duration::from_millis(500);
    // SAFETY: our own window.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            HWND(window.hwnd as *mut _),
            windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE,
        );
    }
    std::thread::sleep(Duration::from_millis(400));
    let (stream, mut rx) = start_with(
        CaptureTarget::Window(window.hwnd as i64),
        settings,
        Preference::Auto,
    )
    .expect("start");
    assert_eq!(stream.geometry(), None, "minimized");
    let cpu = cpu_time();
    let wall = Instant::now();
    let (frames, _) = collect(&mut rx, Duration::from_secs(3));
    let used = cpu_time() - cpu;
    report("still", &frames, wall.elapsed(), used);
    assert!(
        (4..=8).contains(&frames.len()),
        "keepalives only: {}",
        frames.len()
    );
    assert!(frames[0].keyframe);
}
