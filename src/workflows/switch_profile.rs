use std::sync::{Arc, Mutex};

use crate::domain::ProfileId;
use crate::services::{
    ServiceResult, game::GameService, process::ProcessMonitor, profile::ProfileService,
    repo::RepoService, save::SaveService,
};

pub struct SwitchProfile {
    games: Arc<GameService>,
    profiles: Arc<ProfileService>,
    saves: Arc<SaveService>,
    repo: Arc<RepoService>,
    processes: Arc<dyn ProcessMonitor>,
    operations: Arc<Mutex<()>>,
}

impl SwitchProfile {
    /// operations — общий mutex операций с репозиторием и сохранениями.
    pub fn new(
        games: Arc<GameService>,
        profiles: Arc<ProfileService>,
        saves: Arc<SaveService>,
        repo: Arc<RepoService>,
        processes: Arc<dyn ProcessMonitor>,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            games,
            profiles,
            saves,
            repo,
            processes,
            operations,
        }
    }

    /// Захватить operations на весь сценарий; затем перепроверить профиль и процессы.
    /// Не захватывать operations повторно и не держать локи хранилищ во время I/O.
    pub fn execute(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        let _guard = self.operations.lock().map_err(|_| {
            crate::services::ServiceError::Conflict("Операция уже выполняется".into())
        })?;
        self.profiles.get(profile_id)?;
        self.processes.refresh_running_games()?;
        self.execute_locked(profile_id)
    }

    /// The caller holds operations and has refreshed process state.
    fn execute_locked(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        self.profiles.get(profile_id)?;
        let running = self
            .processes
            .running_games()
            .ok_or(crate::services::ServiceError::ProcessesUnknown)?;
        if !running.is_empty() {
            return Err(crate::services::ServiceError::GameRunning(running));
        }
        let current = self.profiles.active()?.ok_or_else(|| {
            crate::services::ServiceError::InvalidInput("Нет активного профиля".into())
        })?;
        let mut games = Vec::new();
        for game in self.games.list()? {
            match self.games.check_paths(game.id)? {
                crate::domain::GameAvailability::Ignored => (),
                crate::domain::GameAvailability::Ready { .. } => games.push(game),
                crate::domain::GameAvailability::Missing {
                    save_dir_missing: true,
                    exe_missing: false,
                } if self.saves.is_bound(game.id, &current.id)? => games.push(game),
                crate::domain::GameAvailability::Missing { .. } => {
                    return Err(crate::services::ServiceError::InvalidInput(format!(
                        "Укажите локальные пути игры {}",
                        game.name
                    )));
                }
            }
        }
        self.repo.checkout_profile(&current.id.to_string())?;
        for game in &games {
            self.saves.bind(game.id, &current.id.to_string())?;
            self.saves.create_save(
                game.id,
                &current.id.to_string(),
                crate::domain::SaveKind::BeforeSwitch,
            )?;
        }
        self.repo
            .checkpoint(&current.id.to_string(), "SaveHub: before profile switch")?;
        if self.repo.configuration().is_some() {
            self.repo.fetch()?;
            if !self
                .repo
                .apply_profile_updates(&profile_id.to_string())?
                .is_empty()
            {
                return Err(crate::services::ServiceError::Conflict(
                    "Разрешите конфликты целевого профиля".into(),
                ));
            }
        }
        if current.id == *profile_id {
            return Ok(());
        }
        self.repo.checkout_profile(&profile_id.to_string())?;
        for game in &games {
            self.saves.initialize(game.id, &profile_id.to_string())?;
        }
        let mut bound = Vec::new();
        let result = (|| {
            for game in &games {
                self.saves.bind(game.id, &profile_id.to_string())?;
                bound.push(game.id);
            }
            self.profiles.set_active_locked(profile_id)
        })();
        if let Err(error) = result {
            for game in bound.into_iter().rev() {
                if let Err(rollback) = self.saves.bind(game, &current.id.to_string()) {
                    return Err(crate::services::ServiceError::Conflict(format!(
                        "Переключение: {error:?}; восстановление привязок: {rollback:?}. Не запускайте игры до исправления путей"
                    )));
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// Refresh the active profile before launching another game without changing it.
    pub fn refresh_active_profile(&self) -> ServiceResult<()> {
        if !self.repo.has_local_repository() {
            return Ok(());
        }
        let _guard = self.operations.lock().map_err(|_| {
            crate::services::ServiceError::Conflict("Операция уже выполняется".into())
        })?;
        self.processes.refresh_running_games()?;
        let running = self
            .processes
            .running_games()
            .ok_or(crate::services::ServiceError::ProcessesUnknown)?;
        if !running.is_empty() {
            return Err(crate::services::ServiceError::GameRunning(running));
        }
        let profile = self.profiles.active()?.ok_or_else(|| {
            crate::services::ServiceError::InvalidInput("Нет активного профиля".into())
        })?;
        self.repo.checkout_profile(&profile.id.to_string())?;
        self.repo
            .checkpoint(&profile.id.to_string(), "SaveHub: before game launch")?;
        if self.repo.configuration().is_some() {
            self.repo.fetch().map_err(|error| {
                crate::services::ServiceError::RemoteUnavailable(format!("{error:?}"))
            })?;
            if !self
                .repo
                .apply_profile_updates(&profile.id.to_string())?
                .is_empty()
            {
                return Err(crate::services::ServiceError::Conflict(
                    "Разрешите конфликты сохранений перед запуском игры".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::{config, fs, git},
        domain::*,
    };
    use std::{collections::HashMap, path::PathBuf};

    struct IdleProcessMonitor(Arc<RuntimeState>);

    impl crate::services::process::ProcessMonitor for IdleProcessMonitor {
        fn refresh_running_games(&self) -> crate::services::ServiceResult<Vec<GameProcessEvent>> {
            self.0.set_running_games(Vec::new());
            Ok(Vec::new())
        }

        fn running_games(&self) -> Option<Vec<GameId>> {
            self.0.running_games()
        }
    }

    struct Fixture {
        dir: PathBuf,
        source: PathBuf,
        runtime: Arc<RuntimeState>,
        workflow: SwitchProfile,
        old: ProfileId,
        new: ProfileId,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("savehub-switch-{}", uuid::Uuid::new_v4()));
            let root = dir.join("repo");
            let source = dir.join("live");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("save"), b"old profile").unwrap();
            std::fs::write(dir.join("game.exe"), b"exe").unwrap();
            git2::Repository::init(&root).unwrap();
            let commit = git::commit_files(&root, None, &[], "initial").unwrap();
            git::create_branch(&root, "main", &commit).unwrap();
            git::checkout(&root, "main").unwrap();
            let old = uuid::Uuid::new_v4();
            let new = uuid::Uuid::new_v4();
            let game = uuid::Uuid::new_v4();
            let catalog = Arc::new(Catalog::new(
                HashMap::from([(
                    game,
                    Game {
                        id: game,
                        name: "Game".into(),
                        paths: GamePaths {
                            save_dir_path: source.to_string_lossy().into(),
                            exe_path: dir.join("game.exe").to_string_lossy().into(),
                        },
                        process_names: vec![],
                    },
                )]),
                [(old, "main"), (new, "other")]
                    .into_iter()
                    .map(|(id, branch)| {
                        (
                            id,
                            Profile {
                                id,
                                name: branch.into(),
                                branch: branch.into(),
                                last_played_game: None,
                            },
                        )
                    })
                    .collect(),
            ));
            let runtime = Arc::new(RuntimeState::new(
                Some(old),
                vec![],
                SyncStatus {
                    state: SyncState::Idle,
                    last_sync_at: None,
                    error: None,
                },
            ));
            runtime.set_running_games(vec![]);
            let settings = Arc::new(Settings::new(
                SettingsData {
                    catalog_repo_url: None,
                    repo_url: None,
                    local_repo_path: Some(root),
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
            let operations = Arc::new(Mutex::new(()));
            let repo = Arc::new(RepoService::new(
                catalog.clone(),
                runtime.clone(),
                settings.clone(),
            ));
            let profiles = Arc::new(
                ProfileService::with_operations(
                    catalog.clone(),
                    runtime.clone(),
                    repo.clone(),
                    operations.clone(),
                )
                .persistent(dir.clone()),
            );
            let workflow = SwitchProfile::new(
                Arc::new(GameService::new(
                    catalog.clone(),
                    runtime.clone(),
                    settings.clone(),
                )),
                profiles,
                Arc::new(SaveService::new(
                    catalog.clone(),
                    runtime.clone(),
                    settings.clone(),
                )),
                repo,
                Arc::new(IdleProcessMonitor(runtime.clone())),
                operations,
            );
            Self {
                dir,
                source,
                runtime,
                workflow,
                old,
                new,
            }
        }
        fn switch(&self, id: ProfileId) -> ServiceResult<()> {
            let _guard = self.workflow.operations.lock().unwrap();
            self.workflow.execute_locked(&id)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    #[test]
    fn switches_independent_trees_checkpoints_departing_profile_and_persists_active() {
        let f = Fixture::new();
        f.switch(f.new).unwrap();
        assert_eq!(f.runtime.active_profile(), Some(f.new));
        assert!(!f.source.join("save").exists());
        std::fs::write(f.source.join("save"), b"new profile").unwrap();
        f.switch(f.old).unwrap();
        assert_eq!(fs::read(&f.source.join("save")).unwrap(), b"old profile");
        assert_eq!(
            config::read_active_profile(&f.dir.join("active-profile.json")).unwrap(),
            Some(f.old)
        );
        let saves = f.workflow.saves.list(None, None).unwrap();
        assert_eq!(saves.len(), 2);
        assert!(saves.iter().any(|s| s.profile_id == f.old));
        assert!(saves.iter().any(|s| s.profile_id == f.new));
        f.switch(f.new).unwrap();
        assert_eq!(fs::read(&f.source.join("save")).unwrap(), b"new profile");
    }
    #[test]
    fn active_profile_write_failure_rolls_back_links_and_runtime() {
        let f = Fixture::new();
        std::fs::create_dir(f.dir.join("active-profile.json")).unwrap();
        assert!(f.switch(f.new).is_err());
        assert_eq!(f.runtime.active_profile(), Some(f.old));
        assert_eq!(fs::read(&f.source.join("save")).unwrap(), b"old profile");
    }
    #[test]
    fn unknown_processes_leave_live_saves_unbound() {
        let f = Fixture::new();
        f.runtime.invalidate_process_state();
        assert!(matches!(
            f.switch(f.new),
            Err(crate::services::ServiceError::ProcessesUnknown)
        ));
        assert!(fs::read_symlink(&f.source).unwrap().is_none());
        assert_eq!(f.runtime.active_profile(), Some(f.old));
    }

    #[test]
    fn refreshing_active_profile_preserves_it_after_process_refresh() {
        let f = Fixture::new();
        f.workflow.refresh_active_profile().unwrap();
        assert_eq!(f.runtime.active_profile(), Some(f.old));
        assert_eq!(fs::read(&f.source.join("save")).unwrap(), b"old profile");

        f.runtime.invalidate_process_state();
        f.workflow.refresh_active_profile().unwrap();
        assert!(f.runtime.running_games().unwrap().is_empty());
    }

    #[test]
    fn switch_repairs_missing_target_of_current_profile_link() {
        let f = Fixture::new();
        f.switch(f.old).unwrap();
        let target = fs::canonicalize(&f.source).unwrap();
        std::fs::remove_dir_all(&target).unwrap();
        f.switch(f.new).unwrap();
        assert!(target.is_dir());
        assert!(f.source.is_dir());
        assert_eq!(f.runtime.active_profile(), Some(f.new));
    }

    #[test]
    fn switch_does_not_repair_links_without_exe_or_from_another_profile() {
        let f = Fixture::new();
        f.switch(f.old).unwrap();
        let target = fs::canonicalize(&f.source).unwrap();
        std::fs::remove_dir_all(&target).unwrap();
        std::fs::remove_file(f.dir.join("game.exe")).unwrap();
        assert!(f.switch(f.new).is_err());
        assert!(!target.exists());
        assert_eq!(f.runtime.active_profile(), Some(f.old));

        let f = Fixture::new();
        f.switch(f.new).unwrap();
        let target = fs::canonicalize(&f.source).unwrap();
        std::fs::remove_dir_all(&target).unwrap();
        f.runtime.set_active_profile(Some(f.old)).unwrap();
        assert!(f.switch(f.new).is_err());
        assert!(!target.exists());
        assert_eq!(f.runtime.active_profile(), Some(f.old));
    }

    #[test]
    fn switch_rejects_missing_ordinary_directory_and_foreign_dangling_link() {
        let f = Fixture::new();
        std::fs::remove_dir_all(&f.source).unwrap();
        assert!(f.switch(f.new).is_err());
        assert!(!f.source.exists());
        let foreign = f.dir.join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        fs::create_symlink(&f.source, &foreign).unwrap();
        std::fs::remove_dir(&foreign).unwrap();
        assert!(f.switch(f.new).is_err());
        assert!(!foreign.exists());
        assert!(fs::read_symlink(&f.source).unwrap().is_some());
        assert_eq!(f.runtime.active_profile(), Some(f.old));
    }
}
