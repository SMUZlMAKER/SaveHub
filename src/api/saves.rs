//! Операции с версиями файлов сохранений.
//!
//! Эти команды являются IPC-контрактом приложения. Работа с файловой системой,
//! git и снапшотами должна быть вынесена в SaveService и адаптеры.

use serde::{Deserialize, Serialize};

use super::{ApiError, ApiResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDto {
    pub id: String,
    pub game_id: String,
    pub profile_id: String,
    pub created_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveChoice {
    pub save_id: String,
    /// При true перед переключением создается снапшот текущего состояния.
    #[serde(default)]
    pub create_snapshot: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConflictDto {
    pub id: String,
    pub game_id: String,
    pub local: SaveDto,
    pub remote: SaveDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveConflictResolution {
    KeepLocal,
    KeepRemote,
    KeepBothLocalFirst,
    KeepBothRemoteFirst,
}

/// GET /saves/{game}
#[tauri::command]
pub fn list_saves(game: String) -> ApiResult<Vec<SaveDto>> {
    let _ = game;
    Err(ApiError::NotImplemented("list_saves"))
}

/// POST /saves/{game}/choose
#[tauri::command]
pub async fn choose_save(game: String, choice: SaveChoice) -> ApiResult<()> {
    let _ = (game, choice);
    Err(ApiError::NotImplemented("choose_save"))
}

/// GET /saves/{game}/conflict
#[tauri::command]
pub fn get_save_conflict(game: String) -> ApiResult<Option<SaveConflictDto>> {
    let _ = game;
    Err(ApiError::NotImplemented("get_save_conflict"))
}

/// POST /saves/{game}/conflict
#[tauri::command]
pub async fn resolve_save_conflict(
    game: String,
    conflict_id: String,
    resolution: SaveConflictResolution,
) -> ApiResult<()> {
    let _ = (game, conflict_id, resolution);
    Err(ApiError::NotImplemented("resolve_save_conflict"))
}
