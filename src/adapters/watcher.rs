#![allow(unused_variables)]
use super::AdapterResult;
use std::{path::PathBuf, sync::mpsc::Receiver, time::Duration};

pub struct WatchHandle {
    _private: (),
}

/// Подписаться на изменения и переименования с debounce 5–10 секунд.
pub fn watch(
    paths: &[PathBuf],
    debounce: Duration,
) -> AdapterResult<(WatchHandle, Receiver<Vec<PathBuf>>)> {
    todo!()
}

pub fn stop(handle: WatchHandle) -> AdapterResult<()> {
    todo!()
}
