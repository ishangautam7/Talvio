//! Talvio accounts: sign in on this computer and on a phone, and the phone
//! can then use this computer as its speech engine and keyboard.
//!
//! The account server (`server/`, contract in `server/PROTOCOL.md`) keeps
//! accounts and devices only; a phone's audio and text pass through it to this
//! computer and are never stored there. The session token lives in the OS
//! keychain, never in the settings file, in a log or in the webview.
//!
//! - [`api`]: the HTTP routes. [`google`]: the browser sign-in.
//!   [`discovery`]: where the server is; the only source of its address.
//! - [`socket`]: the connection that carries the device list and the phone's
//!   requests.
//! - [`secrets_sync`]: "Sync API keys", the user's provider keys shared by
//!   every computer signed in to the account.
//! - [`remote`]: what this computer does with those requests, and the
//!   extension point for new kinds of them. [`approval`]: the popup that asks
//!   whether a phone may use this computer.
//!
//! Every change the account page shows is pushed as `account-status-changed`
//! with the whole [`AccountStatus`]; command errors are codes the frontend
//! translates, never prose.

mod api;
pub(crate) mod approval;
mod discovery;
mod google;
mod remote;
mod secrets_sync;
mod socket;

#[cfg(test)]
mod account_live_tests;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{mpsc, oneshot};

use crate::settings::{get_settings, write_settings};
use api::{Api, AuthResponse, DeviceInfo};

/// The server a development build uses while discovery has found none.
const DEFAULT_ACCOUNT_SERVER_URL: &str = "http://localhost:8787";

