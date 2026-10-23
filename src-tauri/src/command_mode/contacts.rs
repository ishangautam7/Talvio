//! The user's contacts, from a local JSON file, matched by name.
//!
//! `<app data>/command_contacts.json` is a plain list the user edits:
//!
//! ```json
//! [{ "name": "Ron Weasley", "aliases": ["Ron"], "emails": ["ron@example.com"] }]
//! ```
//!
//! Matching is deliberately conservative. Sending an email to the wrong person
//! is worse than asking, so one clear winner is used and anything else (two
//! close candidates, or nothing close) becomes a question.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use strsim::jaro_winkler;

pub const CONTACTS_FILE: &str = "command_contacts.json";

/// Below this, a name is not a match at all.
const MIN_SCORE: f64 = 0.86;
/// A winner must lead the runner-up by this much, or the user is asked.
const CLEAR_LEAD: f64 = 0.04;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Contact {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(default)]
    pub phones: Vec<String>,
}

impl Contact {
    pub fn primary_email(&self) -> Option<&str> {
        self.emails
            .iter()
            .map(|e| e.trim())
            .find(|e| looks_like_email(e))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContactMatch {
    Unique(Contact),
    /// Several contacts are about as close; best first.
    Ambiguous(Vec<Contact>),
    None,
}

#[derive(Debug, Clone, Default)]
pub struct ContactBook {
    contacts: Vec<Contact>,
    /// Why the file could not be read, when it exists but is broken. Shown to
    /// the user instead of silently matching nobody.
    pub load_error: Option<String>,
}

impl ContactBook {
    pub fn new(contacts: Vec<Contact>) -> Self {
        Self {
            contacts,
            load_error: None,
        }
    }

    pub fn path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
        crate::portable::app_data_dir(app)
            .map(|dir| dir.join(CONTACTS_FILE))
            .map_err(|e| format!("Could not find the app data folder: {e}"))
    }

    /// Load from `path`. A missing file is an empty book; a broken one is an
    /// empty book that says why.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => Self {
                contacts: Vec::new(),
                load_error: Some(format!("Could not read {}: {e}", path.display())),
            },
        }
    }

    pub fn parse(text: &str) -> Self {
        // A UTF-8 BOM is what Notepad writes; it is not damage.
        let text = text.trim_start_matches('\u{feff}');
        if text.trim().is_empty() {
            return Self::default();
        }
        match serde_json::from_str::<Vec<Contact>>(text) {
            Ok(contacts) => Self::new(contacts),
            Err(e) => Self {
                contacts: Vec::new(),
                load_error: Some(format!("The contacts file is not valid JSON: {e}")),
            },
        }
    }

    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// Find who `query` means. See the module docs for the rule.
    pub fn resolve(&self, query: &str) -> ContactMatch {
        let query = normalize(query);
        if query.is_empty() {
            return ContactMatch::None;
        }

        // An address spoken outright is its own contact.
        if looks_like_email(&query) {
            return ContactMatch::Unique(Contact {
                name: query.clone(),
                aliases: Vec::new(),
                emails: vec![query],
                phones: Vec::new(),
            });
        }

        let mut scored: Vec<(f64, &Contact)> = self
            .contacts
            .iter()
            .map(|contact| (score(contact, &query), contact))
            .filter(|(score, _)| *score >= MIN_SCORE)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));

        match scored.as_slice() {
            [] => ContactMatch::None,
            [(_, only)] => ContactMatch::Unique((*only).clone()),
            [(best, winner), (second, _), ..] if best - second >= CLEAR_LEAD => {
                ContactMatch::Unique((*winner).clone())
            }
            close => ContactMatch::Ambiguous(
                close
                    .iter()
                    .take_while(|(s, _)| close[0].0 - s < CLEAR_LEAD)
                    .take(5)
                    .map(|(_, c)| (*c).clone())
                    .collect(),
            ),
        }
    }
}

