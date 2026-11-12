//! Tauri orchestration: updates -> profile policy -> workflow -> executable.
use crate::{
    api::{
        ApiError, ApiResult, events,
        launch::{GameProcessDto, LaunchProfileDto, LaunchStatus},
    },
    domain::{GameId, ProfileId},
    services::game::GameService,
    workflows::launch_game::LaunchGame,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager};

pub struct LaunchReady;

#[derive(Default)]
pub struct Launches {
    status: Mutex<LaunchStatus>,
    pub operation: tokio::sync::Mutex<()>,
    dialog_mode: AtomicBool,
    cancel_requested: AtomicBool,
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
    pub fn publish(&self, app: &tauri::AppHandle, mut status: LaunchStatus) {
        {
            let mut current = self.status.lock().unwrap();
            if self.cancel_requested.load(Ordering::SeqCst)
                && matches!(
                    status.phase.as_str(),
                    "preparing" | "awaitingProfile" | "awaitingLocalLaunch"
                )
            {
                status.phase = "cancelled".into();
            }
            *current = status.clone();
        }
        let finished = matches!(status.phase.as_str(), "running" | "cancelled");
        let _ = app.emit(events::LAUNCH_STATUS_CHANGED, status);
        if finished && self.dialog_mode.load(Ordering::SeqCst) {
            if let Some(window) = app.get_webview_window("launch") {
                let _ = window.hide();
            }
        }
    }
    fn starting(&self, app: &tauri::AppHandle) -> bool {
        let next = {
            let mut status = self.status.lock().unwrap();
            if self.cancel_requested.load(Ordering::SeqCst) {
                return false;
            }
            status.phase = "starting".into();
            status.error = None;
            status.clone()
        };
        let _ = app.emit(events::LAUNCH_STATUS_CHANGED, next);
        true
    }
}
fn error_message(error: &ApiError) -> String {
    match error {
        ApiError::Rejected(s) | ApiError::Internal(s) => s.clone(),
        ApiError::NotImplemented(s) => format!("Пока не реализовано: {s}"),
    }
}

