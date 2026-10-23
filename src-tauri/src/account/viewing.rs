//! "Phone is viewing" on this computer, for as long as a live view runs, with
//! Stop (`remote::Router::stop_watching`): a card in the phone-approval window
//! (`approval.rs`), announced to the main window and in the log too.

use log::info;
use tauri::{AppHandle, Emitter};

use super::remote::Viewing;

/// The event the main window hears; `null` when the view ended.
pub const VIEWING_EVENT: &str = "account-viewing";

pub fn show(app: &AppHandle, notice: Option<Viewing>) {
    match &notice {
        Some(viewing) => info!("A phone ({}) is viewing a window", viewing.phone),
        None => info!("The phone stopped viewing"),
    }
    let card = notice.map(|viewing| super::approval::ViewingCard {
        phone: viewing.phone,
        title: viewing.title,
    });
    let _ = app.emit(VIEWING_EVENT, &card);
    super::approval::set_viewing(app, card);
}
