use super::{GameId, PathResolution};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

pub type ProfileId = uuid::Uuid;
pub type SaveId = String;
pub type ConflictId = String;
pub type CommitId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub branch: String,
    pub last_played_game: Option<GameId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryConfig {
    pub url: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsData {
    /// Отдельный репозиторий списка игр, единственная рабочая ветка — main.
    #[serde(default)]
    pub catalog_repo_url: Option<String>,
    pub repo_url: Option<String>,
    pub local_repo_path: Option<PathBuf>,
    pub sync_after_session: bool,
    pub sync_while_gaming: bool,
    pub auto_switch_profile: bool,
    #[serde(default = "default_ask_profile_before_launch")]
    pub ask_profile_before_launch: bool,
    pub start_minimized: bool,
}

/// Локальный файл вне Git-репозитория сохранений; ключ — UUID игры, не профиль.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfig {
    pub machine_id: String,
    /// В том числе запоминает выбор UpdateRepository.
    pub games: HashMap<GameId, PathResolution>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SaveKind {
    BeforeSwitch,
    AfterSession,
    Conflict,
    BeforeRestore,
    FileChanged,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GameSession {
    pub id: uuid::Uuid,
    pub started_at: u64,
    pub ended_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Save {
    /// Отсутствует у снимков, созданных до появления учёта сессий.
    #[serde(default)]
    pub session_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub session_started_at: Option<u64>,
    #[serde(default)]
    pub session_ended_at: Option<u64>,
    pub id: SaveId,
    pub game_id: GameId,
    pub profile_id: ProfileId,
    pub kind: SaveKind,
    /// Unix-время в миллисекундах.
    pub created_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveVersion {
    pub machine_id: String,
    pub modified_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub id: ConflictId,
    pub game_id: GameId,
    pub profile_id: ProfileId,
    pub local: SaveVersion,
    pub remote: SaveVersion,
}

#[derive(Debug, Clone)]
pub enum ConflictResolution {
    KeepLocal,
    KeepRemote,
    KeepBothLocalFirst,
    KeepBothRemoteFirst,
    CreateProfile { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    NotConfigured,
    Idle,
    Syncing,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    pub state: SyncState,
    pub last_sync_at: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum GameProcessEvent {
    Started(GameId),
    Stopped(GameId),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileActivationError {
    ProcessesUnknown,
    GamesRunning(Vec<GameId>),
}

fn default_ask_profile_before_launch() -> bool {
    true
}

#[cfg(test)]
mod session_compatibility_tests {
    #[test]
    fn snapshots_without_session_metadata_remain_readable() {
        let save: super::Save = serde_json::from_value(serde_json::json!({
            "id": "old-save", "game_id": uuid::Uuid::nil(), "profile_id": uuid::Uuid::nil(),
            "kind": "AfterSession", "created_at": 1, "size_bytes": 0
        }))
        .unwrap();
        assert!(save.session_id.is_none());
        assert!(save.session_started_at.is_none());
        assert!(save.session_ended_at.is_none());
    }
}
