//! `press_keys`: press a keyboard shortcut in the focused app.
//!
//! "Reopen the tab I closed" is Ctrl+Shift+T, "next tab" is Ctrl+Tab, "go to
//! the address bar" is Ctrl+L: a great deal of what people ask a computer to do
//! is one shortcut away, and this tool is how command mode reaches all of it
//! without a tool per app.
//!
//! Three things keep it from being a liability.
//!
//! **The combo is parsed here, not trusted.** Models write shortcuts every way
//! people do ("Ctrl+Shift+T", "cmd+t", "control + page down", "up arrow"), so
//! [`parse_combo`] accepts the usual aliases and refuses anything that is not
//! exactly one key plus modifiers, with a sentence the model can repair from.
//! "primary" / "mod" mean Cmd on a Mac and Ctrl elsewhere, so a model that
//! knows the user's OS from the context line and one that does not both work.
//!
//! **Keys that can send or submit wait for the user** ([`is_send_combo`]):
//! Enter with any modifiers or none, Alt+S (Outlook), Cmd+Shift+D (Apple Mail)
//! and Ctrl+M / Ctrl+J (Enter in a terminal). After a `type_text`, Enter posts
//! the message in Slack, Teams or WhatsApp and runs the command in a terminal,
//! which is the irreversible half of a send, and sends need the user's
//! go-ahead in both tiers. Such a press comes back as a confirmation naming
//! the window in front, and [`confirm`] presses it only if that same window
//! (same id, same title) is still in front when the user says yes.
//!
//! **Some combos are refused outright** ([`deny_reason`]):
//! - closing or quitting the app: Alt+F4, Ctrl+Q and Ctrl+Shift+Q outside a
//!   Mac, Cmd+Q (and Cmd+Shift+Q, log out) on a Mac;
//! - closing a whole window or every window: primary+Shift+W, primary+Alt+W
//!   (Cmd+Option+W closes them all on a Mac);
//! - closing a run of tabs: primary+W or Ctrl+F4 more than once in one call;
//! - the secure-attention, lock and log-out keys: Ctrl+Alt+Delete, Win/Super+L
//!   outside a Mac, Ctrl+Alt+L on Linux, Ctrl+Cmd+Q on a Mac (under Cmd+Q),
//!   Ctrl+Alt+Backspace (ends the session on some Linux desktops);
//! - deleting past the bin: Shift+Delete, Cmd+Option+Backspace, and
//!   Cmd+Shift+Backspace (empties the Trash, whose dialog a following Enter
//!   would accept).
//!
//! A refusal is not escalated, since the computer-use fallback pressing the
//! same keys would be exactly as wrong.

use serde_json::{json, Value};

use crate::command_mode::effects::{is_known_key, KeyCombo, WindowEntry};
use crate::command_mode::registry::{
    arg_str, Feedback, Modifier, PendingAction, Platform, Risk, ToolCtx, ToolOutcome, ToolSpec,
    NOT_WAYLAND,
};

/// How many times one call may press its combo. Enough for "next tab three
/// times" or "scroll down a page five times", not enough to hold a key down.
const MAX_REPEAT: i64 = 20;

pub static SPEC: ToolSpec = ToolSpec {
    name: "press_keys",
    description: "Press a keyboard shortcut or a single key in the app that has the focus, e.g. \
                  \"ctrl+shift+t\" to reopen a closed browser tab, \"ctrl+l\" for the address bar, \
                  \"pagedown\". Write it as modifiers and one key joined by +; \"primary\" means \
                  Cmd on a Mac and Ctrl elsewhere. Keys that can send or submit (Enter with or \
                  without modifiers, Alt+S) are only pressed after the user confirms. Closing or \
                  quitting apps or windows, and locking the screen, are refused.",
    schema,
    // Most presses are immediate; only a send combo waits (see the module
    // docs), and its pressing half is `confirm`.
    risk: Risk::NeedsConfirmation,
    // Wayland forbids synthetic input from an ordinary client.
    platforms: NOT_WAYLAND,
    keywords: &[
        "press", "key", "keys", "shortcut", "hotkey", "tab", "enter", "escape", "undo", "redo",
        "refresh", "reload", "scroll", "page down", "page up", "zoom",
    ],
    handler: run,
    on_confirm: Some(confirm),
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "keys": {
                "type": "string",
                "description": "Modifiers and exactly one key joined by +, e.g. \"ctrl+shift+t\", \"alt+tab\", \"pagedown\", \"primary+l\". Modifiers: ctrl, alt, shift, meta (Win/Cmd/Super), primary.",
                "minLength": 1,
                "maxLength": 64
            },
            "repeat": {
                "type": "integer",
                "description": "How many times to press it. Defaults to 1.",
                "minimum": 1,
                "maximum": MAX_REPEAT
            }
        },
        "required": ["keys"],
        "additionalProperties": false
    })
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(keys) = arg_str(&args, "keys") else {
        return ToolOutcome::failed("No keys were given to press.", false);
    };
    let repeat = match checked_repeat(whole_number(&args, "repeat")) {
        Ok(repeat) => repeat,
        Err(refusal) => return refusal,
    };
    let combo = match checked_combo(keys, repeat, ctx.platform) {
        Ok(combo) => combo,
        Err(refusal) => return refusal,
    };

    if is_send_combo(&combo, ctx.platform) {
        return ask_before_sending(ctx, keys, &combo, repeat);
    }
    press(ctx, &combo, repeat)
}

