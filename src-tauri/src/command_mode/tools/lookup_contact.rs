//! `lookup_contact`: say who a name in the contact book is, and their
//! address.
//!
//! Read-only. It answers "what's Ron's email?" and lets the model check a
//! name before it builds something on it. Matching is the contact book's own
//! conservative rule, so a name that fits two people is a question here too.

use log::warn;
use serde_json::{json, Value};

use crate::command_mode::contacts::{Contact, ContactMatch};
use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};

pub static SPEC: ToolSpec = ToolSpec {
    name: "lookup_contact",
    description: "Look up a person in the user's contacts and tell the user their email \
                  address and phone number. Use when the user asks for someone's details. Not \
                  needed before compose_email, which looks contacts up itself.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &["contact", "address", "email", "phone", "number", "who is"],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "The person's name as the user said it, or an email address."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(name) = arg_str(&args, "name") else {
        return ToolOutcome::ask("Whose details should I look up?", Vec::new());
    };
    match ctx.contacts.resolve(name) {
        // A spoken address resolves to itself; show the contact who has it,
        // or say there is none rather than reading the address back.
        ContactMatch::Unique(found) if is_bare_address(&found) => {
            match owner_of_address(ctx.contacts.contacts(), &found) {
                Some(owner) => describe(&owner),
                None => ToolOutcome::done(
                    "contactNotFound",
                    "I don't have a contact called {{name}}.",
                    &[("name", &found.name)],
                ),
            }
        }
        ContactMatch::Unique(found) => describe(&found),
        ContactMatch::Ambiguous(options) => ToolOutcome::ask(
            format!("Which {name}?"),
            options.iter().map(choice_label).collect(),
        ),
        ContactMatch::None => match &ctx.contacts.load_error {
            Some(e) => {
                warn!("command mode: contacts unavailable: {e}");
                ToolOutcome::failed(
                    format!("I couldn't read your contacts file, so I can't look up {name}."),
                    false,
                )
            }
            None => ToolOutcome::done(
                "contactNotFound",
                "I don't have a contact called {{name}}.",
                &[("name", name)],
            ),
        },
    }
}

/// Whether `found` is the stand-in the contact book makes for a spoken
/// address (named after the address itself), not a saved contact.
fn is_bare_address(found: &Contact) -> bool {
    found
        .primary_email()
        .is_some_and(|address| found.name.eq_ignore_ascii_case(address))
}

/// The saved contact who has `found`'s address.
fn owner_of_address(contacts: &[Contact], found: &Contact) -> Option<Contact> {
    let address = found.primary_email()?;
    contacts
        .iter()
        .find(|c| c.emails.iter().any(|e| e.trim().eq_ignore_ascii_case(address)))
        .cloned()
}

fn describe(contact: &Contact) -> ToolOutcome {
    let emails: Vec<&str> = contact
        .emails
        .iter()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .collect();
    let phones: Vec<&str> = contact
        .phones
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let details = emails
        .iter()
        .chain(phones.iter())
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if details.is_empty() {
        ToolOutcome::done(
            "contactNoDetails",
            "{{name}} is in your contacts, but has no email address or phone number saved.",
            &[("name", &contact.name)],
        )
    } else {
        ToolOutcome::done(
            "contactFound",
            "{{name}}: {{details}}.",
            &[("name", &contact.name), ("details", &details)],
        )
    }
}

