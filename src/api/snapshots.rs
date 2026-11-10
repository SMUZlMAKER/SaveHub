//! Экран «Снапшоты»: копии сохранений, к которым можно откатиться.
// TODO UI в тг отписал
use serde::Serialize;

use super::{ApiError, ApiResult, stub};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDto {
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    pub profile_name: String,
    pub kind: SnapshotKind,
    /// Unix-время в миллисекундах.
    pub created_at: u64,
    pub size_bytes: u64,
}

/// Почему снапшот создан. Подпись для пользователя выбирает фронт.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotKind {
    /// Перед переключением профиля.
    BeforeSwitch,
    /// Автоматически после игровой сессии.
    AfterSession,
    /// Вторая версия, сохраненная при разрешении конфликта.
    Conflict,
    /// Текущее состояние перед откатом.
    BeforeRestore,
}

/// Снапшоты, новые сверху. `game_id = None` — по всем играм.
/// TODO(backend): SaveService/SnapshotService::list.
#[tauri::command]
pub fn list_snapshots(game_id: Option<String>) -> ApiResult<Vec<SnapshotDto>> {
    let snapshot = |id: &str, game: (&str, &str), kind, minutes, size_mb: u64| SnapshotDto {
        id: id.into(),
        game_id: game.0.into(),
        game_name: game.1.into(),
        profile_name: "Основной".into(),
        kind,
        created_at: stub::minutes_ago(minutes),
        size_bytes: size_mb * 1024 * 1024,
    };
    let hk = ("hollow-knight", "Hollow Knight");
    let sv = ("stardew-valley", "Stardew Valley");
    let all = vec![
        snapshot("s4", hk, SnapshotKind::AfterSession, 25, 3),
        snapshot("s3", sv, SnapshotKind::BeforeSwitch, 95, 12),
        snapshot("s2", sv, SnapshotKind::AfterSession, 180, 12),
        snapshot("s1", hk, SnapshotKind::Conflict, 60 * 26, 3),
    ];
    Ok(all
        .into_iter()
        .filter(|s| game_id.as_deref().is_none_or(|id| s.game_id == id))
        .collect())
}

/// Откатить сохранения игры к снапшоту. Текущее состояние сначала сохраняется снапшотом `BeforeRestore`.
/// TODO(backend): сценарий отката; если игра запущена — `ApiError::Rejected`. После — `SNAPSHOTS_CHANGED`.
#[tauri::command]
pub async fn restore_snapshot(snapshot_id: String) -> ApiResult<()> {
    let _ = snapshot_id;
    Err(ApiError::NotImplemented("restore_snapshot"))
}
