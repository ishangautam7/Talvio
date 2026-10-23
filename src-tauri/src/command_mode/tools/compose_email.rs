//! `compose_email`: open a filled-in Gmail draft, and send it only when the
//! user says so.
//!
//! The draft is Gmail's own compose page (`view=cm&fs=1`) opened in the
//! default browser, so it is the user's real account, already signed in, and
//! they can see and edit every word before anything leaves. That is the
//! reversible half. The send is the irreversible half and runs only from
//! [`send`], after the user confirms: it brings the draft's window forward
//! and presses Gmail's send shortcut (Ctrl+Enter, Cmd+Enter on a Mac).
//!
//! **The send only ever goes to the window the draft opened in.** [`run`]
//! watches the foreground after opening the link and records the Gmail
//! window that came forward (id, app and title). [`send`] focuses exactly that
//! window, and reads the foreground again immediately before the key press:
//! same id, and the title it had (or a Gmail title naming the subject). Any
//! other window, a tab switched in the meantime, or a draft that was never
//! identified, and nothing is pressed. An earlier version took the front-most
//! window whose title merely mentioned Gmail, "compose", "new message" or the
//! subject, which matched `docker-compose.yml` in an editor, an Outlook "New
//! Message", a Teams chat named after the subject and any open Gmail reply,
//! and Ctrl+Enter in any of those sends something.
//!
//! Recipients are spoken names, so they go through the contact book, and the
//! rule is the contact book's: one clear person with an address is used,
//! anything else becomes a question. Mailing the wrong person is worse than
//! asking. A model-written "Name <address>" is not taken on trust either (see
//! [`resolve_recipient`]), and the confirmation names every address, so the
//! user confirms where the mail goes, not only to whom.

use log::{info, warn};
use reqwest::Url;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use crate::command_mode::contacts::{looks_like_email, Contact, ContactMatch};
use crate::command_mode::effects::{KeyCombo, WindowEntry};
use crate::command_mode::registry::{
    arg_str, arg_strings, Feedback, PendingAction, Risk, ToolCtx, ToolOutcome, ToolSpec,
    ALL_PLATFORMS,
};

/// Browsers and Gmail both handle far longer URLs, but some launch paths
/// (`ShellExecute`, `open`, `xdg-open` through a desktop file) and some
/// proxies cut a command line or request line around 8 KB. Staying under this
/// keeps the draft intact everywhere; a longer body is cut at a word.
const MAX_URL_LEN: usize = 7500;

/// Appended to a body that had to be cut, so the user sees it in the draft.
const TRUNCATION_MARK: &str = " …";

/// How long the draft window gets to take the keyboard before the send
/// shortcut is pressed. Activation is asynchronous on every platform, and a
/// key pressed too early lands in whatever was in front before. The
/// foreground is read again after it, so this only makes a refusal rarer.
const FOCUS_SETTLE: Duration = Duration::from_millis(400);

/// How long [`run`] watches for the draft to come forward after opening the
/// link, and how often it looks. A browser that has to start, or a busy
/// machine, takes seconds to show the tab; past this the draft is left
/// unidentified and the send refuses. Tests use a sliver of it, since most of
/// them never put a Gmail window in front.
const DRAFT_WAIT: Duration = if cfg!(test) {
    Duration::from_millis(30)
} else {
    Duration::from_secs(6)
};
const DRAFT_POLL: Duration = if cfg!(test) {
    Duration::from_millis(5)
} else {
    Duration::from_millis(250)
};

/// What every Gmail window title carries ("Compose Mail - … - Gmail"),
/// compared lowercase. The only title fragment trusted on its own: "compose"
/// and "new message" also name editors, Outlook and Apple Mail.
const GMAIL_MARK: &str = "gmail";

/// Why a confirmed send pressed nothing, when the draft was never identified.
const NO_DRAFT_WINDOW: &str =
    "I couldn't confirm which window holds the draft, so nothing was sent. Press Send in Gmail yourself.";

pub static SPEC: ToolSpec = ToolSpec {
    name: "compose_email",
    description: "Write an email in Gmail: opens a filled-in draft in the browser for the user \
                  to review. The email is only sent after the user confirms. Use for any \
                  request to email, mail or message someone by email.",
    schema,
    risk: Risk::NeedsConfirmation,
    platforms: ALL_PLATFORMS,
    keywords: &["email", "mail", "gmail", "send", "write", "message"],
    handler: run,
    on_confirm: Some(send),
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "to": {
                "type": "string",
                "minLength": 1,
                "maxLength": 500,
                "description": "Who the email is for: a contact's name as the user said it \
                                (\"Ron\", \"Hermione Granger\") or an email address the user \
                                said, written alone. Several recipients are separated by \
                                commas."
            },
            "subject": {
                "type": "string",
                "maxLength": 300,
                "description": "A short subject line. Write one from the request if the user \
                                did not say it."
            },
            "body": {
                "type": "string",
                "maxLength": 20000,
                "description": "The full email text, ready to send, written in the user's \
                                voice. Use \\n for line breaks. No signature placeholder."
            },
            "cc": {
                "type": "array",
                "maxItems": 10,
                "items": { "type": "string", "minLength": 1, "maxLength": 320 },
                "description": "People to copy, as names or addresses. Only when asked."
            }
        },
        "required": ["to", "subject", "body"],
        "additionalProperties": false
    })
}

/// One recipient, resolved to an address.
#[derive(Debug, Clone, PartialEq)]
struct Recipient {
    name: String,
    email: String,
}

