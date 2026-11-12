use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    services::{ServiceError, ServiceResult},
    workflows::sync_repository::{SyncAttempt, SyncOutcome, SyncRepository, SyncRequest},
};

#[derive(Clone, Copy)]
pub struct RepoWorkerOptions {
    pub commit_interval: Duration,
    pub scan_interval: Duration,
    pub fetch_interval: Duration,
    pub retry_interval: Duration,
}

impl Default for RepoWorkerOptions {
    fn default() -> Self {
        Self {
            commit_interval: Duration::from_secs(30 * 60),
            scan_interval: Duration::from_secs(60),
            fetch_interval: Duration::from_secs(300),
            retry_interval: Duration::from_secs(10),
        }
    }
}

#[derive(Default)]
struct Requests {
    wake: bool,
    sync: bool,
    flush: Option<FlushRequest>,
}

struct FlushRequest {
    reply: SyncSender<ServiceResult<()>>,
    mode: FlushMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlushMode {
    Pause,
    Stop,
}

#[derive(Default, PartialEq, Eq)]
enum WorkerStatus {
    #[default]
    Running,
    Paused,
    Stopped,
}

#[derive(Default)]
struct WorkerState {
    status: WorkerStatus,
    requests: Requests,
}

#[derive(Default)]
struct Wakeup {
    state: Mutex<WorkerState>,
    ready: Condvar,
}

impl Wakeup {
    fn close(&self) {
        let pending = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.status = WorkerStatus::Stopped;
            state.requests.flush.take()
        };

        if let Some(flush) = pending {
            let _ = flush.reply.try_send(Err(ServiceError::Conflict(
                "Repository worker is stopped".into(),
            )));
        }
        self.ready.notify_all();
    }

    fn pause(&self) -> ServiceResult<()> {
        let pending = {
            let mut state = self.state.lock().expect("Worker state poisoned");
            if state.status == WorkerStatus::Stopped {
                return Err(ServiceError::Conflict(
                    "Repository worker is stopped".into(),
                ));
            }
            state.status = WorkerStatus::Paused;
            state.requests.flush.take()
        };

        if let Some(flush) = pending {
            let _ = flush.reply.try_send(Err(ServiceError::Conflict(
                "Repository worker is paused for installation".into(),
            )));
        }
        Ok(())
    }

    fn resume(&self) {
        let mut state = self.state.lock().expect("Worker state poisoned");
        if state.status == WorkerStatus::Paused {
            state.status = WorkerStatus::Running;
            state.requests.wake = true;
            self.ready.notify_one();
        }
    }
}

/// Keeps the worker paused without holding the repository mutex.
pub struct RepoWorkerPause {
    wakeup: Arc<Wakeup>,
}

impl Drop for RepoWorkerPause {
    fn drop(&mut self) {
        self.wakeup.resume();
    }
}

#[derive(Clone)]
pub struct RepoWorkerHandle {
    wakeup: Arc<Wakeup>,
}

impl RepoWorkerHandle {
    pub fn wake(&self) -> ServiceResult<()> {
        self.request_sync(false)
    }

    pub fn sync_now(&self) -> ServiceResult<()> {
        self.request_sync(true)
    }

    fn request_sync(&self, force_checkpoint: bool) -> ServiceResult<()> {
        let mut state = self.wakeup.state.lock().expect("Worker state poisoned");
        if state.status == WorkerStatus::Stopped {
            return Err(ServiceError::Conflict(
                "Repository worker is stopped".into(),
            ));
        }

        state.requests.wake = true;
        state.requests.sync |= force_checkpoint;
        self.wakeup.ready.notify_one();
        Ok(())
    }

    pub fn pause_after_flush(&self) -> ServiceResult<RepoWorkerPause> {
        self.request_flush(FlushMode::Pause)?;
        Ok(RepoWorkerPause {
            wakeup: self.wakeup.clone(),
        })
    }

