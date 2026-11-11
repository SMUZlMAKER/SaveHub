use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
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
    pub debounce: Duration,
    pub scan_interval: Duration,
    pub fetch_interval: Duration,
    pub retry_interval: Duration,
}

impl Default for RepoWorkerOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(30),
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
    flush: Option<FlushRequest>,
}

struct FlushRequest {
    reply: SyncSender<ServiceResult<()>>,
    mode: FlushMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlushMode {
    Continue,
    Pause,
    Stop,
}

#[derive(Default)]
struct Wakeup {
    stopped: AtomicBool,
    paused: AtomicBool,
    requests: Mutex<Requests>,
    ready: Condvar,
}

impl Wakeup {
    fn close(&self) {
        let pending = {
            let mut requests = self
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.stopped.store(true, Ordering::Release);
            requests.stop = true;
            requests.flush.take()
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
            let mut requests = self.requests.lock().expect("Worker requests poisoned");
            if self.stopped.load(Ordering::Acquire) {
                return Err(ServiceError::Conflict(
                    "Repository worker is stopped".into(),
                ));
            }
            self.paused.store(true, Ordering::Release);
            requests.flush.take()
        };
        if let Some(flush) = pending {
            let _ = flush.reply.try_send(Err(ServiceError::Conflict(
                "Repository worker is paused for installation".into(),
            )));
        }
        Ok(())
    }

    fn resume(&self) {
        let mut requests = self.requests.lock().expect("Worker requests poisoned");
        self.paused.store(false, Ordering::Release);
        if !self.stopped.load(Ordering::Acquire) {
            requests.sync = true;
        }
        self.ready.notify_one();
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

    /// Blocking synchronization barrier; leaves the worker running.
    pub fn flush(&self) -> ServiceResult<()> {
        self.request_flush(FlushMode::Continue)
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
            if self.wakeup.paused.load(Ordering::Acquire) {
                return Err(ServiceError::Conflict(
                    "Repository worker is paused for installation".into(),
                ));
            }
            if requests.flush.is_some() {
                return Err(ServiceError::Conflict(
                    "Repository flush is already requested".into(),
                ));
            }
            requests.flush = Some(FlushRequest { reply, mode });
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
                let requests = wakeup
                    .ready
                    .wait_while(requests, |requests| {
                        wakeup.paused.load(Ordering::Acquire) && !requests.stop
                    })
                    .expect("Worker requests poisoned");
                let timeout = self
                    .next_deadline()
                    .saturating_duration_since(Instant::now());
                let (mut requests, _) = wakeup
                    .ready
                    .wait_timeout_while(requests, timeout, |requests| {
                        !requests.stop
                            && !requests.sync
                            && requests.last_change.is_none()
                            && requests.flush.is_none()
                    })
                    .expect("Worker requests poisoned");

                std::mem::take(&mut *requests)
            };
            if requests.stop {
                return Ok(());
            }

            let now = Instant::now();
            self.schedule_requests(&requests, now);
            let mut result = self.tick(now, requests.flush.is_some());

            if let Some(flush) = requests.flush {
                if result.is_ok() {
                    match flush.mode {
                        FlushMode::Continue => {}
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
        if let Some(changed_at) = requests.last_change {
            self.checkpoint_due = changed_at + self.options.debounce;
        }

        if requests.sync || requests.flush.is_some() {
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

    fn tick(&mut self, now: Instant, flushing: bool) -> ServiceResult<()> {
        if !flushing && now < self.operations_retry_after {
            return Ok(());
        }

        let request = SyncRequest {
            checkpoint: now >= self.checkpoint_deadline(),
            push: now >= self.push_due,
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
                let next_check = finished_at + self.options.scan_interval;
                self.checkpoint_due = next_check;
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
