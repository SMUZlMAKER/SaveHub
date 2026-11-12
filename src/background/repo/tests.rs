use super::*;
use crate::{
    domain::{CommitId, ProfileId, RepositoryConfig, RuntimeState, SyncState, SyncStatus},
    services::repo::RepoOperations,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

const PROFILE_A: ProfileId = uuid::Uuid::from_u128(1);
const PROFILE_B: ProfileId = uuid::Uuid::from_u128(2);

fn profile_name(profile: &ProfileId) -> &'static str {
    match *profile {
        PROFILE_A => "a",
        PROFILE_B => "b",
        _ => panic!("unexpected profile: {profile}"),
    }
}

#[derive(Default)]
struct FakeRepo {
    calls: Mutex<Vec<String>>,
    events: Mutex<Option<mpsc::Sender<String>>>,
    fail_push: AtomicBool,
    fail_fetch: AtomicBool,
    fail_checkpoint: AtomicBool,
    clean: AtomicBool,
    unconfigured: AtomicBool,
    repository: std::sync::atomic::AtomicUsize,
    panic_fetch: AtomicBool,
    creates_commit: AtomicBool,
    checkpoint_block: Mutex<Option<CheckpointBlock>>,
    fetch_delay: Mutex<Duration>,
    fetch_finished: Mutex<Option<Instant>>,
    unpublished_profiles: Mutex<std::collections::BTreeSet<ProfileId>>,
    fail_discovery: AtomicBool,
    disable_auto_push: AtomicBool,
    allow_push_while_gaming: AtomicBool,
}

struct CheckpointBlock {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    panic: bool,
}

impl FakeRepo {
    fn block_next_checkpoint(&self, panic: bool) -> (mpsc::Receiver<()>, mpsc::SyncSender<()>) {
        let (entered, started) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        *self.checkpoint_block.lock().unwrap() = Some(CheckpointBlock {
            entered,
            release: resume,
            panic,
        });
        (started, release)
    }

    fn record(&self, call: String, fail: bool) -> ServiceResult<()> {
        self.calls.lock().unwrap().push(call.clone());
        if let Some(events) = self.events.lock().unwrap().as_ref() {
            let _ = events.send(call);
        }
        if fail {
            Err(ServiceError::Conflict("test failure".into()))
        } else {
            Ok(())
        }
    }

    fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl RepoOperations for FakeRepo {
    fn sync_preferences(&self) -> (bool, bool) {
        (
            !self.disable_auto_push.load(Ordering::SeqCst),
            self.allow_push_while_gaming.load(Ordering::SeqCst),
        )
    }

    fn unpushed_profiles(&self) -> ServiceResult<Vec<ProfileId>> {
        if self.fail_discovery.load(Ordering::SeqCst) {
            return Err(ServiceError::Conflict("cannot inspect profile refs".into()));
        }
        Ok(self
            .unpublished_profiles
            .lock()
            .unwrap()
            .iter()
            .copied()
            .collect())
    }

    fn configuration(&self) -> Option<RepositoryConfig> {
        (!self.unconfigured.load(Ordering::SeqCst)).then(|| RepositoryConfig {
            url: format!("repo-{}", self.repository.load(Ordering::SeqCst)),
            path: "saves".into(),
        })
    }

    fn checkpoint(&self, profile: &ProfileId) -> ServiceResult<Option<CommitId>> {
        let block = self.checkpoint_block.lock().unwrap().take();
        if let Some(block) = block {
            block.entered.send(()).unwrap();
            block.release.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(!block.panic, "test checkpoint panic");
        }
        self.record(
            format!("checkpoint:{}", profile_name(profile)),
            self.fail_checkpoint.load(Ordering::SeqCst),
        )?;
        let created = self.creates_commit.load(Ordering::SeqCst);
        if created || !self.clean.load(Ordering::SeqCst) {
            self.unpublished_profiles.lock().unwrap().insert(*profile);
        }
        Ok(created.then(|| "commit".into()))
    }

    fn has_unpushed_commits(&self, _profile: &ProfileId) -> ServiceResult<bool> {
        Ok(!self.clean.load(Ordering::SeqCst))
    }

    fn apply_profile_updates(&self, _profile: &ProfileId) -> ServiceResult<bool> {
        Ok(false)
    }

    fn push(&self, profile: &ProfileId) -> ServiceResult<()> {
        self.record(
            format!("push:{}", profile_name(profile)),
            self.fail_push.load(Ordering::SeqCst),
        )?;
        self.unpublished_profiles.lock().unwrap().remove(profile);
        Ok(())
    }

    fn fetch(&self) -> ServiceResult<()> {
        assert!(
            !self.panic_fetch.load(Ordering::SeqCst),
            "test worker panic"
        );
        thread::sleep(*self.fetch_delay.lock().unwrap());
        let result = self.record("fetch".into(), self.fail_fetch.load(Ordering::SeqCst));
        *self.fetch_finished.lock().unwrap() = Some(Instant::now());
        result
    }
}

fn runtime() -> Arc<RuntimeState> {
    let runtime = Arc::new(RuntimeState::new(
        Some(PROFILE_A),
        Vec::new(),
        SyncStatus {
            state: SyncState::Idle,
            last_sync_at: None,
            error: None,
        },
    ));
    runtime.set_running_games(Vec::new());
    runtime
}

fn engine(repo: Arc<FakeRepo>) -> (RepoWorkerLoop, Arc<RuntimeState>, Arc<Mutex<()>>) {
    let runtime = runtime();
    let operations = Arc::new(Mutex::new(()));
    let sync = SyncRepository::new(repo, runtime.clone(), operations.clone());

    (
        RepoWorkerLoop::new(sync, RepoWorkerOptions::default()),
        runtime,
        operations,
    )
}

#[test]
fn startup_pushes_existing_commits_even_without_new_checkpoint() {
    let repo = Arc::new(FakeRepo::default());
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    engine.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
}

#[test]
fn push_retry_keeps_profile_and_does_not_repeat_checkpoint() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_push.store(true, Ordering::SeqCst);
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    let now = Instant::now();

