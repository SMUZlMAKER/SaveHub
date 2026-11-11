//! Thin IPC entrypoints for desktop shortcuts and pre-launch orchestration.
use crate::{
    api::{ApiError, ApiResult},
    app::launch::Launches,
    domain::{GameId, ProfileId},
    services::process::ProcessService,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{Manager, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfileDto {
    pub id: ProfileId,
    pub name: String,
    pub is_active: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchStatus {
    pub request_id: Option<uuid::Uuid>,
    pub game_id: Option<GameId>,
    pub game_name: Option<String>,
    pub phase: String,
    pub profiles: Vec<LaunchProfileDto>,
    pub active_profile_id: Option<ProfileId>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub monitor_error: Option<String>,
    pub pid: Option<u32>,
}
impl Default for LaunchStatus {
    fn default() -> Self {
        Self {
            request_id: None,
            game_id: None,
            game_name: None,
            phase: "idle".into(),
            profiles: vec![],
            active_profile_id: None,
            warnings: vec![],
            error: None,
            monitor_error: None,
            pid: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutDto {
    pub path: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameProcessDto {
    pub game_id: GameId,
    pub state: String,
    pub pid: Option<u32>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_launch_status(state: State<'_, Launches>) -> LaunchStatus {
    state.status()
}

#[tauri::command]
pub async fn create_game_shortcut(
    app: tauri::AppHandle,
    game_id: GameId,
    service: State<'_, Arc<ProcessService>>,
) -> ApiResult<ShortcutDto> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .join("launchers");
    let service = service.inner().clone();
    let path =
        tauri::async_runtime::spawn_blocking(move || service.create_shortcut(game_id, &directory))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(ShortcutDto {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn begin_game_launch(app: tauri::AppHandle, game_id: GameId) -> ApiResult<LaunchStatus> {
    crate::app::launch::begin(&app, game_id).await
}

#[tauri::command]
pub async fn confirm_game_launch(
    app: tauri::AppHandle,
    request_id: uuid::Uuid,
    profile_id: ProfileId,
) -> ApiResult<LaunchStatus> {
    crate::app::launch::confirm(&app, request_id, profile_id).await
}

#[tauri::command]
pub async fn cancel_game_launch(app: tauri::AppHandle, request_id: uuid::Uuid) -> ApiResult<()> {
    crate::app::launch::cancel(&app, Some(request_id))
}

#[tauri::command]
pub async fn dismiss_launch_dialog(app: tauri::AppHandle) -> ApiResult<()> {
    crate::app::launch::cancel(&app, None)?;
    if let Some(window) = app.get_webview_window("launch") {
        window
            .hide()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    }
    Ok(())
}
