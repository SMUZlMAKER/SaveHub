use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, TryLockError},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    domain::{ProfileId, RepositoryConfig, RuntimeState, SyncState, SyncStatus},
    services::{ServiceError, ServiceResult, repo::RepoOperations},
};

#[derive(Default)]
pub(crate) struct SyncRequest {
    pub checkpoint: bool,
    pub push: bool,
    pub fetch: bool,
}

/// None means the operation was skipped.
#[derive(Default)]
pub(crate) struct SyncOutcome {
    pub checkpoint: Option<ServiceResult<bool>>,
    pub push: Option<ServiceResult<()>>,
    pub fetch: Option<ServiceResult<()>>,
}

impl SyncOutcome {
    pub fn into_result(self) -> ServiceResult<()> {
        self.checkpoint
            .unwrap_or(Ok(false))
            .map(|_| ())
            .and(self.push.unwrap_or(Ok(())))
            .and(self.fetch.unwrap_or(Ok(())))
    }
}

pub(crate) enum SyncAttempt {
    NotConfigured,
    Busy,
    Completed(SyncOutcome),
}

pub struct SyncRepository {
    repo: Arc<dyn RepoOperations>,
    runtime: Arc<RuntimeState>,
    operations: Arc<Mutex<()>>,
    repository: Option<RepositoryConfig>,
    pending_push: BTreeSet<ProfileId>,
    checkpoint_error: Option<String>,
    push_error: Option<String>,
    fetch_error: Option<String>,
}

impl SyncRepository {
    /// Use the same operations mutex as all save/profile mutation workflows.
    /// Those workflows must checkpoint the departing profile before switching.
    pub fn new(
        repo: Arc<dyn RepoOperations>,
        runtime: Arc<RuntimeState>,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            repo,
            runtime,
            operations,
            repository: None,
            pending_push: BTreeSet::new(),
            checkpoint_error: None,
            push_error: None,
            fetch_error: None,
        }
    }

    pub fn has_pending_push(&self) -> bool {
        !self.pending_push.is_empty()
    }

    pub(crate) fn execute(&mut self, mut request: SyncRequest) -> ServiceResult<SyncAttempt> {
        if !request.checkpoint && !request.push && !request.fetch {
            return Ok(SyncAttempt::Completed(SyncOutcome::default()));
        }

        let operations = self.operations.clone();
        let guard = match operations.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Ok(SyncAttempt::Busy),
            Err(TryLockError::Poisoned(_)) => {
                let error = ServiceError::Conflict("Repository operations lock poisoned".into());
                self.publish_result(Some(&error), false);
                return Err(error);
            }
        };

        let configuration = self.repo.configuration();
        if self.repository != configuration {
            self.repository = configuration;
            self.pending_push.clear();
            self.checkpoint_error = None;
            self.push_error = None;
            self.fetch_error = None;
            self.runtime.set_sync_status(SyncStatus {
                state: SyncState::Idle,
                last_sync_at: None,
                error: None,
            });
            request.push = true;
        }

        if self.repository.is_none() {
            let previous = self.runtime.sync_status();
            self.runtime.set_sync_status(SyncStatus {
                state: SyncState::NotConfigured,
                error: None,
                ..previous
            });
            return Ok(SyncAttempt::NotConfigured);
        }

        let previous = self.runtime.sync_status();
        self.runtime.set_sync_status(SyncStatus {
            state: SyncState::Syncing,
            error: None,
            ..previous
        });
        let mut outcome = SyncOutcome::default();

        if request.checkpoint {
            let result = self.checkpoint_current_profile();
            self.checkpoint_error = error_message(&result);
            outcome.checkpoint = Some(result);
        }

        // SwitchProfile and conflict resolution can commit outside this worker.
        // Rebuild discovery from Git refs instead of relying on the old active UUID
        // or the in-memory retry queue, which disappears on application restart.
        if request.push || request.checkpoint {
            match self.repo.unpushed_profiles() {
                Ok(profiles) => {
                    // Authoritative refs also remove profiles deleted or published
                    // outside the worker since an earlier failed push.
                    self.pending_push = profiles.into_iter().collect();
                    if self.pending_push.is_empty() {
                        self.push_error = None;
                    }
                }
                Err(error) => {
                    self.push_error = Some(format!("{error:?}"));
                    outcome.push = Some(Err(error));
                }
            }
        }

        if outcome.push.is_none()
            && (request.push || matches!(outcome.checkpoint, Some(Ok(true))))
            && self.has_pending_push()
        {
            let result = self.push_pending_profiles();
            self.push_error = error_message(&result);
            outcome.push = Some(result);
        }

        if request.fetch {
            let result = self.repo.fetch();
            self.fetch_error = error_message(&result);
            outcome.fetch = Some(result);
        }

        let exchanged =
            matches!(outcome.push, Some(Ok(()))) || matches!(outcome.fetch, Some(Ok(())));
        // Publish under the lock so a newer operation cannot be overwritten.
        self.publish_result(None, exchanged);
        drop(guard);
        Ok(SyncAttempt::Completed(outcome))
    }

    pub(crate) fn worker_stopped(&self) {
        self.publish_result(
            Some(&ServiceError::Conflict(
                "Repository worker stopped after a panic".into(),
            )),
            false,
        );
    }

    fn push_pending_profiles(&mut self) -> ServiceResult<()> {
        let mut result = Ok(());

        for profile in self.pending_push.clone() {
            match self.repo.push(&profile) {
                Ok(()) => {
                    self.pending_push.remove(&profile);
                }
                Err(error) => {
                    result = result.and(Err(error));
                }
            }
        }

        result
    }

    fn checkpoint_current_profile(&mut self) -> ServiceResult<bool> {
        let (profile, running) = self.runtime.profile_and_running_games();
        let Some(profile) = profile else {
            return Ok(false);
        };
        let games = running.ok_or(ServiceError::ProcessesUnknown)?;

        if !games.is_empty() {
            return Err(ServiceError::GameRunning(games));
        }

        if !self.runtime.conflicts().is_empty() {
            return Err(ServiceError::Conflict(
                "Resolve save conflicts before checkpointing".into(),
            ));
        }

        let commit = self.repo.checkpoint(&profile)?;

        if commit.is_some() || self.repo.has_unpushed_commits(&profile)? {
            self.pending_push.insert(profile);
        }
        Ok(commit.is_some())
    }

    fn publish_result(&self, failure: Option<&ServiceError>, exchanged: bool) {
        let previous = self.runtime.sync_status();
        let error = failure
            .map(|error| format!("{error:?}"))
            .or_else(|| self.checkpoint_error.clone())
            .or_else(|| self.push_error.clone())
            .or_else(|| self.fetch_error.clone());
        let completed = exchanged && error.is_none() && self.pending_push.is_empty();

        self.runtime.set_sync_status(SyncStatus {
            state: if error.is_some() {
                SyncState::Error
            } else if self.pending_push.is_empty() {
                SyncState::Idle
            } else {
                SyncState::Syncing
            },
            last_sync_at: if completed {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|elapsed| elapsed.as_millis() as u64)
            } else {
                previous.last_sync_at
            },
            error,
        });
    }
}

