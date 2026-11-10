pub fn start() {
    // TODO: прочитать конфигурацию, создать хранилища и сервисы, запустить RepoWorker (см. app/mod.rs)
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("не удалось запустить tauri-приложение");
}