    assert!(engine.tick(now, false).is_err());
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
    _runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    repo.fail_push.store(false, Ordering::SeqCst);

    engine.tick(engine.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["push:a"]);
    assert!(!engine.sync.has_pending_push());
}

#[test]
fn network_failure_does_not_block_local_checkpoint() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_fetch.store(true, Ordering::SeqCst);
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    let now = Instant::now();

    assert!(engine.tick(now, false).is_err());
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
    assert!(engine.tick(engine.next_deadline(), false).is_err());
    assert_eq!(repo.take_calls(), ["fetch"]);
}

#[test]
fn unknown_or_running_processes_block_checkpoint_but_allow_fetch() {
    let repo = Arc::new(FakeRepo::default());
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    let now = Instant::now();
    _runtime.invalidate_process_state();

    assert!(matches!(
        engine.tick(now, false),
        Err(ServiceError::ProcessesUnknown)
    ));
    assert_eq!(repo.take_calls(), ["fetch"]);
    _runtime.set_running_games(vec![uuid::Uuid::new_v4()]);
    engine.tick(engine.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["fetch"]);

    _runtime.set_running_games(Vec::new());
    engine.tick(engine.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a"]);
}

#[test]
fn automatic_push_respects_after_session_and_while_gaming_preferences() {
    let repo = Arc::new(FakeRepo::default());
    repo.disable_auto_push.store(true, Ordering::SeqCst);
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    let (mut engine, runtime, _) = engine(repo.clone());
    engine.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "fetch"]);

    engine.force_sync = true;
    engine.tick(engine.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["push:a"]);

    repo.disable_auto_push.store(true, Ordering::SeqCst);
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    let now = Instant::now();
    engine.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        now,
    );
    engine.tick(now, false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "fetch"]);

    repo.disable_auto_push.store(false, Ordering::SeqCst);
    runtime.set_running_games(vec![uuid::Uuid::new_v4()]);
    engine.tick(now + Duration::from_secs(301), false).unwrap();
    assert_eq!(repo.take_calls(), ["fetch"]);

    repo.allow_push_while_gaming.store(true, Ordering::SeqCst);
    engine.tick(now + Duration::from_secs(302), false).unwrap();
    assert_eq!(repo.take_calls(), ["push:a"]);
}

