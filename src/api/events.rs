//! События, которые бэкенд шлет фронту (`AppHandle::emit`). Фронт по ним перезапрашивает данные,
//! payload не нужен. Имена продублированы во frontend/src/lib/api.ts.

/// Изменился список игр или их статус (нашелся путь, игра запустилась или закрылась).
pub const GAMES_CHANGED: &str = "games-changed";
/// Сменился активный профиль (в том числе из трея) или список профилей.
pub const PROFILE_CHANGED: &str = "profile-changed";
/// Появились новые снапшоты.
pub const SNAPSHOTS_CHANGED: &str = "snapshots-changed";
/// Изменился статус синхронизации (началась, закончилась, упала).
pub const SYNC_STATUS_CHANGED: &str = "sync-status-changed";
/// Появились или разрешились конфликты сохранений.
pub const CONFLICTS_CHANGED: &str = "conflicts-changed";
/// Открыть в окне конкретный экран. Payload — id экрана: "games", "profiles", "snapshots", "settings", "conflicts".
pub const NAVIGATE: &str = "navigate";
