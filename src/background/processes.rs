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
        let mut failed = false;
        loop {
            let update = {
                let operations = app.state::<Arc<std::sync::Mutex<()>>>();
                let _guard = operations
                    .lock()
                    .expect("Repository operations lock poisoned");
                let result = processes.refresh_running_games();
                if let Ok(changes) = &result {
                    let settings = app.state::<Arc<crate::domain::Settings>>();
                    let runtime = app.state::<Arc<crate::domain::RuntimeState>>();
                    if settings.data().local_repo_path.is_some() {
                        if let Some(profile) = runtime.active_profile() {
                            let saves = app.state::<Arc<crate::services::save::SaveService>>();
                            for event in changes {
                                if let GameProcessEvent::Stopped(game) = event {
                                    let snapshot = (|| -> crate::services::ServiceResult<()> {
                                        if saves.is_bound(*game, &profile)? {
                                            saves.create_save(
                                                *game,
                                                &profile.to_string(),
                                                crate::domain::SaveKind::AfterSession,
                                            )?;
                                        }
                                        Ok(())
                                    })();
                                    if let Err(error) = snapshot {
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
                    }
                }
                result
            };
            match update {
                Ok(changes) => {
                    if failed {
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
                    failed = false;
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
                    if changes
                        .iter()
                        .any(|event| matches!(event, GameProcessEvent::Stopped(_)))
                    {
                        let _ = app.emit(events::SAVES_CHANGED, ());
                        if let Some(worker) =
                            app.try_state::<crate::background::repo::RepoWorkerHandle>()
                        {
                            let _ = worker.wake();
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
                    if !failed {
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
                    failed = true;
                }
            }
            crate::app::tray::refresh_sync_status(&app);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}
