//! Команды и DTO экрана обновлений.
use crate::{api::ApiResult, app::updates::Updates};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub check_on_startup: bool,
    pub check_periodically: bool,
    pub check_before_game_launch: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            check_periodically: true,
            check_before_game_launch: false,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub configured: bool,
    pub phase: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub last_checked_at: Option<u64>,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub launch_at_login: bool,
    pub check_on_startup: bool,
    pub check_periodically: bool,
    pub check_before_game_launch: bool,
}
#[tauri::command]
pub async fn get_app_preferences(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
) -> ApiResult<AppPreferences> {
    state.preferences(&app)
}
#[tauri::command]
pub async fn save_app_preferences(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
    preferences: Preferences,
    launch_at_login: bool,
) -> ApiResult<()> {
    state.save(&app, preferences, launch_at_login)
}
#[tauri::command]
pub fn get_update_status(state: tauri::State<'_, Updates>) -> UpdateStatus {
    state.status()
}
#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
) -> ApiResult<UpdateStatus> {
    state.check(&app).await
}
#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
) -> ApiResult<()> {
    state.install(&app).await
}
