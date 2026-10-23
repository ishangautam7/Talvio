//! `#[ignore]`d checks of the router against the model this machine's own
//! settings would use (the same fallback to the assistant's provider), so a
//! prompt or tool change can be tried on the real model before a release. Only
//! the plan is asked for; nothing is opened, pressed or played.
//!
//! ```text
//! cargo test --lib command_mode::live_tests -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use super::context::CommandContext;
use super::llm;
use super::registry::Platform;
use super::router::{self, Followup};
use crate::settings::AppSettings;

/// The settings the installed app would load, with its keys from the keychain.
pub(crate) fn installed_settings() -> AppSettings {
    let dir = std::env::var("APPDATA").expect("APPDATA");
    let path = std::path::Path::new(&dir).join("com.talvio.desktop/settings_store.json");
    let text = std::fs::read_to_string(&path).expect("the installed app's settings");
    let text = text.trim_start_matches('\u{feff}');
    let store: serde_json::Value = serde_json::from_str(text).expect("settings JSON");
    let mut settings: AppSettings =
        serde_json::from_value(store["settings"].clone()).expect("settings shape");
    crate::settings::hydrate_secrets(&mut settings);
    settings
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the model this machine's settings name"]
async fn command_mode_live_routes_everyday_commands() {
    let settings = installed_settings();
    let llm = llm::router_for(&settings).expect("a usable router model");
    println!("router: {}", llm.label());
    let context = CommandContext::capture();
    for (command, tool) in [
        ("open youtube", None),
        ("play believer by imagine dragons", Some("play_media")),
        ("turn the volume down", Some("set_volume")),
        ("pause the music", Some("media_control")),
        ("open gmail", None),
    ] {
        let started = Instant::now();
        let plan = router::route(
            llm.as_ref(),
            Platform::current(),
            command,
            &context,
            &Followup::default(),
            tokio::time::Instant::now() + Duration::from_secs(40),
        )
        .await
        .unwrap_or_else(|e| panic!("{command}: {e:?}"));
        let calls: Vec<String> = plan
            .calls
            .iter()
            .map(|call| format!("{} {}", call.name, call.args))
            .collect();
        println!("{command:?} -> {calls:?} in {:?}", started.elapsed());
        if let Some(tool) = tool {
            assert!(
                plan.calls.iter().any(|call| call.name == tool),
                "{command}: expected {tool}, got {calls:?}"
            );
        }
    }
}
