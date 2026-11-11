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
            match processes.refresh_running_games() {
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
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}
