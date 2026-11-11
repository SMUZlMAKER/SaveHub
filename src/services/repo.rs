use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct RepoService;

#[allow(unused_variables)]
impl RepoService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
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
        todo!()
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
