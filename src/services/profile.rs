use super::{ServiceError, ServiceResult, repo::RepoService};
use crate::domain::*;
use std::sync::{Arc, Mutex};

pub struct ProfileService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    repo: Arc<RepoService>,
    operations: Arc<Mutex<()>>,
    persistence: Option<std::path::PathBuf>,
}

impl ProfileService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, repo: Arc<RepoService>) -> Self {
        Self::with_operations(catalog, runtime, repo, Arc::new(Mutex::new(())))
    }

    pub fn with_operations(
        catalog: Arc<Catalog>,
        runtime: Arc<RuntimeState>,
        repo: Arc<RepoService>,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            catalog,
            runtime,
            repo,
            operations,
            persistence: None,
        }
    }

    pub fn persistent(mut self, directory: std::path::PathBuf) -> Self {
        self.persistence = Some(directory);
        self
    }

    fn persist(&self) -> ServiceResult<()> {
        if let Some(directory) = &self.persistence {
            crate::adapters::config::write_profiles(
                &directory.join("profiles.json"),
                &self.list()?,
            )
            .map_err(ServiceError::Adapter)?;
        }
        Ok(())
    }

    pub fn list(&self) -> ServiceResult<Vec<Profile>> {
        Ok(self.catalog.profiles().into_values().collect())
    }

    pub fn get(&self, profile_id: &ProfileId) -> ServiceResult<Profile> {
        self.catalog
            .profiles()
            .remove(profile_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Профиль не найден: {profile_id}")))
    }

    pub fn active(&self) -> ServiceResult<Option<Profile>> {
        self.runtime
            .active_profile()
            .map(|id| self.get(&id))
            .transpose()
    }

    /// Создать профиль и его ветку; каталог игр общий.
    pub fn create(&self, name: &str) -> ServiceResult<Profile> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка операций профиля повреждена".into()))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(ServiceError::InvalidInput(
                "Имя профиля не может быть пустым".into(),
            ));
        }
        let id = uuid::Uuid::new_v4();
        let branch = self.repo.create_profile_branch(name, id)?;
        let profile = Profile {
            id,
            branch,
            name: name.to_owned(),
            last_played_game: None,
        };
        self.catalog.add_profile(profile.clone());
        if let Err(error) = self.persist() {
            self.catalog.remove_profile(&profile.id);
            self.repo
                .rollback_created_profile(&profile)
                .map_err(|rollback| {
                    ServiceError::Conflict(format!(
                        "Запись профиля: {error:?}; откат новой ветки: {rollback:?}"
                    ))
                })?;
            return Err(error);
        }
        Ok(profile)
    }

    /// Переименовать без изменения идентификатора.
    pub fn rename(&self, profile_id: &ProfileId, name: &str) -> ServiceResult<()> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка профиля повреждена".into()))?;
        let mut profile = self.get(profile_id)?;
        let previous = profile.clone();
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Имя профиля не может быть пустым".into(),
            ));
        }
        profile.name = name.trim().to_owned();
        if !self.catalog.update_profile(profile) {
            return Err(ServiceError::NotFound("Профиль не найден".into()));
        }
        if let Err(error) = self.persist() {
            self.catalog.update_profile(previous);
            return Err(error);
        }
        Ok(())
    }

    /// Запретить удаление активного профиля.
    pub fn delete(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка операций профиля повреждена".into()))?;
        self.get(profile_id)?;
        if self.runtime.active_profile().as_ref() == Some(profile_id) {
            return Err(ServiceError::InvalidInput(
                "Активный профиль удалить нельзя".into(),
            ));
        }
        let profile = self.get(profile_id)?;
        self.repo.mark_profile_deleted(&profile)?;
        self.catalog.remove_profile(profile_id);
        if let Err(error) = self.persist() {
            self.catalog.add_profile(profile);
            if let Err(rollback) = self.repo.unmark_profile_deleted(profile_id) {
                return Err(ServiceError::Conflict(format!(
                    "Сохранение списка профилей: {error:?}; откат отметки удаления: {rollback:?}"
                )));
            }
            return Err(error);
        }
        // RepoService validates the profile through Catalog, so retain it until archival completes.
        self.catalog.add_profile(profile.clone());
        if let Err(error) = self.repo.delete_profile(profile_id) {
            self.persist().map_err(|rollback| {
                ServiceError::Conflict(format!(
                    "Удаление: {error:?}; восстановление метаданных: {rollback:?}"
                ))
            })?;
            self.repo
                .unmark_profile_deleted(profile_id)
                .map_err(|rollback| {
                    ServiceError::Conflict(format!(
                        "Удаление: {error:?}; откат отметки удаления: {rollback:?}"
                    ))
                })?;
            return Err(error);
        }
        self.catalog.remove_profile(profile_id);
        Ok(())
    }

    /// Зафиксировать активный профиль после успешной смены сохранений.
    pub fn set_active(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка операций профиля повреждена".into()))?;
        self.set_active_locked(profile_id)
    }

    /// Caller already holds the shared operations mutex.
    pub(crate) fn set_active_locked(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        if !self.catalog.profiles().contains_key(profile_id) {
            return Err(ServiceError::NotFound("Профиль не найден".into()));
        }
        let previous = self.runtime.active_profile();
        self.runtime
            .set_active_profile(Some(*profile_id))
            .map_err(|error| match error {
                ProfileActivationError::ProcessesUnknown => ServiceError::ProcessesUnknown,
                ProfileActivationError::GamesRunning(games) => ServiceError::GameRunning(games),
            })?;
        if let Some(directory) = &self.persistence {
            if let Err(error) = crate::adapters::config::write_active_profile(
                &directory.join("active-profile.json"),
                profile_id,
            ) {
                let _ = self.runtime.set_active_profile(previous);
                return Err(ServiceError::Adapter(error));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, path::PathBuf};

    struct Fixture {
        root: PathBuf,
        service: ProfileService,
        settings: Arc<Settings>,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("savehub-profile-test-{}", uuid::Uuid::new_v4()));
            git2::Repository::init(&root).unwrap();
            let catalog = Arc::new(Catalog::new(HashMap::new(), HashMap::new()));
            let runtime = Arc::new(RuntimeState::new(
                None,
                Vec::new(),
                SyncStatus {
                    state: SyncState::Idle,
                    last_sync_at: None,
                    error: None,
                },
            ));
            let settings = Arc::new(Settings::new(
                SettingsData {
                    catalog_repo_url: None,
                    ask_profile_before_launch: true,
                    repo_url: Some("https://github.com/test/saves.git".into()),
                    local_repo_path: Some(root.clone()),
                    sync_after_session: false,
                    sync_while_gaming: false,
                    auto_switch_profile: false,
                    start_minimized: false,
                },
                LocalConfig {
                    machine_id: "test".into(),
                    games: HashMap::new(),
                },
            ));
            Self {
                root,
                service: ProfileService::new(
                    catalog.clone(),
                    runtime.clone(),
                    Arc::new(RepoService::new(catalog, runtime, settings.clone())),
                ),
                settings,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn creation_does_not_claim_an_unborn_main_worktree_branch() {
        let fixture = Fixture::new();
        let repository = git2::Repository::open(&fixture.root).unwrap();
        repository.set_head("refs/heads/master").unwrap();
        let profile = fixture.service.create("Master").unwrap();
        assert!(profile.branch.starts_with("master-"));
        assert!(profile.branch.ends_with(&profile.id.simple().to_string()));
        assert!(repository.head().is_err());
        fixture.service.runtime.set_running_games(Vec::new());
        fixture.service.delete(&profile.id).unwrap();
    }

    #[test]
    fn failed_profile_persistence_rolls_back_branch_and_retry_reuses_name() {
        let mut fixture = Fixture::new();
        let directory = fixture.root.join("local-state");
        std::fs::create_dir_all(directory.join("profiles.json")).unwrap();
        fixture.service.persistence = Some(directory.clone());
        assert!(fixture.service.create("Retry").is_err());
        assert!(fixture.service.list().unwrap().is_empty());
        assert!(
            crate::adapters::git::list_branches(&fixture.root)
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir(directory.join("profiles.json")).unwrap();
        let profile = fixture.service.create("Retry").unwrap();
        assert!(profile.branch.starts_with("retry-"));
    }

    #[test]
    fn deletion_removes_local_branch_and_catalog_but_preserves_history_and_remote() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Delete me").unwrap();
        let repository = git2::Repository::open(&fixture.root).unwrap();
        let tip = repository
            .find_branch(&profile.branch, git2::BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let remote_ref = format!("refs/remotes/origin/{}", profile.branch);
        repository
            .reference(&remote_ref, tip, false, "test")
            .unwrap();
        fixture.service.delete(&profile.id).unwrap();
        assert!(fixture.service.get(&profile.id).is_err());
        assert!(
            repository
                .find_branch(&profile.branch, git2::BranchType::Local)
                .is_err()
        );
        assert_eq!(
            repository.find_reference(&remote_ref).unwrap().target(),
            Some(tip)
        );
        let archives: Vec<_> = repository
            .references_glob("refs/savehub/deleted-profiles/*")
            .unwrap()
            .map(|item| item.unwrap().target().unwrap())
            .collect();
        assert!(archives.contains(&tip));
    }

    #[test]
    fn deletion_persists_a_tombstone_before_removing_profile_metadata() {
        let mut fixture = Fixture::new();
        let directory = fixture.root.join("local-data");
        fixture.service.persistence = Some(directory.clone());
        fixture.service.repo = Arc::new(
            RepoService::new(
                fixture.service.catalog.clone(),
                fixture.service.runtime.clone(),
                fixture.settings.clone(),
            )
            .persistent(directory.clone()),
        );
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Persistent deletion").unwrap();

        fixture.service.delete(&profile.id).unwrap();

        assert!(fixture.service.get(&profile.id).is_err());
        assert!(
            crate::adapters::config::read_profile_deletions(
                &directory.join("deleted-profiles.json")
            )
            .unwrap()
            .unwrap()
            .iter()
            .any(|deletion| deletion.id == profile.id)
        );
    }

    #[test]
    fn deletion_ignores_another_profiles_missing_worktree() {
        for missing_directory in [true, false] {
            let fixture = Fixture::new();
            fixture.service.runtime.set_running_games(Vec::new());
            let other = fixture.service.create("Moved saves").unwrap();
            let other_tree = crate::adapters::git::prepare_worktree(
                &fixture.root,
                &other.branch,
                &other.id.to_string(),
            )
            .unwrap();
            std::fs::write(other_tree.join("save.dat"), b"keep other saves").unwrap();
            let moved = fixture.root.join("moved-tree");
            if missing_directory {
                std::fs::rename(&other_tree, &moved).unwrap();
            } else {
                std::fs::rename(other_tree.join(".git"), &moved).unwrap();
            }
            let profile = fixture.service.create("Delete me").unwrap();
            crate::adapters::git::prepare_worktree(
                &fixture.root,
                &profile.branch,
                &profile.id.to_string(),
            )
            .unwrap();
            fixture.service.delete(&profile.id).unwrap();
            assert!(fixture.service.get(&profile.id).is_err());
            assert!(fixture.service.get(&other.id).is_ok());
            assert_eq!(
                std::fs::read(
                    if missing_directory {
                        &moved
                    } else {
                        &other_tree
                    }
                    .join("save.dat")
                )
                .unwrap(),
                b"keep other saves"
            );
            let repository = git2::Repository::open(&fixture.root).unwrap();
            assert_eq!(repository.worktrees().unwrap().len(), 1);
            assert!(
                repository
                    .find_branch(&other.branch, git2::BranchType::Local)
                    .is_ok()
            );
        }
    }

    #[test]
    fn deletion_archives_worktree_with_uncommitted_saves_and_previous_backups() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Saves").unwrap();
        fixture
            .service
            .repo
            .checkout_profile(&profile.id.to_string())
            .unwrap();
        let tree = crate::adapters::git::prepare_worktree(
            &fixture.root,
            &profile.branch,
            &profile.id.to_string(),
        )
        .unwrap();
        std::fs::write(tree.join("save.dat"), b"uncommitted save\0").unwrap();
        let metadata = crate::adapters::git::metadata_dir(&tree).unwrap();
        std::fs::write(metadata.join("previous-backup"), b"older save").unwrap();
        fixture.service.delete(&profile.id).unwrap();
        assert!(!tree.exists());
        assert!(fixture.service.get(&profile.id).is_err());
        let repository = git2::Repository::open(&fixture.root).unwrap();
        assert!(repository.worktrees().unwrap().is_empty());
        assert!(
            repository
                .find_branch(&profile.branch, git2::BranchType::Local)
                .is_err()
        );
        let archive = std::fs::read_dir(repository.commondir().join("savehub/deleted-profiles"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::read(archive.join("worktree/save.dat")).unwrap(),
            b"uncommitted save\0"
        );
        assert_eq!(
            std::fs::read(archive.join("git-metadata/previous-backup")).unwrap(),
            b"older save"
        );
    }

    #[test]
    fn failed_or_unsafe_deletion_preserves_profile_and_branch() {
        let fixture = Fixture::new();
        let profile = fixture.service.create("Protected").unwrap();
        assert!(matches!(
            fixture.service.delete(&profile.id),
            Err(ServiceError::ProcessesUnknown)
        ));
        fixture
            .service
            .runtime
            .set_running_games(vec![uuid::Uuid::new_v4()]);
        assert!(matches!(
            fixture.service.delete(&profile.id),
            Err(ServiceError::GameRunning(_))
        ));
        fixture.service.runtime.set_running_games(Vec::new());
        fixture.service.set_active(&profile.id).unwrap();
        assert!(matches!(
            fixture.service.delete(&profile.id),
            Err(ServiceError::InvalidInput(_))
        ));
        fixture.service.runtime.set_active_profile(None).unwrap();
        fixture
            .settings
            .set_repository(None, Some(fixture.root.join("missing")));
        assert!(fixture.service.delete(&profile.id).is_err());
        assert!(fixture.service.get(&profile.id).is_ok());
        assert!(
            crate::adapters::git::list_branches(&fixture.root)
                .unwrap()
                .contains(&profile.branch)
        );
    }

    #[test]
    fn deletion_preserves_an_unregistered_profile_directory() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Unexpected directory").unwrap();
        let repository = git2::Repository::open(&fixture.root).unwrap();
        let tree = repository
            .commondir()
            .join("savehub/trees")
            .join(profile.id.to_string());
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("save.dat"), b"keep").unwrap();
        assert!(fixture.service.delete(&profile.id).is_err());
        assert!(fixture.service.get(&profile.id).is_ok());
        assert_eq!(std::fs::read(tree.join("save.dat")).unwrap(), b"keep");
        assert!(
            repository
                .find_branch(&profile.branch, git2::BranchType::Local)
                .is_ok()
        );
    }

    #[test]
    fn deletion_rejects_a_cached_foreign_lease() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Busy").unwrap();
        let bytes = serde_json::to_vec(
            &serde_json::json!({"machine_id": "another-pc", "expires_at": u64::MAX}),
        )
        .unwrap();
        let commit = crate::adapters::git::commit_files(
            &fixture.root,
            None,
            &[("lease.json", &bytes)],
            "lease",
        )
        .unwrap();
        let repository = git2::Repository::open(&fixture.root).unwrap();
        repository
            .reference(
                &format!("refs/remotes/origin/savehub/leases/{}", profile.id),
                git2::Oid::from_str(&commit).unwrap(),
                false,
                "test",
            )
            .unwrap();
        assert!(matches!(
            fixture.service.delete(&profile.id),
            Err(ServiceError::Conflict(_))
        ));
        assert!(fixture.service.get(&profile.id).is_ok());
    }

    #[test]
    fn deletion_rejects_locked_worktree_and_preserves_files() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Locked").unwrap();
        fixture
            .service
            .repo
            .checkout_profile(&profile.id.to_string())
            .unwrap();
        let tree = crate::adapters::git::prepare_worktree(
            &fixture.root,
            &profile.branch,
            &profile.id.to_string(),
        )
        .unwrap();
        let repository = git2::Repository::open(&tree).unwrap();
        let worktree = git2::Worktree::open_from_repository(&repository).unwrap();
        worktree.lock(Some("test lock")).unwrap();
        assert!(fixture.service.delete(&profile.id).is_err());
        assert!(tree.exists());
        assert!(fixture.service.get(&profile.id).is_ok());
        worktree.unlock().unwrap();
    }
    #[test]
    fn creates_independent_history_without_touching_current_saves() {
        let fixture = Fixture::new();
        let original = crate::adapters::git::commit_files(
            &fixture.root,
            None,
            &[("save.dat", b"existing saves")],
            "original",
        )
        .unwrap();
        crate::adapters::git::create_branch(&fixture.root, "existing", &original).unwrap();
        crate::adapters::git::checkout(&fixture.root, "existing").unwrap();
        std::fs::write(fixture.root.join("save.dat"), b"unsaved changes").unwrap();
        let profile = fixture.service.create("  Новый профиль  ").unwrap();
        assert_eq!(profile.name, "Новый профиль");
        assert!(profile.branch.starts_with("novyy-profil-"));
        assert!(profile.branch.ends_with(&profile.id.simple().to_string()));
        let repository = git2::Repository::open(&fixture.root).unwrap();
        let commit = repository
            .find_branch(&profile.branch, git2::BranchType::Local)
            .unwrap()
            .get()
            .peel_to_commit()
            .unwrap();
        assert_eq!(commit.parent_count(), 0);
        assert!(commit.tree().unwrap().is_empty());
        assert_eq!(crate::adapters::git::head(&fixture.root).unwrap(), original);
        assert_eq!(
            std::fs::read(fixture.root.join("save.dat")).unwrap(),
            b"unsaved changes"
        );
        assert_eq!(
            fixture.service.get(&profile.id).unwrap().branch,
            profile.branch
        );
        assert_eq!(fixture.service.runtime.active_profile(), None);
    }

    #[test]
    fn avoids_catalog_local_remote_and_namespace_collisions() {
        let fixture = Fixture::new();
        let commit = crate::adapters::git::commit_files(&fixture.root, None, &[], "root").unwrap();
        crate::adapters::git::create_branch(&fixture.root, "test", &commit).unwrap();
        let repository = git2::Repository::open(&fixture.root).unwrap();
        repository
            .reference(
                "refs/remotes/origin/test-2",
                git2::Oid::from_str(&commit).unwrap(),
                false,
                "test",
            )
            .unwrap();
        fixture.service.catalog.add_profile(Profile {
            id: uuid::Uuid::new_v4(),
            name: "Other".into(),
            branch: "test-3".into(),
            last_played_game: None,
        });
        let first = fixture.service.create("Test").unwrap();
        let second = fixture.service.create("Test").unwrap();
        assert!(first.branch.starts_with("test-"));
        assert!(second.branch.starts_with("test-"));
        assert_ne!(first.branch, second.branch);
        crate::adapters::git::create_branch(&fixture.root, "savehub/catalog", &commit).unwrap();
        assert!(
            fixture
                .service
                .create("SaveHub")
                .unwrap()
                .branch
                .starts_with("savehub-")
        );
        assert!(
            fixture
                .service
                .create("🎮")
                .unwrap()
                .branch
                .starts_with("profile-")
        );
    }

    #[test]
    fn recreating_a_deleted_profile_name_uses_a_different_branch() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());

        let deleted = fixture.service.create("Replay").unwrap();
        fixture.service.delete(&deleted.id).unwrap();
        let replacement = fixture.service.create("Replay").unwrap();

        assert_ne!(replacement.id, deleted.id);
        assert_ne!(replacement.branch, deleted.branch);
        assert!(
            replacement
                .branch
                .ends_with(&replacement.id.simple().to_string())
        );
    }

    #[test]
    fn invalid_name_or_repository_does_not_add_profile() {
        let fixture = Fixture::new();
        assert!(matches!(
            fixture.service.create("  "),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(fixture.service.list().unwrap().is_empty());
        fixture.settings.set_repository(None, None);
        assert!(matches!(
            fixture.service.create("Test"),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(fixture.service.list().unwrap().is_empty());
        fixture
            .settings
            .set_repository(None, Some(fixture.root.join("missing")));
        assert!(matches!(
            fixture.service.create("Test"),
            Err(ServiceError::Adapter(_))
        ));
        assert!(fixture.service.list().unwrap().is_empty());
    }

    #[test]
    fn concurrent_creation_uses_distinct_branches() {
        let fixture = Fixture::new();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| fixture.service.create("Same").unwrap());
            let second = scope.spawn(|| fixture.service.create("Same").unwrap());
            let first = first.join().unwrap();
            let second = second.join().unwrap();
            assert_ne!(first.branch, second.branch);
            assert_ne!(first.id, second.id);
        });
        assert_eq!(fixture.service.list().unwrap().len(), 2);
    }
    #[test]
    fn persists_profile_metadata_and_active_id_and_rolls_back_failed_rename() {
        let mut fixture = Fixture::new();
        let directory = fixture.root.join("local-state");
        fixture.service.persistence = Some(directory.clone());
        fixture.service.runtime.set_running_games(Vec::new());
        let profile = fixture.service.create("Persistent").unwrap();
        let stored = crate::adapters::config::read_profiles(&directory.join("profiles.json"))
            .unwrap()
            .unwrap();
        assert_eq!(stored[0].id, profile.id);
        fixture.service.set_active(&profile.id).unwrap();
        assert_eq!(
            crate::adapters::config::read_active_profile(&directory.join("active-profile.json"))
                .unwrap(),
            Some(profile.id)
        );
        std::fs::remove_file(directory.join("profiles.json")).unwrap();
        std::fs::create_dir(directory.join("profiles.json")).unwrap();
        assert!(fixture.service.rename(&profile.id, "Renamed").is_err());
        assert_eq!(fixture.service.get(&profile.id).unwrap().name, "Persistent");
    }
}
