//! Well-known websites by the name people say: "open YouTube", "go to Gmail".
//!
//! The router model would otherwise have to guess an address, or call
//! `open_app` for something that has no app, which fails. `open_app` and
//! `open_url` both look a spoken name up here, so "open YouTube" always opens
//! youtube.com and never a search for the word "youtube".
//!
//! A site can also have a desktop app ([`Site::apps`]); `open_app` launches
//! that app instead when it is installed under exactly that name, so "open
//! Spotify" starts Spotify and only falls back to the web player without it.

use super::fuzzy;

/// One well-known site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    /// How it is named back to the user: "YouTube".
    pub name: &'static str,
    pub url: &'static str,
    /// Other names people use for it, beside `name`.
    pub aliases: &'static [&'static str],
    /// Installed apps that are this site's own desktop app, by name. Only an
    /// app with exactly one of these names beats the site: a near miss such
    /// as "Google Chrome" for "Google" must not.
    pub apps: &'static [&'static str],
}

pub const SITES: &[Site] = &[
    Site {
        name: "YouTube",
        url: "https://www.youtube.com/",
        aliases: &["you tube", "yt"],
        apps: &["YouTube"],
    },
    Site {
        name: "YouTube Music",
        url: "https://music.youtube.com/",
        aliases: &["yt music"],
        apps: &["YouTube Music"],
    },
    Site {
        name: "Gmail",
        url: "https://mail.google.com/",
        aliases: &["google mail", "g mail"],
        apps: &["Gmail"],
    },
    Site {
        name: "Google",
        url: "https://www.google.com/",
        aliases: &["google search", "google.com"],
        apps: &[],
    },
    Site {
        name: "Google Maps",
        url: "https://www.google.com/maps",
        aliases: &["maps", "google map"],
        apps: &["Google Maps"],
    },
    Site {
        name: "Google Drive",
        url: "https://drive.google.com/",
        aliases: &["drive"],
        apps: &["Google Drive"],
    },
    Site {
        name: "Google Docs",
        url: "https://docs.google.com/",
        aliases: &["docs"],
        apps: &["Google Docs"],
    },
    Site {
        name: "GitHub",
        url: "https://github.com/",
        aliases: &["git hub"],
        apps: &["GitHub"],
    },
    Site {
        name: "Netflix",
        url: "https://www.netflix.com/",
        aliases: &[],
        apps: &["Netflix"],
    },
    Site {
        name: "Spotify",
        url: "https://open.spotify.com/",
        aliases: &["spotify web", "spotify web player"],
        apps: &["Spotify"],
    },
    Site {
        name: "WhatsApp",
        url: "https://web.whatsapp.com/",
        aliases: &["whatsapp web", "whats app"],
        apps: &["WhatsApp", "WhatsApp Beta"],
    },
    Site {
        name: "ChatGPT",
        url: "https://chatgpt.com/",
        aliases: &["chat gpt", "openai chat"],
        apps: &["ChatGPT"],
    },
    Site {
        name: "Facebook",
        url: "https://www.facebook.com/",
        aliases: &["fb"],
        apps: &["Facebook"],
    },
    Site {
        name: "Instagram",
        url: "https://www.instagram.com/",
        aliases: &["insta", "ig"],
        apps: &["Instagram"],
    },
    Site {
        name: "X",
        url: "https://x.com/",
        aliases: &["twitter", "x twitter", "x.com"],
        apps: &["X", "Twitter"],
    },
    Site {
        name: "LinkedIn",
        url: "https://www.linkedin.com/",
        aliases: &["linked in"],
        apps: &["LinkedIn"],
    },
    Site {
        name: "Reddit",
        url: "https://www.reddit.com/",
        aliases: &[],
        apps: &["Reddit"],
    },
    Site {
        name: "Amazon",
        url: "https://www.amazon.com/",
        aliases: &[],
        apps: &["Amazon"],
    },
    Site {
        name: "Wikipedia",
        url: "https://www.wikipedia.org/",
        aliases: &["wiki"],
        apps: &[],
    },
];

/// Words around a site's name that are not part of it: "the YouTube website",
/// "my Gmail", "YouTube.com" is handled by the `.com` aliases where it matters.
const FILLER: &[&str] = &[
    "the", "my", "website", "site", "web", "page", "homepage", "home", "app", "tab",
];

