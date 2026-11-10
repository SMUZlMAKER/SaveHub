//! Windows Run needs a quoted executable path, including paths with spaces.
use tauri_plugin_autostart::ManagerExt;

pub fn is_enabled(app: &tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_registry::{CURRENT_USER, Type};
        const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
        let key = CURRENT_USER.create(RUN).map_err(|e| e.to_string())?;
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            key.set_string("SaveHub", format!("\"{}\" --autostart", exe.display()))
                .map_err(|e| e.to_string())?;
            // Explicitly enabling in Settings also clears a Task Manager disable override.
            if let Ok(approved) = CURRENT_USER
                .options()
                .write()
                .open(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run")
            {
                approved
                    .set_bytes(
                        "SaveHub",
                        Type::Bytes,
                        &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    )
                    .map_err(|e| e.to_string())?;
            }
        } else if key.get_string("SaveHub").is_ok() {
            key.remove_value("SaveHub").map_err(|e| e.to_string())?;
        }
        let _ = app;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let manager = app.autolaunch();
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|e| e.to_string())
    }
}
