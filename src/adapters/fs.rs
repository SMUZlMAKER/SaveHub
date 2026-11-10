#![allow(unused_variables)]
use super::AdapterResult;
use std::path::{Path, PathBuf};

/// Раскрыть Windows-переменные окружения на текущем ПК.
pub fn expand_path(path: &str) -> AdapterResult<PathBuf> {
    todo!()
}

pub fn exists(path: &Path) -> AdapterResult<bool> {
    todo!()
}

pub fn is_directory(path: &Path) -> AdapterResult<bool> {
    todo!()
}

pub fn create_dir_all(path: &Path) -> AdapterResult<()> {
    todo!()
}

pub fn read(path: &Path) -> AdapterResult<Vec<u8>> {
    todo!()
}

/// Записать через временный файл и атомарную замену.
pub fn write_atomic(path: &Path, contents: &[u8]) -> AdapterResult<()> {
    todo!()
}

/// Скопировать дерево, не следуя symlink; ошибка блокировки доступна вызывающему для retry.
pub fn copy_tree(source: &Path, destination: &Path) -> AdapterResult<()> {
    todo!()
}

pub fn rename(source: &Path, destination: &Path) -> AdapterResult<()> {
    todo!()
}

/// Удалить обычное дерево; не следовать ссылкам.
pub fn remove_tree(path: &Path) -> AdapterResult<()> {
    todo!()
}

/// Размер дерева в байтах.
pub fn size(path: &Path) -> AdapterResult<u64> {
    todo!()
}

/// Создать символическую ссылку на каталог.
pub fn create_symlink(link: &Path, target: &Path) -> AdapterResult<()> {
    todo!()
}

pub fn read_symlink(link: &Path) -> AdapterResult<Option<PathBuf>> {
    todo!()
}

/// Удалить только ссылку, сохранив целевой каталог.
pub fn remove_symlink(link: &Path) -> AdapterResult<()> {
    todo!()
}
