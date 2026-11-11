//! Shared safeguards for metadata changes that would abandon a live save binding.
use super::{ServiceError, ServiceResult};
use crate::{
    adapters::{fs, git},
    domain::*,
};
use std::{collections::HashMap, path::PathBuf};

fn paths<'a>(game: &'a Game, local: &'a LocalConfig) -> Option<&'a GamePaths> {
    match local.games.get(&game.id) {
        Some(PathResolution::Ignore) => None,
        Some(PathResolution::UseLocal(p) | PathResolution::UpdateRepository(p)) => Some(p),
        None => Some(&game.paths),
    }
}

/// Include shared paths as well as overrides, so legacy ignored bindings are not forgotten.
pub(super) fn linked_paths(game: &Game, local: &LocalConfig) -> ServiceResult<Vec<PathBuf>> {
    let mut candidates = vec![&game.paths.save_dir_path];
    if let Some(PathResolution::UseLocal(p) | PathResolution::UpdateRepository(p)) =
        local.games.get(&game.id)
    {
        candidates.push(&p.save_dir_path);
    }
    let mut result = Vec::new();
    for path in candidates {
        let path = fs::expand_path(path).map_err(ServiceError::Adapter)?;
        if !result.contains(&path)
            && fs::read_symlink(&path)
                .map_err(ServiceError::Adapter)?
                .is_some()
        {
            result.push(path);
        }
    }
    Ok(result)
}

pub(super) fn changing_bindings(
    catalog: &Catalog,
    current: &LocalConfig,
    games: &HashMap<GameId, Game>,
    local: &LocalConfig,
) -> ServiceResult<Vec<(GameId, PathBuf)>> {
    let mut affected = Vec::new();
    for game in catalog.games().values() {
        let next = games.get(&game.id);
        if next.is_some_and(|next| {
            paths(game, current) == paths(next, local) && game.process_names == next.process_names
        }) {
            continue;
        }
        for source in linked_paths(game, current)? {
            affected.push((game.id, source));
        }
    }
    Ok(affected)
}

/// Detach only links owned by this game/profile repository; keep target and original data.
pub(super) fn detach(
    catalog: &Catalog,
    runtime: &RuntimeState,
    settings: &Settings,
    affected: &[(GameId, PathBuf)],
) -> ServiceResult<()> {
    if affected.is_empty() {
        return Ok(());
    }
    let running = runtime
        .running_games()
        .ok_or(ServiceError::ProcessesUnknown)?;
    if !running.is_empty() {
        return Err(ServiceError::GameRunning(running));
    }
    let root = settings.data().local_repo_path.ok_or_else(|| {
        ServiceError::Conflict("Не настроен репозиторий для отключения сохранений".into())
    })?;
    // Validate every link before modifying any of them.
    for (game, source) in affected {
        let actual = fs::canonicalize(source).map_err(ServiceError::Adapter)?;
        let mut owned = false;
        for profile in catalog.profiles().values() {
            if let Some(tree) =
                git::existing_worktree(&root, &profile.branch, &profile.id.to_string())
                    .map_err(ServiceError::Adapter)?
            {
                let target = tree.join("games").join(game.to_string());
                fs::require_plain_path(&tree, &target).map_err(ServiceError::Adapter)?;
                if fs::exists(&target).map_err(ServiceError::Adapter)?
                    && fs::canonicalize(&target).map_err(ServiceError::Adapter)? == actual
                {
                    owned = true;
                    break;
                }
            }
        }
        if !owned {
            return Err(ServiceError::Conflict(format!(
                "Посторонняя ссылка сохранений: {}. Изменение отменено",
                source.display()
            )));
        }
    }
    for (_, source) in affected {
        fs::detach_directory_link(source).map_err(ServiceError::Adapter)?;
    }
    Ok(())
}