/// The repeat count, 1 when absent, or the refusal for one out of range.
fn checked_repeat(repeat: Option<i64>) -> Result<i64, ToolOutcome> {
    match repeat {
        None => Ok(1),
        Some(n) if (1..=MAX_REPEAT).contains(&n) => Ok(n),
        Some(n) => Err(ToolOutcome::failed(
            format!("Can't press keys {n} times; it has to be between 1 and {MAX_REPEAT}."),
            false,
        )),
    }
}

/// Parse `keys` and check it against the deny list: the combo to press, or
/// the refusal.
fn checked_combo(keys: &str, repeat: i64, platform: Platform) -> Result<KeyCombo, ToolOutcome> {
    let combo = parse_combo(keys, platform).map_err(|e| ToolOutcome::failed(e, false))?;
    if let Some(reason) = deny_reason(&combo, repeat, platform) {
        let label = combo.label();
        log::info!("command mode: refused to press {label}: {reason}");
        return Err(ToolOutcome::failed(
            format!("I won't press {label}: {reason}"),
            false,
        ));
    }
    Ok(combo)
}

/// A send combo: name the window that would get it and wait for the user.
/// The payload keeps the keys as written (re-parsed and re-checked at
/// confirmation) and the window, which [`confirm`] requires to still be in
/// front.
fn ask_before_sending(ctx: &ToolCtx, keys: &str, combo: &KeyCombo, repeat: i64) -> ToolOutcome {
    let label = combo.label();
    let front = ctx.effects.foreground_window();
    let place = front
        .as_ref()
        .map(window_name)
        .unwrap_or_else(|| "the window in front".to_string());
    let action = if repeat == 1 {
        format!("Press {label}")
    } else {
        format!("Press {label} {repeat} times")
    };
    log::info!("command mode: {label} can send or submit; asking first");
    ToolOutcome::NeedsConfirmation {
        pending: PendingAction {
            tool: "press_keys",
            summary: format!("{action} in {place}"),
            payload: json!({
                "keys": keys,
                "repeat": repeat,
                "window": front.map(|w| json!({"id": w.id, "title": w.title})),
            }),
        },
        feedback: Feedback::new(
            "pressKeysConfirm",
            "{{keys}} could send or submit something in {{window}}. Say yes to press it, or cancel.",
            &[("keys", label.as_str()), ("window", place.as_str())],
        ),
    }
}

/// “Slack | general”, quoted; the app's name when the window has no title.
fn window_name(window: &WindowEntry) -> String {
    let title = window.title.trim();
    let name = if title.is_empty() {
        window.app.trim()
    } else {
        title
    };
    if name.is_empty() {
        "the window in front".to_string()
    } else {
        format!("\u{201c}{name}\u{201d}")
    }
}

/// Press a send combo the user confirmed, but only into the window it was
/// confirmed for: the foreground is read once more, immediately before the
/// press, and must be the same window with the same title. A dry run only
/// records the press.
fn confirm(ctx: &ToolCtx, payload: Value) -> ToolOutcome {
    let Some(keys) = payload.get("keys").and_then(Value::as_str) else {
        return ToolOutcome::failed("The keys to press were lost, so nothing was pressed.", false);
    };
    let repeat = match checked_repeat(Some(whole_number(&payload, "repeat").unwrap_or(1))) {
        Ok(repeat) => repeat,
        Err(refusal) => return refusal,
    };
    let combo = match checked_combo(keys, repeat, ctx.platform) {
        Ok(combo) => combo,
        Err(refusal) => return refusal,
    };
    if ctx.dry_run {
        return press(ctx, &combo, repeat);
    }

    let label = combo.label();
    let Some(expected) = stored_window(&payload) else {
        log::warn!("command mode: no window was known for {label}; not pressing it");
        return ToolOutcome::failed(
            format!("I couldn't tell which window would get {label}, so I didn't press it."),
            false,
        );
    };
    match ctx.effects.foreground_window() {
        Some(front)
            if front.id == expected.id
                && front.title == expected.title
                && ctx.effects.foreground_is_exactly(expected.id) => {}
        _ => {
            log::warn!("command mode: the window in front changed before {label}; not pressing it");
            return ToolOutcome::failed(
                format!("The window in front has changed, so I didn't press {label}."),
                false,
            );
        }
    }
    press(ctx, &combo, repeat)
}

