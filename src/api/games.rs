//! Game setup IPC. Catalogue is persisted separately; no save files are moved.
use super::{ApiError, ApiResult};
use crate::{
    domain::{Game, GameId, GamePaths, PathResolution, RuntimeState, Settings},
    services::game::GameService,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, OnceLock},
};
use tauri::{Emitter, State};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDto {
    pub id: String,
    pub name: String,
    pub save_path: Option<String>,
    pub exe_path: String,
    pub status: GameStatus,
    pub exe_missing: bool,
    pub save_path_missing: bool,
    pub path_scope: PathScope,
    pub process_state_known: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameStatus {
    Found,
    Missing,
    Running,
    Ignored,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PathScope {
    Shared,
    Local,
    Ignored,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathAction {
    UseLocal,
    UpdateRepository,
    Ignore,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathValidationDto {
    pub exe_path: Option<String>,
    pub save_path: Option<String>,
}

#[tauri::command]
pub async fn list_games(
    service: State<'_, Arc<GameService>>,
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<RuntimeState>>,
) -> ApiResult<Vec<GameDto>> {
    let (service, settings, runtime) = (
        service.inner().clone(),
        settings.inner().clone(),
        runtime.inner().clone(),
    );
    blocking(move || {
        let mut games = service.list()?;
        games.sort_by_key(|g| (g.name.to_lowercase(), g.id));
        Ok(games
            .iter()
            .map(|g| to_dto(g, &settings, &runtime))
            .collect())
    })
    .await
}
#[tauri::command]
pub async fn get_game_status(
    game_id: GameId,
    service: State<'_, Arc<GameService>>,
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<RuntimeState>>,
) -> ApiResult<GameStatus> {
    let (service, settings, runtime) = (
        service.inner().clone(),
        settings.inner().clone(),
        runtime.inner().clone(),
    );
    blocking(move || Ok(to_dto(&service.get(game_id)?, &settings, &runtime).status)).await
}
#[tauri::command]
pub async fn validate_game_paths(
    exe_path: String,
    save_path: String,
) -> ApiResult<PathValidationDto> {
    blocking(move || {
        let (exe_path, save_path) =
            crate::adapters::game_paths::validate(&paths(exe_path, save_path));
        Ok(PathValidationDto {
            exe_path,
            save_path,
        })
    })
    .await
}
#[tauri::command]
pub async fn add_game(
    name: String,
    exe_path: String,
    save_path: String,
    app: tauri::AppHandle,
    service: State<'_, Arc<GameService>>,
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<RuntimeState>>,
) -> ApiResult<GameDto> {
    let (service, settings, runtime) = (
        service.inner().clone(),
        settings.inner().clone(),
        runtime.inner().clone(),
    );
    let result = blocking(move || {
        let game = service.add_with_paths(&name, paths(exe_path, save_path))?;
        Ok(to_dto(&game, &settings, &runtime))
    })
    .await;
    // Refresh even after partial failure; event delivery must not change the mutation result.
    let _ = app.emit(super::events::GAMES_CHANGED, ());
    result
}
#[tauri::command]
pub async fn update_game(
    game_id: GameId,
    name: String,
    exe_path: String,
    save_path: String,
    app: tauri::AppHandle,
    service: State<'_, Arc<GameService>>,
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<RuntimeState>>,
) -> ApiResult<GameDto> {
    let (service, settings, runtime) = (
        service.inner().clone(),
        settings.inner().clone(),
        runtime.inner().clone(),
    );
    let result = blocking(move || {
        let game = service.update_with_paths(game_id, &name, paths(exe_path, save_path))?;
        Ok(to_dto(&game, &settings, &runtime))
    })
    .await;
    let _ = app.emit(super::events::GAMES_CHANGED, ());
    result
}
#[tauri::command]
pub async fn resolve_game_paths(
    game_id: GameId,
    action: PathAction,
    exe_path: Option<String>,
    save_path: Option<String>,
    app: tauri::AppHandle,
    service: State<'_, Arc<GameService>>,
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<RuntimeState>>,
) -> ApiResult<GameDto> {
    let (service, settings, runtime) = (
        service.inner().clone(),
        settings.inner().clone(),
        runtime.inner().clone(),
    );
    let result = blocking(move || {
        let game = match action {
            PathAction::UpdateRepository => service.update_shared_paths(
                game_id,
                paths(exe_path.unwrap_or_default(), save_path.unwrap_or_default()),
            )?,
            PathAction::Ignore => service.ignore_on_this_pc(game_id)?,
            PathAction::UseLocal => service.use_local_paths(
                game_id,
                paths(exe_path.unwrap_or_default(), save_path.unwrap_or_default()),
            )?,
        };
        Ok(to_dto(&game, &settings, &runtime))
    })
    .await;
    let _ = app.emit(super::events::GAMES_CHANGED, ());
    result
}

#[tauri::command]
pub fn get_catalog_status(
    service: State<'_, Arc<GameService>>,
) -> ApiResult<super::sync::SyncStatusDto> {
    let status = service.status();
    Ok(super::sync::SyncStatusDto {
        state: match status.state {
            crate::domain::SyncState::NotConfigured => super::sync::SyncState::NotConfigured,
            crate::domain::SyncState::Idle => super::sync::SyncState::Idle,
            crate::domain::SyncState::Syncing => super::sync::SyncState::Syncing,
            crate::domain::SyncState::Error => super::sync::SyncState::Error,
        },
        last_sync_at: status.last_sync_at,
        error: status.error,
    })
}

#[tauri::command]
pub async fn sync_catalog(
    app: tauri::AppHandle,
    service: State<'_, Arc<GameService>>,
) -> ApiResult<()> {
    let service = service.inner().clone();
    let result = blocking(move || service.sync_catalog().map_err(Into::into)).await;
    let _ = app.emit(super::events::GAMES_CHANGED, ());
    result
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSuggestionDto {
    pub game: String,
    pub save_dirs: Vec<String>,
}
#[tauri::command]
pub async fn suggest_save_dirs(exe_path: String) -> ApiResult<Option<SaveSuggestionDto>> {
    blocking(move || {
        static DB: OnceLock<crate::adapters::save_paths::SaveDb> = OnceLock::new();
        let db = DB.get_or_init(crate::adapters::save_paths::SaveDb::bundled);
        Ok(db.suggest(Path::new(&exe_path)).map(|s| SaveSuggestionDto {
            game: s.game,
            save_dirs: s
                .save_dirs
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        }))
    })
    .await
}
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
}
fn paths(exe_path: String, save_path: String) -> GamePaths {
    GamePaths {
        exe_path: exe_path.trim().into(),
        save_dir_path: save_path.trim().into(),
    }
}
fn to_dto(game: &Game, settings: &Settings, runtime: &RuntimeState) -> GameDto {
    let local = settings.local();
    let (paths, scope) = match local.games.get(&game.id) {
        Some(PathResolution::UseLocal(p)) => (p, PathScope::Local),
        Some(PathResolution::Ignore) => (&game.paths, PathScope::Ignored),
        Some(PathResolution::UpdateRepository(p)) => (p, PathScope::Local),
        None => (&game.paths, PathScope::Shared),
    };
    let (exe, saves) = crate::adapters::game_paths::validate(paths);
    let running = runtime.running_games();
    let status = if running.as_ref().is_some_and(|ids| ids.contains(&game.id)) {
        GameStatus::Running
    } else if matches!(scope, PathScope::Ignored) {
        GameStatus::Ignored
    } else if exe.is_some() || saves.is_some() {
        GameStatus::Missing
    } else {
        GameStatus::Found
    };
    GameDto {
        id: game.id.to_string(),
        name: game.name.clone(),
        save_path: Some(paths.save_dir_path.clone()),
        exe_path: paths.exe_path.clone(),
        status,
        exe_missing: exe.is_some(),
        save_path_missing: saves.is_some(),
        path_scope: scope,
        process_state_known: running.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalConfig, SettingsData, SyncState, SyncStatus};
    use std::collections::HashMap;

    #[test]
    fn reports_local_paths_ignored_state_and_running_guard() {
        let game = Game {
            id: uuid::Uuid::new_v4(),
            name: "Fixture".into(),
            paths: paths("shared.exe".into(), "shared-saves".into()),
            process_names: vec![],
        };
        let settings = Settings::new(
            SettingsData {
                catalog_repo_url: None,
                repo_url: None,
                local_repo_path: None,
                sync_after_session: false,
                sync_while_gaming: false,
                auto_switch_profile: false,
                ask_profile_before_launch: true,
                start_minimized: false,
            },
            LocalConfig {
                machine_id: "test".into(),
                games: HashMap::new(),
            },
        );
        let runtime = RuntimeState::new(
            None,
            vec![],
            SyncStatus {
                state: SyncState::NotConfigured,
                last_sync_at: None,
                error: None,
            },
        );
        settings.add_game_resolution(
            game.id,
            PathResolution::UseLocal(paths("local.exe".into(), "local-saves".into())),
        );
        let dto = to_dto(&game, &settings, &runtime);
        assert_eq!(dto.exe_path, "local.exe");
        assert_eq!(dto.save_path.as_deref(), Some("local-saves"));
        assert!(matches!(dto.path_scope, PathScope::Local));
        assert!(dto.exe_missing && dto.save_path_missing && !dto.process_state_known);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["exePath"], "local.exe");
        assert_eq!(json["savePathMissing"], true);
        assert_eq!(game.paths.exe_path, "shared.exe");
        settings.add_game_resolution(game.id, PathResolution::Ignore);
        assert!(matches!(
            to_dto(&game, &settings, &runtime).status,
            GameStatus::Ignored
        ));
        runtime.set_running_games(vec![game.id]);
        assert!(matches!(
            to_dto(&game, &settings, &runtime).status,
            GameStatus::Running
        ));
        let service = GameService::new(
            Arc::new(crate::domain::Catalog::new(HashMap::new(), HashMap::new())),
            Arc::new(runtime),
            Arc::new(settings),
        );
        assert!(matches!(
            service.ignore_on_this_pc(game.id),
            Err(crate::services::ServiceError::GameRunning(_))
        ));
    }
}
