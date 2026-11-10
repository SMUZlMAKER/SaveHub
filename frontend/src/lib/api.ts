// Единственное место, где фронт вызывает Rust. Экраны импортируют `api`, а не `invoke`.
// Имя ручки = имя функции с #[tauri::command] в src/api/*.rs.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ApiErrorKind,
  Conflict,
  ConflictResolution,
  Game,
  Profile,
  Settings,
  Snapshot,
  SyncStatus,
} from './types'

export class BackendError extends Error {
  kind: ApiErrorKind

  constructor(kind: ApiErrorKind, message: string) {
    super(message)
    this.kind = kind
  }
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args)
  } catch (e) {
    if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
      throw new BackendError(e.kind as ApiErrorKind, String(e.message))
    }
    throw new BackendError('internal', `${command}: ${String(e)}`)
  }
}

export function describeError(e: unknown): string {
  if (e instanceof BackendError) {
    switch (e.kind) {
      case 'notImplemented':
        return `Бэкенд пока не умеет: ${e.message}`
      case 'rejected':
        return e.message
      case 'internal':
        return `Ошибка: ${e.message}`
    }
  }
  return String(e)
}

// Ключи аргументов — camelCase: Tauri сам сопоставит их со snake_case-параметрами в Rust.
export const api = {
  // src/api/games.rs
  listGames: () => call<Game[]>('list_games'),
  addGame: (name: string, savePath: string) => call<Game>('add_game', { name, savePath }),
  setGameSavePath: (gameId: string, savePath: string) =>
    call<void>('set_game_save_path', { gameId, savePath }),

  // src/api/profiles.rs
  listProfiles: () => call<Profile[]>('list_profiles'),
  switchProfile: (profileId: string) => call<void>('switch_profile', { profileId }),
  createProfile: (name: string) => call<Profile>('create_profile', { name }),
  deleteProfile: (profileId: string) => call<void>('delete_profile', { profileId }),

  // src/api/snapshots.rs
  listSnapshots: (gameId: string | null) => call<Snapshot[]>('list_snapshots', { gameId }),
  restoreSnapshot: (snapshotId: string) => call<void>('restore_snapshot', { snapshotId }),

  // src/api/sync.rs
  getSyncStatus: () => call<SyncStatus>('get_sync_status'),
  syncNow: () => call<void>('sync_now'),
  listConflicts: () => call<Conflict[]>('list_conflicts'),
  resolveConflict: (conflictId: string, resolution: ConflictResolution) =>
    call<void>('resolve_conflict', { conflictId, resolution }),

  // src/api/settings.rs
  getSettings: () => call<Settings>('get_settings'),
  updateSettings: (settings: Settings) => call<void>('update_settings', { settings }),
  setGithubToken: (token: string) => call<void>('set_github_token', { token }),
}

/** События бэкенда, см. src/api/events.rs. */
export const events = {
  gamesChanged: 'games-changed',
  profileChanged: 'profile-changed',
  snapshotsChanged: 'snapshots-changed',
  syncStatusChanged: 'sync-status-changed',
  conflictsChanged: 'conflicts-changed',
  navigate: 'navigate',
} as const

/** Подписка на событие бэкенда. Возвращает функцию отписки — удобно вернуть из onMount. */
export function subscribe<T = unknown>(event: string, handler: (payload: T) => void): () => void {
  const unlisten = listen<T>(event, (e) => handler(e.payload)).catch((e) => {
    console.error(`Не удалось подписаться на событие ${event}`, e)
    return null
  })
  return () => {
    unlisten.then((stop) => stop?.())
  }
}
