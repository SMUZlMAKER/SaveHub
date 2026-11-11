use super::{ServiceError, ServiceResult};
use crate::{
    adapters::{config, credentials, fs, git},
    domain::*,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

const CATALOG_BRANCH: &str = "savehub/catalog";
const LEASE_DURATION_MS: u64 = 120_000;

/// Complete save workflows must additionally hold their shared operations lock.
pub struct RepoService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    settings: Arc<Settings>,
    operations: Mutex<()>,
    catalog_baseline: Mutex<Option<serde_json::Value>>,
}

#[derive(Serialize, Deserialize)]
struct Lease {
    machine_id: String,
    expires_at: u64,
}
#[derive(Serialize, Deserialize)]
struct ConflictKey {
    profile: ProfileId,
    path: PathBuf,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Merge records by stable UUID; never silently overwrite concurrent edits.
fn merge_catalog_entries<T: Serialize + serde::de::DeserializeOwned>(
    baseline: Option<&serde_json::Value>,
    local: &[T],
    disk: &[T],
) -> ServiceResult<Vec<T>> {
    use std::collections::{BTreeMap, BTreeSet};
    fn records(value: serde_json::Value) -> ServiceResult<BTreeMap<String, serde_json::Value>> {
        let mut result = BTreeMap::new();
        for item in value
            .as_array()
            .ok_or_else(|| ServiceError::InvalidInput("Ожидается список каталога".into()))?
        {
            let id = item
                .get("id")
                .and_then(|id| id.as_str())
                .ok_or_else(|| ServiceError::InvalidInput("Нет UUID записи каталога".into()))?;
            if result.insert(id.to_owned(), item.clone()).is_some() {
                return Err(ServiceError::Conflict("Повторяющийся UUID каталога".into()));
            }
        }
        Ok(result)
    }
    let encode = |items: &[T]| {
        serde_json::to_value(items).map_err(|e| ServiceError::InvalidInput(e.to_string()))
    };
    let local = records(encode(local)?)?;
    let disk = records(encode(disk)?)?;
    let base = baseline.cloned().map(records).transpose()?;
    let ids: BTreeSet<_> = local
        .keys()
        .chain(disk.keys())
        .chain(base.iter().flat_map(|b| b.keys()))
        .collect();
    let mut merged = Vec::new();
    for id in ids {
        let ours = local.get(id);
        let theirs = disk.get(id);
        let before = base.as_ref().and_then(|b| b.get(id));
        let chosen = if ours == theirs {
            ours
        } else if base.is_none() && ours.is_none() {
            theirs
        } else if base.is_none() && theirs.is_none() {
            ours
        } else if base.is_some() && ours == before {
            theirs
        } else if base.is_some() && theirs == before {
            ours
        } else {
            return Err(ServiceError::Conflict(format!(
                "Одновременное изменение записи каталога {id}"
            )));
        };
        if let Some(item) = chosen {
            merged.push(
                serde_json::from_value(item.clone())
                    .map_err(|e| ServiceError::InvalidInput(e.to_string()))?,
            );
        }
    }
    Ok(merged)
}

fn unique_branch<'a, I>(base: &str, branches: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let branches: std::collections::HashSet<&str> = branches.into_iter().collect();
    if !branches.contains(base) {
        return base.to_owned();
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if !branches.contains(candidate.as_str()) {
            return candidate;
        }
        index += 1;
    }
}