/// Keychain account holding the session (token, device id, user).
const SESSION_ACCOUNT: &str = "account:session";
const STATUS_EVENT: &str = "account-status-changed";
/// How soon an unreachable server's `/v1/config` is asked again.
const CONFIG_RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AccountUser {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub has_password: bool,
    pub google_linked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct AccountDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub online: bool,
    pub last_seen_at: i64,
    pub current: bool,
    pub capabilities: Vec<String>,
    /// A phone this computer's user allowed to dictate and type here.
    pub approved: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum AccountConnection {
    #[default]
    Offline,
    Connecting,
    Online,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct AccountStatus {
    pub signed_in: bool,
    /// The server in use, as discovery found it; empty while it has found
    /// none.
    pub server_url: String,
    pub user: Option<AccountUser>,
    pub device_id: Option<String>,
    pub connection: AccountConnection,
    /// `None` until the server has answered `/v1/config`.
    pub google_available: Option<bool>,
    /// A Google sign-in is waiting on the browser.
    pub google_pending: bool,
    pub devices: Vec<AccountDevice>,
    pub remote_input_enabled: bool,
}

/// What is kept in the keychain. No `Debug`: it holds the token.
#[derive(Clone, Serialize, Deserialize)]
struct Session {
    token: String,
    device_id: String,
    user: AccountUser,
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    /// Bumped whenever the session starts or ends, so a socket event from an
    /// earlier session is ignored.
    generation: u64,
    connection: AccountConnection,
    devices: Vec<AccountDevice>,
    google_available: Option<bool>,
    config_asked: Option<Instant>,
    tasks: Vec<JoinHandle<()>>,
    google_cancel: Option<oneshot::Sender<()>>,
    /// The session's handler of relayed requests, while one runs.
    router: Option<Arc<remote::Router>>,
    /// Phones the user said Don't allow to in this run of the app.
    refused: HashSet<String>,
    discovery: discovery::Discovery,
}

pub struct AccountManager {
    app: AppHandle,
    inner: Mutex<Inner>,
}

/// Manage the account state and, off the main thread, resume a saved session.
pub fn init(app: &AppHandle) {
    let manager = Arc::new(AccountManager {
        app: app.clone(),
        inner: Mutex::default(),
    });
    app.manage(manager.clone());
    secrets_sync::init(app);
    std::thread::spawn(move || {
        manager.discover();
        if let Some(session) = load_session() {
            info!("Resuming the account session");
            manager.begin(session);
            manager.emit();
            secrets_sync::sync_now();
        }
    });
}

fn manager(app: &AppHandle) -> Arc<AccountManager> {
    app.state::<Arc<AccountManager>>().inner().clone()
}

/// The server in use, if there is one yet.
fn server_url(app: &AppHandle) -> Option<String> {
    let settings = get_settings(app);
    resolve_server(&settings.account_discovered_server, RELEASE)
}

/// A client for the server in use (`no_server` while there is none).
fn server_api(app: &AppHandle) -> Result<Api, String> {
    Api::new(&server_url(app).ok_or_else(|| "no_server".to_string())?)
}

/// The server discovery last found, if this build may use it; else, in
/// development builds only, the local default. There is no other source: the
/// address is never chosen or typed by the user, and the old Custom server
/// setting (`account_server_url`) is ignored.
fn resolve_server(discovered: &str, release: bool) -> Option<String> {
    discovery::server_from(discovered, release)
        .or_else(|| (!release).then(|| DEFAULT_ACCOUNT_SERVER_URL.to_string()))
}

/// Whether a server discovery `found` moves this app, and a signed-in session
/// with it: only to a different server.
fn moves(discovered: &str, found: &str, release: bool) -> bool {
    resolve_server(discovered, release) != resolve_server(found, release)
}

/// Release builds refuse a loopback server, so a development server never
/// receives a real password or token; debug builds also use one. Plain http
/// and https are both accepted in every build: the address comes only from
/// the https discovery answer, which is the trust anchor (`discovery.rs`).
const RELEASE: bool = !cfg!(debug_assertions);

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether this build may talk to the server at `server_url`, by discovery's
/// own rule ([`discovery::server_from`]): an explicit http or https URL with no
/// user/password part, and no loopback host in a release build. Also checked
/// where the stored address is used, since a debug build may have saved one a
/// release build refuses.
fn server_allowed(server_url: &str) -> bool {
    server_url.contains("://") && discovery::server_from(server_url, RELEASE).is_some()
}

impl AccountManager {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn status(&self) -> AccountStatus {
        let settings = get_settings(&self.app);
        let inner = self.lock();
        AccountStatus {
            signed_in: inner.session.is_some(),
            server_url: resolve_server(&settings.account_discovered_server, RELEASE)
                .unwrap_or_default(),
            user: inner.session.as_ref().map(|s| s.user.clone()),
            device_id: inner.session.as_ref().map(|s| s.device_id.clone()),
            connection: inner.connection,
            google_available: inner.google_available,
            // The flow holds the receiving end until it ends, however it ends.
            google_pending: inner
                .google_cancel
                .as_ref()
                .is_some_and(|cancel| !cancel.is_closed()),
            devices: inner
                .devices
                .iter()
                .cloned()
                .map(|mut device| {
                    device.approved = device.kind == "mobile"
                        && settings.account_approved_devices.contains(&device.id);
                    device
                })
                .collect(),
            remote_input_enabled: settings.remote_input_enabled,
        }
    }

    fn emit(&self) {
        if let Err(e) = self.app.emit(STATUS_EVENT, self.status()) {
            warn!("Could not send the account status: {e}");
        }
    }

    fn signed_in(&self) -> bool {
        self.lock().session.is_some()
    }

    fn session(&self) -> Option<(Session, u64)> {
        let inner = self.lock();
        inner.session.clone().map(|s| (s, inner.generation))
    }

    /// The socket address of the server in use, if this build may reach it.
    fn ws_url(&self) -> Option<String> {
        server_url(&self.app)
            .filter(|url| server_allowed(url))
            .and_then(|url| socket::ws_url(&url))
    }

    /// Start a session and its connection. False when one is already running.
    fn begin(&self, session: Session) -> bool {
        let ws_url = self.ws_url();
        let mut inner = self.lock();
        if inner.session.is_some() {
            return false;
        }
        self.connect(&mut inner, ws_url, &session.token);
        inner.session = Some(session);
        true
    }

    /// (Re)start the session's connection, to `ws_url` or, without one, nowhere.
    fn connect(&self, inner: &mut Inner, ws_url: Option<String>, token: &str) {
        for task in inner.tasks.drain(..) {
            task.abort();
        }
        inner.router = None;
        inner.generation += 1;
        inner.devices.clear();
        inner.connection = AccountConnection::Offline;
        if let Some(ws_url) = ws_url {
            let (router, outbox) = remote::Router::new(Arc::new(remote::AppHost(self.app.clone())));
            inner.router = Some(router.clone());
            let (events, mut received) = mpsc::unbounded_channel();
            let socket = tauri::async_runtime::spawn(socket::run(
                ws_url,
                token.to_string(),
                router,
                outbox,
                events,
            ));
            let app = self.app.clone();
            let generation = inner.generation;
            let pump = tauri::async_runtime::spawn(async move {
                while let Some(event) = received.recv().await {
                    manager(&app).on_socket_event(generation, event);
                }
            });
            inner.tasks = vec![socket, pump];
        } else {
            warn!("No account server this build may use yet; staying offline");
        }
    }

    /// Ask where the server is now, at most once a minute.
    fn discover(self: &Arc<Self>) {
        if !self.lock().discovery.due(Instant::now()) {
            return;
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let found = discovery::fetch(RELEASE).await;
            manager.discovered(found);
        });
    }

    /// Keep what discovery found, and follow it if the server moved.
    fn discovered(self: &Arc<Self>, found: Result<String, String>) {
        let found = match found {
            Ok(found) => {
                self.lock().discovery.succeeded();
                found
            }
            Err(why) => {
                if self.lock().discovery.failed() {
                    info!("Could not look up the account server ({why}); will ask again later");
                }
                return;
            }
        };
        let mut settings = get_settings(&self.app);
        if settings.account_discovered_server == found {
            return;
        }
        let moved = moves(&settings.account_discovered_server, &found, RELEASE);
        info!("The account server is at {found}");
        settings.account_discovered_server = found;
        write_settings(&self.app, settings);
        if moved {
            self.moved();
        } else {
            self.emit();
        }
    }

    /// The server in use changed: what was asked of the old one is forgotten,
    /// and a session reconnects to the new one with the same token.
    fn moved(self: &Arc<Self>) {
        let ws_url = self.ws_url();
        {
            let mut inner = self.lock();
            inner.google_available = None;
            inner.config_asked = None;
            if let Some(token) = inner.session.as_ref().map(|s| s.token.clone()) {
                self.connect(&mut inner, ws_url, &token);
            }
        }
        self.emit();
        self.ensure_config();
    }

    /// End the session locally (the keychain is the caller's), returning it.
    /// Its phones' approvals go with it.
    fn end(&self) -> Option<Session> {
        let session = {
            let mut inner = self.lock();
            inner.generation += 1;
            for task in inner.tasks.drain(..) {
                task.abort();
            }
            inner.router = None;
            inner.connection = AccountConnection::Offline;
            inner.devices.clear();
            inner.session.take()
        };
        self.keep_approved(|_| false);
        session
    }

    fn router(&self) -> Option<Arc<remote::Router>> {
        self.lock().router.clone()
    }

    fn is_refused(&self, phone: &str) -> bool {
        self.lock().refused.contains(phone)
    }

    /// Don't allow: no more questions about `phone` until the app restarts or
    /// the user allows it in Settings → Account.
    fn refuse(&self, phone: &str) {
        self.lock().refused.insert(phone.to_string());
    }

    /// Allow `phone` (saved, and any Don't allow forgotten) or take that away.
    fn set_approved(&self, phone: &str, approved: bool) {
        if approved {
            self.lock().refused.remove(phone);
        }
        let mut settings = get_settings(&self.app);
        let listed = settings
            .account_approved_devices
            .iter()
            .any(|id| id == phone);
        if approved != listed {
            if approved {
                settings.account_approved_devices.push(phone.to_string());
            } else {
                settings.account_approved_devices.retain(|id| id != phone);
            }
            write_settings(&self.app, settings);
        }
        self.emit();
    }

    /// Keep only the approved phones `keep` accepts.
    fn keep_approved(&self, keep: impl Fn(&str) -> bool) {
        let mut settings = get_settings(&self.app);
        let before = settings.account_approved_devices.len();
        settings.account_approved_devices.retain(|id| keep(id));
        if settings.account_approved_devices.len() != before {
            write_settings(&self.app, settings);
        }
    }

    /// The server no longer knows this session: clear it everywhere.
    fn revoke(&self) {
        if self.end().is_some() {
            info!("Account session ended by the server");
            tauri::async_runtime::spawn_blocking(forget_session);
            self.emit();
        }
    }

    /// A call's error, after asking whether the server moved when it could
    /// not be reached.
    fn reached(self: &Arc<Self>, code: String) -> String {
        if code == "network" {
            self.discover();
        }
        code
    }

    /// An authenticated call's error, after clearing a revoked session.
    fn checked(self: &Arc<Self>, code: String) -> String {
        if code == "unauthorized" {
            self.revoke();
        }
        self.reached(code)
    }

    fn on_socket_event(self: &Arc<Self>, generation: u64, event: socket::Event) {
        let mut changed_session = None;
        {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            match event {
                socket::Event::Connecting => inner.connection = AccountConnection::Connecting,
                socket::Event::Offline => {
                    inner.connection = AccountConnection::Offline;
                    // Nobody's presence is known while this computer is offline.
                    for device in &mut inner.devices {
                        device.online = false;
                    }
                }
                socket::Event::Ready { device_id, user } => {
                    inner.connection = AccountConnection::Online;
                    let user = AccountUser::from(user);
                    if let Some(session) = inner.session.as_mut() {
                        if session.user != user || session.device_id != device_id {
                            session.user = user;
                            session.device_id = device_id;
                            changed_session = Some(session.clone());
                        }
                    }
                }
                socket::Event::Devices(devices) => {
                    inner.devices = devices.into_iter().map(Into::into).collect();
                    // A device id is per sign-in: one that left never returns.
                    let phones = phone_ids(&inner.devices);
                    drop(inner);
                    self.keep_approved(|id| phones.contains(id));
                    return self.emit();
                }
                socket::Event::Revoked => {
                    drop(inner);
                    return self.revoke();
                }
                socket::Event::Unreachable => {
                    drop(inner);
                    return self.discover();
                }
            }
        }
        if let Some(session) = changed_session {
            tauri::async_runtime::spawn_blocking(move || store_session(&session));
        }
        self.emit();
    }

    /// Ask the server whether it offers Google sign-in, unless that is known
    /// or was just asked.
    fn ensure_config(self: &Arc<Self>) {
        let Some(url) = server_url(&self.app) else {
            // Nothing to ask yet: keep looking for the server instead.
            return self.discover();
        };
        {
            let mut inner = self.lock();
            if inner.google_available.is_some()
                || inner
                    .config_asked
                    .is_some_and(|at| at.elapsed() < CONFIG_RETRY)
            {
                return;
            }
            inner.config_asked = Some(Instant::now());
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let google = match Api::new(&url) {
                Ok(api) => api
                    .config()
                    .await
                    .map(|config| config.google)
                    .map_err(|code| manager.reached(code))
                    .ok(),
                Err(_) => None,
            };
            // A config for a server URL that has since changed says nothing.
            let Some(google) =
                google.filter(|_| server_url(&manager.app).as_deref() == Some(url.as_str()))
            else {
                return;
            };
            manager.lock().google_available = Some(google);
            manager.emit();
        });
    }

    /// Keep a new session, or undo it on the server when it cannot be kept.
    async fn complete_sign_in(
        &self,
        api: &Api,
        auth: AuthResponse,
    ) -> Result<AccountStatus, String> {
        let session = Session {
            token: auth.token,
            device_id: auth.device.id,
            user: auth.user.into(),
        };
        // The server was changed while this waited: the session belongs to
        // the old one, so it is dropped there as if the user had cancelled.
        if server_url(&self.app).as_deref() != Some(api.base()) {
            let _ = api.logout(&session.token).await;
            return Err("cancelled".to_string());
        }
        if !self.begin(session.clone()) {
            let _ = api.logout(&session.token).await;
            return Err("signed_in".to_string());
        }
        // A new sign-in starts with no phone allowed, whatever an earlier
        // session that ended without `end` (an unreadable keychain) left.
        self.keep_approved(|_| false);
        let stored = {
            let session = session.clone();
            tauri::async_runtime::spawn_blocking(move || store_session(&session))
                .await
                .unwrap_or(false)
        };
        if !stored {
            warn!("Could not keep the account session in the OS keychain; signing out again");
            self.end();
            let _ = api.logout(&session.token).await;
            self.emit();
            return Err("keychain".to_string());
        }
        info!("Signed in to the account server");
        self.emit();
        secrets_sync::sync_now();
        Ok(self.status())
    }

    /// End the session here and in the keychain.
    async fn sign_out_locally(&self) -> Result<(), String> {
        self.end();
        self.emit();
        let forgot = tauri::async_runtime::spawn_blocking(forget_session)
            .await
            .unwrap_or(false);
        if forgot {
            Ok(())
        } else {
            Err("keychain".to_string())
        }
    }
}

