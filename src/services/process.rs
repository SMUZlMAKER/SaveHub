use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct ProcessService;

#[allow(unused_variables)]
impl ProcessService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
    }

    /// Обновить список запущенных игр и вернуть события запуска и завершения.
    pub fn refresh_running_games(&self) -> ServiceResult<Vec<GameProcessEvent>> {
        todo!()
    }

    /// Шаг workflow запуска после fetch, выбора профиля и разрешения конфликтов.
    pub fn launch(&self, game_id: GameId) -> ServiceResult<u32> {
        todo!()
    }

    /// Создать ярлык запуска через SaveHub для проверки синхронизации до старта.
    pub fn create_shortcut(
        &self,
        game_id: GameId,
        destination: &std::path::Path,
    ) -> ServiceResult<()> {
        todo!()
    }
}