/// The `{id, title}` [`ask_before_sending`] recorded; `None` for `null`.
fn stored_window(payload: &Value) -> Option<WindowEntry> {
    let window = payload.get("window")?;
    Some(WindowEntry {
        id: window.get("id")?.as_i64()?,
        title: window.get("title")?.as_str()?.to_string(),
        app: String::new(),
    })
}

/// Press `combo` `repeat` times and say so.
fn press(ctx: &ToolCtx, combo: &KeyCombo, repeat: i64) -> ToolOutcome {
    let label = combo.label();
    for press in 0..repeat {
        if let Err(e) = ctx.effects.press_keys(combo) {
            log::warn!(
                "command mode: pressing {label} failed on press {} of {repeat}: {e}",
                press + 1
            );
            return ToolOutcome::failed(e, false);
        }
    }

    if repeat == 1 {
        ToolOutcome::done("pressedKeys", "Pressed {{keys}}.", &[("keys", label.as_str())])
    } else {
        let count = repeat.to_string();
        ToolOutcome::done(
            "pressedKeysRepeat",
            "Pressed {{keys}} {{count}} times.",
            &[("keys", label.as_str()), ("count", count.as_str())],
        )
    }
}

/// An integer argument, accepting a whole-number float ("3.0") the way the
/// schema validator does; `arg_i64` alone would read that as absent.
fn whole_number(args: &Value, key: &str) -> Option<i64> {
    let value = args.get(key)?;
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.is_finite() && f.fract() == 0.0)
            .map(|f| f as i64)
    })
}

/// Parse "ctrl+shift+t" into a [`KeyCombo`]. Case-insensitive, spaces around
/// the `+` ignored, modifiers deduplicated and put in a fixed order (Ctrl, Alt,
/// Shift, Meta) so the same shortcut always reads the same way. Errors are
/// phrased for the model to correct itself from.
pub(crate) fn parse_combo(keys: &str, platform: Platform) -> Result<KeyCombo, String> {
    let mut modifiers: Vec<Modifier> = Vec::new();
    let mut key: Option<String> = None;

    for raw in keys.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            return Err(format!(
                "\"{keys}\" has an empty part; write it like \"ctrl+shift+t\"."
            ));
        }
        if let Some(modifier) = modifier_named(part, platform) {
            if !modifiers.contains(&modifier) {
                modifiers.push(modifier);
            }
            continue;
        }
        let Some(name) = key_named(part) else {
            return Err(format!(
                "\"{part}\" is not a key I can press. Use a letter, a digit, f1-f12, or one of: {}.",
                crate::command_mode::effects::KEY_NAMES.join(", ")
            ));
        };
        if let Some(first) = &key {
            return Err(format!(
                "\"{keys}\" has two keys ({first} and {name}); a shortcut is modifiers plus one key. \
                 Press them in separate calls."
            ));
        }
        key = Some(name);
    }

    let Some(key) = key else {
        return Err(format!(
            "\"{keys}\" is only modifiers; add the key to press with them."
        ));
    };
    modifiers.sort_by_key(|m| modifier_rank(*m));
    Ok(KeyCombo { modifiers, key })
}

fn modifier_rank(modifier: Modifier) -> u8 {
    match modifier {
        Modifier::Ctrl => 0,
        Modifier::Alt => 1,
        Modifier::Shift => 2,
        Modifier::Meta => 3,
    }
}

fn modifier_named(part: &str, platform: Platform) -> Option<Modifier> {
    match part.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "ctl" => Some(Modifier::Ctrl),
        "alt" | "option" | "opt" | "altgr" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        "meta" | "win" | "windows" | "cmd" | "command" | "super" | "⌘" => Some(Modifier::Meta),
        "primary" | "mod" => Some(platform.primary_modifier()),
        _ => None,
    }
}

