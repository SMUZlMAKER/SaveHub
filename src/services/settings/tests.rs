use super::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Barrier, mpsc},
    thread,
    time::Duration,
};

struct Fixture {
    directory: PathBuf,
    settings: Arc<Settings>,
    operations: Arc<Mutex<()>>,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("savehub-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        git2::Repository::init(directory.join("saves")).unwrap();
        Self {
            directory,
            operations: Arc::new(Mutex::new(())),
            settings: Arc::new(Settings::new(
                SettingsData {
                    catalog_repo_url: None,
                    repo_url: None,
                    local_repo_path: None,
                    sync_after_session: false,
                    sync_while_gaming: false,
                    auto_switch_profile: false,
                    ask_profile_before_launch: true,
                    start_minimized: false,
                },
                LocalConfig {
                    machine_id: "test".into(),
                    games: HashMap::new(),
                },
            )),
        }
    }

    fn service(&self) -> SettingsService {
        SettingsService::new(
            self.settings.clone(),
            self.directory.join("settings.json"),
            self.operations.clone(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn failed_save_preserves_the_entire_in_memory_configuration() {
    let fixture = Fixture::new();
    let before = serde_json::to_value(fixture.settings.data()).unwrap();
    std::fs::create_dir(fixture.directory.join("settings.json")).unwrap();

    assert!(
        fixture
            .service()
            .update(|data| {
                data.repo_url = Some("https://github.com/test/saves".into());
                data.local_repo_path = Some(fixture.directory.join("saves"));
                data.sync_after_session = true;
            })
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(fixture.settings.data()).unwrap(),
        before
    );
}

#[test]
fn repository_change_waits_for_the_current_operation() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let repository_path = fixture.directory.join("saves");
    let guard = fixture.operations.lock().unwrap();
    let (started, start) = mpsc::channel();
    let (finished, finish) = mpsc::channel();
    let writer = thread::spawn(move || {
        started.send(()).unwrap();
        finished
            .send(service.set_repository(
                Some("https://github.com/test/saves".into()),
                Some(repository_path),
            ))
            .unwrap();
    });

    start.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(finish.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(fixture.settings.data().repo_url.is_none());
    drop(guard);
    finish
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    writer.join().unwrap();
    assert_eq!(
        fixture.settings.data().repo_url.as_deref(),
        Some("https://github.com/test/saves")
    );
}

#[test]
fn concurrent_changes_preserve_other_fields_on_disk_and_in_memory() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    thread::scope(|scope| {
        for field in 0..2 {
            let service = fixture.service();
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                if field == 0 {
                    service.set_sync_after_session(true)
                } else {
                    service.set_auto_switch_profile(true)
                }
                .unwrap();
            });
        }
    });

    let memory = fixture.settings.data();
    let disk = crate::adapters::config::read_settings(&fixture.directory.join("settings.json"))
        .unwrap()
        .unwrap();
    assert!(memory.sync_after_session && memory.auto_switch_profile);
    assert_eq!(
        serde_json::to_value(memory).unwrap(),
        serde_json::to_value(disk).unwrap()
    );
}
