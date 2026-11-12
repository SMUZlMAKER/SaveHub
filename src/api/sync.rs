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
pub async fn sync_now(app: tauri::AppHandle) -> ApiResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::app::actions::sync_now(&app))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDto {
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    pub profile_id: String,
    pub profile_name: String,
    pub profile_branch: String,
    pub file_path: String,
    /// Версия на этом ПК.
    pub local: SaveVersionDto,
    /// Версия, полученная из origin.
    pub remote: SaveVersionDto,
    /// Коммит локальной стороны конфликта.
    pub local_commit: ConflictCommitDto,
    /// Коммит стороны, полученной из origin.
    pub remote_commit: ConflictCommitDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictCommitDto {
    pub id: String,
    pub committed_at: u64,
    pub summary: String,
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
}

impl From<ConflictResolution> for crate::domain::ConflictResolution {
    fn from(value: ConflictResolution) -> Self {
        match value {
            ConflictResolution::KeepLocal => Self::KeepLocal,
            ConflictResolution::KeepRemote => Self::KeepRemote,
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
        let profiles = catalog.profiles();
        Ok(repo
            .list_conflicts()?
            .into_iter()
            .map(|c| {
                let profile = profiles.get(&c.profile_id);
                let game_root = std::path::Path::new("games").join(c.game_id.to_string());
                let file_path = c
                    .file_path
                    .strip_prefix(&game_root)
                    .unwrap_or(&c.file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                ConflictDto {
                    id: c.id,
                    game_id: c.game_id.to_string(),
                    game_name: games
                        .get(&c.game_id)
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| c.game_id.to_string()),
                    profile_id: c.profile_id.to_string(),
                    profile_name: profile
                        .map(|profile| profile.name.clone())
                        .unwrap_or_else(|| c.profile_id.to_string()),
                    profile_branch: profile
                        .map(|profile| profile.branch.clone())
                        .unwrap_or_else(|| "неизвестна".into()),
                    file_path,
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
                    local_commit: ConflictCommitDto {
                        id: c.local_commit.id,
                        committed_at: c.local_commit.committed_at,
                        summary: c.local_commit.summary,
                    },
                    remote_commit: ConflictCommitDto {
                        id: c.remote_commit.id,
                        committed_at: c.remote_commit.committed_at,
                        summary: c.remote_commit.summary,
                    },
                }
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
        let conflicts = repo.list_conflicts()?;
        let conflict = conflicts
            .iter()
            .find(|conflict| conflict.id == id && game.is_none_or(|game| conflict.game_id == game));
        if conflict.is_none() {
            return Err(ApiError::Rejected(
                "Конфликт не найден или не принадлежит игре".into(),
            ));
        }
        repo.resolve_conflict(&id, resolution)?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    for event in [
        super::events::SYNC_STATUS_CHANGED,
        super::events::CONFLICTS_CHANGED,
        super::events::SAVES_CHANGED,
    ] {
        let _ = app.emit(event, ());
    }
    result
}