fn normalize(text: &str) -> String {
    text.trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() && c != '@' && c != '.')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn looks_like_email(text: &str) -> bool {
    let text = text.trim();
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !text.contains(char::is_whitespace)
}

/// How well `query` names `contact`: the best of the full name, each word of
/// it (people say "Ron" and "Weasley" alike), each alias and each address's
/// local part. An exact match on any of them scores 1.0 outright.
fn score(contact: &Contact, query: &str) -> f64 {
    let full = normalize(&contact.name);
    let mut candidates = vec![full.clone()];
    candidates.extend(full.split(' ').map(str::to_string));
    candidates.extend(contact.aliases.iter().map(|a| normalize(a)));
    candidates.extend(
        contact
            .emails
            .iter()
            .filter_map(|e| e.split('@').next())
            .map(normalize),
    );
    candidates
        .iter()
        .filter(|c| !c.is_empty())
        .map(|candidate| {
            if candidate == query {
                1.0
            } else {
                jaro_winkler(candidate, query)
            }
        })
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, aliases: &[&str], email: &str) -> Contact {
        Contact {
            name: name.into(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            emails: vec![email.into()],
            phones: Vec::new(),
        }
    }

    fn book() -> ContactBook {
        ContactBook::new(vec![
            contact("Ron Weasley", &["Ron"], "ron@example.com"),
            contact("Hermione Granger", &[], "hermione@example.com"),
            contact("Ronald Reagan", &[], "rr@example.com"),
            contact("Ginny Weasley", &[], "ginny@example.com"),
        ])
    }

    #[test]
    fn an_alias_spoken_exactly_wins_even_with_similar_names_around() {
        let ContactMatch::Unique(found) = book().resolve("Ron") else {
            panic!("expected a unique match");
        };
        assert_eq!(found.name, "Ron Weasley");
        assert_eq!(found.primary_email(), Some("ron@example.com"));
    }

    #[test]
    fn a_first_name_and_a_slight_mishearing_still_match() {
        assert!(
            matches!(book().resolve("hermione"), ContactMatch::Unique(c) if c.name == "Hermione Granger")
        );
        assert!(
            matches!(book().resolve("Hermoine"), ContactMatch::Unique(c) if c.name == "Hermione Granger")
        );
    }

    /// Two people the name fits equally well: ask, never guess.
    #[test]
    fn a_shared_surname_is_ambiguous() {
        let ContactMatch::Ambiguous(options) = book().resolve("Weasley") else {
            panic!("expected ambiguity");
        };
        let names: Vec<&str> = options.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Ron Weasley") && names.contains(&"Ginny Weasley"));
    }

    #[test]
    fn nobody_close_is_no_match() {
        assert_eq!(book().resolve("Dumbledore"), ContactMatch::None);
        assert_eq!(book().resolve("   "), ContactMatch::None);
    }

    #[test]
    fn a_spoken_address_is_used_as_is() {
        let ContactMatch::Unique(found) = book().resolve("neville@hogwarts.edu") else {
            panic!("expected the address itself");
        };
        assert_eq!(found.primary_email(), Some("neville@hogwarts.edu"));
    }

    #[test]
    fn a_broken_file_says_why_and_a_bom_is_fine() {
        let broken = ContactBook::parse("[{\"name\": ");
        assert!(broken.contacts().is_empty());
        assert!(broken.load_error.is_some());

        let with_bom = ContactBook::parse("\u{feff}[{\"name\":\"Ron\",\"emails\":[\"r@x.io\"]}]");
        assert_eq!(with_bom.contacts().len(), 1);
        assert!(with_bom.load_error.is_none());
    }

    #[test]
    fn email_shapes() {
        assert!(looks_like_email("a@b.co"));
        assert!(!looks_like_email("a@b"));
        assert!(!looks_like_email("@b.co"));
        assert!(!looks_like_email("a b@c.co"));
    }
}