#[test]
fn busy_workflow_defers_worker_and_profile_is_read_after_lock() {
    let repo = Arc::new(FakeRepo::default());
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    let lock = _operations.clone();
    let guard = lock.lock().unwrap();
    let now = Instant::now();

    engine.tick(now, false).unwrap();
    assert!(repo.take_calls().is_empty());
    _runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    drop(guard);
    engine.tick(engine.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:b", "push:b", "fetch"]);
}

#[test]
fn failed_checkpoint_is_retried_without_hot_loop() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_checkpoint.store(true, Ordering::SeqCst);
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    let now = Instant::now();

    assert!(engine.tick(now, false).is_err());
    assert_eq!(repo.take_calls(), ["checkpoint:a", "fetch"]);
    engine.tick(now + Duration::from_secs(1), false).unwrap();
    assert!(repo.take_calls().is_empty());
    repo.fail_checkpoint.store(false, Ordering::SeqCst);
    engine.tick(engine.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a"]);
}

#[test]
fn shutdown_flushes_joins_and_reports_failure() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_push.store(true, Ordering::SeqCst);
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo.clone(), runtime(), Arc::new(Mutex::new(()))),
        RepoWorkerOptions::default(),
    )
    .unwrap();
    assert!(worker.shutdown().is_err());
    let calls = repo.take_calls();
    assert!(calls.iter().any(|call| call == "checkpoint:a"));
    assert!(calls.iter().any(|call| call == "push:a"));
}

#[test]
fn notifications_are_coalesced_and_manual_sync_is_preserved() {
    let handle = RepoWorkerHandle {
        wakeup: Arc::new(Wakeup::default()),
    };
    handle.sync_now().unwrap();
    for _ in 0..1000 {
        handle.wake().unwrap();
    }
    let state = handle.wakeup.state.lock().unwrap();
    assert!(state.requests.sync);
    assert!(state.requests.wake);
}

#[test]
fn busy_mutex_does_not_hang_shutdown() {
    let repo = Arc::new(FakeRepo::default());
    let operations = Arc::new(Mutex::new(()));
    let guard = operations.lock().unwrap();
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo.clone(), runtime(), operations.clone()),
        RepoWorkerOptions::default(),
    )
    .unwrap();
    assert!(worker.shutdown().is_err());
    assert!(repo.take_calls().is_empty());
    assert!(worker.handle().sync_now().is_ok());
    drop(guard);
    worker.shutdown().unwrap();
    assert_eq!(&repo.take_calls()[..2], ["checkpoint:a", "push:a"]);
}

#[test]
fn fetch_success_does_not_clear_an_unresolved_checkpoint_error() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_checkpoint.store(true, Ordering::SeqCst);
    let (mut engine, _runtime, _operations) = engine(repo);
    let now = Instant::now();
    assert!(engine.tick(now, false).is_err());

    engine.fetch_due = now;
    engine.tick(now + Duration::from_secs(1), false).unwrap();
    assert!(matches!(_runtime.sync_status().state, SyncState::Error));
    assert!(_runtime.sync_status().last_sync_at.is_none());
}

#[test]
fn poisoned_operation_lock_never_permits_io() {
    let repo = Arc::new(FakeRepo::default());
    let (mut engine, _runtime, _operations) = engine(repo.clone());
    let operations = _operations.clone();
    let _ = thread::spawn(move || {
        let _guard = operations.lock().unwrap();
        panic!("test poisoned operation");
    })
    .join();

    assert!(engine.tick(Instant::now(), false).is_err());
    assert!(repo.take_calls().is_empty());
}

#[test]
fn idle_worker_waits_until_next_operation_not_polling_interval() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    let (mut worker, _, _) = engine(repo);
    worker.tick(Instant::now(), false).unwrap();

    assert_eq!(worker.next_deadline(), worker.scan_due);
    assert!(worker.next_deadline() > Instant::now() + Duration::from_secs(59));
}

#[test]
fn wake_does_not_bypass_checkpoint_backoff() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_checkpoint.store(true, Ordering::SeqCst);
    let (mut worker, _, _) = engine(repo.clone());
    let now = Instant::now();
    assert!(worker.tick(now, false).is_err());
    repo.take_calls();

    let changed = now + Duration::from_secs(1);
    worker.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        changed,
    );
    worker.tick(changed, false).unwrap();
    assert_eq!(repo.take_calls(), ["fetch"]);

    repo.fail_checkpoint.store(false, Ordering::SeqCst);
    worker.tick(worker.checkpoint_retry_after, false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a"]);
}

