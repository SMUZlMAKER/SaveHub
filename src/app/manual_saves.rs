//! Integration for the button, global shortcut and passive notification window.
use crate::{
    adapters::{fs, hotkey},
    api::{
        ApiError, ApiResult, events,
        manual_saves::{ManualSavePreferences, ManualSaveStatus},
    },
    domain::{Catalog, GameId, RuntimeState},
    services::{process::ProcessService, save::SaveService},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager};

pub struct ManualSaves {
    preferences: Mutex<ManualSavePreferences>,
    path: PathBuf,
    keyboard: hotkey::Hotkey,
    status: Mutex<ManualSaveStatus>,
    configure: Mutex<()>,
    recording_shortcut: AtomicBool,
}
impl ManualSaves {
    pub fn load(app: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let path = app.path().app_config_dir()?.join("manual-saves.json");
        let preferences = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ManualSavePreferences {
                shortcut: "Ctrl+Alt+F9".into(),
                error: None,
            },
            Err(e) => return Err(e.into()),
        };
        let handle = app.clone();
        let keyboard = hotkey::Hotkey::start(move || {
            let app = handle.clone();
            tauri::async_runtime::spawn(async move {
                let shortcut = {
                    let state = app.state::<ManualSaves>();
                    state
                        .recording_shortcut
                        .load(Ordering::SeqCst)
                        .then(|| state.preferences().shortcut)
                };
                if let Some(shortcut) = shortcut {
                    if app
                        .get_webview_window("main")
                        .is_some_and(|window| window.is_focused().unwrap_or(false))
                    {
                        let _ = app.emit("shortcut-recorded", shortcut);
                        return;
                    }
                }
                let _ = capture(app, None).await;
            });
        });
        Ok(Self {
            preferences: Mutex::new(preferences),
            path,
            keyboard,
            configure: Mutex::new(()),
            recording_shortcut: AtomicBool::new(false),
            status: Mutex::new(ManualSaveStatus {
                sequence: 0,
                success: true,
                message: String::new(),
            }),
        })
    }
    pub fn set_shortcut_recording(&self, recording: bool) {
        self.recording_shortcut.store(recording, Ordering::SeqCst);
    }
    pub fn activate(&self) {
        let value = self.preferences().shortcut;
        if let Err(error) = hotkey::parse(&value).and_then(|key| self.keyboard.set(key)) {
            self.preferences.lock().unwrap().error = Some(error);
        }
    }
    pub fn preferences(&self) -> ManualSavePreferences {
        self.preferences.lock().unwrap().clone()
    }
    pub fn status(&self) -> ManualSaveStatus {
        self.status.lock().unwrap().clone()
    }
    pub fn save(&self, shortcut: String) -> ApiResult<()> {
        let parsed = hotkey::parse(&shortcut).map_err(ApiError::Rejected)?;
        let _guard = self.configure.lock().unwrap();
        let previous = self.preferences();
        self.keyboard.set(parsed).map_err(ApiError::Rejected)?;
        let next = ManualSavePreferences {
            shortcut: shortcut.trim().to_owned(),
            error: None,
        };
        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(self.path.parent().unwrap()).map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
            fs::write_atomic(&self.path, &bytes).map_err(|e| format!("{e:?}"))
        })();
        if let Err(error) = result {
            if let Err(rollback) =
                hotkey::parse(&previous.shortcut).and_then(|key| self.keyboard.set(key))
            {
                self.preferences.lock().unwrap().error = Some(rollback);
            }
            return Err(ApiError::Internal(error));
        }
        *self.preferences.lock().unwrap() = next;
        Ok(())
    }
    fn publish(&self, app: &tauri::AppHandle, success: bool, message: String) {
        let next = {
            let mut status = self.status.lock().unwrap();
            *status = ManualSaveStatus {
                sequence: status.sequence + 1,
                success,
                message,
            };
            status.clone()
        };
        let _ = app.emit("manual-save-status", &next);
        if let Some(window) = app.get_webview_window("save-toast") {
            // No focus, activation or cursor interception: even failures remain passive.
            if window.set_ignore_cursor_events(true).is_ok() {
                let _ = window.show();
            }
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            if app.state::<ManualSaves>().status().sequence == next.sequence {
                if let Some(window) = app.get_webview_window("save-toast") {
                    let _ = window.hide();
                }
            }
        });
    }
}

pub async fn capture(app: tauri::AppHandle, game: Option<GameId>) -> ApiResult<()> {
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> ApiResult<String> {
        let operations = worker_app.state::<Arc<Mutex<()>>>();
        let _guard = operations.try_lock().map_err(|_| ApiError::Rejected("SaveHub занят другой операцией. Повторите ручной снимок".into()))?;
        worker_app.state::<Arc<ProcessService>>().refresh_running_games()?;
        let runtime = worker_app.state::<Arc<RuntimeState>>();
        let running = runtime.running_games().ok_or_else(|| ApiError::Rejected("Состояние процессов неизвестно".into()))?;
        let game = match game {
            Some(id) if running.contains(&id) => id,
            Some(_) => return Err(ApiError::Rejected("Игра уже завершена".into())),
            None if running.len() == 1 => running[0],
            None => return Err(ApiError::Rejected("Выберите запущенную игру. Автоматический выбор доступен, если запущена ровно одна игра".into())),
        };
        worker_app.state::<Arc<SaveService>>().capture_manual(game)?;
        let name = worker_app.state::<Arc<Catalog>>().games().get(&game).map(|g| g.name.clone()).unwrap_or_else(|| "Игра".into());
        Ok(format!("{name}: ручной снимок сохранён"))
    }).await.map_err(|e| ApiError::Internal(e.to_string())).and_then(|result| result);
    let _ = app.emit(events::GAMES_CHANGED, ());
    match &result {
        Ok(message) => {
            app.state::<ManualSaves>()
                .publish(&app, true, message.clone());
            let _ = app.emit(events::SAVES_CHANGED, ());
        }
        Err(error) => {
            let message = match error {
                ApiError::Rejected(s) | ApiError::Internal(s) => s.clone(),
                ApiError::NotImplemented(s) => s.to_string(),
            };
            app.state::<ManualSaves>()
                .publish(&app, false, format!("Снимок не создан: {message}"));
        }
    }
    result.map(|_| ())
}
