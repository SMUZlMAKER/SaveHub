use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct GameService;

#[allow(unused_variables)]
impl GameService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
    }

    /// Включая игры с отсутствующими путями и проигнорированные на этом ПК.
    pub fn list(&self) -> ServiceResult<Vec<Game>> {
        todo!()
    }

    pub fn get(&self, game_id: GameId) -> ServiceResult<Game> {
        todo!()
    }

    pub fn add(
        &self,
        name: &str,
        paths: GamePaths,
        process_names: Vec<String>,
    ) -> ServiceResult<Game> {
        todo!()
    }

    pub fn update(
        &self,
        game_id: GameId,
        name: &str,
        paths: GamePaths,
        process_names: Vec<String>,
    ) -> ServiceResult<Game> {
        todo!()
    }

    /// Удалить регистрацию игры; сохранения не удалять.
    pub fn delete(&self, game_id: GameId) -> ServiceResult<()> {
        todo!()
    }

    /// Применить выбор к общему или локальному конфигу и запомнить локально.
    pub fn resolve_paths(&self, game_id: GameId, resolution: PathResolution) -> ServiceResult<()> {
        todo!()
    }

    /// Вернуть использование общих путей, отменить игнорирование.
    pub fn clear_local_resolution(&self, game_id: GameId) -> ServiceResult<()> {
        todo!()
    }

    /// Раскрыть переменные окружения и проверить локально оба пути.
    pub fn check_paths(&self, game_id: GameId) -> ServiceResult<GameAvailability> {
        todo!()
    }

    pub fn reload_catalog(&self) -> ServiceResult<()> {
        todo!()
    }
}
