use super::{ServiceError, ServiceResult};
use crate::domain::*;
use std::sync::{Arc, Mutex};

pub fn validate_catalog_url(url: &str) -> ServiceResult<()> {
    let path = url.strip_prefix("https://github.com/").ok_or_else(|| {
        ServiceError::InvalidInput("Укажите HTTPS-адрес репозитория на github.com".into())
    })?;
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    {
        return Err(ServiceError::InvalidInput(
            "Ожидается https://github.com/owner/repository".into(),
        ));
    }
    Ok(())
}

pub struct SettingsService {
    settings: Arc<Settings>,
    path: std::path::PathBuf,
    operations: Arc<Mutex<()>>,
    bindings: Option<(Arc<Catalog>, Arc<RuntimeState>)>,
}

impl SettingsService {
    pub fn new(
        settings: Arc<Settings>,
        path: std::path::PathBuf,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            settings,
            path,
            operations,
            bindings: None,
        }
    }

    pub fn with_bindings(mut self, catalog: Arc<Catalog>, runtime: Arc<RuntimeState>) -> Self {
        self.bindings = Some((catalog, runtime));
        self
    }

    /// Захватывает operations; вызывающий код не должен уже удерживать этот mutex.
    pub fn update(&self, change: impl FnOnce(&mut SettingsData)) -> ServiceResult<()> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Repository operations lock poisoned".into()))?;
        let mut data = self.settings.data();
        change(&mut data);
        data.repo_url = data
            .repo_url
            .map(|url| url.trim().trim_end_matches('/').to_owned())
            .filter(|url| !url.is_empty());
        data.local_repo_path = data
            .local_repo_path
            .filter(|path| !path.as_os_str().is_empty());
        if let Some(path) = &data.local_repo_path {
            if !path.is_absolute() {
                return Err(ServiceError::InvalidInput(
                    "Укажите абсолютный путь репозитория".into(),
                ));
            }
            crate::adapters::git::validate_repository_root(path).map_err(ServiceError::Adapter)?;
        }
        if let Some(url) = &data.repo_url {
            validate_catalog_url(url)?;
            if let Some(path) = &data.local_repo_path {
                if path.join(".git").exists() {
                    crate::adapters::git::validate_remote(path, "origin", url)
                        .map_err(ServiceError::Adapter)?;
                }
            }
        }
        data.catalog_repo_url = data
            .catalog_repo_url
            .map(|url| url.trim().trim_end_matches('/').to_owned())
            .filter(|url| !url.is_empty());
        if let Some(url) = &data.catalog_repo_url {
            validate_catalog_url(url)?;
            if data.repo_url.as_deref().is_some_and(|s| {
                s.trim()
                    .trim_end_matches('/')
                    .trim_end_matches(".git")
                    .eq_ignore_ascii_case(url.trim_end_matches(".git"))
            }) {
                return Err(ServiceError::InvalidInput("Для списка игр нужен отдельный репозиторий, отличный от репозитория сохранений".into()));
            }
        }

        if data.local_repo_path != self.settings.data().local_repo_path {
            if let Some((catalog, runtime)) = &self.bindings {
                if !runtime.pending_sessions().is_empty() {
                    return Err(ServiceError::Conflict(
                        "Дождитесь сохранения завершённой игровой сессии перед сменой репозитория"
                            .into(),
                    ));
                }
                let mut affected = Vec::new();
                for game in catalog.games().values() {
                    affected.extend(
                        super::bindings::linked_paths(game, &self.settings.local())?
                            .into_iter()
                            .map(|p| (game.id, p)),
                    );
                }
                super::bindings::detach(catalog, runtime, &self.settings, &affected)?;
            }
        }
        crate::adapters::config::write_settings(&self.path, &data)
            .map_err(ServiceError::Adapter)?;
        self.settings.replace_data(data);
        Ok(())
    }

    pub fn get(&self) -> ServiceResult<SettingsData> {
        Ok(self.settings.data())
    }

    pub fn set_repository(
        &self,
        url: Option<String>,
        path: Option<std::path::PathBuf>,
    ) -> ServiceResult<()> {
        self.update(|data| {
            data.repo_url = url;
            data.local_repo_path = path;
        })
    }

    pub fn set_catalog_repository(&self, url: Option<String>) -> ServiceResult<()> {
        self.update(|data| data.catalog_repo_url = url)
    }

    pub fn set_sync_after_session(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.sync_after_session = enabled)
    }

    pub fn set_sync_while_gaming(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.sync_while_gaming = enabled)
    }

    pub fn set_auto_switch_profile(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.auto_switch_profile = enabled)
    }

    pub fn set_ask_profile_before_launch(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.ask_profile_before_launch = enabled)
    }

    pub fn set_start_minimized(&self, enabled: bool) -> ServiceResult<()> {
        self.update(|data| data.start_minimized = enabled)
    }

    pub fn has_github_token(&self) -> ServiceResult<bool> {
        crate::adapters::credentials::has_token().map_err(ServiceError::Adapter)
    }

    /// Записать токен в системное хранилище.
    pub fn set_github_token(&self, token: &str) -> ServiceResult<()> {
        crate::adapters::credentials::set_token(token).map_err(ServiceError::Adapter)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use crate::adapters::config;
    use std::{collections::HashMap, path::PathBuf};

    struct Fixture {
        root: PathBuf,
        service: SettingsService,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("savehub-settings-{}", uuid::Uuid::new_v4()));
            let service = SettingsService::new(
                Arc::new(Settings::new(
                    SettingsData {
                        ask_profile_before_launch: true,
                        catalog_repo_url: None,
                        repo_url: None,
                        local_repo_path: None,
                        sync_after_session: false,
                        sync_while_gaming: false,
                        auto_switch_profile: false,
                        start_minimized: true,
                    },
                    LocalConfig {
                        machine_id: "test".into(),
                        games: HashMap::new(),
                    },
                )),
                root.join("settings.json"),
                Arc::new(Mutex::new(())),
            );
            service.update(|_| {}).unwrap();
            Self { root, service }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn invalid_or_nested_repository_path_does_not_change_settings() {
        let fixture = Fixture::new();
        let root = fixture.root.join("repo");
        git2::Repository::init(&root).unwrap();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let before = std::fs::read(&fixture.service.path).unwrap();
        for path in [fixture.root.join("missing"), nested, root.join(".git")] {
            assert!(fixture.service.set_repository(None, Some(path)).is_err());
            assert_eq!(std::fs::read(&fixture.service.path).unwrap(), before);
            assert!(fixture.service.get().unwrap().local_repo_path.is_none());
        }
        fixture.service.set_repository(None, Some(root)).unwrap();
    }

    #[test]
    fn empty_repository_fields_disable_network_and_remove_local_path() {
        let fixture = Fixture::new();
        fixture
            .service
            .set_repository(Some("  ".into()), Some(PathBuf::new()))
            .unwrap();
        let stored = config::read_settings(&fixture.service.path)
            .unwrap()
            .unwrap();
        assert!(stored.repo_url.is_none());
        assert!(stored.local_repo_path.is_none());
        assert!(fixture.service.get().unwrap().repo_url.is_none());
    }

    #[test]
    fn different_origin_is_rejected_before_saving_settings() {
        let fixture = Fixture::new();
        let root = fixture.root.join("saves");
        let repository = git2::Repository::init(&root).unwrap();
        let original = "https://github.com/test/original";
        repository.remote("origin", original).unwrap();
        let before = std::fs::read(&fixture.service.path).unwrap();
        assert!(
            fixture
                .service
                .set_repository(
                    Some("https://github.com/test/other".into()),
                    Some(root.clone())
                )
                .is_err()
        );
        assert_eq!(std::fs::read(&fixture.service.path).unwrap(), before);
        assert_eq!(
            repository.find_remote("origin").unwrap().url().unwrap(),
            original
        );
        fixture
            .service
            .set_repository(
                Some(" https://github.com/test/original.git/ ".into()),
                Some(root),
            )
            .unwrap();
    }

    #[test]
    fn invalid_update_changes_neither_memory_nor_disk() {
        let fixture = Fixture::new();
        let before = std::fs::read(&fixture.service.path).unwrap();
        assert!(
            fixture
                .service
                .update(|data| {
                    data.repo_url = Some("https://github.com/owner/saves".into());
                    data.catalog_repo_url = Some("https://example.com/owner/catalog".into());
                    data.sync_after_session = true;
                })
                .is_err()
        );
        assert_eq!(std::fs::read(&fixture.service.path).unwrap(), before);
        assert!(fixture.service.get().unwrap().repo_url.is_none());
        assert!(!fixture.service.get().unwrap().sync_after_session);
        fixture
            .service
            .set_catalog_repository(Some("https://github.com/owner/catalog".into()))
            .unwrap();
        assert!(
            fixture
                .service
                .set_repository(Some("https://github.com/OWNER/catalog.git".into()), None)
                .is_err()
        );
        assert!(fixture.service.get().unwrap().repo_url.is_none());
    }

    #[test]
    fn write_failure_does_not_publish_settings_to_memory() {
        let fixture = Fixture::new();
        std::fs::remove_file(&fixture.service.path).unwrap();
        std::fs::create_dir(&fixture.service.path).unwrap();
        assert!(fixture.service.set_sync_after_session(true).is_err());
        assert!(!fixture.service.get().unwrap().sync_after_session);
    }

    #[test]
    fn parallel_updates_keep_both_changes_on_disk() {
        let fixture = Fixture::new();
        std::thread::scope(|scope| {
            scope.spawn(|| fixture.service.set_sync_after_session(true).unwrap());
            scope.spawn(|| fixture.service.set_sync_while_gaming(true).unwrap());
        });
        let saved = config::read_settings(&fixture.service.path)
            .unwrap()
            .unwrap();
        assert!(saved.sync_after_session && saved.sync_while_gaming && saved.start_minimized);
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            serde_json::to_value(fixture.service.get().unwrap()).unwrap()
        );
    }
}
