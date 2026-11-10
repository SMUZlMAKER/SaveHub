use serde::Serialize;

pub type ApiResult<T> = Result<T, ApiError>;

/// Ошибка, которую получает фронт. Сериализуется как `{ "kind": "...", "message": "..." }`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum ApiError {
    /// Ручка объявлена, но бэкенд для нее еще не написан. В сообщении — имя ручки.
    NotImplemented(&'static str),
    /// Действие нельзя выполнить сейчас, например переключить профиль во время игры.
    /// Сообщение показывается пользователю как есть.
    Rejected(String),
    /// Любая другая ошибка сервисов или адаптеров.
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_kind_and_message() {
        let json = serde_json::to_value(ApiError::NotImplemented("sync_now")).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "notImplemented", "message": "sync_now" }));
    }
}