pub async fn begin(app: &tauri::AppHandle, game_id: GameId) -> ApiResult<LaunchStatus> {
    begin_internal(app, game_id, true).await
}
async fn begin_internal(
    app: &tauri::AppHandle,
    game_id: GameId,
    dialog: bool,
) -> ApiResult<LaunchStatus> {
    let state = app.state::<Launches>();
    let _guard = state
        .operation
        .try_lock()
        .map_err(|_| ApiError::Rejected("Другой запуск уже подготавливается".into()))?;
    if matches!(
        state.status().phase.as_str(),
        "awaitingProfile" | "awaitingLocalLaunch"
    ) {
        return Err(ApiError::Rejected(
            "Сначала завершите или отмените текущий запуск".into(),
        ));
    }
    state.cancel_requested.store(false, Ordering::SeqCst);
    state.dialog_mode.store(dialog, Ordering::SeqCst);
    let status = LaunchStatus {
        request_id: Some(uuid::Uuid::new_v4()),
        game_id: Some(game_id),
        phase: "preparing".into(),
        ..Default::default()
    };
    state.publish(app, status);
    let result = prepare(app, game_id).await;
    if state.cancel_requested.load(Ordering::SeqCst) {
        return Ok(state.status());
    }
    if let Err(error) = &result {
        let mut status = state.status();
        status.phase = "error".into();
        status.error = Some(error_message(error));
        state.publish(app, status);
        show_dialog(app)?;
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
    let workflow = app.state::<Arc<LaunchGame>>().inner().clone();
    let games = app.state::<Arc<GameService>>().inner().clone();
    let (plan, game) = tauri::async_runtime::spawn_blocking(move || {
        games.restore_launcher_registration(game_id, &directory)?;
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
    let updates = app.state::<crate::app::updates::Updates>();
    if updates.check_preferences().check_before_game_launch && updates.status().configured {
        // Optional application maintenance, separate from save synchronization.
        // The updater publishes its own status; a failed check must not block a game.
        let _ = updates.check(app).await;
    }
    if state.cancel_requested.load(Ordering::SeqCst) {
        return Ok(state.status());
    }
    if plan.needs_profile {
        status.phase = "awaitingProfile".into();
        state.publish(app, status);
        if state.dialog_mode.load(Ordering::SeqCst) && state.status().phase == "awaitingProfile" {
            show_dialog(app)?;
        }
        Ok(state.status())
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

pub async fn confirm_local(
    app: &tauri::AppHandle,
    request_id: uuid::Uuid,
) -> ApiResult<LaunchStatus> {
    let state = app.state::<Launches>();
    let _guard = state
        .operation
        .try_lock()
        .map_err(|_| ApiError::Rejected("Запуск уже выполняется".into()))?;
    let mut status = state.status();
    if status.request_id != Some(request_id) || status.phase != "awaitingLocalLaunch" {
        return Err(ApiError::Rejected(
            "Запрос локального запуска уже изменился".into(),
        ));
    }
    let profile_id = status
        .active_profile_id
        .ok_or_else(|| ApiError::Rejected("Нет активного профиля".into()))?;
    match execute_with_policy(app, status.game_id.unwrap(), profile_id, true).await {
        Ok(status) => Ok(status),
        Err(error) => {
            status.phase = "awaitingLocalLaunch".into();
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
    execute_with_policy(app, game_id, profile_id, false).await
}

async fn execute_with_policy(
    app: &tauri::AppHandle,
    game_id: GameId,
    profile_id: ProfileId,
    local_only: bool,
) -> ApiResult<LaunchStatus> {
    let state = app.state::<Launches>();
    if !state.starting(app) {
        return Ok(state.status());
    }
    let workflow = app.state::<Arc<LaunchGame>>().inner().clone();
    let launch_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let proceed = || {
            !launch_app
                .state::<Launches>()
                .cancel_requested
                .load(Ordering::SeqCst)
        };
        if local_only {
            workflow.execute_guarded_locally(game_id, profile_id, proceed)
        } else {
            workflow.execute_guarded(game_id, profile_id, proceed)
        }
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    for event in [
        crate::api::events::PROFILE_CHANGED,
        crate::api::events::SAVES_CHANGED,
        crate::api::events::CONFLICTS_CHANGED,
    ] {
        let _ = app.emit(event, ());
    }
    let pid = match result {
        Ok(pid) => pid,
        Err(crate::services::ServiceError::RemoteUnavailable(_)) if !local_only => {
            let mut status = state.status();
            status.phase = "awaitingLocalLaunch".into();
            status.error = None;
            status.warnings = vec![
                "Не удалось связаться с репозиторием. Можно запустить игру с сохранениями на этом ПК."
                    .into(),
            ];
            state.publish(app, status);
            let status = state.status();
            if status.phase == "awaitingLocalLaunch" && state.dialog_mode.load(Ordering::SeqCst) {
                show_dialog(app)?;
            }
            return Ok(status);
        }
        Err(error) => return Err(error.into()),
    };
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

fn show_dialog(app: &tauri::AppHandle) -> ApiResult<()> {
    let window = app
        .get_webview_window("launch")
        .ok_or_else(|| ApiError::Internal("Диалог запуска недоступен".into()))?;
    window
        .show()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    window
        .unminimize()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    window
        .set_focus()
        .map_err(|e| ApiError::Internal(e.to_string()))
}

/// The state lock makes cancellation atomic with the transition to OS spawn.
/// Cancelling preparation never starts a game after a delayed update response.
pub fn cancel(app: &tauri::AppHandle, request_id: Option<uuid::Uuid>) -> ApiResult<()> {
    let state = app.state::<Launches>();
    let next = {
        let mut status = state.status.lock().unwrap();
        if request_id.is_some() && status.request_id != request_id {
            return Err(ApiError::Rejected("Запрос запуска уже изменился".into()));
        }
        if matches!(
            status.phase.as_str(),
            "preparing" | "awaitingProfile" | "awaitingLocalLaunch"
        ) {
            state.cancel_requested.store(true, Ordering::SeqCst);
            status.phase = "cancelled".into();
            status.error = None;
        } else if status.phase == "starting" && request_id.is_none() {
            // OS spawn may already be in flight. If it fails, do not reopen a
            // profile picker that the user has just dismissed.
            state.cancel_requested.store(true, Ordering::SeqCst);
        } else if request_id.is_some() {
            return Err(ApiError::Rejected("Запрос запуска уже изменился".into()));
        }
        status.clone()
    };
    state.publish(app, next);
    Ok(())
}

pub fn handle_arguments(app: tauri::AppHandle, args: Vec<String>) {
    if !args.iter().any(|arg| arg == "--launch-game") {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        return;
    }
    tauri::async_runtime::spawn(async move {
        for _ in 0..100 {
            if app.try_state::<LaunchReady>().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let result = match game_argument(&args) {
            Ok(Some(id)) if app.try_state::<LaunchReady>().is_some() => {
                begin_internal(&app, id, true).await.map(|_| ())
            }
            Ok(Some(_)) => Err(ApiError::Rejected(
                "SaveHub ещё запускается. Повторите запуск ярлыка".into(),
            )),
            Ok(None) => Ok(()),
            Err(error) => Err(ApiError::Rejected(error)),
        };
        if let Err(error) = result {
            let state = app.state::<Launches>();
            state.dialog_mode.store(true, Ordering::SeqCst);
            let mut status = state.status();
            status.error = Some(error_message(&error));
            if status.phase == "idle" {
                status.phase = "error".into();
            }
            state.publish(&app, status);
            let _ = show_dialog(&app);
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