fn load_session() -> Option<Session> {
    let stored = crate::secret_store::get(SESSION_ACCOUNT)?;
    match serde_json::from_str(&stored) {
        Ok(session) => Some(session),
        Err(_) => {
            warn!("The saved account session is unreadable; signing out");
            crate::secret_store::delete(SESSION_ACCOUNT);
            None
        }
    }
}

/// Never falls back to the settings file, unlike API keys: a session that
/// cannot be kept safely is not kept.
fn store_session(session: &Session) -> bool {
    serde_json::to_string(session)
        .map(|json| crate::secret_store::set(SESSION_ACCOUNT, &json))
        .unwrap_or(false)
}

fn forget_session() -> bool {
    // Without a keychain no session was ever stored.
    !crate::secret_store::is_available() || crate::secret_store::delete(SESSION_ACCOUNT)
}

/// The ids of the account's phones in a device list.
fn phone_ids(devices: &[AccountDevice]) -> HashSet<String> {
    devices
        .iter()
        .filter(|device| device.kind == "mobile")
        .map(|device| device.id.clone())
        .collect()
}

/// The user's answer to the approval question about `phone`.
fn answer_approval(app: &AppHandle, phone: &str, allow: bool) {
    match manager(app).router() {
        Some(router) => router.answer(phone, allow),
        // No session, so no question either: just take the popup down.
        None => approval::close(app, phone),
    }
}

