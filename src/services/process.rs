use super::{ServiceError, ServiceResult};
use crate::{
    adapters::{config, process},
    domain::*,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex},
};

pub struct ProcessService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
    /// Serializes OS refresh, duplicate detection and spawn, including the worker.
    operation: Mutex<()>,
    children: Mutex<HashMap<GameId, Child>>,
}

impl ProcessService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            settings,
            operation: Mutex::new(()),
            children: Mutex::new(HashMap::new()),
        }
    }

    pub fn refresh_running_games(&self) -> ServiceResult<Vec<GameProcessEvent>> {
        let _guard = self.operation.lock().unwrap();
        self.refresh()
    }

    fn refresh(&self) -> ServiceResult<Vec<GameProcessEvent>> {
        let processes = match process::list() {
            Ok(processes) => processes,
            Err(error) => {
                self.runtime.invalidate_process_state();
                return Err(ServiceError::Adapter(error));
            }
        };
        let mut running = Vec::new();
        let mut children = self.children.lock().unwrap();
        let mut failure = None;
        children.retain(|id, child| match child.try_wait() {
            Ok(None) => {
                running.push(*id);
                true
            }
            Ok(Some(_)) => false,
            Err(error) => {
                failure = Some(error);
                true
            }
        });
        if let Some(error) = failure {
            self.runtime.invalidate_process_state();
            return Err(ServiceError::Adapter(crate::adapters::AdapterError::Io(
                error,
            )));
        }
        let local = self.settings.local();
        for game in self.catalog.games().values() {
            let paths = effective_paths(game, &local);
            if let Some(paths) = paths {
                let exe = crate::adapters::fs::expand_path(&paths.exe_path)
                    .map_err(ServiceError::Adapter)?;
                if processes
                    .iter()
                    .any(|p| matches_process(&exe, &game.process_names, p))
                    && !running.contains(&game.id)
                {
                    running.push(game.id);
                }
            }
        }
        Ok(self.runtime.set_running_games(running))
    }

    pub fn running_games(&self) -> Option<Vec<GameId>> {
        self.runtime.running_games()
    }

    pub fn launch(&self, game_id: GameId) -> ServiceResult<u32> {
        let _guard = self.operation.lock().unwrap();
        self.refresh()?;
        if self
            .runtime
            .running_games()
            .is_some_and(|ids| ids.contains(&game_id))
        {
            return Err(ServiceError::GameRunning(vec![game_id]));
        }
        let game = self.game(game_id)?;
        let paths = effective_paths(&game, &self.settings.local())
            .ok_or_else(|| ServiceError::InvalidInput("Игра игнорируется на этом ПК".into()))?;
        let (exe_error, save_error) = crate::adapters::game_paths::validate(&paths);
        if let Some(error) = exe_error.or(save_error) {
            return Err(ServiceError::InvalidInput(error));
        }
        let executable =
            crate::adapters::fs::expand_path(&paths.exe_path).map_err(ServiceError::Adapter)?;
        let child =
            process::spawn(&executable, &[], executable.parent()).map_err(ServiceError::Adapter)?;
        let pid = child.id();
        self.children.lock().unwrap().insert(game_id, child);
        let mut running = self.runtime.running_games().unwrap_or_default();
        running.push(game_id);
        self.runtime.set_running_games(running);
        Ok(pid)
    }

    fn game(&self, id: GameId) -> ServiceResult<Game> {
        self.catalog
            .games()
            .remove(&id)
            .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))
    }

    /// Persist only a local launch registration. Does not publish or modify saves.
    /// It lets the shortcut work after SaveHub restarts, while the main catalog is in memory.
    pub fn create_shortcut(&self, game_id: GameId, registrations: &Path) -> ServiceResult<PathBuf> {
        if !cfg!(windows) {
            return Err(ServiceError::InvalidInput(
                "Ярлыки .lnk поддерживаются только в Windows".into(),
            ));
        }
        let mut game = self.game(game_id)?;
        game.paths = effective_paths(&game, &self.settings.local()).ok_or_else(|| {
            ServiceError::InvalidInput("Сначала подключите игру на этом ПК".into())
        })?;
        let (exe, saves) = crate::adapters::game_paths::validate(&game.paths);
        if let Some(error) = exe.or(saves) {
            return Err(ServiceError::InvalidInput(error));
        }
        let desktop = dirs::desktop_dir()
            .ok_or_else(|| ServiceError::InvalidInput("Рабочий стол не найден".into()))?;
        let destination = desktop.join(shortcut_filename(&game.name, game.id));
        if destination.exists() {
            return Err(ServiceError::Conflict(format!(
                "Ярлык уже существует: {}",
                destination.display()
            )));
        }
        config::write_games(
            &registrations.join(format!("{game_id}.json")),
            &[game.clone()],
        )
        .map_err(ServiceError::Adapter)?;
        let savehub = std::env::current_exe()
            .map_err(|e| ServiceError::Adapter(crate::adapters::AdapterError::Io(e)))?;
        let icon = crate::adapters::fs::expand_path(&game.paths.exe_path)
            .map_err(ServiceError::Adapter)?;
        process::create_shortcut(
            &destination,
            &savehub,
            &["--launch-game".into(), game_id.to_string()],
            savehub.parent(),
            &icon,
        )
        .map_err(ServiceError::Adapter)?;
        Ok(destination)
    }

    pub fn restore_registration(&self, game_id: GameId, registrations: &Path) -> ServiceResult<()> {
        if self.catalog.games().contains_key(&game_id) {
            return Ok(());
        }
        let games = config::read_games(&registrations.join(format!("{game_id}.json")))
            .map_err(ServiceError::Adapter)?
            .ok_or_else(|| {
                ServiceError::NotFound(
                    "Регистрация ярлыка не найдена. Создайте ярлык заново в SaveHub".into(),
                )
            })?;
        let game = games
            .into_iter()
            .find(|g| g.id == game_id)
            .ok_or_else(|| ServiceError::InvalidInput("Некорректная регистрация ярлыка".into()))?;
        self.catalog.add_game(game);
        Ok(())
    }
}

