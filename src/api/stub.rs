//! Помощники для демо-данных в заглушках. Удалить вместе с последней заглушкой.

use std::time::{SystemTime, UNIX_EPOCH};

/// Unix-время в миллисекундах `minutes` минут назад.
pub fn minutes_ago(minutes: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (now.as_millis() as u64).saturating_sub(minutes * 60 * 1000)
}
