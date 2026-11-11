//! Экран «Снапшоты»: копии сохранений, к которым можно откатиться.
// TODO UI в тг отписал
use serde::Serialize;

use super::{ApiError, ApiResult};

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
    let _ = game_id;
    Err(ApiError::NotImplemented("list_snapshots"))
}

/// Откатить сохранения игры к снапшоту. Текущее состояние сначала сохраняется снапшотом `BeforeRestore`.
/// TODO(backend): сценарий отката; если игра запущена — `ApiError::Rejected`. После — `SNAPSHOTS_CHANGED`.
#[tauri::command]
pub async fn restore_snapshot(snapshot_id: String) -> ApiResult<()> {
    let _ = snapshot_id;
    Err(ApiError::NotImplemented("restore_snapshot"))
}
