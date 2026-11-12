//! Pre-launch policy. Tauri, update installation and UI belong to app/.
use crate::{
    domain::{GameId, Profile, ProfileId, Settings},
    services::{ServiceError, ServiceResult, process::ProcessService, profile::ProfileService},
};
use std::sync::Arc;

pub struct LaunchGame {
    processes: Arc<ProcessService>,
    profiles: Arc<ProfileService>,
    settings: Arc<Settings>,
    saves: Option<Arc<crate::services::save::SaveService>>,
    switch: Option<Arc<crate::workflows::SwitchProfile>>,
    operations: Arc<std::sync::Mutex<()>>,
}

pub struct LaunchPlan {
    pub profiles: Vec<Profile>,
    pub active_profile: Option<ProfileId>,
    pub needs_profile: bool,
}

impl LaunchGame {
    pub fn new(
        processes: Arc<ProcessService>,
        profiles: Arc<ProfileService>,
        settings: Arc<Settings>,
    ) -> Self {
        Self {
            processes,
            profiles,
            settings,
            saves: None,
            switch: None,
            operations: Arc::new(std::sync::Mutex::new(())),
        }
    }

    pub fn with_saves(
        mut self,
        saves: Arc<crate::services::save::SaveService>,
        switch: Arc<crate::workflows::SwitchProfile>,
        operations: Arc<std::sync::Mutex<()>>,
    ) -> Self {
        self.saves = Some(saves);
        self.switch = Some(switch);
        self.operations = operations;
        self
    }

    pub fn prepare(&self) -> ServiceResult<LaunchPlan> {
        self.processes.refresh_running_games()?;
        build_launch_plan(
            self.profiles.list()?,
            self.profiles.active()?.map(|p| p.id),
            self.settings.data().ask_profile_before_launch,
        )
    }

    pub fn execute(&self, game_id: GameId, profile_id: ProfileId) -> ServiceResult<u32> {
        self.execute_guarded(game_id, profile_id, || true)
    }

    pub fn execute_guarded(
        &self,
        game_id: GameId,
        profile_id: ProfileId,
        proceed: impl FnOnce() -> bool,
    ) -> ServiceResult<u32> {
        self.profiles.get(&profile_id)?;
        if self.profiles.active()?.map(|p| p.id) != Some(profile_id) {
            let switch = self.switch.as_ref().ok_or_else(|| {
                ServiceError::Conflict("Переключение сохранений не настроено".into())
            })?;
            switch.execute(&profile_id)?;
        } else if let Some(switch) = &self.switch {
            switch.refresh_active_profile()?;
        }
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка операций повреждена".into()))?;
        if self.profiles.active()?.map(|p| p.id) != Some(profile_id) {
            return Err(ServiceError::Conflict(
                "Активный профиль изменился; повторите запуск".into(),
            ));
        }
        self.processes.refresh_running_games()?;
        if self.settings.data().local_repo_path.is_some() {
            if let Some(saves) = &self.saves {
                saves.bind(game_id, &profile_id.to_string())?;
            }
        }
        if !proceed() {
            return Err(ServiceError::Conflict("Запуск отменён".into()));
        }
        self.processes.launch(game_id)
    }
}

fn build_launch_plan(
    mut profiles: Vec<Profile>,
    active_profile: Option<ProfileId>,
    ask_before_launch: bool,
) -> ServiceResult<LaunchPlan> {
    profiles.sort_by_key(|p| (p.name.to_lowercase(), p.id));
    if profiles.is_empty() || active_profile.is_none() {
        return Err(ServiceError::InvalidInput(
            "Сначала настройте активный профиль".into(),
        ));
    }
    let needs_profile = ask_before_launch && profiles.len() > 1;
    Ok(LaunchPlan {
        profiles,
        active_profile,
        needs_profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use std::collections::HashMap;

    fn fixture(include_other: bool) -> (LaunchGame, Arc<Settings>, Arc<RuntimeState>, ProfileId) {
        let active = uuid::Uuid::new_v4();
        let other = uuid::Uuid::new_v4();
        let profiles = [active, other]
            .into_iter()
            .filter(|id| include_other || *id == active)
            .map(|id| {
                (
                    id,
                    Profile {
                        id,
                        name: id.to_string(),
                        branch: id.to_string(),
                        last_played_game: None,
                    },
                )
            })
            .collect();
        let catalog = Arc::new(Catalog::new(HashMap::new(), profiles));
        let runtime = Arc::new(RuntimeState::new(
            Some(active),
            vec![],
            SyncStatus {
                state: SyncState::NotConfigured,
                last_sync_at: None,
                error: None,
            },
        ));
        // Old settings must keep working and enable the picker by default.
        let data: SettingsData = serde_json::from_value(serde_json::json!({
            "repo_url": null, "local_repo_path": null, "sync_after_session": true,
            "sync_while_gaming": false, "auto_switch_profile": true, "start_minimized": false
        }))
        .unwrap();
        let settings = Arc::new(Settings::new(
            data,
            LocalConfig {
                machine_id: "test".into(),
                games: HashMap::new(),
            },
        ));
        let processes = Arc::new(ProcessService::new(
            catalog.clone(),
            runtime.clone(),
            settings.clone(),
        ));
        let repo = Arc::new(crate::services::repo::RepoService::new(
            catalog.clone(),
            runtime.clone(),
            settings.clone(),
        ));
        let profiles = Arc::new(ProfileService::new(catalog, runtime.clone(), repo));
        (
            LaunchGame::new(processes, profiles, settings.clone()),
            settings,
            runtime,
            other,
        )
    }
    #[test]
    fn picker_respects_setting_and_profile_count() {
        let (workflow, _, runtime, _) = fixture(true);
        let profiles = workflow.profiles.list().unwrap();
        let active = runtime.active_profile();
        assert!(
            build_launch_plan(profiles.clone(), active, true)
                .unwrap()
                .needs_profile
        );
        assert!(
            !build_launch_plan(profiles.clone(), active, false)
                .unwrap()
                .needs_profile
        );
        let (single, _, runtime, _) = fixture(false);
        let plan = build_launch_plan(
            single.profiles.list().unwrap(),
            runtime.active_profile(),
            true,
        )
        .unwrap();
        assert_eq!(plan.profiles.len(), 1);
        assert!(!plan.needs_profile);
        assert!(build_launch_plan(vec![], active, true).is_err());
        assert!(build_launch_plan(profiles, None, true).is_err());
    }

    #[test]
    fn unavailable_profile_switch_cannot_spawn_or_change_active_profile() {
        let (workflow, _, runtime, other) = fixture(true);
        let active = runtime.active_profile();
        assert!(matches!(
            workflow.execute(uuid::Uuid::new_v4(), other),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(runtime.active_profile(), active);
        assert!(matches!(
            workflow.execute(uuid::Uuid::new_v4(), uuid::Uuid::new_v4()),
            Err(ServiceError::NotFound(_))
        ));
    }
}
