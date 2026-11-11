//! Чтение: None при отсутствии файла, ошибка при повреждении. Запись атомарная.
use super::AdapterResult;
use crate::domain::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// One application owns the directory until the OS closes this handle, including after a crash.
/// Keep the lock file itself: unlinking it would allow locking different inodes.
pub struct DataDirectory {
    path: PathBuf,
    _lock: fs::File,
}

impl DataDirectory {
    pub fn lock(path: PathBuf) -> AdapterResult<Self> {
        fs::create_dir_all(&path).map_err(super::AdapterError::Io)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join("savehub.lock"))
            .map_err(super::AdapterError::Io)?;
        lock.try_lock().map_err(|error| super::AdapterError::InvalidData(format!(
            "Не удалось заблокировать каталог SaveHub. Закройте другую копию приложения: {error}"
        )))?;
        Ok(Self { path, _lock: lock })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn read_games(path: &Path) -> AdapterResult<Option<Vec<Game>>> {
    read_json(path)
}

pub fn write_games(path: &Path, games: &[Game]) -> AdapterResult<()> {
    write_json(path, games)
}

/// Stage launcher metadata first, but make it addressable by game ID only after
/// publishing the shortcut. A failed publication never replaces an older record.
pub fn publish_game_registration(
    path: &Path,
    game: &Game,
    publish: impl FnOnce() -> AdapterResult<()>,
    rollback: impl FnOnce() -> AdapterResult<()>,
) -> AdapterResult<()> {
    let staged = temporary_path(path);
    write_games(&staged, std::slice::from_ref(game))?;
    let result = match publish() {
        Err(error) => Err(error),
        Ok(()) => match atomic_replace(&staged, path) {
            Ok(()) => Ok(()),
            Err(error) => match rollback() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(super::AdapterError::InvalidData(format!(
                    "Не удалось сохранить регистрацию: {error:?}; не удалось удалить созданный ярлык: {cleanup:?}"
                ))),
            },
        },
    };
    if staged.exists() {
        if let Err(cleanup) = fs::remove_file(&staged) {
            return Err(super::AdapterError::InvalidData(format!(
                "Результат создания ярлыка: {result:?}; не удалось удалить временную регистрацию: {cleanup}"
            )));
        }
    }
    result
}

#[derive(Debug)]
pub enum RegistrationWriteError {
    Unchanged(super::AdapterError),
    LocalOnly {
        write: super::AdapterError,
        rollback: super::AdapterError,
    },
}

