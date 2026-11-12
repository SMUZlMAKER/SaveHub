// Единственное место, где фронт вызывает Rust. Экраны импортируют `api`, а не `invoke`.
// Имя ручки = имя функции с #[tauri::command] в src/api/*.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createSubscription, type Subscription } from "./subscription";
import type {
  ApiErrorKind,
  ManualSavePreferences,
  ManualSaveStatus,
  AppFeatures,
  AppPreferences,
  UpdateStatus,
  Conflict,
  ConflictResolution,
  Game,
  GameInput,
  LaunchStatus,
  Shortcut,
  PathAction,
  PathValidation,
  SaveSuggestion,
  Profile,
  Save,
  SaveChoice,
  SaveConflict,
  SaveConflictResolution,
  Settings,
  SaveHistoryEntry,
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
        return "Эта функция пока недоступна в установленной версии SaveHub.";
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
  getManualSavePreferences: () => call<ManualSavePreferences>("get_manual_save_preferences"),
  getManualSaveStatus: () => call<ManualSaveStatus>("get_manual_save_status"),
  saveManualSaveShortcut: (shortcut: string) => call<void>("save_manual_save_shortcut", { shortcut }),
  createManualSave: (gameId: string | null = null) => call<void>("create_manual_save", { gameId }),
  getAppFeatures: () => call<AppFeatures>("get_app_features"),
  dismissLaunchDialog: () => call<void>("dismiss_launch_dialog"),
  getLaunchStatus: () => call<LaunchStatus>("get_launch_status"),
  createGameShortcut: (gameId: string) => call<Shortcut>("create_game_shortcut", { gameId }),
  beginGameLaunch: (gameId: string) => call<LaunchStatus>("begin_game_launch", { gameId }),
  confirmGameLaunch: (requestId: string, profileId: string) => call<LaunchStatus>("confirm_game_launch", { requestId, profileId }),
  cancelGameLaunch: (requestId: string) => call<void>("cancel_game_launch", { requestId }),
  getAppPreferences: () => call<AppPreferences>("get_app_preferences"),
  saveAppPreferences: (preferences: AppPreferences) =>
    call<void>("save_app_preferences", {
      preferences: {
        checkOnStartup: preferences.checkOnStartup,
        checkPeriodically: preferences.checkPeriodically,
        checkBeforeGameLaunch: preferences.checkBeforeGameLaunch,
      },
      launchAtLogin: preferences.launchAtLogin,
    }),
  getUpdateStatus: () => call<UpdateStatus>("get_update_status"),
  checkForUpdates: () => call<UpdateStatus>("check_for_updates"),
  installUpdate: () => call<void>("install_update"),
  // src/api/games.rs
  getCatalogStatus: () => call<SyncStatus>("get_catalog_status"),
  syncCatalog: () => call<void>("sync_catalog"),
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
  createProfile: (name: string): Promise<Profile> =>
    call<Profile>("create_profile", { name }),
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

  // src/api/saves.rs
  listSaveHistory: (gameId: string | null) =>
    call<SaveHistoryEntry[]>("list_save_history", { gameId }),
  restoreSave: (saveId: string) =>
    call<void>("restore_save", { saveId }),

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
  launchStatusChanged: 'launch-status-changed',
  gameProcessChanged: 'game-process-changed',
  gamesChanged: "games-changed",
  catalogStatusChanged: "catalog-status-changed",
  profileChanged: "profile-changed",
  savesChanged: "saves-changed",
  syncStatusChanged: "sync-status-changed",
  conflictsChanged: "conflicts-changed",
  navigate: "navigate",
  updateStatusChanged: "update-status-changed",
} as const;

/** Подписка на событие бэкенда. Возвращает функцию отписки — удобно вернуть из onMount. */
export function subscribe<T = unknown>(
  event: string,
  handler: (payload: T) => void,
  initialRead?: () => void,
): Subscription {
  return createSubscription<T>(
    (handler) => listen<T>(event, (e) => handler(e.payload)),
    handler,
    initialRead,
    (error) => console.error(`Не удалось подписаться на событие ${event}`, error),
  );
}
