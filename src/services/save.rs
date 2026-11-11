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
        if fs::read_symlink(source)
            .map_err(ServiceError::Adapter)?
            .is_none()
        {
            return Ok(());
        }
        let actual = fs::canonicalize(source).map_err(ServiceError::Adapter)?;
        for profile in self.catalog.profiles().values() {
            let expected = self.target(game, profile)?;
            if fs::exists(&expected).map_err(ServiceError::Adapter)?
                && fs::canonicalize(&expected).map_err(ServiceError::Adapter)? == actual
            {
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
        let mut count = 0;
        for game in self.catalog.games().keys() {
            if fs::is_directory(&self.target(*game, &profile)?).map_err(ServiceError::Adapter)? {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn is_bound(&self, game: GameId, profile: &ProfileId) -> ServiceResult<bool> {
        let _guard = self.lock()?;
        let source = self.source(game)?;
        if fs::read_symlink(&source)
            .map_err(ServiceError::Adapter)?
            .is_none()
        {
            return Ok(false);
        }
        let target = self.target(game, &self.profile(&profile.to_string())?)?;
        Ok(fs::canonicalize(&source).map_err(ServiceError::Adapter)?
            == fs::canonicalize(&target).map_err(ServiceError::Adapter)?)
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
        if self.runtime.active_profile() == Some(profile.id)
            && fs::read_symlink(&source)
                .map_err(ServiceError::Adapter)?
                .is_some()
        {
            return Err(ServiceError::Conflict(
                "Ссылка игры указывает на другой профиль; импорт остановлен".into(),
            ));
        }
        fs::create_dir_all(target.parent().unwrap()).map_err(ServiceError::Adapter)?;
        if self.runtime.active_profile() == Some(profile.id)
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
        fs::copy_tree(&source, &staged).map_err(ServiceError::Adapter)?;
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
        let history = self.history()?;
        fs::create_dir_all(&history).map_err(ServiceError::Adapter)?;
        let mut saves = config::read_saves(&history.join("index.json"))
            .map_err(ServiceError::Adapter)?
            .unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let destination = history.join(&id);
        fs::copy_tree(&data, &destination).map_err(ServiceError::Adapter)?;
        self.stopped(game)?;
        let save = Save {
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
        saves.push(save.clone());
        config::write_saves(&history.join("index.json"), &saves).map_err(ServiceError::Adapter)?;
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
        let destination = if fs::read_symlink(&source)
            .map_err(ServiceError::Adapter)?
            .is_none()
            && fs::exists(&source).map_err(ServiceError::Adapter)?
        {
            source
        } else {
            target
        };
        let staged =
            destination.with_file_name(format!(".savehub-restore-{}", uuid::Uuid::new_v4()));
        fs::copy_tree(&snapshot, &staged).map_err(ServiceError::Adapter)?;
        self.create_inner(save.game_id, &profile, SaveKind::BeforeRestore)?;
        self.stopped(save.game_id)?;
        let backup = fs::replace_directory(&destination, &staged).map_err(ServiceError::Adapter)?;
        // The complete previous contents are already published in BeforeRestore.
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
}
