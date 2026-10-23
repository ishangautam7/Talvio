//! `web_search`: open a search results page in the default browser.
//!
//! The engine is the user's choice (`command_search_url`, a template with a
//! `{query}` placeholder, Google by default), so this tool only builds the
//! address. It opens results for the user to read; the assistant's own web
//! search (`crate::web_search`) is a different thing that reads results for
//! the model.

use log::{info, warn};
use reqwest::Url;
use serde_json::{json, Value};

use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};

/// Used when the setting is blank, so a cleared field does not break search.
const DEFAULT_TEMPLATE: &str = "https://www.google.com/search?q={query}";
const PLACEHOLDER: &str = "{query}";

pub static SPEC: ToolSpec = ToolSpec {
    name: "web_search",
    description: "Search the web and open the results page in the default browser. Use to look \
                  something up, find a page whose address is not known, or search one site \
                  (with site).",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &["search", "google", "look up", "find", "web", "browse", "what is"],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "maxLength": 500,
                "description": "What to search for, as the user would type it into a search box."
            },
            "site": {
                "type": "string",
                "maxLength": 200,
                "description": "Limit results to one site, as a domain (\"reddit.com\"). Only \
                                when the user names a site."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(query) = arg_str(&args, "query") else {
        return ToolOutcome::ask("What should I search for?", Vec::new());
    };
    let full_query = match arg_str(&args, "site").map(site_domain) {
        Some(site) if !site.is_empty() => format!("site:{site} {query}"),
        _ => query.to_string(),
    };
    let url = match search_url(&ctx.settings.command_search_url, &full_query) {
        Ok(url) => url,
        Err(e) => {
            warn!("command mode: web_search has an unusable search address: {e}");
            return ToolOutcome::failed(e, false);
        }
    };
    if let Err(e) = ctx.effects.open_url(url.as_str()) {
        warn!("command mode: web_search could not open the browser: {e}");
        return ToolOutcome::failed(e, false);
    }
    info!(
        "command mode: web_search opened results on {}",
        url.host_str().unwrap_or_default()
    );
    ToolOutcome::done(
        "searchOpened",
        "Searching for {{query}}.",
        &[("query", query)],
    )
}

