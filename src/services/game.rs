use super::{ServiceError, ServiceResult};
use crate::{
    adapters::{catalog_git, config, git},
    domain::*,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub struct GameService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
    storage: Option<config::DataDirectory>,
    token_for_url: fn(&str) -> ServiceResult<String>,
    operation: Mutex<()>,
    sync_status: Mutex<SyncStatus>,
    revision: AtomicU64,
}

impl GameService {
    /// Read-only view used by process checks; mutations require persistent storage.
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            settings,
            storage: None,
            token_for_url: Self::catalog_token,
            operation: Mutex::new(()),
            revision: AtomicU64::new(0),
            sync_status: Mutex::new(SyncStatus {
                state: SyncState::NotConfigured,
                last_sync_at: None,
                error: None,
            }),
        }
    }

    pub fn persistent(
        catalog: Arc<Catalog>,
        runtime: Arc<RuntimeState>,
        settings: Arc<Settings>,
        data_dir: PathBuf,
    ) -> ServiceResult<Self> {
        let directory = config::DataDirectory::lock(data_dir).map_err(ServiceError::Adapter)?;
        Self::persistent_in(catalog, runtime, settings, directory)
    }

    pub fn persistent_in(
        catalog: Arc<Catalog>,
        runtime: Arc<RuntimeState>,
        settings: Arc<Settings>,
        directory: config::DataDirectory,
    ) -> ServiceResult<Self> {
        let mut service = Self::new(catalog, runtime, settings);
        service.storage = Some(directory);
        service.reload_catalog()?;
        Ok(service)
    }

    fn root(&self) -> ServiceResult<PathBuf> {
        self.storage
            .as_ref()
            .map(|p| p.path().join("catalog"))
            .ok_or_else(|| ServiceError::InvalidInput("Хранилище каталога не подключено".into()))
    }

    pub fn list(&self) -> ServiceResult<Vec<Game>> {
        Ok(self.catalog.games().into_values().collect())
    }

    pub fn get(&self, game_id: GameId) -> ServiceResult<Game> {
        self.catalog
            .games()
            .remove(&game_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Игра не найдена: {game_id}")))
    }

    /// Import a legacy shortcut only if the persistent catalogue has no newer entry.
    /// Keep its UUID so existing desktop shortcuts continue to address the same game.
    pub fn restore_launcher_registration(
        &self,
        game_id: GameId,
        registrations: &std::path::Path,
    ) -> ServiceResult<()> {
        if self.catalog.games().contains_key(&game_id) {
            return Ok(());
        }
        self.mutate(|games| {
            if games.contains_key(&game_id) {
                return Ok(());
            }
            let saved = config::read_games(&registrations.join(format!("{game_id}.json")))
                .map_err(ServiceError::Adapter)?
                .ok_or_else(|| {
                    ServiceError::NotFound(
                        "Регистрация ярлыка не найдена. Создайте ярлык заново в SaveHub".into(),
                    )
                })?;
            let game = saved
                .into_iter()
                .find(|game| game.id == game_id)
                .ok_or_else(|| {
                    ServiceError::InvalidInput("Некорректная регистрация ярлыка".into())
                })?;
            Self::validate_game(&game)?;
            games.insert(game_id, game);
            Ok(())
        })
    }

    pub fn status(&self) -> SyncStatus {
        self.sync_status
            .lock()
            .expect("Catalogue status lock poisoned")
            .clone()
    }

    /// Monotonic notification counter; failures without accepted changes leave it unchanged.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    fn replace_games(&self, games: HashMap<GameId, Game>) {
        if self.catalog.games() != games {
            self.catalog.replace_games(games);
            self.revision.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn replace_local(&self, local: LocalConfig) {
        if self.settings.local() != local {
            self.settings.replace_local(local);
            self.revision.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn mark_sync_pending(&self) {
        let mut status = self
            .sync_status
            .lock()
            .expect("Catalogue status lock poisoned");
        status.state = if self.settings.data().catalog_repo_url.is_some() {
            SyncState::Syncing
        } else {
            SyncState::NotConfigured
        };
        status.error = None;
    }

    fn report(&self, result: &ServiceResult<()>) {
        let mut status = self
            .sync_status
            .lock()
            .expect("Catalogue status lock poisoned");
        match result {
            Ok(()) => {
                status.state = if self.settings.data().catalog_repo_url.is_some() {
                    SyncState::Idle
                } else {
                    SyncState::NotConfigured
                };
                status.error = None;
                if matches!(status.state, SyncState::Idle) {
                    status.last_sync_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    );
                }
            }
            Err(error) => {
                status.state = SyncState::Error;
                status.error = Some(format!(
                    "Не удалось синхронизировать список. Локальные данные не удалены: {error:?}"
                ));
            }
        }
    }

    fn validate_game(game: &Game) -> ServiceResult<()> {
        if game.id.is_nil()
            || game.name.trim().is_empty()
            || game.paths.exe_path.trim().is_empty()
            || game.paths.save_dir_path.trim().is_empty()
        {
            return Err(ServiceError::InvalidInput(
                "Некорректная запись в games.json: UUID, название и пути обязательны".into(),
            ));
        }
        Ok(())
    }

    fn validate_games(games: Vec<Game>) -> ServiceResult<HashMap<GameId, Game>> {
        let mut result = HashMap::new();
        for game in games {
            Self::validate_game(&game)?;
            if result.insert(game.id, game).is_some() {
                return Err(ServiceError::InvalidInput(
                    "Повтор UUID в games.json".into(),
                ));
            }
        }
        Ok(result)
    }

    fn decode(bytes: &[u8]) -> ServiceResult<HashMap<GameId, Game>> {
        let games = serde_json::from_slice(bytes)
            .map_err(|e| ServiceError::InvalidInput(format!("Повреждён games.json: {e}")))?;
        Self::validate_games(games)
    }

    fn read_local(&self) -> ServiceResult<HashMap<GameId, Game>> {
        let root = self.root()?;
        catalog_git::open(&root).map_err(ServiceError::Adapter)?;
        match config::read_games(&root.join(catalog_git::FILE)).map_err(ServiceError::Adapter)? {
            Some(games) => Self::validate_games(games),
            None => {
                if catalog_git::file_at(&root, "HEAD")
                    .map_err(ServiceError::Adapter)?
                    .is_some()
                {
                    return Err(ServiceError::InvalidInput(
                        "Локальный games.json удалён; восстановите его из Git".into(),
                    ));
                }
                Ok(HashMap::new())
            }
        }
    }

    pub fn reload_catalog(&self) -> ServiceResult<()> {
        let _operation = self
            .operation
            .lock()
            .expect("Catalogue operation lock poisoned");
        self.replace_games(self.read_local()?);
        Ok(())
    }

    fn save(&self, games: HashMap<GameId, Game>, local: LocalConfig) -> ServiceResult<()> {
        let root = self.root()?;
        let mut ordered: Vec<_> = games.values().cloned().collect();
        for game in &ordered {
            Self::validate_game(game)?;
        }
        ordered.sort_by_key(|game| game.id);
        let previous_local = self.settings.local();
        let shared_changed = games != self.catalog.games();
        let local_changed = local != previous_local;
        let directory = self
            .storage
            .as_ref()
            .expect("Persistent storage checked by root");
        match config::write_registration(
            &root.join(catalog_git::FILE),
            shared_changed.then_some(ordered.as_slice()),
            &directory.path().join("local_config.json"),
            local_changed.then_some((&previous_local, &local)),
        ) {
            Ok(()) => {}
            Err(config::RegistrationWriteError::Unchanged(error)) => {
                return Err(ServiceError::Adapter(error));
            }
            Err(config::RegistrationWriteError::LocalOnly { write, rollback }) => {
                self.replace_local(local);
                return Err(ServiceError::Conflict(format!(
                    "Сохранены только локальные пути. Общий каталог не изменён и не отправлен. Не удалось восстановить прежние локальные пути: запись {write:?}, восстановление {rollback:?}"
                )));
            }
        }
        self.replace_local(local);
        self.replace_games(games);
        if shared_changed {
            // Both configs are accepted. Failed commit/push is reported separately;
            // retrying the publication must not repeat a successful add.
            let result = self.sync_locked();
            self.report(&result);
        }
        Ok(())
    }

    pub fn sync_catalog(&self) -> ServiceResult<()> {
        let _operation = self
            .operation
            .lock()
            .expect("Catalogue operation lock poisoned");
        self.sync_status
            .lock()
            .expect("Catalogue status lock poisoned")
            .state = SyncState::Syncing;
        let result = self.sync_locked();
        self.report(&result);
        result
    }

    fn sync_locked(&self) -> ServiceResult<()> {
        let root = self.root()?;
        self.replace_games(self.read_local()?);
        catalog_git::checkpoint(&root).map_err(ServiceError::Adapter)?;
        let Some(url) = self.settings.data().catalog_repo_url else {
            return Ok(());
        };
        let token = (self.token_for_url)(&url)?;
        self.sync_remote(&url, &token)
    }

    fn catalog_token(url: &str) -> ServiceResult<String> {
        super::settings::validate_catalog_url(url)?;
        match crate::adapters::credentials::get_token() {
            Ok(token) => Ok(token.unwrap_or_default()),
            Err(crate::adapters::AdapterError::UnsupportedPlatform) => Ok(String::new()),
            Err(error) => Err(ServiceError::Adapter(error)),
        }
    }

    fn sync_remote(&self, url: &str, token: &str) -> ServiceResult<()> {
        let root = self.root()?;
        let provisional = catalog_git::connect(&root, url).map_err(ServiceError::Adapter)?;
        let validation = (|| {
            git::fetch(&root, "origin", token).map_err(ServiceError::Adapter)?;
            let empty = catalog_git::remote_is_empty(&root).map_err(ServiceError::Adapter)?;
            if !empty {
                let bytes = catalog_git::file_at(&root, catalog_git::REMOTE_REF)
                    .map_err(ServiceError::Adapter)?
                    .ok_or_else(|| {
                        ServiceError::InvalidInput("Удалённый каталог не найден".into())
                    })?;
                Self::decode(&bytes)?;
            }
            Ok(empty)
        })();
        let empty = match validation {
            Ok(empty) => empty,
            Err(error) => {
                if provisional {
                    catalog_git::discard_connection(&root).map_err(ServiceError::Adapter)?;
                }
                return Err(error);
            }
        };
        if !empty {
            if matches!(
                git::merge(&root, catalog_git::REMOTE_REF, true).map_err(ServiceError::Adapter)?,
                git::MergeOutcome::Diverged
            ) {
                return Err(ServiceError::Conflict("Каталог изменён на двух ПК. Обе версии сохранены в Git; требуется объединить games.json в ветке main и повторить синхронизацию".into()));
            }
            self.replace_games(self.read_local()?);
        }
        if config::read_games(&root.join(catalog_git::FILE))
            .map_err(ServiceError::Adapter)?
            .is_none()
        {
            config::write_games(&root.join(catalog_git::FILE), &[])
                .map_err(ServiceError::Adapter)?;
            catalog_git::checkpoint(&root).map_err(ServiceError::Adapter)?;
        }
        git::push(&root, "origin", "refs/heads/main:refs/heads/main", token)
            .map_err(ServiceError::Adapter)
    }

    fn edit(
        &self,
        refresh_shared: bool,
        change: impl FnOnce(&mut HashMap<GameId, Game>, &mut LocalConfig) -> ServiceResult<()>,
    ) -> ServiceResult<()> {
        let _operation = self
            .operation
            .lock()
            .expect("Catalogue operation lock poisoned");
        if refresh_shared {
            // Network errors do not prevent durable local editing.
            let result = self.sync_locked();
            self.report(&result);
        }
        let mut games = self.read_local()?;
        self.replace_games(games.clone());
        let mut local = self.settings.local();
        change(&mut games, &mut local)?;
        self.save(games, local)
    }

    fn mutate(
        &self,
        change: impl FnOnce(&mut HashMap<GameId, Game>) -> ServiceResult<()>,
    ) -> ServiceResult<()> {
        self.edit(true, |games, _| change(games))
    }

    pub fn add(
        &self,
        name: &str,
        paths: GamePaths,
        process_names: Vec<String>,
    ) -> ServiceResult<Game> {
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Название игры не может быть пустым".into(),
            ));
        }
        let game = Game {
            id: uuid::Uuid::new_v4(),
            name: name.trim().into(),
            paths,
            process_names,
        };
        self.mutate(|games| {
            games.insert(game.id, game.clone());
            Ok(())
        })?;
        Ok(game)
    }

    pub fn update(
        &self,
        id: GameId,
        name: &str,
        paths: GamePaths,
        process_names: Vec<String>,
    ) -> ServiceResult<Game> {
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Название игры не может быть пустым".into(),
            ));
        }
        let game = Game {
            id,
            name: name.trim().into(),
            paths,
            process_names,
        };
        self.ensure_editable(id)?;
        self.mutate(|games| {
            self.ensure_editable(id)?;
            if !games.contains_key(&id) {
                return Err(ServiceError::NotFound("Игра не найдена".into()));
            }
            games.insert(id, game.clone());
            Ok(())
        })?;
        Ok(game)
    }

    /// Removes only registration, never save data.
    pub fn delete(&self, id: GameId) -> ServiceResult<()> {
        self.ensure_editable(id)?;
        self.mutate(|games| {
            self.ensure_editable(id)?;
            games
                .remove(&id)
                .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))?;
            Ok(())
        })
    }

    pub fn resolve_paths(&self, id: GameId, resolution: PathResolution) -> ServiceResult<()> {
        if let PathResolution::UpdateRepository(paths) = resolution {
            self.update_shared_paths(id, paths)?;
            return Ok(());
        }
        self.set_local_resolution(id, Some(resolution))
    }

    fn set_local_resolution(
        &self,
        id: GameId,
        resolution: Option<PathResolution>,
    ) -> ServiceResult<()> {
        self.edit(false, |games, local| {
            self.ensure_editable(id)?;
            if !games.contains_key(&id) {
                return Err(ServiceError::NotFound("Игра не найдена".into()));
            }
            match resolution {
                Some(value) => {
                    local.games.insert(id, value);
                }
                None => {
                    local.games.remove(&id);
                }
            }
            Ok(())
        })
    }

    pub fn clear_local_resolution(&self, id: GameId) -> ServiceResult<()> {
        self.set_local_resolution(id, None)
    }

    /// Раскрыть переменные окружения и проверить локально оба пути.
    pub fn check_paths(&self, game_id: GameId) -> ServiceResult<GameAvailability> {
        let game = self.get(game_id)?;
        let local = self.settings.local();
        let resolution = local.games.get(&game_id);
        if matches!(resolution, Some(PathResolution::Ignore)) {
            return Ok(GameAvailability::Ignored);
        }
        let paths = match resolution {
            Some(PathResolution::UpdateRepository(paths) | PathResolution::UseLocal(paths)) => {
                paths
            }
            None => &game.paths,
            Some(PathResolution::Ignore) => unreachable!(),
        };
        let save = crate::adapters::fs::expand_path(&paths.save_dir_path)
            .map_err(ServiceError::Adapter)?;
        let exe =
            crate::adapters::fs::expand_path(&paths.exe_path).map_err(ServiceError::Adapter)?;
        let save_missing =
            !crate::adapters::fs::is_directory(&save).map_err(ServiceError::Adapter)?;
        let exe_missing = !crate::adapters::fs::exists(&exe).map_err(ServiceError::Adapter)?;
        if save_missing || exe_missing {
            Ok(GameAvailability::Missing {
                save_dir_missing: save_missing,
                exe_missing,
            })
        } else {
            Ok(GameAvailability::Ready {
                save_dir_path: save,
                exe_path: exe,
            })
        }
    }

    pub fn add_with_paths(&self, name: &str, paths: GamePaths) -> ServiceResult<Game> {
        Self::validate_registration_paths(&paths)?;
        let processes = Self::process_names(&paths);
        self.add(name, paths, processes)
    }

    pub fn update_with_paths(
        &self,
        id: GameId,
        name: &str,
        paths: GamePaths,
    ) -> ServiceResult<Game> {
        self.ensure_editable(id)?;
        Self::validate_registration_paths(&paths)?;
        self.edit(true, |games, local| {
            self.ensure_editable(id)?;
            let game = games
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))?;
            game.name = name.trim().into();
            match local.games.get(&id) {
                Some(PathResolution::UseLocal(_) | PathResolution::Ignore) => {
                    local.games.insert(id, PathResolution::UseLocal(paths));
                }
                _ => {
                    game.process_names = Self::process_names(&paths);
                    game.paths = paths;
                    local.games.remove(&id);
                }
            }
            Ok(())
        })?;
        self.get(id)
    }

    pub fn update_shared_paths(&self, id: GameId, paths: GamePaths) -> ServiceResult<Game> {
        self.ensure_editable(id)?;
        Self::validate_registration_paths(&paths)?;
        self.edit(true, |games, local| {
            self.ensure_editable(id)?;
            let game = games
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))?;
            game.process_names = Self::process_names(&paths);
            game.paths = paths;
            local.games.remove(&id);
            Ok(())
        })?;
        self.get(id)
    }

    pub fn use_local_paths(&self, id: GameId, paths: GamePaths) -> ServiceResult<Game> {
        self.ensure_editable(id)?;
        Self::validate_registration_paths(&paths)?;
        self.resolve_paths(id, PathResolution::UseLocal(paths))?;
        self.get(id)
    }

    pub fn ignore_on_this_pc(&self, id: GameId) -> ServiceResult<Game> {
        self.ensure_editable(id)?;
        self.resolve_paths(id, PathResolution::Ignore)?;
        self.get(id)
    }

    fn ensure_editable(&self, id: GameId) -> ServiceResult<()> {
        let games = self
            .runtime
            .running_games()
            .ok_or(ServiceError::ProcessesUnknown)?;
        if games.contains(&id) {
            return Err(ServiceError::GameRunning(vec![id]));
        }
        Ok(())
    }

    fn validate_registration_paths(paths: &GamePaths) -> ServiceResult<()> {
        let (exe, saves) = crate::adapters::game_paths::validate(paths);
        if let Some(error) = exe {
            return Err(ServiceError::InvalidInput(format!("Файл игры: {error}")));
        }
        if let Some(error) = saves {
            return Err(ServiceError::InvalidInput(format!("Сохранения: {error}")));
        }
        Ok(())
    }

    fn process_names(paths: &GamePaths) -> Vec<String> {
        std::path::Path::new(&paths.exe_path)
            .file_name()
            .map(|n| vec![n.to_string_lossy().into_owned()])
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("savehub-catalog-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn service(&self, name: &str) -> GameService {
            let data = self.0.join(name);
            let local = config::read_local_config(&data.join("local_config.json"))
                .unwrap()
                .unwrap_or(LocalConfig {
                    machine_id: name.into(),
                    games: HashMap::new(),
                });
            let service = GameService::persistent(
                Arc::new(Catalog::new(HashMap::new(), HashMap::new())),
                Arc::new(RuntimeState::new(
                    None,
                    vec![],
                    SyncStatus {
                        state: SyncState::NotConfigured,
                        last_sync_at: None,
                        error: None,
                    },
                )),
                Arc::new(Settings::new(
                    SettingsData {
                        ask_profile_before_launch: true,
                        catalog_repo_url: None,
                        repo_url: None,
                        local_repo_path: None,
                        sync_after_session: false,
                        sync_while_gaming: false,
                        auto_switch_profile: false,
                        start_minimized: false,
                    },
                    local,
                )),
                data,
            )
            .unwrap();
            service.runtime.set_running_games(vec![]);
            service
        }
        fn remote(&self) -> PathBuf {
            let path = self.0.join("remote.git");
            let repo = git2::Repository::init_bare(&path).unwrap();
            repo.set_head("refs/heads/main").unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn add(service: &GameService, name: &str) -> Game {
        service
            .add(
                name,
                GamePaths {
                    exe_path: "%USERPROFILE%/game.exe".into(),
                    save_dir_path: "%APPDATA%/saves".into(),
                },
                vec!["game.exe".into()],
            )
            .unwrap()
    }

    #[test]
    fn persists_games_local_overrides_and_stable_ids_across_restart() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let game = add(&service, "First");
        service
            .resolve_paths(game.id, PathResolution::Ignore)
            .unwrap();
        drop(service);
        let restarted = fixture.service("pc");
        assert_eq!(restarted.get(game.id).unwrap().name, "First");
        assert!(matches!(
            restarted.settings.local().games.get(&game.id),
            Some(PathResolution::Ignore)
        ));
        assert!(git::status(&restarted.root().unwrap()).unwrap().is_empty());
        assert!(matches!(
            restarted.check_paths(game.id).unwrap(),
            GameAvailability::Ignored
        ));
        restarted.delete(game.id).unwrap();
        drop(restarted);
        assert!(fixture.service("pc").list().unwrap().is_empty());
    }

    #[test]
    fn independent_repositories_exchange_one_config_without_local_overrides() {
        let fixture = Fixture::new();
        let remote = fixture.remote();
        let url = remote.to_str().unwrap();
        let one = fixture.service("one");
        let first = add(&one, "First");
        one.resolve_paths(first.id, PathResolution::Ignore).unwrap();
        one.sync_remote(url, "").unwrap();
        let two = fixture.service("two");
        two.sync_remote(url, "").unwrap();
        assert_eq!(
            two.get(first.id).unwrap().paths.exe_path,
            "%USERPROFILE%/game.exe"
        );
        assert!(two.settings.local().games.is_empty());
        let second = add(&two, "Second");
        two.sync_remote(url, "").unwrap();
        one.sync_remote(url, "").unwrap();
        assert!(one.get(second.id).is_ok());
        assert_eq!(one.list().unwrap().len(), 2);
        let repo = git2::Repository::open(one.root().unwrap()).unwrap();
        assert!(!repo.is_worktree());
        assert_eq!(
            git::list_branches(&one.root().unwrap()).unwrap(),
            vec!["main"]
        );
        assert_eq!(repo.head().unwrap().peel_to_tree().unwrap().len(), 1);
    }

    #[test]
    fn retry_after_network_failure_keeps_commit_and_does_not_duplicate_games() {
        let fixture = Fixture::new();
        let remote = fixture.remote();
        let one = fixture.service("one");
        let game = add(&one, "Offline");
        let head = git::head(&one.root().unwrap()).unwrap();
        let moved = fixture.0.join("offline.git");
        std::fs::rename(&remote, &moved).unwrap();
        assert!(one.sync_remote(remote.to_str().unwrap(), "").is_err());
        assert_eq!(git::head(&one.root().unwrap()).unwrap(), head);
        assert!(one.get(game.id).is_ok());
        std::fs::rename(&moved, &remote).unwrap();
        one.sync_remote(remote.to_str().unwrap(), "").unwrap();
        one.sync_remote(remote.to_str().unwrap(), "").unwrap();
        assert_eq!(git::head(&one.root().unwrap()).unwrap(), head);
        assert_eq!(one.list().unwrap().len(), 1);
    }

    #[test]
    fn divergent_catalogues_preserve_both_histories_and_working_copy() {
        let fixture = Fixture::new();
        let remote = fixture.remote();
        let url = remote.to_str().unwrap();
        let one = fixture.service("one");
        add(&one, "Base");
        one.sync_remote(url, "").unwrap();
        let two = fixture.service("two");
        two.sync_remote(url, "").unwrap();
        add(&one, "From one");
        one.sync_remote(url, "").unwrap();
        let local = add(&two, "From two");
        let before = git::head(&two.root().unwrap()).unwrap();
        assert!(matches!(
            two.sync_remote(url, ""),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(git::head(&two.root().unwrap()).unwrap(), before);
        assert!(two.get(local.id).is_ok());
        assert!(git::status(&two.root().unwrap()).unwrap().is_empty());
        assert_eq!(
            git2::Repository::open(&remote)
                .unwrap()
                .head()
                .unwrap()
                .target()
                .unwrap()
                .to_string(),
            git::head(&one.root().unwrap()).unwrap()
        );
    }

    #[test]
    fn malformed_remote_is_rejected_before_checkout_and_duplicate_ids_are_rejected() {
        let fixture = Fixture::new();
        let remote = fixture.remote();
        let url = remote.to_str().unwrap();
        let one = fixture.service("one");
        let game = add(&one, "Good");
        one.sync_remote(url, "").unwrap();
        let two = fixture.service("two");
        two.sync_remote(url, "").unwrap();
        let before = git::head(&two.root().unwrap()).unwrap();
        let root = one.root().unwrap();
        std::fs::write(root.join(catalog_git::FILE), b"broken").unwrap();
        catalog_git::checkpoint(&root).unwrap();
        git::push(&root, "origin", "refs/heads/main:refs/heads/main", "").unwrap();
        assert!(two.sync_remote(url, "").is_err());
        assert_eq!(git::head(&two.root().unwrap()).unwrap(), before);
        assert_eq!(two.get(game.id).unwrap().name, "Good");
        let duplicate = serde_json::to_vec(&vec![game.clone(), game]).unwrap();
        assert!(GameService::decode(&duplicate).is_err());
    }

    #[test]
    fn local_path_edit_preserves_shared_paths_and_shared_edit_clears_override() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let game = add(&service, "Game");
        let exe = fixture.0.join("local.exe");
        std::fs::write(&exe, b"fixture").unwrap();
        let paths = GamePaths {
            exe_path: exe.to_string_lossy().into_owned(),
            save_dir_path: fixture.0.to_string_lossy().into_owned(),
        };
        service.use_local_paths(game.id, paths.clone()).unwrap();
        service
            .update_with_paths(game.id, "Renamed", paths.clone())
            .unwrap();
        drop(service);
        let restarted = fixture.service("pc");
        assert_eq!(
            restarted.get(game.id).unwrap().paths.exe_path,
            game.paths.exe_path
        );
        assert_eq!(restarted.get(game.id).unwrap().name, "Renamed");
        assert!(matches!(
            restarted.settings.local().games.get(&game.id),
            Some(PathResolution::UseLocal(_))
        ));
        restarted
            .update_shared_paths(game.id, paths.clone())
            .unwrap();
        drop(restarted);
        let restarted = fixture.service("pc");
        assert_eq!(
            restarted.get(game.id).unwrap().paths.exe_path,
            paths.exe_path
        );
        assert!(!restarted.settings.local().games.contains_key(&game.id));
        assert_eq!(std::fs::read(exe).unwrap(), b"fixture");
    }

    #[test]
    fn broken_local_config_and_foreign_remote_are_not_overwritten() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let game = add(&service, "Safe");
        let root = service.root().unwrap();
        std::fs::write(root.join(catalog_git::FILE), b"broken").unwrap();
        assert!(service.delete(game.id).is_err());
        assert_eq!(
            std::fs::read(root.join(catalog_git::FILE)).unwrap(),
            b"broken"
        );
        assert!(service.get(game.id).is_ok());
        assert!(matches!(service.status().state, SyncState::Error));
        catalog_git::connect(&root, "https://github.com/owner/first").unwrap();
        assert!(catalog_git::connect(&root, "https://github.com/owner/second").is_err());
        assert_eq!(
            git2::Repository::open(&root)
                .unwrap()
                .find_remote("origin")
                .unwrap()
                .url()
                .unwrap(),
            "https://github.com/owner/first"
        );
    }

    fn valid_local_paths(fixture: &Fixture) -> GamePaths {
        let exe = fixture.0.join("game.exe");
        std::fs::write(&exe, b"fixture").unwrap();
        GamePaths {
            exe_path: exe.to_string_lossy().into_owned(),
            save_dir_path: fixture.0.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn registration_restarts_and_rejects_second_owner() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let paths = valid_local_paths(&fixture);
        let game = service.add_with_paths("Registered", paths.clone()).unwrap();
        assert!(
            GameService::persistent(
                service.catalog.clone(),
                service.runtime.clone(),
                service.settings.clone(),
                fixture.0.join("pc")
            )
            .is_err()
        );
        drop(service);
        assert_eq!(fixture.service("pc").get(game.id).unwrap().paths, paths);
    }

    #[test]
    fn failed_local_write_does_not_change_or_commit_shared_paths() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let game = add(&service, "Original");
        service.ignore_on_this_pc(game.id).unwrap();
        let local_path = fixture.0.join("pc/local_config.json");
        std::fs::remove_file(&local_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let before = git::head(&service.root().unwrap()).unwrap();
        assert!(
            service
                .update_shared_paths(game.id, valid_local_paths(&fixture))
                .is_err()
        );
        assert!(
            service
                .update_with_paths(game.id, "Changed", valid_local_paths(&fixture))
                .is_err()
        );
        assert_eq!(service.get(game.id).unwrap(), game);
        assert_eq!(git::head(&service.root().unwrap()).unwrap(), before);
        assert!(matches!(
            service.settings.local().games.get(&game.id),
            Some(PathResolution::Ignore)
        ));
        assert!(git::status(&service.root().unwrap()).unwrap().is_empty());
    }

    #[test]
    fn invalid_public_add_and_update_cannot_poison_catalogue() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let game = add(&service, "Valid");
        let head = git::head(&service.root().unwrap()).unwrap();
        let empty = GamePaths {
            exe_path: " ".into(),
            save_dir_path: String::new(),
        };
        assert!(service.add("Invalid", empty.clone(), vec![]).is_err());
        assert!(service.update(game.id, "Invalid", empty, vec![]).is_err());
        assert_eq!(git::head(&service.root().unwrap()).unwrap(), head);
        drop(service);
        assert_eq!(fixture.service("pc").list().unwrap(), vec![game]);
    }

    fn set_test_remote(service: &mut GameService, path: &std::path::Path) {
        // Only authorization is replaced: exercise the actual fetch, config,
        // checkpoint, retry and status code through the public entry point.
        service.token_for_url = |url| {
            assert!(std::path::Path::new(url).is_absolute());
            Ok(String::new())
        };
        let mut settings = service.settings.data();
        settings.catalog_repo_url = Some(path.to_string_lossy().into_owned());
        service.settings.replace_data(settings);
    }

    #[test]
    fn public_sync_recovers_from_wrong_first_address_and_retries_without_duplicates() {
        let fixture = Fixture::new();
        let mut service = fixture.service("pc");
        set_test_remote(&mut service, &fixture.0.join("missing.git"));
        let game = service
            .add_with_paths("Offline", valid_local_paths(&fixture))
            .unwrap();
        assert!(matches!(service.status().state, SyncState::Error));
        let root = service.root().unwrap();
        let head = git::head(&root).unwrap();
        assert!(
            git2::Repository::open(&root)
                .unwrap()
                .find_remote("origin")
                .is_err()
        );
        let remote = fixture.remote();
        set_test_remote(&mut service, &remote);
        service.sync_catalog().unwrap();
        service.sync_catalog().unwrap();
        assert!(matches!(service.status().state, SyncState::Idle));
        assert!(service.status().last_sync_at.is_some());
        assert_eq!(git::head(&root).unwrap(), head);
        assert_eq!(service.list().unwrap(), vec![game]);
        let remote_repo = git2::Repository::open(&remote).unwrap();
        assert_eq!(
            remote_repo.head().unwrap().target().unwrap().to_string(),
            head
        );
    }

    #[test]
    fn unknown_or_running_processes_block_edits_but_not_initial_registration() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        service.runtime.invalidate_process_state();
        let paths = valid_local_paths(&fixture);
        let game = service.add_with_paths("Registered", paths.clone()).unwrap();
        let revision = service.revision();
        let head = git::head(&service.root().unwrap()).unwrap();
        let attempts = || {
            vec![
                service
                    .update_with_paths(game.id, "Changed", paths.clone())
                    .map(|_| ()),
                service
                    .update_shared_paths(game.id, paths.clone())
                    .map(|_| ()),
                service.use_local_paths(game.id, paths.clone()).map(|_| ()),
                service.ignore_on_this_pc(game.id).map(|_| ()),
                service
                    .update(game.id, "Changed", paths.clone(), vec![])
                    .map(|_| ()),
                service.clear_local_resolution(game.id),
                service.delete(game.id),
            ]
        };
        assert!(
            attempts()
                .iter()
                .all(|r| matches!(r, Err(ServiceError::ProcessesUnknown)))
        );
        service.runtime.set_running_games(vec![game.id]);
        assert!(
            attempts()
                .iter()
                .all(|r| matches!(r, Err(ServiceError::GameRunning(ids)) if ids == &vec![game.id]))
        );
        assert_eq!(service.revision(), revision);
        assert_eq!(git::head(&service.root().unwrap()).unwrap(), head);
        assert_eq!(service.get(game.id).unwrap(), game);
        service.runtime.set_running_games(vec![]);
        service
            .update_with_paths(game.id, "Allowed", paths)
            .unwrap();
        assert_eq!(service.get(game.id).unwrap().name, "Allowed");
        assert!(service.revision() > revision);
    }

    #[test]
    fn failed_sync_without_data_changes_keeps_notification_revision() {
        let fixture = Fixture::new();
        let mut service = fixture.service("pc");
        set_test_remote(&mut service, &fixture.0.join("missing.git"));
        let revision = service.revision();
        assert!(service.sync_catalog().is_err());
        assert_eq!(service.revision(), revision);
        assert!(matches!(service.status().state, SyncState::Error));
    }

    #[test]
    fn accepted_remote_data_is_notified_even_when_push_fails() {
        let fixture = Fixture::new();
        let source = fixture.service("source");
        let game = add(&source, "Remote");
        let mut receiver = fixture.service("receiver");
        // libgit2 fetches from a local working repo but refuses to push to it.
        set_test_remote(&mut receiver, &source.root().unwrap());
        let revision = receiver.revision();
        assert!(receiver.sync_catalog().is_err());
        assert_eq!(receiver.get(game.id).unwrap(), game);
        assert!(receiver.revision() > revision);
        assert!(matches!(receiver.status().state, SyncState::Error));
    }

    #[test]
    fn legacy_shortcut_import_survives_reload_without_overwriting_newer_catalogue() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        let registrations = fixture.0.join("launchers");
        let game = Game {
            id: uuid::Uuid::new_v4(),
            name: "Legacy shortcut".into(),
            paths: valid_local_paths(&fixture),
            process_names: vec!["game.exe".into()],
        };
        config::write_games(
            &registrations.join(format!("{}.json", game.id)),
            std::slice::from_ref(&game),
        )
        .unwrap();
        service
            .restore_launcher_registration(game.id, &registrations)
            .unwrap();
        service.reload_catalog().unwrap();
        assert_eq!(service.get(game.id).unwrap(), game);
        service
            .update_with_paths(game.id, "Newer catalogue", game.paths.clone())
            .unwrap();
        drop(service);
        let restarted = fixture.service("pc");
        restarted
            .restore_launcher_registration(game.id, &registrations)
            .unwrap();
        assert_eq!(restarted.get(game.id).unwrap().name, "Newer catalogue");
        assert_eq!(restarted.list().unwrap().len(), 1);
        assert!(matches!(
            restarted.restore_launcher_registration(uuid::Uuid::new_v4(), &registrations),
            Err(ServiceError::NotFound(_))
        ));
    }

    #[test]
    fn parallel_adds_are_serialized_without_losing_games() {
        let fixture = Fixture::new();
        let service = fixture.service("pc");
        std::thread::scope(|scope| {
            for i in 0..8 {
                let service = &service;
                scope.spawn(move || add(service, &format!("Game {i}")));
            }
        });
        drop(service);
        assert_eq!(fixture.service("pc").list().unwrap().len(), 8);
    }
}
