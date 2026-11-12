pub type ServiceResult<T> = Result<T, ServiceError>;

#[derive(Debug)]
pub enum ServiceError {
    NotFound(String),
    InvalidInput(String),
    GameRunning(Vec<crate::domain::GameId>),
    ProcessesUnknown,
    Conflict(String),
    RemoteUnavailable(String),
    Adapter(crate::adapters::AdapterError),
}
pub mod game;
pub mod process;
pub mod profile;
pub mod repo;
pub mod save;
pub mod settings;

mod bindings;

pub fn log_error(error: &crate::services::ServiceError) {
    use crate::services::ServiceError;
    let category = match error {
        ServiceError::NotFound(_) => "not_found",
        ServiceError::InvalidInput(_) => "invalid_input",
        ServiceError::Conflict(_) => "conflict",
        ServiceError::RemoteUnavailable(_) => "remote_unavailable",
        ServiceError::GameRunning(_) => "game_running",
        ServiceError::ProcessesUnknown => "processes_unknown",
        ServiceError::Adapter(_) => "adapter",
    };
    crate::adapters::diagnostics::event("service_error", category);
}
