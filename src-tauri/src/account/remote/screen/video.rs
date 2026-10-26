//! Capturing a window or a screen and encoding it to H.264 for a live view.
//!
//! The contract the WebRTC session relies on:
//!
//! - [`start`] begins capturing `target` and returns a [`VideoStream`] handle
//!   plus a channel of [`EncodedFrame`]s. Frames come only when the picture
//!   changes, and at least every [`VideoSettings::keepalive`] (the last
//!   picture again) so the phone can tell the link is alive.
//! - Every frame is one H.264 access unit in Annex-B form (start codes), Constrained
//!   Baseline profile, no B-frames, so `packetization-mode=1` RTP can carry it
//!   as is. A keyframe carries its SPS and PPS in front of the IDR slice, so a
//!   phone that joins or loses packets can start decoding from any keyframe.
//! - The picture is at most [`VideoSettings::max_long_side`] pixels on its long
//!   side, scaled down keeping its aspect ratio, with even width and height.
//!   When the window is resized the stream carries on at the new size,
//!   starting with a keyframe.
//! - [`VideoStream::geometry`] says where the picture is on the screen right
//!   now, in physical pixels, so a tap at (x, y) across the picture can be
//!   turned into a click at the right place.
//! - The channel closing means the capture ended: the window closed
//!   ([`VideoStream::ended`] says why), or [`VideoStream::stop`] was called.

use std::time::Duration;

use tokio::sync::mpsc;

/// What to capture: a window by its platform handle (HWND on Windows, as
/// `command_mode::effects::WindowEntry::id`), or a screen by [`ScreenInfo::id`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureTarget {
    Window(i64),
    Screen(String),
}

/// A screen (monitor) that can be captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenInfo {
    /// Stable for as long as the monitor is attached (on Windows its device
    /// name, `\\.\DISPLAY1`).
    pub id: String,
    /// What a person calls it: "Display 1", or the monitor's friendly name.
    pub name: String,
    /// Physical pixels.
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Clone, Debug)]
pub struct VideoSettings {
    pub max_long_side: u32,
    pub max_fps: u32,
    /// Target bitrate; changed while running with [`VideoStream::set_bitrate`].
    pub bitrate_bps: u32,
    /// The longest gap between two frames while nothing changes.
    pub keepalive: Duration,
    /// Draw the mouse pointer into the picture.
    pub cursor: bool,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            max_long_side: 1920,
            max_fps: 30,
            bitrate_bps: 2_500_000,
            keepalive: Duration::from_secs(2),
            cursor: true,
        }
    }
}

/// One encoded picture. Its keyframe flag and size are for bandwidth
/// adaptation, which the session does not do yet.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct EncodedFrame {
    /// One Annex-B access unit.
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// Picture size, even numbers.
    pub width: u32,
    pub height: u32,
    /// Since the stream started; increases.
    pub timestamp: Duration,
}

/// Where the captured picture is on the screen, in physical pixels: its
/// top-left corner and its size before scaling. A tap at (x, y) in 0..1 across
/// the picture is the screen point (left + x * width, top + y * height).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// Why a capture ended on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The window closed or the screen was disconnected.
    TargetGone,
    /// Capture or encoding failed and could not recover.
    Failed(String),
}

#[derive(Debug)]
pub enum StartError {
    /// No such window or screen (any more).
    TargetGone,
    /// This platform or this machine cannot capture (no Windows.Graphics.Capture,
    /// no H.264 encoder).
    Unsupported(String),
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TargetGone => f.write_str("the window or screen is gone"),
            Self::Unsupported(why) => write!(f, "not supported here: {why}"),
            Self::Failed(why) => write!(f, "capture failed: {why}"),
        }
    }
}

/// A running capture. Dropping it stops the capture, as [`VideoStream::stop`] does.
pub trait VideoStream: Send + Sync {
    /// The next frame is a keyframe (the phone lost the picture, or just joined).
    fn request_keyframe(&self);
    /// Change the target bitrate; takes effect within a frame or two. For
    /// bandwidth adaptation, which the session does not do yet.
    #[allow(dead_code)]
    fn set_bitrate(&self, bps: u32);
    /// Where the picture is on the screen now, or `None` when the window is
    /// minimized.
    fn geometry(&self) -> Option<Geometry>;
    /// Why the frame channel closed, once it has.
    fn ended(&self) -> Option<Ended>;
    fn stop(&self);
}

/// The screens attached now, the primary one first.
pub fn list_screens() -> Vec<ScreenInfo> {
    #[cfg(target_os = "windows")]
    {
        windows::list_screens()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Start capturing `target`. Frames arrive on the returned channel.
pub fn start(
    target: CaptureTarget,
    settings: VideoSettings,
) -> Result<(Box<dyn VideoStream>, mpsc::Receiver<EncodedFrame>), StartError> {
    #[cfg(target_os = "windows")]
    {
        windows::start(target, settings)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (target, settings);
        Err(StartError::Unsupported(
            "live view is only built for Windows so far".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
mod windows;
