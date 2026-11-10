use super::{ServiceError, ServiceResult};
use crate::domain::*;
use std::sync::Arc;

pub struct GameService {
    catalog: Arc<Catalog>,
    _runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
}

impl GameService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            _runtime: runtime,
            settings,
        }
    }

    /// Включая игры с отсутствующими путями и проигнорированные на этом ПК.
    pub fn list(&self) -> ServiceResult<Vec<Game>> {
        Ok(self.catalog.games().into_values().collect())
    }

    pub fn get(&self, game_id: GameId) -> ServiceResult<Game> {
        self.catalog
            .games()
            .remove(&game_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Игра не найдена: {game_id}")))
    }

    pub fn add(
        &self,
        name: &str,
        paths: GamePaths,
        process_names: Vec<String>,
    ) -> ServiceResult<Game> {
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Название игры не может быть пустым".into(),
            ));
        }
        let game = Game {
            id: uuid::Uuid::new_v4(),
            name: name.trim().to_owned(),
            paths,
            process_names,
        };
        self.catalog.add_game(game.clone());
        Ok(game)
    }

    pub fn update(
        &self,
        game_id: GameId,
        name: &str,
        paths: GamePaths,
        process_names: Vec<String>,
    ) -> ServiceResult<Game> {
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Название игры не может быть пустым".into(),
            ));
        }
        if !self.catalog.update_game(Game {
            id: game_id,
            name: name.trim().to_owned(),
            paths,
            process_names,
        }) {
            return Err(ServiceError::NotFound("Игра не найдена".into()));
        }
        self.get(game_id)
    }

    /// Удалить регистрацию игры; сохранения не удалять.
    pub fn delete(&self, game_id: GameId) -> ServiceResult<()> {
        if self.catalog.remove_game(game_id) {
            Ok(())
        } else {
            Err(ServiceError::NotFound("Игра не найдена".into()))
        }
    }

    /// Применить выбор к общему или локальному конфигу и запомнить локально.
    pub fn resolve_paths(&self, game_id: GameId, resolution: PathResolution) -> ServiceResult<()> {
        if !self.catalog.games().contains_key(&game_id) {
            return Err(ServiceError::NotFound("Игра не найдена".into()));
        }
        self.settings.add_game_resolution(game_id, resolution);
        Ok(())
    }

    /// Вернуть использование общих путей, отменить игнорирование.
    pub fn clear_local_resolution(&self, game_id: GameId) -> ServiceResult<()> {
        let _ = self.get(game_id)?;
        self.settings.clear_game_resolution(&game_id);
        Ok(())
    }

    /// Раскрыть переменные окружения и проверить локально оба пути.
    pub fn check_paths(&self, game_id: GameId) -> ServiceResult<GameAvailability> {
        let game = self.get(game_id)?;
        let local = self.settings.local();
        let resolution = local.games.get(&game_id);
        if matches!(resolution, Some(PathResolution::Ignore)) {
            return Ok(GameAvailability::Ignored);
        }
        let paths = match resolution {
            Some(PathResolution::UpdateRepository(paths) | PathResolution::UseLocal(paths)) => {
                paths
            }
            None => &game.paths,
            Some(PathResolution::Ignore) => unreachable!(),
        };
        let save = crate::adapters::fs::expand_path(&paths.save_dir_path)
            .map_err(ServiceError::Adapter)?;
        let exe =
            crate::adapters::fs::expand_path(&paths.exe_path).map_err(ServiceError::Adapter)?;
        let save_missing =
            !crate::adapters::fs::is_directory(&save).map_err(ServiceError::Adapter)?;
        let exe_missing = !crate::adapters::fs::exists(&exe).map_err(ServiceError::Adapter)?;
        if save_missing || exe_missing {
            Ok(GameAvailability::Missing {
                save_dir_missing: save_missing,
                exe_missing,
            })
        } else {
            Ok(GameAvailability::Ready {
                save_dir_path: save,
                exe_path: exe,
            })
        }
    }

    pub fn reload_catalog(&self) -> ServiceResult<()> {
        Ok(())
    }
}
