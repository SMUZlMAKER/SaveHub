//! Polling watcher. Debounce and retry policy belong to the background worker.
use super::{AdapterError, AdapterResult};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

type Fingerprint = BTreeMap<PathBuf, (u64, Option<SystemTime>)>;
pub struct WatchHandle {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for WatchHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn scan(path: &Path, entries: &mut Fingerprint) -> AdapterResult<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AdapterError::Io(error)),
    };
    if metadata.is_symlink() || super::fs::read_symlink(path)?.is_some() {
        return Err(AdapterError::InvalidData(
            "Watcher requires resolved paths without nested links".into(),
        ));
    }
    entries.insert(path.to_owned(), (metadata.len(), metadata.modified().ok()));
    if metadata.is_dir() {
        for child in std::fs::read_dir(path).map_err(AdapterError::Io)? {
            let child = child.map_err(AdapterError::Io)?;
            if child.file_name() != ".git" {
                scan(&child.path(), entries)?;
            }
        }
    }
    Ok(())
}

fn fingerprint(paths: &[PathBuf]) -> AdapterResult<Fingerprint> {
    let mut entries = BTreeMap::new();
    for path in paths {
        scan(path, &mut entries)?;
    }
    Ok(entries)
}

pub fn watch(
    paths: &[PathBuf],
) -> AdapterResult<(WatchHandle, Receiver<AdapterResult<Vec<PathBuf>>>)> {
    let paths = paths
        .iter()
        .map(|path| {
            if path.exists() {
                super::fs::canonicalize(path)
            } else {
                Ok(path.clone())
            }
        })
        .collect::<AdapterResult<Vec<_>>>()?;
    let mut previous = fingerprint(&paths)?;
    let (sender, events) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("savehub-watch".into())
        .spawn(move || {
            while matches!(
                stopped.recv_timeout(Duration::from_millis(250)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                match fingerprint(&paths) {
                    Ok(current) => {
                        let changes: std::collections::BTreeSet<_> = previous
                            .keys()
                            .chain(current.keys())
                            .filter(|path| previous.get(*path) != current.get(*path))
                            .cloned()
                            .collect();
                        previous = current;
                        if !changes.is_empty()
                            && sender.send(Ok(changes.into_iter().collect())).is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        if sender.send(Err(error)).is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .map_err(AdapterError::Io)?;
    Ok((
        WatchHandle {
            stop,
            thread: Some(thread),
        },
        events,
    ))
}

pub fn stop(mut handle: WatchHandle) -> AdapterResult<()> {
    let _ = handle.stop.send(());
    if let Some(thread) = handle.thread.take() {
        thread
            .join()
            .map_err(|_| AdapterError::InvalidData("Watcher panicked".into()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_creation_and_deletion_and_stops() {
        let root =
            std::env::temp_dir().join(format!("savehub-watch-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (handle, events) = watch(std::slice::from_ref(&root)).unwrap();
        let file = root.join("save");
        std::fs::write(&file, b"save").unwrap();
        assert!(
            events
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .contains(&file)
        );
        std::fs::remove_file(&file).unwrap();
        assert!(
            events
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .contains(&file)
        );
        stop(handle).unwrap();
        assert!(events.recv().is_err());
        std::fs::remove_dir(&root).unwrap();
    }
}
