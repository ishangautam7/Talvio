//! Picking the thing a spoken name means: an app, a window, a file.
//!
//! Same rule as contacts: one clear winner is used, several close candidates
//! become a question, nothing close is "not found". Speech-to-text mangles
//! names ("Spotify" → "spot if i", "VS Code" → "vs code"), so names are
//! compared with spaces and punctuation stripped as well as whole.

use strsim::jaro_winkler;

/// The outcome of [`pick`].
#[derive(Debug, Clone, PartialEq)]
pub enum Pick<T> {
    Unique(T),
    /// Close candidates, best first, at most five.
    Ambiguous(Vec<T>),
    None,
}

/// Lowercase, keep letters and digits, collapse everything else to one space.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// How well `query` names `candidate`, 0.0–1.0.
///
/// - identical after normalizing, or identical with spaces removed: 1.0
/// - the candidate's words start with the query's ("code" in "visual studio
///   code", "chrome" in "google chrome"): 0.95
/// - otherwise Jaro-Winkler on the space-free forms.
pub fn score(query: &str, candidate: &str) -> f64 {
    let q = normalize(query);
    let c = normalize(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    let q_tight: String = q.split(' ').collect();
    let c_tight: String = c.split(' ').collect();
    if q == c || q_tight == c_tight {
        return 1.0;
    }
    let c_words: Vec<&str> = c.split(' ').collect();
    let q_words: Vec<&str> = q.split(' ').collect();
    // Every query word is a whole word of the candidate, in order.
    if contains_words_in_order(&c_words, &q_words) {
        return 0.95;
    }
    jaro_winkler(&q_tight, &c_tight)
}

fn contains_words_in_order(haystack: &[&str], needle: &[&str]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    let mut position = 0;
    for word in needle {
        match haystack[position..].iter().position(|w| w == word) {
            Some(found) => position += found + 1,
            None => return false,
        }
    }
    true
}

/// Choose among `items` by name.
///
/// `min_score` is the floor for a match at all; `lead` is how far ahead the
/// winner must be. Each item can be known by several names (`names` returns
/// them); its score is the best of them.
pub fn pick<T: Clone>(
    query: &str,
    items: &[T],
    names: impl Fn(&T) -> Vec<String>,
    min_score: f64,
    lead: f64,
) -> Pick<T> {
    let mut scored: Vec<(f64, &T)> = items
        .iter()
        .map(|item| {
            let best = names(item)
                .iter()
                .map(|name| score(query, name))
                .fold(0.0, f64::max);
            (best, item)
        })
        .filter(|(s, _)| *s >= min_score)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));

    match scored.as_slice() {
        [] => Pick::None,
        [(_, only)] => Pick::Unique((*only).clone()),
        [(best, winner), (second, _), ..] if best - second >= lead => {
            Pick::Unique((*winner).clone())
        }
        close => {
            let top = close[0].0;
            Pick::Ambiguous(
                close
                    .iter()
                    .take_while(|(s, _)| top - s < lead)
                    .take(5)
                    .map(|(_, item)| (*item).clone())
                    .collect(),
            )
        }
    }
}

/// The defaults the app, window and file tools use.
pub const DEFAULT_MIN_SCORE: f64 = 0.84;
pub const DEFAULT_LEAD: f64 = 0.03;

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn pick_name(query: &str, items: &[&str]) -> Pick<String> {
        pick(
            query,
            &names(items),
            |s| vec![s.clone()],
            DEFAULT_MIN_SCORE,
            DEFAULT_LEAD,
        )
    }

    #[test]
    fn normalizing_drops_case_and_punctuation() {
        assert_eq!(normalize("  Visual-Studio  CODE! "), "visual studio code");
        assert_eq!(normalize("Ünïcode Ñame"), "ünïcode ñame");
    }

    #[test]
    fn speech_spacing_does_not_matter() {
        assert_eq!(score("spot ify", "Spotify"), 1.0);
        assert_eq!(score("vscode", "VS Code"), 1.0);
    }

    #[test]
    fn a_word_of_the_name_is_enough_when_it_is_unambiguous() {
        assert_eq!(
            pick_name("code", &["Visual Studio Code", "Notepad", "Calculator"]),
            Pick::Unique("Visual Studio Code".into())
        );
        assert_eq!(
            pick_name("chrome", &["Google Chrome", "Firefox"]),
            Pick::Unique("Google Chrome".into())
        );
    }

    #[test]
    fn an_exact_name_beats_a_partial_one() {
        assert_eq!(
            pick_name("word", &["Word", "WordPad"]),
            Pick::Unique("Word".into())
        );
    }

    #[test]
    fn two_equally_good_candidates_are_a_question() {
        let Pick::Ambiguous(options) = pick_name(
            "teams",
            &[
                "Microsoft Teams (work or school)",
                "Microsoft Teams classic",
            ],
        ) else {
            panic!("expected ambiguity");
        };
        assert_eq!(options.len(), 2);
    }

    #[test]
    fn nothing_close_is_none() {
        assert_eq!(
            pick_name("photoshop", &["Notepad", "Calculator"]),
            Pick::None
        );
        assert_eq!(pick_name("", &["Notepad"]), Pick::None);
    }

    #[test]
    fn a_small_mishearing_still_matches() {
        assert_eq!(
            pick_name("spotfy", &["Spotify", "Slack"]),
            Pick::Unique("Spotify".into())
        );
    }
}
