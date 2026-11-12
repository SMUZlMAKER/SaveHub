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

impl From<crate::services::ServiceError> for ApiError {
    fn from(error: crate::services::ServiceError) -> Self {
        match error {
            crate::services::ServiceError::NotFound(message)
            | crate::services::ServiceError::InvalidInput(message)
            | crate::services::ServiceError::Conflict(message)
            | crate::services::ServiceError::RemoteUnavailable(message) => {
                ApiError::Rejected(message)
            }
            crate::services::ServiceError::GameRunning(games) => {
                ApiError::Rejected(format!("Идут игры: {games:?}, переключение недоступно"))
            }
            crate::services::ServiceError::ProcessesUnknown => {
                ApiError::Rejected("Состояние процессов неизвестно".into())
            }
            crate::services::ServiceError::Adapter(error) => {
                ApiError::Internal(format!("Ошибка адаптера: {error:?}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_kind_and_message() {
        let json = serde_json::to_value(ApiError::NotImplemented("sync_now")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "notImplemented", "message": "sync_now" })
        );
    }
}
