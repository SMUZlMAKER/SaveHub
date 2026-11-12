//! Poll the OS away from the UI thread and publish domain process transitions.
use crate::{
    api::{events, launch::GameProcessDto},
    app::launch::Launches,
    domain::{Catalog, GameProcessEvent},
    services::process::ProcessService,
};
use std::{sync::Arc, time::Duration};
use tauri::{Emitter, Manager};

pub fn spawn(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let processes = app.state::<Arc<ProcessService>>().inner().clone();
        let mut previous_poll = None;
        loop {
            let mut saves_changed = false;
            let update = {
                let operations = app.state::<Arc<std::sync::Mutex<()>>>();
                let _guard = operations
                    .lock()
                    .expect("Repository operations lock poisoned");
                let result = processes.refresh_running_games();
                if result.is_ok() {
                    let settings = app.state::<Arc<crate::domain::Settings>>();
                    let runtime = app.state::<Arc<crate::domain::RuntimeState>>();
                    let saves = app.state::<Arc<crate::services::save::SaveService>>();
                    for (game, profile) in runtime.pending_sessions() {
                        let snapshot = (|| -> crate::services::ServiceResult<()> {
                            if settings.data().local_repo_path.is_some() {
                                saves_changed |= saves.capture_session(game, profile)?.is_some();
                            }
                            Ok(())
                        })();
                        match snapshot {
                            Ok(()) => {
                                runtime.acknowledge_session(game, profile);
                            }
                            Err(error) => {
                                let mut status = runtime.sync_status();
                                status.state = crate::domain::SyncState::Error;
                                status.error = Some(format!(
                                    "Не удалось сохранить завершённую сессию: {error:?}"
                                ));
                                runtime.set_sync_status(status);
                            }
                        }
                    }
                }
                result
            };
            match update {
                Ok(changes) => {
                    let session_ended = changes
                        .iter()
                        .any(|event| matches!(event, GameProcessEvent::Stopped(_)));
                    if publish_poll_state(previous_poll, true) {
                        let _ = app.emit(events::GAMES_CHANGED, ());
                        app.state::<Launches>().monitor_status(&app, None);
                        let running = processes.running_games().unwrap_or_default();
                        for id in app.state::<Arc<Catalog>>().games().keys() {
                            let _ = app.emit(
                                events::GAME_PROCESS_CHANGED,
                                GameProcessDto {
                                    game_id: *id,
                                    state: if running.contains(id) {
                                        "started"
                                    } else {
                                        "stopped"
                                    }
                                    .into(),
                                    pid: None,
                                    error: None,
                                },
                            );
                        }
                    }
                    previous_poll = Some(true);
                    // Reconcile even if a very short-lived child exited before the
                    // launch command finished publishing its status.
                    let launches = app.state::<Launches>();
                    if let Some(id) = launches.status().game_id {
                        if processes
                            .running_games()
                            .is_some_and(|ids| !ids.contains(&id))
                        {
                            launches.mark_stopped(&app, id);
                        }
                    }
                    if saves_changed || session_ended {
                        let _ = app.emit(events::SAVES_CHANGED, ());
                        if let Some(worker) =
                            app.try_state::<crate::background::repo::RepoWorkerHandle>()
                        {
                            let sync_after_session = app
                                .state::<Arc<crate::domain::Settings>>()
                                .data()
                                .sync_after_session;
                            if session_ended && sync_after_session {
                                let _ = worker.sync_now();
                            } else {
                                let _ = worker.wake();
                            }
                        }
                    }
                    for event in changes {
                        let (game_id, state) = match event {
                            GameProcessEvent::Started(id) => (id, "started"),
                            GameProcessEvent::Stopped(id) => (id, "stopped"),
                        };
                        let _ = app.emit(
                            events::GAME_PROCESS_CHANGED,
                            GameProcessDto {
                                game_id,
                                state: state.into(),
                                pid: None,
                                error: None,
                            },
                        );
                        let _ = app.emit(events::GAMES_CHANGED, ());
                    }
                }
                Err(error) => {
                    if publish_poll_state(previous_poll, false) {
                        app.state::<Launches>().monitor_status(
                            &app,
                            Some(format!("Не удалось опросить процессы: {error:?}")),
                        );
                        for game_id in app.state::<Arc<Catalog>>().games().keys() {
                            let _ = app.emit(
                                events::GAME_PROCESS_CHANGED,
                                GameProcessDto {
                                    game_id: *game_id,
                                    state: "unknown".into(),
                                    pid: None,
                                    error: Some(format!("Не удалось опросить процессы: {error:?}")),
                                },
                            );
                        }
                        let _ = app.emit(events::GAMES_CHANGED, ());
                    }
                    previous_poll = Some(false);
                }
            }
            crate::app::tray::refresh_sync_status(&app);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

// Both the initial result and transitions must reach UI; unchanged polls stay quiet.
fn publish_poll_state(previous: Option<bool>, success: bool) -> bool {
    previous != Some(success)
}

#[cfg(test)]
mod tests {
    use super::publish_poll_state;

    #[test]
    fn publishes_initial_results_and_recovery_without_repeating_stable_polls() {
        assert!(publish_poll_state(None, true));
        assert!(publish_poll_state(None, false));
        assert!(publish_poll_state(Some(false), true));
        assert!(publish_poll_state(Some(true), false));
        assert!(!publish_poll_state(Some(true), true));
        assert!(!publish_poll_state(Some(false), false));
    }
}
