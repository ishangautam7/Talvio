//! What the computer is playing, for a live view's sound.
//!
//! The contract the WebRTC session relies on:
//!
//! - [`start`] begins capturing the default output device's mix (on Windows,
//!   WASAPI loopback) and returns an [`AudioStream`] handle plus a channel of
//!   [`OpusPacket`]s: 20 ms each, 48 kHz stereo, in order, about 64 kbit/s.
//! - While nothing is playing, packets keep coming (Opus encodes silence in a
//!   few bytes), so the RTP clock stays steady.
//! - [`AudioStream::set_enabled`] with `false` keeps the capture open but
//!   sends nothing, so turning sound back on is instant; the stream starts
//!   disabled.
//! - When the default output device changes (headphones plugged in), the
//!   capture follows it.
//! - The channel closing means the capture ended ([`AudioStream::stop`], or a
//!   failure it could not recover from).

use std::time::Duration;

use tokio::sync::mpsc;

/// One 20 ms Opus packet.
#[derive(Clone, Debug)]
pub struct OpusPacket {
    pub data: Vec<u8>,
    pub duration: Duration,
}

pub trait AudioStream: Send + Sync {
    fn set_enabled(&self, on: bool);
    fn stop(&self);
}

#[derive(Debug)]
pub enum StartError {
    /// Only built where there is no capture (not Windows), and in tests.
    #[allow(dead_code)]
    Unsupported(String),
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(why) => write!(f, "not supported here: {why}"),
            Self::Failed(why) => write!(f, "sound capture failed: {why}"),
        }
    }
}

/// Start capturing the computer's sound, disabled until
/// [`AudioStream::set_enabled`].
pub fn start() -> Result<(Box<dyn AudioStream>, mpsc::Receiver<OpusPacket>), StartError> {
    #[cfg(target_os = "windows")]
    {
        windows::start()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(StartError::Unsupported(
            "the computer's sound is only captured on Windows so far".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
mod windows;
