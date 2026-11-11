//! Синхронизация с GitHub и конфликты сохранений между ПК.

use serde::{Deserialize, Serialize};

use super::{ApiError, ApiResult, stub};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusDto {
    pub state: SyncState,
    /// Unix-время последней успешной синхронизации в миллисекундах.
    pub last_sync_at: Option<u64>,
    /// Текст ошибки, если `state = Error`.
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    /// Репозиторий или токен не настроены.
    NotConfigured,
    Idle,
    Syncing,
    Error,
}

/// TODO(backend): состояние RepoWorker.
#[tauri::command]
pub fn get_sync_status() -> ApiResult<SyncStatusDto> {
    Ok(SyncStatusDto {
        state: SyncState::Idle,
        last_sync_at: Some(stub::minutes_ago(3)),
        error: None,
    })
}

/// TODO(backend): попросить RepoWorker синхронизироваться, не дожидаясь таймера.
/// Ход синхронизации фронт узнает из `events::SYNC_STATUS_CHANGED`.
#[tauri::command]
pub async fn sync_now() -> ApiResult<()> {
    Err(ApiError::NotImplemented("sync_now"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDto {
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    /// Версия на этом ПК.
    pub local: SaveVersionDto,
    /// Версия из репозитория (с другого ПК).
    pub remote: SaveVersionDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveVersionDto {
    pub machine_name: String,
    /// Unix-время в миллисекундах.
    pub modified_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    KeepLocal,
    KeepRemote,
    /// Оставить выбранную пользователем локальную версию, вторую сохранить отдельным сохранением `Conflict`.
    KeepBothLocalFirst,
    /// То же, но основной становится версия из репозитория.
    KeepBothRemoteFirst,
}

/// Нерешенные конфликты. TODO(backend): RepoWorker находит их при синхронизации, хранить в RuntimeState.
#[tauri::command]
pub fn list_conflicts() -> ApiResult<Vec<ConflictDto>> {
    Ok(vec![ConflictDto {
        id: "c1".into(),
        game_id: "hollow-knight".into(),
        game_name: "Hollow Knight".into(),
        local: SaveVersionDto {
            machine_name: "Этот ПК".into(),
            modified_at: stub::minutes_ago(40),
            size_bytes: 3 * 1024 * 1024,
        },
        remote: SaveVersionDto {
            machine_name: "Ноутбук".into(),
            modified_at: stub::minutes_ago(190),
            size_bytes: 2 * 1024 * 1024 + 300 * 1024,
        },
    }])
}

/// TODO(backend): сценарий разрешения конфликта; после — `CONFLICTS_CHANGED` и `SAVES_CHANGED`.
#[tauri::command]
pub async fn resolve_conflict(
    conflict_id: String,
    resolution: ConflictResolution,
) -> ApiResult<()> {
    let _ = (conflict_id, resolution);
    Err(ApiError::NotImplemented("resolve_conflict"))
}
