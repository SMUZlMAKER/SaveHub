// Типы ответов Rust-ручек из src/api/*. Поля — camelCase, как отдает serde.
// Меняешь DTO в Rust — поправь и здесь.

export type GameStatus = 'found' | 'missing' | 'running'

export interface Game {
  id: string
  name: string
  savePath: string | null
  status: GameStatus
}

export interface Profile {
  id: string
  name: string
  isActive: boolean
  gamesCount: number
  lastPlayedGame: string | null
}

export interface Save {
  id: string
  gameId: string
  profileId: string
  createdAt: number
  sizeBytes: number
}

export interface SaveChoice {
  saveId: string
  createSnapshot?: boolean
}

export interface SaveConflict {
  id: string
  gameId: string
  local: Save
  remote: Save
}

export type SaveConflictResolution =
  | 'keepLocal'
  | 'keepRemote'
  | 'keepBothLocalFirst'
  | 'keepBothRemoteFirst'

export type SnapshotKind = 'beforeSwitch' | 'afterSession' | 'conflict' | 'beforeRestore'

export interface Snapshot {
  id: string
  gameId: string
  gameName: string
  profileName: string
  kind: SnapshotKind
  /** Unix-время, мс */
  createdAt: number
  sizeBytes: number
}

export type SyncState = 'notConfigured' | 'idle' | 'syncing' | 'error'

export interface SyncStatus {
  state: SyncState
  /** Unix-время, мс */
  lastSyncAt: number | null
  error: string | null
}

export interface SaveVersion {
  machineName: string
  /** Unix-время, мс */
  modifiedAt: number
  sizeBytes: number
}

export interface Conflict {
  id: string
  gameId: string
  gameName: string
  local: SaveVersion
  remote: SaveVersion
}

export type ConflictResolution =
  | 'keepLocal'
  | 'keepRemote'
  | 'keepBothLocalFirst'
  | 'keepBothRemoteFirst'

export interface Settings {
  repoUrl: string | null
  localRepoPath: string | null
  hasGithubToken: boolean
  syncAfterSession: boolean
  syncWhileGaming: boolean
  autoSwitchProfile: boolean
}

export type ApiErrorKind = 'notImplemented' | 'rejected' | 'internal'

export type ScreenId = 'games' | 'profiles' | 'snapshots' | 'settings' | 'conflicts'

export interface AppPreferences {
  launchAtLogin: boolean
  checkOnStartup: boolean
  checkPeriodically: boolean
}
export interface UpdateStatus {
  currentVersion: string
  configured: boolean
  phase: 'idle' | 'checking' | 'available' | 'upToDate' | 'installing' | 'error'
  version: string | null
  notes: string | null
  lastCheckedAt: number | null
  error: string | null
}

/** Значение токена, запрашиваемое только при явном раскрытии в настройках. */
export type GithubToken = string | null
