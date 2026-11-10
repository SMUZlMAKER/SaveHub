use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct SettingsService;

#[allow(unused_variables)]
impl SettingsService {
    pub fn new(settings: Arc<Settings>) -> Self {
        todo!()
    }

    pub fn get(&self) -> ServiceResult<SettingsData> {
        todo!()
    }

    pub fn set_repository(
        &self,
        url: Option<String>,
        path: Option<std::path::PathBuf>,
    ) -> ServiceResult<()> {
        todo!()
    }

    pub fn set_sync_after_session(&self, enabled: bool) -> ServiceResult<()> {
        todo!()
    }

    pub fn set_sync_while_gaming(&self, enabled: bool) -> ServiceResult<()> {
        todo!()
    }

    pub fn set_auto_switch_profile(&self, enabled: bool) -> ServiceResult<()> {
        todo!()
    }

    pub fn set_start_minimized(&self, enabled: bool) -> ServiceResult<()> {
        todo!()
    }

    pub fn has_github_token(&self) -> ServiceResult<bool> {
        todo!()
    }

    /// Записать токен в системное хранилище.
    pub fn set_github_token(&self, token: &str) -> ServiceResult<()> {
        todo!()
    }
}
