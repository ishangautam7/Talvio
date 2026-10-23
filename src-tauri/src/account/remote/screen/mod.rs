//! Live view (`PROTOCOL.md`, "Live view"): one window or screen of this
//! computer, streamed to a phone over WebRTC, with the phone's keys, text,
//! taps and scrolls coming back over its data channel.
//!
//! The pieces, each behind a small interface so they are built and tested on
//! their own:
//!
//! - [`video`]: capture a window or screen and encode it to H.264 access units
//!   ([`video::start`], one platform module per OS; Windows first).
//! - [`audio`]: what the computer is playing, as 20 ms Opus packets
//!   ([`audio::start`]).
//! - `session.rs`: the `screen.watch` request: the WebRTC peer connection, the
//!   handshake over the relay, and the data channel.

pub mod audio;
pub mod control;
pub mod input;
pub mod rtc;
pub mod session;
pub mod video;