fn optional_name(name: Option<String>) -> Option<String> {
    name.map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

#[tauri::command]
#[specta::specta]
pub fn account_get_status(app: AppHandle) -> AccountStatus {
    let manager = manager(&app);
    manager.ensure_config();
    manager.status()
}

#[tauri::command]
#[specta::specta]
pub async fn account_sign_up(
    app: AppHandle,
    email: String,
    password: String,
    name: Option<String>,
) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    if manager.signed_in() {
        return Err("signed_in".to_string());
    }
    let api = server_api(&app)?;
    let name = optional_name(name);
    let auth = api
        .signup(
            email.trim(),
            &password,
            name.as_deref(),
            &DeviceInfo::this_computer(),
        )
        .await
        .map_err(|code| manager.reached(code))?;
    manager.complete_sign_in(&api, auth).await
}

#[tauri::command]
#[specta::specta]
pub async fn account_sign_in(
    app: AppHandle,
    email: String,
    password: String,
) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    if manager.signed_in() {
        return Err("signed_in".to_string());
    }
    let api = server_api(&app)?;
    let auth = api
        .login(email.trim(), &password, &DeviceInfo::this_computer())
        .await
        .map_err(|code| manager.reached(code))?;
    manager.complete_sign_in(&api, auth).await
}

#[tauri::command]
#[specta::specta]
pub async fn account_sign_in_google(app: AppHandle) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    if manager.signed_in() {
        return Err("signed_in".to_string());
    }
    let api = server_api(&app)?;
    let (cancel, cancelled) = oneshot::channel();
    // A flow still waiting on an earlier browser tab ends here as `cancelled`.
    if let Some(earlier) = manager.lock().google_cancel.replace(cancel) {
        let _ = earlier.send(());
    }
    // The page shows the wait from the status, so it survives a remount.
    manager.emit();
    let opener = app.clone();
    let auth = google::sign_in(
        &api,
        api.base(),
        &DeviceInfo::this_computer(),
        move |start| {
            opener.opener().open_url(start, None::<&str>).map_err(|e| {
                warn!("Could not open the browser for Google sign-in: {e}");
                "google_unavailable".to_string()
            })
        },
        cancelled,
    )
    .await
    .map_err(|code| {
        manager.emit();
        manager.reached(code)
    })?;
    manager.complete_sign_in(&api, auth).await
}