#[test]
fn wake_does_not_bypass_busy_mutex_backoff() {
    let repo = Arc::new(FakeRepo::default());
    let (mut worker, _, operations) = engine(repo.clone());
    let guard = operations.lock().unwrap();
    let now = Instant::now();
    worker.tick(now, false).unwrap();
    drop(guard);

    let changed = now + Duration::from_secs(1);
    worker.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        changed,
    );
    worker.tick(changed, false).unwrap();
    assert!(repo.take_calls().is_empty());
    assert_eq!(worker.next_deadline(), worker.operations_retry_after);

    worker.tick(worker.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
}

#[test]
fn retry_delay_starts_after_slow_network_operation() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_fetch.store(true, Ordering::SeqCst);
    *repo.fetch_delay.lock().unwrap() = Duration::from_millis(20);
    let (mut worker, _, _) = engine(repo.clone());
    worker.options.retry_interval = Duration::from_millis(5);

    assert!(worker.tick(Instant::now(), false).is_err());
    let finished = repo.fetch_finished.lock().unwrap().unwrap();
    assert!(worker.fetch_due >= finished + worker.options.retry_interval);
}

#[test]
fn clean_scan_skips_push_but_discovers_external_commits_later() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    worker.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "fetch"]);
    assert!(runtime.sync_status().last_sync_at.is_some());

    repo.clean.store(false, Ordering::SeqCst);
    worker.tick(worker.scan_due, false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a"]);
    assert!(runtime.sync_status().last_sync_at.is_some());
}

#[test]
fn newly_created_commit_is_pushed_without_ahead_query() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    repo.creates_commit.store(true, Ordering::SeqCst);
    let (mut worker, _, _) = engine(repo.clone());
    worker.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
}

#[test]
fn automatic_commits_wait_thirty_minutes_despite_wakes_and_scans() {
    let repo = Arc::new(FakeRepo::default());
    repo.creates_commit.store(true, Ordering::SeqCst);
    let (mut worker, _, _) = engine(repo.clone());
    worker.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
    assert_eq!(worker.options.commit_interval, Duration::from_secs(1800));

    let allowed_at = worker.next_commit_allowed;
    let changed_at = allowed_at - Duration::from_secs(1);
    worker.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        changed_at,
    );
    worker.tick(changed_at, false).unwrap();
    assert_eq!(repo.take_calls(), ["fetch"]);
    assert_eq!(worker.next_deadline(), allowed_at);

    worker.tick(allowed_at, false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a"]);
}

#[test]
fn failed_push_is_retried_while_commits_are_throttled() {
    let repo = Arc::new(FakeRepo::default());
    repo.creates_commit.store(true, Ordering::SeqCst);
    repo.fail_push.store(true, Ordering::SeqCst);
    let (mut worker, _, _) = engine(repo.clone());
    assert!(worker.tick(Instant::now(), false).is_err());
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);

    repo.fail_push.store(false, Ordering::SeqCst);
    assert!(worker.next_deadline() < worker.next_commit_allowed);
    worker.tick(worker.next_deadline(), false).unwrap();
    assert_eq!(repo.take_calls(), ["push:a"]);
}

#[test]
fn manual_sync_and_flush_commit_immediately_and_restart_automatic_interval() {
    for flushing in [false, true] {
        let repo = Arc::new(FakeRepo::default());
        repo.creates_commit.store(true, Ordering::SeqCst);
        let (mut worker, _, _) = engine(repo.clone());
        worker.tick(Instant::now(), false).unwrap();
        repo.take_calls();

        let now = worker.next_commit_allowed - Duration::from_secs(60);
        let (reply, _result) = mpsc::sync_channel(1);
        worker.schedule_requests(
            &Requests {
                sync: !flushing,
                flush: flushing.then_some(FlushRequest {
                    reply,
                    mode: FlushMode::Pause,
                }),
                ..Requests::default()
            },
            now,
        );
        worker.tick(now, flushing).unwrap();
        let calls = repo.take_calls();
        assert_eq!(&calls[..2], ["checkpoint:a", "push:a"]);
        assert_eq!(
            worker.next_commit_allowed,
            now + worker.options.commit_interval
        );
        assert!(!worker.checkpoint_forced);
        assert_eq!(worker.checkpoint_deadline(), worker.next_commit_allowed);
    }
}

