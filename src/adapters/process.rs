#![allow(unused_variables)]
use super::AdapterResult;
use std::path::{Path, PathBuf};

pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
}

pub fn list() -> AdapterResult<Vec<ProcessInfo>> {
    todo!()
}

pub fn spawn(executable: &Path, args: &[String], working_dir: Option<&Path>) -> AdapterResult<u32> {
    todo!()
}

/// Создать Windows .lnk.
pub fn create_shortcut(
    path: &Path,
    executable: &Path,
    args: &[String],
    working_dir: Option<&Path>,
) -> AdapterResult<()> {
    todo!()
}