    fn request_flush(&self, mode: FlushMode) -> ServiceResult<()> {
        let (reply, result) = mpsc::sync_channel(1);
        {
            let mut state = self.wakeup.state.lock().expect("Worker state poisoned");
            if state.status == WorkerStatus::Stopped {
                return Err(ServiceError::Conflict(
                    "Repository worker is stopped".into(),
                ));
            }
            if state.status == WorkerStatus::Paused {
                return Err(ServiceError::Conflict(
                    "Repository worker is paused for installation".into(),
                ));
            }
            if state.requests.flush.is_some() {
                return Err(ServiceError::Conflict(
                    "Repository flush is already requested".into(),
                ));
            }
            state.requests.flush = Some(FlushRequest { reply, mode });
            self.wakeup.ready.notify_one();
        }
        result
            .recv()
            .map_err(|_| ServiceError::Conflict("Repository worker stopped during flush".into()))?
    }

    fn stop(&self) {
        self.wakeup.close();
    }
}

pub struct RepoWorker {
    handle: RepoWorkerHandle,
    thread: Option<JoinHandle<ServiceResult<()>>>,
}

impl RepoWorker {
    pub fn start(sync: SyncRepository, options: RepoWorkerOptions) -> std::io::Result<Self> {
        if options.commit_interval.is_zero()
            || options.scan_interval.is_zero()
            || options.fetch_interval.is_zero()
            || options.retry_interval.is_zero()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Worker intervals must be positive",
            ));
        }

        let handle = RepoWorkerHandle {
            wakeup: Arc::new(Wakeup::default()),
        };
        let wakeup = handle.wakeup.clone();
        let thread = thread::Builder::new()
            .name("savehub-repo".into())
            .spawn(move || {
                let mut worker_loop = RepoWorkerLoop::new(sync, options);
                let result = catch_unwind(AssertUnwindSafe(|| worker_loop.run(wakeup.clone())));
                wakeup.close();
                match result {
                    Ok(result) => result,
                    Err(_) => {
                        worker_loop.sync.worker_stopped();
                        Err(ServiceError::Conflict("Repository worker panicked".into()))
                    }
                }
            })?;

        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }

    pub fn handle(&self) -> RepoWorkerHandle {
        self.handle.clone()
    }

    /// Errors leave the worker running so shutdown can be retried.
    /// Call on a blocking thread; service I/O must have its own timeouts.
    pub fn shutdown(&mut self) -> ServiceResult<()> {
        if self.thread.is_none() {
            return Ok(());
        }

        self.handle.request_flush(FlushMode::Stop)?;
        self.thread
            .take()
            .expect("Worker thread missing")
            .join()
            .map_err(|_| ServiceError::Conflict("Repository worker panicked".into()))?
    }
}

impl Drop for RepoWorker {
    fn drop(&mut self) {
        // Dropping only stops the thread; shutdown is required for a final flush.
        self.handle.stop();
    }
}

struct RepoWorkerLoop {
    sync: SyncRepository,
    options: RepoWorkerOptions,
    scan_due: Instant,
    push_due: Instant,
    fetch_due: Instant,
    checkpoint_retry_after: Instant,
    next_commit_allowed: Instant,
    checkpoint_forced: bool,
    force_sync: bool,
    operations_retry_after: Instant,
}

impl RepoWorkerLoop {
    fn new(sync: SyncRepository, options: RepoWorkerOptions) -> Self {
        let now = Instant::now();

        Self {
            sync,
            options,
            scan_due: now,
            push_due: now,
            fetch_due: now,
            checkpoint_retry_after: now,
            next_commit_allowed: now,
            checkpoint_forced: false,
            force_sync: false,
            operations_retry_after: now,
        }
    }

    fn run(&mut self, wakeup: Arc<Wakeup>) -> ServiceResult<()> {
        loop {
            let requests = {
                let state = wakeup.state.lock().expect("Worker state poisoned");
                let state = wakeup
                    .ready
                    .wait_while(state, |state| state.status == WorkerStatus::Paused)
                    .expect("Worker state poisoned");
                let timeout = self
                    .next_deadline()
                    .saturating_duration_since(Instant::now());
                let (mut state, _) = wakeup
                    .ready
                    .wait_timeout_while(state, timeout, |state| {
                        state.status == WorkerStatus::Running
                            && !state.requests.wake
                            && !state.requests.sync
                            && state.requests.flush.is_none()
                    })
                    .expect("Worker state poisoned");

                if state.status == WorkerStatus::Stopped {
                    return Ok(());
                }
                std::mem::take(&mut state.requests)
            };

            let now = Instant::now();
            self.schedule_requests(&requests, now);
            let mut result = self.tick(now, requests.flush.is_some());

            if let Some(flush) = requests.flush {
                if result.is_ok() {
                    match flush.mode {
                        FlushMode::Pause => result = wakeup.pause(),
                        FlushMode::Stop => wakeup.close(),
                    }
                }
                let succeeded = result.is_ok();
                if flush.reply.send(result).is_err() && succeeded && flush.mode == FlushMode::Pause
                {
                    wakeup.resume();
                }
                if succeeded && flush.mode == FlushMode::Stop {
                    return Ok(());
                }
            }
        }
    }

