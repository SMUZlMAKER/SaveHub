//! Экран «Настройки»: репозиторий GitHub и поведение синхронизации.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

use super::{ApiError, ApiResult};
use crate::background::repo::RepoWorkerHandle;
use crate::services::settings::SettingsService;

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
    pub ask_profile_before_launch: bool,
}

#[tauri::command]
pub fn get_settings(service: State<'_, Arc<SettingsService>>) -> ApiResult<SettingsDto> {
    let data = service.get()?;
    Ok(SettingsDto {
        repo_url: data.repo_url,
        local_repo_path: data
            .local_repo_path
            .map(|path| path.to_string_lossy().into_owned()),
        has_github_token: service.has_github_token()?,
        sync_after_session: data.sync_after_session,
        sync_while_gaming: data.sync_while_gaming,
        auto_switch_profile: data.auto_switch_profile,
        ask_profile_before_launch: data.ask_profile_before_launch,
    })
}

/// Поле `has_github_token` при сохранении игнорируется: токен меняется через `set_github_token`.
#[tauri::command]
pub async fn update_settings(
    settings: SettingsDto,
    service: State<'_, Arc<SettingsService>>,
    worker: State<'_, RepoWorkerHandle>,
) -> ApiResult<()> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.update(|data| {
            data.repo_url = settings.repo_url;
            data.local_repo_path = settings.local_repo_path.map(std::path::PathBuf::from);
            data.sync_after_session = settings.sync_after_session;
            data.sync_while_gaming = settings.sync_while_gaming;
            data.auto_switch_profile = settings.auto_switch_profile;
            data.ask_profile_before_launch = settings.ask_profile_before_launch;
        })
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))??;
    let _ = worker.sync_now();
    Ok(())
}

/// TODO(backend): положить токен в системное хранилище (Windows Credential Manager), не в файл настроек.
#[tauri::command]
pub fn set_github_token(
    token: String,
    service: State<'_, Arc<SettingsService>>,
    worker: State<'_, RepoWorkerHandle>,
) -> ApiResult<()> {
    service.set_github_token(token.trim())?;
    let _ = worker.sync_now();
    Ok(())
}
