use super::*;
use std::{collections::HashMap, path::PathBuf, sync::RwLock};

pub struct Catalog {
    inner: RwLock<CatalogInner>,
}

struct CatalogInner {
    games: HashMap<GameId, Game>,
    profiles: HashMap<ProfileId, Profile>,
}

impl Catalog {
    /// Replace profile metadata without touching the independently managed games.
    pub fn replace_profiles(&self, profiles: Vec<Profile>) {
        let mut inner = self.inner.write().expect("Catalog lock poisoned");
        inner.profiles = profiles
            .into_iter()
            .map(|profile| (profile.id, profile))
            .collect();
    }
    pub fn new(games: HashMap<GameId, Game>, profiles: HashMap<ProfileId, Profile>) -> Self {
        Self {
            inner: RwLock::new(CatalogInner { games, profiles }),
        }
    }

    pub fn games(&self) -> HashMap<GameId, Game> {
        let inner = self.inner.read().expect("Catalog lock poisoned");
        inner.games.clone()
    }

    pub fn replace_games(&self, games: HashMap<GameId, Game>) {
        self.inner.write().expect("Catalog lock poisoned").games = games;
    }

    /// Добавить игру или заменить существующую по её ID.
    pub fn add_game(&self, game: Game) {
        let mut inner = self.inner.write().expect("Catalog lock poisoned");
        inner.games.insert(game.id, game);
    }

    pub fn update_game(&self, game: Game) -> bool {
        let mut inner = self.inner.write().expect("Catalog lock poisoned");
        if !inner.games.contains_key(&game.id) {
            return false;
        }
        inner.games.insert(game.id, game);
        true
    }

    pub fn remove_game(&self, game_id: GameId) -> bool {
        self.inner
            .write()
            .expect("Catalog lock poisoned")
            .games
            .remove(&game_id)
            .is_some()
    }

    pub fn profiles(&self) -> HashMap<ProfileId, Profile> {
        let inner = self.inner.read().expect("Catalog lock poisoned");
        inner.profiles.clone()
    }

    /// Добавить профиль или заменить существующий по его ID.
    pub fn add_profile(&self, profile: Profile) {
        let mut inner = self.inner.write().expect("Catalog lock poisoned");
        inner.profiles.insert(profile.id.clone(), profile);
    }

    pub fn update_profile(&self, profile: Profile) -> bool {
        let mut inner = self.inner.write().expect("Catalog lock poisoned");
        if !inner.profiles.contains_key(&profile.id) {
            return false;
        }
        inner.profiles.insert(profile.id.clone(), profile);
        true
    }

    pub fn remove_profile(&self, profile_id: &ProfileId) -> bool {
        self.inner
            .write()
            .expect("Catalog lock poisoned")
            .profiles
            .remove(profile_id)
            .is_some()
    }
}

pub struct RuntimeState {
    inner: RwLock<RuntimeStateInner>,
    sync_changes: tokio::sync::watch::Sender<()>,
}

struct RuntimeStateInner {
    active_profile: Option<ProfileId>,
    running_games: Option<Vec<GameId>>,
    conflicts: Vec<Conflict>,
    sync_status: SyncStatus,
}

impl RuntimeStateInner {
    fn set_active_profile(
        &mut self,
        active_profile: Option<ProfileId>,
    ) -> Result<(), ProfileActivationError> {
        let games = self
            .running_games
            .as_ref()
            .ok_or(ProfileActivationError::ProcessesUnknown)?;
        if !games.is_empty() {
            return Err(ProfileActivationError::GamesRunning(games.clone()));
        }
        self.active_profile = active_profile;
        Ok(())
    }

    fn set_running_games(&mut self, running_games: Vec<GameId>) -> Vec<GameProcessEvent> {
        let mut unique = Vec::new();
        for game in running_games {
            if !unique.contains(&game) {
                unique.push(game);
            }
        }
        let previous = self.running_games.as_deref().unwrap_or_default();
        let mut events = Vec::new();
        for game in previous {
            if !unique.contains(game) {
                events.push(GameProcessEvent::Stopped(*game));
            }
        }
        for game in &unique {
            if !previous.contains(game) {
                events.push(GameProcessEvent::Started(*game));
            }
        }
        self.running_games = Some(unique);
        events
    }

    fn add_conflict(&mut self, conflict: Conflict) {
        if let Some(existing) = self
            .conflicts
            .iter_mut()
            .find(|item| item.id == conflict.id)
        {
            *existing = conflict;
        } else {
            self.conflicts.push(conflict);
        }
    }
}

