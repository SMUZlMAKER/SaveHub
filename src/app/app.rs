use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use crate::adapters::config;
use crate::api;
use crate::app::tray;
use crate::background::repo::{RepoWorker, RepoWorkerOptions};
use crate::domain::{
    Catalog, LocalConfig, RuntimeState, Settings, SettingsData, SyncState, SyncStatus,
};
use crate::services::{
    game::GameService, profile::ProfileService, repo::RepoService, settings::SettingsService,
};
use crate::workflows::SyncRepository;

pub fn start() {
    crate::adapters::app_identity::initialize()
        .expect("Не удалось настроить идентификацию SaveHub в Windows");
    let worker = Arc::new(Mutex::new(None::<RepoWorker>));
    let worker_for_setup = worker.clone();
    let shutdown_complete = Arc::new(AtomicBool::new(true));

    let shutdown_for_setup = shutdown_complete.clone();

    let app = tauri::Builder::default()
        .manage(crate::app::launch::Launches::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == "--autostart") {
                crate::app::launch::handle_arguments(app.clone(), args);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("SaveHub")
                .arg("--autostart")
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            api::diagnostics::record_diagnostic,
            api::features::get_app_features,
            api::manual_saves::get_manual_save_preferences,
            api::manual_saves::get_manual_save_status,
            api::manual_saves::save_manual_save_shortcut,
            api::manual_saves::set_shortcut_recording,
            api::manual_saves::create_manual_save,
            api::launch::get_launch_status,
            api::launch::dismiss_launch_dialog,
            api::launch::create_game_shortcut,
            api::launch::begin_game_launch,
            api::launch::confirm_game_launch,
            api::launch::confirm_local_game_launch,
            api::launch::cancel_game_launch,
            api::games::list_games,
            api::games::get_catalog_status,
            api::games::sync_catalog,
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
            api::saves::list_save_history,
            api::saves::restore_save,
            api::sync::get_sync_status,
            api::sync::sync_now,
            api::sync::list_conflicts,
            api::sync::resolve_conflict,
            api::settings::get_settings,
            api::settings::update_settings,
            api::settings::connect_repository,
            api::settings::set_github_token,
            api::updates::get_app_preferences,
            api::updates::save_app_preferences,
            api::updates::get_update_status,
            api::updates::check_for_updates,
            api::updates::install_update,
        ])
        .setup(move |app| {
            if let Err(error) = app
                .path()
                .app_log_dir()
                .map_err(|error| std::io::Error::other(error.to_string()))
                .and_then(crate::adapters::diagnostics::initialize)
            {
                eprintln!("Не удалось открыть диагностический лог: {error}");
            }
            let data_dir = app.path().app_data_dir()?;
            let directory = config::DataDirectory::lock(data_dir.clone())
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            let settings_path = data_dir.join("settings.json");
            let local_config_path = data_dir.join("local_config.json");
            let default_settings = SettingsData {
                catalog_repo_url: None,
                repo_url: None,
                local_repo_path: None,
                sync_after_session: true,
                sync_while_gaming: false,
                auto_switch_profile: true,
                ask_profile_before_launch: true,
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
            if let Some(stored) = config::read_profiles(&data_dir.join("profiles.json"))
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?
            {
                profiles.clear();
                let mut branches = std::collections::HashSet::new();
                for profile in stored {
                    if profile.name.trim().is_empty()
                        || profile.branch.starts_with("savehub/")
                        || !git2::Reference::is_valid_name(&format!(
                            "refs/heads/{}",
                            profile.branch
                        ))
                        || !branches.insert(profile.branch.clone())
                        || profiles.insert(profile.id, profile).is_some()
                    {
                        return Err(std::io::Error::other(
                            "Повреждён каталог профилей: неверные или повторяющиеся записи",
                        )
                        .into());
                    }
                }
            }
            let active = config::read_active_profile(&data_dir.join("active-profile.json"))
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?
                .or_else(|| {
                    profiles
                        .contains_key(&uuid::Uuid::nil())
                        .then_some(uuid::Uuid::nil())
                });
            if active.is_some_and(|id| !profiles.contains_key(&id)) {
                return Err(
                    std::io::Error::other("Активный профиль отсутствует в каталоге").into(),
                );
            }
            let catalog = Arc::new(Catalog::new(HashMap::new(), profiles));
            let runtime = Arc::new(RuntimeState::new(
                active,
                Vec::new(),
                SyncStatus {
                    state: SyncState::NotConfigured,
                    last_sync_at: None,
                    error: None,
                },
            ));
            let operations = Arc::new(Mutex::new(()));
            app.manage(Arc::new(
                SettingsService::new(settings.clone(), settings_path, operations.clone())
                    .with_bindings(catalog.clone(), runtime.clone()),
            ));
            let game_service = Arc::new(
                GameService::persistent_in(
                    catalog.clone(),
                    runtime.clone(),
                    settings.clone(),
                    directory,
                )
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?
                .with_save_operations(operations.clone()),
            );
            let repo_service = Arc::new(
                RepoService::new(catalog.clone(), runtime.clone(), settings.clone())
                    .persistent(data_dir.clone()),
            );
            let profile_service = Arc::new(
                ProfileService::with_operations(
                    catalog.clone(),
                    runtime.clone(),
                    repo_service.clone(),
                    operations.clone(),
                )
                .persistent(data_dir.clone()),
            );
            let process_service = Arc::new(crate::services::process::ProcessService::new(
                catalog.clone(),
                runtime.clone(),
                settings.clone(),
            ));
            let saves = Arc::new(crate::services::save::SaveService::new(
                catalog.clone(),
                runtime.clone(),
                settings.clone(),
            ));
            let switch = Arc::new(crate::workflows::SwitchProfile::new(
                game_service.clone(),
                profile_service.clone(),
                saves.clone(),
                repo_service.clone(),
                process_service.clone(),
                operations.clone(),
            ));
            app.manage(Arc::new(
                crate::workflows::launch_game::LaunchGame::new(
                    process_service.clone(),
                    profile_service.clone(),
                    settings.clone(),
                )
                .with_saves(saves.clone(), switch.clone(), operations.clone()),
            ));
            app.manage(Arc::new(crate::workflows::manual_sync::ManualSync::new(
                repo_service.clone(),
                catalog.clone(),
                runtime.clone(),
                process_service.clone(),
                operations.clone(),
            )));
            app.manage(switch);
            app.manage(saves);
            app.manage(process_service);
            app.manage(game_service.clone());
            app.manage(profile_service);
            app.manage(settings);
            app.manage(repo_service.clone());
            app.manage(operations.clone());
            app.manage(catalog);
            app.manage(runtime.clone());
            tray::setup(app.handle())?;
            app.manage(crate::app::updates::Updates::load(app.handle())?);
            crate::background::processes::spawn(app.handle().clone());
            let mut sync_changes = runtime.subscribe_sync_status();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while sync_changes.changed().await.is_ok() {
                    let _ = app_handle.emit(api::events::SYNC_STATUS_CHANGED, ());
                    tray::refresh_sync_status(&app_handle);
                }
            });

            let sync = SyncRepository::new(repo_service, runtime, operations);
            let repo_worker = RepoWorker::start(sync, RepoWorkerOptions::default())?;
            app.manage(repo_worker.handle());
            *worker_for_setup.lock().expect("RepoWorker state poisoned") = Some(repo_worker);
            shutdown_for_setup.store(false, Ordering::Release);

            crate::background::updates::spawn(app.handle().clone());
            app.manage(crate::background::catalog::spawn(
                app.handle().clone(),
                game_service,
            ));
            // Hidden webviews also run frontend IPC. Create both windows only after
            // every command dependency is managed, including the background workers.
            app.manage(crate::app::manual_saves::ManualSaves::load(app.handle())?);
            for window_config in &app.config().app.windows {
                tauri::WebviewWindowBuilder::from_config(app.handle(), window_config)?.build()?;
            }
            if let Some(window) = app.get_webview_window("save-toast") {
                window.set_ignore_cursor_events(true)?;
                window.set_focusable(false)?;
            }
            app.state::<crate::app::manual_saves::ManualSaves>()
                .activate();
            app.manage(crate::app::launch::LaunchReady);
            if !std::env::args().any(|arg| arg == "--autostart" || arg == "--launch-game") {
                if let Some(window) = app.get_webview_window("main") {
                    window.show()?;
                }
            }
            let args: Vec<String> = std::env::args().collect();
            if args.iter().any(|arg| arg == "--launch-game") {
                crate::app::launch::handle_arguments(app.handle().clone(), args);
            }
            Ok(())
        })
        // Приложение живет в трее: крестик прячет окно, а не завершает процесс
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api: close, .. } = event {
                close.prevent_close();
                let _ = crate::app::launch::cancel(window.app_handle(), None);
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("не удалось запустить tauri-приложение");

    app.run(move |app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if shutdown_complete.load(Ordering::Acquire) {
                return;
            }

            api.prevent_exit();
            let repo_worker = worker.lock().expect("RepoWorker state poisoned").take();
            let Some(mut repo_worker) = repo_worker else {
                // None also means another exit request is already joining the worker.
                return;
            };
            let app = app.clone();
            let shutdown_complete = shutdown_complete.clone();
            let worker = worker.clone();

            crate::app::diagnostics::spawn_blocking(move || {
                loop {
                    match repo_worker.shutdown() {
                        Ok(()) => break,
                        Err(error) => {
                            eprintln!("Не удалось завершить синхронизацию: {error:?}");
                            match shutdown_decision(&app) {
                                ShutdownDecision::Retry => continue,
                                ShutdownDecision::Exit => break,
                                ShutdownDecision::Stay => {
                                    *worker.lock().expect("RepoWorker state poisoned") =
                                        Some(repo_worker);
                                    return;
                                }
                            }
                        }
                    }
                }

                drop(repo_worker);
                shutdown_complete.store(true, Ordering::Release);
                if code == Some(tauri::RESTART_EXIT_CODE) {
                    app.request_restart();
                } else {
                    app.exit(code.unwrap_or(0));
                }
            });
        }
    });
}

enum ShutdownDecision {
    Retry,
    Exit,
    Stay,
}

fn shutdown_decision(app: &tauri::AppHandle) -> ShutdownDecision {
    const RETRY: &str = "Повторить";
    const EXIT: &str = "Выйти без синхронизации";
    let result = app.dialog()
        .message("Не удалось завершить синхронизацию сохранений. Можно повторить попытку или остаться в приложении. При выходе сейчас последние изменения могут остаться без контрольной точки и отправки на другой ПК.")
        .title("Синхронизация не завершена")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom(RETRY.into(), EXIT.into(), "Остаться".into()))
        .blocking_show_with_result();
    match result {
        MessageDialogResult::Custom(label) if label == RETRY => ShutdownDecision::Retry,
        MessageDialogResult::Custom(label) if label == EXIT => ShutdownDecision::Exit,
        _ => ShutdownDecision::Stay,
    }
}
