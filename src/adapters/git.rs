//! Git operations. Callers must stop games and back up saves before changing trees.
use super::{AdapterError, AdapterResult, fs};
use crate::domain::CommitId;
use git2::{BranchType, Cred, ErrorCode, Repository, RepositoryState};
use std::path::{Component, Path, PathBuf};

impl From<git2::Error> for AdapterError {
    fn from(error: git2::Error) -> Self {
        Self::Git(error.message().to_owned())
    }
}
impl From<std::io::Error> for AdapterError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    UpToDate,
    Merged(CommitId),
    Conflicts(Vec<PathBuf>),
    Diverged,
}

fn invalid(message: &str) -> AdapterError {
    AdapterError::InvalidData(message.into())
}
fn relative(path: &Path) -> AdapterResult<()> {
    if path.as_os_str().is_empty()
        || path.components().any(|c| match c {
            Component::Normal(s) => s.to_string_lossy().eq_ignore_ascii_case(".git"),
            _ => true,
        })
    {
        return Err(invalid("Expected a relative path outside .git"));
    }
    Ok(())
}

/// Use only the supplied HTTPS token; an empty token allows unauthenticated access.
fn callbacks(token: &str) -> git2::RemoteCallbacks<'_> {
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(move |_, username, allowed| {
        if allowed.contains(git2::CredentialType::USERNAME) {
            return Cred::username(username.unwrap_or("git"));
        }
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) && !token.is_empty() {
            return Cred::userpass_plaintext(username.unwrap_or("x-access-token"), token);
        }
        Err(git2::Error::from_str(
            "Authentication requires a supplied HTTPS token",
        ))
    });
    callbacks
}

pub fn clone(url: &str, path: &Path, token: &str) -> AdapterResult<()> {
    let mut options = git2::FetchOptions::new();
    options.remote_callbacks(callbacks(token));
    git2::build::RepoBuilder::new()
        .fetch_options(options)
        .clone(url, path)?;
    Ok(())
}

pub fn set_remote(repo: &Path, name: &str, url: &str) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    match repo.find_remote(name) {
        Ok(_) => repo.remote_set_url(name, url)?,
        Err(e) if e.code() == ErrorCode::NotFound => {
            repo.remote(name, url)?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub fn fetch(repo: &Path, remote: &str, token: &str) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    let mut options = git2::FetchOptions::new();
    options.remote_callbacks(callbacks(token));
    repo.find_remote(remote)?
        .fetch(&[] as &[&str], Some(&mut options), None)?;
    Ok(())
}

pub fn push(repo: &Path, remote: &str, refspec: &str, token: &str) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    let mut rejected = false;
    let mut callbacks = callbacks(token);
    callbacks.push_update_reference(|_, error| {
        rejected |= error.is_some();
        Ok(())
    });
    let mut options = git2::PushOptions::new();
    options.remote_callbacks(callbacks);
    repo.find_remote(remote)?
        .push(&[refspec], Some(&mut options))?;
    drop(options);
    if rejected {
        return Err(AdapterError::Git("Remote rejected push".into()));
    }
    Ok(())
}

pub fn list_branches(repo: &Path) -> AdapterResult<Vec<String>> {
    let repo = Repository::open(repo)?;
    let mut names = Vec::new();
    for branch in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch?;
        names.push(
            branch
                .name()?
                .ok_or_else(|| invalid("Non-UTF-8 branch name"))?
                .to_owned(),
        );
    }
    names.sort();
    Ok(names)
}

pub fn create_branch(repo: &Path, branch: &str, from: &str) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    repo.branch(
        branch,
        &repo.revparse_single(from)?.peel_to_commit()?,
        false,
    )?;
    Ok(())
}

