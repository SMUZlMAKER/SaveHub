//! Filesystem operations. Callers must stop games and back up saves before replacing data.
use super::{AdapterError, AdapterResult};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

fn io_error(path: &Path, error: io::Error) -> AdapterError {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return AdapterError::FileLocked(path.to_owned());
    }
    let _ = path;
    AdapterError::Io(error)
}

fn invalid(message: &str) -> AdapterError {
    AdapterError::InvalidData(message.into())
}

fn metadata(path: &Path) -> AdapterResult<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, error)),
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.is_symlink()
}

fn require_missing(path: &Path) -> AdapterResult<()> {
    if metadata(path)?.is_some() {
        return Err(invalid("Destination already exists"));
    }
    Ok(())
}

/// Раскрыть Windows-переменные окружения на текущем ПК.
pub fn expand_path(path: &str) -> AdapterResult<PathBuf> {
    let variables: Vec<_> = std::env::vars_os().collect();
    let mut expanded = OsString::new();
    let mut remaining = path;
    while let Some(start) = remaining.find('%') {
        expanded.push(&remaining[..start]);
        let after = &remaining[start + 1..];
        let Some(end) = after.find('%') else {
            expanded.push(&remaining[start..]);
            return Ok(PathBuf::from(expanded));
        };
        let name = &after[..end];
        match variables.iter().find(|(key, _)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case(name))
        }) {
            Some((_, value)) => expanded.push(value),
            None => expanded.push(&remaining[start..start + end + 2]),
        }
        remaining = &after[end + 1..];
    }
    expanded.push(remaining);
    Ok(PathBuf::from(expanded))
}

pub fn exists(path: &Path) -> AdapterResult<bool> {
    path.try_exists().map_err(|error| io_error(path, error))
}

pub fn is_directory(path: &Path) -> AdapterResult<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(path, error)),
    }
}

pub fn create_dir_all(path: &Path) -> AdapterResult<()> {
    fs::create_dir_all(path).map_err(|error| io_error(path, error))
}

pub fn read(path: &Path) -> AdapterResult<Vec<u8>> {
    fs::read(path).map_err(|error| io_error(path, error))
}

