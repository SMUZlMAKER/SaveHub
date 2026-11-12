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
    trace_id: Option<String>,
) -> ApiResult<ProfileDto> {
    crate::adapters::diagnostics::request(trace_id, "current_profile", async move {
        let service = service.inner().clone();
        let saves = saves.inner().clone();
        let operations = operations.inner().clone();
        crate::app::diagnostics::spawn_blocking(move || {
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
    })
    .await
}

#[tauri::command]
pub async fn list_profiles(
    service: State<'_, Arc<ProfileService>>,
    saves: State<'_, Arc<crate::services::save::SaveService>>,
    operations: State<'_, Arc<std::sync::Mutex<()>>>,
    trace_id: Option<String>,
) -> ApiResult<Vec<ProfileDto>> {
    crate::adapters::diagnostics::request(trace_id, "list_profiles", async move {
        let service = service.inner().clone();
        let saves = saves.inner().clone();
        let operations = operations.inner().clone();
        crate::app::diagnostics::spawn_blocking(move || {
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
    })
    .await
}

/// Переключить сохранения всех игр на профиль. Операция долгая, поэтому `async` — не блокирует окно.
/// После успеха — событие `events::PROFILE_CHANGED` и обновить меню трея.
#[tauri::command]
pub async fn switch_profile(
    app: tauri::AppHandle,
    profile_id: ProfileId,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request(trace_id, "switch_profile", async move {
        crate::app::diagnostics::spawn_blocking(move || {
            crate::app::actions::switch_profile(&app, profile_id)
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
    })
    .await
}

#[tauri::command]
pub async fn create_profile(
    app: tauri::AppHandle,
    name: String,
    service: State<'_, Arc<ProfileService>>,
    trace_id: Option<String>,
) -> ApiResult<ProfileDto> {
    crate::adapters::diagnostics::request(trace_id, "create_profile", async move {
        let service = service.inner().clone();
        let profile = crate::app::diagnostics::spawn_blocking(move || service.create(&name))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))??;
        app.emit(PROFILE_CHANGED, ())
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        Ok(to_dto(&profile, false))
    })
    .await
}

#[tauri::command]
pub async fn delete_profile(
    app: tauri::AppHandle,
    profile_id: ProfileId,
    confirmed: bool,
    service: State<'_, Arc<ProfileService>>,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request(trace_id, "delete_profile", async move {
        if !confirmed {
            return Err(ApiError::Rejected(
                "Подтвердите удаление профиля и его ветки сохранений".into(),
            ));
        }
        let service = service.inner().clone();
        crate::app::diagnostics::spawn_blocking(move || {
            crate::adapters::diagnostics::event(
                "delete_profile",
                &format!("start profile={profile_id}"),
            );
            let result = service.delete(&profile_id);
            crate::adapters::diagnostics::event(
                "delete_profile",
                &format!("finish profile={profile_id} success={}", result.is_ok()),
            );
            result
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))??;
        app.emit(PROFILE_CHANGED, ())
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        Ok(())
    })
    .await
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