/// Open the draft and wait for "send".
fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(to_raw) = arg_str(&args, "to") else {
        return ToolOutcome::ask("Who should the email go to?", Vec::new());
    };
    let subject = arg_str(&args, "subject").unwrap_or("");
    let body = args
        .get("body")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");

    let mut to = Vec::new();
    for query in split_recipients(to_raw) {
        match resolve_recipient(ctx, &query) {
            Ok(recipient) => to.push(recipient),
            Err(question) => return question,
        }
    }
    if to.is_empty() {
        return ToolOutcome::ask("Who should the email go to?", Vec::new());
    }
    let mut cc = Vec::new();
    for query in arg_strings(&args, "cc")
        .iter()
        .flat_map(|item| split_recipients(item))
    {
        match resolve_recipient(ctx, &query) {
            Ok(recipient) => cc.push(recipient),
            Err(question) => return question,
        }
    }

    let account = ctx.settings.command_google_account.trim();
    let draft = match build_draft_url(&to, &cc, subject, body, account) {
        Ok(draft) => draft,
        Err(e) => return ToolOutcome::failed(e, false),
    };

    // What was in front before the link opened, so a Gmail tab that was
    // already there is not mistaken for the draft.
    let before = if ctx.dry_run {
        None
    } else {
        ctx.effects.foreground_window()
    };
    if let Err(e) = ctx.effects.open_url(draft.url.as_str()) {
        warn!("command mode: compose_email could not open the draft: {e}");
        return ToolOutcome::failed(e, false);
    }
    info!(
        "command mode: compose_email opened a draft ({} recipient(s), {} cc, body {} chars{})",
        to.len(),
        cc.len(),
        body.chars().count(),
        if draft.truncated { ", cut to fit" } else { "" }
    );
    // A dry run opened nothing, so there is no window to wait for.
    let window = if ctx.dry_run {
        None
    } else {
        wait_for_draft_window(ctx, before.as_ref())
    };
    let unidentified = !ctx.dry_run && window.is_none();
    if unidentified {
        warn!("command mode: compose_email saw no Gmail window come forward; the send will refuse");
    }

    let names = to
        .iter()
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let emails = to
        .iter()
        .map(|r| r.email.as_str())
        .collect::<Vec<_>>()
        .join(",");
    // The confirmation names every address: a familiar name alone could hide
    // an address the user never gave.
    let addressed = recipient_labels(&to);
    let mut summary = format!("Send email to {addressed}");
    if !cc.is_empty() {
        summary.push_str(&format!(", cc {}", recipient_labels(&cc)));
    }
    if !subject.is_empty() {
        summary.push_str(&format!(" · {subject}"));
    }
    if unidentified {
        summary.push_str(" (I couldn't identify the draft window, so I can't press Send for you)");
    }
    let feedback = if draft.truncated {
        Feedback::new(
            "emailDraftTruncated",
            "Draft to {{name}} is open. The message was too long to pass to Gmail, so it was \
             cut short; check the end before sending. Say send or cancel.",
            &[("name", &addressed)],
        )
    } else {
        Feedback::new(
            "emailDraft",
            "Draft to {{name}} is open. Say send or cancel.",
            &[("name", &addressed)],
        )
    };
    let window = window.map(|w| json!({"id": w.id, "app": w.app, "title": w.title}));
    ToolOutcome::NeedsConfirmation {
        pending: PendingAction {
            tool: "compose_email",
            summary,
            payload: json!({
                "subject": subject,
                "to_name": names,
                "to_email": emails,
                "window": window,
            }),
        },
        feedback,
    }
}

