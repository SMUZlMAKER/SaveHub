// Типы ответов Rust-ручек из src/api/*. Поля — camelCase, как отдает serde.
// Меняешь DTO в Rust — поправь и здесь.

/** Build capabilities; true does not confirm a repository connection. */
export interface AppFeatures {
  sync: boolean;
  profileSwitch: boolean;
  saveHistory: boolean;
  conflicts: boolean;
  persistentGames: boolean;
  persistentProfiles: boolean;
}

export type GameStatus = "found" | "missing" | "running" | "ignored";

export interface Game {
  id: string;
  name: string;
  savePath: string | null;
  status: GameStatus;
  exePath: string;
  exeMissing: boolean;
  savePathMissing: boolean;
  pathScope: "shared" | "local" | "ignored";
  processStateKnown: boolean;
}

export interface Profile {
  id: string;
  name: string;
  branch: string;
  isActive: boolean;
  gamesCount: number;
  lastPlayedGame: string | null;
}

export interface Save {
  id: string;
  gameId: string;
  profileId: string;
  createdAt: number;
  sizeBytes: number;
}

export interface SaveChoice {
  saveId: string;
  createSave?: boolean;
}

export interface SaveConflict {
  id: string;
  gameId: string;
  local: Save;
  remote: Save;
}

export type SaveConflictResolution =
  | "keepLocal"
  | "keepRemote"
  | "keepBothLocalFirst"
  | "keepBothRemoteFirst";

export type SaveKind =
  | "beforeSwitch"
  | "afterSession"
  | "conflict"
  | "beforeRestore"
  | "fileChanged";

export interface SaveHistoryEntry {
  id: string;
  gameId: string;
  gameName: string;
  profileName: string;
  kind: SaveKind;
  /** Unix-время, мс */
  createdAt: number;
  sizeBytes: number;
}

export type SyncState = "notConfigured" | "idle" | "syncing" | "error";

export interface SyncStatus {
  state: SyncState;
  /** Последний успешный fetch/push без ошибок и ожидающих отправки коммитов, Unix-мс. Не означает применение сохранений. */
  lastSyncAt: number | null;
  error: string | null;
}

export interface SaveVersion {
  machineName: string;
  /** Unix-время, мс */
  modifiedAt: number;
  sizeBytes: number;
}

export interface Conflict {
  id: string;
  gameId: string;
  gameName: string;
  local: SaveVersion;
  remote: SaveVersion;
}

export type ConflictResolution =
  | "keepLocal"
  | "keepRemote"
  | "keepBothLocalFirst"
  | "keepBothRemoteFirst";

export interface Settings {
  catalogRepoUrl: string | null;
  repoUrl: string | null;
  localRepoPath: string | null;
  hasGithubToken: boolean;
  syncAfterSession: boolean;
  syncWhileGaming: boolean;
  autoSwitchProfile: boolean;
  askProfileBeforeLaunch: boolean;
}

export type ApiErrorKind = "notImplemented" | "rejected" | "internal";

export type ScreenId =
  | "games"
  | "profiles"
  | "saves"
  | "settings"
  | "conflicts";

export interface AppPreferences {
  launchAtLogin: boolean;
  checkOnStartup: boolean;
  checkPeriodically: boolean;
  checkBeforeGameLaunch: boolean;
}
export interface UpdateStatus {
  currentVersion: string;
  configured: boolean;
  phase:
    | "idle"
    | "checking"
    | "available"
    | "upToDate"
    | "installing"
    | "restartRequired"
    | "error";
  version: string | null;
  notes: string | null;
  lastCheckedAt: number | null;
  error: string | null;
}

export interface GameInput {
  name: string;
  exePath: string;
  savePath: string;
}
export type PathAction = "useLocal" | "updateRepository" | "ignore";
export interface PathValidation {
  exePath: string | null;
  savePath: string | null;
}
export interface SaveSuggestion {
  game: string;
  saveDirs: string[];
}

export interface LaunchProfile { id: string; name: string; isActive: boolean }
export interface LaunchStatus {
  requestId: string | null;
  gameId: string | null;
  gameName: string | null;
  phase: 'idle' | 'preparing' | 'starting' | 'awaitingProfile' | 'running' | 'stopped' | 'cancelled' | 'error';
  profiles: LaunchProfile[];
  activeProfileId: string | null;
  warnings: string[];
  error: string | null;
  pid: number | null;
  monitorError: string | null;
}
export interface Shortcut { path: string }
export interface GameProcessEvent {
  gameId: string;
  state: 'started' | 'stopped' | 'unknown';
  pid: number | null;
  error: string | null;
}
