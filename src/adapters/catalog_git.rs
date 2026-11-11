//! Dedicated catalogue repository: one branch and one tracked configuration file.
use super::{AdapterError, AdapterResult, git};
use git2::{ErrorCode, Repository, RepositoryState};
use std::path::Path;

pub const FILE: &str = "games.json";
pub const BRANCH: &str = "main";
pub const REMOTE_REF: &str = "refs/remotes/origin/main";

fn invalid(message: &str) -> AdapterError {
    AdapterError::InvalidData(message.into())
}

pub fn open(path: &Path) -> AdapterResult<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    let repo = if path.join(".git").exists() {
        Repository::open(path)?
    } else {
        if std::fs::read_dir(path)?.next().is_some() {
            return Err(invalid("Каталог репозитория не пуст"));
        }
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head(BRANCH);
        Repository::init_opts(path, &options)?
    };
    if repo.is_worktree() || repo.is_bare() || repo.state() != RepositoryState::Clean {
        return Err(invalid(
            "Нужен отдельный репозиторий каталога без незавершённых Git-операций",
        ));
    }
    if repo.find_reference("HEAD")?.symbolic_target()? != Some("refs/heads/main") {
        return Err(invalid("Каталог должен находиться в ветке main"));
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path.join(FILE)) {
        if !metadata.is_file() {
            return Err(invalid("games.json должен быть обычным файлом"));
        }
    }
    Ok(())
}

/// Returns true for a provisional first connection; discard it if validation fails.
pub fn connect(path: &Path, url: &str) -> AdapterResult<bool> {
    let repo = Repository::open(path)?;
    match repo.find_remote("origin") {
        Ok(remote) if remote.url()? != url => {
            return Err(invalid(
                "Этот каталог уже подключён к другому репозиторию. Верните прежний адрес; автоматическая замена репозитория не поддерживается",
            ));
        }
        Ok(_) => Ok(false),
        Err(e) if e.code() == ErrorCode::NotFound => {
            git::set_remote(path, "origin", url)?;
            Ok(true)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn discard_connection(path: &Path) -> AdapterResult<()> {
    Repository::open(path)?.remote_delete("origin")?;
    Ok(())
}

/// None means an unborn branch / empty remote, never a corrupt Git object.
pub fn file_at(path: &Path, revision: &str) -> AdapterResult<Option<Vec<u8>>> {
    let repo = Repository::open(path)?;
    let object = match repo.revparse_single(revision) {
        Ok(object) => object,
        Err(e) if matches!(e.code(), ErrorCode::NotFound | ErrorCode::UnbornBranch) => {
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let tree = object.peel_to_tree()?;
    if tree.len() != 1 {
        return Err(invalid(
            "Репозиторий каталога должен содержать только games.json",
        ));
    }
    let entry = tree
        .get_name(FILE)
        .ok_or_else(|| invalid("В репозитории отсутствует games.json"))?;
    if entry.filemode() != 0o100644 {
        return Err(invalid("games.json должен быть обычным JSON-файлом"));
    }
    Ok(Some(repo.find_blob(entry.id())?.content().to_vec()))
}

pub fn checkpoint(path: &Path) -> AdapterResult<()> {
    open(path)?;
    let dirty = git::status(path)?;
    if dirty.iter().any(|p| p != Path::new(FILE)) {
        return Err(invalid(
            "В репозитории каталога обнаружены посторонние изменения",
        ));
    }
    if !dirty.is_empty() {
        git::stage(path, &[FILE.into()])?;
        git::commit(path, "Update game catalogue")?;
    }
    Ok(())
}

pub fn remote_is_empty(path: &Path) -> AdapterResult<bool> {
    let repo = Repository::open(path)?;
    if repo.find_reference(REMOTE_REF).is_ok() {
        return Ok(false);
    }
    if repo
        .branches(Some(git2::BranchType::Remote))?
        .next()
        .is_some()
    {
        return Err(invalid("В удалённом каталоге нет ветки main"));
    }
    Ok(true)
}