/// Watch the foreground until a Gmail window comes forward that was not
/// already there before the link opened (`before`: a different window, or the
/// same browser window now showing another tab), or until [`DRAFT_WAIT`] runs
/// out.
fn wait_for_draft_window(ctx: &ToolCtx, before: Option<&WindowEntry>) -> Option<WindowEntry> {
    let deadline = Instant::now() + DRAFT_WAIT;
    loop {
        if let Some(front) = ctx.effects.foreground_window() {
            let already_there =
                before.is_some_and(|b| b.id == front.id && b.title == front.title);
            if is_gmail_title(&front.title) && !already_there {
                return Some(front);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(DRAFT_POLL);
    }
}

fn is_gmail_title(title: &str) -> bool {
    title.to_lowercase().contains(GMAIL_MARK)
}

/// "Ron Weasley <ron@example.com>, luna@quibbler.example": each recipient
/// with the address the mail goes to.
fn recipient_labels(recipients: &[Recipient]) -> String {
    recipients
        .iter()
        .map(|r| {
            if r.name.trim().eq_ignore_ascii_case(r.email.trim()) {
                r.email.clone()
            } else {
                format!("{} <{}>", r.name, r.email)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The window [`run`] recorded, from the pending payload. `None` when it
/// recorded none (`"window": null`) or the payload is malformed.
fn stored_window(payload: &Value) -> Option<WindowEntry> {
    let window = payload.get("window")?;
    Some(WindowEntry {
        id: window.get("id")?.as_i64()?,
        app: window.get("app")?.as_str()?.to_string(),
        title: window.get("title")?.as_str()?.to_string(),
    })
}

/// Whether `front`, read just before the key press, is still the draft
/// [`run`] recorded: the same window showing the same title, or the same
/// window with a Gmail title that names the subject (Gmail retitles a draft
/// once it has one).
fn still_the_draft(front: &WindowEntry, draft: &WindowEntry, subject: &str) -> bool {
    if front.id != draft.id {
        return false;
    }
    if front.title == draft.title {
        return true;
    }
    let subject = subject.trim().to_lowercase();
    let title = front.title.to_lowercase();
    !subject.is_empty() && title.contains(GMAIL_MARK) && title.contains(&subject)
}

/// Send the draft [`run`] opened: focus its window, check it is still the one
/// in front, and press the send shortcut.
///
/// Phase 1 cannot verify that Gmail actually sent it: there is no API call
/// and nothing on screen is read back. What it does guarantee is that no key
/// is pressed unless the recorded draft window is the foreground at the
/// moment of the press, because Ctrl+Enter in any other app can submit
/// anything.
fn send(ctx: &ToolCtx, payload: Value) -> ToolOutcome {
    let subject = payload
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or("");
    let name = payload
        .get("to_name")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("them");
    let combo = KeyCombo {
        modifiers: vec![ctx.platform.primary_modifier()],
        key: "enter".to_string(),
    };

    // A dry run touches no real window: the press is only recorded, and
    // there is no draft to check because none was opened.
    if ctx.dry_run {
        return press_send(ctx, &combo, name);
    }

    let Some(draft) = stored_window(&payload) else {
        warn!("command mode: compose_email has no identified draft window; not sending");
        return ToolOutcome::failed(NO_DRAFT_WINDOW, false);
    };
    let Some(window) = ctx
        .effects
        .open_windows()
        .into_iter()
        .find(|w| w.id == draft.id)
    else {
        warn!("command mode: compose_email's draft window is gone; not sending");
        return ToolOutcome::failed(
            "The Gmail draft's window has closed, so nothing was sent.",
            false,
        );
    };

    if let Err(e) = ctx.effects.focus_window(&window) {
        warn!("command mode: compose_email could not focus the draft: {e}");
        return ToolOutcome::failed(
            format!("I couldn't bring the Gmail draft to the front, so nothing was sent. {e}"),
            false,
        );
    }
    std::thread::sleep(FOCUS_SETTLE);

    // The last read before the key: a click, a toast or a tab switch in the
    // moments since focusing must not receive Ctrl+Enter.
    match ctx.effects.foreground_window() {
        // `foreground_is_exactly` as well: a dialog the draft's browser owns
        // can be reported under the draft's id and title.
        Some(front)
            if still_the_draft(&front, &draft, subject)
                && ctx.effects.foreground_is_exactly(draft.id) => {}
        _ => {
            warn!(
                "command mode: compose_email's draft was not in front at send time; not sending"
            );
            return ToolOutcome::failed(
                "The Gmail draft wasn't in front when I went to send, so nothing was sent. \
                 Press Send in Gmail yourself.",
                false,
            );
        }
    }
    press_send(ctx, &combo, name)
}

/// Press the send shortcut in whatever has the keyboard; [`send`] has
/// already made sure that is the draft.
fn press_send(ctx: &ToolCtx, combo: &KeyCombo, name: &str) -> ToolOutcome {
    if let Err(e) = ctx.effects.press_keys(combo) {
        warn!(
            "command mode: compose_email could not press {}: {e}",
            combo.label()
        );
        return ToolOutcome::failed(format!("I couldn't press send in the Gmail draft. {e}"), false);
    }
    info!(
        "command mode: compose_email pressed {} in the draft",
        combo.label()
    );
    ToolOutcome::done("emailSent", "Sent to {{name}}.", &[("name", name)])
}

/// "Ron, hermione@example.com; Ginny" → three queries. A `Name <address>`
/// entry keeps its comma-free form.
fn split_recipients(raw: &str) -> Vec<String> {
    raw.split([',', ';'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// Turn one spoken recipient into an address, or the question to ask.
///
/// A "Name <address>" is written by the model, even when it repeats a choice
/// from an earlier question (the answer goes back through the router), so the
/// address in it is not taken on trust. It is used when it is one of a
/// contact's own addresses (named after that contact), or when it stands
/// alone ("<address>"). Otherwise the name decides, through the contact book.
/// The written address is used only where the book has none to prefer: nobody
/// by that name (the recipient is then named after the address, which is no
/// more than a bare address would get), or a contact with no address on file.
/// Either way the confirmation shows exactly where the mail goes.
fn resolve_recipient(ctx: &ToolCtx, query: &str) -> Result<Recipient, ToolOutcome> {
    if let Some((name, email)) = parse_named_address(query) {
        if let Some(contact) = contact_with_address(ctx, &email) {
            return Ok(Recipient {
                name: contact.name.clone(),
                email,
            });
        }
        if name == email {
            return resolve_query(ctx, &email);
        }
        return match ctx.contacts.resolve(&name) {
            ContactMatch::None => Ok(Recipient {
                name: email.clone(),
                email,
            }),
            // The answer to "I don't have an email address for Neville. What
            // is it?": the book has no address to prefer, and the summary
            // shows the one given.
            ContactMatch::Unique(contact) if contact.primary_email().is_none() => Ok(Recipient {
                name: contact.name.clone(),
                email,
            }),
            _ => resolve_query(ctx, &name),
        };
    }
    resolve_query(ctx, query)
}

/// The contact that has `email` among its addresses, compared without case.
fn contact_with_address<'a>(ctx: &'a ToolCtx, email: &str) -> Option<&'a Contact> {
    ctx.contacts.contacts().iter().find(|contact| {
        contact
            .emails
            .iter()
            .any(|e| e.trim().eq_ignore_ascii_case(email.trim()))
    })
}

/// A name or a bare address, through the contact book.
fn resolve_query(ctx: &ToolCtx, query: &str) -> Result<Recipient, ToolOutcome> {
    match ctx.contacts.resolve(query) {
        ContactMatch::Unique(contact) => match contact.primary_email() {
            Some(email) => Ok(Recipient {
                name: contact.name.clone(),
                email: email.to_string(),
            }),
            None => Err(ToolOutcome::ask(
                format!(
                    "I don't have an email address for {}. What is it?",
                    contact.name
                ),
                Vec::new(),
            )),
        },
        ContactMatch::Ambiguous(options) => Err(ToolOutcome::ask(
            format!("Which {query}?"),
            options.iter().map(choice_label).collect(),
        )),
        ContactMatch::None => {
            let question = match &ctx.contacts.load_error {
                Some(e) => {
                    warn!("command mode: contacts unavailable: {e}");
                    format!(
                        "Who is {query}? I couldn't read your contacts file, so say their \
                         email address."
                    )
                }
                None => format!("Who is {query}? Say their email address."),
            };
            Err(ToolOutcome::ask(question, Vec::new()))
        }
    }
}

/// "Name <address>" → (name, address), when the address is one.
fn parse_named_address(query: &str) -> Option<(String, String)> {
    let open = query.rfind('<')?;
    let close = query[open..].find('>')? + open;
    let email = query[open + 1..close].trim();
    if !looks_like_email(email) {
        return None;
    }
    let name = query[..open].trim().trim_matches('"').trim();
    let name = if name.is_empty() { email } else { name };
    Some((name.to_string(), email.to_string()))
}

/// How a contact is offered as a choice: "Ron Weasley <ron@example.com>".
fn choice_label(contact: &Contact) -> String {
    match contact.primary_email() {
        Some(email) => format!("{} <{email}>", contact.name),
        None => contact.name.clone(),
    }
}

struct Draft {
    url: Url,
    truncated: bool,
}

/// Gmail's compose URL for this email, with the body cut at a word if the
/// whole thing would pass [`MAX_URL_LEN`].
fn build_draft_url(
    to: &[Recipient],
    cc: &[Recipient],
    subject: &str,
    body: &str,
    account: &str,
) -> Result<Draft, String> {
    let without_body = compose_url(to, cc, subject, "", account)?;
    // `body=` is already in it, with an empty value: what is left is the
    // room for the encoded body.
    let budget = MAX_URL_LEN.saturating_sub(without_body.as_str().len());
    if encoded_len(body) <= budget {
        return Ok(Draft {
            url: compose_url(to, cc, subject, body, account)?,
            truncated: false,
        });
    }
    let cut = cut_to_fit(body, budget.saturating_sub(encoded_len(TRUNCATION_MARK)));
    let shortened = format!("{}{TRUNCATION_MARK}", cut.trim_end());
    Ok(Draft {
        url: compose_url(to, cc, subject, &shortened, account)?,
        truncated: true,
    })
}

fn compose_url(
    to: &[Recipient],
    cc: &[Recipient],
    subject: &str,
    body: &str,
    account: &str,
) -> Result<Url, String> {
    let mut url = Url::parse("https://mail.google.com/mail/")
        .map_err(|e| format!("Could not build the Gmail address: {e}"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("view", "cm").append_pair("fs", "1");
        if !account.is_empty() {
            query.append_pair("authuser", account);
        }
        query.append_pair("to", &join_emails(to));
        if !cc.is_empty() {
            query.append_pair("cc", &join_emails(cc));
        }
        query.append_pair("su", subject).append_pair("body", body);
    }
    Ok(url)
}

fn join_emails(recipients: &[Recipient]) -> String {
    recipients
        .iter()
        .map(|r| r.email.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// The length `text` takes once form-encoded the way `query_pairs_mut` does
/// it (`application/x-www-form-urlencoded`): ASCII letters, digits and
/// `*-._` stay, a space becomes `+`, every other byte becomes `%XX`.
fn encoded_len(text: &str) -> usize {
    text.bytes().map(encoded_byte_len).sum()
}

fn encoded_byte_len(byte: u8) -> usize {
    match byte {
        b'*' | b'-' | b'.' | b'_' | b' ' => 1,
        b if b.is_ascii_alphanumeric() => 1,
        _ => 3,
    }
}

/// The longest prefix of `text` that encodes into `budget` bytes, ending at a
/// word boundary when there is one; a single word longer than the budget is
/// cut mid-word rather than dropped.
fn cut_to_fit(text: &str, budget: usize) -> &str {
    let mut used = 0;
    let mut last_break = None;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() {
            last_break = Some(index);
        }
        let mut buf = [0u8; 4];
        let cost: usize = ch.encode_utf8(&mut buf).bytes().map(encoded_byte_len).sum();
        if used + cost > budget {
            return match last_break {
                Some(at) if at > 0 => &text[..at],
                _ => &text[..index],
            };
        }
        used += cost;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::effects::{AppEntry, Effects, MediaAction, VolumeAction};
    use crate::command_mode::registry::Platform;
    use chrono::{FixedOffset, TimeZone};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn contact(name: &str, aliases: &[&str], emails: &[&str]) -> Contact {
        Contact {
            name: name.into(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            emails: emails.iter().map(|e| e.to_string()).collect(),
            phones: Vec::new(),
        }
    }

    fn book() -> ContactBook {
        ContactBook::new(vec![
            contact("Ron Weasley", &["Ron"], &["ron@example.com"]),
            contact("Ginny Weasley", &[], &["ginny@example.com"]),
            contact("Hermione Granger", &[], &["hermione@example.com"]),
            contact("Neville Longbottom", &[], &[]),
        ])
    }

    fn ctx(fake: &Arc<FakeEffects>, contacts: ContactBook, platform: Platform) -> ToolCtx {
        ctx_over(fake.clone(), contacts, platform)
    }

    fn ctx_over(effects: Arc<dyn Effects>, contacts: ContactBook, platform: Platform) -> ToolCtx {
        let mut settings = crate::settings::get_default_settings();
        settings.command_google_account = String::new();
        settings.command_search_url = "https://www.google.com/search?q={query}".into();
        settings.command_dry_run = false;
        let now = FixedOffset::east_opt(5 * 3600 + 45 * 60)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 9, 15, 0)
            .unwrap();
        ToolCtx {
            settings,
            context: CommandContext::at(now, Some("Asia/Kathmandu".into()), platform, None, None),
            effects,
            contacts: Arc::new(contacts),
            platform,
            dry_run: false,
        }
    }

    fn opened_url(fake: &FakeEffects) -> String {
        let actions = fake.actions();
        assert_eq!(actions.len(), 1, "{actions:?}");
        actions[0]
            .strip_prefix("open_url ")
            .expect("an open_url action")
            .to_string()
    }

    fn pairs(url: &str) -> Vec<(String, String)> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn pair(url: &str, key: &str) -> Option<String> {
        pairs(url).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The draft carries exactly what the model wrote: spaces, line breaks,
    /// punctuation and non-ASCII all survive the round trip through the URL.
    #[test]
    fn the_draft_url_encodes_every_field_and_decodes_back_exactly() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let body = "Hi Ron,\n\nMeeting at 9:00 & bring 50% of the café notes?\nThanks";
        let outcome = run(
            &ctx,
            json!({"to": "Ron", "subject": "Meeting at 9:00", "body": body}),
        );
        assert!(matches!(outcome, ToolOutcome::NeedsConfirmation { .. }), "{outcome:?}");

        let url = opened_url(&fake);
        assert!(url.starts_with("https://mail.google.com/mail/?view=cm&fs=1&"), "{url}");
        assert!(url.contains("su=Meeting+at+9%3A00"), "{url}");
        assert!(url.contains("%0A%0A"), "newlines must be %0A: {url}");
        assert_eq!(
            pairs(&url),
            vec![
                ("view".to_string(), "cm".to_string()),
                ("fs".to_string(), "1".to_string()),
                ("to".to_string(), "ron@example.com".to_string()),
                ("su".to_string(), "Meeting at 9:00".to_string()),
                ("body".to_string(), body.to_string()),
            ]
        );
    }

    /// With a Google account set, the draft opens in that account rather than
    /// the browser's default one; without, no `authuser` is sent at all.
    #[test]
    fn the_google_account_setting_becomes_authuser() {
        let fake = Arc::new(FakeEffects::default());
        let mut ctx = ctx(&fake, book(), Platform::Windows);
        ctx.settings.command_google_account = "me@work.example".into();
        run(&ctx, json!({"to": "Ron", "subject": "Hi", "body": "Hello"}));
        assert_eq!(
            pair(&opened_url(&fake), "authuser").as_deref(),
            Some("me@work.example")
        );

        let fake = Arc::new(FakeEffects::default());
        let ctx = super::tests::ctx(&fake, book(), Platform::Windows);
        run(&ctx, json!({"to": "Ron", "subject": "Hi", "body": "Hello"}));
        assert_eq!(pair(&opened_url(&fake), "authuser"), None);
    }

    /// A body too long for one URL is cut at a word, marked, and the user is
    /// told; the rest of the draft is intact.
    #[test]
    fn a_very_long_body_is_cut_at_a_word_and_the_user_is_told() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let body = "lorem ipsum dolor sit amet, ".repeat(400);
        let outcome = run(&ctx, json!({"to": "Ron", "subject": "Long", "body": body}));
        let ToolOutcome::NeedsConfirmation { feedback, .. } = outcome else {
            panic!("expected a draft");
        };
        assert_eq!(feedback.key, "commandMode.feedback.emailDraftTruncated");

        let url = opened_url(&fake);
        assert!(url.len() <= MAX_URL_LEN, "url is {} bytes", url.len());
        assert!(url.len() > MAX_URL_LEN - 100, "cut far too much: {}", url.len());
        let sent = pair(&url, "body").unwrap();
        let kept = sent.strip_suffix(TRUNCATION_MARK).expect("marked as cut");
        assert!(body.starts_with(kept));
        let next = body[kept.len()..].chars().next();
        assert!(
            next.is_some_and(|c| c.is_whitespace()) || kept.ends_with(','),
            "not cut at a word: …{:?} / {next:?}",
            &kept[kept.len().saturating_sub(10)..]
        );
        assert_eq!(pair(&url, "su").as_deref(), Some("Long"));
    }

    /// The word-boundary cut counts encoded bytes, so multi-byte text is not
    /// split inside a character and still fits.
    #[test]
    fn the_cut_counts_encoded_bytes_and_respects_characters() {
        assert_eq!(cut_to_fit("hello world", 100), "hello world");
        assert_eq!(cut_to_fit("hello world again", 13), "hello world");
        // "é" is two bytes, six once encoded.
        assert_eq!(cut_to_fit("éééé", 13), "éé");
        assert_eq!(encoded_len("a b\n"), 1 + 1 + 1 + 3);
        assert_eq!(cut_to_fit("abcdefgh", 3), "abc");
    }

    #[test]
    fn a_contact_without_an_address_is_asked_for_it() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let outcome = run(&ctx, json!({"to": "Neville", "subject": "x", "body": "y"}));
        assert_eq!(
            outcome,
            ToolOutcome::ask(
                "I don't have an email address for Neville Longbottom. What is it?",
                vec![]
            )
        );
        assert!(fake.actions().is_empty());
    }

    /// Two people fit the name: the question lists them with their addresses,
    /// and nothing is opened.
    #[test]
    fn an_ambiguous_name_asks_which_one_with_addresses() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let ToolOutcome::AskUser { question, choices } =
            run(&ctx, json!({"to": "Weasley", "subject": "x", "body": "y"}))
        else {
            panic!("expected a question");
        };
        assert_eq!(question, "Which Weasley?");
        assert!(choices.contains(&"Ron Weasley <ron@example.com>".to_string()), "{choices:?}");
        assert!(choices.contains(&"Ginny Weasley <ginny@example.com>".to_string()), "{choices:?}");
        assert!(fake.actions().is_empty());
    }

    /// The answer to that question comes back as the choice text, which
    /// resolves to the address in it because that address is the contact's.
    #[test]
    fn a_chosen_name_with_its_own_address_is_used() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let outcome = run(
            &ctx,
            json!({"to": "Ginny Weasley <GINNY@example.com>", "subject": "x", "body": "y"}),
        );
        let ToolOutcome::NeedsConfirmation { pending, .. } = outcome else {
            panic!("expected a draft");
        };
        assert_eq!(pending.payload["to_name"], "Ginny Weasley");
        assert_eq!(pair(&opened_url(&fake), "to").as_deref(), Some("GINNY@example.com"));
    }

    fn resolved(ctx: &ToolCtx, query: &str) -> Result<Recipient, ToolOutcome> {
        resolve_recipient(ctx, query)
    }

    fn recipient(name: &str, email: &str) -> Recipient {
        Recipient {
            name: name.into(),
            email: email.into(),
        }
    }

    /// A model-written "Name <address>" cannot smuggle an address in behind a
    /// familiar name: an address that is not the contact's gives way to the
    /// name, and an unknown name is shown as the bare address.
    #[test]
    fn a_written_address_is_trusted_only_where_the_book_has_none_to_prefer() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        // A known name with a stranger's address: the contact's own address.
        assert_eq!(
            resolved(&ctx, "Ron <attacker@evil.example>"),
            Ok(recipient("Ron Weasley", "ron@example.com"))
        );
        // A known address under another name: named after its owner.
        assert_eq!(
            resolved(&ctx, "Ron <hermione@example.com>"),
            Ok(recipient("Hermione Granger", "hermione@example.com"))
        );
        // Nobody by that name: the address, named after itself.
        assert_eq!(
            resolved(&ctx, "Albus Dumbledore <albus@hogwarts.example>"),
            Ok(recipient("albus@hogwarts.example", "albus@hogwarts.example"))
        );
        // An address alone, bracketed or bare, is the user's own.
        assert_eq!(
            resolved(&ctx, "<luna@quibbler.example>"),
            Ok(recipient("luna@quibbler.example", "luna@quibbler.example"))
        );
        // A contact with no address on file takes the one given for them.
        assert_eq!(
            resolved(&ctx, "Neville <neville@hogwarts.example>"),
            Ok(recipient("Neville Longbottom", "neville@hogwarts.example"))
        );
        // A name that fits several people is still a question.
        assert!(matches!(
            resolved(&ctx, "Weasley <x@evil.example>"),
            Err(ToolOutcome::AskUser { .. })
        ));
    }

    /// Whoever the recipient is named after, the confirmation shows the
    /// address the draft goes to.
    #[test]
    fn the_summary_shows_an_unknown_address_bare() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let ToolOutcome::NeedsConfirmation { pending, .. } = run(
            &ctx,
            json!({"to": "Ron Weasley <x@evil.example>, Albus <albus@hogwarts.example>", "subject": "Hi", "body": "y"}),
        ) else {
            panic!("expected a draft");
        };
        assert!(
            pending
                .summary
                .starts_with("Send email to Ron Weasley <ron@example.com>, albus@hogwarts.example · Hi"),
            "{}",
            pending.summary
        );
    }

    #[test]
    fn an_unknown_name_asks_for_the_address_and_mentions_a_broken_contacts_file() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        assert_eq!(
            run(&ctx, json!({"to": "Dumbledore", "subject": "x", "body": "y"})),
            ToolOutcome::ask("Who is Dumbledore? Say their email address.", vec![])
        );

        let ctx = super::tests::ctx(&fake, ContactBook::parse("[{"), Platform::Windows);
        let ToolOutcome::AskUser { question, .. } =
            run(&ctx, json!({"to": "Dumbledore", "subject": "x", "body": "y"}))
        else {
            panic!("expected a question");
        };
        assert!(question.contains("couldn't read your contacts file"), "{question}");
        assert!(fake.actions().is_empty());
    }

    /// A spoken address needs no contact, even with no contacts file at all.
    #[test]
    fn a_spoken_address_and_cc_names_are_resolved() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let outcome = run(
            &ctx,
            json!({
                "to": "luna@quibbler.example",
                "subject": "Hi",
                "body": "Hello",
                "cc": ["Hermione", "Ron"]
            }),
        );
        assert!(matches!(outcome, ToolOutcome::NeedsConfirmation { .. }), "{outcome:?}");
        let url = opened_url(&fake);
        assert_eq!(pair(&url, "to").as_deref(), Some("luna@quibbler.example"));
        assert_eq!(
            pair(&url, "cc").as_deref(),
            Some("hermione@example.com,ron@example.com")
        );
    }

    /// A cc that cannot be resolved stops the draft with a question, like `to`.
    #[test]
    fn an_ambiguous_cc_asks_before_opening_anything() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book(), Platform::Windows);
        let outcome = run(
            &ctx,
            json!({"to": "Ron", "subject": "x", "body": "y", "cc": ["Weasley"]}),
        );
        assert!(matches!(outcome, ToolOutcome::AskUser { .. }), "{outcome:?}");
        assert!(fake.actions().is_empty());
    }

    /// What waits for confirmation says who and what, with every address, and
    /// carries the window the send must use.
    #[test]
    fn the_pending_send_names_the_recipient_its_address_and_the_subject() {
        let fake = Arc::new(FakeEffects::default());
        let draft = window(2, "Compose Mail - me@example.com - Gmail - Google Chrome", "chrome");
        let ctx = ctx_over(browser(&fake, Some(draft)), book(), Platform::Windows);
        let ToolOutcome::NeedsConfirmation { pending, feedback } = run(
            &ctx,
            json!({"to": "Ron", "subject": "Meeting at 9:00", "body": "See you", "cc": ["Hermione"]}),
        ) else {
            panic!("expected a draft");
        };
        assert_eq!(pending.tool, "compose_email");
        assert_eq!(
            pending.summary,
            "Send email to Ron Weasley <ron@example.com>, cc Hermione Granger \
             <hermione@example.com> · Meeting at 9:00"
        );
        assert_eq!(
            pending.payload,
            json!({
                "subject": "Meeting at 9:00",
                "to_name": "Ron Weasley",
                "to_email": "ron@example.com",
                "window": {
                    "id": 2,
                    "app": "chrome",
                    "title": "Compose Mail - me@example.com - Gmail - Google Chrome"
                }
            })
        );
        assert_eq!(feedback.key, "commandMode.feedback.emailDraft");
        assert_eq!(
            feedback.render_en(),
            "Draft to Ron Weasley <ron@example.com> is open. Say send or cancel."
        );
    }

    #[test]
    fn a_browser_that_will_not_open_fails_without_escalating() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("No browser".into()),
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::Windows);
        assert_eq!(
            run(&ctx, json!({"to": "Ron", "subject": "x", "body": "y"})),
            ToolOutcome::failed("No browser", false)
        );
    }

    fn window(id: i64, title: &str, app: &str) -> WindowEntry {
        WindowEntry {
            id,
            title: title.into(),
            app: app.into(),
        }
    }

    /// [`FakeEffects`] plus a browser: opening a URL brings `opens_to` to the
    /// front, the way a real browser shows the new tab. `run` needs the
    /// foreground to change after the link opens, which the plain fake never
    /// does.
    struct Browser {
        inner: Arc<FakeEffects>,
        opens_to: Option<WindowEntry>,
    }

    impl Effects for Browser {
        fn installed_apps(&self) -> Vec<AppEntry> {
            self.inner.installed_apps()
        }
        fn open_windows(&self) -> Vec<WindowEntry> {
            self.inner.open_windows()
        }
        fn recent_files(&self) -> Vec<PathBuf> {
            self.inner.recent_files()
        }
        fn known_folders(&self) -> Vec<PathBuf> {
            self.inner.known_folders()
        }
        fn foreground_window(&self) -> Option<WindowEntry> {
            self.inner.foreground_window()
        }
        fn open_url(&self, url: &str) -> Result<(), String> {
            self.inner.open_url(url)?;
            if let Some(window) = &self.opens_to {
                *self.inner.foreground.lock().unwrap() = Some(window.clone());
            }
            Ok(())
        }
        fn open_path(&self, path: &Path) -> Result<(), String> {
            self.inner.open_path(path)
        }
        fn launch_app(&self, app: &AppEntry) -> Result<(), String> {
            self.inner.launch_app(app)
        }
        fn focus_window(&self, window: &WindowEntry) -> Result<(), String> {
            self.inner.focus_window(window)
        }
        fn type_text(&self, text: &str) -> Result<(), String> {
            self.inner.type_text(text)
        }
        fn press_keys(&self, combo: &KeyCombo) -> Result<(), String> {
            self.inner.press_keys(combo)
        }
        fn media(&self, action: MediaAction) -> Result<(), String> {
            self.inner.media(action)
        }
        fn volume(&self, action: VolumeAction) -> Result<(), String> {
            self.inner.volume(action)
        }
    }

    fn browser(fake: &Arc<FakeEffects>, opens_to: Option<WindowEntry>) -> Arc<dyn Effects> {
        Arc::new(Browser {
            inner: fake.clone(),
            opens_to,
        })
    }

    const DRAFT_TITLE: &str = "Compose Mail - me@example.com - Gmail - Google Chrome";

    fn payload_with(window: Value) -> Value {
        json!({
            "subject": "Meeting at 9:00",
            "to_name": "Ron Weasley",
            "to_email": "ron@example.com",
            "window": window,
        })
    }

    fn payload() -> Value {
        payload_with(json!({"id": 2, "app": "chrome", "title": DRAFT_TITLE}))
    }

    fn ctrl_enter() -> String {
        let combo = KeyCombo {
            modifiers: vec![crate::command_mode::registry::Modifier::Ctrl],
            key: "enter".into(),
        };
        format!("press_keys {}", combo.label())
    }

    /// The whole path: the draft comes forward after the link opens, is
    /// recorded, and confirming focuses exactly that window, finds it still
    /// in front and presses Ctrl+Enter there.
    #[test]
    fn the_draft_window_is_recorded_and_the_send_goes_to_it() {
        let draft = window(2, DRAFT_TITLE, "chrome");
        let fake = Arc::new(FakeEffects {
            windows: vec![window(1, "Spotify Premium", "Spotify"), draft.clone()],
            foreground: std::sync::Mutex::new(Some(window(1, "Spotify Premium", "Spotify"))),
            focus_follows: true,
            ..Default::default()
        });
        let ctx = ctx_over(browser(&fake, Some(draft)), book(), Platform::Windows);
        let ToolOutcome::NeedsConfirmation { pending, .. } = run(
            &ctx,
            json!({"to": "Ron", "subject": "Meeting at 9:00", "body": "See you"}),
        ) else {
            panic!("expected a draft");
        };
        assert_eq!(pending.payload["window"]["id"], 2);
        // Something else in front by the time the user says "send".
        *fake.foreground.lock().unwrap() = Some(window(1, "Spotify Premium", "Spotify"));

        let outcome = send(&ctx, pending.payload);
        assert_eq!(
            outcome,
            ToolOutcome::done("emailSent", "Sent to {{name}}.", &[("name", "Ron Weasley")])
        );
        let actions = fake.actions();
        assert_eq!(actions.len(), 3, "{actions:?}");
        assert!(actions[0].starts_with("open_url "), "{actions:?}");
        assert_eq!(actions[1..], [String::from("focus_window 2"), ctrl_enter()]);
    }

    /// A dialog the browser owns ("Leave site?") has the keyboard, reported
    /// under the draft's own id and title: Ctrl+Enter must not go to it.
    #[test]
    fn a_dialog_over_the_draft_gets_no_key() {
        let draft = window(2, DRAFT_TITLE, "chrome");
        let fake = Arc::new(FakeEffects {
            windows: vec![draft.clone()],
            foreground: std::sync::Mutex::new(Some(draft)),
            focus_follows: true,
            popup_in_front: true,
            ..Default::default()
        });
        let ctx = ctx_over(fake.clone(), book(), Platform::Windows);
        let outcome = send(&ctx, payload());
        assert!(matches!(outcome, ToolOutcome::Failed { escalate: false, .. }), "{outcome:?}");
        assert!(
            !fake.actions().iter().any(|a| a.starts_with("press_keys")),
            "{:?}",
            fake.actions()
        );
    }

    /// The user (or a toast, or another app) takes the foreground between the
    /// focus and the key: nothing is pressed.
    #[test]
    fn a_window_that_takes_the_foreground_after_the_focus_gets_no_key() {
        let fake = Arc::new(FakeEffects {
            windows: vec![window(2, DRAFT_TITLE, "chrome")],
            // focus_follows is off: the focus "succeeds" but Teams stays in
            // front, as when focus stealing prevention refuses the switch.
            foreground: std::sync::Mutex::new(Some(window(
                5,
                "Meeting at 9:00 | Microsoft Teams",
                "Teams",
            ))),
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::Windows);
        let ToolOutcome::Failed { reason, escalate } = send(&ctx, payload()) else {
            panic!("expected a refusal");
        };
        assert!(reason.contains("nothing was sent"), "{reason}");
        assert!(!escalate);
        assert_eq!(fake.actions(), vec!["focus_window 2".to_string()]);
    }

    /// Same browser window, but another tab in front (a Gmail thread with a
    /// reply open): Ctrl+Enter there would send that reply, so no key.
    #[test]
    fn the_same_window_showing_another_gmail_tab_gets_no_key() {
        let fake = Arc::new(FakeEffects {
            windows: vec![window(2, DRAFT_TITLE, "chrome")],
            foreground: std::sync::Mutex::new(Some(window(
                2,
                "Re: budget - me@example.com - Gmail - Google Chrome",
                "chrome",
            ))),
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::Windows);
        assert!(matches!(send(&ctx, payload()), ToolOutcome::Failed { escalate: false, .. }));
        assert!(
            !fake.actions().contains(&ctrl_enter()),
            "{:?}",
            fake.actions()
        );
    }

    /// Gmail retitles a draft after its subject; that is still the draft.
    #[test]
    fn the_draft_retitled_with_its_subject_is_still_the_draft() {
        let retitled = window(2, "Meeting at 9:00 - me@example.com - Gmail", "chrome");
        assert!(still_the_draft(&retitled, &window(2, DRAFT_TITLE, "chrome"), "Meeting at 9:00"));
        // Without a subject there is nothing to recognise it by.
        assert!(!still_the_draft(&retitled, &window(2, DRAFT_TITLE, "chrome"), ""));
        // A subject in a non-Gmail title, or in another window, is not enough.
        let teams = window(2, "Meeting at 9:00 | Microsoft Teams", "Teams");
        assert!(!still_the_draft(&teams, &window(2, DRAFT_TITLE, "chrome"), "Meeting at 9:00"));
        let other = window(3, DRAFT_TITLE, "chrome");
        assert!(!still_the_draft(&other, &window(2, DRAFT_TITLE, "chrome"), "Meeting at 9:00"));
    }

    /// The draft's window was closed before the confirmation: nothing is
    /// focused and nothing is pressed, whatever else is open.
    #[test]
    fn a_draft_window_that_is_gone_gets_nothing() {
        let fake = Arc::new(FakeEffects {
            windows: vec![
                window(4, "Inbox (3) - Gmail - Google Chrome", "chrome"),
                window(6, "docker-compose.yml - Visual Studio Code", "Code"),
            ],
            foreground: std::sync::Mutex::new(Some(window(
                4,
                "Inbox (3) - Gmail - Google Chrome",
                "chrome",
            ))),
            focus_follows: true,
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::Windows);
        let ToolOutcome::Failed { reason, .. } = send(&ctx, payload()) else {
            panic!("expected a refusal");
        };
        assert!(reason.contains("nothing was sent"), "{reason}");
        assert!(fake.actions().is_empty(), "{:?}", fake.actions());
    }

    /// No Gmail window came forward after the link opened (the browser was
    /// slow, or a Gmail inbox that was already in front stayed there): the
    /// draft is still offered, the summary says the window is unknown, and
    /// the send refuses without touching anything.
    #[test]
    fn a_draft_whose_window_never_appeared_cannot_be_sent() {
        let inbox = window(4, "Inbox (3) - Gmail - Google Chrome", "chrome");
        let fake = Arc::new(FakeEffects {
            windows: vec![inbox.clone()],
            foreground: std::sync::Mutex::new(Some(inbox)),
            focus_follows: true,
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::Windows);
        let ToolOutcome::NeedsConfirmation { pending, .. } = run(
            &ctx,
            json!({"to": "Ron", "subject": "Meeting at 9:00", "body": "See you"}),
        ) else {
            panic!("expected a draft");
        };
        assert_eq!(pending.payload["window"], Value::Null);
        assert!(
            pending.summary.contains("couldn't identify the draft window"),
            "{}",
            pending.summary
        );

        assert_eq!(
            send(&ctx, pending.payload),
            ToolOutcome::failed(NO_DRAFT_WINDOW, false)
        );
        let actions = fake.actions();
        assert_eq!(actions.len(), 1, "only the draft opened: {actions:?}");
        assert!(actions[0].starts_with("open_url "));
    }

    /// A dry run waits for no window and checks none: the send's key press
    /// is only recorded by the effects.
    #[test]
    fn a_dry_run_records_the_send_without_looking_for_a_window() {
        let fake = Arc::new(FakeEffects::default());
        let mut ctx = ctx(&fake, book(), Platform::Windows);
        ctx.dry_run = true;
        let ToolOutcome::NeedsConfirmation { pending, .. } = run(
            &ctx,
            json!({"to": "Ron", "subject": "Meeting at 9:00", "body": "See you"}),
        ) else {
            panic!("expected a draft");
        };
        assert_eq!(pending.payload["window"], Value::Null);
        assert!(!pending.summary.contains("couldn't identify"), "{}", pending.summary);

        assert!(matches!(send(&ctx, pending.payload), ToolOutcome::Done { .. }));
        let actions = fake.actions();
        assert_eq!(actions.len(), 2, "{actions:?}");
        assert_eq!(actions[1], ctrl_enter());
    }

    /// On a Mac the send shortcut is Cmd+Enter. The window id is the app's
    /// pid there, and the foreground's title is the real window title read
    /// through Accessibility, while the window list only names the app.
    #[test]
    fn on_a_mac_the_send_uses_cmd_enter_in_the_recorded_window() {
        let fake = Arc::new(FakeEffects {
            windows: vec![window(7, "Notes", "Notes"), window(9, "Safari", "Safari")],
            foreground: std::sync::Mutex::new(Some(window(9, DRAFT_TITLE, "Safari"))),
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::MacOs);
        let outcome = send(
            &ctx,
            payload_with(json!({"id": 9, "app": "Safari", "title": DRAFT_TITLE})),
        );
        assert!(matches!(outcome, ToolOutcome::Done { .. }), "{outcome:?}");
        let cmd_enter = KeyCombo {
            modifiers: vec![crate::command_mode::registry::Modifier::Meta],
            key: "enter".into(),
        };
        assert_eq!(
            fake.actions(),
            vec![
                "focus_window 9".to_string(),
                format!("press_keys {}", cmd_enter.label())
            ]
        );
    }

    /// If the window cannot be focused, the key press would land elsewhere,
    /// so it is not made.
    #[test]
    fn a_draft_that_cannot_be_focused_is_not_sent() {
        let fake = Arc::new(FakeEffects {
            windows: vec![window(2, DRAFT_TITLE, "chrome")],
            foreground: std::sync::Mutex::new(Some(window(2, DRAFT_TITLE, "chrome"))),
            fail_with: Some("Access denied".into()),
            ..Default::default()
        });
        let ctx = ctx(&fake, book(), Platform::Windows);
        let ToolOutcome::Failed { reason, escalate } = send(&ctx, payload()) else {
            panic!("expected a failure");
        };
        assert!(reason.contains("nothing was sent"), "{reason}");
        assert!(!escalate);
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn named_addresses_and_recipient_lists_parse() {
        assert_eq!(
            parse_named_address("\"Ron Weasley\" <ron@example.com>"),
            Some(("Ron Weasley".into(), "ron@example.com".into()))
        );
        assert_eq!(
            parse_named_address("<ron@example.com>"),
            Some(("ron@example.com".into(), "ron@example.com".into()))
        );
        assert_eq!(parse_named_address("Ron <not an address>"), None);
        assert_eq!(parse_named_address("Ron"), None);
        assert_eq!(
            split_recipients("Ron, a@b.co; ,Ginny"),
            vec!["Ron".to_string(), "a@b.co".to_string(), "Ginny".to_string()]
        );
    }

    #[test]
    fn the_schema_requires_the_recipient_subject_and_body() {
        let schema = schema();
        assert!(crate::command_mode::schema::check_schema(&schema).is_ok());
        assert!(crate::command_mode::schema::validate(
            &schema,
            &json!({"to": "Ron", "subject": "", "body": ""})
        )
        .is_ok());
        assert!(crate::command_mode::schema::validate(&schema, &json!({"to": "Ron"})).is_err());
        assert!(crate::command_mode::schema::validate(
            &schema,
            &json!({"to": "Ron", "subject": "", "body": "", "bcc": ["x"]})
        )
        .is_err());
    }
}