#[test]
fn new_commit_is_pushed_even_before_push_retry_deadline() {
    let repo = Arc::new(FakeRepo::default());
    repo.creates_commit.store(true, Ordering::SeqCst);
    let (mut worker, _, _) = engine(repo.clone());
    worker.tick(Instant::now(), false).unwrap();
    repo.take_calls();

    worker.push_due = worker.next_commit_allowed + Duration::from_secs(10);
    worker.tick(worker.next_commit_allowed, false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
}

#[test]
fn empty_request_does_not_publish_success() {
    let repo = Arc::new(FakeRepo::default());
    let runtime = runtime();
    runtime.set_sync_status(SyncStatus {
        state: SyncState::Error,
        last_sync_at: Some(42),
        error: Some("previous error".into()),
    });
    let mut sync = SyncRepository::new(repo.clone(), runtime.clone(), Arc::new(Mutex::new(())));
    sync.execute(SyncRequest::default()).unwrap();

    let status = runtime.sync_status();
    assert!(matches!(status.state, SyncState::Error));
    assert_eq!(status.last_sync_at, Some(42));
    assert_eq!(status.error.as_deref(), Some("previous error"));
    assert!(repo.take_calls().is_empty());
}

#[test]
fn successful_fetch_alone_updates_last_sync_but_failure_preserves_it() {
    let repo = Arc::new(FakeRepo::default());
    let runtime = runtime();
    let mut sync = SyncRepository::new(repo.clone(), runtime.clone(), Arc::new(Mutex::new(())));
    let fetch = || SyncRequest {
        fetch: true,
        ..SyncRequest::default()
    };

    sync.execute(fetch()).unwrap();
    assert_eq!(repo.take_calls(), ["fetch"]);
    let previous = runtime.sync_status().last_sync_at;
    assert!(previous.is_some());
    assert!(matches!(runtime.sync_status().state, SyncState::Idle));

    repo.fail_fetch.store(true, Ordering::SeqCst);
    sync.execute(fetch()).unwrap();
    assert_eq!(runtime.sync_status().last_sync_at, previous);
    assert!(matches!(runtime.sync_status().state, SyncState::Error));
}

#[test]
fn successful_fetch_does_not_publish_sync_with_pending_push() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_push.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    assert!(worker.tick(Instant::now(), false).is_err());
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
    assert!(worker.sync.has_pending_push());
    assert!(runtime.sync_status().last_sync_at.is_none());

    worker
        .sync
        .execute(SyncRequest {
            fetch: true,
            ..SyncRequest::default()
        })
        .unwrap();
    assert_eq!(repo.take_calls(), ["fetch"]);
    assert!(runtime.sync_status().last_sync_at.is_none());
    assert!(matches!(runtime.sync_status().state, SyncState::Error));
}

#[test]
fn fetch_without_active_profile_updates_last_sync() {
    let repo = Arc::new(FakeRepo::default());
    let (mut worker, runtime, _) = engine(repo.clone());
    runtime.set_active_profile(None).unwrap();
    worker.tick(Instant::now(), false).unwrap();

    assert_eq!(repo.take_calls(), ["fetch"]);
    assert!(runtime.sync_status().last_sync_at.is_some());
}

#[test]
fn notifications_and_shutdown_wake_a_live_worker_with_distant_deadlines() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    let (events, received) = mpsc::channel();
    *repo.events.lock().unwrap() = Some(events);
    let options = RepoWorkerOptions {
        scan_interval: Duration::from_secs(3600),
        fetch_interval: Duration::from_secs(3600),
        ..RepoWorkerOptions::default()
    };
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo, runtime(), Arc::new(Mutex::new(()))),
        options,
    )
    .unwrap();
    let handle = worker.handle();
    let next_event = || {
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("worker did not wake")
    };

    assert_eq!(next_event(), "checkpoint:a");
    assert_eq!(next_event(), "fetch");
    assert!(matches!(
        received.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    handle.wake().unwrap();
    assert_eq!(next_event(), "checkpoint:a");
    assert_eq!(next_event(), "fetch");
    handle.sync_now().unwrap();
    assert_eq!(next_event(), "checkpoint:a");
    assert_eq!(next_event(), "fetch");

    let (finished, completion) = mpsc::channel();
    let shutdown = thread::spawn(move || finished.send(worker.shutdown()).unwrap());
    completion
        .recv_timeout(Duration::from_secs(2))
        .expect("shutdown did not wake worker")
        .unwrap();
    shutdown.join().unwrap();
    assert_eq!(next_event(), "checkpoint:a");
}

#[test]
fn unconfigured_worker_skips_git_and_resumes_after_settings_change() {
    let repo = Arc::new(FakeRepo::default());
    repo.unconfigured.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    let now = Instant::now();

    worker.tick(now, false).unwrap();
    assert!(repo.take_calls().is_empty());
    assert!(matches!(
        runtime.sync_status().state,
        SyncState::NotConfigured
    ));
    assert!(worker.next_deadline() >= now + worker.options.scan_interval);

    repo.unconfigured.store(false, Ordering::SeqCst);
    worker.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        now,
    );
    worker.tick(now, false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:a", "push:a", "fetch"]);
    assert!(matches!(runtime.sync_status().state, SyncState::Idle));

    repo.unconfigured.store(true, Ordering::SeqCst);
    worker.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        now,
    );
    worker.tick(now, false).unwrap();
    assert!(repo.take_calls().is_empty());
    assert!(matches!(
        runtime.sync_status().state,
        SyncState::NotConfigured
    ));
}

