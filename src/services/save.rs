use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct SaveService;

#[allow(unused_variables)]
impl SaveService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
    }

    /// Импортировать или создать пустое хранилище; существующие данные не перезаписывать.
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

    pub fn create_save(
        &self,
        game_id: GameId,
        profile_id: &str,
        kind: SaveKind,
    ) -> ServiceResult<Save> {
        todo!()
    }

    /// Сохранения от новых к старым.
    pub fn list(
        &self,
        game_id: Option<GameId>,
        profile_id: Option<&str>,
    ) -> ServiceResult<Vec<Save>> {
        todo!()
    }

    pub fn get_save(&self, save_id: &str) -> ServiceResult<Save> {
        todo!()
    }

    /// Применить сохранение; требуется бэкап текущих сохранений и остановленная игра.
    pub fn apply_save(&self, save_id: &str) -> ServiceResult<()> {
        todo!()
    }
}
