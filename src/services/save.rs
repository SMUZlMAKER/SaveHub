use super::{ServiceError, ServiceResult};
use crate::{
    adapters::{config, fs, git},
    domain::*,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

/// Workflows hold the shared repository operations mutex around complete scenarios.
/// The private lock also serializes direct calls and publication of snapshot metadata.
pub struct SaveService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
    operation: Mutex<()>,
}

impl SaveService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            settings,
            operation: Mutex::new(()),
        }
    }

    fn lock(&self) -> ServiceResult<std::sync::MutexGuard<'_, ()>> {
        self.operation
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка сохранений повреждена".into()))
    }

    fn stopped(&self, game: GameId) -> ServiceResult<()> {
        let running = self
            .runtime
            .running_games()
            .ok_or(ServiceError::ProcessesUnknown)?;
        if running.contains(&game) {
            return Err(ServiceError::GameRunning(vec![game]));
        }
        Ok(())
    }

    fn profile(&self, id: &str) -> ServiceResult<Profile> {
        let id: ProfileId = id
            .parse()
            .map_err(|_| ServiceError::InvalidInput("Неверный UUID профиля".into()))?;
        self.catalog
            .profiles()
            .remove(&id)
            .filter(|p| !p.branch.starts_with("savehub/"))
            .ok_or_else(|| ServiceError::NotFound("Профиль не найден".into()))
    }

    fn root(&self) -> ServiceResult<PathBuf> {
        self.settings
            .data()
            .local_repo_path
            .filter(|p| p.is_absolute())
            .ok_or_else(|| {
                ServiceError::InvalidInput("Настройте локальный репозиторий сохранений".into())
            })
    }

    fn history(&self) -> ServiceResult<PathBuf> {
        let metadata = git::metadata_dir(&self.root()?).map_err(ServiceError::Adapter)?;
        let history = metadata.join("savehub/snapshots");
        fs::require_plain_path(&metadata, &history.join("index.json"))
            .map_err(ServiceError::Adapter)?;
        Ok(history)
    }

    fn target(&self, game: GameId, profile: &Profile) -> ServiceResult<PathBuf> {
        let tree = git::prepare_worktree(&self.root()?, &profile.branch, &profile.id.to_string())
            .map_err(ServiceError::Adapter)?;
        if !git::conflict_paths(&tree)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return Err(ServiceError::Conflict(
                "Сначала разрешите конфликты профиля".into(),
            ));
        }
        let path = tree.join("games").join(game.to_string());
        fs::require_plain_path(&tree, &path).map_err(ServiceError::Adapter)?;
        Ok(path)
    }

    fn source(&self, game: GameId) -> ServiceResult<PathBuf> {
        let game = self
            .catalog
            .games()
            .remove(&game)
            .ok_or_else(|| ServiceError::NotFound("Игра не найдена".into()))?;
        let local = self.settings.local();
        let paths = match local.games.get(&game.id) {
            Some(PathResolution::Ignore) => {
                return Err(ServiceError::InvalidInput(
                    "Игра игнорируется на этом ПК".into(),
                ));
            }
            Some(PathResolution::UseLocal(paths) | PathResolution::UpdateRepository(paths)) => {
                paths
            }
            None => &game.paths,
        };
        let path = fs::expand_path(&paths.save_dir_path).map_err(ServiceError::Adapter)?;
        if !path.is_absolute() || path.parent().is_none() {
            return Err(ServiceError::InvalidInput(
                "Нужен абсолютный путь папки сохранений".into(),
            ));
        }
        // A game path inside the repository could alias another profile or Git metadata.
        let root = fs::canonicalize(&self.root()?).map_err(ServiceError::Adapter)?;
        let parent = fs::canonicalize(path.parent().unwrap()).map_err(ServiceError::Adapter)?;
        if parent.starts_with(&root)
            || root.starts_with(
                parent.join(
                    path.file_name()
                        .ok_or_else(|| ServiceError::InvalidInput("Неверный путь".into()))?,
                ),
            )
        {
            return Err(ServiceError::InvalidInput(
                "Папка игры пересекается с репозиторием".into(),
            ));
        }
        Ok(path)
    }

    fn validate_link(&self, game: GameId, source: &std::path::Path) -> ServiceResult<()> {
        let Some(actual) = fs::link_destination(source).map_err(ServiceError::Adapter)? else {
            return Ok(());
        };
        for profile in self.catalog.profiles().values() {
            let Some(tree) =
                git::existing_worktree(&self.root()?, &profile.branch, &profile.id.to_string())
                    .map_err(ServiceError::Adapter)?
            else {
                continue;
            };
            let expected = tree.join("games").join(game.to_string());
            fs::require_plain_path(&tree, &expected).map_err(ServiceError::Adapter)?;
            if fs::canonicalize_missing(&expected).map_err(ServiceError::Adapter)? == actual {
                return Ok(());
            }
        }
        Err(ServiceError::Conflict(
            "Папка сохранений — посторонняя ссылка; привяжите обычный каталог".into(),
        ))
    }

    pub fn games_count(&self, profile_id: &ProfileId) -> ServiceResult<u32> {
        let _guard = self.lock()?;
        if self.settings.data().local_repo_path.is_none() {
            return Ok(0);
        }
        let profile = self.profile(&profile_id.to_string())?;
        let Some(tree) =
            git::existing_worktree(&self.root()?, &profile.branch, &profile.id.to_string())
                .map_err(ServiceError::Adapter)?
        else {
            return Ok(0);
        };
        let mut count = 0;
        for game in self.catalog.games().keys() {
            let target = tree.join("games").join(game.to_string());
            fs::require_plain_path(&tree, &target).map_err(ServiceError::Adapter)?;
            if fs::is_directory(&target).map_err(ServiceError::Adapter)? {
                count += 1;
            }
        }
        Ok(count)
    }

    /// A delayed stop belongs to the original managed tree, even if game metadata changed.
    pub fn capture_session(
        &self,
        game: GameId,
        profile_id: ProfileId,
    ) -> ServiceResult<Option<Save>> {
        let _guard = self.lock()?;
        self.stopped(game)?;
        let profile = self.profile(&profile_id.to_string())?;
        let Some(tree) =
            git::existing_worktree(&self.root()?, &profile.branch, &profile.id.to_string())
                .map_err(ServiceError::Adapter)?
        else {
            return Ok(None);
        };
        if !git::conflict_paths(&tree)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return Err(ServiceError::Conflict(
                "Сначала разрешите конфликты завершённой сессии".into(),
            ));
        }
        let target = tree.join("games").join(game.to_string());
        fs::require_plain_path(&tree, &target).map_err(ServiceError::Adapter)?;
        if !fs::is_directory(&target).map_err(ServiceError::Adapter)? {
            return Ok(None);
        }
        self.snapshot_tree(game, &profile, SaveKind::AfterSession, &target)
            .map(Some)
    }

    /// Never change bindings or live files while the game is running.
    pub fn capture_manual(&self, game: GameId) -> ServiceResult<Save> {
        let _guard = self.lock()?;
        let running = self
            .runtime
            .running_games()
            .ok_or(ServiceError::ProcessesUnknown)?;
        if !running.contains(&game) {
            return Err(ServiceError::InvalidInput(
                "Игра уже завершена; дождитесь снимка после сессии".into(),
            ));
        }
        let profile_id = self
            .runtime
            .active_profile()
            .ok_or_else(|| ServiceError::InvalidInput("Нет активного профиля".into()))?;
        let profile = self.profile(&profile_id.to_string())?;
        let source = self.source(game)?;
        let target = self.target(game, &profile)?;
        if fs::link_destination(&source).map_err(ServiceError::Adapter)?
            != Some(fs::canonicalize(&target).map_err(ServiceError::Adapter)?)
        {
            return Err(ServiceError::Conflict("Сохранения игры не привязаны к текущему профилю. Закройте игру и запустите её через SaveHub".into()));
        }
        self.snapshot_tree(game, &profile, SaveKind::Manual, &target)
    }

    pub fn is_bound(&self, game: GameId, profile: &ProfileId) -> ServiceResult<bool> {
        let _guard = self.lock()?;
        let source = self.source(game)?;
        let Some(actual) = fs::link_destination(&source).map_err(ServiceError::Adapter)? else {
            return Ok(false);
        };
        let profile = self.profile(&profile.to_string())?;
        let Some(tree) =
            git::existing_worktree(&self.root()?, &profile.branch, &profile.id.to_string())
                .map_err(ServiceError::Adapter)?
        else {
            return Ok(false);
        };
        let target = tree.join("games").join(game.to_string());
        fs::require_plain_path(&tree, &target).map_err(ServiceError::Adapter)?;
        Ok(actual == fs::canonicalize_missing(&target).map_err(ServiceError::Adapter)?)
    }

    /// Import only into the current profile; new profiles start empty.
    pub fn initialize(&self, game_id: GameId, profile_id: &str) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.initialize_inner(game_id, &self.profile(profile_id)?)
    }

    fn initialize_inner(&self, game: GameId, profile: &Profile) -> ServiceResult<()> {
        self.stopped(game)?;
        let source = self.source(game)?;
        self.validate_link(game, &source)?;
        let target = self.target(game, profile)?;
        if fs::exists(&target).map_err(ServiceError::Adapter)? {
            if !fs::is_directory(&target).map_err(ServiceError::Adapter)? {
                return Err(ServiceError::Conflict(
                    "Хранилище игры не является каталогом".into(),
                ));
            }
            return Ok(());
        }
        let link = fs::link_destination(&source).map_err(ServiceError::Adapter)?;
        if self.runtime.active_profile() == Some(profile.id) {
            if let Some(link) = &link {
                if *link != fs::canonicalize_missing(&target).map_err(ServiceError::Adapter)? {
                    return Err(ServiceError::Conflict(
                        "Ссылка игры указывает на другой профиль; импорт остановлен".into(),
                    ));
                }
            }
        }
        fs::create_dir_all(target.parent().unwrap()).map_err(ServiceError::Adapter)?;
        if self.runtime.active_profile() == Some(profile.id)
            && link.is_none()
            && fs::exists(&source).map_err(ServiceError::Adapter)?
        {
            let staged = target.with_file_name(format!(".savehub-import-{}", uuid::Uuid::new_v4()));
            fs::copy_tree(&source, &staged).map_err(ServiceError::Adapter)?;
            fs::rename(&staged, &target).map_err(ServiceError::Adapter)
        } else {
            fs::create_dir_all(&target).map_err(ServiceError::Adapter)
        }
    }

    /// Preserve ordinary source directories beside the original path before linking.
    pub fn bind(&self, game_id: GameId, profile_id: &str) -> ServiceResult<()> {
        let _guard = self.lock()?;
        let profile = self.profile(profile_id)?;
        self.initialize_inner(game_id, &profile)?;
        let source = self.source(game_id)?;
        self.validate_link(game_id, &source)?;
        let target = self.target(game_id, &profile)?;
        let previous_link = fs::read_symlink(&source).map_err(ServiceError::Adapter)?;
        if previous_link.is_some()
            && fs::canonicalize(&source).map_err(ServiceError::Adapter)?
                == fs::canonicalize(&target).map_err(ServiceError::Adapter)?
        {
            return Ok(());
        }
        self.stopped(game_id)?;
        if previous_link.is_none()
            && fs::exists(&source).map_err(ServiceError::Adapter)?
            && self.runtime.active_profile() == Some(profile.id)
            && !fs::trees_equal(&source, &target).map_err(ServiceError::Adapter)?
        {
            return Err(ServiceError::Conflict("Локальные сохранения отличаются от хранилища профиля. Сохраните обе папки и выберите данные перед привязкой".into()));
        }
        let backup = source.with_file_name(format!(".savehub-original-{}", uuid::Uuid::new_v4()));
        let existed = fs::exists(&source).map_err(ServiceError::Adapter)?;
        if existed {
            fs::rename(&source, &backup).map_err(ServiceError::Adapter)?;
        }
        if let Err(error) = fs::create_symlink(&source, &target) {
            if existed {
                fs::rename(&backup, &source).map_err(|rollback| ServiceError::Conflict(format!("Не удалось подключить сохранения: {error:?}; восстановить путь: {rollback:?}. Исходные данные: {}", backup.display())))?;
            }
            return Err(ServiceError::Adapter(error));
        }
        if previous_link.is_some() {
            // Binding is already committed. A redundant old link may remain if cleanup
            // fails; reporting a failed bind here would prevent workflow rollback of it.
            let _ = fs::remove_symlink(&backup);
        }
        Ok(())
    }

    /// Detach into a real copy; the managed target and snapshots remain intact.
    pub fn unbind(&self, game_id: GameId) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.stopped(game_id)?;
        let source = self.source(game_id)?;
        self.validate_link(game_id, &source)?;
        if fs::read_symlink(&source)
            .map_err(ServiceError::Adapter)?
            .is_none()
        {
            return Ok(());
        }
        let staged = source.with_file_name(format!(".savehub-detach-{}", uuid::Uuid::new_v4()));
        if fs::exists(&source).map_err(ServiceError::Adapter)? {
            fs::copy_tree(&source, &staged).map_err(ServiceError::Adapter)?;
        } else {
            fs::create_dir_all(&staged).map_err(ServiceError::Adapter)?;
        }
        self.stopped(game_id)?;
        fs::replace_directory(&source, &staged).map_err(ServiceError::Adapter)?;
        Ok(())
    }

    pub fn create_save(
        &self,
        game_id: GameId,
        profile_id: &str,
        kind: SaveKind,
    ) -> ServiceResult<Save> {
        let _guard = self.lock()?;
        self.create_inner(game_id, &self.profile(profile_id)?, kind)
    }

    fn create_inner(&self, game: GameId, profile: &Profile, kind: SaveKind) -> ServiceResult<Save> {
        self.stopped(game)?;
        self.initialize_inner(game, profile)?;
        let target = self.target(game, profile)?;
        let source = self.source(game)?;
        self.validate_link(game, &source)?;
        if self.runtime.active_profile() == Some(profile.id)
            && fs::read_symlink(&source)
                .map_err(ServiceError::Adapter)?
                .is_some()
            && fs::canonicalize(&source).map_err(ServiceError::Adapter)?
                != fs::canonicalize(&target).map_err(ServiceError::Adapter)?
        {
            return Err(ServiceError::Conflict(
                "Привязка игры не соответствует активному профилю".into(),
            ));
        }
        // Before initial binding the live directory is authoritative for the active profile.
        let data = if self.runtime.active_profile() == Some(profile.id)
            && fs::read_symlink(&source)
                .map_err(ServiceError::Adapter)?
                .is_none()
            && fs::exists(&source).map_err(ServiceError::Adapter)?
        {
            source
        } else {
            target
        };
        self.snapshot_tree(game, profile, kind, &data)
    }

    fn snapshot_tree(
        &self,
        game: GameId,
        profile: &Profile,
        kind: SaveKind,
        data: &std::path::Path,
    ) -> ServiceResult<Save> {
        let history = self.history()?;
        fs::create_dir_all(&history).map_err(ServiceError::Adapter)?;
        let mut saves = config::read_saves(&history.join("index.json"))
            .map_err(ServiceError::Adapter)?
            .unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let destination = history.join(&id);
        if matches!(kind, SaveKind::Manual) {
            fs::copy_live_tree(data, &destination).map_err(ServiceError::Adapter)?;
        } else {
            fs::copy_tree(data, &destination).map_err(ServiceError::Adapter)?;
            self.stopped(game)?;
        }
        let session = if matches!(kind, SaveKind::AfterSession) {
            self.runtime.pending_session(game, profile.id)
        } else {
            self.runtime.session(game)
        };
        let save = Save {
            session_id: session.map(|s| s.id),
            session_started_at: session.map(|s| s.started_at),
            session_ended_at: session.and_then(|s| s.ended_at),
            id,
            game_id: game,
            profile_id: profile.id,
            kind,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            size_bytes: fs::size(&destination).map_err(ServiceError::Adapter)?,
        };
        if save.session_id.is_some() && save.session_ended_at.is_some() {
            for previous in &mut saves {
                if previous.session_id == save.session_id {
                    previous.session_ended_at = save.session_ended_at;
                }
            }
        }
        saves.push(save.clone());
        if let Err(error) = config::write_saves(&history.join("index.json"), &saves) {
            let _ = fs::remove_tree(&destination);
            return Err(ServiceError::Adapter(error));
        }
        Ok(save)
    }

    pub fn list(
        &self,
        game_id: Option<GameId>,
        profile_id: Option<&str>,
    ) -> ServiceResult<Vec<Save>> {
        let _guard = self.lock()?;
        let profile = profile_id.map(|id| self.profile(id)).transpose()?;
        if self.settings.data().local_repo_path.is_none() {
            return Ok(Vec::new());
        }
        let mut saves = config::read_saves(&self.history()?.join("index.json"))
            .map_err(ServiceError::Adapter)?
            .unwrap_or_default();
        for save in &saves {
            Self::validate_id(&save.id)?;
        }
        saves.retain(|s| {
            game_id.is_none_or(|id| id == s.game_id)
                && profile.as_ref().is_none_or(|p| p.id == s.profile_id)
        });
        saves.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(saves)
    }

    fn validate_id(id: &str) -> ServiceResult<()> {
        if uuid::Uuid::parse_str(id)
            .ok()
            .is_none_or(|uuid| uuid.to_string() != id)
        {
            return Err(ServiceError::InvalidInput(
                "Неверный UUID сохранения".into(),
            ));
        }
        Ok(())
    }

    pub fn get_save(&self, save_id: &str) -> ServiceResult<Save> {
        Self::validate_id(save_id)?;
        self.list(None, None)?
            .into_iter()
            .find(|s| s.id == save_id)
            .ok_or_else(|| ServiceError::NotFound("Сохранение не найдено".into()))
    }

    /// Restores only to its own active profile, with an unconditional BeforeRestore backup.
    pub fn apply_save(&self, save_id: &str) -> ServiceResult<()> {
        let save = self.get_save(save_id)?;
        let _guard = self.lock()?;
        self.stopped(save.game_id)?;
        if self.runtime.active_profile() != Some(save.profile_id) {
            return Err(ServiceError::Conflict(
                "Сначала переключитесь на профиль этого сохранения".into(),
            ));
        }
        if self
            .runtime
            .conflicts()
            .iter()
            .any(|c| c.game_id == save.game_id && c.profile_id == save.profile_id)
        {
            return Err(ServiceError::Conflict(
                "Сначала разрешите конфликт игры".into(),
            ));
        }
        let history = self.history()?;
        let snapshot = history.join(&save.id);
        fs::require_plain_path(&history, &snapshot).map_err(ServiceError::Adapter)?;
        if fs::size(&snapshot).map_err(ServiceError::Adapter)? != save.size_bytes {
            return Err(ServiceError::Conflict(
                "Размер снимка не соответствует метаданным; восстановление отменено".into(),
            ));
        }
        let profile = self.profile(&save.profile_id.to_string())?;
        let target = self.target(save.game_id, &profile)?;
        let source = self.source(save.game_id)?;
        self.validate_link(save.game_id, &source)?;
        let linked = fs::read_symlink(&source)
            .map_err(ServiceError::Adapter)?
            .is_some();
        self.create_inner(save.game_id, &profile, SaveKind::BeforeRestore)?;
        // An unbound live directory and an older managed copy may both contain unique data.
        if !linked
            && fs::exists(&source).map_err(ServiceError::Adapter)?
            && !fs::trees_equal(&source, &target).map_err(ServiceError::Adapter)?
        {
            self.snapshot_tree(save.game_id, &profile, SaveKind::BeforeRestore, &target)?;
        }
        let staged = target.with_file_name(format!(".savehub-restore-{}", uuid::Uuid::new_v4()));
        fs::copy_tree(&snapshot, &staged).map_err(ServiceError::Adapter)?;
        self.stopped(save.game_id)?;
        let backup = fs::replace_directory(&target, &staged).map_err(ServiceError::Adapter)?;
        if !linked {
            let original =
                source.with_file_name(format!(".savehub-original-{}", uuid::Uuid::new_v4()));
            let existed = fs::exists(&source).map_err(ServiceError::Adapter)?;
            let binding = (|| {
                if existed {
                    fs::rename(&source, &original)?;
                }
                if let Err(error) = fs::create_symlink(&source, &target) {
                    if existed {
                        fs::rename(&original, &source)?;
                    }
                    return Err(error);
                }
                Ok(())
            })();
            if let Err(error) = binding {
                let rollback = (|| {
                    fs::rename(&target, &staged)?;
                    fs::rename(&backup, &target)
                })();
                return match rollback {
                    Ok(()) => {
                        let _ = fs::remove_tree(&staged);
                        Err(ServiceError::Adapter(error))
                    }
                    Err(rollback) => Err(ServiceError::Conflict(format!(
                        "Восстановление привязки: {error:?}; откат: {rollback:?}; резервная копия: {}",
                        backup.display()
                    ))),
                };
            }
        }
        // Both previous versions have durable snapshots; a regular live source also remains beside the link.
        fs::remove_tree(&backup).map_err(ServiceError::Adapter)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    struct Fixture {
        dir: PathBuf,
        source: PathBuf,
        service: SaveService,
        game: GameId,
        profile: ProfileId,
    }
    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("savehub-save-test-{}", uuid::Uuid::new_v4()));
            let root = dir.join("repo");
            let source = dir.join("live");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("save.dat"), b"original\0\xff").unwrap();
            git2::Repository::init(&root).unwrap();
            let commit = git::commit_files(&root, None, &[], "initial").unwrap();
            git::create_branch(&root, "main", &commit).unwrap();
            git::checkout(&root, "main").unwrap();
            let game = uuid::Uuid::new_v4();
            let profile = uuid::Uuid::new_v4();
            let catalog = Arc::new(Catalog::new(
                HashMap::from([(
                    game,
                    Game {
                        id: game,
                        name: "Game".into(),
                        paths: GamePaths {
                            save_dir_path: source.to_string_lossy().into(),
                            exe_path: dir.join("game.exe").to_string_lossy().into(),
                        },
                        process_names: vec![],
                    },
                )]),
                HashMap::from([(
                    profile,
                    Profile {
                        id: profile,
                        name: "Main".into(),
                        branch: "main".into(),
                        last_played_game: None,
                    },
                )]),
            ));
            let runtime = Arc::new(RuntimeState::new(
                Some(profile),
                vec![],
                SyncStatus {
                    state: SyncState::NotConfigured,
                    last_sync_at: None,
                    error: None,
                },
            ));
            runtime.set_running_games(vec![]);
            let settings = Arc::new(Settings::new(
                SettingsData {
                    catalog_repo_url: None,
                    repo_url: None,
                    local_repo_path: Some(root),
                    sync_after_session: true,
                    sync_while_gaming: false,
                    auto_switch_profile: false,
                    ask_profile_before_launch: true,
                    start_minimized: false,
                },
                LocalConfig {
                    machine_id: "test".into(),
                    games: HashMap::new(),
                },
            ));
            Self {
                dir,
                source,
                service: SaveService::new(catalog, runtime, settings),
                game,
                profile,
            }
        }
        fn snapshot(&self) -> Save {
            self.service
                .create_save(self.game, &self.profile.to_string(), SaveKind::AfterSession)
                .unwrap()
        }
        fn bind(&self) {
            self.service
                .bind(self.game, &self.profile.to_string())
                .unwrap();
        }
        fn target(&self) -> PathBuf {
            self.service
                .target(
                    self.game,
                    &self.service.profile(&self.profile.to_string()).unwrap(),
                )
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn manual_and_final_snapshots_share_the_recorded_session_and_preserve_live_data() {
        let f = Fixture::new();
        f.bind();
        f.service.runtime.set_running_games(vec![f.game]);
        let manual = f.service.capture_manual(f.game).unwrap();
        assert!(matches!(manual.kind, SaveKind::Manual));
        assert!(manual.session_id.is_some());
        assert!(manual.session_started_at.is_some());
        assert!(manual.session_ended_at.is_none());
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        f.service.runtime.set_running_games(vec![]);
        let final_save = f
            .service
            .capture_session(f.game, f.profile)
            .unwrap()
            .unwrap();
        assert_eq!(manual.session_id, final_save.session_id);
        assert_eq!(manual.session_started_at, final_save.session_started_at);
        assert!(final_save.session_ended_at.unwrap() >= manual.session_started_at.unwrap());
        let stored_manual = f.service.get_save(&manual.id).unwrap();
        assert_eq!(stored_manual.session_ended_at, final_save.session_ended_at);
        assert!(f.service.capture_manual(f.game).is_err());
    }

    #[test]
    fn manual_snapshot_does_not_bind_a_running_game_or_accept_unknown_processes() {
        let f = Fixture::new();
        f.service.runtime.set_running_games(vec![f.game]);
        assert!(f.service.capture_manual(f.game).is_err());
        assert!(fs::read_symlink(&f.source).unwrap().is_none());
        f.service.runtime.invalidate_process_state();
        assert!(matches!(
            f.service.capture_manual(f.game),
            Err(ServiceError::ProcessesUnknown)
        ));
    }

    #[test]
    fn import_bind_snapshot_restore_and_detach_preserve_data() {
        let f = Fixture::new();
        f.bind();
        assert!(fs::read_symlink(&f.source).unwrap().is_some());
        assert_eq!(
            fs::read(&f.target().join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        let save = f.snapshot();
        std::fs::write(f.source.join("save.dat"), b"new version").unwrap();
        std::fs::write(f.source.join("extra"), b"extra").unwrap();
        f.service.apply_save(&save.id).unwrap();
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        assert!(!f.source.join("extra").exists());
        let saves = f
            .service
            .list(Some(f.game), Some(&f.profile.to_string()))
            .unwrap();
        let backup = saves
            .iter()
            .find(|s| matches!(s.kind, SaveKind::BeforeRestore))
            .unwrap();
        assert_eq!(
            fs::read(
                &f.service
                    .history()
                    .unwrap()
                    .join(&backup.id)
                    .join("save.dat")
            )
            .unwrap(),
            b"new version"
        );
        f.service.unbind(f.game).unwrap();
        assert!(fs::read_symlink(&f.source).unwrap().is_none());
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        assert!(f.target().join("save.dat").exists());
    }

    #[test]
    fn differing_unbound_files_block_binding_and_can_be_restored() {
        let f = Fixture::new();
        let save = f.snapshot();
        std::fs::write(f.source.join("save.dat"), b"latest").unwrap();
        assert!(matches!(
            f.service.bind(f.game, &f.profile.to_string()),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(fs::read(&f.source.join("save.dat")).unwrap(), b"latest");
        f.service.apply_save(&save.id).unwrap();
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
    }

    #[test]
    fn unknown_or_running_processes_block_mutations() {
        let f = Fixture::new();
        f.service.runtime.invalidate_process_state();
        assert!(matches!(
            f.service.initialize(f.game, &f.profile.to_string()),
            Err(ServiceError::ProcessesUnknown)
        ));
        f.service.runtime.set_running_games(vec![f.game]);
        assert!(matches!(
            f.service.bind(f.game, &f.profile.to_string()),
            Err(ServiceError::GameRunning(_))
        ));
        assert!(
            f.service
                .create_save(f.game, &f.profile.to_string(), SaveKind::AfterSession)
                .is_err()
        );
        assert!(fs::read_symlink(&f.source).unwrap().is_none());
    }

    #[test]
    fn history_survives_restart_and_rejects_bad_ids_and_metadata() {
        let f = Fixture::new();
        let save = f.snapshot();
        let reopened = SaveService::new(
            f.service.catalog.clone(),
            f.service.runtime.clone(),
            f.service.settings.clone(),
        );
        assert_eq!(reopened.get_save(&save.id).unwrap().size_bytes, 10);
        assert!(reopened.get_save("../outside").is_err());
        std::fs::write(f.service.history().unwrap().join("index.json"), "broken").unwrap();
        assert!(reopened.list(None, None).is_err());
        assert!(
            reopened
                .create_save(f.game, &f.profile.to_string(), SaveKind::FileChanged)
                .is_err()
        );
    }

    #[test]
    fn new_profile_starts_empty_and_cannot_restore_another_profiles_save() {
        let f = Fixture::new();
        let save = f.snapshot();
        let other = uuid::Uuid::new_v4();
        f.service.catalog.add_profile(Profile {
            id: other,
            name: "Other".into(),
            branch: "other".into(),
            last_played_game: None,
        });
        f.service.initialize(f.game, &other.to_string()).unwrap();
        let target = f
            .service
            .target(f.game, &f.service.profile(&other.to_string()).unwrap())
            .unwrap();
        assert_eq!(fs::size(&target).unwrap(), 0);
        f.service.runtime.set_active_profile(Some(other)).unwrap();
        assert!(matches!(
            f.service.apply_save(&save.id),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
    }

    #[test]
    fn ignored_paths_and_foreign_links_are_not_modified() {
        let f = Fixture::new();
        f.service
            .settings
            .add_game_resolution(f.game, PathResolution::Ignore);
        assert!(f.service.bind(f.game, &f.profile.to_string()).is_err());
        f.service.settings.clear_game_resolution(&f.game);
        let foreign = f.dir.join("foreign");
        std::fs::rename(&f.source, &foreign).unwrap();
        fs::create_symlink(&f.source, &foreign).unwrap();
        assert!(f.service.bind(f.game, &f.profile.to_string()).is_err());
        assert!(f.service.unbind(f.game).is_err());
        assert_eq!(
            fs::read(&foreign.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
    }

    #[test]
    fn nested_links_do_not_publish_partial_import_or_snapshot() {
        let f = Fixture::new();
        fs::create_symlink(&f.source.join("nested"), &f.dir).unwrap();
        assert!(
            f.service
                .initialize(f.game, &f.profile.to_string())
                .is_err()
        );
        assert!(!f.target().exists());
        assert!(f.service.list(None, None).unwrap().is_empty());
    }

    #[test]
    fn local_override_and_snapshot_filters_use_domain_ids() {
        let f = Fixture::new();
        let local = f.dir.join("local");
        std::fs::create_dir(&local).unwrap();
        std::fs::write(local.join("local.dat"), b"local").unwrap();
        f.service.settings.add_game_resolution(
            f.game,
            PathResolution::UseLocal(GamePaths {
                save_dir_path: local.to_string_lossy().into(),
                exe_path: "unused".into(),
            }),
        );
        let save = f.snapshot();
        assert_eq!(save.size_bytes, 5);
        assert_eq!(f.service.list(Some(f.game), None).unwrap().len(), 1);
        assert!(
            f.service
                .list(Some(uuid::Uuid::new_v4()), None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
    }
    #[test]
    fn corrupted_or_missing_snapshot_never_replaces_live_data() {
        let f = Fixture::new();
        f.bind();
        let save = f.snapshot();
        let snapshot = f.service.history().unwrap().join(&save.id);
        std::fs::write(snapshot.join("save.dat"), b"truncated").unwrap();
        assert!(f.service.apply_save(&save.id).is_err());
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        std::fs::remove_dir_all(snapshot).unwrap();
        assert!(f.service.apply_save(&save.id).is_err());
        assert_eq!(f.service.list(None, None).unwrap().len(), 1);
    }

    #[test]
    fn interrupted_binding_does_not_import_another_profiles_data() {
        let f = Fixture::new();
        f.bind();
        let other = uuid::Uuid::new_v4();
        f.service.catalog.add_profile(Profile {
            id: other,
            name: "Other".into(),
            branch: "other".into(),
            last_played_game: None,
        });
        f.service.runtime.set_active_profile(Some(other)).unwrap();
        assert!(f.service.initialize(f.game, &other.to_string()).is_err());
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
    }
    #[test]
    fn restore_reconnects_a_missing_game_directory() {
        let f = Fixture::new();
        f.bind();
        let save = f.snapshot();
        fs::remove_symlink(&f.source).unwrap();
        std::fs::write(f.target().join("save.dat"), b"changed").unwrap();
        f.service.apply_save(&save.id).unwrap();
        assert!(f.service.is_bound(f.game, &f.profile).unwrap());
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
    }

    #[test]
    fn restore_recovers_owned_dangling_link_after_git_deletes_last_file() {
        let f = Fixture::new();
        f.bind();
        let save = f.snapshot();
        let target = f.target();
        let tree = target.parent().unwrap().parent().unwrap();
        git::stage(tree, &git::status(tree).unwrap()).unwrap();
        let base = git::commit(tree, "saved files").unwrap();
        let empty = git::commit_files(tree, Some(&base), &[], "remote deletion").unwrap();
        git::merge(tree, &empty, true).unwrap();
        assert!(!target.exists());
        assert!(fs::read_symlink(&f.source).unwrap().is_some());
        f.service.apply_save(&save.id).unwrap();
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        assert!(f.service.is_bound(f.game, &f.profile).unwrap());
        assert!(
            f.service
                .list(None, None)
                .unwrap()
                .iter()
                .any(|s| matches!(s.kind, SaveKind::BeforeRestore))
        );
    }

    #[test]
    fn own_dangling_links_can_be_rebound_or_detached_but_foreign_links_are_preserved() {
        let f = Fixture::new();
        f.bind();
        std::fs::remove_dir_all(f.target()).unwrap();
        assert!(f.service.is_bound(f.game, &f.profile).unwrap());
        assert!(!f.target().exists());
        f.bind();
        assert!(f.source.is_dir());
        std::fs::remove_dir_all(f.target()).unwrap();
        f.service.unbind(f.game).unwrap();
        assert!(f.source.is_dir());
        assert!(fs::read_symlink(&f.source).unwrap().is_none());

        std::fs::remove_dir(&f.source).unwrap();
        let foreign = f.dir.join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        fs::create_symlink(&f.source, &foreign).unwrap();
        std::fs::remove_dir(&foreign).unwrap();
        assert!(!f.service.is_bound(f.game, &f.profile).unwrap());
        assert!(f.service.bind(f.game, &f.profile.to_string()).is_err());
        assert!(f.service.unbind(f.game).is_err());
        assert!(fs::read_symlink(&f.source).unwrap().is_some());
        assert!(!foreign.exists());
    }

    #[test]
    fn restore_unbound_data_preserves_both_previous_versions_and_allows_launch_binding() {
        let f = Fixture::new();
        let save = f.snapshot();
        std::fs::write(f.source.join("save.dat"), b"live").unwrap();
        std::fs::write(f.target().join("save.dat"), b"managed").unwrap();
        f.service.apply_save(&save.id).unwrap();
        f.bind();
        assert_eq!(
            fs::read(&f.source.join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        let versions: Vec<_> = f
            .service
            .list(None, None)
            .unwrap()
            .iter()
            .map(|s| fs::read(&f.service.history().unwrap().join(&s.id).join("save.dat")).unwrap())
            .collect();
        assert!(versions.contains(&b"live".to_vec()));
        assert!(versions.contains(&b"managed".to_vec()));
    }

    #[test]
    fn reading_counts_and_validating_a_binding_does_not_create_profile_branches() {
        let f = Fixture::new();
        f.bind();
        let other = uuid::Uuid::new_v4();
        f.service.catalog.add_profile(Profile {
            id: other,
            name: "Remote".into(),
            branch: "not-fetched".into(),
            last_played_game: None,
        });
        assert_eq!(f.service.games_count(&other).unwrap(), 0);
        f.snapshot();
        assert_eq!(
            git::list_branches(&f.service.root().unwrap()).unwrap(),
            vec!["main"]
        );
    }

    #[test]
    fn git_conflicts_block_restore_even_when_runtime_has_not_loaded_them() {
        let f = Fixture::new();
        f.bind();
        let save = f.snapshot();
        let root = f.service.root().unwrap();
        let path = PathBuf::from(format!("games/{}/save.dat", f.game));
        git::stage(&root, std::slice::from_ref(&path)).unwrap();
        let base = git::commit(&root, "base").unwrap();
        let other = f.dir.join("other");
        git::create_worktree(&root, "other", &other, Some(&base)).unwrap();
        std::fs::write(root.join(&path), b"local\n").unwrap();
        git::stage(&root, std::slice::from_ref(&path)).unwrap();
        git::commit(&root, "local").unwrap();
        std::fs::write(other.join(&path), b"remote\n").unwrap();
        git::stage(&other, std::slice::from_ref(&path)).unwrap();
        let remote = git::commit(&other, "remote").unwrap();
        assert!(matches!(
            git::merge(&root, &remote, false).unwrap(),
            git::MergeOutcome::Conflicts(_)
        ));
        assert!(f.service.runtime.conflicts().is_empty());
        let before = fs::read(&f.source.join("save.dat")).unwrap();
        assert!(matches!(
            f.service.apply_save(&save.id),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(fs::read(&f.source.join("save.dat")).unwrap(), before);
        assert!(f.service.bind(f.game, &f.profile.to_string()).is_err());
    }
    #[test]
    fn delayed_session_snapshot_uses_original_tree_after_game_was_ignored_or_deleted() {
        let f = Fixture::new();
        f.bind();
        f.service.runtime.set_running_games(vec![f.game]);
        f.service.runtime.set_running_games(vec![]);
        f.service
            .settings
            .add_game_resolution(f.game, PathResolution::Ignore);
        f.service.catalog.remove_game(f.game);
        let save = f
            .service
            .capture_session(f.game, f.profile)
            .unwrap()
            .unwrap();
        assert!(matches!(save.kind, SaveKind::AfterSession));
        assert_eq!(
            fs::read(&f.service.history().unwrap().join(save.id).join("save.dat")).unwrap(),
            b"original\0\xff"
        );
        f.service.runtime.acknowledge_session(f.game, f.profile);
        assert!(f.service.runtime.pending_sessions().is_empty());
    }
}
