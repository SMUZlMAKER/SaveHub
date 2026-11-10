use super::{GameId, PathResolution};
use std::{collections::HashMap, path::PathBuf};

pub type ProfileId = String;
pub type SnapshotId = String;
pub type ConflictId = String;
pub type CommitId = String;

#[derive(Debug, Clone)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub branch: String,
    pub last_played_game: Option<GameId>,
}

#[derive(Debug, Clone)]
pub struct SettingsData {
    pub repo_url: Option<String>,
    pub local_repo_path: Option<PathBuf>,
    pub sync_after_session: bool,
    pub sync_while_gaming: bool,
    pub auto_switch_profile: bool,
    pub start_minimized: bool,
}

/// Локальный файл вне Git-репозитория сохранений; ключ — UUID игры, не профиль.
#[derive(Debug, Clone)]
pub struct LocalConfig {
    pub machine_id: String,
    /// В том числе запоминает выбор UpdateRepository.
    pub games: HashMap<GameId, PathResolution>,
}

#[derive(Debug, Clone, Copy)]
pub enum SnapshotKind {
    BeforeSwitch,
    AfterSession,
    Conflict,
    BeforeRestore,
    FileChanged,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub game_id: GameId,
    pub profile_id: ProfileId,
    pub kind: SnapshotKind,
    /// Unix-время в миллисекундах.
    pub created_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct SaveVersion {
    pub machine_id: String,
    pub modified_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy)]
pub enum SyncState {
    NotConfigured,
    Idle,
    Syncing,
    Error,
}

#[derive(Debug, Clone)]
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
