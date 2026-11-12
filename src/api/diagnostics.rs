use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub trace_id: String,
    pub action: String,
    pub phase: DiagnosticPhase,
    pub elapsed_ms: Option<u64>,
    pub error_kind: Option<DiagnosticErrorKind>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticPhase {
    Success,
    Error,
    Click,
    UnhandledError,
    UnhandledRejection,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticErrorKind {
    Rejected,
    Internal,
    NotImplemented,
    Unknown,
}

#[tauri::command]
pub async fn record_diagnostic(event: DiagnosticEvent) -> Result<(), String> {
    // Only code identifiers, never user-entered values, labels, URLs or error messages.
    if event.action.is_empty()
        || event.action.len() > 96
        || !event
            .action
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    {
        return Err("Invalid diagnostic action".into());
    }
    crate::app::diagnostics::spawn_blocking(move || {
        crate::adapters::diagnostics::request_sync(Some(event.trace_id), "ui_event", || {
            crate::adapters::diagnostics::event(
                "ui",
                &format!(
                    "action={} phase={:?} elapsed_ms={:?} error_kind={:?}",
                    event.action, event.phase, event.elapsed_ms, event.error_kind
                ),
            );
        });
    })
    .await
    .map_err(|_| "Diagnostic worker failed".to_owned())
}
