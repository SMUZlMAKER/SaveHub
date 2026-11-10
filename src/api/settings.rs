//! Экран «Настройки»: репозиторий GitHub и поведение синхронизации.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

use super::ApiResult;
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
}

/// TODO(backend): хранилище Settings.
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
    })
}

/// Поле `has_github_token` при сохранении игнорируется: токен меняется через `set_github_token`.
/// TODO(backend): записать Settings на диск; если сменился репозиторий — переподключить RepoWorker.
#[tauri::command]
pub fn update_settings(
    settings: SettingsDto,
    service: State<'_, Arc<SettingsService>>,
) -> ApiResult<()> {
    service.set_repository(
        settings.repo_url,
        settings.local_repo_path.map(std::path::PathBuf::from),
    )?;
    service.set_sync_after_session(settings.sync_after_session)?;
    service.set_sync_while_gaming(settings.sync_while_gaming)?;
    service.set_auto_switch_profile(settings.auto_switch_profile)?;
    Ok(())
}

/// TODO(backend): положить токен в системное хранилище (Windows Credential Manager), не в файл настроек.
#[tauri::command]
pub fn set_github_token(token: String, service: State<'_, Arc<SettingsService>>) -> ApiResult<()> {
    service.set_github_token(token.trim())?;
    Ok(())
}