#[test]
fn unconfigured_worker_shutdown_does_not_attempt_git() {
    let repo = Arc::new(FakeRepo::default());
    repo.unconfigured.store(true, Ordering::SeqCst);
    let runtime = runtime();
    let sync = SyncRepository::new(repo.clone(), runtime.clone(), Arc::new(Mutex::new(())));
    let mut worker = RepoWorker::start(sync, RepoWorkerOptions::default()).unwrap();

    worker.shutdown().unwrap();
    assert!(repo.take_calls().is_empty());
    assert!(matches!(
        runtime.sync_status().state,
        SyncState::NotConfigured
    ));
}

#[test]
fn changing_repository_does_not_send_pending_profiles_to_new_remote() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_push.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    assert!(worker.tick(Instant::now(), false).is_err());
    assert!(worker.sync.has_pending_push());
    repo.take_calls();

    repo.repository.store(1, Ordering::SeqCst);
    // Model the refs of the newly configured clean repository, not the old clone.
    repo.unpublished_profiles.lock().unwrap().clear();
    repo.clean.store(true, Ordering::SeqCst);
    repo.fail_push.store(false, Ordering::SeqCst);
    let now = Instant::now();
    worker.schedule_requests(
        &Requests {
            wake: true,
            ..Requests::default()
        },
        now,
    );
    worker.tick(now, false).unwrap();

    assert_eq!(repo.take_calls(), ["checkpoint:a", "fetch"]);
    assert!(!worker.sync.has_pending_push());
    let status = runtime.sync_status();
    assert!(matches!(status.state, SyncState::Idle));
    assert!(status.error.is_none());
    assert!(status.last_sync_at.is_some());
}

#[test]
fn status_changes_notify_subscribers_without_blocking_operations() {
    let runtime = runtime();
    let mut changes = runtime.subscribe_sync_status();
    runtime.set_sync_status(SyncStatus {
        state: SyncState::Syncing,
        last_sync_at: None,
        error: None,
    });
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    runtime.set_sync_status(SyncStatus {
        state: SyncState::Error,
        last_sync_at: None,
        error: Some("failure".into()),
    });
    assert!(changes.has_changed().unwrap());
    assert!(matches!(runtime.sync_status().state, SyncState::Error));
}

#[test]
fn panicked_worker_reports_failure_and_rejects_further_requests() {
    let repo = Arc::new(FakeRepo::default());
    repo.panic_fetch.store(true, Ordering::SeqCst);
    let runtime = runtime();
    let mut changes = runtime.subscribe_sync_status();
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo, runtime.clone(), Arc::new(Mutex::new(()))),
        RepoWorkerOptions::default(),
    )
    .unwrap();
    let handle = worker.handle();
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                changes.changed().await.unwrap();
                if matches!(runtime.sync_status().state, SyncState::Error) {
                    break;
                }
            }
        })
        .await
        .unwrap();
    });

    assert!(handle.sync_now().is_err());
    assert!(handle.wake().is_err());
    assert!(worker.shutdown().is_err());
}

