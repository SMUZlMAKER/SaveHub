//! Проверка обновлений при запуске и каждые шесть часов.
use crate::app::updates::Updates;
use std::time::Duration;
use tauri::Manager;

pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Updates>();
        let mut interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Первый tick немедленный; проверка при запуске управляется отдельно.
        interval.tick().await;
        let startup = state.check_preferences().check_on_startup;
        if startup && state.status().configured {
            let _ = state.check(&app).await;
        }
        loop {
            interval.tick().await;
            let enabled = state.check_preferences().check_periodically;
            if enabled && state.status().configured {
                let _ = state.check(&app).await;
            }
        }
    });
}
