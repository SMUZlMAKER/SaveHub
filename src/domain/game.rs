use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Создаётся как UUID v4 при добавлении игры; одинаков на всех ПК и во всех профилях.
pub type GameId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: GameId,
    pub name: String,
    /// Общие пути хранятся в версионируемом каталоге Git, не в служебной папке .git.
    pub paths: GamePaths,
    pub process_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePaths {
    /// Исходная строка может содержать %APPDATA%, %USERPROFILE% и т.п.
    pub save_dir_path: String,
    pub exe_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathResolution {
    Ignore,
    UpdateRepository(GamePaths),
    UseLocal(GamePaths),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameAvailability {
    Ready {
        save_dir_path: PathBuf,
        exe_path: PathBuf,
    },
    /// Один или оба пути отсутствуют на этом ПК.
    Missing {
        save_dir_missing: bool,
        exe_missing: bool,
    },
    Ignored,
}