/// "https://www.reddit.com/r/x" → "www.reddit.com": `site:` takes a domain.
fn site_domain(site: &str) -> String {
    let site = site.trim();
    let without_scheme = site.split_once("://").map_or(site, |(_, rest)| rest);
    without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The results address for `query` from the user's template. `{query}` is
/// replaced with the percent-encoded query everywhere it appears; a template
/// without it gets `q=` appended, which is what nearly every engine uses.
fn search_url(template: &str, query: &str) -> Result<Url, String> {
    let template = template.trim();
    let template = if template.is_empty() {
        DEFAULT_TEMPLATE
    } else {
        template
    };
    let encoded = encode_component(query);
    let address = if template.contains(PLACEHOLDER) {
        template.replace(PLACEHOLDER, &encoded)
    } else {
        let joiner = if !template.contains('?') {
            "?"
        } else if template.ends_with('?') || template.ends_with('&') {
            ""
        } else {
            "&"
        };
        format!("{template}{joiner}q={encoded}")
    };
    let url = Url::parse(&address).map_err(|_| {
        "The search address in Settings is not a web address. Check Command mode's search \
         engine."
            .to_string()
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(
            "The search address in Settings must start with https://. Check Command mode's \
             search engine."
                .to_string(),
        );
    }
    Ok(url)
}

/// Percent-encode everything but RFC 3986's unreserved characters. A space
/// becomes `%20`, not `+`, because a template may put the query in a path
/// (`https://example.com/search/{query}`), where `+` is a literal plus.
fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
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

    fn ctx(fake: &Arc<FakeEffects>, template: &str) -> ToolCtx {
        let mut settings = crate::settings::get_default_settings();
        settings.command_search_url = template.to_string();
        settings.command_dry_run = false;
        let now = FixedOffset::east_opt(5 * 3600 + 45 * 60)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 9, 15, 0)
            .unwrap();
        ToolCtx {
            settings,
            context: CommandContext::at(now, None, Platform::Windows, None, None),
            effects: fake.clone(),
            contacts: Arc::new(ContactBook::new(Vec::new())),
            platform: Platform::Windows,
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

    fn query_param(url: &str, key: &str) -> Option<String> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    }

    /// The query survives the template exactly: spaces, `&`, `#`, `+` and
    /// non-ASCII cannot leak into the URL's structure.
    #[test]
    fn the_query_is_encoded_into_the_default_template() {
        let fake = Arc::new(FakeEffects::default());
        let query = "C++ & Rust #1 café?";
        let outcome = run(
            &ctx(&fake, "https://www.google.com/search?q={query}"),
            json!({ "query": query }),
        );
        assert_eq!(
            outcome,
            ToolOutcome::done(
                "searchOpened",
                "Searching for {{query}}.",
                &[("query", query)]
            )
        );
        let url = opened_url(&fake);
        assert!(url.starts_with("https://www.google.com/search?q="), "{url}");
        assert!(url.contains("C%2B%2B%20%26%20Rust%20%231"), "{url}");
        assert_eq!(query_param(&url, "q").as_deref(), Some(query));
        assert_eq!(Url::parse(&url).unwrap().fragment(), None);
    }

    #[test]
    fn a_site_is_prepended_as_a_site_operator() {
        let fake = Arc::new(FakeEffects::default());
        run(
            &ctx(&fake, "https://www.google.com/search?q={query}"),
            json!({"query": "rust tauri", "site": "https://www.reddit.com/r/rust"}),
        );
        assert_eq!(
            query_param(&opened_url(&fake), "q").as_deref(),
            Some("site:www.reddit.com rust tauri")
        );
    }

    /// Another engine, a template without the placeholder, and a blank
    /// setting all still produce a working search.
    #[test]
    fn other_templates_work_and_a_missing_placeholder_appends_q() {
        let url = search_url("https://duckduckgo.com/?q={query}&ia=web", "a b").unwrap();
        assert_eq!(url.as_str(), "https://duckduckgo.com/?q=a%20b&ia=web");

        let url = search_url("https://search.example/find", "a b").unwrap();
        assert_eq!(url.as_str(), "https://search.example/find?q=a%20b");

        let url = search_url("https://search.example/find?lang=en", "a").unwrap();
        assert_eq!(url.as_str(), "https://search.example/find?lang=en&q=a");

        let url = search_url("https://example.com/search/{query}", "a+b c").unwrap();
        assert_eq!(url.as_str(), "https://example.com/search/a%2Bb%20c");

        let url = search_url("   ", "x").unwrap();
        assert_eq!(url.as_str(), "https://www.google.com/search?q=x");
    }

    #[test]
    fn a_template_that_is_not_a_web_address_fails_without_escalating() {
        for template in ["not a url {query}", "file:///C:/search?q={query}", "javascript:{query}"] {
            let fake = Arc::new(FakeEffects::default());
            let outcome = run(&ctx(&fake, template), json!({"query": "x"}));
            assert!(
                matches!(outcome, ToolOutcome::Failed { escalate: false, .. }),
                "{template}: {outcome:?}"
            );
            assert!(fake.actions().is_empty());
        }
    }

    #[test]
    fn site_domains_are_trimmed_to_the_host() {
        assert_eq!(site_domain("reddit.com"), "reddit.com");
        assert_eq!(site_domain("https://github.com/x?y#z"), "github.com");
        assert_eq!(site_domain("  "), "");
    }

    #[test]
    fn a_browser_that_will_not_open_fails_without_escalating() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("No browser".into()),
            ..Default::default()
        });
        assert_eq!(
            run(&ctx(&fake, DEFAULT_TEMPLATE), json!({"query": "x"})),
            ToolOutcome::failed("No browser", false)
        );
    }
}