/// Write through directory junctions using a temporary file beside the destination.
/// A link at the file itself is rejected. On failure, the previous file is preserved.
pub fn write_atomic(path: &Path, contents: &[u8]) -> AdapterResult<()> {
    if let Some(metadata) = metadata(path)? {
        if is_link(&metadata) || !metadata.is_file() {
            return Err(invalid("Destination must be a regular file, not a link"));
        }
        // Detect sharing violations before MoveFileEx can report a generic access error.
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|error| io_error(path, error))?;
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    create_dir_all(parent)?;
    let temporary = parent.join(format!(".savehub-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| io_error(&temporary, error))?;
    let result = (|| {
        file.write_all(contents)
            .map_err(|error| io_error(path, error))?;
        file.sync_all().map_err(|error| io_error(path, error))?;
        drop(file);
        replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> AdapterResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    // Canonicalization provides the extended Windows path prefix required by
    // MoveFileExW for long paths. Resolve only the destination parent: the leaf
    // may not exist yet and must never be followed as a symlink.
    let source_path = fs::canonicalize(source).map_err(|error| io_error(source, error))?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = destination
        .file_name()
        .ok_or_else(|| invalid("Missing destination name"))?;
    let destination_path = fs::canonicalize(parent)
        .map_err(|error| io_error(destination, error))?
        .join(name);
    let source: Vec<u16> = source_path.as_os_str().encode_wide().chain([0]).collect();
    let destination_wide: Vec<u16> = destination_path
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io_error(destination, io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> AdapterResult<()> {
    fs::rename(source, destination).map_err(|error| io_error(destination, error))
}

/// Copy to a new directory; its parent must exist. Never overwrite existing data.
/// An explicitly supplied junction root is followed; nested links are rejected.
/// A failed copy is left in place so callers can inspect the incomplete backup.
pub fn copy_tree(source: &Path, destination: &Path) -> AdapterResult<()> {
    let source = fs::canonicalize(source).map_err(|error| io_error(source, error))?;
    if !is_directory(&source)? {
        return Err(invalid("Source must be a directory"));
    }
    require_missing(destination)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| io_error(parent, error))?;
    let name = destination
        .file_name()
        .ok_or_else(|| invalid("Missing destination name"))?;
    if parent.join(name).starts_with(&source) {
        return Err(invalid("Cannot copy a directory into itself"));
    }
    let mut entries = Vec::new();
    collect_tree(&source, &mut entries)?;
    fs::create_dir(destination).map_err(|error| io_error(destination, error))?;
    for (path, metadata) in entries {
        let relative = path
            .strip_prefix(&source)
            .map_err(|_| invalid("Path outside source"))?;
        let target = destination.join(relative);
        // Recheck after preflight so a changed entry cannot silently become a link.
        if is_link(&fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?) {
            return Err(invalid("Nested links are not allowed in a copied tree"));
        }
        if metadata.is_dir() {
            fs::create_dir(&target).map_err(|error| io_error(&target, error))?;
        } else {
            let mut input = fs::File::open(&path).map_err(|error| io_error(&path, error))?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|error| io_error(&target, error))?;
            io::copy(&mut input, &mut output).map_err(|error| io_error(&path, error))?;
            output
                .sync_all()
                .map_err(|error| io_error(&target, error))?;
        }
    }
    Ok(())
}

pub fn remove_file(path: &Path) -> AdapterResult<()> {
    match metadata(path)? {
        Some(metadata) if is_link(&metadata) => remove_symlink(path),
        _ => fs::remove_file(path).map_err(|error| io_error(path, error)),
    }
}

pub fn rename(source: &Path, destination: &Path) -> AdapterResult<()> {
    require_missing(destination)?;
    fs::rename(source, destination).map_err(|error| io_error(source, error))
}

/// Удалить обычное дерево; не следовать ссылкам.
pub fn remove_tree(path: &Path) -> AdapterResult<()> {
    let metadata = metadata(path)?.ok_or_else(|| {
        io_error(
            path,
            io::Error::new(io::ErrorKind::NotFound, "Directory does not exist"),
        )
    })?;
    if is_link(&metadata) {
        return remove_symlink(path);
    }
    if !metadata.is_dir() {
        return Err(invalid("Expected a directory"));
    }
    // std removes directory links encountered during traversal without following them.
    fs::remove_dir_all(path).map_err(|error| io_error(path, error))
}

/// File contents size in bytes. Follow an explicit root junction, reject nested links.
pub fn size(path: &Path) -> AdapterResult<u64> {
    let root = fs::canonicalize(path).map_err(|error| io_error(path, error))?;
    let metadata = fs::metadata(&root).map_err(|error| io_error(path, error))?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut entries = Vec::new();
    collect_tree(&root, &mut entries)?;
    entries
        .iter()
        .filter(|(_, metadata)| metadata.is_file())
        .try_fold(0u64, |size, (_, metadata)| {
            size.checked_add(metadata.len())
                .ok_or_else(|| invalid("Tree size overflow"))
        })
}

fn collect_tree(root: &Path, entries: &mut Vec<(PathBuf, fs::Metadata)>) -> AdapterResult<()> {
    for entry in fs::read_dir(root).map_err(|error| io_error(root, error))? {
        let entry = entry.map_err(|error| io_error(root, error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if is_link(&metadata) || !(metadata.is_file() || metadata.is_dir()) {
            return Err(invalid("Nested links and special files are not allowed"));
        }
        let directory = metadata.is_dir();
        entries.push((path.clone(), metadata));
        if directory {
            collect_tree(&path, entries)?;
        }
    }
    Ok(())
}

/// Create a directory junction on Windows (a directory symlink on Unix).
/// The target must exist; an existing link or ordinary directory is never replaced.
pub fn create_symlink(link: &Path, target: &Path) -> AdapterResult<()> {
    require_missing(link)?;
    let target = fs::canonicalize(target).map_err(|error| io_error(target, error))?;
    if !is_directory(&target)? {
        return Err(invalid("Link target must be a directory"));
    }
    #[cfg(windows)]
    return junction::create(&target, link).map_err(|error| io_error(link, error));
    #[cfg(unix)]
    return std::os::unix::fs::symlink(&target, link).map_err(|error| io_error(link, error));
    #[cfg(not(any(windows, unix)))]
    Err(AdapterError::UnsupportedPlatform)
}

/// Read a junction/symlink target, including dangling links; None for missing/ordinary paths.
pub fn read_symlink(link: &Path) -> AdapterResult<Option<PathBuf>> {
    let Some(metadata) = metadata(link)? else {
        return Ok(None);
    };
    if !is_link(&metadata) {
        return Ok(None);
    }
    fs::read_link(link)
        .map(Some)
        .map_err(|error| io_error(link, error))
}

/// Удалить только ссылку, сохранив целевой каталог.
pub fn remove_symlink(link: &Path) -> AdapterResult<()> {
    let metadata = metadata(link)?.ok_or_else(|| {
        io_error(
            link,
            io::Error::new(io::ErrorKind::NotFound, "Link does not exist"),
        )
    })?;
    if !is_link(&metadata) {
        return Err(invalid("Refusing to remove an ordinary path as a link"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Validate the reparse point even when its target no longer exists.
        fs::read_link(link).map_err(|error| io_error(link, error))?;
        // RemoveDirectory removes the junction itself; junction::delete only clears its tag.
        if metadata.file_attributes() & 0x10 != 0 {
            return fs::remove_dir(link).map_err(|error| io_error(link, error));
        }
    }
    fs::remove_file(link).map_err(|error| io_error(link, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("savehub-fs-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn expand_known_variables_once_and_preserve_unknown_variables() {
        let (name, value) = std::env::vars_os()
            .find(|(name, _)| {
                name.to_str()
                    .is_some_and(|s| !s.is_empty() && !s.contains(['%', '=']))
            })
            .expect("at least one environment variable");
        let name = name.to_str().unwrap().to_ascii_lowercase();
        let mut expected = value;
        expected.push("/saves");
        assert_eq!(
            expand_path(&format!("%{name}%/saves")).unwrap(),
            PathBuf::from(expected)
        );
        let unknown = format!("%SAVEHUB_UNDEFINED_{}%/saves%", uuid::Uuid::new_v4());
        assert_eq!(expand_path(&unknown).unwrap(), PathBuf::from(unknown));
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_supports_paths_longer_than_windows_max_path() {
        use std::os::windows::ffi::OsStrExt;
        let sandbox = Sandbox::new();
        let mut directory = sandbox.path("long-path");
        while directory.as_os_str().encode_wide().count() < 280 {
            directory.push("nested-save-directory");
        }
        let path = directory.join("save.dat");
        assert!(path.as_os_str().encode_wide().count() > 260);
        write_atomic(&path, b"original\0").unwrap();
        write_atomic(&path, b"replacement\0").unwrap();
        assert_eq!(read(&path).unwrap(), b"replacement\0");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    }

    #[test]
    fn atomic_write_creates_parents_and_replaces_contents_without_temporary_files() {
        let sandbox = Sandbox::new();
        let path = sandbox.path("nested/save.dat");
        assert!(!exists(&path).unwrap());
        assert!(!is_directory(&path).unwrap());
        write_atomic(&path, b"first\0").unwrap();
        write_atomic(&path, b"second\0").unwrap();
        assert_eq!(read(&path).unwrap(), b"second\0");
        assert!(exists(&path).unwrap());
        assert!(is_directory(path.parent().unwrap()).unwrap());
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        write_atomic(&path, b"").unwrap();
        assert_eq!(size(&path).unwrap(), 0);
    }

    #[test]
    fn copy_tree_preserves_binary_files_empty_directories_and_source() {
        let sandbox = Sandbox::new();
        let source = sandbox.path("source");
        create_dir_all(&source.join("nested/empty")).unwrap();
        write_atomic(&source.join("one"), b"abc").unwrap();
        write_atomic(&source.join("nested/two"), &[0, 255]).unwrap();
        let destination = sandbox.path("backup");
        copy_tree(&source, &destination).unwrap();
        assert_eq!(read(&destination.join("one")).unwrap(), b"abc");
        assert_eq!(read(&destination.join("nested/two")).unwrap(), [0, 255]);
        assert!(is_directory(&destination.join("nested/empty")).unwrap());
        assert_eq!(size(&source).unwrap(), 5);
        assert_eq!(size(&destination).unwrap(), 5);
        assert!(copy_tree(&source, &destination).is_err());
        assert!(copy_tree(&source, &source.join("recursive")).is_err());
        assert!(!exists(&source.join("recursive")).unwrap());
        assert_eq!(read(&destination.join("one")).unwrap(), b"abc");
    }

    #[test]
    fn rename_and_delete_preserve_existing_destination_and_unrelated_files() {
        let sandbox = Sandbox::new();
        let source = sandbox.path("source");
        let destination = sandbox.path("destination");
        write_atomic(&source, b"source").unwrap();
        write_atomic(&destination, b"destination").unwrap();
        assert!(rename(&source, &destination).is_err());
        assert_eq!(read(&source).unwrap(), b"source");
        assert_eq!(read(&destination).unwrap(), b"destination");
        remove_file(&destination).unwrap();
        rename(&source, &destination).unwrap();
        assert!(!exists(&source).unwrap());
        assert_eq!(read(&destination).unwrap(), b"source");
        let tree = sandbox.path("tree");
        write_atomic(&tree.join("nested/file"), b"data").unwrap();
        remove_tree(&tree).unwrap();
        assert!(!exists(&tree).unwrap());
        assert!(remove_tree(&destination).is_err());
        assert_eq!(read(&destination).unwrap(), b"source");
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn junction_writes_and_root_backups_use_external_saves() {
        let sandbox = Sandbox::new();
        let saves = sandbox.path("game-saves");
        create_dir_all(&saves).unwrap();
        let link = sandbox.path("linked-saves");
        create_symlink(&link, &saves).unwrap();
        #[cfg(windows)]
        assert!(junction::exists(&link).unwrap());
        assert_eq!(
            fs::canonicalize(read_symlink(&link).unwrap().unwrap()).unwrap(),
            fs::canonicalize(&saves).unwrap()
        );
        write_atomic(&link.join("save.dat"), b"first").unwrap();
        write_atomic(&link.join("save.dat"), b"updated").unwrap();
        assert_eq!(read(&saves.join("save.dat")).unwrap(), b"updated");
        assert_eq!(size(&link).unwrap(), 7);
        copy_tree(&link, &sandbox.path("backup")).unwrap();
        assert_eq!(read(&sandbox.path("backup/save.dat")).unwrap(), b"updated");
        assert!(create_symlink(&link, &saves).is_err());
        remove_symlink(&link).unwrap();
        assert!(metadata(&link).unwrap().is_none());
        assert_eq!(read(&saves.join("save.dat")).unwrap(), b"updated");
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn remove_tree_never_follows_root_or_nested_junctions() {
        let sandbox = Sandbox::new();
        let saves = sandbox.path("saves");
        write_atomic(&saves.join("keep.dat"), b"keep").unwrap();
        let tree = sandbox.path("tree");
        create_dir_all(&tree).unwrap();
        create_symlink(&tree.join("junction"), &saves).unwrap();
        assert!(size(&tree).is_err());
        let backup = sandbox.path("backup");
        assert!(copy_tree(&tree, &backup).is_err());
        assert!(!exists(&backup).unwrap());
        remove_tree(&tree).unwrap();
        assert_eq!(read(&saves.join("keep.dat")).unwrap(), b"keep");
        let link = sandbox.path("root-junction");
        create_symlink(&link, &saves).unwrap();
        remove_tree(&link).unwrap();
        assert!(metadata(&link).unwrap().is_none());
        assert_eq!(read(&saves.join("keep.dat")).unwrap(), b"keep");
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn ordinary_paths_are_not_removed_as_links_and_dangling_links_can_be_removed() {
        let sandbox = Sandbox::new();
        let saves = sandbox.path("saves");
        create_dir_all(&saves).unwrap();
        assert!(read_symlink(&saves).unwrap().is_none());
        assert!(remove_symlink(&saves).is_err());
        assert!(create_symlink(&saves, &saves).is_err());
        assert!(is_directory(&saves).unwrap());
        let link = sandbox.path("link");
        create_symlink(&link, &saves).unwrap();
        fs::remove_dir(&saves).unwrap();
        assert!(!exists(&link).unwrap());
        assert!(read_symlink(&link).unwrap().is_some());
        assert!(create_symlink(&link, &sandbox.0).is_err());
        remove_symlink(&link).unwrap();
        assert!(metadata(&link).unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn locked_save_is_reported_and_failed_atomic_write_preserves_original() {
        use std::os::windows::fs::OpenOptionsExt;
        let sandbox = Sandbox::new();
        let saves = sandbox.path("saves");
        let file = saves.join("save.dat");
        write_atomic(&file, b"original").unwrap();
        let canonical_file = fs::canonicalize(&file).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&file)
            .unwrap();
        assert!(matches!(read(&file), Err(AdapterError::FileLocked(path)) if path == file));
        assert!(
            matches!(write_atomic(&file, b"replacement"), Err(AdapterError::FileLocked(path)) if path == file)
        );
        assert!(
            matches!(copy_tree(&saves, &sandbox.path("backup")), Err(AdapterError::FileLocked(path)) if path == canonical_file)
        );
        assert_eq!(fs::read_dir(&saves).unwrap().count(), 1);
        drop(lock);
        assert_eq!(read(&file).unwrap(), b"original");
    }
}
