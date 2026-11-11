//! Keep SaveHub's taskbar identity independent of game shortcuts and their icons.
#[cfg(windows)]
pub fn initialize() -> super::AdapterResult<()> {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    let id: Vec<u16> = "io.github.savehub".encode_utf16().chain([0]).collect();
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
    if result < 0 {
        return Err(super::AdapterError::InvalidData(format!(
            "Не удалось установить AppUserModelID SaveHub: {result:#x}"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn initialize() -> super::AdapterResult<()> {
    Ok(())
}
