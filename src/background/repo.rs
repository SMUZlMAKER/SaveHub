use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
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
    pub debounce: Duration,
    pub scan_interval: Duration,
    pub fetch_interval: Duration,
    pub retry_interval: Duration,
}

impl Default for RepoWorkerOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(2),
            scan_interval: Duration::from_secs(60),
            fetch_interval: Duration::from_secs(300),
            retry_interval: Duration::from_secs(10),
        }
    }
}

#[derive(Default)]
struct Requests {
    last_change: Option<Instant>,
    sync: bool,
    stop: bool,
}

#[derive(Default)]
struct Wakeup {
    stopped: AtomicBool,
    requests: Mutex<Requests>,
    ready: Condvar,
}

#[derive(Clone)]
pub struct RepoWorkerHandle {
    wakeup: Arc<Wakeup>,
}

impl RepoWorkerHandle {
    /// Forward relevant save-file events here, excluding Git's own files.
    pub fn files_changed(&self) -> ServiceResult<()> {
        let mut requests = self
            .wakeup
            .requests
            .lock()
            .expect("Worker requests poisoned");
        if self.wakeup.stopped.load(Ordering::Acquire) {
            return Err(ServiceError::Conflict(
                "Repository worker is stopped".into(),
            ));
        }
        requests.last_change = Some(Instant::now());
        self.wakeup.ready.notify_one();
        Ok(())
    }

    pub fn sync_now(&self) -> ServiceResult<()> {
        let mut requests = self
            .wakeup
            .requests
            .lock()
            .expect("Worker requests poisoned");
        if self.wakeup.stopped.load(Ordering::Acquire) {
            return Err(ServiceError::Conflict(
                "Repository worker is stopped".into(),
            ));
        }
        requests.sync = true;
        self.wakeup.ready.notify_one();
        Ok(())
    }

    fn stop(&self) {
        let mut requests = self
            .wakeup
            .requests
            .lock()
            .expect("Worker requests poisoned");
        self.wakeup.stopped.store(true, Ordering::Release);
        requests.stop = true;
        self.wakeup.ready.notify_one();
    }
}

pub struct RepoWorker {
    handle: RepoWorkerHandle,
    thread: Option<JoinHandle<ServiceResult<()>>>,
}

impl RepoWorker {
    pub fn start(sync: SyncRepository, options: RepoWorkerOptions) -> std::io::Result<Self> {
        if options.scan_interval.is_zero()
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
                wakeup.stopped.store(true, Ordering::Release);
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

    /// Flush once and join. Call on a blocking thread, never on the UI thread.
    /// In-flight service calls must have their own I/O timeouts.
    pub fn shutdown(mut self) -> ServiceResult<()> {
        self.handle.stop();
        self.thread
            .take()
            .expect("Worker thread missing")
            .join()
            .map_err(|_| ServiceError::Conflict("Repository worker panicked".into()))?
    }
}

impl Drop for RepoWorker {
    fn drop(&mut self) {
        // Explicit shutdown is needed to wait for the flush and observe its result.
        self.handle.stop();
    }
}

struct RepoWorkerLoop {
    sync: SyncRepository,
    options: RepoWorkerOptions,
    checkpoint_due: Instant,
    scan_due: Instant,
    push_due: Instant,
    fetch_due: Instant,
    checkpoint_retry_after: Instant,
    operations_retry_after: Instant,
}

impl RepoWorkerLoop {
    fn new(sync: SyncRepository, options: RepoWorkerOptions) -> Self {
        let now = Instant::now();

        Self {
            sync,
            options,
            checkpoint_due: now,
            scan_due: now,
            push_due: now,
            fetch_due: now,
            checkpoint_retry_after: now,
            operations_retry_after: now,
        }
    }

    fn run(&mut self, wakeup: Arc<Wakeup>) -> ServiceResult<()> {
        loop {
            let requests = {
                let requests = wakeup.requests.lock().expect("Worker requests poisoned");
                let timeout = self
                    .next_deadline()
                    .saturating_duration_since(Instant::now());
                let (mut requests, _) = wakeup
                    .ready
                    .wait_timeout_while(requests, timeout, |requests| {
                        !requests.stop && !requests.sync && requests.last_change.is_none()
                    })
                    .expect("Worker requests poisoned");

                std::mem::take(&mut *requests)
            };
            let now = Instant::now();

            self.schedule_requests(&requests, now);

            let result = self.tick(now, requests.stop);

            if requests.stop {
                return result;
            }
        }
    }

    fn schedule_requests(&mut self, requests: &Requests, now: Instant) {
        if let Some(changed_at) = requests.last_change {
            self.checkpoint_due = changed_at + self.options.debounce;
        }

        if requests.sync || requests.stop {
            self.checkpoint_due = now;
            self.push_due = now;
            self.fetch_due = now;
            self.checkpoint_retry_after = now;
            self.operations_retry_after = now;
        }
    }

    fn checkpoint_deadline(&self) -> Instant {
        self.checkpoint_due
            .min(self.scan_due)
            .max(self.checkpoint_retry_after)
    }

    fn next_deadline(&self) -> Instant {
        let mut deadline = self.checkpoint_deadline().min(self.fetch_due);

        if self.sync.has_pending_push() {
            deadline = deadline.min(self.push_due);
        }

        deadline.max(self.operations_retry_after)
    }

    fn tick(&mut self, now: Instant, stopping: bool) -> ServiceResult<()> {
        if !stopping && now < self.operations_retry_after {
            return Ok(());
        }

        let request = SyncRequest {
            checkpoint: now >= self.checkpoint_deadline(),
            push: now >= self.push_due,
            fetch: now >= self.fetch_due && !stopping,
        };

        if !request.checkpoint && !(request.push && self.sync.has_pending_push()) && !request.fetch
        {
            return Ok(());
        }

        let attempt = self.sync.execute(request);
        let finished_at = Instant::now().max(now);

        match attempt {
            Ok(SyncAttempt::NotConfigured) => {
                let next_check = finished_at + self.options.scan_interval;
                self.checkpoint_due = next_check;
                self.scan_due = next_check;
                self.push_due = next_check;
                self.fetch_due = next_check;
                Ok(())
            }
            Ok(SyncAttempt::Busy) if stopping => Err(ServiceError::Conflict(
                "Repository operations lock unavailable during shutdown".into(),
            )),
            Ok(SyncAttempt::Busy) => {
                self.operations_retry_after = finished_at + self.options.retry_interval;
                Ok(())
            }
            Ok(SyncAttempt::Completed(outcome)) => {
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
        if let Some(result) = &outcome.checkpoint {
            self.scan_due = now + self.next_interval(result, self.options.scan_interval);
            self.checkpoint_due = self.scan_due;
            self.checkpoint_retry_after = if result.is_err() { self.scan_due } else { now };
        }

        if outcome.push.is_some() {
            self.push_due = now + self.options.retry_interval;
        }

        if let Some(result) = &outcome.fetch {
            self.fetch_due = now + self.next_interval(result, self.options.fetch_interval);
        }
    }

    fn next_interval(&self, result: &ServiceResult<()>, regular: Duration) -> Duration {
        if result.is_ok() {
            regular
        } else {
            self.options.retry_interval
        }
    }
}

#[cfg(test)]
mod tests;
