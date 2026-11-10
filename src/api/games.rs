//! Экран «Игры»: какие игры отслеживаются и где лежат их сохранения.

use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use super::{ApiError, ApiResult};
use crate::domain::{Game, GameAvailability, GameId};
use crate::services::game::GameService;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDto {
    pub id: String,
    pub name: String,
    pub save_path: Option<String>,
    pub status: GameStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameStatus {
    Found,
    Missing,
    Running,
}

#[tauri::command]
pub fn get_game_status(
    game: String,
    service: State<'_, Arc<GameService>>,
) -> ApiResult<GameStatus> {
    let id = parse_game_id(&game)?;
    let game = service.get(id)?;
    status_for(&game, &service)
}

#[tauri::command]
pub fn list_games(service: State<'_, Arc<GameService>>) -> ApiResult<Vec<GameDto>> {
    service
        .list()?
        .into_iter()
        .map(|game| to_dto(&game, &service))
        .collect()
}

/// TODO: IPC currently provides only a save path, while GameService requires
/// both save and executable paths. Keep this explicit until the contract grows.
#[tauri::command]
pub fn add_game(name: String, save_path: String) -> ApiResult<GameDto> {
    let _ = (name, save_path);
    Err(ApiError::NotImplemented("add_game"))
}

/// TODO: resolve_paths also needs the executable path or an explicit resolution.
#[tauri::command]
pub fn set_game_save_path(game_id: String, save_path: String) -> ApiResult<()> {
    let _ = (game_id, save_path);
    Err(ApiError::NotImplemented("set_game_save_path"))
}

fn parse_game_id(value: &str) -> ApiResult<GameId> {
    value
        .parse()
        .map_err(|_| ApiError::Rejected(format!("Некорректный идентификатор игры: {value}")))
}

fn to_dto(game: &Game, service: &GameService) -> ApiResult<GameDto> {
    let availability = service.check_paths(game.id)?;
    let (save_path, status) = match availability {
        GameAvailability::Ready { save_dir_path, .. } => (
            Some(save_dir_path.to_string_lossy().into_owned()),
            GameStatus::Found,
        ),
        GameAvailability::Missing { .. } => {
            (Some(game.paths.save_dir_path.clone()), GameStatus::Missing)
        }
        GameAvailability::Ignored => (None, GameStatus::Missing),
    };
    Ok(GameDto {
        id: game.id.to_string(),
        name: game.name.clone(),
        save_path,
        status,
    })
}

fn status_for(game: &Game, service: &GameService) -> ApiResult<GameStatus> {
    Ok(to_dto(game, service)?.status)
}
