use tauri::WindowEvent;

use crate::api;
use crate::app::tray;

pub fn start() {
    // TODO: прочитать конфигурацию, создать хранилища и сервисы, запустить RepoWorker (см. app/mod.rs)
    tauri::Builder::default()
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
        ])
        .setup(|app| {
            tray::setup(app.handle())?;
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
