#![allow(unused_variables)]
use super::AdapterResult;
use crate::domain::CommitId;
use std::path::{Path, PathBuf};

pub enum MergeOutcome {
    UpToDate,
    Merged(CommitId),
    Conflicts(Vec<PathBuf>),
    Diverged,
}

/// Клонировать с ключом системного хранилища учётных данных.
pub fn clone(url: &str, path: &Path, credential_key: &str) -> AdapterResult<()> {
    todo!()
}

pub fn set_remote(repo: &Path, name: &str, url: &str) -> AdapterResult<()> {
    todo!()
}

pub fn fetch(repo: &Path, remote: &str, credential_key: &str) -> AdapterResult<()> {
    todo!()
}

pub fn push(repo: &Path, remote: &str, refspec: &str, credential_key: &str) -> AdapterResult<()> {
    todo!()
}

pub fn list_branches(repo: &Path) -> AdapterResult<Vec<String>> {
    todo!()
}

pub fn create_branch(repo: &Path, branch: &str, from: &str) -> AdapterResult<()> {
    todo!()
}

/// Удалить локальную ветку без force.
pub fn delete_branch(repo: &Path, branch: &str) -> AdapterResult<()> {
    todo!()
}

/// Переключить рабочее дерево; не перезаписывать грязные файлы.
pub fn checkout(repo: &Path, revision: &str) -> AdapterResult<()> {
    todo!()
}

/// from = None создаёт ветку без истории, Some — от указанной ревизии.
pub fn create_worktree(
    repo: &Path,
    branch: &str,
    path: &Path,
    from: Option<&str>,
) -> AdapterResult<()> {
    todo!()
}

pub fn status(repo: &Path) -> AdapterResult<Vec<PathBuf>> {
    todo!()
}

/// Добавить указанные пути в индекс, включая удаления.
pub fn stage(repo: &Path, paths: &[PathBuf]) -> AdapterResult<()> {
    todo!()
}

pub fn commit(repo: &Path, message: &str) -> AdapterResult<CommitId> {
    todo!()
}

pub fn head(repo: &Path) -> AdapterResult<CommitId> {
    todo!()
}

/// При fast_forward_only расхождение возвращает Diverged без изменения дерева.
pub fn merge(repo: &Path, revision: &str, fast_forward_only: bool) -> AdapterResult<MergeOutcome> {
    todo!()
}

pub fn abort_merge(repo: &Path) -> AdapterResult<()> {
    todo!()
}

/// Выгрузить дерево версии для сохранения обеих сторон конфликта.
pub fn export_tree(
    repo: &Path,
    revision: &str,
    path: &Path,
    destination: &Path,
) -> AdapterResult<()> {
    todo!()
}

/// Записать выбранную версию и убрать конфликт индекса.
pub fn resolve_file(repo: &Path, path: &Path, contents: &[u8]) -> AdapterResult<()> {
    todo!()
}
