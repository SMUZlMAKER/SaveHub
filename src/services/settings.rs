use super::{ServiceError, ServiceResult};
use crate::domain::*;
use std::sync::{Arc, Mutex};

pub struct SettingsService {
    settings: Arc<Settings>,
    path: std::path::PathBuf,
    operations: Arc<Mutex<()>>,
}

impl SettingsService {
    pub fn new(
        settings: Arc<Settings>,
        path: std::path::PathBuf,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            settings,
            path,
            operations,
        }
    }

    /// Захватывает operations; вызывающий код не должен уже удерживать этот mutex.
    pub fn update(&self, change: impl FnOnce(&mut SettingsData)) -> ServiceResult<()> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Repository operations lock poisoned".into()))?;
        let mut data = self.settings.data();
        change(&mut data);
        crate::adapters::config::write_settings(&self.path, &data)
            .map_err(ServiceError::Adapter)?;
        self.settings.replace_data(data);
        Ok(())
    }

    pub fn get(&self) -> ServiceResult<SettingsData> {
        Ok(self.settings.data())
    }

    pub fn set_repository(
        &self,
        url: Option<String>,
        path: Option<std::path::PathBuf>,
    ) -> ServiceResult<()> {
        self.update(|data| {
            data.repo_url = url;
            data.local_repo_path = path;
        })
    }

    pub fn set_sync_after_session(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.sync_after_session = enabled)
    }

    pub fn set_sync_while_gaming(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.sync_while_gaming = enabled)
    }

    pub fn set_auto_switch_profile(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.auto_switch_profile = enabled)
    }

    pub fn set_ask_profile_before_launch(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.ask_profile_before_launch = enabled)
    }

    pub fn set_start_minimized(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.start_minimized = enabled)
    }

    pub fn has_github_token(&self) -> ServiceResult<bool> {
        crate::adapters::credentials::has_token().map_err(ServiceError::Adapter)
    }

    /// Записать токен в системное хранилище.
    pub fn set_github_token(&self, token: &str) -> ServiceResult<()> {
        crate::adapters::credentials::set_token(token).map_err(ServiceError::Adapter)
    }
}

#[cfg(test)]
mod tests;