/// The canonical [`KeyCombo`] key name for what a model wrote, or `None`.
fn key_named(part: &str) -> Option<String> {
    let lower = part.to_lowercase();
    // A single punctuation character is the key that types it.
    let symbol = match lower.as_str() {
        "-" => Some("minus"),
        "=" => Some("equal"),
        "," => Some("comma"),
        "." => Some("period"),
        "/" => Some("slash"),
        "\\" => Some("backslash"),
        ";" => Some("semicolon"),
        "'" => Some("quote"),
        "[" => Some("bracketleft"),
        "]" => Some("bracketright"),
        "`" => Some("backquote"),
        _ => None,
    };
    if let Some(name) = symbol {
        return Some(name.to_string());
    }

    // "Up Arrow", "page_down", "arrow-left" all compact to one spelling.
    let compact: String = lower
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .collect();
    let alias = match compact.as_str() {
        "return" | "ret" => "enter",
        "esc" => "escape",
        "del" | "forwarddelete" => "delete",
        "ins" => "insert",
        "bksp" | "backspace" => "backspace",
        "spacebar" => "space",
        "pgup" | "pageup" => "pageup",
        "pgdn" | "pgdown" | "pagedown" => "pagedown",
        "up" | "uparrow" | "arrowup" => "up",
        "down" | "downarrow" | "arrowdown" => "down",
        "left" | "leftarrow" | "arrowleft" => "left",
        "right" | "rightarrow" | "arrowright" => "right",
        "dash" | "hyphen" => "minus",
        "equals" => "equal",
        "dot" | "fullstop" => "period",
        "apostrophe" => "quote",
        "backtick" | "grave" => "backquote",
        "leftbracket" => "bracketleft",
        "rightbracket" => "bracketright",
        other => other,
    };
    is_known_key(alias).then(|| alias.to_string())
}

/// Whether `combo` can send a message or submit something, so it is pressed
/// only after the user confirms. See the module docs.
pub(crate) fn is_send_combo(combo: &KeyCombo, platform: Platform) -> bool {
    let has = |m: Modifier| combo.modifiers.contains(&m);
    let only = |m: Modifier| combo.modifiers == [m];
    match combo.key.as_str() {
        // Bare Enter sends in every chat app and runs a typed terminal
        // command; Shift, Ctrl, Cmd and Alt variants send in mail and chat
        // apps that keep bare Enter for a new line.
        "enter" => true,
        // Send in classic Outlook for Windows (and Alt+S submits forms that
        // set it as an access key). On a Mac, Option+S only types a letter.
        "s" => platform != Platform::MacOs && only(Modifier::Alt),
        // Send in Apple Mail.
        "d" => platform == Platform::MacOs && has(Modifier::Meta) && has(Modifier::Shift),
        // Carriage return and line feed: Enter in a terminal.
        "m" | "j" => only(Modifier::Ctrl),
        _ => false,
    }
}

