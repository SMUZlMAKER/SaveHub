use crate::services::game::GameService;
use std::{sync::Arc, time::Duration};
use tauri::Emitter;

/// Startup refresh and retries also publish commits left by a previous offline run.
pub fn spawn(app: tauri::AppHandle, service: Arc<GameService>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let service = service.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || service.sync_catalog()).await;
            let _ = app.emit(crate::api::events::GAMES_CHANGED, ());
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}
