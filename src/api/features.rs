//! Capabilities of this build, not repository connection or authentication status.
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppFeatures {
    pub sync: bool,
    pub profile_switch: bool,
    pub save_history: bool,
    pub conflicts: bool,
    pub persistent_games: bool,
    pub persistent_profiles: bool,
}

/// Enable a capability only when its complete user scenario is wired up.
#[tauri::command]
pub fn get_app_features() -> AppFeatures {
    AppFeatures {
        sync: false,
        profile_switch: false,
        save_history: false,
        conflicts: false,
        persistent_games: false,
        persistent_profiles: false,
    }
}

#[cfg(test)]
mod tests {
    use crate::api::{ApiError, saves, sync};

    #[test]
    fn unavailable_read_operations_do_not_report_demo_success() {
        assert!(matches!(
            sync::list_conflicts(),
            Err(ApiError::NotImplemented(_))
        ));
        assert!(matches!(
            saves::list_save_history(None),
            Err(ApiError::NotImplemented(_))
        ));
    }
}