fn effective_paths(game: &Game, local: &LocalConfig) -> Option<GamePaths> {
    match local.games.get(&game.id) {
        Some(PathResolution::Ignore) => None,
        Some(PathResolution::UseLocal(paths) | PathResolution::UpdateRepository(paths)) => {
            Some(paths.clone())
        }
        None => Some(game.paths.clone()),
    }
}

fn matches_process(exe: &Path, names: &[String], process: &process::ProcessInfo) -> bool {
    if let Some(path) = &process.executable {
        let normalize = |p: &Path| {
            let path = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            if cfg!(windows) {
                path.to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_lowercase()
            } else {
                path.to_string_lossy().into_owned()
            }
        };
        if normalize(exe) == normalize(path) {
            return true;
        }
        // Do not confuse two accessible executables with the same file name.
        return false;
    }
    exe.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&process.name))
        || names.iter().any(|n| n.eq_ignore_ascii_case(&process.name))
}

fn shortcut_filename(name: &str, id: GameId) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .take(60)
        .collect();
    format!(
        "SaveHub - {} ({id}).lnk",
        clean.trim_end_matches([' ', '.'])
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_name_cannot_escape_desktop_or_use_reserved_names() {
        let name = shortcut_filename("../CON:\\bad\"|?*\n", uuid::Uuid::nil());
        assert_eq!(Path::new(&name).components().count(), 1);
        assert!(name.starts_with("SaveHub - "));
        assert!(!name.contains(['/', '\\', ':', '"', '|', '?', '*', '\n']));
    }
    #[test]
    fn distinguishes_executable_paths_and_falls_back_for_protected_processes() {
        let mut p = process::ProcessInfo {
            pid: 1,
            name: "game.exe".into(),
            executable: Some(PathBuf::from("/other/game.exe")),
        };
        assert!(!matches_process(Path::new("/games/game.exe"), &[], &p));
        p.executable = None;
        assert!(matches_process(Path::new("/games/game.exe"), &[], &p));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn launch_tracks_reaps_and_rejects_duplicate_process() {
        let root = std::env::temp_dir().join(format!("savehub-launch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("sleep");
        std::fs::copy("/bin/sleep", &executable).unwrap();
        let id = uuid::Uuid::new_v4();
        let game = Game {
            id,
            name: "Test".into(),
            paths: GamePaths {
                exe_path: executable.display().to_string(),
                save_dir_path: root.display().to_string(),
            },
            process_names: vec![],
        };
        let runtime = Arc::new(RuntimeState::new(
            None,
            vec![],
            SyncStatus {
                state: SyncState::NotConfigured,
                last_sync_at: None,
                error: None,
            },
        ));
        let catalog = Arc::new(Catalog::new(HashMap::from([(id, game)]), HashMap::new()));
        let settings = Arc::new(Settings::new(
            SettingsData {
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
        ));
        let service = ProcessService::new(catalog, runtime.clone(), settings);
        // A held child gives a deterministic duplicate check without timing a short-lived launcher.
        let child = process::spawn(&executable, &["30".into()], Some(&root)).unwrap();
        service.children.lock().unwrap().insert(id, child);
        service.refresh_running_games().unwrap();
        assert_eq!(runtime.running_games(), Some(vec![id]));
        assert!(matches!(
            service.launch(id),
            Err(ServiceError::GameRunning(_))
        ));
        service
            .children
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .kill()
            .unwrap();
        service
            .children
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .wait()
            .unwrap();
        let events = service.refresh_running_games().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GameProcessEvent::Stopped(g) if *g == id))
        );
        assert_eq!(runtime.running_games(), Some(vec![]));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod launch_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture(root: &Path, executable: &Path) -> (ProcessService, Arc<RuntimeState>, GameId) {
        let id = uuid::Uuid::new_v4();
        let game = Game {
            id,
            name: "Launch fixture".into(),
            paths: GamePaths {
                exe_path: executable.display().to_string(),
                save_dir_path: root.display().to_string(),
            },
            process_names: vec![],
        };
        let catalog = Arc::new(Catalog::new(HashMap::from([(id, game)]), HashMap::new()));
        let runtime = Arc::new(RuntimeState::new(
            None,
            vec![],
            SyncStatus {
                state: SyncState::NotConfigured,
                last_sync_at: None,
                error: None,
            },
        ));
        let settings = Arc::new(Settings::new(
            SettingsData {
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
        ));
        (
            ProcessService::new(catalog, runtime.clone(), settings),
            runtime,
            id,
        )
    }
    #[test]
    fn successful_spawn_uses_executable_directory_and_reports_exit() {
        let root =
            std::env::temp_dir().join(format!("savehub-launch-success-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("игра & $(no-shell)");
        std::fs::write(&executable, "#!/bin/sh\npwd > 'actual cwd.txt'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (service, runtime, id) = fixture(&root, &executable);
        let pid = service.launch(id).unwrap();
        assert!(pid > 0);
        assert_eq!(runtime.running_games(), Some(vec![id]));
        service
            .children
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("actual cwd.txt"))
                .unwrap()
                .trim(),
            root.to_str().unwrap()
        );
        let events = service.refresh_running_games().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GameProcessEvent::Stopped(g) if *g == id))
        );
        assert_eq!(runtime.running_games(), Some(vec![]));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_spawn_does_not_mark_game_running() {
        let root =
            std::env::temp_dir().join(format!("savehub-launch-fail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("game");
        std::fs::write(&executable, "not executable").unwrap();
        let (service, runtime, id) = fixture(&root, &executable);
        assert!(matches!(service.launch(id), Err(ServiceError::Adapter(_))));
        assert_eq!(runtime.running_games(), Some(vec![]));
        assert!(service.children.lock().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn restores_saved_launcher_but_preserves_current_registration() {
        let root = std::env::temp_dir().join(format!("savehub-restore-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let (service, _, id) = fixture(&root, &root.join("game"));
        let original = service.game(id).unwrap();
        config::write_games(&root.join(format!("{id}.json")), &[original.clone()]).unwrap();
        service.catalog.remove_game(id);
        service.restore_registration(id, &root).unwrap();
        assert_eq!(service.game(id).unwrap().name, original.name);
        let mut changed = original;
        changed.name = "newer in memory".into();
        service.catalog.update_game(changed);
        service.restore_registration(id, &root).unwrap();
        assert_eq!(service.game(id).unwrap().name, "newer in memory");
        assert!(matches!(
            service.restore_registration(uuid::Uuid::new_v4(), &root),
            Err(ServiceError::NotFound(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
