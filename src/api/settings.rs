//! Экран «Настройки»: репозиторий GitHub и поведение синхронизации.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

use super::{ApiError, ApiResult};
use crate::background::{catalog::CatalogWorker, repo::RepoWorkerHandle};
use crate::services::{game::GameService, settings::SettingsService};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// Отдельный репозиторий списка игр.
    pub catalog_repo_url: Option<String>,
    /// Удаленный репозиторий сохранений.
    pub repo_url: Option<String>,
    /// Локальный клон этого репозитория.
    pub local_repo_path: Option<String>,
    /// Сохранен ли токен GitHub. Сам токен фронту не отдаем.
    pub has_github_token: bool,
    pub sync_after_session: bool,
    pub sync_while_gaming: bool,
    pub ask_profile_before_launch: bool,
}

#[tauri::command]
pub fn get_settings(
    service: State<'_, Arc<SettingsService>>,
    trace_id: Option<String>,
) -> ApiResult<SettingsDto> {
    crate::adapters::diagnostics::request_sync(trace_id, "get_settings", || {
        let data = service.get()?;
        Ok(SettingsDto {
            catalog_repo_url: data.catalog_repo_url,
            repo_url: data.repo_url,
            local_repo_path: data
                .local_repo_path
                .map(|path| path.to_string_lossy().into_owned()),
            has_github_token: service.has_github_token()?,
            sync_after_session: data.sync_after_session,
            sync_while_gaming: data.sync_while_gaming,
            ask_profile_before_launch: data.ask_profile_before_launch,
        })
    })
}

/// Поле `has_github_token` при сохранении игнорируется: токен меняется через `set_github_token`.
#[tauri::command]
pub async fn update_settings(
    settings: SettingsDto,
    service: State<'_, Arc<SettingsService>>,
    worker: State<'_, RepoWorkerHandle>,
    catalog_worker: State<'_, CatalogWorker>,
    games: State<'_, Arc<GameService>>,
    app: tauri::AppHandle,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request(trace_id, "update_settings", async move {
        let previous_url = service.get()?.catalog_repo_url;
        let updated_service = service.inner().clone();
        let service = service.inner().clone();
        crate::app::diagnostics::spawn_blocking(move || {
            service.update(|data| {
                data.catalog_repo_url = settings.catalog_repo_url;
                data.repo_url = settings.repo_url;
                data.local_repo_path = settings.local_repo_path.map(std::path::PathBuf::from);
                data.sync_after_session = settings.sync_after_session;
                data.sync_while_gaming = settings.sync_while_gaming;
                data.ask_profile_before_launch = settings.ask_profile_before_launch;
            })
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))??;
        let _ = worker.wake();
        if previous_url != updated_service.get()?.catalog_repo_url {
            games.mark_sync_pending();
            crate::app::catalog::notify_status(&app);
            catalog_worker.request_sync();
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn connect_repository(
    app: tauri::AppHandle,
    repo: State<'_, Arc<crate::services::repo::RepoService>>,
    worker: State<'_, RepoWorkerHandle>,
    operations: State<'_, Arc<std::sync::Mutex<()>>>,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request(trace_id, "connect_repository", async move {
        let (repo, operations) = (repo.inner().clone(), operations.inner().clone());
        crate::app::diagnostics::spawn_blocking(move || {
            let _guard = operations.lock().map_err(|_| {
                crate::services::ServiceError::Conflict("Блокировка операций повреждена".into())
            })?;
            repo.connect()
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))??;
        use tauri::Emitter;
        let _ = app.emit(crate::api::events::PROFILE_CHANGED, ());
        worker.wake()?;
        Ok(())
    })
    .await
}

/// Токен сохраняется отдельно в Windows Credential Manager.
#[tauri::command]
pub fn set_github_token(
    token: String,
    service: State<'_, Arc<SettingsService>>,
    worker: State<'_, RepoWorkerHandle>,
    catalog_worker: State<'_, CatalogWorker>,
    games: State<'_, Arc<GameService>>,
    app: tauri::AppHandle,
    trace_id: Option<String>,
) -> ApiResult<()> {
    crate::adapters::diagnostics::request_sync(trace_id, "set_github_token", || {
        service.set_github_token(token.trim())?;
        let _ = worker.wake();
        games.mark_sync_pending();
        crate::app::catalog::notify_status(&app);
        catalog_worker.request_sync();
        Ok(())
    })
}
