//! The one network read a tool may make: fetch a page as text.
//!
//! Tools run on a blocking thread under the executor's time limit, while
//! `reqwest` is async only in this build. Each fetch therefore runs on a
//! thread of its own with a current-thread runtime, which works the same
//! whether the caller is a `spawn_blocking` thread, a test, or anything else,
//! and never borrows (or blocks) the app's shared runtime.

use futures_util::StreamExt;
use std::time::Duration;

/// The largest body a fetch reads. A YouTube results page is about 1 MB.
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// What a desktop browser sends, so a site answers with the page a person
/// would get rather than a bot or "unsupported browser" page.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// Fetch `url` (http or https only) and return its body as text, cut at
/// [`MAX_BODY_BYTES`]. `headers` are added to the browser-like defaults.
/// The error is a sentence fit for a log; tools fall back rather than show it.
pub fn fetch_text(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "not a web address".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("only web pages can be fetched".to_string());
    }
    let headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let worker = std::thread::Builder::new()
        .name("command-fetch".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("could not start a network runtime: {e}"))?;
            runtime.block_on(async move {
                tokio::time::timeout(timeout, fetch(parsed, headers, timeout))
                    .await
                    .unwrap_or_else(|_| Err("the page took too long to answer".to_string()))
            })
        })
        .map_err(|e| format!("could not start a network thread: {e}"))?;
    worker
        .join()
        .unwrap_or_else(|_| Err("the network thread failed".to_string()))
}

async fn fetch(
    url: reqwest::Url,
    headers: Vec<(String, String)>,
    timeout: Duration,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .connect_timeout(timeout.min(Duration::from_secs(10)))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("could not build an HTTP client: {e}"))?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT_LANGUAGE, ACCEPT_LANGUAGE)
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        );
    for (name, value) in &headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("could not reach the page: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the page answered {status}"));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("the page stopped mid-way: {e}"))?;
        let room = MAX_BODY_BYTES - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= MAX_BODY_BYTES {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_addresses_are_fetched() {
        let timeout = Duration::from_secs(1);
        assert!(fetch_text("file:///C:/Windows/win.ini", &[], timeout).is_err());
        assert!(fetch_text("javascript:alert(1)", &[], timeout).is_err());
        assert!(fetch_text("not a url", &[], timeout).is_err());
    }
}
