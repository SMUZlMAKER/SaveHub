//! События, которые бэкенд шлет фронту (`AppHandle::emit`). Фронт по ним перезапрашивает данные,
//! Для событий без payload фронт перезапрашивает данные. Имена продублированы во frontend/src/lib/api.ts.

/// Изменился список игр или их статус (нашелся путь, игра запустилась или закрылась).
pub const GAMES_CHANGED: &str = "games-changed";
/// Состояние синхронизации каталога; не требует перечитывать список игр.
pub const CATALOG_STATUS_CHANGED: &str = "catalog-status-changed";
/// Сменился активный профиль (в том числе из трея) или список профилей.
pub const PROFILE_CHANGED: &str = "profile-changed";
/// Появились новые сохранения.
pub const SAVES_CHANGED: &str = "saves-changed";
/// Изменился статус синхронизации (началась, закончилась, упала).
pub const SYNC_STATUS_CHANGED: &str = "sync-status-changed";
/// Появились или разрешились конфликты сохранений.
pub const CONFLICTS_CHANGED: &str = "conflicts-changed";
/// Открыть в окне конкретный экран. Payload — id экрана: "games", "profiles", "saves", "settings", "conflicts".
pub const NAVIGATE: &str = "navigate";

/// Payload — UpdateStatus: состояние проверки или установки обновления.
pub const UPDATE_STATUS_CHANGED: &str = "update-status-changed";

/// Payload — LaunchStatus: подготовка, выбор профиля и результат запуска.
pub const LAUNCH_STATUS_CHANGED: &str = "launch-status-changed";
/// Payload — GameProcessDto: gameId, state (started/stopped/unknown), pid, error.
pub const GAME_PROCESS_CHANGED: &str = "game-process-changed";
