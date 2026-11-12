//! Capabilities of this build, not repository connection or authentication status.
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppFeatures {
    pub sync: bool,
    pub profile_switch: bool,
    pub save_history: bool,
    pub conflicts: bool,
    pub persistent_games: bool,
    pub persistent_profiles: bool,
}

/// Enable a capability only when its complete user scenario is wired up.
#[tauri::command]
pub fn get_app_features(trace_id: Option<String>) -> AppFeatures {
    crate::adapters::diagnostics::request_sync(trace_id, "get_app_features", || AppFeatures {
        sync: true,
        profile_switch: true,
        save_history: true,
        conflicts: true,
        persistent_games: true,
        persistent_profiles: true,
    })
}
