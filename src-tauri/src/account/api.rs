//! The account server's HTTP routes (`server/PROTOCOL.md`). Every error is one
//! of the codes the account commands return, never prose: the server's own
//! code when it is one the app knows, `network` when the server could not be
//! reached, `server` for anything else.

use std::time::Duration;

use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{AccountDevice, AccountUser};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Server error codes the account commands pass through. Any other code, and
/// any failure without one, is reported as `server`.
const KNOWN_CODES: &[&str] = &[
    "invalid_credentials",
    "email_taken",
    "invalid_email",
    "weak_password",
    "rate_limited",
    "invalid_code",
    "google_unavailable",
    "unauthorized",
    "not_found",
    "invalid_request",
    "payload_too_large",
];

/// What a client says about itself when it signs in.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub name: String,
    pub platform: &'static str,
    pub kind: &'static str,
    pub app_version: &'static str,
}

impl DeviceInfo {
    /// This computer: its host name, its OS and this build's version.
    pub fn this_computer() -> Self {
        Self {
            name: device_name(&tauri_plugin_os::hostname()),
            platform: platform(),
            kind: "desktop",
            app_version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// A host name as a device name: 1–64 characters, without macOS's `.local`.
fn device_name(host: &str) -> String {
    let host = host.trim();
    let host = host.strip_suffix(".local").unwrap_or(host).trim();
    if host.is_empty() {
        return "Computer".to_string();
    }
    host.chars().take(64).collect()
}

fn platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

/// `User` on the wire.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub has_password: bool,
    pub google_linked: bool,
}

impl From<User> for AccountUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email,
            name: user.name,
            has_password: user.has_password,
            google_linked: user.google_linked,
        }
    }
}

/// `Device` on the wire.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub online: bool,
    pub last_seen_at: i64,
    pub current: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl From<Device> for AccountDevice {
    fn from(device: Device) -> Self {
        Self {
            id: device.id,
            name: device.name,
            platform: device.platform,
            kind: device.kind,
            online: device.online,
            last_seen_at: device.last_seen_at,
            current: device.current,
            capabilities: device.capabilities,
            approved: false,
        }
    }
}

#[derive(Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: User,
    pub device: Device,
}

#[derive(Deserialize)]
pub struct Config {
    pub google: bool,
}

#[derive(Deserialize)]
struct DeviceList {
    devices: Vec<Device>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
}

/// The code an account command returns for a failed response.
fn error_code(status: reqwest::StatusCode, code: Option<&str>) -> String {
    match code {
        Some(code) if KNOWN_CODES.contains(&code) => code.to_string(),
        _ if status == reqwest::StatusCode::UNAUTHORIZED => "unauthorized".to_string(),
        // A reverse proxy in front may refuse these with a body of its own.
        _ if status == reqwest::StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large".to_string(),
        _ if status == reqwest::StatusCode::TOO_MANY_REQUESTS => "rate_limited".to_string(),
        _ => "server".to_string(),
    }
}

fn transport_error(error: reqwest::Error) -> String {
    debug!("Account server request failed: {error}");
    if error.is_decode() {
        "server".to_string()
    } else {
        "network".to_string()
    }
}

/// A device id from the frontend goes into a URL path, so it must not be able
/// to name another route (`../me` would delete the account).
pub fn valid_device_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Where an API key's record name may come from: the four kinds of provider
/// key, each with a provider id. Mirrors the server's
/// `^(post_process|cloud_stt|assistant_tts|web_search):[a-z0-9_.-]{1,64}$`, and
/// keeps a name from naming another route when it goes into a URL path.
pub fn valid_secret_name(name: &str) -> bool {
    let Some((kind, id)) = name.split_once(':') else {
        return false;
    };
    matches!(
        kind,
        "post_process" | "cloud_stt" | "assistant_tts" | "web_search"
    ) && (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
}

/// The server's longest accepted key.
pub const MAX_SECRET_LEN: usize = 8192;

/// One synced API key on the wire (`deleted` with an empty value is a
/// tombstone). No derived `Debug`: it holds the key.
#[derive(Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRecord {
    pub name: String,
    #[serde(default)]
    pub value: String,
    pub updated_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

impl std::fmt::Debug for SecretRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretRecord")
            .field("name", &self.name)
            .field("updated_at", &self.updated_at)
            .field("deleted", &self.deleted)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct SecretList {
    secrets: Vec<SecretRecord>,
}

#[derive(Deserialize)]
struct SecretBody {
    secret: SecretRecord,
}

/// What the server did with a key this computer sent.
#[derive(Debug, PartialEq)]
pub enum SecretWrite {
    /// Kept; the record as the server now holds it.
    Saved(SecretRecord),
    /// Refused because the server's copy is newer; that copy, which wins.
    Stale(SecretRecord),
}

/// An ICE server as `GET /v1/turn` hands it out.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IceServers {
    ice_servers: Vec<IceServer>,
}