/// Only merged local branches may be deleted; linked worktrees are protected by libgit2.
pub fn delete_branch(repo: &Path, branch: &str) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    let mut branch = repo.find_branch(branch, BranchType::Local)?;
    if branch.is_head() {
        return Err(invalid("Cannot delete the current branch"));
    }
    let tip = branch.get().peel_to_commit()?.id();
    let target = match branch.upstream() {
        Ok(upstream) => upstream.get().peel_to_commit()?.id(),
        Err(e) if e.code() == ErrorCode::NotFound => repo.head()?.peel_to_commit()?.id(),
        Err(e) => return Err(e.into()),
    };
    if tip != target && !repo.graph_descendant_of(target, tip)? {
        return Err(invalid("Branch contains unmerged commits"));
    }
    branch.delete()?;
    Ok(())
}

fn clean(repo: &Repository) -> AdapterResult<()> {
    if repo.state() != RepositoryState::Clean || !statuses(repo)?.is_empty() {
        return Err(invalid("Working tree must be clean"));
    }
    Ok(())
}

/// Reject dirty trees, even if the changed paths would not be overwritten.
pub fn checkout(repo: &Path, revision: &str) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    clean(&repo)?;
    let (object, reference) = repo.revparse_ext(revision)?;
    let commit = object.peel_to_commit()?;
    repo.checkout_tree(
        commit.as_object(),
        Some(git2::build::CheckoutBuilder::new().safe()),
    )?;
    match reference.filter(|r| r.is_branch()) {
        Some(reference) => repo.set_head(
            reference
                .name()
                .map_err(|_| invalid("Non-UTF-8 reference"))?,
        )?,
        None => repo.set_head_detached(commit.id())?,
    }
    Ok(())
}

/// None starts independent history with an empty root commit (no inherited saves).
pub fn create_worktree(
    repo: &Path,
    branch: &str,
    path: &Path,
    from: Option<&str>,
) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    let reference_name = format!("refs/heads/{branch}");
    if !git2::Reference::is_valid_name(&reference_name) {
        return Err(invalid("Invalid branch name"));
    }
    if fs::exists(path)? {
        return Err(invalid("Worktree destination already exists"));
    }
    let tip = match from {
        Some(revision) => repo.revparse_single(revision)?.peel_to_commit()?.id(),
        None => {
            let tree = repo.find_tree(repo.treebuilder(None)?.write()?)?;
            let signature = signature(&repo)?;
            repo.commit(
                None,
                &signature,
                &signature,
                "Initialize independent save history",
                &tree,
                &[],
            )?
        }
    };
    let mut branch = repo.branch(branch, &repo.find_commit(tip)?, false)?;
    let mut options = git2::WorktreeAddOptions::new();
    options.reference(Some(branch.get()));
    let name = format!("savehub-{}", uuid::Uuid::new_v4());
    let result = repo.worktree(&name, path, Some(&options));
    drop(options);
    if let Err(error) = result {
        // Never remove a partially created worktree or user files on failure.
        if repo.find_worktree(&name).is_err() {
            branch.delete()?;
        }
        return Err(error.into());
    }
    Ok(())
}

fn statuses(repo: &Repository) -> AdapterResult<git2::Statuses<'_>> {
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(false);
    Ok(repo.statuses(Some(&mut options))?)
}

pub fn status(repo: &Path) -> AdapterResult<Vec<PathBuf>> {
    let repo = Repository::open(repo)?;
    let mut paths = status_paths(&repo)?;
    paths.sort_unstable();
    paths.dedup();
    Ok(paths)
}

fn status_paths(repo: &Repository) -> AdapterResult<Vec<PathBuf>> {
    let statuses = statuses(repo)?;
    let mut paths = Vec::with_capacity(statuses.len());
    for entry in statuses.iter() {
        paths.push(PathBuf::from(
            entry.path().map_err(|_| invalid("Non-UTF-8 file path"))?,
        ));
    }
    Ok(paths)
}

