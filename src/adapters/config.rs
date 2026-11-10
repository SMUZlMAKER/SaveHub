#![allow(unused_variables)]
use super::AdapterResult;
use crate::domain::*;
use std::path::Path;

pub fn read_catalog(path: &Path) -> AdapterResult<Vec<Game>> {
    todo!()
}

/// Атомарно сериализовать общий каталог; выбор Git-ветки делает сервис.
pub fn write_catalog(path: &Path, catalog: &Vec<Game>) -> AdapterResult<()> {
    todo!()
}

pub fn read_settings(path: &Path) -> AdapterResult<SettingsData> {
    todo!()
}

/// Атомарно записать настройки без токенов.
pub fn write_settings(path: &Path, settings: &SettingsData) -> AdapterResult<()> {
    todo!()
}

pub fn read_local_config(path: &Path) -> AdapterResult<LocalConfig> {
    todo!()
}

/// Атомарно записать локальный конфиг.
pub fn write_local_config(path: &Path, config: &LocalConfig) -> AdapterResult<()> {
    todo!()
}

pub fn read_snapshots(path: &Path) -> AdapterResult<Vec<Snapshot>> {
    todo!()
}

/// Атомарно записать индекс снимков.
pub fn write_snapshots(path: &Path, snapshots: &[Snapshot]) -> AdapterResult<()> {
    todo!()
}

/// Удалить файл маркера после проверки владельца сервисом.
pub fn remove_lease(path: &Path) -> AdapterResult<()> {
    todo!()
}
