//! Tauri orchestration: updates -> profile policy -> workflow -> executable.
use crate::{
    api::{
        ApiError, ApiResult, events,
        launch::{GameProcessDto, LaunchProfileDto, LaunchStatus},
    },
    domain::{GameId, ProfileId},
    services::{game::GameService, process::ProcessService},
    workflows::launch_game::LaunchGame,
};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub struct LaunchReady;

#[derive(Default)]
pub struct Launches {
    status: Mutex<LaunchStatus>,
    pub operation: tokio::sync::Mutex<()>,
}
impl Launches {
    pub fn status(&self) -> LaunchStatus {
        self.status.lock().unwrap().clone()
    }
    pub fn monitor_status(&self, app: &tauri::AppHandle, error: Option<String>) {
        let next = {
            let mut status = self.status.lock().unwrap();
            if status.monitor_error == error {
                return;
            }
            status.monitor_error = error;
            status.clone()
        };
        let _ = app.emit(events::LAUNCH_STATUS_CHANGED, next);
    }
    pub fn mark_stopped(&self, app: &tauri::AppHandle, game_id: GameId) {
        let next = {
            let mut status = self.status.lock().unwrap();
            if status.game_id != Some(game_id) || status.phase != "running" {
                return;
            }
            status.phase = "stopped".into();
            status.pid = None;
            status.clone()
        };
        let _ = app.emit(events::LAUNCH_STATUS_CHANGED, next);
    }
    pub fn publish(&self, app: &tauri::AppHandle, status: LaunchStatus) {
        *self.status.lock().unwrap() = status.clone();
        let _ = app.emit(events::LAUNCH_STATUS_CHANGED, status);
    }
}
fn error_message(error: &ApiError) -> String {
    match error {
        ApiError::Rejected(s) | ApiError::Internal(s) => s.clone(),
        ApiError::NotImplemented(s) => format!("Пока не реализовано: {s}"),
    }
}

