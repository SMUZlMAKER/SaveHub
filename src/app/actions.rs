//! Общие действия окна и трея. Не имитировать успех неподключённых сценариев.
use crate::{
    api::{ApiError, ApiResult},
    domain::ProfileId,
};
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub fn profile_switch_blocker(
    available: bool,
    running: Option<&[crate::domain::GameId]>,
) -> Option<&'static str> {
    match running {
        Some(games) if !games.is_empty() => {
            Some("Идёт игра. Закройте все игры перед переключением профиля.")
        }
        None => {
            Some("Не удалось проверить запущенные игры. Переключение профиля временно недоступно.")
        }
        _ if !available => {
            Some("Переключение сохранений между профилями пока недоступно в этой версии SaveHub.")
        }
        _ => None,
    }
}

pub fn switch_profile(app: &tauri::AppHandle, profile_id: ProfileId) -> ApiResult<()> {
    let result = app
        .state::<Arc<crate::workflows::SwitchProfile>>()
        .execute(&profile_id);
    // Refresh after errors too: the workflow can publish conflicts before stopping.
    for event in [
        crate::api::events::PROFILE_CHANGED,
        crate::api::events::SAVES_CHANGED,
        crate::api::events::CONFLICTS_CHANGED,
    ] {
        let _ = app.emit(event, ());
    }
    result.map_err(|error| match error {
        crate::services::ServiceError::GameRunning(_) => {
            ApiError::Rejected("Идёт игра. Закройте все игры перед переключением профиля.".into())
        }
        crate::services::ServiceError::ProcessesUnknown => ApiError::Rejected(
            "Не удалось проверить запущенные игры. Переключение профиля временно недоступно."
                .into(),
        ),
        other => other.into(),
    })
}

pub fn sync_now(app: &tauri::AppHandle) -> ApiResult<()> {
    let result = app
        .state::<Arc<crate::workflows::manual_sync::ManualSync>>()
        .execute()
        .map_err(|error| match error {
            crate::services::ServiceError::GameRunning(_) => {
                ApiError::Rejected("Закройте все игры перед синхронизацией.".into())
            }
            crate::services::ServiceError::ProcessesUnknown => ApiError::Rejected(
                "Не удалось проверить запущенные игры. Синхронизация отменена.".into(),
            ),
            other => other.into(),
        });
    for event in [
        crate::api::events::SYNC_STATUS_CHANGED,
        crate::api::events::CONFLICTS_CHANGED,
        crate::api::events::SAVES_CHANGED,
    ] {
        let _ = app.emit(event, ());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switch_is_blocked_for_running_unknown_and_unavailable_states() {
        let game = uuid::Uuid::new_v4();
        for available in [false, true] {
            assert!(
                profile_switch_blocker(available, None)
                    .unwrap()
                    .contains("проверить")
            );
            assert!(
                profile_switch_blocker(available, Some(&[game]))
                    .unwrap()
                    .contains("Идёт игра")
            );
        }
        assert!(
            profile_switch_blocker(false, Some(&[]))
                .unwrap()
                .contains("пока недоступно")
        );
        assert_eq!(profile_switch_blocker(true, Some(&[])), None);
    }
}