/// How a contact is offered as a choice: "Ron Weasley <ron@example.com>".
fn choice_label(contact: &Contact) -> String {
    match contact.primary_email() {
        Some(email) => format!("{} <{email}>", contact.name),
        None => contact.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::registry::Platform;
    use chrono::{FixedOffset, TimeZone};
    use std::sync::Arc;

    fn contact(name: &str, emails: &[&str], phones: &[&str]) -> Contact {
        Contact {
            name: name.into(),
            aliases: Vec::new(),
            emails: emails.iter().map(|e| e.to_string()).collect(),
            phones: phones.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn book() -> ContactBook {
        ContactBook::new(vec![
            contact("Ron Weasley", &["ron@example.com", "ron@work.example"], &["+977 980000000"]),
            contact("Ginny Weasley", &["ginny@example.com"], &[]),
            contact("Neville Longbottom", &[], &[]),
        ])
    }

    fn ctx(fake: &Arc<FakeEffects>, contacts: ContactBook) -> ToolCtx {
        let mut settings = crate::settings::get_default_settings();
        settings.command_dry_run = false;
        let now = FixedOffset::east_opt(5 * 3600 + 45 * 60)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 9, 15, 0)
            .unwrap();
        ToolCtx {
            settings,
            context: CommandContext::at(now, None, Platform::MacOs, None, None),
            effects: fake.clone(),
            contacts: Arc::new(contacts),
            platform: Platform::MacOs,
            dry_run: false,
        }
    }

    /// A lookup lists every address and number, and touches nothing.
    #[test]
    fn a_unique_match_lists_its_addresses_and_numbers() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(&ctx(&fake, book()), json!({"name": "Ron"}));
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done");
        };
        assert_eq!(feedback.key, "commandMode.feedback.contactFound");
        assert_eq!(
            feedback.render_en(),
            "Ron Weasley: ron@example.com, ron@work.example, +977 980000000."
        );
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn a_contact_with_nothing_saved_says_so() {
        let fake = Arc::new(FakeEffects::default());
        let ToolOutcome::Done { feedback } = run(&ctx(&fake, book()), json!({"name": "Neville"}))
        else {
            panic!("expected done");
        };
        assert_eq!(feedback.key, "commandMode.feedback.contactNoDetails");
    }

    #[test]
    fn an_ambiguous_name_asks_which_one() {
        let fake = Arc::new(FakeEffects::default());
        let ToolOutcome::AskUser { question, choices } =
            run(&ctx(&fake, book()), json!({"name": "Weasley"}))
        else {
            panic!("expected a question");
        };
        assert_eq!(question, "Which Weasley?");
        assert!(choices.contains(&"Ron Weasley <ron@example.com>".to_string()), "{choices:?}");
        assert!(choices.contains(&"Ginny Weasley <ginny@example.com>".to_string()), "{choices:?}");
    }

    #[test]
    fn nobody_by_that_name_is_said_plainly() {
        let fake = Arc::new(FakeEffects::default());
        assert_eq!(
            run(&ctx(&fake, book()), json!({"name": "Dumbledore"})),
            ToolOutcome::done(
                "contactNotFound",
                "I don't have a contact called {{name}}.",
                &[("name", "Dumbledore")]
            )
        );
    }

    /// A broken contacts file is not "nobody by that name": the user is told
    /// the file is the problem.
    #[test]
    fn a_broken_contacts_file_is_reported_not_hidden() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(&ctx(&fake, ContactBook::parse("[{")), json!({"name": "Ron"}));
        let ToolOutcome::Failed { reason, escalate } = outcome else {
            panic!("expected a failure");
        };
        assert!(reason.contains("contacts file"), "{reason}");
        assert!(!escalate);
    }

    /// Looking up an address finds the person it belongs to, and an address
    /// nobody has is not read back as if it were a contact.
    #[test]
    fn an_address_finds_its_owner() {
        let fake = Arc::new(FakeEffects::default());
        let ToolOutcome::Done { feedback } =
            run(&ctx(&fake, book()), json!({"name": "Ginny@Example.com"}))
        else {
            panic!("expected done");
        };
        assert_eq!(feedback.render_en(), "Ginny Weasley: ginny@example.com.");

        let ToolOutcome::Done { feedback } =
            run(&ctx(&fake, book()), json!({"name": "luna@quibbler.example"}))
        else {
            panic!("expected done");
        };
        assert_eq!(
            feedback.render_en(),
            "I don't have a contact called luna@quibbler.example."
        );
    }
}
