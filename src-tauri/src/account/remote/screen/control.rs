//! The `control` data channel's messages (`PROTOCOL.md`, "Live view"), read
//! from the phone and written back, kept apart from the session so every
//! rule about what is accepted is a pure, tested function.

use serde_json::{json, Value};

use crate::command_mode::effects::KeyCombo;

/// The most combos one `keys` message presses, as `windows.keys`.
const MAX_KEYS: usize = 20;
const MAX_TEXT_CHARS: usize = 10_000;
/// A message id longer than this is not one a phone makes.
const MAX_ID_CHARS: usize = 64;
/// The most wheel notches one scroll turns.
const MAX_NOTCHES: i32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// A point across the picture, each coordinate in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Keys(Vec<KeyCombo>),
    Text(String),
    Tap {
        at: Point,
        button: Button,
        count: u8,
        confirmed: bool,
    },
    Scroll {
        at: Point,
        dx: i32,
        dy: i32,
    },
}

/// One message from the phone.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Input to deliver, answered by an `ack` with `id`.
    Input {
        id: String,
        input: Input,
    },
    Audio(bool),
    Keyframe,
}

/// Why a message is refused before anything is done with it: an `ack` error
/// code when it had an id, else it is dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub id: Option<String>,
    pub code: &'static str,
}

/// Read one text message. `parse_keys` turns `keys` strings into combos the
/// way `windows.keys` does (its refusals come back as their codes).
pub fn parse(
    text: &str,
    parse_keys: impl Fn(&[Value]) -> Result<Vec<KeyCombo>, &'static str>,
) -> Result<Message, Refused> {
    let refused = |id: Option<String>, code| Refused { id, code };
    let value: Value = serde_json::from_str(text).map_err(|_| refused(None, "unsupported"))?;
    let kind = value.get("t").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "audio" => {
            let on = value
                .get("on")
                .and_then(Value::as_bool)
                .ok_or(refused(None, "unsupported"))?;
            return Ok(Message::Audio(on));
        }
        "keyframe" => return Ok(Message::Keyframe),
        _ => {}
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| (1..=MAX_ID_CHARS).contains(&id.chars().count()))
        .map(str::to_string)
        .ok_or(refused(None, "unsupported"))?;
    let fail = |code| refused(Some(id.clone()), code);
    let input = match kind {
        "keys" => {
            let keys = value
                .get("keys")
                .and_then(Value::as_array)
                .filter(|keys| (1..=MAX_KEYS).contains(&keys.len()))
                .ok_or(fail("unsupported"))?;
            Input::Keys(parse_keys(keys).map_err(fail)?)
        }
        "text" => {
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| (1..=MAX_TEXT_CHARS).contains(&text.chars().count()))
                .ok_or(fail("unsupported"))?;
            Input::Text(text.to_string())
        }
        "tap" => {
            let at = point(&value).ok_or(fail("unsupported"))?;
            let button = match value.get("button").and_then(Value::as_str) {
                None | Some("left") => Button::Left,
                Some("right") => Button::Right,
                Some(_) => return Err(fail("unsupported")),
            };
            let count = match value.get("count").and_then(Value::as_u64) {
                None | Some(1) => 1,
                Some(2) => 2,
                Some(_) => return Err(fail("unsupported")),
            };
            let confirmed = value
                .get("confirmed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Input::Tap {
                at,
                button,
                count,
                confirmed,
            }
        }
        "scroll" => {
            let at = point(&value).ok_or(fail("unsupported"))?;
            let notches = |key| {
                value
                    .get(key)
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(-(MAX_NOTCHES as i64), MAX_NOTCHES as i64) as i32
            };
            Input::Scroll {
                at,
                dx: notches("dx"),
                dy: notches("dy"),
            }
        }
        _ => return Err(fail("unsupported")),
    };
    Ok(Message::Input { id, input })
}

/// `x` and `y`, each a number in 0..=1.
fn point(value: &Value) -> Option<Point> {
    let coordinate = |key| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|c| (0.0..=1.0).contains(c))
    };
    Some(Point {
        x: coordinate("x")?,
        y: coordinate("y")?,
    })
}

/// The answer to an input message.
pub fn ack(id: &str, outcome: Result<(), Ack>) -> String {
    match outcome {
        Ok(()) => json!({ "t": "ack", "id": id, "ok": true }),
        Err(Ack::Error(code)) => json!({ "t": "ack", "id": id, "ok": false, "error": code }),
        Err(Ack::Confirm(label)) => {
            json!({ "t": "ack", "id": id, "ok": false, "error": "confirm", "label": label })
        }
    }
    .to_string()
}

/// Why input was not delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ack {
    Error(&'static str),
    /// The control under the tap sends, buys, posts or deletes: ask first.
    Confirm(String),
}

/// Where the text cursor is, across the picture; `None` for no cursor.
pub fn caret(rect: Option<(f64, f64, f64, f64)>) -> String {
    match rect {
        Some((x, y, w, h)) => json!({ "t": "caret", "x": x, "y": y, "w": w, "h": h }),
        None => json!({ "t": "caret" }),
    }
    .to_string()
}