/// Why `combo`, pressed `repeat` times, is refused here, or `None` to press it.
/// See the module docs for why each is on the list.
pub(crate) fn deny_reason(
    combo: &KeyCombo,
    repeat: i64,
    platform: Platform,
) -> Option<&'static str> {
    let has = |m: Modifier| combo.modifiers.contains(&m);
    let primary = platform.primary_modifier();
    let mac = platform == Platform::MacOs;
    let linux = matches!(platform, Platform::LinuxX11 | Platform::LinuxWayland);
    let key = combo.key.as_str();

    if key == "f4" && has(Modifier::Alt) {
        return Some("it closes the app, and anything unsaved in it.");
    }
    if key == "q" && mac && has(Modifier::Meta) {
        return Some("it quits the app (with Shift it logs you out, with Ctrl it locks the screen).");
    }
    // Win+Q is search on Windows, so only Ctrl quits outside a Mac; on a Mac
    // Ctrl+Q alone does nothing of the kind.
    if key == "q" && !mac && has(Modifier::Ctrl) {
        return Some("it quits the app, and anything unsaved in it.");
    }
    if key == "w" && has(primary) && (has(Modifier::Shift) || has(Modifier::Alt)) {
        return Some("it closes the whole window, or every window, and every tab in them.");
    }
    let closes_a_tab = (key == "w" && has(primary)) || (key == "f4" && has(Modifier::Ctrl));
    if closes_a_tab && repeat > 1 {
        return Some("closing several tabs or windows at once can lose work. Close one at a time.");
    }
    if key == "delete" && has(Modifier::Ctrl) && has(Modifier::Alt) {
        return Some("it opens the system security screen.");
    }
    if key == "backspace" && has(Modifier::Ctrl) && has(Modifier::Alt) {
        return Some("on some Linux desktops it ends the whole session.");
    }
    if key == "l" && has(Modifier::Meta) && !has(Modifier::Shift) && !has(Modifier::Alt) && !mac {
        // Win+L locks Windows and many Linux desktops; on a Mac Cmd+L is the
        // address bar, so it is only refused where it locks.
        return Some("it locks the computer.");
    }
    if key == "l" && linux && has(Modifier::Ctrl) && has(Modifier::Alt) {
        return Some("it locks the computer.");
    }
    if key == "delete" && has(Modifier::Shift) {
        return Some("it deletes files permanently, skipping the Recycle Bin.");
    }
    if mac && key == "backspace" && has(Modifier::Meta) && has(Modifier::Alt) {
        return Some("it deletes files permanently, skipping the Trash.");
    }
    if mac && key == "backspace" && has(Modifier::Meta) && has(Modifier::Shift) {
        return Some("it empties the Trash.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::registry::validate_args;
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    fn ctx_on(platform: Platform, effects: Arc<FakeEffects>) -> ToolCtx {
        ToolCtx {
            settings: crate::settings::get_default_settings(),
            context: CommandContext::at(
                Utc.with_ymd_and_hms(2026, 10, 22, 9, 0, 0).unwrap(),
                None,
                platform,
                None,
                None,
            ),
            effects,
            contacts: Arc::new(ContactBook::new(Vec::new())),
            platform,
            dry_run: false,
        }
    }

    fn combo(modifiers: &[Modifier], key: &str) -> KeyCombo {
        KeyCombo {
            modifiers: modifiers.to_vec(),
            key: key.to_string(),
        }
    }

    /// Every spelling a model is likely to write lands on the same combo.
    #[test]
    fn shortcuts_parse_with_the_usual_aliases() {
        use Modifier::*;
        let cases: &[(&str, KeyCombo)] = &[
            ("ctrl+shift+t", combo(&[Ctrl, Shift], "t")),
            ("Shift + Control + T", combo(&[Ctrl, Shift], "t")),
            ("CTRL+L", combo(&[Ctrl], "l")),
            ("alt+tab", combo(&[Alt], "tab")),
            ("option+left", combo(&[Alt], "left")),
            ("cmd+shift+z", combo(&[Shift, Meta], "z")),
            ("win+d", combo(&[Meta], "d")),
            ("super+e", combo(&[Meta], "e")),
            ("command+1", combo(&[Meta], "1")),
            ("enter", combo(&[], "enter")),
            ("return", combo(&[], "enter")),
            ("esc", combo(&[], "escape")),
            ("del", combo(&[], "delete")),
            ("pgup", combo(&[], "pageup")),
            ("ctrl+pgdn", combo(&[Ctrl], "pagedown")),
            ("page down", combo(&[], "pagedown")),
            ("up arrow", combo(&[], "up")),
            ("ArrowDown", combo(&[], "down")),
            ("arrow_left", combo(&[], "left")),
            ("shift+right arrow", combo(&[Shift], "right")),
            ("F5", combo(&[], "f5")),
            ("ctrl+-", combo(&[Ctrl], "minus")),
            ("ctrl+=", combo(&[Ctrl], "equal")),
            ("ctrl+ctrl+s", combo(&[Ctrl], "s")),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_combo(input, Platform::Windows).as_ref(),
                Ok(expected),
                "parsing {input:?}"
            );
        }
    }

    /// "primary" is the one alias that depends on where it runs.
    #[test]
    fn primary_means_cmd_on_a_mac_and_ctrl_elsewhere() {
        assert_eq!(
            parse_combo("primary+t", Platform::Windows),
            Ok(combo(&[Modifier::Ctrl], "t"))
        );
        assert_eq!(
            parse_combo("mod+t", Platform::LinuxX11),
            Ok(combo(&[Modifier::Ctrl], "t"))
        );
        assert_eq!(
            parse_combo("primary+shift+t", Platform::MacOs),
            Ok(combo(&[Modifier::Shift, Modifier::Meta], "t"))
        );
    }

    /// A malformed combo comes back with a reason, never a half-guess.
    #[test]
    fn malformed_combos_are_refused_with_a_reason() {
        for bad in [
            "ctrl+a+b",
            "ctrl+shift",
            "shift",
            "ctrl+hyper",
            "ctrl+",
            "+t",
            "ctrl+é",
            "f13",
        ] {
            let result = parse_combo(bad, Platform::Windows);
            assert!(result.is_err(), "{bad:?} parsed as {result:?}");
        }
        let two = parse_combo("a+b", Platform::Windows).unwrap_err();
        assert!(two.contains("two keys"), "{two}");
        let only = parse_combo("ctrl+alt", Platform::Windows).unwrap_err();
        assert!(only.contains("only modifiers"), "{only}");
        let unknown = parse_combo("ctrl+hyper", Platform::Windows).unwrap_err();
        assert!(unknown.contains("\"hyper\""), "{unknown}");
    }

    /// The deny list: each entry refused, and its harmless neighbours allowed.
    #[test]
    fn destructive_and_system_combos_are_refused() {
        let win = Platform::Windows;
        let mac = Platform::MacOs;
        let x11 = Platform::LinuxX11;
        let refused: &[(&str, i64, Platform)] = &[
            ("alt+f4", 1, win),
            ("ctrl+alt+delete", 1, win),
            ("ctrl+alt+del", 1, x11),
            ("win+l", 1, win),
            ("super+l", 1, x11),
            ("ctrl+alt+l", 1, x11),
            ("ctrl+alt+l", 1, Platform::LinuxWayland),
            ("ctrl+w", 2, win),
            ("ctrl+f4", 2, win),
            ("ctrl+shift+w", 1, win),
            ("ctrl+shift+w", 3, win),
            ("ctrl+alt+w", 1, x11),
            ("cmd+w", 2, mac),
            ("cmd+shift+w", 1, mac),
            ("cmd+option+w", 1, mac),
            ("ctrl+q", 1, win),
            ("ctrl+q", 1, x11),
            ("ctrl+shift+q", 1, win),
            ("ctrl+shift+q", 1, x11),
            ("cmd+q", 1, mac),
            ("cmd+shift+q", 1, mac),
            ("ctrl+cmd+q", 1, mac),
            ("shift+delete", 1, win),
            ("cmd+option+backspace", 1, mac),
            ("cmd+shift+backspace", 1, mac),
        ];
        for (keys, repeat, platform) in refused {
            let parsed = parse_combo(keys, *platform).unwrap();
            assert!(
                deny_reason(&parsed, *repeat, *platform).is_some(),
                "{keys} x{repeat} on {platform:?} was allowed"
            );
        }
        let allowed: &[(&str, i64, Platform)] = &[
            ("ctrl+w", 1, win),
            ("cmd+w", 1, mac),
            ("ctrl+f4", 1, win),
            ("ctrl+shift+t", 5, win),
            ("cmd+l", 1, mac),
            ("ctrl+l", 1, x11),
            ("ctrl+alt+l", 1, win),
            ("delete", 1, win),
            ("ctrl+delete", 1, win),
            ("cmd+backspace", 1, mac),
            ("f4", 1, win),
            ("win+q", 1, win),
            ("ctrl+q", 1, mac),
            ("ctrl+tab", 20, win),
            // Sends are not refused: they wait for the user instead.
            ("enter", 1, win),
            ("ctrl+enter", 1, win),
            ("cmd+enter", 1, mac),
        ];
        for (keys, repeat, platform) in allowed {
            let parsed = parse_combo(keys, *platform).unwrap();
            assert_eq!(
                deny_reason(&parsed, *repeat, *platform),
                None,
                "{keys} x{repeat} on {platform:?} was refused"
            );
        }
    }

    /// Every key that can send or submit is caught, on the platforms where it
    /// does, and ordinary shortcuts are not.
    #[test]
    fn send_and_submit_keys_are_recognised() {
        let win = Platform::Windows;
        let mac = Platform::MacOs;
        let sends: &[(&str, Platform)] = &[
            ("enter", win),
            ("return", win),
            ("shift+enter", win),
            ("alt+enter", win),
            ("ctrl+enter", win),
            ("ctrl+shift+enter", win),
            ("win+enter", win),
            ("cmd+enter", mac),
            ("cmd+shift+enter", mac),
            ("alt+s", win),
            ("alt+s", Platform::LinuxX11),
            ("cmd+shift+d", mac),
            ("ctrl+m", win),
            ("ctrl+j", Platform::LinuxX11),
        ];
        for (keys, platform) in sends {
            let parsed = parse_combo(keys, *platform).unwrap();
            assert!(is_send_combo(&parsed, *platform), "{keys} on {platform:?} is a send");
        }
        let ordinary: &[(&str, Platform)] = &[
            ("ctrl+s", win),
            ("ctrl+shift+s", win),
            ("option+s", mac),
            ("cmd+d", mac),
            ("ctrl+shift+d", win),
            ("ctrl+shift+m", win),
            ("tab", win),
            ("ctrl+shift+t", win),
            ("pagedown", win),
        ];
        for (keys, platform) in ordinary {
            let parsed = parse_combo(keys, *platform).unwrap();
            assert!(!is_send_combo(&parsed, *platform), "{keys} on {platform:?} is not a send");
        }
    }

    /// A refused combo never reaches the keyboard and is not escalated.
    #[test]
    fn a_refused_combo_presses_nothing() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(
            &ctx_on(Platform::Windows, fake.clone()),
            json!({"keys": "ctrl+shift+q"}),
        );
        match outcome {
            ToolOutcome::Failed { reason, escalate } => {
                assert!(!escalate);
                assert!(reason.contains("Ctrl+Shift+Q"), "{reason}");
                assert!(reason.contains("quits"), "{reason}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(fake.actions().is_empty());
    }

    fn slack() -> WindowEntry {
        WindowEntry {
            id: 42,
            title: "general | Acme - Slack".into(),
            app: "slack".into(),
        }
    }

    fn with_foreground(window: Option<WindowEntry>) -> Arc<FakeEffects> {
        Arc::new(FakeEffects {
            foreground: std::sync::Mutex::new(window),
            ..Default::default()
        })
    }

    /// "Tell the team I'm late" plans type_text then Enter. The Enter does
    /// not post: it waits, naming the window it would go to.
    #[test]
    fn enter_waits_for_the_user_and_names_the_window() {
        let fake = with_foreground(Some(slack()));
        let outcome = run(
            &ctx_on(Platform::Windows, fake.clone()),
            json!({"keys": "enter"}),
        );
        let ToolOutcome::NeedsConfirmation { pending, feedback } = outcome else {
            panic!("expected a confirmation, got {outcome:?}");
        };
        assert_eq!(pending.tool, "press_keys");
        assert_eq!(
            pending.summary,
            "Press Enter in \u{201c}general | Acme - Slack\u{201d}"
        );
        assert_eq!(
            pending.payload,
            json!({
                "keys": "enter",
                "repeat": 1,
                "window": {"id": 42, "title": "general | Acme - Slack"}
            })
        );
        assert_eq!(feedback.key, "commandMode.feedback.pressKeysConfirm");
        assert!(fake.actions().is_empty(), "{:?}", fake.actions());
    }

    /// Confirmed with the same window still in front: pressed, and only then.
    #[test]
    fn a_confirmed_send_is_pressed_in_the_same_window() {
        let fake = with_foreground(Some(slack()));
        let ctx = ctx_on(Platform::Windows, fake.clone());
        let ToolOutcome::NeedsConfirmation { pending, .. } =
            run(&ctx, json!({"keys": "ctrl+enter"}))
        else {
            panic!("expected a confirmation");
        };
        let outcome = confirm(&ctx, pending.payload);
        assert!(matches!(outcome, ToolOutcome::Done { .. }), "{outcome:?}");
        assert_eq!(fake.actions(), vec!["press_keys Ctrl+Enter".to_string()]);
    }

    /// Another window in front by the time the user says yes, or the same
    /// window now showing something else (another channel, another tab):
    /// nothing is pressed.
    #[test]
    fn a_confirmed_send_is_not_pressed_once_the_window_changed() {
        let payload = json!({
            "keys": "enter",
            "repeat": 1,
            "window": {"id": 42, "title": "general | Acme - Slack"}
        });
        let elsewhere = [
            Some(WindowEntry {
                id: 7,
                title: "Windows PowerShell".into(),
                app: "powershell".into(),
            }),
            Some(WindowEntry {
                id: 42,
                title: "random | Acme - Slack".into(),
                app: "slack".into(),
            }),
            None,
        ];
        for front in elsewhere {
            let fake = with_foreground(front.clone());
            let outcome = confirm(&ctx_on(Platform::Windows, fake.clone()), payload.clone());
            assert!(
                matches!(outcome, ToolOutcome::Failed { escalate: false, .. }),
                "{front:?}: {outcome:?}"
            );
            assert!(fake.actions().is_empty(), "{front:?}: {:?}", fake.actions());
        }
    }

    /// With no window known when it was asked, there is nothing to check the
    /// foreground against, so the key is not pressed.
    #[test]
    fn a_send_with_no_known_window_is_never_pressed() {
        let fake = with_foreground(None);
        let ctx = ctx_on(Platform::Windows, fake.clone());
        let ToolOutcome::NeedsConfirmation { pending, .. } = run(&ctx, json!({"keys": "alt+s"}))
        else {
            panic!("expected a confirmation");
        };
        assert_eq!(pending.payload["window"], Value::Null);
        assert_eq!(pending.summary, "Press Alt+S in the window in front");
        // The window that is in front by then is not the one it was meant for.
        *fake.foreground.lock().unwrap() = Some(slack());
        assert!(matches!(
            confirm(&ctx, pending.payload),
            ToolOutcome::Failed { .. }
        ));
        assert!(fake.actions().is_empty());
    }

    /// A dry run still asks, and its confirmation only records the presses.
    #[test]
    fn a_dry_run_send_is_recorded_without_checking_windows() {
        let fake = with_foreground(None);
        let mut ctx = ctx_on(Platform::Windows, fake.clone());
        ctx.dry_run = true;
        let ToolOutcome::NeedsConfirmation { pending, .. } =
            run(&ctx, json!({"keys": "enter", "repeat": 2}))
        else {
            panic!("expected a confirmation");
        };
        assert_eq!(pending.summary, "Press Enter 2 times in the window in front");
        assert!(matches!(
            confirm(&ctx, pending.payload),
            ToolOutcome::Done { .. }
        ));
        assert_eq!(fake.actions().len(), 2);
    }

    /// The payload is re-checked at confirmation: a deny-listed combo slipped
    /// into it is still refused.
    #[test]
    fn a_confirmation_rechecks_the_deny_list() {
        let fake = with_foreground(Some(slack()));
        let payload = json!({
            "keys": "alt+f4",
            "repeat": 1,
            "window": {"id": 42, "title": "general | Acme - Slack"}
        });
        assert!(matches!(
            confirm(&ctx_on(Platform::Windows, fake.clone()), payload),
            ToolOutcome::Failed { .. }
        ));
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn a_shortcut_is_pressed_and_named_in_the_feedback() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(
            &ctx_on(Platform::Windows, fake.clone()),
            json!({"keys": "Control+Shift+t"}),
        );
        match outcome {
            ToolOutcome::Done { feedback } => {
                assert_eq!(feedback.render_en(), "Pressed Ctrl+Shift+T.");
                assert_eq!(feedback.key, "commandMode.feedback.pressedKeys");
            }
            other => panic!("expected Done, got {other:?}"),
        }
        assert_eq!(fake.actions(), vec!["press_keys Ctrl+Shift+T".to_string()]);
    }

    /// "Next tab three times" is three presses and says so.
    #[test]
    fn repeat_presses_the_combo_that_many_times() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(
            &ctx_on(Platform::Windows, fake.clone()),
            json!({"keys": "ctrl+tab", "repeat": 3}),
        );
        match outcome {
            ToolOutcome::Done { feedback } => {
                assert_eq!(feedback.render_en(), "Pressed Ctrl+Tab 3 times.");
            }
            other => panic!("expected Done, got {other:?}"),
        }
        assert_eq!(fake.actions().len(), 3);

        // A whole-number float counts the same, as the validator allows it.
        let fake = Arc::new(FakeEffects::default());
        run(
            &ctx_on(Platform::Windows, fake.clone()),
            json!({"keys": "down", "repeat": 2.0}),
        );
        assert_eq!(fake.actions().len(), 2);
    }

    /// Out-of-range repeats are refused by the schema and again by the handler.
    #[test]
    fn repeat_outside_the_range_is_refused() {
        for repeat in [0, 21, -1] {
            let args = json!({"keys": "down", "repeat": repeat});
            assert!(validate_args(&SPEC, &args).is_err(), "schema took {repeat}");
            let fake = Arc::new(FakeEffects::default());
            assert!(matches!(
                run(&ctx_on(Platform::Windows, fake.clone()), args),
                ToolOutcome::Failed { .. }
            ));
            assert!(fake.actions().is_empty());
        }
        assert!(validate_args(&SPEC, &json!({"keys": "down", "repeat": 20})).is_ok());
    }

    #[test]
    fn a_failing_keyboard_stops_and_reports_the_error() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("The keyboard could not be reached.".into()),
            ..Default::default()
        });
        match run(
            &ctx_on(Platform::Windows, fake),
            json!({"keys": "ctrl+tab", "repeat": 4}),
        ) {
            ToolOutcome::Failed { reason, escalate } => {
                assert_eq!(reason, "The keyboard could not be reached.");
                assert!(!escalate);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// A bad combo is reported to the model, which can repair it.
    #[test]
    fn an_unparseable_combo_fails_without_pressing() {
        let fake = Arc::new(FakeEffects::default());
        assert!(matches!(
            run(
                &ctx_on(Platform::Windows, fake.clone()),
                json!({"keys": "ctrl+a+b"})
            ),
            ToolOutcome::Failed { escalate: false, .. }
        ));
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn pressing_keys_is_not_offered_on_wayland() {
        assert!(!SPEC.supports(Platform::LinuxWayland));
        assert!(SPEC.supports(Platform::LinuxX11));
    }
}