#[tauri::command]
#[specta::specta]
pub fn account_cancel_google(app: AppHandle) {
    if let Some(cancel) = manager(&app).lock().google_cancel.take() {
        let _ = cancel.send(());
    }
}

#[tauri::command]
#[specta::specta]
pub async fn account_sign_out(app: AppHandle) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    let session = manager.session();
    let local = manager.sign_out_locally().await;
    // Best effort, and not waited for: the session is already gone here.
    if let (Some((session, _)), Ok(api)) = (session, server_api(&app)) {
        tauri::async_runtime::spawn(async move {
            let _ = api.logout(&session.token).await;
        });
    }
    local.map(|()| manager.status())
}

#[tauri::command]
#[specta::specta]
pub async fn account_delete(app: AppHandle) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    let Some((session, _)) = manager.session() else {
        return Err("unauthorized".to_string());
    };
    server_api(&app)?
        .delete_account(&session.token)
        .await
        .map_err(|code| manager.checked(code))?;
    info!("Account deleted");
    manager.sign_out_locally().await?;
    Ok(manager.status())
}

#[tauri::command]
#[specta::specta]
pub async fn account_remove_device(
    app: AppHandle,
    device_id: String,
) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    let Some((session, generation)) = manager.session() else {
        return Err("unauthorized".to_string());
    };
    let api = server_api(&app)?;
    api.remove_device(&session.token, &device_id)
        .await
        .map_err(|code| manager.checked(code))?;
    if device_id == session.device_id {
        manager.sign_out_locally().await?;
        return Ok(manager.status());
    }
    // The socket reports the change too, but only once it is connected.
    match api.devices(&session.token).await {
        Ok(devices) => {
            let mut inner = manager.lock();
            if inner.generation == generation {
                inner.devices = devices.into_iter().map(Into::into).collect();
            }
        }
        Err(code) => {
            manager.checked(code);
        }
    }
    manager.emit();
    Ok(manager.status())
}

