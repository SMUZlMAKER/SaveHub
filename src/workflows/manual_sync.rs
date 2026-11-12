use crate::{
    domain::{Catalog, RuntimeState, SyncState, SyncStatus},
    services::{ServiceError, ServiceResult, process::ProcessMonitor, repo::RepoOperations},
};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct ManualSync {
    repo: Arc<dyn RepoOperations>,
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    processes: Arc<dyn ProcessMonitor>,
    operations: Arc<Mutex<()>>,
}

impl ManualSync {
    pub fn new(
        repo: Arc<dyn RepoOperations>,
        catalog: Arc<Catalog>,
        runtime: Arc<RuntimeState>,
        processes: Arc<dyn ProcessMonitor>,
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

        self.repo.sync_catalog()?;
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

    struct IdleProcessMonitor(Arc<RuntimeState>);

    impl crate::services::process::ProcessMonitor for IdleProcessMonitor {
        fn refresh_running_games(&self) -> ServiceResult<Vec<crate::domain::GameProcessEvent>> {
            self.0.set_running_games(Vec::new());
            Ok(Vec::new())
        }

        fn running_games(&self) -> Option<Vec<crate::domain::GameId>> {
            self.0.running_games()
        }
    }

    #[derive(Default)]
    struct FakeRepo {
        calls: Mutex<Vec<String>>,
        dirty: Mutex<bool>,
        conflict: Mutex<bool>,
        catalog: Mutex<Option<Arc<Catalog>>>,
        synced_profile: Mutex<Option<Profile>>,
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

        fn sync_catalog(&self) -> ServiceResult<()> {
            self.record("sync_catalog".into());
            if let (Some(catalog), Some(profile)) = (
                self.catalog.lock().unwrap().as_ref(),
                self.synced_profile.lock().unwrap().clone(),
            ) {
                catalog.add_profile(profile);
            }
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
        let processes = Arc::new(IdleProcessMonitor(runtime.clone()));
        let repo = Arc::new(FakeRepo::default());
        *repo.catalog.lock().unwrap() = Some(catalog.clone());
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
                "sync_catalog".into(),
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
                "sync_catalog".into(),
                format!("apply:{id}"),
            ]
        );
        assert!(matches!(runtime.sync_status().state, SyncState::Error));
    }

    #[test]
    fn manual_sync_processes_profiles_imported_from_the_shared_catalog() {
        let (workflow, repo, _, local_id) = setup();
        let remote_profile = Profile {
            id: uuid::Uuid::from_u128(2),
            name: "Remote".into(),
            branch: "profiles/remote".into(),
            last_played_game: None,
        };
        *repo.synced_profile.lock().unwrap() = Some(remote_profile.clone());

        workflow.execute().unwrap();

        let calls = repo.calls.lock().unwrap();
        let sync_catalog = calls
            .iter()
            .position(|call| call == "sync_catalog")
            .unwrap();
        let apply_local = calls
            .iter()
            .position(|call| call == &format!("apply:{local_id}"))
            .unwrap();
        let apply_remote = calls
            .iter()
            .position(|call| call == &format!("apply:{}", remote_profile.id))
            .unwrap();
        assert!(sync_catalog < apply_local && sync_catalog < apply_remote);
    }
}
