//! Операции с версиями файлов сохранений.
//!
//! Эти команды являются IPC-контрактом приложения. Работа с файловой системой,
//! git и сохранениями должна быть вынесена в SaveService и адаптеры.

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
    /// При true перед переключением создается сохранение текущего состояния.
    #[serde(default, alias = "createSnapshot")]
    pub create_save: bool,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveHistoryEntryDto {
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    pub profile_name: String,
    pub kind: SaveKind,
    /// Unix-время в миллисекундах.
    pub created_at: u64,
    pub size_bytes: u64,
}

/// Причина создания сохранения. Подпись для пользователя выбирает фронт.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveKind {
    /// Перед переключением профиля.
    BeforeSwitch,
    /// Автоматически после игровой сессии.
    AfterSession,
    /// Вторая версия, сохраненная при разрешении конфликта.
    Conflict,
    /// Текущее состояние перед откатом.
    BeforeRestore,
}

/// Сохранения, новые сверху. `game_id = None` — по всем играм.
/// TODO(backend): SaveService::list.
#[tauri::command]
pub fn list_save_history(game_id: Option<String>) -> ApiResult<Vec<SaveHistoryEntryDto>> {
    let _ = game_id;
    Err(ApiError::NotImplemented("list_save_history"))
}

/// Восстановить выбранное сохранение игры. Перед восстановлением создать сохранение `BeforeRestore`.
/// TODO(backend): сценарий отката; если игра запущена — `ApiError::Rejected`. После — `SAVES_CHANGED`.
#[tauri::command]
pub async fn restore_save(save_id: String) -> ApiResult<()> {
    let _ = save_id;
    Err(ApiError::NotImplemented("restore_save"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_choice_accepts_current_and_legacy_field_names() {
        for field in ["createSave", "createSnapshot"] {
            let choice: SaveChoice = serde_json::from_value(serde_json::json!({
                "saveId": "save-1",
                (field): true,
            }))
            .unwrap();
            assert_eq!(choice.save_id, "save-1");
            assert!(choice.create_save);
        }
        let choice: SaveChoice = serde_json::from_value(serde_json::json!({
            "saveId": "save-1",
        }))
        .unwrap();
        assert!(!choice.create_save);
    }
}
