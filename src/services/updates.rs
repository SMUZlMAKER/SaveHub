//! Настройки обновления отделены от ещё не реализованного каталога сохранений.
use crate::adapters::autostart;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::api::{ApiError, ApiResult};
const ENDPOINT: &str = "https://github.com/SMUZlMAKER/SaveHub/releases/latest/download/latest.json";
const PUBLIC_KEY: Option<&str> = option_env!("SAVEHUB_UPDATER_PUBLIC_KEY");

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub check_on_startup: bool,
    pub check_periodically: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            check_periodically: true,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub configured: bool,
    pub phase: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub last_checked_at: Option<u64>,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub launch_at_login: bool,
    pub check_on_startup: bool,
    pub check_periodically: bool,
}
pub struct Updates {
    preferences: Mutex<Preferences>,
    path: PathBuf,
    status: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
    operation: tokio::sync::Mutex<()>,
}
fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::Internal(e.to_string())
}
impl Updates {
    pub fn load(app: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let path = app.path().app_config_dir()?.join("updates.json");
        let preferences = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Preferences::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            preferences: Mutex::new(preferences),
            path,
            status: Mutex::new(UpdateStatus {
                current_version: app.package_info().version.to_string(),
                configured: PUBLIC_KEY.is_some_and(|key| !key.trim().is_empty()),
                phase: "idle".into(),
                version: None,
                notes: None,
                last_checked_at: None,
                error: None,
            }),
            pending: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
        })
    }
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }
    fn publish(&self, app: &tauri::AppHandle) {
        let _ = app.emit(crate::api::events::UPDATE_STATUS_CHANGED, self.status());
    }
    pub fn preferences(&self, app: &tauri::AppHandle) -> ApiResult<AppPreferences> {
        let p = self.preferences.lock().unwrap();
        Ok(AppPreferences {
            launch_at_login: autostart::is_enabled(app).map_err(internal)?,
            check_on_startup: p.check_on_startup,
            check_periodically: p.check_periodically,
        })
    }
    pub fn save(&self, app: &tauri::AppHandle, p: Preferences, launch: bool) -> ApiResult<()> {
        let mut current = self.preferences.lock().unwrap();
        let previous = autostart::is_enabled(app).map_err(internal)?;
        autostart::set_enabled(app, launch).map_err(internal)?;
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(self.path.parent().unwrap())?;
            let temporary = self.path.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_vec_pretty(&p)?)?;
            std::fs::rename(temporary, &self.path)
        })();
        if let Err(e) = result {
            let _ = autostart::set_enabled(app, previous);
            return Err(internal(e));
        }
        *current = p;
        Ok(())
    }
    pub async fn check(&self, app: &tauri::AppHandle) -> ApiResult<UpdateStatus> {
        let _operation = self.operation.try_lock().map_err(|_| {
            ApiError::Rejected("Обновление уже проверяется или устанавливается".into())
        })?;
        let Some(key) = PUBLIC_KEY.filter(|key| !key.trim().is_empty()) else {
            return Err(ApiError::Rejected(
                "В этой сборке не настроен ключ обновлений".into(),
            ));
        };
        {
            let mut s = self.status.lock().unwrap();
            s.phase = "checking".into();
            s.error = None;
        }
        self.publish(app);
        let result = async {
            app.updater_builder()
                .pubkey(key.trim())
                .endpoints(vec![ENDPOINT.parse().map_err(internal)?])
                .map_err(internal)?
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(internal)?
                .check()
                .await
                .map_err(internal)
        }
        .await;
        let mut s = self.status.lock().unwrap();
        s.last_checked_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        match result {
            Ok(update) => {
                s.version = update.as_ref().map(|u| u.version.clone());
                s.notes = update.as_ref().and_then(|u| u.body.clone());
                s.phase = if update.is_some() {
                    "available"
                } else {
                    "upToDate"
                }
                .into();
                *self.pending.lock().unwrap() = update;
            }
            Err(e) => {
                s.phase = "error".into();
                s.error = Some(format!("Не удалось проверить обновления: {e:?}"));
            }
        }
        drop(s);
        self.publish(app);
        Ok(self.status())
    }
    pub async fn install(&self, app: &tauri::AppHandle) -> ApiResult<()> {
        let _operation = self.operation.try_lock().map_err(|_| {
            ApiError::Rejected("Обновление уже проверяется или устанавливается".into())
        })?;
        let update = self
            .pending
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ApiError::Rejected("Сначала проверьте наличие обновления".into()))?;
        {
            let mut s = self.status.lock().unwrap();
            s.phase = "installing".into();
            s.error = None;
        }
        self.publish(app);
        if let Err(e) = update.download_and_install(|_, _| {}, || {}).await {
            {
                let mut s = self.status.lock().unwrap();
                s.phase = "error".into();
                s.error = Some(e.to_string());
            }
            self.publish(app);
            return Err(internal(e));
        }
        app.restart();
    }
}

pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Updates>();
        let startup = state.preferences.lock().unwrap().check_on_startup;
        if startup && state.status().configured {
            let _ = state.check(&app).await;
        }
        loop {
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
            let enabled = state.preferences.lock().unwrap().check_periodically;
            if enabled && state.status().configured {
                let _ = state.check(&app).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_allow_missing_fields_and_roundtrip() {
        let defaults: Preferences = serde_json::from_str("{}").unwrap();
        assert!(defaults.check_on_startup && defaults.check_periodically);
        let p = Preferences {
            check_on_startup: false,
            check_periodically: true,
        };
        let restored: Preferences =
            serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(!restored.check_on_startup && restored.check_periodically);
    }
}