impl RuntimeState {
    pub fn replace_conflicts(&self, conflicts: Vec<Conflict>) {
        self.inner
            .write()
            .expect("RuntimeState lock poisoned")
            .conflicts = conflicts;
    }
    pub fn new(
        active_profile: Option<ProfileId>,
        conflicts: Vec<Conflict>,
        sync_status: SyncStatus,
    ) -> Self {
        Self {
            sync_changes: tokio::sync::watch::channel(()).0,
            inner: RwLock::new(RuntimeStateInner {
                active_profile,
                running_games: None,
                conflicts,
                sync_status,
            }),
        }
    }

    pub fn active_profile(&self) -> Option<ProfileId> {
        let inner = self.inner.read().expect("RuntimeState lock poisoned");
        inner.active_profile.clone()
    }

    pub fn profile_and_running_games(&self) -> (Option<ProfileId>, Option<Vec<GameId>>) {
        let inner = self.inner.read().expect("RuntimeState lock poisoned");
        (inner.active_profile.clone(), inner.running_games.clone())
    }

    pub fn set_active_profile(
        &self,
        active_profile: Option<ProfileId>,
    ) -> Result<(), ProfileActivationError> {
        let mut inner = self.inner.write().expect("RuntimeState lock poisoned");
        inner.set_active_profile(active_profile)
    }

    /// None — процессы ещё не проверены или последний опрос завершился ошибкой.
    pub fn running_games(&self) -> Option<Vec<GameId>> {
        let inner = self.inner.read().expect("RuntimeState lock poisoned");
        inner.running_games.clone()
    }

    pub fn invalidate_process_state(&self) {
        let mut inner = self.inner.write().expect("RuntimeState lock poisoned");
        inner.running_games = None;
    }

    /// Сравнение и замена выполняются под одним локом, чтобы не дублировать события.
    pub fn set_running_games(&self, running_games: Vec<GameId>) -> Vec<GameProcessEvent> {
        let mut inner = self.inner.write().expect("RuntimeState lock poisoned");
        inner.set_running_games(running_games)
    }

    pub fn conflicts(&self) -> Vec<Conflict> {
        let inner = self.inner.read().expect("RuntimeState lock poisoned");
        inner.conflicts.clone()
    }

    /// Добавить конфликт или заменить существующий по его ID.
    pub fn add_conflict(&self, conflict: Conflict) {
        let mut inner = self.inner.write().expect("RuntimeState lock poisoned");
        inner.add_conflict(conflict);
    }

    pub fn sync_status(&self) -> SyncStatus {
        let inner = self.inner.read().expect("RuntimeState lock poisoned");
        inner.sync_status.clone()
    }

    pub fn set_sync_status(&self, sync_status: SyncStatus) {
        let mut inner = self.inner.write().expect("RuntimeState lock poisoned");
        inner.sync_status = sync_status;
        drop(inner);
        self.sync_changes.send_replace(());
    }

    pub fn subscribe_sync_status(&self) -> tokio::sync::watch::Receiver<()> {
        self.sync_changes.subscribe()
    }
}

pub struct Settings {
    inner: RwLock<SettingsInner>,
}

struct SettingsInner {
    data: SettingsData,
    local: LocalConfig,
}

impl SettingsInner {
    fn set_repository(&mut self, url: Option<String>, path: Option<PathBuf>) {
        self.data.repo_url = url;
        self.data.local_repo_path = path;
    }
}

impl Settings {
    pub fn new(data: SettingsData, local: LocalConfig) -> Self {
        Self {
            inner: RwLock::new(SettingsInner { data, local }),
        }
    }

    pub fn data(&self) -> SettingsData {
        let inner = self.inner.read().expect("Settings lock poisoned");
        inner.data.clone()
    }

    pub fn set_repository(&self, url: Option<String>, path: Option<PathBuf>) {
        let mut inner = self.inner.write().expect("Settings lock poisoned");
        inner.set_repository(url, path);
    }

    pub fn replace_data(&self, data: SettingsData) {
        self.inner.write().expect("Settings lock poisoned").data = data;
    }

    pub fn replace_local(&self, local: LocalConfig) {
        self.inner.write().expect("Settings lock poisoned").local = local;
    }

    pub fn set_sync_after_session(&self, enabled: bool) {
        let mut inner = self.inner.write().expect("Settings lock poisoned");
        inner.data.sync_after_session = enabled;
    }

    pub fn set_sync_while_gaming(&self, enabled: bool) {
        let mut inner = self.inner.write().expect("Settings lock poisoned");
        inner.data.sync_while_gaming = enabled;
    }

    pub fn set_auto_switch_profile(&self, enabled: bool) {
        let mut inner = self.inner.write().expect("Settings lock poisoned");
        inner.data.auto_switch_profile = enabled;
    }

