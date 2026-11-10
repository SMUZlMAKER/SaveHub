#![allow(unused_variables)]
use super::AdapterResult;
use std::{path::PathBuf, sync::mpsc::Receiver};

pub struct WatchHandle {
    _private: (),
}

/// Передавать пути изменённых файлов и ошибки наблюдения без debounce.
pub fn watch(
    paths: &[PathBuf],
) -> AdapterResult<(WatchHandle, Receiver<AdapterResult<Vec<PathBuf>>>)> {
    todo!()
}

pub fn stop(handle: WatchHandle) -> AdapterResult<()> {
    todo!()
}
