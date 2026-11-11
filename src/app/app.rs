use std::{collections::HashMap, sync::Arc};
use tauri::{Manager, WindowEvent};

use crate::adapters::config;
use crate::api;
use crate::app::tray;
use crate::domain::{
    Catalog, LocalConfig, RuntimeState, Settings, SettingsData, SyncState, SyncStatus,
};
use crate::services::{game::GameService, profile::ProfileService, settings::SettingsService};

pub fn start() {
    // TODO: прочитать конфигурацию, создать хранилища и сервисы, запустить RepoWorker (см. app/mod.rs)
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("SaveHub")
                .arg("--autostart")
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            api::games::list_games,
            api::games::get_game_status,
            api::games::add_game,
            api::games::update_game,
            api::games::resolve_game_paths,
            api::games::validate_game_paths,
            api::games::suggest_save_dirs,
            api::profiles::list_profiles,
            api::profiles::current_profile,
            api::profiles::switch_profile,
            api::profiles::create_profile,
            api::profiles::delete_profile,
            api::saves::list_saves,
            api::saves::choose_save,
            api::saves::get_save_conflict,
            api::saves::resolve_save_conflict,
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
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let settings_path = data_dir.join("settings.json");
            let local_config_path = data_dir.join("local_config.json");
            let default_settings = SettingsData {
                repo_url: None,
                local_repo_path: None,
                sync_after_session: true,
                sync_while_gaming: false,
                auto_switch_profile: true,
                start_minimized: false,
            };
            let settings_data = config::read_settings(&settings_path)
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?
                .unwrap_or(default_settings);
            if !settings_path.exists() {
                config::write_settings(&settings_path, &settings_data)
                    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            }
            let local_config = config::read_local_config(&local_config_path)
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?
                .unwrap_or_else(|| LocalConfig {
                    machine_id: uuid::Uuid::new_v4().to_string(),
                    games: HashMap::new(),
                });
            if !local_config_path.exists() {
                config::write_local_config(&local_config_path, &local_config)
                    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            }
            let settings = Arc::new(Settings::new(settings_data, local_config));
            let mut profiles = HashMap::new();
            profiles.insert(
                uuid::Uuid::nil(),
                crate::domain::Profile {
                    id: uuid::Uuid::nil(),
                    name: "Основной".into(),
                    branch: "main".into(),
                    last_played_game: None,
                },
            );
            let catalog = Arc::new(Catalog::new(HashMap::new(), profiles));
            let runtime = Arc::new(RuntimeState::new(
                Some(uuid::Uuid::nil()),
                Vec::new(),
                SyncStatus {
                    state: SyncState::NotConfigured,
                    last_sync_at: None,
                    error: None,
                },
            ));
            app.manage(Arc::new(SettingsService::new(
                settings.clone(),
                settings_path,
            )));
            let game_service = Arc::new(GameService::new(
                catalog.clone(),
                runtime.clone(),
                settings.clone(),
            ));
            let profile_service = Arc::new(ProfileService::new(
                catalog.clone(),
                runtime.clone(),
                settings.clone(),
            ));
            app.manage(game_service);
            app.manage(profile_service);
            app.manage(settings);
            app.manage(catalog);
            app.manage(runtime);
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