    fn schedule_requests(&mut self, requests: &Requests, now: Instant) {
        let force_checkpoint = requests.sync || requests.flush.is_some();
        self.force_sync |= force_checkpoint;

        if requests.wake || force_checkpoint {
            self.scan_due = now;
            self.push_due = now;
            self.fetch_due = now;
        }

        if force_checkpoint {
            self.checkpoint_forced = true;
            self.checkpoint_retry_after = now;
            self.operations_retry_after = now;
        }
    }

    fn checkpoint_deadline(&self) -> Instant {
        let deadline = self.scan_due.max(self.checkpoint_retry_after);

        if self.checkpoint_forced {
            deadline
        } else {
            deadline.max(self.next_commit_allowed)
        }
    }

    fn next_deadline(&self) -> Instant {
        let checkpoint_due = if self.sync.can_checkpoint() {
            self.checkpoint_deadline()
        } else {
            self.fetch_due
        };
        let mut deadline = checkpoint_due.min(self.fetch_due);

        if self.sync.has_pending_push() && (self.force_sync || self.sync.auto_push_allowed()) {
            deadline = deadline.min(self.push_due);
        }

        deadline.max(self.operations_retry_after)
    }

    fn tick(&mut self, now: Instant, flushing: bool) -> ServiceResult<()> {
        if !flushing && now < self.operations_retry_after {
            return Ok(());
        }

        let request = SyncRequest {
            checkpoint: flushing
                || (self.sync.can_checkpoint() && now >= self.checkpoint_deadline()),
            push: now >= self.push_due && (self.force_sync || self.sync.auto_push_allowed()),
            fetch: now >= self.fetch_due && !flushing,
        };

        if !request.checkpoint && !(request.push && self.sync.has_pending_push()) && !request.fetch
        {
            return Ok(());
        }

        let attempt = self.sync.execute(request);
        let finished_at = Instant::now().max(now);

        match attempt {
            Ok(SyncAttempt::NotConfigured) => {
                self.checkpoint_forced = false;
                let next_check = finished_at + self.options.scan_interval;
                self.scan_due = next_check;
                self.push_due = next_check;
                self.fetch_due = next_check;
                Ok(())
            }
            Ok(SyncAttempt::Busy) => {
                self.operations_retry_after = finished_at + self.options.retry_interval;
                if flushing {
                    Err(ServiceError::Conflict(
                        "Repository operations lock unavailable during flush".into(),
                    ))
                } else {
                    Ok(())
                }
            }
            Ok(SyncAttempt::Completed(outcome)) => {
                self.force_sync = false;
                self.schedule_next_attempts(finished_at, &outcome);
                outcome.into_result()
            }
            Err(error) => {
                self.operations_retry_after = finished_at + self.options.retry_interval;
                Err(error)
            }
        }
    }

    fn schedule_next_attempts(&mut self, now: Instant, outcome: &SyncOutcome) {
        if matches!(outcome.checkpoint, Some(Ok(true))) {
            self.next_commit_allowed = now + self.options.commit_interval;
        }

        if let Some(result) = &outcome.checkpoint {
            self.checkpoint_forced = false;
            self.scan_due = now + self.next_interval(result, self.options.scan_interval);
            self.checkpoint_retry_after = if result.is_err() { self.scan_due } else { now };
        }

        if outcome.push.is_some() {
            self.push_due = now + self.options.retry_interval;
        }

        if let Some(result) = &outcome.fetch {
            self.fetch_due = now + self.next_interval(result, self.options.fetch_interval);
        }
    }

    fn next_interval<T>(&self, result: &ServiceResult<T>, regular: Duration) -> Duration {
        if result.is_ok() {
            regular
        } else {
            self.options.retry_interval
        }
    }
}

#[cfg(test)]
mod tests;