/// Settings → Account: let one of the account's phones dictate and type on
/// this computer, or take that away. A question about it that is waiting
/// ends the same way.
#[tauri::command]
#[specta::specta]
pub fn account_set_device_approved(
    app: AppHandle,
    device_id: String,
    approved: bool,
) -> Result<AccountStatus, String> {
    let manager = manager(&app);
    let router = {
        let inner = manager.lock();
        if inner.session.is_none() {
            return Err("unauthorized".to_string());
        }
        if approved && !phone_ids(&inner.devices).contains(&device_id) {
            return Err("not_found".to_string());
        }
        inner.router.clone()
    };
    manager.set_approved(&device_id, approved);
    if let Some(router) = router {
        router.approval_changed(&device_id, approved);
    }
    Ok(manager.status())
}

/// The phones the approval popup asks about, for a popup that just loaded.
#[tauri::command]
#[specta::specta]
pub fn account_pending_approvals() -> Vec<remote::AccountPhone> {
    approval::asking()
}

/// The approval popup's Allow or Don't allow.
#[tauri::command]
#[specta::specta]
pub async fn account_answer_approval(app: AppHandle, device_id: String, allow: bool) {
    answer_from_popup(&app, vec![device_id], allow).await;
}

/// An answer given on the popup (a button, Esc, closing it): the cards go,
/// activation goes back to the user's app whatever the answer, so the next
/// paste lands there, and then the requests waiting on them hear it.
async fn answer_from_popup(app: &AppHandle, phones: Vec<String>, allow: bool) {
    for phone in &phones {
        approval::close(app, phone);
    }
    approval::hand_back_focus(app).await;
    for phone in &phones {
        answer_approval(app, phone, allow);
    }
}

