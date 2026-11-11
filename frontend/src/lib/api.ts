// Единственное место, где фронт вызывает Rust. Экраны импортируют `api`, а не `invoke`.
// Имя ручки = имя функции с #[tauri::command] в src/api/*.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ApiErrorKind,
  AppPreferences,
  UpdateStatus,
  Conflict,
  ConflictResolution,
  Game,
  GameInput,
  PathAction,
  PathValidation,
  SaveSuggestion,
  Profile,
  Save,
  SaveChoice,
  SaveConflict,
  SaveConflictResolution,
  Settings,
  Snapshot,
  SyncStatus,
} from "./types";

export class BackendError extends Error {
  kind: ApiErrorKind;

  constructor(kind: ApiErrorKind, message: string) {
    super(message);
    this.kind = kind;
  }
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    if (e && typeof e === "object" && "kind" in e && "message" in e) {
      throw new BackendError(e.kind as ApiErrorKind, String(e.message));
    }
    throw new BackendError("internal", `${command}: ${String(e)}`);
  }
}

export function describeError(e: unknown): string {
  if (e instanceof BackendError) {
    switch (e.kind) {
      case "notImplemented":
        return `Бэкенд пока не умеет: ${e.message}`;
      case "rejected":
        return e.message;
      case "internal":
        return `Ошибка: ${e.message}`;
    }
  }
  return String(e);
}

// Ключи аргументов — camelCase: Tauri сам сопоставит их со snake_case-параметрами в Rust.
export const api = {
  getAppPreferences: () => call<AppPreferences>("get_app_preferences"),
  saveAppPreferences: (preferences: AppPreferences) =>
    call<void>("save_app_preferences", {
      preferences: {
        checkOnStartup: preferences.checkOnStartup,
        checkPeriodically: preferences.checkPeriodically,
      },
      launchAtLogin: preferences.launchAtLogin,
    }),
  getUpdateStatus: () => call<UpdateStatus>("get_update_status"),
  checkForUpdates: () => call<UpdateStatus>("check_for_updates"),
  installUpdate: () => call<void>("install_update"),
  // src/api/games.rs
  listGames: () => call<Game[]>("list_games"),
  getGameStatus: (gameId: string) =>
    call<Game["status"]>("get_game_status", { gameId }),
  addGame: (input: GameInput) => call<Game>("add_game", { ...input }),
  updateGame: (gameId: string, input: GameInput) =>
    call<Game>("update_game", { gameId, ...input }),
  resolveGamePaths: (
    gameId: string,
    action: PathAction,
    paths?: Omit<GameInput, "name">,
  ) =>
    call<Game>("resolve_game_paths", {
      gameId,
      action,
      exePath: paths?.exePath ?? null,
      savePath: paths?.savePath ?? null,
    }),
  validateGamePaths: (paths: Omit<GameInput, "name">) =>
    call<PathValidation>("validate_game_paths", { ...paths }),
  suggestSaveDirs: (exePath: string) =>
    call<SaveSuggestion | null>("suggest_save_dirs", { exePath }),

  // src/api/profiles.rs
  listProfiles: () => call<Profile[]>("list_profiles"),
  currentProfile: () => call<Profile>("current_profile"),
  switchProfile: (profileId: string) =>
    call<void>("switch_profile", { profileId }),
  createProfile: (name: string) => call<Profile>("create_profile", { name }),
  deleteProfile: (profileId: string) =>
    call<void>("delete_profile", { profileId }),

  // src/api/saves.rs
  listSaves: (game: string) => call<Save[]>("list_saves", { game }),
  chooseSave: (game: string, choice: SaveChoice) =>
    call<void>("choose_save", { game, choice }),
  getSaveConflict: (game: string) =>
    call<SaveConflict | null>("get_save_conflict", { game }),
  resolveSaveConflict: (
    game: string,
    conflictId: string,
    resolution: SaveConflictResolution,
  ) => call<void>("resolve_save_conflict", { game, conflictId, resolution }),

  // src/api/snapshots.rs
  listSnapshots: (gameId: string | null) =>
    call<Snapshot[]>("list_snapshots", { gameId }),
  restoreSnapshot: (snapshotId: string) =>
    call<void>("restore_snapshot", { snapshotId }),

  // src/api/sync.rs
  getSyncStatus: () => call<SyncStatus>("get_sync_status"),
  syncNow: () => call<void>("sync_now"),
  listConflicts: () => call<Conflict[]>("list_conflicts"),
  resolveConflict: (conflictId: string, resolution: ConflictResolution) =>
    call<void>("resolve_conflict", { conflictId, resolution }),

  // src/api/settings.rs
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) =>
    call<void>("update_settings", { settings }),
  setGithubToken: (token: string) => call<void>("set_github_token", { token }),
};

/** События бэкенда, см. src/api/events.rs. */
export const events = {
  gamesChanged: "games-changed",
  profileChanged: "profile-changed",
  snapshotsChanged: "snapshots-changed",
  syncStatusChanged: "sync-status-changed",
  conflictsChanged: "conflicts-changed",
  navigate: "navigate",
  updateStatusChanged: "update-status-changed",
} as const;

/** Подписка на событие бэкенда. Возвращает функцию отписки — удобно вернуть из onMount. */
export function subscribe<T = unknown>(
  event: string,
  handler: (payload: T) => void,
): () => void {
  const unlisten = listen<T>(event, (e) => handler(e.payload)).catch((e) => {
    console.error(`Не удалось подписаться на событие ${event}`, e);
    return null;
  });
  return () => {
    unlisten.then((stop) => stop?.());
  };
}
