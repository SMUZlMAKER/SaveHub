//! Экран «Настройки»: репозиторий GitHub и поведение синхронизации.

use serde::{Deserialize, Serialize};

use super::{ApiError, ApiResult};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// Удаленный репозиторий для сохранений, например `https://github.com/user/saves`.
    pub repo_url: Option<String>,
    /// Локальный клон этого репозитория.
    pub local_repo_path: Option<String>,
    /// Сохранен ли токен GitHub. Сам токен фронту не отдаем.
    pub has_github_token: bool,
    pub sync_after_session: bool,
    pub sync_while_gaming: bool,
    /// Переключать профиль на того, кто последним запускал игру.
    pub auto_switch_profile: bool,
}

/// TODO(backend): хранилище Settings.
#[tauri::command]
pub fn get_settings() -> ApiResult<SettingsDto> {
    Ok(SettingsDto {
        repo_url: Some("https://github.com/example/savehub-saves".into()),
        local_repo_path: Some(r"%LOCALAPPDATA%\SaveHub\repo".into()),
        has_github_token: true,
        sync_after_session: true,
        sync_while_gaming: false,
        auto_switch_profile: true,
    })
}

/// Поле `has_github_token` при сохранении игнорируется: токен меняется через `set_github_token`.
/// TODO(backend): записать Settings на диск; если сменился репозиторий — переподключить RepoWorker.
#[tauri::command]
pub fn update_settings(settings: SettingsDto) -> ApiResult<()> {
    let _ = settings;
    Err(ApiError::NotImplemented("update_settings"))
}

/// TODO(backend): положить токен в системное хранилище (Windows Credential Manager), не в файл настроек.
#[tauri::command]
pub fn set_github_token(token: String) -> ApiResult<()> {
    let _ = token;
    Err(ApiError::NotImplemented("set_github_token"))
}
