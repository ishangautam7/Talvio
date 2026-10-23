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

    pub async fn devices(&self, token: &str) -> Result<Vec<Device>, String> {
        let request = self.client.get(self.url("/v1/devices")).bearer_auth(token);
        self.json::<DeviceList>(request).await.map(|l| l.devices)
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
            error_code(StatusCode::PAYLOAD_TOO_LARGE, Some("payload_too_large")),
            "server"
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
