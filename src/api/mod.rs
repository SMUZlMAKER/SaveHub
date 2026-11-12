//! Tauri-команды вызывают сервисы и сценарии; файловые операции выполняются вне UI-потока.
//
// Контракт с фронтом: каждая ручка — `#[tauri::command]`, ответы — DTO с `camelCase`-полями,
// ошибки — `ApiError`. Типы на стороне фронта: frontend/src/lib/types.ts (держать в синхроне).
// Нереализованные ручки возвращают `ApiError::NotImplemented`, без демонстрационных данных.
pub mod error;
pub mod events;
pub mod features;
pub mod games;
pub mod profiles;
pub mod saves;
pub mod settings;
pub mod sync;

pub use error::{ApiError, ApiResult};

pub mod updates;

pub mod launch;

pub mod manual_saves;

pub mod diagnostics;
