//! Синхронизация с GitHub и конфликты сохранений между ПК.

use crate::{
    domain::Catalog,
    services::{process::ProcessService, repo::RepoService},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

use super::{ApiError, ApiResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusDto {
    pub state: SyncState,
    /// Unix-время последнего успешного fetch/push без ошибок и ожидающих отправки коммитов.
    /// В миллисекундах; не означает применение удалённых сохранений.
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

#[tauri::command]
pub fn get_sync_status(
    runtime: tauri::State<'_, std::sync::Arc<crate::domain::RuntimeState>>,
) -> ApiResult<SyncStatusDto> {
    let status = runtime.sync_status();
    let state = match status.state {
        crate::domain::SyncState::NotConfigured => SyncState::NotConfigured,
        crate::domain::SyncState::Idle => SyncState::Idle,
        crate::domain::SyncState::Syncing => SyncState::Syncing,
        crate::domain::SyncState::Error => SyncState::Error,
    };

    Ok(SyncStatusDto {
        state,
        last_sync_at: status.last_sync_at,
        error: status.error,
    })
}

#[tauri::command]
pub fn sync_now(app: tauri::AppHandle) -> ApiResult<()> {
    crate::app::actions::sync_now(&app)
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
    /// Оставить локальную версию; обе версии сохраняются адаптером Git перед разрешением.
    KeepBothLocalFirst,
    /// Сохранить обе версии; основной становится версия из репозитория.
    KeepBothRemoteFirst,
}

impl From<ConflictResolution> for crate::domain::ConflictResolution {
    fn from(value: ConflictResolution) -> Self {
        match value {
            ConflictResolution::KeepLocal => Self::KeepLocal,
            ConflictResolution::KeepRemote => Self::KeepRemote,
            ConflictResolution::KeepBothLocalFirst => Self::KeepBothLocalFirst,
            ConflictResolution::KeepBothRemoteFirst => Self::KeepBothRemoteFirst,
        }
    }
}

#[tauri::command]
pub async fn list_conflicts(
    repo: State<'_, Arc<RepoService>>,
    catalog: State<'_, Arc<Catalog>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<Vec<ConflictDto>> {
    let repo = repo.inner().clone();
    let catalog = catalog.inner().clone();
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        let games = catalog.games();
        Ok(repo
            .list_conflicts()?
            .into_iter()
            .map(|c| ConflictDto {
                id: c.id,
                game_id: c.game_id.to_string(),
                game_name: games
                    .get(&c.game_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| c.game_id.to_string()),
                local: SaveVersionDto {
                    machine_name: c.local.machine_id,
                    modified_at: c.local.modified_at,
                    size_bytes: c.local.size_bytes,
                },
                remote: SaveVersionDto {
                    machine_name: c.remote.machine_id,
                    modified_at: c.remote.modified_at,
                    size_bytes: c.remote.size_bytes,
                },
            })
            .collect())
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn resolve_conflict(
    app: tauri::AppHandle,
    conflict_id: String,
    resolution: ConflictResolution,
    repo: State<'_, Arc<RepoService>>,
    processes: State<'_, Arc<ProcessService>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<()> {
    resolve(
        app,
        None,
        conflict_id,
        resolution.into(),
        repo.inner().clone(),
        processes.inner().clone(),
        operations.inner().clone(),
    )
    .await
}

pub(super) async fn resolve(
    app: tauri::AppHandle,
    game: Option<crate::domain::GameId>,
    id: String,
    resolution: crate::domain::ConflictResolution,
    repo: Arc<RepoService>,
    processes: Arc<ProcessService>,
    operations: Arc<Mutex<()>>,
) -> ApiResult<()> {
    let result = tauri::async_runtime::spawn_blocking(move || -> ApiResult<()> {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        processes.refresh_running_games()?;
        if let Some(game) = game {
            if !repo
                .list_conflicts()?
                .iter()
                .any(|c| c.id == id && c.game_id == game)
            {
                return Err(ApiError::Rejected("Конфликт не принадлежит игре".into()));
            }
        }
        repo.resolve_conflict(&id, resolution)?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    for event in [
        super::events::CONFLICTS_CHANGED,
        super::events::SAVES_CHANGED,
    ] {
        let _ = app.emit(event, ());
    }
    result
}
