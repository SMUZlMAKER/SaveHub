use crate::api;

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
        .run(tauri::generate_context!())
        .expect("не удалось запустить tauri-приложение");
}