#[test]
fn shutdown_with_unknown_processes_keeps_worker_available_for_retry() {
    let repo = Arc::new(FakeRepo::default());
    let runtime = runtime();
    runtime.invalidate_process_state();
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo.clone(), runtime.clone(), Arc::new(Mutex::new(()))),
        RepoWorkerOptions::default(),
    )
    .unwrap();

    assert!(matches!(
        worker.shutdown(),
        Err(ServiceError::ProcessesUnknown)
    ));
    assert!(worker.handle().wake().is_ok());
    assert!(
        !repo
            .take_calls()
            .iter()
            .any(|call| call.starts_with("checkpoint:") || call.starts_with("push:"))
    );

    runtime.set_running_games(Vec::new());
    worker.shutdown().unwrap();
    let calls = repo.take_calls();
    assert!(calls.iter().any(|call| call == "checkpoint:a"));
    assert!(calls.iter().any(|call| call == "push:a"));
    assert!(worker.handle().sync_now().is_err());
    worker.shutdown().unwrap();
}

#[test]
fn failed_final_push_can_be_retried_without_recreating_worker() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_push.store(true, Ordering::SeqCst);
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo.clone(), runtime(), Arc::new(Mutex::new(()))),
        RepoWorkerOptions::default(),
    )
    .unwrap();

    assert!(worker.shutdown().is_err());
    assert!(worker.handle().wake().is_ok());
    assert!(repo.take_calls().iter().any(|call| call == "push:a"));

    repo.fail_push.store(false, Ordering::SeqCst);
    worker.shutdown().unwrap();
    assert!(repo.take_calls().iter().any(|call| call == "push:a"));
    assert!(worker.handle().wake().is_err());
}

#[test]
fn updater_flush_reports_failure_and_keeps_worker_running() {
    let repo = Arc::new(FakeRepo::default());
    repo.fail_push.store(true, Ordering::SeqCst);
    let mut worker = RepoWorker::start(
        SyncRepository::new(repo.clone(), runtime(), Arc::new(Mutex::new(()))),
        RepoWorkerOptions::default(),
    )
    .unwrap();
    let handle = worker.handle();

    assert!(handle.pause_after_flush().is_err());
    assert!(handle.wake().is_ok());
    repo.fail_push.store(false, Ordering::SeqCst);
    drop(handle.pause_after_flush().unwrap());
    assert!(handle.wake().is_ok());
    worker.shutdown().unwrap();
}

fn started_worker(repo: &Arc<FakeRepo>) -> RepoWorker {
    let (events, received) = mpsc::channel();
    *repo.events.lock().unwrap() = Some(events);
    let worker = RepoWorker::start(
        SyncRepository::new(repo.clone(), runtime(), Arc::new(Mutex::new(()))),
        RepoWorkerOptions::default(),
    )
    .unwrap();
    while received.recv_timeout(Duration::from_secs(2)).unwrap() != "fetch" {}
    repo.take_calls();
    worker
}

fn assert_queued_flush_is_completed_when_worker_ends(panic: bool) {
    let repo = Arc::new(FakeRepo::default());
    let mut worker = started_worker(&repo);
    let handle = worker.handle();
    let (entered, release) = repo.block_next_checkpoint(panic);
    let shutdown = thread::spawn(move || worker.shutdown());
    entered.recv_timeout(Duration::from_secs(2)).unwrap();

    let (finished, completion) = mpsc::channel();
    let waiting_handle = handle.clone();
    let waiter = thread::spawn(move || finished.send(waiting_handle.pause_after_flush()).unwrap());
    let state = handle.wakeup.state.lock().unwrap();
    let (state, timeout) = handle
        .wakeup
        .ready
        .wait_timeout_while(state, Duration::from_secs(2), |state| {
            state.requests.flush.is_none()
        })
        .unwrap();
    assert!(!timeout.timed_out());
    drop(state);

    release.send(()).unwrap();
    assert!(
        completion
            .recv_timeout(Duration::from_secs(2))
            .expect("queued flush hung")
            .is_err()
    );
    waiter.join().unwrap();
    assert_eq!(shutdown.join().unwrap().is_err(), panic);
}

#[test]
fn queued_flush_gets_an_error_when_in_flight_shutdown_succeeds() {
    assert_queued_flush_is_completed_when_worker_ends(false);
}

#[test]
fn queued_flush_gets_an_error_when_in_flight_operation_panics() {
    assert_queued_flush_is_completed_when_worker_ends(true);
}

#[test]
fn installation_pause_retains_notifications_and_resumes_on_guard_drop() {
    let repo = Arc::new(FakeRepo::default());
    let mut worker = started_worker(&repo);
    let handle = worker.handle();
    let pause = handle.pause_after_flush().unwrap();
    repo.take_calls();
    let (events, received) = mpsc::channel();
    *repo.events.lock().unwrap() = Some(events);

    handle.wake().unwrap();
    handle.sync_now().unwrap();
    assert!(handle.pause_after_flush().is_err());
    assert!(worker.shutdown().is_err());
    assert!(matches!(
        received.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(repo.take_calls().is_empty());

    drop(pause);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(2)).unwrap(),
        "checkpoint:a"
    );
    worker.shutdown().unwrap();
}