    pub fn set_ask_profile_before_launch(&self, enabled: bool) {
        self.inner
            .write()
            .expect("Settings lock poisoned")
            .data
            .ask_profile_before_launch = enabled;
    }

    pub fn set_start_minimized(&self, enabled: bool) {
        let mut inner = self.inner.write().expect("Settings lock poisoned");
        inner.data.start_minimized = enabled;
    }

    pub fn local(&self) -> LocalConfig {
        let inner = self.inner.read().expect("Settings lock poisoned");
        inner.local.clone()
    }

    /// Добавить локальное решение или заменить существующее для этой игры.
    pub fn add_game_resolution(&self, game_id: GameId, resolution: PathResolution) {
        let mut inner = self.inner.write().expect("Settings lock poisoned");
        inner.local.games.insert(game_id, resolution);
    }

    pub fn clear_game_resolution(&self, game_id: &GameId) {
        self.inner
            .write()
            .expect("Settings lock poisoned")
            .local
            .games
            .remove(game_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn settings() -> Settings {
        Settings::new(
            SettingsData {
                catalog_repo_url: None,
                repo_url: None,
                local_repo_path: None,
                sync_after_session: false,
                sync_while_gaming: false,
                auto_switch_profile: false,
                ask_profile_before_launch: true,
                start_minimized: false,
            },
            LocalConfig {
                machine_id: "test".into(),
                games: HashMap::new(),
            },
        )
    }

    fn runtime() -> RuntimeState {
        RuntimeState::new(
            Some(uuid::Uuid::from_u128(1)),
            Vec::new(),
            SyncStatus {
                state: SyncState::Idle,
                last_sync_at: None,
                error: None,
            },
        )
    }

    #[test]
    fn parallel_settings_changes_preserve_other_fields() {
        let settings = Arc::new(settings());
        let barrier = Arc::new(Barrier::new(2));
        thread::scope(|scope| {
            for field in 0..2 {
                let settings = settings.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    if field == 0 {
                        settings.set_sync_after_session(true);
                    } else {
                        settings.set_auto_switch_profile(true);
                    }
                });
            }
        });
        let result = settings.data();
        assert!(result.sync_after_session && result.auto_switch_profile);
        assert!(!result.sync_while_gaming);
    }

    #[test]
    fn repository_fields_are_read_as_one_pair() {
        let settings = settings();
        thread::scope(|scope| {
            scope.spawn(|| {
                for i in 0..1000 {
                    settings
                        .set_repository(Some(i.to_string()), Some(PathBuf::from(i.to_string())));
                }
            });
            for _ in 0..1000 {
                let data = settings.data();
                assert_eq!(data.repo_url.map(PathBuf::from), data.local_repo_path);
            }
        });
    }

    #[test]
    fn process_updates_emit_each_transition_once() {
        let runtime = runtime();
        let next_profile = uuid::Uuid::from_u128(2);
        let game = uuid::Uuid::new_v4();
        thread::scope(|scope| {
            let one = scope.spawn(|| runtime.set_running_games(vec![game, game]));
            let two = scope.spawn(|| runtime.set_running_games(vec![game]));
            let events: Vec<_> = one
                .join()
                .unwrap()
                .into_iter()
                .chain(two.join().unwrap())
                .collect();
            assert_eq!(events.len(), 1);
            assert!(matches!(events[0], GameProcessEvent::Started(id) if id == game));
        });
        assert_eq!(
            runtime.set_active_profile(Some(next_profile)),
            Err(ProfileActivationError::GamesRunning(vec![game]))
        );
        assert_eq!(
            runtime.profile_and_running_games(),
            (Some(uuid::Uuid::from_u128(1)), Some(vec![game]))
        );
        let events = runtime.set_running_games(vec![]);
        assert!(matches!(events.as_slice(), [GameProcessEvent::Stopped(id)] if *id == game));
        runtime.set_active_profile(Some(next_profile)).unwrap();
        assert_eq!(
            runtime.profile_and_running_games(),
            (Some(next_profile), Some(vec![]))
        );
    }
    #[test]
    fn unverified_processes_block_profile_activation() {
        let runtime = runtime();
        let next_profile = uuid::Uuid::from_u128(2);
        assert_eq!(runtime.running_games(), None);
        assert_eq!(
            runtime.set_active_profile(Some(next_profile)),
            Err(ProfileActivationError::ProcessesUnknown)
        );
        runtime.set_running_games(vec![]);
        runtime.set_active_profile(Some(next_profile)).unwrap();
        runtime.invalidate_process_state();
        assert_eq!(
            runtime.set_active_profile(Some(uuid::Uuid::from_u128(1))),
            Err(ProfileActivationError::ProcessesUnknown)
        );
        assert_eq!(runtime.active_profile(), Some(next_profile));
    }
}
