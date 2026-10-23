//! `open_url`: open a web page in the default browser.
//!
//! Only `http` and `https` are opened. The URL comes from a model, and the
//! model reads text the user did not write (window titles, a selection), so
//! anything else the OS would happily hand to a handler (`file:` runs or
//! reveals local files, `javascript:` and `data:` are pages that are code,
//! custom schemes launch arbitrary apps) is refused. A bare domain the way
//! people say it ("github.com") gets `https://`.

use log::{info, warn};
use reqwest::Url;
use serde_json::{json, Value};

use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};

pub static SPEC: ToolSpec = ToolSpec {
    name: "open_url",
    description: "Open a website or web address in the default browser. Use when the user names \
                  a site or gives an address (\"open github.com\"); use web_search to look \
                  something up instead.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &[
        "open", "website", "site", "page", "url", "link", "browser", "go to", ".com",
    ],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "minLength": 1,
                "maxLength": 4000,
                "description": "The address: a full https:// URL, or a domain such as \
                                \"github.com\"."
            }
        },
        "required": ["url"],
        "additionalProperties": false
    })
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(raw) = arg_str(&args, "url") else {
        return ToolOutcome::ask("Which website should I open?", Vec::new());
    };
    let url = match web_url(raw) {
        Ok(url) => url,
        Err(reason) => {
            warn!("command mode: open_url refused an address: {reason}");
            return ToolOutcome::failed(reason, false);
        }
    };
    if let Err(e) = ctx.effects.open_url(url.as_str()) {
        warn!("command mode: open_url could not open the browser: {e}");
        return ToolOutcome::failed(e, false);
    }
    let host = url.host_str().unwrap_or_default();
    let site = host.strip_prefix("www.").unwrap_or(host);
    info!("command mode: open_url opened a page on {site}");
    ToolOutcome::done("urlOpened", "Opened {{site}}.", &[("site", site)])
}

/// `raw` as an `http(s)` URL with a real host, or why it is not one.
fn web_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(char::is_whitespace) {
        return Err(format!("“{raw}” is not a web address."));
    }
    let candidate = if has_scheme(raw) {
        raw.to_string()
    } else {
        format!("https://{}", raw.trim_start_matches('/'))
    };
    let url = Url::parse(&candidate).map_err(|_| format!("“{raw}” is not a web address."))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(format!(
                "I only open web pages, and “{scheme}:” addresses are not web pages."
            ))
        }
    }
    let host = url.host_str().unwrap_or_default();
    let looks_real = host.contains('.')
        || host.eq_ignore_ascii_case("localhost")
        || host.starts_with('[');
    if !looks_real {
        return Err(format!("“{raw}” is not a web address."));
    }
    Ok(url)
}

/// Whether `raw` starts with a URL scheme (`https:`, `file:`, `javascript:`).
/// "localhost:3000" and "example.com:8080" do not: a colon followed by a port
/// number is a host and port, not a scheme.
fn has_scheme(raw: &str) -> bool {
    let Some((scheme, rest)) = raw.split_once(':') else {
        return false;
    };
    let valid_scheme = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let is_port = rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        && !rest.starts_with("//");
    valid_scheme && !is_port
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

    fn ctx(fake: &Arc<FakeEffects>) -> ToolCtx {
        let mut settings = crate::settings::get_default_settings();
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

    #[test]
    fn a_full_https_address_is_opened_as_given() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(
            &ctx(&fake),
            json!({"url": "https://www.example.com/a/b?q=1#top"}),
        );
        assert_eq!(
            outcome,
            ToolOutcome::done("urlOpened", "Opened {{site}}.", &[("site", "example.com")])
        );
        assert_eq!(
            fake.actions(),
            vec!["open_url https://www.example.com/a/b?q=1#top".to_string()]
        );
    }

    /// People say the domain, not the scheme.
    #[test]
    fn a_bare_domain_gets_https() {
        assert_eq!(web_url("github.com").unwrap().as_str(), "https://github.com/");
        assert_eq!(
            web_url("github.com/AbhishekBarali").unwrap().as_str(),
            "https://github.com/AbhishekBarali"
        );
        assert_eq!(
            web_url("localhost:3000").unwrap().as_str(),
            "https://localhost:3000/"
        );
        assert_eq!(
            web_url("example.com:8080/x").unwrap().as_str(),
            "https://example.com:8080/x"
        );
        assert_eq!(web_url("http://example.com").unwrap().scheme(), "http");
    }

    /// Anything that is not a web page is refused before it reaches the OS.
    #[test]
    fn other_schemes_are_refused_without_escalating() {
        for bad in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "data:text/html,<script>x</script>",
            "ms-settings:privacy",
            "vbscript:msgbox",
            "ftp://example.com/file",
            "mailto:ron@example.com",
        ] {
            let fake = Arc::new(FakeEffects::default());
            let outcome = run(&ctx(&fake), json!({ "url": bad }));
            assert!(
                matches!(outcome, ToolOutcome::Failed { escalate: false, .. }),
                "{bad}: {outcome:?}"
            );
            assert!(fake.actions().is_empty(), "{bad} was opened");
        }
    }

    #[test]
    fn words_and_hostless_names_are_not_addresses() {
        assert!(web_url("the weather today").is_err());
        assert!(web_url("github").is_err());
        assert!(web_url("https://").is_err());
        assert!(web_url("").is_err());
    }

    #[test]
    fn a_browser_that_will_not_open_fails_without_escalating() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("No browser".into()),
            ..Default::default()
        });
        assert_eq!(
            run(&ctx(&fake), json!({"url": "example.com"})),
            ToolOutcome::failed("No browser", false)
        );
    }
}
