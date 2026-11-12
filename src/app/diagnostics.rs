//! Propagate request context into blocking workers, without keeping it on async runtime threads.
pub fn spawn_blocking<F, T>(f: F) -> tauri::async_runtime::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let context = Some(
        crate::adapters::diagnostics::context().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    );
    tauri::async_runtime::spawn_blocking(move || {
        crate::adapters::diagnostics::in_context(context, f)
    })
}

/// Keep tracing across detached launch/hotkey tasks as well.
pub fn spawn<F>(future: F) -> tauri::async_runtime::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let context = crate::adapters::diagnostics::context();
    tauri::async_runtime::spawn(crate::adapters::diagnostics::request(
        context,
        "async_task",
        future,
    ))
}
