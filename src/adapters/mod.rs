pub type AdapterResult<T> = Result<T, AdapterError>;

#[derive(Debug)]
pub enum AdapterError {
    Io(std::io::Error),
    InvalidData(String),
    FileLocked(std::path::PathBuf),
    Git(String),
    Credentials(String),
    UnsupportedPlatform,
}
pub mod autostart;
pub mod config;
pub mod fs;
pub mod git;
pub mod process;
pub mod watcher;
