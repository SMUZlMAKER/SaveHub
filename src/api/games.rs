//! Экран «Игры»: какие игры отслеживаются и где лежат их сохранения.

use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use super::ApiResult;
use crate::domain::{Game, GameAvailability, GameId, GamePaths};
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
    game_id: GameId,
    service: State<'_, Arc<GameService>>,
) -> ApiResult<GameStatus> {
    let game = service.get(game_id)?;
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

//TODO нужен ui под путь для ярлыка
#[tauri::command]
pub fn add_game(name: String, save_path: String,service: State<'_, Arc<GameService>>) -> ApiResult<GameDto> {
    let game = service.add(
        &name,
        GamePaths {
            save_dir_path: save_path.clone(),
            exe_path: save_path, // TODO после внедрения ярлыков это заменится на путь до него, пока заглушка
        },
        Vec::new(),
    )?;

    to_dto(&game, &service)
}

//TODO нужен ui для изменения данных игры (имя, два пути)
#[tauri::command]
pub fn update_game(game_id: GameId, name: String, save_path: String, service: State<'_, Arc<GameService>>) -> ApiResult<GameDto> {
    let game = service.update(
        game_id,
        &name,
        GamePaths {
            save_dir_path: save_path.clone(),
            exe_path: save_path, // TODO после внедрения ярлыков это заменится на путь до него, пока заглушка
        },
        Vec::new(),
    )?;

    to_dto(&game, &service)
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
