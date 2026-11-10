use super::{ServiceError, ServiceResult};
use crate::domain::*;
use std::sync::Arc;

pub struct ProcessService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
}

#[allow(unused_variables)]
impl ProcessService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            settings,
        }
    }

    /// Обновить список игр и события; при ошибке опроса сбросить состояние в неизвестное.
    pub fn refresh_running_games(&self) -> ServiceResult<Vec<GameProcessEvent>> {
        let events = self.runtime.set_running_games(Vec::new());
        // TODO опрос OC/ привязка к ярлыку
        Ok(events)
    }

    pub fn running_games(&self) -> Option<Vec<GameId>> {
        self.runtime.running_games()
    }

    pub fn launch(&self, game_id: GameId) -> ServiceResult<u32> {
        let game = self
            .catalog
            .games()
            .remove(&game_id)
            .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))?;
        let availability = crate::services::game::GameService::new(
            self.catalog.clone(),
            self.runtime.clone(),
            self.settings.clone(),
        )
        .check_paths(game_id)?;
        let GameAvailability::Ready { exe_path, .. } = availability else {
            return Err(ServiceError::InvalidInput(
                "Игра недоступна на этом компьютере".into(),
            ));
        };
        crate::adapters::process::spawn(&exe_path, &[], None).map_err(ServiceError::Adapter)
    }

    /// Создать ярлык запуска игры через SaveHub.
    pub fn create_shortcut(
        &self,
        game_id: GameId,
        destination: &std::path::Path,
    ) -> ServiceResult<()> {
        let game = self
            .catalog
            .games()
            .remove(&game_id)
            .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))?;
        let GameAvailability::Ready { exe_path, .. } = crate::services::game::GameService::new(
            self.catalog.clone(),
            self.runtime.clone(),
            self.settings.clone(),
        )
        .check_paths(game_id)?
        else {
            return Err(ServiceError::InvalidInput(
                "Игра недоступна на этом компьютере".into(),
            ));
        };
        crate::adapters::process::create_shortcut(destination, &exe_path, &[], None)
            .map_err(ServiceError::Adapter)?;
        let _ = game;
        Ok(())
    }
}
