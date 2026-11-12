use crate::{api::ApiResult, app::manual_saves::ManualSaves, domain::GameId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualSavePreferences {
    pub shortcut: String,
    #[serde(skip_deserializing, default)]
    pub error: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualSaveStatus {
    pub sequence: u64,
    pub success: bool,
    pub message: String,
}
#[tauri::command]
pub fn get_manual_save_preferences(state: tauri::State<'_, ManualSaves>) -> ManualSavePreferences {
    state.preferences()
}
#[tauri::command]
pub fn get_manual_save_status(state: tauri::State<'_, ManualSaves>) -> ManualSaveStatus {
    state.status()
}
#[tauri::command]
pub async fn save_manual_save_shortcut(app: tauri::AppHandle, shortcut: String) -> ApiResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<ManualSaves>().save(shortcut)
    })
    .await
    .map_err(|e| crate::api::ApiError::Internal(e.to_string()))?
}
#[tauri::command]
pub async fn create_manual_save(app: tauri::AppHandle, game_id: Option<GameId>) -> ApiResult<()> {
    crate::app::manual_saves::capture(app, game_id).await
}

#[tauri::command]
pub fn set_shortcut_recording(state: tauri::State<'_, ManualSaves>, recording: bool) {
    state.set_shortcut_recording(recording);
}
