//! Чтение: None при отсутствии файла, ошибка при повреждении. Запись атомарная.
#![allow(unused_variables)]
use super::AdapterResult;
use crate::domain::*;
use std::path::Path;

pub fn read_games(path: &Path) -> AdapterResult<Option<Vec<Game>>> {
    todo!()
}

pub fn write_games(path: &Path, games: &[Game]) -> AdapterResult<()> {
    todo!()
}

pub fn read_profiles(path: &Path) -> AdapterResult<Option<Vec<Profile>>> {
    todo!()
}

pub fn write_profiles(path: &Path, profiles: &[Profile]) -> AdapterResult<()> {
    todo!()
}

pub fn read_settings(path: &Path) -> AdapterResult<Option<SettingsData>> {
    todo!()
}

/// Атомарно записать настройки без токенов.
pub fn write_settings(path: &Path, settings: &SettingsData) -> AdapterResult<()> {
    todo!()
}

pub fn read_local_config(path: &Path) -> AdapterResult<Option<LocalConfig>> {
    todo!()
}

/// Атомарно записать локальный конфиг.
pub fn write_local_config(path: &Path, config: &LocalConfig) -> AdapterResult<()> {
    todo!()
}

pub fn read_snapshots(path: &Path) -> AdapterResult<Option<Vec<Snapshot>>> {
    todo!()
}

/// Атомарно записать индекс снимков.
pub fn write_snapshots(path: &Path, snapshots: &[Snapshot]) -> AdapterResult<()> {
    todo!()
}
