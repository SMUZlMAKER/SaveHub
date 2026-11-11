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
    pub is_active: bool,
    /// Для скольких игр у профиля есть сохранения.
    pub games_count: u32,
    pub last_played_game: Option<String>,
}

/// GET /me
#[tauri::command]
pub fn current_profile(service: State<'_, Arc<ProfileService>>) -> ApiResult<ProfileDto> {
    service
        .active()?
        .as_ref()
        .map(|profile| to_dto(profile, true))
        .ok_or_else(|| ApiError::Internal("Активный профиль не настроен".into()))
}

#[tauri::command]
pub fn list_profiles(service: State<'_, Arc<ProfileService>>) -> ApiResult<Vec<ProfileDto>> {
    let active_id = service.active()?.map(|profile| profile.id);
    Ok(service
        .list()?
        .into_iter()
        .map(|profile| to_dto(&profile, active_id.as_ref() == Some(&profile.id)))
        .collect())
}

/// Переключить сохранения всех игр на профиль. Операция долгая, поэтому `async` — не блокирует окно.
/// После успеха — событие `events::PROFILE_CHANGED` и обновить меню трея.
// TODO(работа с git): workflows::SwitchProfile. Если запущена игра — `ApiError::Rejected`.
#[tauri::command]
pub async fn switch_profile(profile_id: ProfileId) -> ApiResult<()> {
    let _ = profile_id;
    Err(ApiError::NotImplemented("switch_profile"))
}


#[tauri::command]
pub fn create_profile(
    app: tauri::AppHandle,
    name: String,
    service: State<'_, Arc<ProfileService>>,
) -> ApiResult<ProfileDto> {
    let profile = service.create(&name)?;
    app.emit(PROFILE_CHANGED, ())
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(to_dto(&profile, false))
}

#[tauri::command]
pub fn delete_profile(
    app: tauri::AppHandle,
    profile_id: ProfileId,
    service: State<'_, Arc<ProfileService>>,
) -> ApiResult<()> {
    service.delete(&profile_id)?;
    app.emit(PROFILE_CHANGED, ())
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(())
}

fn to_dto(profile: &Profile, is_active: bool) -> ProfileDto {
    ProfileDto {
        id: profile.id.to_string(),
        name: profile.name.clone(),
        is_active,
        games_count: 0,
        last_played_game: profile.last_played_game.as_ref().map(ToString::to_string),
    }
}
