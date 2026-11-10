//! Экран «Игры»: какие игры отслеживаются и где лежат их сохранения.

use serde::Serialize;

use super::{ApiError, ApiResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDto {
    pub id: String,
    pub name: String,
    /// Папка с сохранениями; `None`, если не нашли автоматически.
    pub save_path: Option<String>,
    pub status: GameStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameStatus {
    /// Папка сохранений найдена.
    Found,
    /// Папку не нашли — пользователь должен указать ее сам.
    Missing,
    /// Игра сейчас запущена: переключать профиль и откатывать сохранения нельзя.
    Running,
}

/// TODO(backend): GameService (каталог игр) + RuntimeState (какие игры запущены).
#[tauri::command]
pub fn list_games() -> ApiResult<Vec<GameDto>> {
    let game = |id: &str, name: &str, path: Option<&str>, status| GameDto {
        id: id.into(),
        name: name.into(),
        save_path: path.map(Into::into),
        status,
    };
    Ok(vec![
        game("hollow-knight", "Hollow Knight", Some(r"%USERPROFILE%\AppData\LocalLow\Team Cherry\Hollow Knight"), GameStatus::Found),
        game("stardew-valley", "Stardew Valley", Some(r"%APPDATA%\StardewValley\Saves"), GameStatus::Found),
        game("celeste", "Celeste", None, GameStatus::Missing),
        game("terraria", "Terraria", Some(r"%USERPROFILE%\Documents\My Games\Terraria"), GameStatus::Running),
    ])
}

/// Добавить игру вручную. TODO(backend): GameService::add, проверить, что папка существует.
#[tauri::command]
pub fn add_game(name: String, save_path: String) -> ApiResult<GameDto> {
    let _ = (name, save_path);
    Err(ApiError::NotImplemented("add_game"))
}

/// Указать папку сохранений для игры со статусом `Missing`. TODO(backend): GameService::set_save_path.
#[tauri::command]
pub fn set_game_save_path(game_id: String, save_path: String) -> ApiResult<()> {
    let _ = (game_id, save_path);
    Err(ApiError::NotImplemented("set_game_save_path"))
}