#[test]
fn resuming_after_installation_does_not_bypass_commit_interval() {
    let repo = Arc::new(FakeRepo::default());
    repo.creates_commit.store(true, Ordering::SeqCst);
    let mut worker = started_worker(&repo);
    let handle = worker.handle();
    let pause = handle.pause_after_flush().unwrap();
    repo.take_calls();
    let (events, received) = mpsc::channel();
    *repo.events.lock().unwrap() = Some(events);

    handle.wake().unwrap();
    drop(pause);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(2)).unwrap(),
        "fetch"
    );
    assert!(matches!(
        received.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(repo.take_calls(), ["fetch"]);
    worker.shutdown().unwrap();
}

#[test]
fn dropping_pause_guard_does_not_revive_stopped_worker() {
    let repo = Arc::new(FakeRepo::default());
    let worker = started_worker(&repo);
    let handle = worker.handle();
    let pause = handle.pause_after_flush().unwrap();

    drop(worker);
    drop(pause);
    assert!(handle.wake().is_err());
    assert!(handle.sync_now().is_err());
    assert!(handle.pause_after_flush().is_err());
}

#[test]
fn restart_with_another_active_profile_publishes_departing_checkpoint() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    // A workflow committed A and persisted B as active before this worker started.
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    let (mut worker, runtime, _) = engine(repo.clone());
    runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    worker.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:b", "push:a", "fetch"]);
    assert!(!worker.sync.has_pending_push());
}

#[test]
fn flush_discovers_checkpoint_made_by_switch_between_worker_ticks() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    worker.tick(Instant::now(), false).unwrap();
    repo.take_calls();
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    worker.checkpoint_forced = true;
    worker.scan_due = Instant::now();
    worker.push_due = Instant::now();
    worker.tick(Instant::now(), true).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:b", "push:a"]);
}

#[test]
fn failed_inactive_push_is_rediscovered_after_restart_without_recheckpointing_it() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    repo.fail_push.store(true, Ordering::SeqCst);
    let (mut first, runtime, _) = engine(repo.clone());
    runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    assert!(first.tick(Instant::now(), false).is_err());
    assert_eq!(repo.take_calls(), ["checkpoint:b", "push:a", "fetch"]);
    drop(first);
    repo.fail_push.store(false, Ordering::SeqCst);
    let (mut restarted, runtime, _) = engine(repo.clone());
    runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    restarted.tick(Instant::now(), false).unwrap();
    assert_eq!(repo.take_calls(), ["checkpoint:b", "push:a", "fetch"]);
}

#[test]
fn discovery_failure_prevents_false_success_and_retries() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    repo.fail_discovery.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    assert!(worker.tick(Instant::now(), false).is_err());
    assert!(runtime.sync_status().error.is_some());
    assert_eq!(runtime.sync_status().last_sync_at, None);
    repo.fail_discovery.store(false, Ordering::SeqCst);
    repo.take_calls();
    worker.tick(worker.next_deadline(), false).unwrap();
    assert!(repo.take_calls().contains(&"push:a".to_owned()));
}

#[test]
fn deleted_pending_profile_does_not_leave_a_permanent_retry() {
    let repo = Arc::new(FakeRepo::default());
    repo.clean.store(true, Ordering::SeqCst);
    repo.unpublished_profiles.lock().unwrap().insert(PROFILE_A);
    repo.fail_push.store(true, Ordering::SeqCst);
    let (mut worker, runtime, _) = engine(repo.clone());
    runtime.set_active_profile(Some(PROFILE_B)).unwrap();
    assert!(worker.tick(Instant::now(), false).is_err());
    repo.take_calls();
    // ProfileService archived the inactive A between retry attempts.
    repo.unpublished_profiles.lock().unwrap().remove(&PROFILE_A);
    repo.fail_push.store(false, Ordering::SeqCst);
    worker.tick(worker.next_deadline(), false).unwrap();
    assert!(repo.take_calls().is_empty());
    assert!(!worker.sync.has_pending_push());
    assert!(runtime.sync_status().error.is_none());
}
