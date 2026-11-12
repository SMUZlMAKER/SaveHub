use crate::{app::catalog, domain::SyncState, services::game::GameService};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Default)]
pub struct CatalogWorker {
    wake: Arc<Notify>,
    immediate: Arc<AtomicBool>,
}

impl CatalogWorker {
    /// Coalesce requests, including one arriving while synchronization is running.
    pub fn request_sync(&self) {
        self.immediate.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }

    /// A foreground sync changed the status: recalculate its next retry/poll,
    /// without issuing a redundant immediate network request.
    pub fn reschedule(&self) {
        self.wake.notify_one();
    }

    async fn wait(&self, delay: Option<Duration>) -> Wake {
        let signalled = match delay {
            Some(delay) => tokio::time::timeout(delay, self.wake.notified())
                .await
                .is_ok(),
            None => {
                self.wake.notified().await;
                true
            }
        };
        if !signalled {
            Wake::Timer
        } else if self.immediate.swap(false, Ordering::Relaxed) {
            Wake::Immediate
        } else {
            Wake::Reschedule
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Wake {
    Timer,
    Immediate,
    Reschedule,
}

fn next_delay(state: SyncState, failures: &mut u32) -> Option<Duration> {
    match state {
        SyncState::NotConfigured => {
            *failures = 0;
            None
        }
        SyncState::Error => {
            let minutes = (1_u64 << (*failures).min(4)).min(15);
            *failures = failures.saturating_add(1);
            Some(Duration::from_secs(minutes * 60))
        }
        _ => {
            *failures = 0;
            Some(POLL_INTERVAL)
        }
    }
}

/// Local edits already synchronize immediately. Poll only for remote edits;
/// configuration changes wake this worker without waiting for the timer.
pub fn spawn(app: tauri::AppHandle, service: Arc<GameService>) -> CatalogWorker {
    let worker = CatalogWorker::default();
    let handle = worker.clone();
    tauri::async_runtime::spawn(async move {
        let mut failures = 0;
        loop {
            service.mark_sync_pending();
            catalog::notify_status(&app);
            let previous_revision = service.revision();
            let working = service.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                crate::adapters::diagnostics::request_sync(None, "catalog_sync", || {
                    let result = working.sync_catalog();
                    if let Err(error) = &result {
                        crate::services::log_error(error);
                    }
                    result
                })
            })
            .await;
            catalog::notify_changes(&app, &service, previous_revision);
            loop {
                let delay = next_delay(service.status().state, &mut failures);
                match handle.wait(delay).await {
                    Wake::Timer => break,
                    Wake::Immediate => {
                        failures = 0;
                        break;
                    }
                    Wake::Reschedule => {
                        failures = 0;
                    }
                }
            }
        }
    });
    worker
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_poll_and_error_retries_have_different_intervals() {
        let mut failures = 0;
        for minutes in [1, 2, 4, 8, 15, 15] {
            assert_eq!(
                next_delay(SyncState::Error, &mut failures),
                Some(Duration::from_secs(minutes * 60))
            );
        }
        assert_eq!(
            next_delay(SyncState::Idle, &mut failures),
            Some(POLL_INTERVAL)
        );
        assert_eq!(
            next_delay(SyncState::Error, &mut failures),
            Some(Duration::from_secs(60))
        );
        assert_eq!(next_delay(SyncState::NotConfigured, &mut failures), None);
        assert_eq!(failures, 0);
    }

    #[test]
    fn configuration_signal_is_kept_and_wakes_without_poll_delay() {
        let worker = CatalogWorker::default();
        worker.request_sync();
        tauri::async_runtime::block_on(async {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), worker.wait(None))
                    .await
                    .unwrap(),
                Wake::Immediate
            );
            let waiting = worker.wait(Some(POLL_INTERVAL));
            worker.request_sync();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), waiting)
                    .await
                    .unwrap(),
                Wake::Immediate
            );
            worker.reschedule();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), worker.wait(Some(POLL_INTERVAL)))
                    .await
                    .unwrap(),
                Wake::Reschedule
            );
            worker.request_sync();
            worker.reschedule();
            assert_eq!(worker.wait(None).await, Wake::Immediate);
        });
    }
}
