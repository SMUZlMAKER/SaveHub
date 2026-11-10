use tauri::{Manager, WindowEvent};

use crate::api;
use crate::app::tray;

pub fn start() {
    // TODO: прочитать конфигурацию, создать хранилища и сервисы, запустить RepoWorker (см. app/mod.rs)
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("SaveHub")
                .arg("--autostart")
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            api::games::list_games,
            api::games::add_game,
            api::games::set_game_save_path,
            api::profiles::list_profiles,
            api::profiles::switch_profile,
            api::profiles::create_profile,
            api::profiles::delete_profile,
            api::snapshots::list_snapshots,
            api::snapshots::restore_snapshot,
            api::sync::get_sync_status,
            api::sync::sync_now,
            api::sync::list_conflicts,
            api::sync::resolve_conflict,
            api::settings::get_settings,
            api::settings::update_settings,
            api::settings::set_github_token,
            api::updates::get_app_preferences,
            api::updates::save_app_preferences,
            api::updates::get_update_status,
            api::updates::check_for_updates,
            api::updates::install_update,
        ])
        .setup(|app| {
            tray::setup(app.handle())?;
            app.manage(crate::app::updates::Updates::load(app.handle())?);
            if !std::env::args().any(|arg| arg == "--autostart") {
                if let Some(window) = app.get_webview_window("main") {
                    window.show()?;
                }
            }
            crate::background::updates::spawn(app.handle().clone());
            Ok(())
        })
        // Приложение живет в трее: крестик прячет окно, а не завершает процесс
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api: close, .. } = event {
                close.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить tauri-приложение");
}