/// What is being watched.
pub fn target(title: &str, width: u32, height: u32, state: &str) -> String {
    json!({ "t": "target", "title": title, "width": width, "height": height, "state": state })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::registry::Modifier;

    fn keys(keys: &[Value]) -> Result<Vec<KeyCombo>, &'static str> {
        keys.iter()
            .map(|key| match key.as_str() {
                Some("enter") => Ok(KeyCombo {
                    modifiers: vec![],
                    key: "enter".into(),
                }),
                Some("ctrl+c") => Ok(KeyCombo {
                    modifiers: vec![Modifier::Ctrl],
                    key: "c".into(),
                }),
                Some("alt+f4") => Err("key_refused"),
                _ => Err("unsupported"),
            })
            .collect()
    }

    fn read(text: &str) -> Result<Message, Refused> {
        parse(text, keys)
    }

    fn input(text: &str) -> Input {
        match read(text) {
            Ok(Message::Input { input, .. }) => input,
            other => panic!("{other:?}"),
        }
    }

    fn refused(text: &str) -> (Option<String>, &'static str) {
        let refused = read(text).unwrap_err();
        (refused.id, refused.code)
    }

    #[test]
    fn keys_and_text_are_read() {
        assert_eq!(
            input(r#"{"t":"keys","id":"k1","keys":["enter","ctrl+c"]}"#),
            Input::Keys(keys(&[json!("enter"), json!("ctrl+c")]).unwrap())
        );
        assert_eq!(
            input(r#"{"t":"text","id":"k2","text":"hi"}"#),
            Input::Text("hi".into())
        );
    }

    #[test]
    fn taps_default_to_one_left_click() {
        assert_eq!(
            input(r#"{"t":"tap","id":"k","x":0.5,"y":0.25}"#),
            Input::Tap {
                at: Point { x: 0.5, y: 0.25 },
                button: Button::Left,
                count: 1,
                confirmed: false
            }
        );
        assert_eq!(
            input(
                r#"{"t":"tap","id":"k","x":0,"y":1,"button":"right","count":2,"confirmed":true}"#
            ),
            Input::Tap {
                at: Point { x: 0.0, y: 1.0 },
                button: Button::Right,
                count: 2,
                confirmed: true
            }
        );
    }

    #[test]
    fn scrolls_are_capped() {
        assert_eq!(
            input(r#"{"t":"scroll","id":"k","x":0.5,"y":0.5,"dy":-900}"#),
            Input::Scroll {
                at: Point { x: 0.5, y: 0.5 },
                dx: 0,
                dy: -MAX_NOTCHES
            }
        );
    }

    #[test]
    fn switches_need_no_id() {
        assert_eq!(read(r#"{"t":"audio","on":true}"#), Ok(Message::Audio(true)));
        assert_eq!(read(r#"{"t":"keyframe"}"#), Ok(Message::Keyframe));
    }

    #[test]
    fn bad_messages_are_refused_with_their_id_when_they_have_one() {
        assert_eq!(refused("not json"), (None, "unsupported"));
        assert_eq!(
            refused(r#"{"t":"keys","keys":["enter"]}"#),
            (None, "unsupported")
        );
        assert_eq!(
            refused(r#"{"t":"keys","id":"k","keys":["alt+f4"]}"#),
            (Some("k".into()), "key_refused")
        );
        for bad in [
            r#"{"t":"keys","id":"k","keys":[]}"#,
            r#"{"t":"text","id":"k","text":""}"#,
            r#"{"t":"tap","id":"k","x":1.5,"y":0.5}"#,
            r#"{"t":"tap","id":"k","x":0.5,"y":0.5,"count":3}"#,
            r#"{"t":"tap","id":"k","x":0.5,"y":0.5,"button":"middle"}"#,
            r#"{"t":"fly","id":"k"}"#,
        ] {
            assert_eq!(refused(bad), (Some("k".into()), "unsupported"), "{bad}");
        }
        let long_id = format!(r#"{{"t":"text","id":"{}","text":"x"}}"#, "k".repeat(65));
        assert_eq!(refused(&long_id), (None, "unsupported"));
    }

    #[test]
    fn acks_and_notices_read_as_the_protocol_says() {
        let ack_ok: Value = serde_json::from_str(&ack("k1", Ok(()))).unwrap();
        assert_eq!(ack_ok, json!({ "t": "ack", "id": "k1", "ok": true }));
        let confirm: Value =
            serde_json::from_str(&ack("k3", Err(Ack::Confirm("Send".into())))).unwrap();
        assert_eq!(
            confirm,
            json!({ "t": "ack", "id": "k3", "ok": false, "error": "confirm", "label": "Send" })
        );
        let none: Value = serde_json::from_str(&caret(None)).unwrap();
        assert_eq!(none, json!({ "t": "caret" }));
    }
}