/// The approval popup measured its content; the window takes that height.
#[tauri::command]
#[specta::specta]
pub fn fit_account_approval(app: AppHandle, height: f64) {
    approval::fit(&app, height);
}

#[tauri::command]
#[specta::specta]
pub fn set_remote_input_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.remote_input_enabled = enabled;
    write_settings(&app, settings);
    manager(&app).emit();
    Ok(())
}

/// Settings → Account → Let phones choose windows.
#[tauri::command]
#[specta::specta]
pub fn set_remote_windows_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.remote_windows_enabled = enabled;
    write_settings(&app, settings);
    Ok(())
}

/// Settings → Account → Sync API keys. Turning it on syncs at once; turning
/// it off stops syncing and leaves every key on this computer.
#[tauri::command]
#[specta::specta]
pub fn set_account_sync_secrets(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.account_sync_secrets = enabled;
    write_settings(&app, settings);
    if enabled {
        secrets_sync::sync_now();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_is_the_discovered_one_or_the_development_default() {
        let found = "https://sync.example.com";
        let lan = "http://192.168.101.69:8787";
        for release in [false, true] {
            assert_eq!(resolve_server(found, release).as_deref(), Some(found));
            assert_eq!(resolve_server(lan, release).as_deref(), Some(lan));
        }
        assert_eq!(
            resolve_server("", false).as_deref(),
            Some(DEFAULT_ACCOUNT_SERVER_URL)
        );
        // A release build with nothing found has no server, and never uses a
        // loopback one a development build saved.
        assert_eq!(resolve_server("", true), None);
        assert_eq!(resolve_server("http://localhost:8787", true), None);
        assert_eq!(
            resolve_server("http://localhost:8787", false).as_deref(),
            Some("http://localhost:8787")
        );
    }

    #[test]
    fn a_session_follows_discovery_to_a_different_server() {
        let (old, new) = ("https://a.example.com", "http://192.168.1.5:8787");
        assert!(moves(old, new, true));
        assert!(moves("", new, true));
        assert!(moves("", new, false), "away from the development default");
        assert!(!moves(old, old, true));
        assert!(!moves(new, new, false));
    }

    #[test]
    fn the_server_may_be_http_or_https() {
        assert!(server_allowed("http://192.168.101.69:8787"));
        assert!(server_allowed("https://sync.example.com"));
        assert_eq!(server_allowed("http://localhost:8787"), !RELEASE);
        for bad in [
            "",
            "sync.example.com",
            "ftp://sync.example.com",
            "https://user:pass@sync.example.com",
        ] {
            assert!(!server_allowed(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_session_round_trips_through_the_keychain_format() {
        let session = Session {
            token: "zv1_x".into(),
            device_id: "d".into(),
            user: AccountUser {
                id: "u".into(),
                email: "a@b.c".into(),
                name: None,
                has_password: true,
                google_linked: false,
            },
        };
        let json = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.token, "zv1_x");
        assert_eq!(back.user, session.user);
    }

    #[test]
    fn only_phones_keep_an_approval() {
        let device = |id: &str, kind: &str| AccountDevice {
            id: id.into(),
            name: id.into(),
            platform: "linux".into(),
            kind: kind.into(),
            online: true,
            last_seen_at: 0,
            current: false,
            capabilities: Vec::new(),
            approved: false,
        };
        let ids = phone_ids(&[
            device("pc", "desktop"),
            device("p1", "mobile"),
            device("p2", "mobile"),
        ]);
        assert_eq!(ids, HashSet::from(["p1".to_string(), "p2".to_string()]));
    }

    #[test]
    fn connection_states_are_lowercase_on_the_wire() {
        assert_eq!(
            serde_json::to_value(AccountConnection::Connecting).unwrap(),
            "connecting"
        );
    }
}