fn error_message<T>(result: &ServiceResult<T>) -> Option<String> {
    result.as_ref().err().map(|error| format!("{error:?}"))
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use crate::{adapters::git, domain::*, services::repo::RepoService};
    use std::collections::HashMap;

    #[test]
    fn discovers_inactive_local_refs_without_creating_remote_only_worktrees() {
        let root = std::env::temp_dir().join(format!("savehub-discover-{}", uuid::Uuid::new_v4()));
        let repository = git2::Repository::init(&root).unwrap();
        let base = git::commit_files(&root, None, &[], "base").unwrap();
        let changed = git::commit_files(&root, Some(&base), &[("save", b"new")], "save").unwrap();
        git::create_branch(&root, "a", &changed).unwrap();
        git::create_branch(&root, "b", &base).unwrap();
        for name in ["a", "b", "remote-only"] {
            repository
                .reference(
                    &format!("refs/remotes/origin/{name}"),
                    git2::Oid::from_str(&base).unwrap(),
                    false,
                    "fixture",
                )
                .unwrap();
        }
        let a = uuid::Uuid::from_u128(1);
        let b = uuid::Uuid::from_u128(2);
        let c = uuid::Uuid::from_u128(3);
        let catalog = Arc::new(Catalog::new(
            HashMap::new(),
            [(a, "a"), (b, "b"), (c, "remote-only")]
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
            Some(b),
            vec![],
            SyncStatus {
                state: SyncState::Idle,
                last_sync_at: None,
                error: None,
            },
        ));
        let settings = Arc::new(Settings::new(
            SettingsData {
                catalog_repo_url: None,
                repo_url: Some("https://github.com/test/saves".into()),
                local_repo_path: Some(root.clone()),
                sync_after_session: true,
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
        let service = RepoService::new(catalog.clone(), runtime.clone(), settings.clone());
        assert_eq!(service.unpushed_profiles().unwrap(), vec![a]);
        let restarted = RepoService::new(catalog, runtime, settings);
        assert_eq!(restarted.unpushed_profiles().unwrap(), vec![a]);
        assert!(!root.join(".git/savehub/trees").exists());
        assert!(
            repository
                .find_branch("remote-only", git2::BranchType::Local)
                .is_err()
        );
        drop(repository);
        std::fs::remove_dir_all(root).unwrap();
    }
}