/// Publish neither file to Git until both writes have succeeded. If the second
/// replacement fails, restore the old local settings and report rollback failures explicitly.
pub fn write_registration(
    games_path: &Path,
    games: Option<&[Game]>,
    local_path: &Path,
    local: Option<(&LocalConfig, &LocalConfig)>,
) -> Result<(), RegistrationWriteError> {
    if let Some((_, next)) = local {
        write_local_config(local_path, next).map_err(RegistrationWriteError::Unchanged)?;
    }
    if let Some(games) = games {
        if let Err(write) = write_games(games_path, games) {
            if let Some((previous, _)) = local {
                if let Err(rollback) = write_local_config(local_path, previous) {
                    return Err(RegistrationWriteError::LocalOnly { write, rollback });
                }
            }
            return Err(RegistrationWriteError::Unchanged(write));
        }
    }
    Ok(())
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

pub fn read_active_profile(path: &Path) -> AdapterResult<Option<ProfileId>> {
    read_json(path)
}

pub fn write_active_profile(path: &Path, profile: &ProfileId) -> AdapterResult<()> {
    write_json(path, profile)
}

pub fn read_saves(path: &Path) -> AdapterResult<Option<Vec<Save>>> {
    read_json(path)
}

/// Атомарно записать индекс сохранений.
pub fn write_saves(path: &Path, saves: &[Save]) -> AdapterResult<()> {
    write_json(path, saves)
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
    fn registration_rolls_back_local_config_when_games_write_fails() {
        let root =
            std::env::temp_dir().join(format!("savehub-paired-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("games.json")).unwrap();
        let local_path = root.join("local_config.json");
        let previous = LocalConfig {
            machine_id: "before".into(),
            games: Default::default(),
        };
        let next = LocalConfig {
            machine_id: "after".into(),
            games: Default::default(),
        };
        write_local_config(&local_path, &previous).unwrap();
        let result = write_registration(
            &root.join("games.json"),
            Some(&[]),
            &local_path,
            Some((&previous, &next)),
        );
        assert!(matches!(result, Err(RegistrationWriteError::Unchanged(_))));
        assert_eq!(read_local_config(&local_path).unwrap().unwrap(), previous);
        assert!(root.join("games.json").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_directory_lock_child() {
        if let Some(path) = std::env::var_os("SAVEHUB_TEST_LOCK_PATH") {
            assert!(DataDirectory::lock(path.into()).is_err());
        }
    }

    #[test]
    fn data_directory_lock_excludes_other_process_and_releases_on_drop() {
        let root = std::env::temp_dir().join(format!("savehub-lock-{}", uuid::Uuid::new_v4()));
        let owner = DataDirectory::lock(root.clone()).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "adapters::config::tests::data_directory_lock_child",
            ])
            .env("SAVEHUB_TEST_LOCK_PATH", &root)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        drop(owner);
        drop(DataDirectory::lock(root.clone()).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

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
            catalog_repo_url: None,
            repo_url: Some("first".into()),
            local_repo_path: None,
            sync_after_session: true,
            sync_while_gaming: false,
            auto_switch_profile: true,
            ask_profile_before_launch: true,
            start_minimized: false,
        };
        let second = SettingsData {
            catalog_repo_url: None,
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

#[cfg(test)]
mod launcher_tests {
    use super::*;

    fn fixture() -> (PathBuf, Game) {
        let root =
            std::env::temp_dir().join(format!("savehub-registration-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let game = Game {
            id: uuid::Uuid::new_v4(),
            name: "New".into(),
            paths: GamePaths {
                exe_path: "game.exe".into(),
                save_dir_path: "saves".into(),
            },
            process_names: vec![],
        };
        (root, game)
    }

    #[test]
    fn failed_shortcut_preserves_previous_registration_and_removes_staging() {
        for existing in [false, true] {
            let (root, game) = fixture();
            let record = root.join(format!("{}.json", game.id));
            let old = b"previous registration bytes";
            if existing {
                fs::write(&record, old).unwrap();
            }
            let result = publish_game_registration(
                &record,
                &game,
                || {
                    if existing {
                        assert_eq!(fs::read(&record).unwrap(), old);
                    } else {
                        assert!(!record.exists());
                    }
                    Err(super::super::AdapterError::InvalidData(
                        "PowerShell / hard_link failed".into(),
                    ))
                },
                || panic!("Must not remove a shortcut which was never published"),
            );
            assert!(result.is_err());
            if existing {
                assert_eq!(fs::read(&record).unwrap(), old);
            } else {
                assert!(!record.exists());
            }
            assert_eq!(fs::read_dir(&root).unwrap().count(), usize::from(existing));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn commits_registration_only_after_shortcut_publication() {
        let (root, game) = fixture();
        let record = root.join(format!("{}.json", game.id));
        let link = root.join("game.lnk");
        publish_game_registration(
            &record,
            &game,
            || {
                assert!(!record.exists());
                fs::write(&link, b"published shortcut").map_err(super::super::AdapterError::Io)
            },
            || panic!("Successful publication needs no rollback"),
        )
        .unwrap();
        assert_eq!(read_games(&record).unwrap().unwrap()[0].id, game.id);
        assert!(link.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollback_failure_is_reported_and_staging_is_still_removed() {
        let (root, game) = fixture();
        let record = root.join("directory.json");
        fs::create_dir(&record).unwrap();
        let link = root.join("game.lnk");
        let result = publish_game_registration(
            &record,
            &game,
            || fs::write(&link, b"published shortcut").map_err(super::super::AdapterError::Io),
            || {
                Err(super::super::AdapterError::InvalidData(
                    "shortcut is locked".into(),
                ))
            },
        );
        let error = format!("{:?}", result.unwrap_err());
        assert!(error.contains("shortcut is locked"));
        assert!(record.is_dir());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_metadata_commit_removes_published_shortcut_and_staging() {
        let (root, game) = fixture();
        let record = root.join("directory-cannot-be-replaced.json");
        fs::create_dir(&record).unwrap();
        let link = root.join("game.lnk");
        let result = publish_game_registration(
            &record,
            &game,
            || fs::write(&link, b"published shortcut").map_err(super::super::AdapterError::Io),
            || fs::remove_file(&link).map_err(super::super::AdapterError::Io),
        );
        assert!(result.is_err());
        assert!(!link.exists());
        assert!(record.is_dir());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
