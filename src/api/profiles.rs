//! Экран «Профили» и переключение профиля (оно же есть в трее).

use serde::Serialize;
use std::sync::Arc;
use tauri::{Emitter, State};

use super::{ApiError, ApiResult};
use crate::api::events::PROFILE_CHANGED;
use crate::domain::{Profile, ProfileId};
use crate::services::profile::ProfileService;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub branch: String,
    pub is_active: bool,
    /// Для скольких игр у профиля есть сохранения.
    pub games_count: u32,
    pub last_played_game: Option<String>,
}

/// GET /me
#[tauri::command]
pub async fn current_profile(
    service: State<'_, Arc<ProfileService>>,
    saves: State<'_, Arc<crate::services::save::SaveService>>,
    operations: State<'_, Arc<std::sync::Mutex<()>>>,
) -> ApiResult<ProfileDto> {
    let service = service.inner().clone();
    let saves = saves.inner().clone();
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        let profile = service
            .active()?
            .ok_or_else(|| ApiError::Rejected("Активный профиль не настроен".into()))?;
        let mut dto = to_dto(&profile, true);
        dto.games_count = saves.games_count(&profile.id)?;
        Ok(dto)
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn list_profiles(
    service: State<'_, Arc<ProfileService>>,
    saves: State<'_, Arc<crate::services::save::SaveService>>,
    operations: State<'_, Arc<std::sync::Mutex<()>>>,
) -> ApiResult<Vec<ProfileDto>> {
    let service = service.inner().clone();
    let saves = saves.inner().clone();
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operations
            .lock()
            .map_err(|_| ApiError::Internal("Блокировка операций повреждена".into()))?;
        let active_id = service.active()?.map(|p| p.id);
        service
            .list()?
            .into_iter()
            .map(|profile| {
                let mut dto = to_dto(&profile, active_id == Some(profile.id));
                dto.games_count = saves.games_count(&profile.id)?;
                Ok(dto)
            })
            .collect()
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
}

/// Переключить сохранения всех игр на профиль. Операция долгая, поэтому `async` — не блокирует окно.
/// После успеха — событие `events::PROFILE_CHANGED` и обновить меню трея.
#[tauri::command]
pub async fn switch_profile(app: tauri::AppHandle, profile_id: ProfileId) -> ApiResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::app::actions::switch_profile(&app, profile_id)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
}

#[tauri::command]
pub async fn create_profile(
    app: tauri::AppHandle,
    name: String,
    service: State<'_, Arc<ProfileService>>,
) -> ApiResult<ProfileDto> {
    let service = service.inner().clone();
    let profile = tauri::async_runtime::spawn_blocking(move || service.create(&name))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))??;
    app.emit(PROFILE_CHANGED, ())
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(to_dto(&profile, false))
}

#[tauri::command]
pub async fn delete_profile(
    app: tauri::AppHandle,
    profile_id: ProfileId,
    confirmed: bool,
    service: State<'_, Arc<ProfileService>>,
) -> ApiResult<()> {
    if !confirmed {
        return Err(ApiError::Rejected(
            "Подтвердите удаление профиля и его ветки сохранений".into(),
        ));
    }
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.delete(&profile_id))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))??;
    app.emit(PROFILE_CHANGED, ())
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(())
}

fn to_dto(profile: &Profile, is_active: bool) -> ProfileDto {
    ProfileDto {
        id: profile.id.to_string(),
        name: profile.name.clone(),
        branch: profile.branch.clone(),
        is_active,
        games_count: 0,
        last_played_game: profile.last_played_game.as_ref().map(ToString::to_string),
    }
}
