use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct SaveService;

#[allow(unused_variables)]
impl SaveService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
    }

    /// Импортировать исходные сохранения или подготовить пустые для нового профиля.
    pub fn initialize(&self, game_id: GameId, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Подключить каталог профиля через symlink; только при остановленной игре.
    pub fn bind(&self, game_id: GameId, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Убрать symlink, не удаляя целевые данные.
    pub fn unbind(&self, game_id: GameId) -> ServiceResult<()> {
        todo!()
    }

    pub fn create_snapshot(
        &self,
        game_id: GameId,
        profile_id: &str,
        kind: SnapshotKind,
    ) -> ServiceResult<Snapshot> {
        todo!()
    }

    /// Снимки от новых к старым.
    pub fn list(
        &self,
        game_id: Option<GameId>,
        profile_id: Option<&str>,
    ) -> ServiceResult<Vec<Snapshot>> {
        todo!()
    }

    pub fn get_snapshot(&self, snapshot_id: &str) -> ServiceResult<Snapshot> {
        todo!()
    }

    /// Шаг workflow отката: до вызова нужен BeforeRestore и проверка процессов.
    pub fn apply_snapshot(&self, snapshot_id: &str) -> ServiceResult<()> {
        todo!()
    }
}