impl RepoService {
    /// Создать отдельную пустую историю профиля и вернуть свободное имя ветки.
    pub fn create_profile_branch(&self, name: &str) -> ServiceResult<String> {
        let _guard = self.lock()?;
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Имя профиля не может быть пустым".into(),
            ));
        }
        let root = self
            .settings
            .data()
            .local_repo_path
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                ServiceError::InvalidInput("Сначала настройте локальный Git-репозиторий".into())
            })?;
        let profiles = self.catalog.profiles();
        let mut branches = git::list_branches(&root).map_err(ServiceError::Adapter)?;
        branches.extend(git::list_remote_branches(&root, "origin").map_err(ServiceError::Adapter)?);
        branches.extend(profiles.values().map(|profile| profile.branch.clone()));
        // A ref foo/bar also reserves foo: Git cannot store both namespaces.
        let occupied: Vec<_> = branches
            .iter()
            .map(|branch| branch.split('/').next().unwrap_or(branch))
            .collect();
        let base = crate::utils::utils::name_to_branch(name);
        let base = if base.is_empty() { "profile" } else { &base };
        let branch = unique_branch(base, occupied);
        // An independent root commit never inherits another profile's saves.
        let commit = git::commit_files(&root, None, &[], "Initialize independent save history")
            .map_err(ServiceError::Adapter)?;
        git::create_branch(&root, &branch, &commit).map_err(ServiceError::Adapter)?;
        Ok(branch)
    }

    /// Архивировать локальные данные профиля и удалить его ветку и worktree.
    /// Удалённая ветка сохраняется: удаление локального профиля не стирает другие ПК.
    pub fn delete_profile(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        let _guard = self.lock()?;
        let profile = self.profile(&profile_id.to_string())?;
        if self.runtime.active_profile() == Some(*profile_id) {
            return Err(ServiceError::InvalidInput(
                "Активный профиль удалить нельзя".into(),
            ));
        }
        self.stopped()?;
        if self
            .runtime
            .pending_sessions()
            .iter()
            .any(|(_, profile)| profile == profile_id)
        {
            return Err(ServiceError::Conflict(
                "Дождитесь сохранения завершённой игровой сессии".into(),
            ));
        }
        if self
            .runtime
            .conflicts()
            .iter()
            .any(|conflict| conflict.profile_id == *profile_id)
        {
            return Err(ServiceError::Conflict(
                "Сначала разрешите конфликты профиля".into(),
            ));
        }
        let root = self
            .settings
            .data()
            .local_repo_path
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                ServiceError::InvalidInput("Сначала настройте локальный Git-репозиторий".into())
            })?;
        if let Some(tree) = git::existing_worktree(&root, &profile.branch, &profile.id.to_string())
            .map_err(ServiceError::Adapter)?
        {
            let tree = fs::canonicalize(&tree).map_err(ServiceError::Adapter)?;
            for game in self.catalog.games().values() {
                for source in super::bindings::linked_paths(game, &self.settings.local())? {
                    if fs::canonicalize(&source)
                        .map_err(ServiceError::Adapter)?
                        .starts_with(tree.join("games").join(game.id.to_string()))
                    {
                        return Err(ServiceError::Conflict(
                            "На профиль ещё ссылается папка игры. Сначала отключите её сохранения"
                                .into(),
                        ));
                    }
                }
            }
        }
        let (_, _, lease) = self.lease(&root, &profile)?;
        if lease.is_some_and(|lease| {
            lease.expires_at > now() && lease.machine_id != self.settings.local().machine_id
        }) {
            return Err(ServiceError::Conflict(
                "Профиль используется другим компьютером".into(),
            ));
        }
        git::archive_profile_branch(&root, &profile.branch, &profile.id.to_string())
            .map_err(ServiceError::Adapter)?;
        Ok(())
    }
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            settings,
            operations: Mutex::new(()),
            catalog_baseline: Mutex::new(None),
        }
    }
    pub fn configuration(&self) -> Option<RepositoryConfig> {
        let settings = self.settings.data();
        let url = settings.repo_url.filter(|url| !url.trim().is_empty())?;
        let path = settings
            .local_repo_path
            .filter(|path| !path.as_os_str().is_empty())?;
        Some(RepositoryConfig { url, path })
    }

    fn lock(&self) -> ServiceResult<std::sync::MutexGuard<'_, ()>> {
        self.operations
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка репозитория повреждена".into()))
    }
    fn repository(&self) -> ServiceResult<(PathBuf, String)> {
        let data = self.settings.data();
        let path = data
            .local_repo_path
            .filter(|p| p.is_absolute())
            .ok_or_else(|| {
                ServiceError::InvalidInput("Укажите абсолютный путь репозитория".into())
            })?;
        let url = data
            .repo_url
            .filter(|url| git::trusted_github_url(url))
            .ok_or_else(|| {
                ServiceError::InvalidInput("Ожидается https://github.com/owner/repository".into())
            })?;
        Ok((path, url))
    }
    fn token(&self) -> ServiceResult<String> {
        credentials::get_token()
            .map_err(ServiceError::Adapter)?
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| ServiceError::InvalidInput("Задайте токен GitHub в настройках".into()))
    }
    fn network<T>(
        &self,
        operation: impl FnOnce(&Path, &str) -> ServiceResult<T>,
    ) -> ServiceResult<T> {
        let (path, url) = self.repository()?;
        let token = self.token()?;
        let previous = self.runtime.sync_status();
        self.runtime.set_sync_status(SyncStatus {
            state: SyncState::Syncing,
            error: None,
            ..previous.clone()
        });
        let result = (|| {
            git::set_remote(&path, "origin", &url).map_err(ServiceError::Adapter)?;
            operation(&path, &token)
        })();
        self.runtime.set_sync_status(match &result {
            Ok(_) => SyncStatus {
                state: SyncState::Idle,
                last_sync_at: Some(now()),
                error: None,
            },
            Err(_) => SyncStatus {
                state: SyncState::Error,
                error: Some("Операция с репозиторием не завершена".into()),
                ..previous
            },
        });
        result
    }
    /// Existing files and repositories are never replaced by a clone.
    pub fn connect(&self) -> ServiceResult<()> {
        let _guard = self.lock()?;
        let (path, url) = self.repository()?;
        if !fs::exists(&path.join(".git")).map_err(ServiceError::Adapter)? {
            git::clone(&url, &path, &self.token()?).map_err(ServiceError::Adapter)?;
        }
        self.network(|root, token| {
            git::fetch(root, "origin", token).map_err(ServiceError::Adapter)?;
            git::prepare_worktree(root, CATALOG_BRANCH, "catalog")
                .map_err(ServiceError::Adapter)?;
            Ok(())
        })
    }
    /// Profile metadata has its own branch; GameService owns the separate games repository.
    pub fn sync_catalog(&self) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.network(|root, token| self.sync_catalog_at(root, token))
    }

    fn sync_catalog_at(&self, root: &Path, token: &str) -> ServiceResult<()> {
        git::fetch(root, "origin", token).map_err(ServiceError::Adapter)?;
        let path = git::prepare_worktree(root, CATALOG_BRANCH, "catalog")
            .map_err(ServiceError::Adapter)?;
        if !git::conflict_paths(&path)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return Err(ServiceError::Conflict(
                "Не разрешён конфликт общего каталога".into(),
            ));
        }
        let mut profiles: Vec<_> = self.catalog.profiles().into_values().collect();
        profiles.sort_by_key(|p| p.id);
        let mut baseline = self
            .catalog_baseline
            .lock()
            .map_err(|_| ServiceError::Conflict("Блокировка каталога повреждена".into()))?;
        let disk_profiles = config::read_profiles(&path.join("profiles.json"))
            .map_err(ServiceError::Adapter)?
            .unwrap_or_default();
        let profiles = merge_catalog_entries(
            baseline.as_ref().and_then(|b| b.get("profiles")),
            &profiles,
            &disk_profiles,
        )?;
        config::write_profiles(&path.join("profiles.json"), &profiles)
            .map_err(ServiceError::Adapter)?;
        Self::commit_changes(&path, "Update shared catalog")?;
        if let Some(remote) =
            git::remote_head(root, CATALOG_BRANCH).map_err(ServiceError::Adapter)?
        {
            git::backup_worktree(&path).map_err(ServiceError::Adapter)?;
            match git::merge(&path, &remote, false).map_err(ServiceError::Adapter)? {
                git::MergeOutcome::Conflicts(_) => {
                    git::abort_merge(&path).map_err(ServiceError::Adapter)?;
                    return Err(ServiceError::Conflict(
                        "Конфликт общего каталога: локальный коммит сохранён".into(),
                    ));
                }
                git::MergeOutcome::Diverged => {
                    return Err(ServiceError::Conflict("Каталог разошёлся".into()));
                }
                _ => (),
            }
        }
        let profiles = config::read_profiles(&path.join("profiles.json"))
            .map_err(ServiceError::Adapter)?
            .unwrap_or_default();
        *baseline = Some(serde_json::json!({"profiles": profiles}));
        self.catalog.replace_profiles(profiles);
        git::push(
            root,
            "origin",
            &format!("refs/heads/{CATALOG_BRANCH}:refs/heads/{CATALOG_BRANCH}"),
            token,
        )
        .map_err(ServiceError::Adapter)?;
        Ok(())
    }
    pub fn status(&self) -> ServiceResult<SyncStatus> {
        let data = self.settings.data();
        if data.repo_url.is_none() || data.local_repo_path.is_none() {
            return Ok(SyncStatus {
                state: SyncState::NotConfigured,
                last_sync_at: None,
                error: None,
            });
        }
        Ok(self.runtime.sync_status())
    }
    /// Fetch only; never change working saves.
    pub fn fetch(&self) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.network(|root, token| git::fetch(root, "origin", token).map_err(ServiceError::Adapter))
    }
    fn profile(&self, id: &str) -> ServiceResult<Profile> {
        let profiles = self.catalog.profiles();
        let profile = profiles
            .values()
            .find(|p| p.id.to_string() == id || p.branch == id)
            .cloned()
            .ok_or_else(|| ServiceError::NotFound("Профиль не найден".into()))?;
        if profile.branch.starts_with("savehub/") {
            return Err(ServiceError::InvalidInput(
                "Зарезервированная ветка профиля".into(),
            ));
        }
        Ok(profile)
    }
    fn tree(&self, profile: &Profile) -> ServiceResult<PathBuf> {
        let root = self
            .settings
            .data()
            .local_repo_path
            .filter(|p| p.is_absolute())
            .ok_or_else(|| {
                ServiceError::InvalidInput("Укажите абсолютный путь репозитория".into())
            })?;
        git::prepare_worktree(&root, &profile.branch, &profile.id.to_string())
            .map_err(ServiceError::Adapter)
    }
    fn stopped(&self) -> ServiceResult<()> {
        match self.runtime.running_games() {
            None => Err(ServiceError::ProcessesUnknown),
            Some(games) if !games.is_empty() => Err(ServiceError::GameRunning(games)),
            _ => Ok(()),
        }
    }
    /// Apply fetched updates only after checking processes and preserving files.
    pub fn apply_profile_updates(&self, profile_id: &str) -> ServiceResult<Vec<Conflict>> {
        let _guard = self.lock()?;
        self.stopped()?;
        let profile = self.profile(profile_id)?;
        let path = self.tree(&profile)?;
        if !git::conflict_paths(&path)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return self.conflicts_for(&profile, &path);
        }
        let (root, _) = self.repository()?;
        if let Some(remote) =
            git::remote_head(&root, &profile.branch).map_err(ServiceError::Adapter)?
        {
            git::backup_worktree(&path).map_err(ServiceError::Adapter)?;
            self.stopped()?;
            match git::merge(&path, &remote, false).map_err(ServiceError::Adapter)? {
                git::MergeOutcome::Diverged => {
                    return Err(ServiceError::Conflict("История профиля разошлась".into()));
                }
                git::MergeOutcome::Conflicts(_) => return self.conflicts_for(&profile, &path),
                _ => (),
            }
        }
        Ok(Vec::new())
    }
    /// Prepare the persistent profile tree; the workflow binds saves and activates it.
    pub fn checkout_profile(&self, profile_id: &str) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.stopped()?;
        let profile = self.profile(profile_id)?;
        let path = self.tree(&profile)?;
        if !git::conflict_paths(&path)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return Err(ServiceError::Conflict("В профиле есть конфликты".into()));
        }
        Ok(())
    }
    fn commit_changes(path: &Path, message: &str) -> ServiceResult<Option<CommitId>> {
        if !git::conflict_paths(path)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return Err(ServiceError::Conflict(
                "Сначала разрешите конфликты сохранений".into(),
            ));
        }
        let paths = git::status(path).map_err(ServiceError::Adapter)?;
        if paths.iter().any(|path| {
            path.components().any(|part| {
                part.as_os_str()
                    .to_str()
                    .is_some_and(|name| name.starts_with(".savehub-"))
            })
        }) {
            return Err(ServiceError::Conflict("В рабочей папке остались временные данные незавершённой операции .savehub-*. Проверьте и сохраните их перед синхронизацией".into()));
        }
        if paths.is_empty() {
            return Ok(None);
        }
        git::stage(path, &paths).map_err(ServiceError::Adapter)?;
        git::commit(path, message)
            .map(Some)
            .map_err(ServiceError::Adapter)
    }
    pub fn checkpoint(&self, profile_id: &str, message: &str) -> ServiceResult<Option<CommitId>> {
        let _guard = self.lock()?;
        self.stopped()?;
        if message.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Пустое сообщение коммита".into(),
            ));
        }
        Self::commit_changes(&self.tree(&self.profile(profile_id)?)?, message)
    }
    pub fn has_unpushed_commits(&self, profile_id: &str) -> ServiceResult<bool> {
        let _guard = self.lock()?;
        let profile = self.profile(profile_id)?;
        let (root, _) = self.repository()?;
        git::has_unpushed_commits(&root, &profile.branch).map_err(ServiceError::Adapter)
    }
    /// Retrying publication never creates another checkpoint.
    pub fn push(&self, profile_id: &str) -> ServiceResult<()> {
        let _guard = self.lock()?;
        let profile = self.profile(profile_id)?;
        let path = self.tree(&profile)?;
        if !git::conflict_paths(&path)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            return Err(ServiceError::Conflict("Сначала разрешите конфликты".into()));
        }
        self.network(|root, token| {
            git::push(
                root,
                "origin",
                &format!("refs/heads/{0}:refs/heads/{0}", profile.branch),
                token,
            )
            .map_err(ServiceError::Adapter)
        })
    }
    fn conflicts_for(&self, profile: &Profile, path: &Path) -> ServiceResult<Vec<Conflict>> {
        let mut conflicts = Vec::new();
        for file in git::conflict_paths(path).map_err(ServiceError::Adapter)? {
            let game_id = file
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .find_map(|s| s.parse::<GameId>().ok())
                .filter(|id| self.catalog.games().contains_key(id))
                .ok_or_else(|| {
                    ServiceError::Conflict(format!(
                        "Конфликт вне каталога игры: {}",
                        file.display()
                    ))
                })?;
            let id = serde_json::to_string(&ConflictKey {
                profile: profile.id,
                path: file.clone(),
            })
            .map_err(|e| ServiceError::InvalidInput(e.to_string()))?;
            let (local_size, remote_size) =
                git::conflict_sizes(path, &file).map_err(ServiceError::Adapter)?;
            let conflict = Conflict {
                id,
                game_id,
                profile_id: profile.id,
                local: SaveVersion {
                    machine_id: self.settings.local().machine_id,
                    modified_at: 0,
                    size_bytes: local_size,
                },
                remote: SaveVersion {
                    machine_id: "remote".into(),
                    modified_at: 0,
                    size_bytes: remote_size,
                },
            };
            self.runtime.add_conflict(conflict.clone());
            conflicts.push(conflict);
        }
        Ok(conflicts)
    }
    pub fn list_conflicts(&self) -> ServiceResult<Vec<Conflict>> {
        let _guard = self.lock()?;
        if self.settings.data().local_repo_path.is_none() {
            return Ok(Vec::new());
        }
        let mut conflicts = Vec::new();
        for profile in self.catalog.profiles().into_values() {
            let path = self.tree(&profile)?;
            conflicts.extend(self.conflicts_for(&profile, &path)?);
        }
        self.runtime.replace_conflicts(conflicts.clone());
        Ok(conflicts)
    }
    pub fn resolve_conflict(
        &self,
        conflict_id: &str,
        resolution: ConflictResolution,
    ) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.stopped()?;
        let key: ConflictKey = serde_json::from_str(conflict_id)
            .map_err(|_| ServiceError::InvalidInput("Неверный ID конфликта".into()))?;
        let profile = self.profile(&key.profile.to_string())?;
        let path = self.tree(&profile)?;
        if !git::conflict_paths(&path)
            .map_err(ServiceError::Adapter)?
            .contains(&key.path)
        {
            return Err(ServiceError::NotFound("Конфликт не найден".into()));
        }
        git::backup_worktree(&path).map_err(ServiceError::Adapter)?;
        git::backup_conflict(&path, &key.path).map_err(ServiceError::Adapter)?;
        let remote_first = matches!(
            resolution,
            ConflictResolution::KeepRemote | ConflictResolution::KeepBothRemoteFirst
        );
        if let ConflictResolution::CreateProfile { name } = &resolution {
            if name.trim().is_empty() {
                return Err(ServiceError::InvalidInput("Пустое имя профиля".into()));
            }
            let id = uuid::Uuid::new_v4();
            let branch = format!("profiles/{id}");
            let (root, _) = self.repository()?;
            let remote = git::merge_head(&path).map_err(ServiceError::Adapter)?;
            git::create_branch(&root, &branch, &remote).map_err(ServiceError::Adapter)?;
            self.catalog.add_profile(Profile {
                id,
                name: name.trim().into(),
                branch,
                last_played_game: None,
            });
            // The new branch preserves the complete remote profile; the original keeps local files.
        }
        if remote_first {
            git::accept_theirs(&path, &key.path)
        } else {
            git::accept_ours(&path, &key.path)
        }
        .map_err(ServiceError::Adapter)?;
        if git::conflict_paths(&path)
            .map_err(ServiceError::Adapter)?
            .is_empty()
        {
            git::commit(&path, "Resolve save conflicts").map_err(ServiceError::Adapter)?;
        }
        self.runtime.replace_conflicts(
            self.runtime
                .conflicts()
                .into_iter()
                .filter(|c| c.id != conflict_id)
                .collect(),
        );
        Ok(())
    }
    fn lease(
        &self,
        root: &Path,
        profile: &Profile,
    ) -> ServiceResult<(String, Option<String>, Option<Lease>)> {
        let branch = format!("savehub/leases/{}", profile.id);
        let tip = git::remote_head(root, &branch).map_err(ServiceError::Adapter)?;
        let lease = tip
            .as_ref()
            .map(|id| {
                let bytes =
                    git::read_blob(root, id, "lease.json").map_err(ServiceError::Adapter)?;
                serde_json::from_slice(&bytes).map_err(|_| {
                    ServiceError::Conflict("Повреждён маркер занятости профиля".into())
                })
            })
            .transpose()?;
        Ok((branch, tip, lease))
    }
    pub fn is_profile_busy(&self, profile_id: &str) -> ServiceResult<bool> {
        let _guard = self.lock()?;
        let profile = self.profile(profile_id)?;
        self.network(|root, token| {
            git::fetch(root, "origin", token).map_err(ServiceError::Adapter)?;
            let (_, _, lease) = self.lease(root, &profile)?;
            Ok(lease.is_some_and(|l| {
                l.expires_at > now() && l.machine_id != self.settings.local().machine_id
            }))
        })
    }
    /// Use the fetched tip as parent so concurrent claims reject each other.
    /// Never force-update a profile lease.
    fn publish_lease(
        root: &Path,
        branch: &str,
        parent: Option<&str>,
        lease: &Lease,
        token: &str,
    ) -> ServiceResult<()> {
        let data =
            serde_json::to_vec(lease).map_err(|e| ServiceError::InvalidInput(e.to_string()))?;
        let id = git::commit_files(
            root,
            parent,
            &[("lease.json", &data)],
            "Update profile lease",
        )
        .map_err(ServiceError::Adapter)?;
        git::push(root, "origin", &format!("{id}:refs/heads/{branch}"), token)
            .map_err(ServiceError::Adapter)
    }
    fn update_lease(&self, profile_id: &str, release: bool) -> ServiceResult<()> {
        let profile = self.profile(profile_id)?;
        self.network(|root, token| self.update_lease_at(root, token, &profile, release))
    }

    fn update_lease_at(
        &self,
        root: &Path,
        token: &str,
        profile: &Profile,
        release: bool,
    ) -> ServiceResult<()> {
        git::fetch(root, "origin", token).map_err(ServiceError::Adapter)?;
        let (branch, tip, lease) = self.lease(root, profile)?;
        let machine_id = self.settings.local().machine_id;
        if machine_id.trim().is_empty() {
            return Err(ServiceError::InvalidInput("Не задан ID компьютера".into()));
        }
        if let Some(lease) = lease {
            if lease.machine_id != machine_id && (release || lease.expires_at > now()) {
                return Err(ServiceError::Conflict(
                    "Маркер принадлежит другому компьютеру".into(),
                ));
            }
        } else if release {
            return Ok(());
        }
        let lease = Lease {
            machine_id,
            expires_at: if release {
                0
            } else {
                now() + LEASE_DURATION_MS
            },
        };
        Self::publish_lease(root, &branch, tip.as_deref(), &lease, token)
    }
    pub fn renew_lease(&self, profile_id: &str) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.update_lease(profile_id, false)
    }
    pub fn release_lease(&self, profile_id: &str) -> ServiceResult<()> {
        let _guard = self.lock()?;
        self.update_lease(profile_id, true)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds_incrementing_suffix_for_duplicate_branches() {
        let branches = ["main", "skyrim", "skyrim-2"];

        assert_eq!(unique_branch("fallout", branches), "fallout");
        assert_eq!(unique_branch("skyrim", branches), "skyrim-3");
    }
    use super::*;
    use std::collections::HashMap;

    struct Fixture {
        dir: PathBuf,
        root: PathBuf,
        remote: PathBuf,
        service: RepoService,
        profile: Profile,
        game: Game,
    }
    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("savehub-repo-service-{}", uuid::Uuid::new_v4()));
            let root = dir.join("repo");
            let remote = dir.join("remote.git");
            let repository = git2::Repository::init(&root).unwrap();
            repository
                .config()
                .unwrap()
                .set_bool("core.autocrlf", false)
                .unwrap();
            repository
                .config()
                .unwrap()
                .set_str("user.name", "Repo test")
                .unwrap();
            repository
                .config()
                .unwrap()
                .set_str("user.email", "repo@localhost")
                .unwrap();
            git2::Repository::init_bare(&remote).unwrap();
            git::set_remote(&root, "origin", remote.to_str().unwrap()).unwrap();
            let profile = Profile {
                id: uuid::Uuid::new_v4(),
                name: "Test".into(),
                branch: "profiles/test".into(),
                last_played_game: None,
            };
            let game = Game {
                id: uuid::Uuid::new_v4(),
                name: "Game".into(),
                paths: GamePaths {
                    save_dir_path: "unused".into(),
                    exe_path: "unused".into(),
                },
                process_names: Vec::new(),
            };
            let catalog = Arc::new(Catalog::new(
                HashMap::from([(game.id, game.clone())]),
                HashMap::from([(profile.id, profile.clone())]),
            ));
            let runtime = Arc::new(RuntimeState::new(
                None,
                Vec::new(),
                SyncStatus {
                    state: SyncState::Idle,
                    last_sync_at: None,
                    error: None,
                },
            ));
            let settings = Arc::new(Settings::new(
                SettingsData {
                    catalog_repo_url: None,
                    ask_profile_before_launch: true,
                    repo_url: Some("https://github.com/test/saves.git".into()),
                    local_repo_path: Some(root.clone()),
                    sync_after_session: false,
                    sync_while_gaming: false,
                    auto_switch_profile: false,
                    start_minimized: false,
                },
                LocalConfig {
                    machine_id: "pc-one".into(),
                    games: HashMap::new(),
                },
            ));
            Self {
                dir,
                root,
                remote,
                service: RepoService::new(catalog, runtime, settings),
                profile,
                game,
            }
        }
        fn tree(&self) -> PathBuf {
            self.service.tree(&self.profile).unwrap()
        }
        fn save_path(&self) -> PathBuf {
            PathBuf::from(format!("games/{}/save.dat", self.game.id))
        }
        fn save(&self, value: &[u8]) {
            fs::write_atomic(&self.tree().join(self.save_path()), value).unwrap();
            self.service
                .checkpoint(&self.profile.id.to_string(), "save")
                .unwrap();
        }
        fn conflict(&self) -> Vec<Conflict> {
            self.service.runtime.set_running_games(Vec::new());
            self.save(b"original\n");
            let path = self.tree();
            let other = self.dir.join("other");
            git::create_worktree(
                &self.root,
                "other",
                &other,
                Some(&git::head(&path).unwrap()),
            )
            .unwrap();
            self.save(b"local\n");
            fs::write_atomic(&other.join(self.save_path()), b"remote\n").unwrap();
            RepoService::commit_changes(&other, "remote edit").unwrap();
            git::push(
                &self.root,
                "origin",
                &format!("refs/heads/other:refs/heads/{}", self.profile.branch),
                "",
            )
            .unwrap();
            git::fetch(&self.root, "origin", "").unwrap();
            self.service
                .apply_profile_updates(&self.profile.id.to_string())
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn worker_detects_unpublished_equal_behind_and_diverged_profile_refs() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        fixture.save(b"base");
        let id = fixture.profile.id.to_string();
        let base = git::head(&fixture.tree()).unwrap();
        assert!(
            RepoOperations::has_unpushed_commits(&fixture.service, &fixture.profile.id).unwrap()
        );
        let repository = git2::Repository::open(&fixture.root).unwrap();
        let remote_ref = format!("refs/remotes/origin/{}", fixture.profile.branch);
        repository
            .reference(
                &remote_ref,
                git2::Oid::from_str(&base).unwrap(),
                true,
                "test",
            )
            .unwrap();
        assert!(!fixture.service.has_unpushed_commits(&id).unwrap());
        let remote = git::commit_files(
            &fixture.root,
            Some(&base),
            &[("remote", b"remote")],
            "remote",
        )
        .unwrap();
        repository
            .reference(
                &remote_ref,
                git2::Oid::from_str(&remote).unwrap(),
                true,
                "test",
            )
            .unwrap();
        assert!(!fixture.service.has_unpushed_commits(&id).unwrap());
        fixture.save(b"local change");
        assert!(fixture.service.has_unpushed_commits(&id).unwrap());
        repository
            .reference(
                &remote_ref,
                git2::Oid::from_str(&base).unwrap(),
                true,
                "test",
            )
            .unwrap();
        assert!(fixture.service.has_unpushed_commits(&id).unwrap());
        assert_eq!(
            fs::read(&fixture.tree().join(fixture.save_path())).unwrap(),
            b"local change"
        );
    }

    #[test]
    fn ignored_legacy_binding_blocks_profile_deletion() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let target = fixture
            .tree()
            .join("games")
            .join(fixture.game.id.to_string());
        fs::create_dir_all(&target).unwrap();
        fs::write_atomic(&target.join("save"), b"progress").unwrap();
        let source = fixture.dir.join("live");
        fs::create_symlink(&source, &target).unwrap();
        let mut game = fixture.game.clone();
        game.paths.save_dir_path = source.to_string_lossy().into();
        fixture.service.catalog.update_game(game);
        fixture
            .service
            .settings
            .add_game_resolution(fixture.game.id, PathResolution::Ignore);
        assert!(matches!(
            fixture.service.delete_profile(&fixture.profile.id),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(fs::read(&source.join("save")).unwrap(), b"progress");
    }

    #[test]
    fn incomplete_save_staging_is_never_checkpointed() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        let tree = fixture.tree();
        fs::write_atomic(
            &tree.join("games/.savehub-import-test/partial"),
            b"incomplete",
        )
        .unwrap();
        assert!(matches!(
            fixture
                .service
                .checkpoint(&fixture.profile.id.to_string(), "save"),
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(
            fs::read(&tree.join("games/.savehub-import-test/partial")).unwrap(),
            b"incomplete"
        );
    }

    #[test]
    fn checkpoints_require_verified_stopped_games_and_are_idempotent() {
        let fixture = Fixture::new();
        let id = fixture.profile.id.to_string();
        assert!(matches!(
            fixture.service.checkpoint(&id, "save"),
            Err(ServiceError::ProcessesUnknown)
        ));
        fixture
            .service
            .runtime
            .set_running_games(vec![fixture.game.id]);
        assert!(matches!(
            fixture.service.checkout_profile(&id),
            Err(ServiceError::GameRunning(_))
        ));
        fixture.service.runtime.set_running_games(Vec::new());
        fixture
            .service
            .checkout_profile(&fixture.profile.branch)
            .unwrap();
        assert!(fixture.service.checkpoint(&id, "save").unwrap().is_none());
        fixture.save(b"binary\0\xff");
        let head = git::head(&fixture.tree()).unwrap();
        assert!(fixture.service.checkpoint(&id, "retry").unwrap().is_none());
        assert_eq!(git::head(&fixture.tree()).unwrap(), head);
        let reopened = RepoService::new(
            fixture.service.catalog.clone(),
            fixture.service.runtime.clone(),
            fixture.service.settings.clone(),
        );
        reopened.checkout_profile(&id).unwrap();
        assert_eq!(
            fs::read(&fixture.tree().join(fixture.save_path())).unwrap(),
            b"binary\0\xff"
        );
        assert!(!fixture.root.join(fixture.save_path()).exists());
    }

    #[test]
    fn conflicts_survive_restart_and_preserve_both_versions() {
        for remote_first in [false, true] {
            let fixture = Fixture::new();
            let conflicts = fixture.conflict();
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].game_id, fixture.game.id);
            assert_eq!(fixture.service.list_conflicts().unwrap().len(), 1);
            assert!(
                fixture
                    .service
                    .checkpoint(&fixture.profile.id.to_string(), "unresolved")
                    .is_err()
            );
            let resolution = if remote_first {
                ConflictResolution::KeepBothRemoteFirst
            } else {
                ConflictResolution::KeepBothLocalFirst
            };
            fixture
                .service
                .resolve_conflict(&conflicts[0].id, resolution)
                .unwrap();
            assert!(fixture.service.list_conflicts().unwrap().is_empty());
            assert_eq!(
                fs::read(&fixture.tree().join(fixture.save_path())).unwrap(),
                if remote_first {
                    b"remote\n".as_slice()
                } else {
                    b"local\n".as_slice()
                }
            );
            let repository = git2::Repository::open(fixture.tree()).unwrap();
            assert_eq!(
                repository
                    .head()
                    .unwrap()
                    .peel_to_commit()
                    .unwrap()
                    .parent_count(),
                2
            );
            let backups: Vec<_> =
                std::fs::read_dir(repository.path().join("savehub-conflict-versions"))
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .collect();
            assert!(
                backups
                    .iter()
                    .any(|p| fs::read(&p.join("local")).unwrap() == b"local\n"
                        && fs::read(&p.join("remote")).unwrap() == b"remote\n")
            );
        }
    }

    #[test]
    fn conflict_can_create_an_independent_remote_profile() {
        let fixture = Fixture::new();
        let conflicts = fixture.conflict();
        fixture
            .service
            .resolve_conflict(
                &conflicts[0].id,
                ConflictResolution::CreateProfile {
                    name: "Remote copy".into(),
                },
            )
            .unwrap();
        let profiles = fixture.service.catalog.profiles();
        let new = profiles
            .values()
            .find(|p| p.id != fixture.profile.id)
            .unwrap();
        assert_eq!(new.name, "Remote copy");
        let new_tree = fixture.service.tree(new).unwrap();
        assert_eq!(
            fs::read(&new_tree.join(fixture.save_path())).unwrap(),
            b"remote\n"
        );
        assert_eq!(
            fs::read(&fixture.tree().join(fixture.save_path())).unwrap(),
            b"local\n"
        );
    }

    #[test]
    fn catalog_sync_is_independent_and_can_publish_deletions() {
        let fixture = Fixture::new();
        fixture.service.runtime.set_running_games(Vec::new());
        fixture.save(b"save remains\0");
        let head = git::head(&fixture.tree()).unwrap();
        fixture.service.sync_catalog_at(&fixture.root, "").unwrap();
        assert_eq!(git::head(&fixture.tree()).unwrap(), head);
        assert_eq!(
            fs::read(&fixture.tree().join(fixture.save_path())).unwrap(),
            b"save remains\0"
        );
        fixture.service.catalog.remove_profile(&fixture.profile.id);
        fixture.service.sync_catalog_at(&fixture.root, "").unwrap();
        let remote = git2::Repository::open_bare(&fixture.remote).unwrap();
        let tree = remote
            .find_reference(&format!("refs/heads/{CATALOG_BRANCH}"))
            .unwrap()
            .peel_to_tree()
            .unwrap();
        let blob = remote
            .find_blob(tree.get_name("profiles.json").unwrap().id())
            .unwrap();
        let profiles: Vec<Profile> = serde_json::from_slice(blob.content()).unwrap();
        assert!(profiles.is_empty());
        assert!(tree.get_name("games.json").is_none());
        assert_eq!(
            fixture.service.catalog.games()[&fixture.game.id].name,
            fixture.game.name
        );
    }

    #[test]
    fn catalog_merge_preserves_remote_records_and_rejects_concurrent_edits() {
        let fixture = Fixture::new();
        let baseline = serde_json::to_value(vec![fixture.game.clone()]).unwrap();
        let mut renamed = fixture.game.clone();
        renamed.name = "New name".into();
        let mut remote = fixture.game.clone();
        remote.name = "Other name".into();
        assert!(merge_catalog_entries(Some(&baseline), &[renamed.clone()], &[remote]).is_err());
        let merged =
            merge_catalog_entries(Some(&baseline), &[renamed], &[fixture.game.clone()]).unwrap();
        assert_eq!(merged[0].name, "New name");
        let merged = merge_catalog_entries::<Game>(None, &[], &[fixture.game.clone()]).unwrap();
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn competing_lease_commits_cannot_overwrite_each_other() {
        let fixture = Fixture::new();
        let branch = format!("savehub/leases/{}", fixture.profile.id);
        RepoService::publish_lease(
            &fixture.root,
            &branch,
            None,
            &Lease {
                machine_id: "pc-one".into(),
                expires_at: now() + LEASE_DURATION_MS,
            },
            "",
        )
        .unwrap();
        git::fetch(&fixture.root, "origin", "").unwrap();
        let parent = git::remote_head(&fixture.root, &branch).unwrap().unwrap();
        RepoService::publish_lease(
            &fixture.root,
            &branch,
            Some(&parent),
            &Lease {
                machine_id: "pc-two".into(),
                expires_at: now() + LEASE_DURATION_MS,
            },
            "",
        )
        .unwrap();
        assert!(
            RepoService::publish_lease(
                &fixture.root,
                &branch,
                Some(&parent),
                &Lease {
                    machine_id: "pc-three".into(),
                    expires_at: now() + LEASE_DURATION_MS
                },
                ""
            )
            .is_err()
        );
        git::fetch(&fixture.root, "origin", "").unwrap();
        let (_, _, lease) = fixture
            .service
            .lease(&fixture.root, &fixture.profile)
            .unwrap();
        assert_eq!(lease.unwrap().machine_id, "pc-two");
    }

    #[test]
    fn lease_renew_release_and_foreign_owner_are_checked() {
        let fixture = Fixture::new();
        fixture
            .service
            .update_lease_at(&fixture.root, "", &fixture.profile, false)
            .unwrap();
        git::fetch(&fixture.root, "origin", "").unwrap();
        let (branch, tip, lease) = fixture
            .service
            .lease(&fixture.root, &fixture.profile)
            .unwrap();
        let lease = lease.unwrap();
        assert_eq!(lease.machine_id, "pc-one");
        assert!(lease.expires_at > now());
        fixture
            .service
            .update_lease_at(&fixture.root, "", &fixture.profile, true)
            .unwrap();
        git::fetch(&fixture.root, "origin", "").unwrap();
        let (_, _, lease) = fixture
            .service
            .lease(&fixture.root, &fixture.profile)
            .unwrap();
        assert_eq!(lease.unwrap().expires_at, 0);
        // A second machine's unexpired marker cannot be renewed or released.
        git::fetch(&fixture.root, "origin", "").unwrap();
        let tip = git::remote_head(&fixture.root, &branch).unwrap().or(tip);
        RepoService::publish_lease(
            &fixture.root,
            &branch,
            tip.as_deref(),
            &Lease {
                machine_id: "pc-two".into(),
                expires_at: now() + LEASE_DURATION_MS,
            },
            "",
        )
        .unwrap();
        assert!(matches!(
            fixture
                .service
                .update_lease_at(&fixture.root, "", &fixture.profile, false),
            Err(ServiceError::Conflict(_))
        ));
        assert!(matches!(
            fixture
                .service
                .update_lease_at(&fixture.root, "", &fixture.profile, true),
            Err(ServiceError::Conflict(_))
        ));
        git::fetch(&fixture.root, "origin", "").unwrap();
        let tip = git::remote_head(&fixture.root, &branch).unwrap();
        RepoService::publish_lease(
            &fixture.root,
            &branch,
            tip.as_deref(),
            &Lease {
                machine_id: "pc-two".into(),
                expires_at: 0,
            },
            "",
        )
        .unwrap();
        fixture
            .service
            .update_lease_at(&fixture.root, "", &fixture.profile, false)
            .unwrap();
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn unconfigured_status_and_bad_identifiers_return_errors() {
        let catalog = Arc::new(Catalog::new(HashMap::new(), HashMap::new()));
        let runtime = Arc::new(RuntimeState::new(
            None,
            Vec::new(),
            SyncStatus {
                state: SyncState::Idle,
                last_sync_at: None,
                error: None,
            },
        ));
        let settings = Arc::new(Settings::new(
            SettingsData {
                catalog_repo_url: None,
                ask_profile_before_launch: true,
                repo_url: None,
                local_repo_path: None,
                sync_after_session: false,
                sync_while_gaming: false,
                auto_switch_profile: false,
                start_minimized: false,
            },
            LocalConfig {
                machine_id: "test".into(),
                games: HashMap::new(),
            },
        ));
        let service = RepoService::new(catalog, runtime.clone(), settings.clone());
        assert!(matches!(
            service.status().unwrap().state,
            SyncState::NotConfigured
        ));
        assert!(matches!(
            service.repository(),
            Err(ServiceError::InvalidInput(_))
        ));
        runtime.set_running_games(Vec::new());
        assert!(matches!(
            service.checkout_profile("../other"),
            Err(ServiceError::NotFound(_))
        ));
        assert!(matches!(
            service.resolve_conflict("../secret", ConflictResolution::KeepLocal),
            Err(ServiceError::InvalidInput(_))
        ));
        settings.set_repository(
            Some("https://github.com.evil.test/owner/repo".into()),
            Some(std::env::temp_dir()),
        );
        assert!(matches!(
            service.repository(),
            Err(ServiceError::InvalidInput(_))
        ));
    }
}

/// Called under the shared operations mutex; do not lock it again or call UI code.
pub trait RepoOperations: Send + Sync + 'static {
    fn configuration(&self) -> Option<RepositoryConfig>;
    fn checkpoint(&self, profile: &ProfileId) -> ServiceResult<Option<CommitId>>;
    fn push(&self, profile: &ProfileId) -> ServiceResult<()>;
    fn fetch(&self) -> ServiceResult<()>;
    fn has_unpushed_commits(&self, profile: &ProfileId) -> ServiceResult<bool>;
    /// Discover durable unpublished branches, including inactive profiles after restart.
    fn unpushed_profiles(&self) -> ServiceResult<Vec<ProfileId>>;
}

