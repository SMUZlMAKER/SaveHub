use crate::{api::events, services::game::GameService};
use tauri::Emitter;

/// Failed fetches/validation do not reload the list. A failed push can still
/// follow an accepted remote update, so the result alone is insufficient.
pub fn notify_changes(app: &tauri::AppHandle, service: &GameService, previous_revision: u64) {
    if service.revision() != previous_revision {
        let _ = app.emit(events::GAMES_CHANGED, ());
    }
    notify_status(app);
}

pub fn notify_status(app: &tauri::AppHandle) {
    let _ = app.emit(events::CATALOG_STATUS_CHANGED, ());
}
