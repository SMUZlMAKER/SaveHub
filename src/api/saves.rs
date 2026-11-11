//! Операции с версиями файлов сохранений.
//!
//! Эти команды являются IPC-контрактом приложения. Работа с файловой системой,
//! git и сохранениями должна быть вынесена в SaveService и адаптеры.

use crate::{
    domain::{Catalog, GameId},
    services::{process::ProcessService, save::SaveService},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

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
pub async fn list_saves(
    game: GameId,
    service: State<'_, Arc<SaveService>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<Vec<SaveDto>> {
    let service = service.inner().clone();
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        Ok(service
            .list(Some(game), None)?
            .into_iter()
            .map(save_dto)
            .collect())
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
}

/// POST /saves/{game}/choose
#[tauri::command]
pub async fn choose_save(
    app: tauri::AppHandle,
    game: GameId,
    choice: SaveChoice,
    service: State<'_, Arc<SaveService>>,
    processes: State<'_, Arc<ProcessService>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<()> {
    restore(
        app,
        Some(game),
        choice.save_id,
        service.inner().clone(),
        processes.inner().clone(),
        operations.inner().clone(),
    )
    .await
}

/// GET /saves/{game}/conflict
#[tauri::command]
pub async fn get_save_conflict(
    game: GameId,
    repo: State<'_, Arc<crate::services::repo::RepoService>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<Option<SaveConflictDto>> {
    let repo = repo.inner().clone();
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        Ok(repo
            .list_conflicts()?
            .into_iter()
            .find(|c| c.game_id == game)
            .map(|c| {
                let version = |v: crate::domain::SaveVersion| SaveDto {
                    id: c.id.clone(),
                    game_id: game.to_string(),
                    profile_id: c.profile_id.to_string(),
                    created_at: v.modified_at,
                    size_bytes: v.size_bytes,
                };
                SaveConflictDto {
                    local: version(c.local),
                    remote: version(c.remote),
                    id: c.id,
                    game_id: game.to_string(),
                }
            }))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn resolve_save_conflict(
    app: tauri::AppHandle,
    game: GameId,
    conflict_id: String,
    resolution: SaveConflictResolution,
    repo: State<'_, Arc<crate::services::repo::RepoService>>,
    processes: State<'_, Arc<ProcessService>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<()> {
    let resolution = match resolution {
        SaveConflictResolution::KeepLocal => crate::domain::ConflictResolution::KeepLocal,
        SaveConflictResolution::KeepRemote => crate::domain::ConflictResolution::KeepRemote,
        SaveConflictResolution::KeepBothLocalFirst => {
            crate::domain::ConflictResolution::KeepBothLocalFirst
        }
        SaveConflictResolution::KeepBothRemoteFirst => {
            crate::domain::ConflictResolution::KeepBothRemoteFirst
        }
    };
    super::sync::resolve(
        app,
        Some(game),
        conflict_id,
        resolution,
        repo.inner().clone(),
        processes.inner().clone(),
        operations.inner().clone(),
    )
    .await
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
    FileChanged,
}

fn save_dto(save: crate::domain::Save) -> SaveDto {
    SaveDto {
        id: save.id,
        game_id: save.game_id.to_string(),
        profile_id: save.profile_id.to_string(),
        created_at: save.created_at,
        size_bytes: save.size_bytes,
    }
}

#[tauri::command]
pub async fn list_save_history(
    game_id: Option<GameId>,
    service: State<'_, Arc<SaveService>>,
    catalog: State<'_, Arc<Catalog>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<Vec<SaveHistoryEntryDto>> {
    let service = service.inner().clone();
    let catalog = catalog.inner().clone();
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        let games = catalog.games();
        let profiles = catalog.profiles();
        Ok(service
            .list(game_id, None)?
            .into_iter()
            .map(|save| SaveHistoryEntryDto {
                id: save.id,
                game_id: save.game_id.to_string(),
                game_name: games
                    .get(&save.game_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| save.game_id.to_string()),
                profile_name: profiles
                    .get(&save.profile_id)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| save.profile_id.to_string()),
                kind: match save.kind {
                    crate::domain::SaveKind::BeforeSwitch => SaveKind::BeforeSwitch,
                    crate::domain::SaveKind::AfterSession => SaveKind::AfterSession,
                    crate::domain::SaveKind::Conflict => SaveKind::Conflict,
                    crate::domain::SaveKind::BeforeRestore => SaveKind::BeforeRestore,
                    crate::domain::SaveKind::FileChanged => SaveKind::FileChanged,
                },
                created_at: save.created_at,
                size_bytes: save.size_bytes,
            })
            .collect())
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn restore_save(
    app: tauri::AppHandle,
    save_id: String,
    service: State<'_, Arc<SaveService>>,
    processes: State<'_, Arc<ProcessService>>,
    operations: State<'_, Arc<Mutex<()>>>,
) -> ApiResult<()> {
    restore(
        app,
        None,
        save_id,
        service.inner().clone(),
        processes.inner().clone(),
        operations.inner().clone(),
    )
    .await
}

async fn restore(
    app: tauri::AppHandle,
    game: Option<GameId>,
    id: String,
    service: Arc<SaveService>,
    processes: Arc<ProcessService>,
    operations: Arc<Mutex<()>>,
) -> ApiResult<()> {
    let result = tauri::async_runtime::spawn_blocking(move || -> ApiResult<()> {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        processes.refresh_running_games()?;
        if game.is_some_and(|game| {
            service
                .get_save(&id)
                .map(|s| s.game_id != game)
                .unwrap_or(true)
        }) {
            return Err(ApiError::Rejected("Сохранение не принадлежит игре".into()));
        }
        service.apply_save(&id)?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    let _ = app.emit(super::events::SAVES_CHANGED, ());
    result
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
