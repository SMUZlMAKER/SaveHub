//! Экран «Профили» и переключение профиля (оно же есть в трее).

use serde::Serialize;

use super::{ApiError, ApiResult};

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

/// TODO(backend): ProfileService::list.
#[tauri::command]
pub fn list_profiles() -> ApiResult<Vec<ProfileDto>> {
    Ok(vec![
        ProfileDto {
            id: "main".into(),
            name: "Основной".into(),
            is_active: true,
            games_count: 3,
            last_played_game: Some("Hollow Knight".into()),
        },
        ProfileDto {
            id: "guest".into(),
            name: "Гость".into(),
            is_active: false,
            games_count: 0,
            last_played_game: None,
        },
    ])
}

/// Переключить сохранения всех игр на профиль. Операция долгая, поэтому `async` — не блокирует окно.
/// TODO(backend): workflows::SwitchProfile. Если запущена игра — `ApiError::Rejected`.
/// После успеха — событие `events::PROFILE_CHANGED` и обновить меню трея.
#[tauri::command]
pub async fn switch_profile(profile_id: String) -> ApiResult<()> {
    let _ = profile_id;
    Err(ApiError::NotImplemented("switch_profile"))
}

/// TODO(backend): ProfileService::create.
#[tauri::command]
pub fn create_profile(name: String) -> ApiResult<ProfileDto> {
    let _ = name;
    Err(ApiError::NotImplemented("create_profile"))
}

/// TODO(backend): ProfileService::delete. Активный профиль удалять нельзя — `ApiError::Rejected`.
#[tauri::command]
pub fn delete_profile(profile_id: String) -> ApiResult<()> {
    let _ = profile_id;
    Err(ApiError::NotImplemented("delete_profile"))
}
