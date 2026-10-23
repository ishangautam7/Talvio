//! Pointer input and what is under it, for a live view in Control mode, and
//! where the text cursor is, for the phone's Follow.
//!
//! Points are physical screen pixels, the space `video::Geometry` describes.

/// A rectangle in physical screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// The control at a point: what it is called, and whether it is a password
/// field (which phone input never goes into).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlInfo {
    /// Command mode's role word ("button", "link", "edit"…).
    pub role: &'static str,
    pub label: String,
    pub password: bool,
}

pub use super::control::Button;

#[cfg(target_os = "windows")]
pub use windows::{caret, click, control_at, focused_is_password, scroll};

#[cfg(not(target_os = "windows"))]
pub fn click(_x: i32, _y: i32, _button: Button, _count: u8) -> Result<(), String> {
    Err("not supported here".into())
}

#[cfg(not(target_os = "windows"))]
pub fn scroll(_x: i32, _y: i32, _dx: i32, _dy: i32) -> Result<(), String> {
    Err("not supported here".into())
}

#[cfg(not(target_os = "windows"))]
pub fn control_at(_x: i32, _y: i32) -> Option<ControlInfo> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn focused_is_password() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn caret() -> Option<Rect> {
    None
}

#[cfg(target_os = "windows")]
mod windows;