/// The site `spoken` names, compared after normalizing (case, punctuation,
/// spacing). An exact name or alias only: anything fuzzier would turn "open
/// my drive notes" into Google Drive.
pub fn named(spoken: &str) -> Option<&'static Site> {
    let exact = |query: &str| {
        let query = tight(query);
        (!query.is_empty())
            .then(|| {
                SITES
                    .iter()
                    .find(|site| names_of(site).any(|n| tight(n) == query))
            })
            .flatten()
    };
    if let Some(site) = exact(spoken) {
        return Some(site);
    }
    let normalized = fuzzy::normalize(spoken);
    let words: Vec<&str> = normalized
        .split(' ')
        .filter(|w| !FILLER.contains(w))
        .collect();
    if words.is_empty() {
        return None;
    }
    exact(&words.join(" ")).or_else(|| {
        // "youtube.com" normalizes to "youtube com".
        let last = words.last().copied();
        (words.len() > 1 && matches!(last, Some("com" | "org")))
            .then(|| exact(&words[..words.len() - 1].join(" ")))
            .flatten()
    })
}

/// Whether `app_name` is this site's own desktop app.
pub fn is_own_app(site: &Site, app_name: &str) -> bool {
    let app = tight(app_name);
    let bare = super::tools::open_app::strip_qualifier(app_name).map(tight);
    site.apps
        .iter()
        .any(|own| tight(own) == app || bare.as_deref() == Some(tight(own).as_str()))
}

fn names_of(site: &Site) -> impl Iterator<Item = &'static str> {
    std::iter::once(site.name).chain(site.aliases.iter().copied())
}

/// Normalized with the spaces removed: "You Tube", "youtube" and "YouTube"
/// are one name.
fn tight(name: &str) -> String {
    fuzzy::normalize(name).split(' ').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_site_the_user_listed_is_known_by_name() {
        for (spoken, url) in [
            ("youtube", "https://www.youtube.com/"),
            ("YouTube", "https://www.youtube.com/"),
            ("You Tube", "https://www.youtube.com/"),
            ("youtube music", "https://music.youtube.com/"),
            ("gmail", "https://mail.google.com/"),
            ("google", "https://www.google.com/"),
            ("maps", "https://www.google.com/maps"),
            ("google drive", "https://drive.google.com/"),
            ("docs", "https://docs.google.com/"),
            ("github", "https://github.com/"),
            ("netflix", "https://www.netflix.com/"),
            ("spotify", "https://open.spotify.com/"),
            ("whatsapp", "https://web.whatsapp.com/"),
            ("chatgpt", "https://chatgpt.com/"),
            ("chat gpt", "https://chatgpt.com/"),
            ("facebook", "https://www.facebook.com/"),
            ("instagram", "https://www.instagram.com/"),
            ("twitter", "https://x.com/"),
            ("x", "https://x.com/"),
            ("linkedin", "https://www.linkedin.com/"),
            ("reddit", "https://www.reddit.com/"),
            ("amazon", "https://www.amazon.com/"),
            ("wikipedia", "https://www.wikipedia.org/"),
        ] {
            assert_eq!(named(spoken).map(|s| s.url), Some(url), "{spoken}");
        }
    }

    #[test]
    fn filler_words_and_a_dot_com_are_ignored() {
        assert_eq!(named("the YouTube website").unwrap().name, "YouTube");
        assert_eq!(named("my Gmail").unwrap().name, "Gmail");
        assert_eq!(named("YouTube.com").unwrap().name, "YouTube");
        assert_eq!(named("netflix.com").unwrap().name, "Netflix");
    }

    #[test]
    fn other_names_are_not_sites() {
        for spoken in [
            "Visual Studio Code",
            "my drive notes",
            "youtubers",
            "",
            "the",
            "google chrome",
            "notepad",
        ] {
            assert_eq!(named(spoken), None, "{spoken}");
        }
    }

    /// Only the site's own app beats it, never a near namesake.
    #[test]
    fn a_site_knows_its_own_app() {
        let spotify = named("spotify").unwrap();
        assert!(is_own_app(spotify, "Spotify"));
        assert!(!is_own_app(spotify, "Spotify Tools"));
        let google = named("google").unwrap();
        assert!(!is_own_app(google, "Google Chrome"));
        let whatsapp = named("whatsapp").unwrap();
        assert!(is_own_app(whatsapp, "WhatsApp"));
        assert!(is_own_app(named("youtube").unwrap(), "YouTube (Chrome)"));
    }
}
