use crate::{
    domain::{Catalog, RuntimeState, SyncState, SyncStatus},
    services::{ServiceError, ServiceResult, process::ProcessService, repo::RepoOperations},
};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct ManualSync {
    repo: Arc<dyn RepoOperations>,
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    processes: Arc<ProcessService>,
    operations: Arc<Mutex<()>>,
}

impl ManualSync {
    pub fn new(
        repo: Arc<dyn RepoOperations>,
        catalog: Arc<Catalog>,
        runtime: Arc<RuntimeState>,
        processes: Arc<ProcessService>,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            repo,
            catalog,
            runtime,
            processes,
            operations,
        }
    }

    pub fn execute(&self) -> ServiceResult<()> {
        let _guard = self.operations.lock().map_err(|_| {
            ServiceError::Conflict("Операция с сохранениями уже выполняется".into())
        })?;

        if self.repo.configuration().is_none() {
            return Err(ServiceError::InvalidInput(
                "Сначала настройте репозиторий сохранений в параметрах.".into(),
            ));
        }
        self.processes.refresh_running_games()?;
        match self.runtime.running_games() {
            Some(games) if !games.is_empty() => {
                return Err(ServiceError::GameRunning(games));
            }
            None => return Err(ServiceError::ProcessesUnknown),
            _ => {}
        }

        self.set_status(SyncState::Syncing, None, None);
        let last_sync_at = self.runtime.sync_status().last_sync_at;
        let result = self.synchronize();
        match &result {
            Ok(()) => self.set_status(
                SyncState::Idle,
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|elapsed| elapsed.as_millis() as u64),
                None,
            ),
            Err(error) => self.runtime.set_sync_status(SyncStatus {
                state: SyncState::Error,
                last_sync_at,
                error: Some(format!("{error:?}")),
            }),
        }
        result
    }

    fn synchronize(&self) -> ServiceResult<()> {
        let active = self.runtime.active_profile();
        if let Some(profile) = active {
            self.repo.checkpoint(&profile)?;
        }

        self.repo.fetch()?;
        self.set_status(SyncState::Syncing, None, None);

        let mut profiles: Vec<_> = self.catalog.profiles().into_values().collect();
        profiles.sort_by_key(|profile| profile.id);
        for profile in &profiles {
            if self.repo.apply_profile_updates(&profile.id)? {
                return Err(ServiceError::Conflict(
                    "Разрешите конфликты сохранений перед завершением синхронизации".into(),
                ));
            }
        }

        for profile in profiles {
            if self.repo.has_unpushed_commits(&profile.id)? {
                self.repo.push(&profile.id)?;
                self.set_status(SyncState::Syncing, None, None);
            }
        }
        Ok(())
    }

    fn set_status(&self, state: SyncState, last_sync_at: Option<u64>, error: Option<String>) {
        let previous = self.runtime.sync_status();
        let last_sync_at = last_sync_at.or(previous.last_sync_at);
        self.runtime.set_sync_status(SyncStatus {
            state,
            last_sync_at: if matches!(state, SyncState::Idle) && error.is_none() {
                last_sync_at.or_else(|| {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|elapsed| elapsed.as_millis() as u64)
                })
            } else {
                last_sync_at
            },
            error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{CommitId, Profile, ProfileId, RepositoryConfig},
        services::{ServiceError, ServiceResult, repo::RepoOperations},
    };
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct FakeRepo {
        calls: Mutex<Vec<String>>,
        dirty: Mutex<bool>,
        conflict: Mutex<bool>,
    }

    impl FakeRepo {
        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
    }

    impl RepoOperations for FakeRepo {
        fn configuration(&self) -> Option<RepositoryConfig> {
            Some(RepositoryConfig {
                url: "https://github.com/example/saves".into(),
                path: "saves".into(),
            })
        }

        fn checkpoint(&self, profile: &ProfileId) -> ServiceResult<Option<CommitId>> {
            self.record(format!("checkpoint:{profile}"));
            Ok(self.dirty.lock().unwrap().then(|| "local".into()))
        }

        fn push(&self, profile: &ProfileId) -> ServiceResult<()> {
            self.record(format!("push:{profile}"));
            Ok(())
        }

        fn fetch(&self) -> ServiceResult<()> {
            self.record("fetch".into());
            Ok(())
        }

        fn has_unpushed_commits(&self, profile: &ProfileId) -> ServiceResult<bool> {
            self.record(format!("unpushed:{profile}"));
            Ok(*self.dirty.lock().unwrap())
        }

        fn unpushed_profiles(&self) -> ServiceResult<Vec<ProfileId>> {
            Ok(Vec::new())
        }

        fn apply_profile_updates(&self, profile: &ProfileId) -> ServiceResult<bool> {
            self.record(format!("apply:{profile}"));
            Ok(*self.conflict.lock().unwrap())
        }
    }

    fn setup() -> (ManualSync, Arc<FakeRepo>, Arc<RuntimeState>, ProfileId) {
        let id = uuid::Uuid::from_u128(1);
        let profile = Profile {
            id,
            name: "Main".into(),
            branch: "main".into(),
            last_played_game: None,
        };
        let catalog = Arc::new(Catalog::new(HashMap::new(), HashMap::from([(id, profile)])));
        let runtime = Arc::new(RuntimeState::new(
            Some(id),
            Vec::new(),
            SyncStatus {
                state: SyncState::Idle,
                last_sync_at: None,
                error: None,
            },
        ));
        let settings = Arc::new(crate::domain::Settings::new(
            crate::domain::SettingsData {
                catalog_repo_url: None,
                repo_url: None,
                local_repo_path: None,
                sync_after_session: true,
                sync_while_gaming: false,
                auto_switch_profile: false,
                ask_profile_before_launch: true,
                start_minimized: false,
            },
            crate::domain::LocalConfig {
                machine_id: "test".into(),
                games: HashMap::new(),
            },
        ));
        let processes = Arc::new(ProcessService::new(
            catalog.clone(),
            runtime.clone(),
            settings,
        ));
        let repo = Arc::new(FakeRepo::default());
        let workflow = ManualSync::new(
            repo.clone(),
            catalog,
            runtime.clone(),
            processes,
            Arc::new(Mutex::new(())),
        );
        (workflow, repo, runtime, id)
    }

    #[test]
    fn manual_sync_checkpoints_fetches_merges_then_pushes() {
        let (workflow, repo, runtime, id) = setup();
        *repo.dirty.lock().unwrap() = true;

        workflow.execute().unwrap();

        assert_eq!(
            *repo.calls.lock().unwrap(),
            [
                format!("checkpoint:{id}"),
                "fetch".into(),
                format!("apply:{id}"),
                format!("unpushed:{id}"),
                format!("push:{id}"),
            ]
        );
        assert!(matches!(runtime.sync_status().state, SyncState::Idle));
        assert!(runtime.sync_status().last_sync_at.is_some());
    }

    #[test]
    fn manual_sync_does_not_push_until_conflicts_are_resolved() {
        let (workflow, repo, runtime, id) = setup();
        *repo.dirty.lock().unwrap() = true;
        *repo.conflict.lock().unwrap() = true;

        assert!(matches!(workflow.execute(), Err(ServiceError::Conflict(_))));
        assert_eq!(
            *repo.calls.lock().unwrap(),
            [
                format!("checkpoint:{id}"),
                "fetch".into(),
                format!("apply:{id}"),
            ]
        );
        assert!(matches!(runtime.sync_status().state, SyncState::Error));
    }
}
