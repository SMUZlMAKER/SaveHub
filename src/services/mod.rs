pub type ServiceResult<T> = Result<T, ServiceError>;

#[derive(Debug)]
pub enum ServiceError {
    NotFound(String),
    InvalidInput(String),
    GameRunning(Vec<crate::domain::GameId>),
    ProcessesUnknown,
    Conflict(String),
    Adapter(crate::adapters::AdapterError),
}
pub mod game;
pub mod process;
pub mod profile;
pub mod repo;
pub mod save;
pub mod settings;
pub mod updates;
