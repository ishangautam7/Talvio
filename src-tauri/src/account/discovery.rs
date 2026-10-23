//! Finding the account server (`server/PROTOCOL.md`, "Finding the server"):
//! its host name changes from time to time, so the app reads it from one
//! fixed address instead of building it in.
//!
//! That address is a trust anchor (whoever controls it decides where sign-ins
//! and tokens go), so it is fetched over https only, never follows a redirect
//! to another host, and carries no token or anything about the user.

use std::time::{Duration, Instant};

use serde_json::Value;

/// The one address that says where the account server is.
pub const DISCOVERY_URL: &str = "https://domain.gautamishan.com.np";
const TIMEOUT: Duration = Duration::from_secs(10);
/// Asked at most this often.
const EVERY: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 5;

/// The server an answer names, if this build may use it: `domain`, else the
/// object's only string field, read by [`server_from`].
pub fn parse(body: &str, release: bool) -> Option<String> {
    let Ok(Value::Object(fields)) = serde_json::from_str(body) else {
        return None;
    };
    let value = match fields.get("domain") {
        Some(domain) => domain.as_str()?,
        None => {
            let mut strings = fields.values().filter_map(Value::as_str);
            match (strings.next(), strings.next()) {
                (Some(only), None) => only,
                _ => return None,
            }
        }
    };
    server_from(value, release)
}

/// `https://<host[:port]>` for a host, `host:port` or https URL (its path,
/// query and fragment dropped); outside release builds also an explicit
/// `http://` URL. Never a user/password part, nor a loopback host in a release
/// build. Also how a saved server is read back, since a development build may
/// have saved one a release build refuses.
pub fn server_from(value: &str, release: bool) -> Option<String> {
    let value = value.trim();
    let url = if value.contains("://") {
        reqwest::Url::parse(value)
    } else {
        reqwest::Url::parse(&format!("https://{value}"))
    }
    .ok()?;
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" => !release,
        _ => false,
    };
    let host = url.host_str().filter(|host| !host.is_empty())?;
    (scheme_ok
        && url.username().is_empty()
        && url.password().is_none()
        && !(release && super::is_loopback(host)))
    .then(|| url.origin().ascii_serialization())
}

/// When to ask, and whether a failure is worth a log line: the address not
/// answering is a normal state, logged once per run of failures.
#[derive(Default)]
pub struct Discovery {
    asked: Option<Instant>,
    failing: bool,
}

impl Discovery {
    /// Whether to ask now, at most once a minute; if so, it counts as asked.
    pub fn due(&mut self, now: Instant) -> bool {
        if self
            .asked
            .is_some_and(|at| now.saturating_duration_since(at) < EVERY)
        {
            return false;
        }
        self.asked = Some(now);
        true
    }

    /// Note a failure; true when it is the first since the last success.
    pub fn failed(&mut self) -> bool {
        !std::mem::replace(&mut self.failing, true)
    }

    pub fn succeeded(&mut self) {
        self.failing = false;
    }
}

/// Ask the discovery address. The error is for the log, never shown.
pub async fn fetch(release: bool) -> Result<String, String> {
    let client = reqwest::Client::builder()
        // Also refuses a redirect to plain http.
        .https_only(true)
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let home = attempt.previous().first().and_then(|url| url.host_str());
            if attempt.previous().len() <= MAX_REDIRECTS && attempt.url().host_str() == home {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(DISCOVERY_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("it answered {}", response.status()));
    }
    let body = response.text().await.map_err(|e| e.to_string())?;
    parse(&body, release).ok_or_else(|| "its answer names no usable server".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: bool = false;
    const RELEASE: bool = true;

    #[test]
    fn reads_the_domain_field() {
        assert_eq!(
            parse(r#"{"domain":"sync.example.com"}"#, RELEASE).as_deref(),
            Some("https://sync.example.com")
        );
        assert_eq!(
            parse(r#"{"domain":"Sync.Example.com:8443","note":"x"}"#, RELEASE).as_deref(),
            Some("https://sync.example.com:8443")
        );
        // A present `domain` that is not usable is not replaced by another field.
        assert_eq!(
            parse(r#"{"domain":1,"host":"sync.example.com"}"#, DEV),
            None
        );
    }

    #[test]
    fn falls_back_to_the_only_string_field() {
        assert_eq!(
            parse(r#"{"host":"sync.example.com","ttl":60}"#, RELEASE).as_deref(),
            Some("https://sync.example.com")
        );
        assert_eq!(
            parse(r#"{"a":"x.example.com","b":"y.example.com"}"#, DEV),
            None
        );
        assert_eq!(parse(r#"{"ttl":60}"#, DEV), None);
    }

    #[test]
    fn takes_hosts_and_https_urls() {
        for (value, server) in [
            ("sync.example.com", "https://sync.example.com"),
            (" sync.example.com:8443 ", "https://sync.example.com:8443"),
            (
                "https://sync.example.com/zv/?a=b#c",
                "https://sync.example.com",
            ),
            ("https://sync.example.com:443/", "https://sync.example.com"),
            (
                "https://sync.example.com:8443",
                "https://sync.example.com:8443",
            ),
            ("203.0.113.7", "https://203.0.113.7"),
        ] {
            assert_eq!(
                server_from(value, RELEASE).as_deref(),
                Some(server),
                "{value:?}"
            );
        }
    }

    #[test]
    fn plain_http_only_outside_release_builds() {
        assert_eq!(
            server_from("http://192.168.1.5:8787", DEV).as_deref(),
            Some("http://192.168.1.5:8787")
        );
        assert_eq!(
            server_from("http://localhost:8787", DEV).as_deref(),
            Some("http://localhost:8787")
        );
        assert_eq!(server_from("http://192.168.1.5:8787", RELEASE), None);
        assert_eq!(server_from("http://sync.example.com", RELEASE), None);
    }

    #[test]
    fn refuses_the_rest() {
        for release in [DEV, RELEASE] {
            for bad in [
                "",
                "   ",
                "user@sync.example.com",
                "https://user:pass@sync.example.com",
                "ftp://sync.example.com",
                "ws://sync.example.com",
                "javascript:alert(1)",
                "sync example.com",
                "https://",
            ] {
                assert_eq!(server_from(bad, release), None, "{bad:?}");
            }
        }
        for loopback in [
            "localhost",
            "https://localhost:8787",
            "127.0.0.1",
            "https://[::1]:8443",
        ] {
            assert_eq!(server_from(loopback, RELEASE), None, "{loopback:?}");
            assert!(server_from(loopback, DEV).is_some(), "{loopback:?}");
        }
        for garbage in ["", "not json", "[]", r#""sync.example.com""#, "null", "{}"] {
            assert_eq!(parse(garbage, DEV), None, "{garbage:?}");
        }
    }

    #[test]
    fn asks_at_most_once_a_minute() {
        let start = Instant::now();
        let mut discovery = Discovery::default();
        assert!(discovery.due(start));
        assert!(!discovery.due(start));
        assert!(!discovery.due(start + Duration::from_secs(59)));
        assert!(discovery.due(start + EVERY));
        assert!(!discovery.due(start + EVERY + Duration::from_secs(30)));
        assert!(discovery.due(start + EVERY * 2));
    }

    #[test]
    fn logs_a_failure_once_per_streak() {
        let mut discovery = Discovery::default();
        assert!(discovery.failed());
        assert!(!discovery.failed());
        discovery.succeeded();
        assert!(discovery.failed());
    }
}