impl RepoOperations for RepoService {
    fn unpushed_profiles(&self) -> ServiceResult<Vec<ProfileId>> {
        let _guard = self.lock()?;
        let (root, _) = self.repository()?;
        let branches = git::list_branches(&root).map_err(ServiceError::Adapter)?;
        let mut pending = Vec::new();
        for profile in self.catalog.profiles().values() {
            // Remote-only profiles have no local commit to publish. Discovery must
            // never create a branch/worktree as a side effect of reading its refs.
            if branches.contains(&profile.branch)
                && git::has_unpushed_commits(&root, &profile.branch)
                    .map_err(ServiceError::Adapter)?
            {
                pending.push(profile.id);
            }
        }
        pending.sort();
        Ok(pending)
    }

    fn configuration(&self) -> Option<RepositoryConfig> {
        self.configuration()
    }

    fn has_unpushed_commits(&self, profile: &ProfileId) -> ServiceResult<bool> {
        self.has_unpushed_commits(&profile.to_string())
    }

    fn checkpoint(&self, profile: &ProfileId) -> ServiceResult<Option<CommitId>> {
        self.checkpoint(&profile.to_string(), "SaveHub: automatic checkpoint")
    }

    fn push(&self, profile: &ProfileId) -> ServiceResult<()> {
        self.push(&profile.to_string())
    }

    fn fetch(&self) -> ServiceResult<()> {
        self.fetch()
    }
}
