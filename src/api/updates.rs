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
    /// Includes restartRequired after installation until a successful restart.
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
    trace_id: Option<String>,
) -> ApiResult<AppPreferences> {
    crate::adapters::diagnostics::request(trace_id, "get_app_preferences", async move {
        state.preferences(&app)
    })
    .await
}
#[tauri::command]
pub async fn save_app_preferences(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
    preferences: Preferences,
    launch_at_login: bool,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request(trace_id, "save_app_preferences", async move {
        state.save(&app, preferences, launch_at_login)
    })
    .await
}
#[tauri::command]
pub fn get_update_status(
    state: tauri::State<'_, Updates>,
    trace_id: Option<String>,
) -> UpdateStatus {
    crate::adapters::diagnostics::request_sync(trace_id, "get_update_status", || state.status())
}
#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
    trace_id: Option<String>,
) -> ApiResult<UpdateStatus> {
    crate::adapters::diagnostics::request(trace_id, "check_for_updates", async move {
        state.check(&app).await
    })
    .await
}
#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Updates>,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request(trace_id, "install_update", async move {
        state.install(&app).await
    })
    .await
}
