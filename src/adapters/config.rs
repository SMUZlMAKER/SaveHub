//! Чтение: None при отсутствии файла, ошибка при повреждении. Запись атомарная.
use super::AdapterResult;
use crate::domain::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn read_games(path: &Path) -> AdapterResult<Option<Vec<Game>>> {
    read_json(path)
}

pub fn write_games(path: &Path, games: &[Game]) -> AdapterResult<()> {
    write_json(path, games)
}

pub fn read_profiles(path: &Path) -> AdapterResult<Option<Vec<Profile>>> {
    read_json(path)
}

pub fn write_profiles(path: &Path, profiles: &[Profile]) -> AdapterResult<()> {
    write_json(path, profiles)
}

pub fn read_settings(path: &Path) -> AdapterResult<Option<SettingsData>> {
    read_json(path)
}

/// Атомарно записать настройки без токенов.
pub fn write_settings(path: &Path, settings: &SettingsData) -> AdapterResult<()> {
    write_json(path, settings)
}

pub fn read_local_config(path: &Path) -> AdapterResult<Option<LocalConfig>> {
    read_json(path)
}

/// Атомарно записать локальный конфиг.
pub fn write_local_config(path: &Path, config: &LocalConfig) -> AdapterResult<()> {
    write_json(path, config)
}

pub fn read_snapshots(path: &Path) -> AdapterResult<Option<Vec<Snapshot>>> {
    read_json(path)
}

/// Атомарно записать индекс снимков.
pub fn write_snapshots(path: &Path, snapshots: &[Snapshot]) -> AdapterResult<()> {
    write_json(path, snapshots)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> AdapterResult<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(super::AdapterError::Io)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| super::AdapterError::InvalidData(format!("{}: {error}", path.display())))
}

fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> AdapterResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(super::AdapterError::Io)?;
    }
    let contents = serde_json::to_vec_pretty(value)
        .map_err(|error| super::AdapterError::InvalidData(error.to_string()))?;
    let temporary = temporary_path(path);
    fs::write(&temporary, contents).map_err(super::AdapterError::Io)?;
    if let Err(error) = atomic_replace(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    path.with_file_name(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()))
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> super::AdapterResult<()> {
    fs::rename(source, destination).map_err(super::AdapterError::Io)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> super::AdapterResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(super::AdapterError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrites_existing_json_atomically() {
        let path = std::env::temp_dir().join(format!(
            "savehub-config-test-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before Unix epoch")
                .as_nanos()
        ));
        let first = SettingsData {
            repo_url: Some("first".into()),
            local_repo_path: None,
            sync_after_session: true,
            sync_while_gaming: false,
            auto_switch_profile: true,
            ask_profile_before_launch: true,
            start_minimized: false,
        };
        let second = SettingsData {
            repo_url: Some("second".into()),
            ..first.clone()
        };

        write_settings(&path, &first).expect("first write");
        write_settings(&path, &second).expect("replacement write");
        let loaded = read_settings(&path)
            .expect("read")
            .expect("settings file exists");

        assert_eq!(loaded.repo_url, second.repo_url);
        let _ = fs::remove_file(path);
    }
}
