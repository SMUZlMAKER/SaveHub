use crate::domain::GamePaths;

/// Read-only validation: never creates directories or changes saves.
pub fn validate(paths: &GamePaths) -> (Option<String>, Option<String>) {
    fn check(value: &str, directory: bool) -> Option<String> {
        if value.trim().is_empty() {
            return Some("Укажите путь".into());
        }
        let path = match super::fs::expand_path(value.trim()) {
            Ok(path) => path,
            Err(_) => return Some("Не удалось раскрыть путь".into()),
        };
        if !path.is_absolute() {
            return Some("Укажите полный путь".into());
        }
        match std::fs::metadata(path) {
            Ok(m) if directory && m.is_dir() => None,
            Ok(m) if !directory && m.is_file() => None,
            Ok(_) => Some(
                if directory {
                    "Выберите папку"
                } else {
                    "Выберите файл игры"
                }
                .into(),
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Some("Путь не найден на этом ПК".into())
            }
            Err(_) => Some("Не удалось прочитать путь. Проверьте доступ".into()),
        }
    }
    (
        check(&paths.exe_path, false),
        check(&paths.save_dir_path, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_file_for_exe_and_directory_for_saves_without_creating_paths() {
        let root = std::env::temp_dir().join(format!("savehub_paths_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("game.exe");
        std::fs::write(&exe, b"fixture").unwrap();
        let valid = GamePaths {
            exe_path: exe.display().to_string(),
            save_dir_path: root.display().to_string(),
        };
        assert_eq!(validate(&valid), (None, None));
        let swapped = GamePaths {
            exe_path: valid.save_dir_path.clone(),
            save_dir_path: valid.exe_path.clone(),
        };
        let result = validate(&swapped);
        assert!(result.0.is_some() && result.1.is_some());
        let missing = root.join("missing");
        assert!(
            validate(&GamePaths {
                exe_path: valid.exe_path,
                save_dir_path: missing.display().to_string()
            })
            .1
            .is_some()
        );
        assert!(!missing.exists());
        assert_eq!(std::fs::read(exe).unwrap(), b"fixture");
        std::fs::remove_dir_all(root).unwrap();
    }
}
