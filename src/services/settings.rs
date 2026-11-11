use super::{ServiceError, ServiceResult};
use crate::domain::*;
use std::sync::Arc;

pub struct SettingsService {
    settings: Arc<Settings>,
    path: std::path::PathBuf,
}

impl SettingsService {
    pub fn new(settings: Arc<Settings>, path: std::path::PathBuf) -> Self {
        Self { settings, path }
    }

    fn persist(&self) -> ServiceResult<()> { //TODO  Изменение настроек и сохранение не сериализованы: возможна перезапись свежих данных старой версией; при ошибке записи память уже изменена. `src/services/settings.rs:15`.
        crate::adapters::config::write_settings(&self.path, &self.settings.data())
            .map_err(ServiceError::Adapter)
    }

    pub fn get(&self) -> ServiceResult<SettingsData> {
        Ok(self.settings.data())
    }

    pub fn set_repository(
        &self,
        url: Option<String>,
        path: Option<std::path::PathBuf>,
    ) -> ServiceResult<()> {
        self.settings.set_repository(url, path);
        self.persist()
    }

    pub fn set_sync_after_session(&self, enabled: bool) -> ServiceResult<()> {
        self.settings.set_sync_after_session(enabled);
        self.persist()
    }

    pub fn set_sync_while_gaming(&self, enabled: bool) -> ServiceResult<()> {
        self.settings.set_sync_while_gaming(enabled);
        self.persist()
    }

    pub fn set_auto_switch_profile(&self, enabled: bool) -> ServiceResult<()> {
        self.settings.set_auto_switch_profile(enabled);
        self.persist()
    }

    pub fn set_ask_profile_before_launch(&self, enabled: bool) -> ServiceResult<()> {
        self.settings.set_ask_profile_before_launch(enabled);
        self.persist()
    }

    pub fn set_start_minimized(&self, enabled: bool) -> ServiceResult<()> {
        self.settings.set_start_minimized(enabled);
        self.persist()
    }

    pub fn has_github_token(&self) -> ServiceResult<bool> {
        crate::adapters::credentials::has_token().map_err(ServiceError::Adapter)
    }

    /// Записать токен в системное хранилище.
    pub fn set_github_token(&self, token: &str) -> ServiceResult<()> {
        crate::adapters::credentials::set_token(token).map_err(ServiceError::Adapter)
    }
}
