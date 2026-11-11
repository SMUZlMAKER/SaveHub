// TODO: взаимодействие с tauri. Ручки для фронта, вызывают сервисы или workflow в зависимости от "сложности".
//
// Контракт с фронтом: каждая ручка — `#[tauri::command]`, ответы — DTO с `camelCase`-полями,
// ошибки — `ApiError`. Типы на стороне фронта: frontend/src/lib/types.ts (держать в синхроне).
// Пока бэкенд не готов, ручки чтения отдают демо-данные, а действия — `ApiError::NotImplemented`.
// Места, где нужно подключить сервисы и сценарии, помечены `TODO(backend)`.
pub mod error;
pub mod events;
pub mod games;
pub mod profiles;
pub mod saves;
pub mod settings;
pub mod sync;

mod stub;

pub use error::{ApiError, ApiResult};

pub mod updates;

pub mod launch;
