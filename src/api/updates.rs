use crate::{
    api::ApiResult,
    services::updates::{AppPreferences, Preferences, UpdateStatus, Updates},
};

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
