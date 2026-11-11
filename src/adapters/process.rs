use super::{AdapterError, AdapterResult};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
}

/// Keep the handle so the worker can reap exited children (also on Unix).
pub fn spawn(
    executable: &Path,
    args: &[String],
    working_dir: Option<&Path>,
) -> AdapterResult<Child> {
    let mut command = Command::new(executable);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(directory) = working_dir {
        command.current_dir(directory);
    }
    command.spawn().map_err(AdapterError::Io)
}

#[cfg(windows)]
pub fn list() -> AdapterResult<Vec<ProcessInfo>> {
    use std::{mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    };
    // All handles are closed on both successful and failed enumeration.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(AdapterError::Io(std::io::Error::last_os_error()));
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Process32FirstW(snapshot, &mut entry);
        let mut processes = Vec::new();
        while found != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
            let executable = if !handle.is_null() {
                let mut buffer = vec![0u16; 32768];
                let mut length = buffer.len() as u32;
                let result =
                    QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
                CloseHandle(handle);
                (result != 0).then(|| {
                    PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]))
                })
            } else {
                None // Protected process: retain its name for conservative matching.
            };
            processes.push(ProcessInfo {
                pid: entry.th32ProcessID,
                name,
                executable,
            });
            found = Process32NextW(snapshot, &mut entry);
        }
        let error = GetLastError();
        CloseHandle(snapshot);
        if error != ERROR_NO_MORE_FILES {
            return Err(AdapterError::Io(std::io::Error::from_raw_os_error(
                error as i32,
            )));
        }
        Ok(processes)
    }
}

#[cfg(target_os = "linux")]
pub fn list() -> AdapterResult<Vec<ProcessInfo>> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir("/proc").map_err(AdapterError::Io)? {
        let entry = entry.map_err(AdapterError::Io)?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let executable = std::fs::read_link(entry.path().join("exe")).ok();
        let name = std::fs::read_to_string(entry.path().join("comm"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        result.push(ProcessInfo {
            pid,
            name,
            executable,
        });
    }
    Ok(result)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn list() -> AdapterResult<Vec<ProcessInfo>> {
    Err(AdapterError::UnsupportedPlatform)
}

/// Windows command-line quoting, including quotes and trailing backslashes.
#[cfg(any(windows, test))]
fn quote_argument(argument: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in argument.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            result.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n('\\', slashes));
        }
        result.push(c);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

/// Create a .lnk using the Windows Shell COM automation interface. Values are
/// passed as environment data, never interpolated into PowerShell source.
#[cfg(windows)]
pub fn create_shortcut(
    path: &Path,
    executable: &Path,
    args: &[String],
    working_dir: Option<&Path>,
    icon: &Path,
) -> AdapterResult<()> {
    use std::os::windows::process::CommandExt;
    let temporary = path.with_file_name(format!(".savehub-{}.lnk", uuid::Uuid::new_v4()));
    let powershell = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| AdapterError::InvalidData("SystemRoot не задан".into()))?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            r#"
$ErrorActionPreference = 'Stop'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($env:SAVEHUB_LINK)
$link.TargetPath = $env:SAVEHUB_TARGET
$link.Arguments = $env:SAVEHUB_ARGS
$link.WorkingDirectory = $env:SAVEHUB_WORKDIR
$link.IconLocation = $env:SAVEHUB_ICON + ',0'
$link.Description = 'Запуск игры через SaveHub'
$link.Save()
"#,
        ])
        .env("SAVEHUB_LINK", &temporary)
        .env("SAVEHUB_TARGET", executable)
        .env(
            "SAVEHUB_ARGS",
            args.iter()
                .map(|a| quote_argument(a))
                .collect::<Vec<_>>()
                .join(" "),
        )
        .env(
            "SAVEHUB_WORKDIR",
            working_dir.unwrap_or_else(|| Path::new("")),
        )
        .env("SAVEHUB_ICON", icon)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(AdapterError::Io)?;
    let result = if output.status.success() && temporary.is_file() {
        // Publish without replacing any existing desktop file (including another link).
        std::fs::hard_link(&temporary, path).map_err(AdapterError::Io)
    } else {
        Err(AdapterError::InvalidData(format!(
            "Windows не создал ярлык: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    };
    let _ = std::fs::remove_file(temporary);
    result
}

#[cfg(not(windows))]
pub fn create_shortcut(
    _: &Path,
    _: &Path,
    _: &[String],
    _: Option<&Path>,
    _: &Path,
) -> AdapterResult<()> {
    Err(AdapterError::UnsupportedPlatform)
}

/// Roll back only a shortcut successfully created by this operation.
pub fn remove_created_shortcut(path: &Path) -> AdapterResult<()> {
    std::fs::remove_file(path).map_err(AdapterError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_windows_arguments_without_shell_interpretation() {
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument("a b&$(x)"), "\"a b&$(x)\"");
        assert_eq!(quote_argument("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_argument("C:\\Games\\"), "\"C:\\Games\\\\\"");
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn lists_current_process() {
        let list = list().unwrap();
        assert!(list.iter().any(|p| p.pid == std::process::id()));
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    #[test]
    fn process_listing_reports_unsupported_platform() {
        assert!(matches!(
            list(),
            Err(super::super::AdapterError::UnsupportedPlatform)
        ));
    }
    #[cfg(windows)]
    #[test]
    fn creates_unicode_shortcut_without_overwriting_existing_file() {
        let root = std::env::temp_dir().join(format!("savehub-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let link = root.join("Игра & ' тест.lnk");
        let exe = std::env::current_exe().unwrap();
        create_shortcut(
            &link,
            &exe,
            &["--launch-game".into(), uuid::Uuid::nil().to_string()],
            exe.parent(),
            &exe,
        )
        .unwrap();
        let original = std::fs::read(&link).unwrap();
        assert_eq!(&original[..4], &[0x4c, 0, 0, 0]);
        assert!(create_shortcut(&link, &exe, &[], None, &exe).is_err());
        assert_eq!(std::fs::read(&link).unwrap(), original);
        std::fs::remove_dir_all(root).unwrap();
    }
}
