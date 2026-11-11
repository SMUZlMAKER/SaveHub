use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct RepoService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
}

#[allow(unused_variables)]
impl RepoService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            settings,
        }
    }

    pub fn configuration(&self) -> Option<RepositoryConfig> {
        let settings = self.settings.data();
        let url = settings.repo_url.filter(|url| !url.trim().is_empty())?;
        let path = settings
            .local_repo_path
            .filter(|path| !path.as_os_str().is_empty())?;
        Some(RepositoryConfig { url, path })
    }

    /// Подготовить репозиторий сохранений и общее хранилище каталога.
    pub fn connect(&self) -> ServiceResult<()> {
        todo!()
    }

    /// Синхронизировать единый каталог независимо от активного профиля.
    pub fn sync_catalog(&self) -> ServiceResult<()> {
        todo!()
    }

    pub fn status(&self) -> ServiceResult<SyncStatus> {
        Ok(self.runtime.sync_status())
    }

    /// Обновить удалённые ссылки, не меняя рабочие сохранения.
    pub fn fetch(&self) -> ServiceResult<()> {
        todo!()
    }

    /// Применить уже полученные изменения после проверки процессов и бэкапа.
    pub fn apply_profile_updates(&self, profile_id: &str) -> ServiceResult<Vec<Conflict>> {
        todo!()
    }

    pub fn checkout_profile(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Создать коммит; None при отсутствии изменений.
    pub fn checkpoint(&self, profile_id: &str, message: &str) -> ServiceResult<Option<CommitId>> {
        todo!()
    }

    /// Повторная отправка не создаёт новое сохранение или коммит.
    pub fn push(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Проверить локальные refs: есть ли коммиты, ещё не отправленные в upstream.
    /// Без сети; ветка с коммитами без upstream также требует первой отправки.
    pub fn has_unpushed_commits(&self, profile_id: &str) -> ServiceResult<bool> {
        todo!()
    }

    pub fn list_conflicts(&self) -> ServiceResult<Vec<Conflict>> {
        todo!()
    }

    /// Применить выбор, сохранив вторую версию конфликта.
    pub fn resolve_conflict(
        &self,
        conflict_id: &str,
        resolution: ConflictResolution,
    ) -> ServiceResult<()> {
        todo!()
    }

    pub fn is_profile_busy(&self, profile_id: &str) -> ServiceResult<bool> {
        todo!()
    }

    pub fn renew_lease(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Снять собственный маркер.
    pub fn release_lease(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }
}

/// Called under the shared operations mutex; do not lock it again or call UI code.
pub trait RepoOperations: Send + Sync + 'static {
    fn configuration(&self) -> Option<RepositoryConfig>;
    fn checkpoint(&self, profile: &ProfileId) -> ServiceResult<Option<CommitId>>;
    fn push(&self, profile: &ProfileId) -> ServiceResult<()>;
    fn fetch(&self) -> ServiceResult<()>;
    fn has_unpushed_commits(&self, profile: &ProfileId) -> ServiceResult<bool>;
}

impl RepoOperations for RepoService {
    fn configuration(&self) -> Option<RepositoryConfig> {
        self.configuration()
    }

    fn has_unpushed_commits(&self, profile: &ProfileId) -> ServiceResult<bool> {
        self.has_unpushed_commits(&profile.to_string())
    }

    fn checkpoint(&self, profile: &ProfileId) -> ServiceResult<Option<CommitId>> {
        self.checkpoint(&profile.to_string(), "SaveHub: automatic checkpoint")
    }

    fn push(&self, profile: &ProfileId) -> ServiceResult<()> {
        self.push(&profile.to_string())
    }

    fn fetch(&self) -> ServiceResult<()> {
        self.fetch()
    }
}
