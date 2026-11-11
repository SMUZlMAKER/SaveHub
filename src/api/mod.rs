// TODO: взаимодействие с tauri. Ручки для фронта, вызывают сервисы или workflow в зависимости от "сложности".
//
// Контракт с фронтом: каждая ручка — `#[tauri::command]`, ответы — DTO с `camelCase`-полями,
// ошибки — `ApiError`. Типы на стороне фронта: frontend/src/lib/types.ts (держать в синхроне).
// Нереализованные ручки возвращают `ApiError::NotImplemented`, без демонстрационных данных.
// Места, где нужно подключить сервисы и сценарии, помечены `TODO(backend)`.
pub mod error;
pub mod events;
pub mod features;
pub mod games;
pub mod profiles;
pub mod saves;
pub mod settings;
pub mod snapshots;
pub mod sync;

pub use error::{ApiError, ApiResult};

pub mod updates;