pub async fn begin(app: &tauri::AppHandle, game_id: GameId) -> ApiResult<LaunchStatus> {
    let state = app.state::<Launches>();
    let _guard = state
        .operation
        .try_lock()
        .map_err(|_| ApiError::Rejected("Другой запуск уже подготавливается".into()))?;
    if state.status().phase == "awaitingProfile" {
        return Err(ApiError::Rejected(
            "Сначала завершите или отмените выбор профиля".into(),
        ));
    }
    let status = LaunchStatus {
        request_id: Some(uuid::Uuid::new_v4()),
        game_id: Some(game_id),
        phase: "preparing".into(),
        ..Default::default()
    };
    state.publish(app, status);
    let result = prepare(app, game_id).await;
    if let Err(error) = &result {
        let mut status = state.status();
        status.phase = "error".into();
        status.error = Some(error_message(error));
        state.publish(app, status);
    }
    result
}
async fn prepare(app: &tauri::AppHandle, game_id: GameId) -> ApiResult<LaunchStatus> {
    let state = app.state::<Launches>();
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .join("launchers");
    let processes = app.state::<Arc<ProcessService>>().inner().clone();
    let workflow = app.state::<Arc<LaunchGame>>().inner().clone();
    let games = app.state::<Arc<GameService>>().inner().clone();
    let (plan, game) = tauri::async_runtime::spawn_blocking(move || {
        processes.restore_registration(game_id, &directory)?;
        let game = games.get(game_id)?;
        Ok::<_, crate::services::ServiceError>((workflow.prepare()?, game))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    let _ = app.emit(events::GAMES_CHANGED, ());
    let mut status = state.status();
    status.game_name = Some(game.name);
    status.active_profile_id = plan.active_profile;
    status.profiles = plan
        .profiles
        .iter()
        .map(|p| LaunchProfileDto {
            id: p.id,
            name: p.name.clone(),
            is_active: Some(p.id) == plan.active_profile,
        })
        .collect();
    status.warnings.push("Актуальность сохранений пока не проверяется; одновременная игра на другом ПК не блокируется. Будет использована текущая локальная папка сохранений.".into());
    let updates = app.state::<crate::app::updates::Updates>();
    if updates.status().configured {
        match updates.check(app).await {
            Ok(update) if update.phase == "error" => status.warnings.push(
                update
                    .error
                    .unwrap_or_else(|| "Не удалось проверить обновления SaveHub".into()),
            ),
            Ok(update) if update.version.is_some() => status.warnings.push(format!(
                "Доступно обновление SaveHub {}. Установить его можно в настройках.",
                update.version.unwrap()
            )),
            Ok(_) => (),
            Err(error) => status.warnings.push(error_message(&error)),
        }
    } else {
        status.warnings.push(
            "Проверка обновлений SaveHub недоступна: ключ подписи не настроен в этой сборке."
                .into(),
        );
    }
    if plan.needs_profile {
        status.phase = "awaitingProfile".into();
        state.publish(app, status.clone());
        Ok(status)
    } else {
        state.publish(app, status);
        execute(
            app,
            game_id,
            plan.active_profile
                .ok_or_else(|| ApiError::Rejected("Нет активного профиля".into()))?,
        )
        .await
    }
}
pub async fn confirm(
    app: &tauri::AppHandle,
    request_id: uuid::Uuid,
    profile_id: ProfileId,
) -> ApiResult<LaunchStatus> {
    let state = app.state::<Launches>();
    let _guard = state
        .operation
        .try_lock()
        .map_err(|_| ApiError::Rejected("Запуск уже выполняется".into()))?;
    let mut status = state.status();
    if status.request_id != Some(request_id) || status.phase != "awaitingProfile" {
        return Err(ApiError::Rejected("Запрос запуска уже изменился".into()));
    }
    // Keep the picker open after a failed profile switch, so the user can choose the active profile.
    match execute(app, status.game_id.unwrap(), profile_id).await {
        Ok(status) => Ok(status),
        Err(error) => {
            status.error = Some(error_message(&error));
            state.publish(app, status);
            Err(error)
        }
    }
}
async fn execute(
    app: &tauri::AppHandle,
    game_id: GameId,
    profile_id: ProfileId,
) -> ApiResult<LaunchStatus> {
    let workflow = app.state::<Arc<LaunchGame>>().inner().clone();
    let pid = tauri::async_runtime::spawn_blocking(move || workflow.execute(game_id, profile_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))??;
    let state = app.state::<Launches>();
    let mut status = state.status();
    status.phase = "running".into();
    status.pid = Some(pid);
    status.error = None;
    state.publish(app, status.clone());
    let _ = app.emit(events::GAMES_CHANGED, ());
    let _ = app.emit(
        events::GAME_PROCESS_CHANGED,
        GameProcessDto {
            game_id,
            state: "started".into(),
            pid: Some(pid),
            error: None,
        },
    );
    Ok(status)
}

pub fn game_argument(args: &[String]) -> Result<Option<GameId>, String> {
    let Some(index) = args.iter().position(|a| a == "--launch-game") else {
        return Ok(None);
    };
    if args
        .iter()
        .filter(|a| a.as_str() == "--launch-game")
        .count()
        != 1
    {
        return Err("Аргумент --launch-game передан несколько раз".into());
    }
    args.get(index + 1)
        .ok_or_else(|| "После --launch-game нужен UUID игры".to_owned())?
        .parse()
        .map(Some)
        .map_err(|_| "Некорректный UUID игры в ярлыке".into())
}

pub fn handle_arguments(app: tauri::AppHandle, args: Vec<String>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    tauri::async_runtime::spawn(async move {
        // A second invocation can arrive while the first instance is still in setup.
        for _ in 0..100 {
            if app.try_state::<LaunchReady>().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let result = match game_argument(&args) {
            Ok(Some(id)) if app.try_state::<LaunchReady>().is_some() => {
                begin(&app, id).await.map(|_| ())
            }
            Ok(Some(_)) => Err(ApiError::Rejected(
                "SaveHub ещё запускается. Повторите запуск ярлыка".into(),
            )),
            Ok(None) => Ok(()),
            Err(error) => Err(ApiError::Rejected(error)),
        };
        if let Err(error) = result {
            let state = app.state::<Launches>();
            let mut status = state.status();
            status.error = Some(error_message(&error));
            if status.phase == "idle" {
                status.phase = "error".into();
            }
            state.publish(&app, status);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_only_explicit_game_uuid() {
        let id = uuid::Uuid::new_v4();
        assert_eq!(
            game_argument(&["SaveHub".into(), "--launch-game".into(), id.to_string()]),
            Ok(Some(id))
        );
        assert_eq!(game_argument(&["--autostart".into()]), Ok(None));
        assert!(game_argument(&["--launch-game".into()]).is_err());
        assert!(game_argument(&["--launch-game".into(), "../../game.exe".into()]).is_err());
        assert!(
            game_argument(&[
                "--launch-game".into(),
                id.to_string(),
                "--launch-game".into(),
                id.to_string()
            ])
            .is_err()
        );
    }
}
