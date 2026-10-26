//! Whether a command carries out its irreversible step (send, buy, post,
//! delete) itself, or stops and waits for the user's Do it.
//!
//! **It carries it out.** Someone who says "email Ron that I'll be late"
//! asked for an email to Ron, not a draft to review; stopping at a card made
//! every such command half-finished, and the user had to come back and press
//! a button for what they had already said. A command waits only when the
//! user's own words ask it to ("let me check it first", "don't send it yet",
//! "draft an email", "ask before sending"), or when the user switched on
//! "Ask before irreversible actions" (`command_confirm_irreversible`).
//!
//! The words are judged twice. The router model sets [`CONFIRM_FIRST`] on the
//! one call they apply to, since it understands phrasing no list does; and
//! [`asks_to_review`] reads the transcript for the common phrasings, English
//! and Nepali in Latin letters, as a backstop for a model that misses one. A
//! false positive only means waiting, which is the old behaviour, so the list
//! leans toward matching.
//!
//! What is never done is a separate matter and stays where it is: the
//! `press_keys` deny list (shutting down, signing out, Alt+F4) and the
//! refusal to close windows are safety rules, not review.

use serde_json::{json, Value};

/// The argument the router sets on a call that must wait for the user.
pub const CONFIRM_FIRST: &str = "confirm_first";

/// The schema property every tool with an irreversible step accepts, and the
/// screen agent's escalation too. Added by the registry, not written into
/// each tool, so a new tool of that kind gets it without remembering to.
pub fn confirm_first_property() -> Value {
    json!({
        "type": "boolean",
        "description": "true ONLY when the user's own words ask to review or be asked first \
                        (\"let me check it\", \"don't send yet\", \"just draft it\", \"ask \
                        before sending\", \"wait for me\"). Leave it out otherwise: the step \
                        is then carried out, sent and all, because that is what was asked."
    })
}

/// `schema` with [`CONFIRM_FIRST`] among its properties.
pub fn with_confirm_first(mut schema: Value) -> Value {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(CONFIRM_FIRST.to_string(), confirm_first_property());
    }
    schema
}

/// Whether a call's arguments ask to wait.
pub fn call_wants_review(args: &Value) -> bool {
    args.get(CONFIRM_FIRST)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Phrases that ask to see something before it goes, compared on the
/// transcript lowercased with its punctuation turned to spaces and its
/// apostrophes dropped ("don't" reads "dont").
const REVIEW_PHRASES: &[&str] = &[
    // English
    "wait for me",
    "ask before",
    "ask me before",
    "ask me first",
    "check with me",
    "confirm with me",
    "confirm first",
    "let me review",
    "let me check",
    "let me see",
    "let me look",
    "let me read",
    "let me edit",
    "for me to review",
    "for me to check",
    "i will send",
    "ill send",
    "i will review",
    "dont send",
    "do not send",
    "dont post",
    "do not post",
    "not send it yet",
    "before sending",
    "before you send",
    "before it goes",
    "draft",
    "just prepare",
    "only prepare",
    "just write it",
    "just open it",
    // Nepali in Latin letters
    "napathau",
    "na pathau",
    "napathaunu",
    "pathaunu aghi",
    "pathauna aghi",
    "pathaunu agadi",
    "pathauna agadi",
    "sodhera",
    "sodhnu",
    "ma herchu",
    "ma hercu",
    "malai dekhau",
    "malai dekhaau",
    "ma pathauchu",
    "ma aafai pathauchu",
    "parkha",
    "pakha",
];

/// Whether the user's own words ask to review before anything irreversible
/// happens. See the module notes: a backstop for the router's
/// [`CONFIRM_FIRST`], leaning toward matching.
pub fn asks_to_review(transcript: &str) -> bool {
    let normalized: String = transcript
        .to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '\u{2019}')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let padded = format!(" {} ", words.join(" "));
    REVIEW_PHRASES.iter().any(|phrase| {
        if phrase.contains(' ') {
            padded.contains(&format!(" {phrase} "))
        } else {
            // One word: a whole word, or the start of one ("drafted",
            // "drafts"), never inside another word.
            words.iter().any(|word| word.starts_with(phrase))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_requests_are_carried_out() {
        for said in [
            "send an email to Ron about the meeting at 9",
            "email Ron that I'll be late",
            "ram lai email pathau ki ma bholi aauchhu",
            "post this in the team chat",
            "press enter",
            "reply to Sita saying thanks",
            "open notepad and type hello",
            "send it",
            "redraw the chart", // "draft" only as a word start
        ] {
            assert!(!asks_to_review(said), "{said}");
        }
    }

    #[test]
    fn asking_to_review_waits() {
        for said in [
            "email Ron about the meeting but let me check it first",
            "write an email to Ron, don't send it yet",
            "Don\u{2019}t send it, I want to read it",
            "draft an email to Ron about the budget",
            "ask before sending",
            "Just wait for me before you send it",
            "prepare the email for me to review",
            "ram lai email lekha tara napathau",
            "email lekha, pathaunu aghi malai dekhau",
            "ma herchu ani pathauchu",
            "I'll send it myself",
        ] {
            assert!(asks_to_review(said), "{said}");
        }
    }

    #[test]
    fn the_router_flag_is_read_only_when_true() {
        assert!(call_wants_review(
            &json!({"to": "Ron", "confirm_first": true})
        ));
        assert!(!call_wants_review(
            &json!({"to": "Ron", "confirm_first": false})
        ));
        assert!(!call_wants_review(&json!({"to": "Ron"})));
        assert!(!call_wants_review(&json!({"confirm_first": "yes"})));
    }

    #[test]
    fn the_flag_is_added_to_a_schema_without_touching_the_rest() {
        let schema = with_confirm_first(json!({
            "type": "object",
            "properties": {"to": {"type": "string"}},
            "required": ["to"],
            "additionalProperties": false
        }));
        assert_eq!(schema["properties"]["to"]["type"], "string");
        assert_eq!(schema["properties"][CONFIRM_FIRST]["type"], "boolean");
        assert_eq!(schema["required"], json!(["to"]));
        assert!(crate::command_mode::schema::check_schema(&schema).is_ok());
        assert!(crate::command_mode::schema::validate(
            &schema,
            &json!({"to": "Ron", "confirm_first": true})
        )
        .is_ok());
    }
}
