use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct ProcessService;

#[allow(unused_variables)]
impl ProcessService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
    }

    /// Обновить список игр и события; при ошибке опроса сбросить состояние в неизвестное.
    pub fn refresh_running_games(&self) -> ServiceResult<Vec<GameProcessEvent>> {
        todo!()
    }

    pub fn launch(&self, game_id: GameId) -> ServiceResult<u32> {
        todo!()
    }

    /// Создать ярлык запуска игры через SaveHub.
    pub fn create_shortcut(
        &self,
        game_id: GameId,
        destination: &std::path::Path,
    ) -> ServiceResult<()> {
        todo!()
    }
}
