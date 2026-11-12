//! Bounded local diagnostics. Never pass tokens, URLs or save contents here.
use std::{
    cell::RefCell,
    fs::{self, OpenOptions},
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
    time::{SystemTime, UNIX_EPOCH},
};

static DIRECTORY: Mutex<Option<PathBuf>> = Mutex::new(None);
tokio::task_local! { static TASK_TRACE: String; }
thread_local! { static THREAD_TRACE: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub fn context() -> Option<String> {
    TASK_TRACE
        .try_with(Clone::clone)
        .ok()
        .or_else(|| THREAD_TRACE.with(|id| id.borrow().clone()))
}

pub fn quiet(command: &str) -> bool {
    command.starts_with("get_")
        || command.starts_with("list_")
        || command == "current_profile"
        || command == "ui_event"
}

fn valid_id(id: Option<String>) -> String {
    id.filter(|id| {
        !id.is_empty()
            && id.len() <= 80
            && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    })
    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

pub struct Span {
    operation: &'static str,
    id: String,
    start: Instant,
    quiet: bool,
}
impl Span {
    pub fn new(operation: &'static str) -> Self {
        let id = context().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let quiet = quiet(operation);
        if !quiet {
            event(operation, &format!("begin span_request={id}"));
        }
        Self {
            operation,
            id,
            start: Instant::now(),
            quiet,
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if !self.quiet {
            event(
                self.operation,
                &format!(
                    "end request={} elapsed_ms={} panicking={}",
                    self.id,
                    self.start.elapsed().as_millis(),
                    std::thread::panicking()
                ),
            );
        }
    }
}

pub async fn request<T>(
    id: Option<String>,
    operation: &'static str,
    future: impl Future<Output = T>,
) -> T {
    TASK_TRACE
        .scope(valid_id(id), async {
            let _span = Span::new(operation);
            future.await
        })
        .await
}

pub fn in_context<T>(id: Option<String>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            THREAD_TRACE.with(|id| *id.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(THREAD_TRACE.with(|current| current.replace(id)));
    f()
}

pub fn request_sync<T>(id: Option<String>, operation: &'static str, f: impl FnOnce() -> T) -> T {
    in_context(Some(valid_id(id)), || {
        let _span = Span::new(operation);
        f()
    })
}

const MAX_BYTES: u64 = 10 * 1024 * 1024;

pub fn initialize(directory: PathBuf) -> io::Result<()> {
    fs::create_dir_all(&directory)?;
    *DIRECTORY
        .lock()
        .map_err(|_| io::Error::other("Diagnostics lock poisoned"))? = Some(directory);
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Panic payloads can contain secrets, so log location only.
        event("panic", &format!("location={:?}", info.location()));
        previous_hook(info);
    }));
    event(
        "startup",
        &format!(
            "version={} os={}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        ),
    );
    Ok(())
}

pub fn event(operation: &str, detail: &str) {
    let Ok(guard) = DIRECTORY.lock() else { return };
    let Some(directory) = guard.as_ref() else {
        return;
    };
    let detail = format!(
        "request={} thread={:?} {detail}",
        context().as_deref().unwrap_or("background"),
        std::thread::current().id()
    );
    if let Err(error) = append(directory, operation, &detail) {
        eprintln!("SaveHub diagnostics: {error}");
    }
}

fn append(directory: &Path, operation: &str, detail: &str) -> io::Result<()> {
    let path = directory.join("diagnostics.log");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    // One event per line; cap individual records as well as file size.
    let clean = |value: &str| {
        value
            .chars()
            .take(4096)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
    };
    let line = format!("{timestamp} {} {}\n", clean(operation), clean(detail));
    if fs::metadata(&path)
        .map(|m| m.len() + line.len() as u64 > MAX_BYTES)
        .unwrap_or(false)
    {
        let previous = directory.join("diagnostics.previous.log");
        match fs::remove_file(&previous) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        fs::rename(&path, previous)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotates_and_keeps_events_on_single_lines() {
        let directory = std::env::temp_dir().join(format!("savehub-log-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("diagnostics.log");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize]).unwrap();
        append(&directory, "delete_profile", "start\nforged event").unwrap();
        let current = fs::read_to_string(&path).unwrap();
        assert_eq!(current.lines().count(), 1);
        assert!(current.contains("start forged event"));
        assert_eq!(
            fs::metadata(directory.join("diagnostics.previous.log"))
                .unwrap()
                .len(),
            MAX_BYTES
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