pub struct Api {
    base: String,
    client: reqwest::Client,
}

impl Api {
    /// A client for the server at `base` (no trailing slash), if this build
    /// may send a password there (`invalid_url` if not).
    pub fn new(base: &str) -> Result<Self, String> {
        if !super::server_allowed(base) {
            return Err("invalid_url".to_string());
        }
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(transport_error)?;
        Ok(Self {
            base: base.to_string(),
            client,
        })
    }

    /// The server this client talks to.
    pub fn base(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let code = response
            .json::<ErrorBody>()
            .await
            .ok()
            .map(|b| b.error.code);
        debug!(
            "Account server answered {status} ({})",
            code.as_deref().unwrap_or("no code")
        );
        Err(error_code(status, code.as_deref()))
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, String> {
        self.send(request)
            .await?
            .json::<T>()
            .await
            .map_err(transport_error)
    }

    pub async fn config(&self) -> Result<Config, String> {
        self.json(self.client.get(self.url("/v1/config"))).await
    }

    pub async fn signup(
        &self,
        email: &str,
        password: &str,
        name: Option<&str>,
        device: &DeviceInfo,
    ) -> Result<AuthResponse, String> {
        let body = json!({ "email": email, "password": password, "name": name, "device": device });
        self.json(self.client.post(self.url("/v1/auth/signup")).json(&body))
            .await
    }

    pub async fn login(
        &self,
        email: &str,
        password: &str,
        device: &DeviceInfo,
    ) -> Result<AuthResponse, String> {
        let body = json!({ "email": email, "password": password, "device": device });
        self.json(self.client.post(self.url("/v1/auth/login")).json(&body))
            .await
    }

    pub async fn exchange(
        &self,
        code: &str,
        code_verifier: &str,
        device: &DeviceInfo,
    ) -> Result<AuthResponse, String> {
        let body = json!({ "code": code, "codeVerifier": code_verifier, "device": device });
        self.json(self.client.post(self.url("/v1/auth/exchange")).json(&body))
            .await
    }

    pub async fn logout(&self, token: &str) -> Result<(), String> {
        let request = self
            .client
            .post(self.url("/v1/auth/logout"))
            .bearer_auth(token);
        self.send(request).await.map(|_| ())
    }

    pub async fn delete_account(&self, token: &str) -> Result<(), String> {
        let request = self.client.delete(self.url("/v1/me")).bearer_auth(token);
        self.send(request).await.map(|_| ())
    }

    /// Sends what a feedback form showed (`POST /v1/feedback`), with the
    /// session's token when there is one. Screenshots can make this a few
    /// megabytes, so it gets longer than the usual request timeout.
    pub async fn feedback(
        &self,
        token: Option<&str>,
        body: &impl Serialize,
        timeout: Duration,
    ) -> Result<(), String> {
        let mut request = self
            .client
            .post(self.url("/v1/feedback"))
            .timeout(timeout)
            .json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        self.send(request).await.map(|_| ())
    }

    /// The STUN and TURN servers a live view connects through (`GET /v1/turn`).
    pub async fn ice_servers(&self, token: &str) -> Result<Vec<IceServer>, String> {
        let request = self.client.get(self.url("/v1/turn")).bearer_auth(token);
        self.json::<IceServers>(request)
            .await
            .map(|l| l.ice_servers)
    }

    pub async fn devices(&self, token: &str) -> Result<Vec<Device>, String> {
        let request = self.client.get(self.url("/v1/devices")).bearer_auth(token);
        self.json::<DeviceList>(request).await.map(|l| l.devices)
    }

    /// Every synced key of the account, tombstones included.
    pub async fn list_secrets(&self, token: &str) -> Result<Vec<SecretRecord>, String> {
        let request = self.client.get(self.url("/v1/secrets")).bearer_auth(token);
        self.json::<SecretList>(request).await.map(|l| l.secrets)
    }

    /// Store `value` as the key `name`, as changed at `updated_at` (ms).
    pub async fn put_secret(
        &self,
        token: &str,
        name: &str,
        value: &str,
        updated_at: i64,
    ) -> Result<SecretWrite, String> {
        if !valid_secret_name(name) || value.is_empty() || value.len() > MAX_SECRET_LEN {
            return Err("invalid_request".to_string());
        }
        let request = self
            .client
            .put(self.url(&format!("/v1/secrets/{name}")))
            .bearer_auth(token)
            .json(&json!({ "value": value, "updatedAt": updated_at }));
        self.write_secret(request).await
    }

    /// Remove the key `name`, as removed at `updated_at` (ms).
    pub async fn delete_secret(
        &self,
        token: &str,
        name: &str,
        updated_at: i64,
    ) -> Result<SecretWrite, String> {
        if !valid_secret_name(name) {
            return Err("invalid_request".to_string());
        }
        let request = self
            .client
            .delete(self.url(&format!("/v1/secrets/{name}?updatedAt={updated_at}")))
            .bearer_auth(token);
        self.write_secret(request).await
    }

    /// A PUT or DELETE of a key: its record, or the newer one a 409 carries.
    async fn write_secret(&self, request: reqwest::RequestBuilder) -> Result<SecretWrite, String> {
        let response = request.send().await.map_err(transport_error)?;
        if response.status() == reqwest::StatusCode::CONFLICT {
            let body = response
                .json::<SecretBody>()
                .await
                .map_err(transport_error)?;
            return Ok(SecretWrite::Stale(body.secret));
        }
        let status = response.status();
        if status.is_success() {
            return response
                .json::<SecretBody>()
                .await
                .map(|body| SecretWrite::Saved(body.secret))
                .map_err(transport_error);
        }
        let code = response
            .json::<ErrorBody>()
            .await
            .ok()
            .map(|b| b.error.code);
        debug!(
            "Account server answered {status} ({})",
            code.as_deref().unwrap_or("no code")
        );
        Err(error_code(status, code.as_deref()))
    }

    pub async fn remove_device(&self, token: &str, id: &str) -> Result<(), String> {
        if !valid_device_id(id) {
            return Err("invalid_request".to_string());
        }
        let request = self
            .client
            .delete(self.url(&format!("/v1/devices/{id}")))
            .bearer_auth(token);
        self.send(request).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn known_server_codes_pass_through() {
        for code in KNOWN_CODES {
            assert_eq!(error_code(StatusCode::BAD_REQUEST, Some(code)), *code);
        }
    }

    #[test]
    fn anything_else_is_a_server_error() {
        assert_eq!(
            error_code(StatusCode::INTERNAL_SERVER_ERROR, Some("internal")),
            "server"
        );
        assert_eq!(
            error_code(StatusCode::BAD_REQUEST, Some("something_new")),
            "server"
        );
        assert_eq!(
            error_code(StatusCode::PAYLOAD_TOO_LARGE, None),
            "payload_too_large"
        );
        assert_eq!(
            error_code(StatusCode::TOO_MANY_REQUESTS, None),
            "rate_limited"
        );
        assert_eq!(error_code(StatusCode::BAD_GATEWAY, None), "server");
        // A proxy's 401 without the server's body still means the token is dead.
        assert_eq!(error_code(StatusCode::UNAUTHORIZED, None), "unauthorized");
    }

    #[test]
    fn device_ids_cannot_reach_other_routes() {
        assert!(valid_device_id("0b6c1f0e-8c1a-4d55-9a57-3f9a2a6c1d2e"));
        for id in ["", "../me", "a/b", "a?b", "a b", "é", &"a".repeat(65)] {
            assert!(!valid_device_id(id), "{id:?}");
        }
    }

    #[test]
    fn secret_names_follow_the_server_rule() {
        for name in [
            "post_process:openai",
            "cloud_stt:elevenlabs",
            "assistant_tts:azure",
            "web_search:brave",
            "post_process:a.b_c-1",
        ] {
            assert!(valid_secret_name(name), "{name}");
        }
        for name in [
            "",
            "post_process:",
            "post_process:OpenAI",
            "account:session",
            "assistant_tts",
            "post_process:a/b",
            "post_process:../x",
            "post_process:a?b",
            &format!("web_search:{}", "a".repeat(65)),
        ] {
            assert!(!valid_secret_name(name), "{name:?}");
        }
    }

    /// A stand-in for the server's `/v1/secrets` routes: last writer by
    /// `updatedAt` wins, a tie overwrites, a removal leaves a tombstone.
    mod mock {
        use std::collections::BTreeMap;
        use std::sync::{Arc, Mutex};

        use serde_json::{json, Value};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        pub const TOKEN: &str = "zv1_test";

        pub type Store = Arc<Mutex<BTreeMap<String, Value>>>;

        fn respond(status: &str, body: &Value) -> Vec<u8> {
            let body = body.to_string();
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_bytes()
        }

        fn handle(store: &Store, method: &str, target: &str, auth: bool, body: &str) -> Vec<u8> {
            if !auth {
                return respond(
                    "401 Unauthorized",
                    &json!({"error": {"code": "unauthorized"}}),
                );
            }
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            let mut store = store.lock().unwrap();
            if method == "GET" && path == "/v1/secrets" {
                let secrets: Vec<&Value> = store.values().collect();
                return respond("200 OK", &json!({ "secrets": secrets }));
            }
            let Some(name) = path.strip_prefix("/v1/secrets/") else {
                return respond("404 Not Found", &json!({"error": {"code": "not_found"}}));
            };
            let (value, updated_at, deleted) = match method {
                "PUT" => {
                    let body: Value = serde_json::from_str(body).unwrap();
                    (
                        body["value"].as_str().unwrap().to_string(),
                        body["updatedAt"].as_i64().unwrap(),
                        false,
                    )
                }
                "DELETE" => {
                    let at = query
                        .strip_prefix("updatedAt=")
                        .and_then(|v| v.parse().ok())
                        .unwrap();
                    (String::new(), at, true)
                }
                _ => return respond("404 Not Found", &json!({"error": {"code": "not_found"}})),
            };
            if let Some(current) = store.get(name) {
                if current["updatedAt"].as_i64().unwrap() > updated_at {
                    return respond(
                        "409 Conflict",
                        &json!({"error": {"code": "stale"}, "secret": current}),
                    );
                }
            }
            let record = json!({
                "name": name, "value": value, "updatedAt": updated_at, "deleted": deleted
            });
            store.insert(name.to_string(), record.clone());
            respond("200 OK", &json!({ "secret": record }))
        }

        /// Serves `store` on a local port and returns the base URL.
        pub async fn serve(store: Store) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let store = store.clone();
                    tokio::spawn(async move {
                        let mut request = Vec::new();
                        let mut buf = [0u8; 4096];
                        let head_end = loop {
                            if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i + 4;
                            }
                            match socket.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => request.extend_from_slice(&buf[..n]),
                            }
                        };
                        let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
                        let header = |name: &str| {
                            head.lines().find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case(name)
                                    .then(|| value.trim().to_string())
                            })
                        };
                        let length: usize = header("content-length")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        while request.len() < head_end + length {
                            match socket.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => request.extend_from_slice(&buf[..n]),
                            }
                        }
                        let body = String::from_utf8_lossy(&request[head_end..]).into_owned();
                        let mut words = head.split_whitespace();
                        let method = words.next().unwrap_or("").to_string();
                        let target = words.next().unwrap_or("/").to_string();
                        let auth = header("authorization").as_deref()
                            == Some(format!("Bearer {TOKEN}").as_str());
                        let response = handle(&store, &method, &target, auth, &body);
                        let _ = socket.write_all(&response).await;
                        let _ = socket.shutdown().await;
                    });
                }
            });
            base
        }
    }

    fn record(name: &str, value: &str, at: i64, deleted: bool) -> SecretRecord {
        SecretRecord {
            name: name.into(),
            value: value.into(),
            updated_at: at,
            deleted,
        }
    }

    #[tokio::test]
    async fn secrets_round_trip_through_the_server() {
        let store = mock::Store::default();
        let api = Api::new(&mock::serve(store.clone()).await).unwrap();
        let token = mock::TOKEN;
        let name = "post_process:openai";

        assert_eq!(api.list_secrets(token).await.unwrap(), vec![]);

        // PUT stores it; GET lists it.
        assert_eq!(
            api.put_secret(token, name, "sk-1", 100).await.unwrap(),
            SecretWrite::Saved(record(name, "sk-1", 100, false))
        );
        assert_eq!(
            api.list_secrets(token).await.unwrap(),
            vec![record(name, "sk-1", 100, false)]
        );

        // An equal time overwrites; an older one is stale and hands back the
        // server's copy, which wins.
        assert_eq!(
            api.put_secret(token, name, "sk-2", 100).await.unwrap(),
            SecretWrite::Saved(record(name, "sk-2", 100, false))
        );
        assert_eq!(
            api.put_secret(token, name, "sk-old", 50).await.unwrap(),
            SecretWrite::Stale(record(name, "sk-2", 100, false))
        );

        // DELETE leaves a tombstone that GET includes; an older DELETE is stale.
        assert_eq!(
            api.delete_secret(token, name, 40).await.unwrap(),
            SecretWrite::Stale(record(name, "sk-2", 100, false))
        );
        assert_eq!(
            api.delete_secret(token, name, 200).await.unwrap(),
            SecretWrite::Saved(record(name, "", 200, true))
        );
        assert_eq!(
            api.list_secrets(token).await.unwrap(),
            vec![record(name, "", 200, true)]
        );
        // A stale PUT against the tombstone gets the tombstone back.
        assert_eq!(
            api.put_secret(token, name, "sk-3", 150).await.unwrap(),
            SecretWrite::Stale(record(name, "", 200, true))
        );
        assert_eq!(store.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn secret_requests_need_the_session_and_a_valid_name() {
        let api = Api::new(&mock::serve(mock::Store::default()).await).unwrap();
        assert_eq!(
            api.list_secrets("wrong").await,
            Err("unauthorized".to_string())
        );
        assert_eq!(
            api.put_secret("wrong", "web_search:brave", "k", 1).await,
            Err("unauthorized".to_string())
        );
        // Refused before anything is sent.
        for (name, value) in [
            ("account:session", "k"),
            ("web_search:../x", "k"),
            ("web_search:brave", ""),
        ] {
            assert_eq!(
                api.put_secret(mock::TOKEN, name, value, 1).await,
                Err("invalid_request".to_string())
            );
        }
        let long = "k".repeat(MAX_SECRET_LEN + 1);
        assert_eq!(
            api.put_secret(mock::TOKEN, "web_search:brave", &long, 1)
                .await,
            Err("invalid_request".to_string())
        );
        assert_eq!(
            api.delete_secret(mock::TOKEN, "account:session", 1).await,
            Err("invalid_request".to_string())
        );
    }

    #[test]
    fn device_names_fit_the_protocol() {
        assert_eq!(device_name("ISHAN-PC"), "ISHAN-PC");
        assert_eq!(
            device_name("Ishans-MacBook-Pro.local"),
            "Ishans-MacBook-Pro"
        );
        assert_eq!(device_name("  "), "Computer");
        assert_eq!(device_name(&"x".repeat(100)).chars().count(), 64);
    }
}