/// Stage literal paths (directories recursively), including deleted files.
pub fn stage(repo: &Path, paths: &[PathBuf]) -> AdapterResult<()> {
    for path in paths {
        relative(path)?;
    }
    let repository = Repository::open(repo)?;
    let workdir = repository
        .workdir()
        .ok_or_else(|| invalid("No working tree"))?;
    let mut index = repository.index()?;
    let mut selected = status_paths(&repository)?;
    for entry in index.iter() {
        let path = PathBuf::from(
            std::str::from_utf8(&entry.path).map_err(|_| invalid("Non-UTF-8 file path"))?,
        );
        selected.push(path);
    }
    selected.extend_from_slice(paths);
    selected.sort_unstable();
    selected.dedup();
    for path in selected {
        if !paths.iter().any(|requested| path.starts_with(requested)) {
            continue;
        }
        let absolute = checked_path(workdir, &path)?;
        if absolute.is_dir() {
            continue;
        }
        if absolute.exists() || absolute.is_symlink() {
            index.add_path(&path)?;
        } else {
            match index.remove_path(&path) {
                Ok(()) => (),
                Err(e) if e.code() == ErrorCode::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
    }
    index.write()?;
    Ok(())
}

fn signature(repo: &Repository) -> AdapterResult<git2::Signature<'static>> {
    match repo.signature() {
        Ok(signature) => Ok(signature),
        Err(e) if e.code() == ErrorCode::NotFound => {
            Ok(git2::Signature::now("SaveHub", "savehub@localhost")?)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn commit(repo: &Path, message: &str) -> AdapterResult<CommitId> {
    let mut repo = Repository::open(repo)?;
    let mut merge_heads = Vec::new();
    if repo.state() == RepositoryState::Merge {
        repo.mergehead_foreach(|id| {
            merge_heads.push(*id);
            true
        })?;
    }
    if !matches!(
        repo.state(),
        RepositoryState::Clean | RepositoryState::Merge
    ) {
        return Err(invalid("Cannot commit during another Git operation"));
    }
    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Err(invalid("Resolve conflicts before committing"));
    }
    let tree = repo.find_tree(index.write_tree()?)?;
    let mut parents = Vec::new();
    match repo.head() {
        Ok(head) => parents.push(head.peel_to_commit()?),
        Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => (),
        Err(e) => return Err(e.into()),
    }
    for id in merge_heads {
        parents.push(repo.find_commit(id)?);
    }
    let signature = signature(&repo)?;
    let refs: Vec<_> = parents.iter().collect();
    let id = repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &refs)?;
    if repo.state() == RepositoryState::Merge {
        repo.cleanup_state()?;
    }
    Ok(id.to_string())
}

pub fn head(repo: &Path) -> AdapterResult<CommitId> {
    Ok(Repository::open(repo)?
        .head()?
        .peel_to_commit()?
        .id()
        .to_string())
}

pub fn merge(repo: &Path, revision: &str, fast_forward_only: bool) -> AdapterResult<MergeOutcome> {
    let repository = Repository::open(repo)?;
    let theirs = repository.revparse_single(revision)?.peel_to_commit()?;
    let annotated = repository.find_annotated_commit(theirs.id())?;
    let (analysis, _) = repository.merge_analysis(&[&annotated])?;
    if analysis.is_up_to_date() {
        return Ok(MergeOutcome::UpToDate);
    }
    if fast_forward_only && !analysis.is_fast_forward() && !analysis.is_unborn() {
        return Ok(MergeOutcome::Diverged);
    }
    clean(&repository)?;
    if analysis.is_fast_forward() || analysis.is_unborn() {
        repository.checkout_tree(
            theirs.as_object(),
            Some(git2::build::CheckoutBuilder::new().safe()),
        )?;
        if analysis.is_unborn() {
            let symbolic = repository.find_reference("HEAD")?;
            let name = symbolic
                .symbolic_target()
                .map_err(|_| invalid("Non-UTF-8 HEAD"))?
                .ok_or_else(|| invalid("HEAD is not symbolic"))?;
            repository.reference(name, theirs.id(), false, "SaveHub fast-forward")?;
        } else {
            repository
                .head()?
                .set_target(theirs.id(), "SaveHub fast-forward")?;
        }
        return Ok(MergeOutcome::Merged(theirs.id().to_string()));
    }
    repository.merge(
        &[&annotated],
        None,
        Some(
            git2::build::CheckoutBuilder::new()
                .safe()
                .allow_conflicts(true),
        ),
    )?;
    let index = repository.index()?;

    if index.has_conflicts() {
        let mut paths = Vec::new();
        for conflict in index.conflicts()? {
            let conflict = conflict?;
            for entry in [conflict.ancestor, conflict.our, conflict.their]
                .into_iter()
                .flatten()
            {
                paths.push(PathBuf::from(
                    std::str::from_utf8(&entry.path)
                        .map_err(|_| invalid("Non-UTF-8 conflict path"))?,
                ));
            }
        }
        paths.sort();
        paths.dedup();
        return Ok(MergeOutcome::Conflicts(paths));
    }
    Ok(MergeOutcome::Merged(commit(
        repo,
        "Merge synchronized saves",
    )?))
}

/// Discard merge changes; only permitted during a merge started from a clean tree.
pub fn abort_merge(repo: &Path) -> AdapterResult<()> {
    let repo = Repository::open(repo)?;
    if repo.state() != RepositoryState::Merge {
        return Err(invalid("No merge in progress"));
    }
    let object = repo.head()?.peel_to_commit()?;
    repo.reset(object.as_object(), git2::ResetType::Hard, None)?;
    repo.cleanup_state()?;
    Ok(())
}

// Reject linked paths inside the working tree.
fn checked_path(root: &Path, path: &Path) -> AdapterResult<PathBuf> {
    relative(path)?;
    let mut absolute = root.to_path_buf();
    for component in path.components() {
        absolute.push(component);
        if let Ok(metadata) = std::fs::symlink_metadata(&absolute) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(invalid("Reparse points are not allowed"));
                }
            }
            if metadata.is_symlink() {
                return Err(invalid("Symlinks are not allowed"));
            }
        }
    }
    Ok(absolute)
}

/// Export a file or subtree to a new destination. Existing destinations are never overwritten.
pub fn export_tree(
    repo: &Path,
    revision: &str,
    path: &Path,
    destination: &Path,
) -> AdapterResult<()> {
    relative(path)?;
    let repo = Repository::open(repo)?;
    let tree = repo.revparse_single(revision)?.peel_to_tree()?;
    let entry = tree.get_path(path)?;
    export_object(&repo, &entry.to_object(&repo)?, destination)
}

fn export_object(
    repo: &Repository,
    object: &git2::Object<'_>,
    destination: &Path,
) -> AdapterResult<()> {
    use std::io::Write;
    match object.kind() {
        Some(git2::ObjectType::Blob) => {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            file.write_all(
                object
                    .as_blob()
                    .ok_or_else(|| invalid("Expected blob"))?
                    .content(),
            )?;
            file.sync_all()?;
        }
        Some(git2::ObjectType::Tree) => {
            std::fs::create_dir(destination)?;
            for entry in object
                .as_tree()
                .ok_or_else(|| invalid("Expected tree"))?
                .iter()
            {
                let name = Path::new(entry.name().map_err(|_| invalid("Non-UTF-8 tree entry"))?);
                relative(name)?;
                export_object(repo, &entry.to_object(repo)?, &destination.join(name))?;
            }
        }
        _ => return Err(invalid("Cannot export submodules or non-file objects")),
    }
    Ok(())
}

/// Preserve the previous working copy in Git metadata before resolving the conflict.
pub fn resolve_file(repo: &Path, path: &Path, contents: &[u8]) -> AdapterResult<()> {
    relative(path)?;
    let repo = Repository::open(repo)?;
    let mut index = repo.index()?;
    index.conflict_get(path)?;
    resolve_contents(&repo, &mut index, path, Resolution::Contents(contents))
}

/// Accept stage 2 (our side) of one conflicted file, including deletion.
/// Callers must stop games first; commit separately after resolving all files.
pub fn accept_ours(repo: &Path, path: &Path) -> AdapterResult<()> {
    accept_side(repo, path, true)
}

/// Accept stage 3 (the incoming side) of one conflicted file, including deletion.
/// Callers must stop games first; commit separately after resolving all files.
pub fn accept_theirs(repo: &Path, path: &Path) -> AdapterResult<()> {
    accept_side(repo, path, false)
}

fn accept_side(repo: &Path, path: &Path, ours: bool) -> AdapterResult<()> {
    relative(path)?;
    let repository = Repository::open(repo)?;
    if repository.state() != RepositoryState::Merge {
        return Err(invalid("No merge in progress"));
    }
    let mut index = repository.index()?;
    let conflict = index.conflict_get(path)?;
    let selected = if ours { conflict.our } else { conflict.their };
    resolve_contents(&repository, &mut index, path, Resolution::Side(selected))
}

enum Resolution<'a> {
    Contents(&'a [u8]),
    Side(Option<git2::IndexEntry>),
}

fn resolve_contents(
    repo: &Repository,
    index: &mut git2::Index,
    path: &Path,
    resolution: Resolution<'_>,
) -> AdapterResult<()> {
    let root = repo.workdir().ok_or_else(|| invalid("No working tree"))?;
    let absolute = checked_path(root, path)?;
    let expected = path
        .to_str()
        .ok_or_else(|| invalid("Non-UTF-8 path"))?
        .replace('\\', "/");
    let blob = match &resolution {
        Resolution::Side(Some(entry)) => {
            if !matches!(entry.mode, 0o100644 | 0o100755) {
                return Err(invalid("Cannot accept a non-regular file"));
            }
            Some(repo.find_blob(entry.id)?)
        }
        _ => None,
    };
    let backup = repo
        .path()
        .join("savehub-conflict-backups")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&backup)?;
    if fs::exists(&absolute)? {
        std::fs::copy(&absolute, backup.join("working-copy"))?;
    }
    fs::write_atomic(&backup.join("path"), expected.as_bytes())?;
    match resolution {
        Resolution::Contents(contents) => {
            fs::write_atomic(&absolute, contents)?;
            index.add_path(path)?;
        }
        Resolution::Side(Some(mut entry)) => {
            let blob = blob.ok_or_else(|| invalid("Missing conflict blob"))?;
            fs::write_atomic(&absolute, blob.content())?;
            index.add_path(path)?;
            // Preserve the selected mode and object ID after clearing the conflict.
            entry.flags &= !0x3000;
            index.add(&entry)?;
        }
        Resolution::Side(None) => {
            match fs::remove_file(&absolute) {
                Ok(()) => (),
                Err(AdapterError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
            index.remove_path(path)?;
        }
    }
    index.write()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("savehub-git-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn repo(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            let repo = Repository::init(&path).unwrap();
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "SaveHub Test").unwrap();
            config.set_str("user.email", "test@localhost").unwrap();
            path
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn save(repo: &Path, path: &str, value: &[u8]) -> CommitId {
        let absolute = repo.join(path);
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(absolute, value).unwrap();
        stage(repo, &[path.into()]).unwrap();
        commit(repo, "checkpoint").unwrap()
    }
    fn initial(repo: &Path) -> CommitId {
        let id = save(repo, "save.dat", b"original\n");
        create_branch(repo, "base", &id).unwrap();
        checkout(repo, "base").unwrap();
        id
    }

    #[test]
    fn stage_deletions_literal_names_and_reject_dirty_checkout() {
        let sandbox = Sandbox::new();
        let path = sandbox.repo("repo");
        initial(&path);
        std::fs::write(path.join("save.dat"), b"dirty").unwrap();
        assert!(checkout(&path, "base").is_err());
        assert_eq!(std::fs::read(path.join("save.dat")).unwrap(), b"dirty");
        std::fs::remove_file(path.join("save.dat")).unwrap();
        std::fs::write(path.join("[slot].dat"), b"one").unwrap();
        std::fs::write(path.join("s.dat"), b"two").unwrap();
        stage(&path, &["save.dat".into(), "[slot].dat".into()]).unwrap();
        commit(&path, "delete and add literal filename").unwrap();
        assert_eq!(status(&path).unwrap(), vec![PathBuf::from("s.dat")]);
        let repo = Repository::open(&path).unwrap();
        let tree = repo.head().unwrap().peel_to_tree().unwrap();
        assert!(tree.get_name("save.dat").is_none());
        assert!(tree.get_name("[slot].dat").is_some());
        assert!(stage(&path, &["../outside".into()]).is_err());
    }

    fn conflicted_repo(sandbox: &Sandbox, ours: Option<&[u8]>, theirs: Option<&[u8]>) -> PathBuf {
        let path = sandbox.repo("repo");
        initial(&path);
        create_branch(&path, "other", "HEAD").unwrap();
        for (branch, contents) in [("base", ours), ("other", theirs)] {
            checkout(&path, branch).unwrap();
            match contents {
                Some(contents) => {
                    save(&path, "save.dat", contents);
                }
                None => {
                    std::fs::remove_file(path.join("save.dat")).unwrap();
                    stage(&path, &["save.dat".into()]).unwrap();
                    commit(&path, "delete save").unwrap();
                }
            }
        }
        checkout(&path, "base").unwrap();
        assert_eq!(
            merge(&path, "other", false).unwrap(),
            MergeOutcome::Conflicts(vec!["save.dat".into()])
        );
        path
    }

    fn assert_accepted(path: &Path, expected: Option<&[u8]>, before: &[u8]) {
        let repository = Repository::open(path).unwrap();
        assert!(!repository.index().unwrap().has_conflicts());
        assert_eq!(repository.state(), RepositoryState::Merge);
        let backups: Vec<_> = std::fs::read_dir(repository.path().join("savehub-conflict-backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read(backups[0].join("working-copy")).unwrap(),
            before
        );
        assert_eq!(std::fs::read(backups[0].join("path")).unwrap(), b"save.dat");
        let id = commit(path, "accept side").unwrap();
        let merged = repository
            .find_commit(git2::Oid::from_str(&id).unwrap())
            .unwrap();
        assert_eq!(merged.parent_count(), 2);
        let tree = merged.tree().unwrap();
        match expected {
            Some(contents) => {
                assert_eq!(std::fs::read(path.join("save.dat")).unwrap(), contents);
                let entry = tree.get_path(Path::new("save.dat")).unwrap();
                assert_eq!(
                    repository.find_blob(entry.id()).unwrap().content(),
                    contents
                );
            }
            None => {
                assert!(!path.join("save.dat").exists());
                assert!(tree.get_name("save.dat").is_none());
            }
        }
        assert_eq!(repository.state(), RepositoryState::Clean);
        assert!(status(path).unwrap().is_empty());
    }

    #[test]
    fn accept_ours_preserves_backup_and_commits_our_blob() {
        let sandbox = Sandbox::new();
        let path = conflicted_repo(&sandbox, Some(b"ours\0\n"), Some(b"theirs\0\n"));
        let before = std::fs::read(path.join("save.dat")).unwrap();
        accept_ours(&path, Path::new("save.dat")).unwrap();
        assert_accepted(&path, Some(b"ours\0\n"), &before);
    }

    #[test]
    fn accept_theirs_preserves_backup_and_commits_their_blob() {
        let sandbox = Sandbox::new();
        let path = conflicted_repo(&sandbox, Some(b"ours\n"), Some(b"theirs\n"));
        let before = std::fs::read(path.join("save.dat")).unwrap();
        accept_theirs(&path, Path::new("save.dat")).unwrap();
        assert_accepted(&path, Some(b"theirs\n"), &before);
    }

    #[test]
    fn accept_ours_can_choose_deletion() {
        let sandbox = Sandbox::new();
        let path = conflicted_repo(&sandbox, None, Some(b"theirs\n"));
        let before = std::fs::read(path.join("save.dat")).unwrap();
        accept_ours(&path, Path::new("save.dat")).unwrap();
        assert_accepted(&path, None, &before);
    }

    #[test]
    fn accept_theirs_can_choose_deletion() {
        let sandbox = Sandbox::new();
        let path = conflicted_repo(&sandbox, Some(b"ours\n"), None);
        let before = std::fs::read(path.join("save.dat")).unwrap();
        accept_theirs(&path, Path::new("save.dat")).unwrap();
        assert_accepted(&path, None, &before);
    }

    #[test]
    fn accept_side_rejects_nonconflicted_and_unsafe_paths_without_changes() {
        let sandbox = Sandbox::new();
        let path = sandbox.repo("repo");
        initial(&path);
        assert!(accept_ours(&path, Path::new("save.dat")).is_err());
        assert!(accept_theirs(&path, Path::new("save.dat")).is_err());
        let conflict_sandbox = Sandbox::new();
        let path = conflicted_repo(&conflict_sandbox, Some(b"ours\n"), Some(b"theirs\n"));
        let before = std::fs::read(path.join("save.dat")).unwrap();
        for accept in [accept_ours, accept_theirs] {
            for name in ["missing.dat", "../outside", ".git/config"] {
                assert!(accept(&path, Path::new(name)).is_err());
            }
        }
        assert_eq!(std::fs::read(path.join("save.dat")).unwrap(), before);
        let repository = Repository::open(&path).unwrap();
        assert!(repository.index().unwrap().has_conflicts());
        assert!(!repository.path().join("savehub-conflict-backups").exists());
    }

    #[test]
    fn fast_forward_and_divergence_preserve_working_tree() {
        let sandbox = Sandbox::new();
        let path = sandbox.repo("repo");
        initial(&path);
        create_branch(&path, "other", "HEAD").unwrap();
        checkout(&path, "other").unwrap();
        let other = save(&path, "remote.dat", b"remote");
        checkout(&path, "base").unwrap();
        assert_eq!(
            merge(&path, "other", true).unwrap(),
            MergeOutcome::Merged(other)
        );
        assert_eq!(merge(&path, "other", true).unwrap(), MergeOutcome::UpToDate);
        save(&path, "local.dat", b"local");
        checkout(&path, "other").unwrap();
        save(&path, "more.dat", b"more");
        checkout(&path, "base").unwrap();
        let before = head(&path).unwrap();
        assert_eq!(merge(&path, "other", true).unwrap(), MergeOutcome::Diverged);
        assert_eq!(head(&path).unwrap(), before);
        assert!(!path.join("more.dat").exists());
        assert!(matches!(
            merge(&path, "other", false).unwrap(),
            MergeOutcome::Merged(_)
        ));
        let repo = Repository::open(&path).unwrap();
        assert_eq!(
            repo.head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .parent_count(),
            2
        );
        assert_eq!(repo.state(), RepositoryState::Clean);
    }

    #[test]
    fn conflicts_can_be_exported_resolved_committed_or_aborted() {
        let sandbox = Sandbox::new();
        let path = sandbox.repo("repo");
        initial(&path);
        create_branch(&path, "other", "HEAD").unwrap();
        save(&path, "save.dat", b"ours\n");
        checkout(&path, "other").unwrap();
        save(&path, "save.dat", b"theirs\n");
        checkout(&path, "base").unwrap();
        assert_eq!(
            merge(&path, "other", false).unwrap(),
            MergeOutcome::Conflicts(vec!["save.dat".into()])
        );
        assert!(commit(&path, "unresolved").is_err());
        let exported = sandbox.0.join("theirs.dat");
        export_tree(&path, "other", Path::new("save.dat"), &exported).unwrap();
        assert_eq!(std::fs::read(&exported).unwrap(), b"theirs\n");
        assert!(export_tree(&path, "HEAD", Path::new("save.dat"), &exported).is_err());
        abort_merge(&path).unwrap();
        assert_eq!(std::fs::read(path.join("save.dat")).unwrap(), b"ours\n");
        assert!(abort_merge(&path).is_err());
        merge(&path, "other", false).unwrap();
        resolve_file(&path, Path::new("save.dat"), b"resolved\n").unwrap();
        let id = commit(&path, "resolved merge").unwrap();
        let repo = Repository::open(&path).unwrap();
        assert_eq!(
            repo.find_commit(git2::Oid::from_str(&id).unwrap())
                .unwrap()
                .parent_count(),
            2
        );
        assert_eq!(repo.state(), RepositoryState::Clean);
        assert!(status(&path).unwrap().is_empty());
        assert!(repo.path().join("savehub-conflict-backups").exists());
    }

    #[test]
    fn worktrees_share_objects_but_independent_history_has_no_saves() {
        let sandbox = Sandbox::new();
        let path = sandbox.repo("repo");
        initial(&path);
        let inherited = sandbox.0.join("inherited");
        create_worktree(&path, "profiles/inherited", &inherited, Some("HEAD")).unwrap();
        assert_eq!(
            std::fs::read(inherited.join("save.dat")).unwrap(),
            b"original\n"
        );
        let independent = sandbox.0.join("independent");
        create_worktree(&path, "profiles/independent", &independent, None).unwrap();
        assert!(!independent.join("save.dat").exists());
        let repo = Repository::open(&independent).unwrap();
        assert!(repo.is_worktree());
        assert_eq!(
            repo.head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .parent_count(),
            0
        );
        assert!(delete_branch(&path, "profiles/inherited").is_err());
    }

    #[test]
    fn deleting_unmerged_branch_is_rejected() {
        let sandbox = Sandbox::new();
        let path = sandbox.repo("repo");
        initial(&path);
        create_branch(&path, "other", "HEAD").unwrap();
        checkout(&path, "other").unwrap();
        save(&path, "other.dat", b"unique");
        checkout(&path, "base").unwrap();
        assert!(delete_branch(&path, "other").is_err());
        merge(&path, "other", true).unwrap();
        delete_branch(&path, "other").unwrap();
        assert!(!list_branches(&path).unwrap().contains(&"other".into()));
    }

    #[test]
    fn local_clone_fetch_push_and_remote_rejection() {
        let sandbox = Sandbox::new();
        let source = sandbox.repo("source");
        initial(&source);
        let bare = sandbox.0.join("bare.git");
        Repository::init_bare(&bare)
            .unwrap()
            .set_head("refs/heads/base")
            .unwrap();
        set_remote(&source, "origin", bare.to_str().unwrap()).unwrap();
        push(&source, "origin", "refs/heads/base:refs/heads/base", "").unwrap();
        let cloned = sandbox.0.join("clone");
        clone(bare.to_str().unwrap(), &cloned, "").unwrap();
        let latest = save(&source, "next.dat", b"next");
        push(&source, "origin", "refs/heads/base:refs/heads/base", "").unwrap();
        fetch(&cloned, "origin", "").unwrap();
        assert_eq!(
            merge(&cloned, "origin/base", true).unwrap(),
            MergeOutcome::Merged(latest)
        );
        save(&source, "source.dat", b"source");
        push(&source, "origin", "refs/heads/base:refs/heads/base", "").unwrap();
        save(&cloned, "clone.dat", b"clone");
        assert!(push(&cloned, "origin", "refs/heads/base:refs/heads/base", "").is_err());
        set_remote(&cloned, "origin", source.to_str().unwrap()).unwrap();
        assert_eq!(
            Repository::open(&cloned)
                .unwrap()
                .find_remote("origin")
                .unwrap()
                .url()
                .unwrap(),
            source.to_str().unwrap()
        );
    }
}
