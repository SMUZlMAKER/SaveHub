use super::{AdapterError, AdapterResult};

#[cfg(windows)]
const TARGET: &str = "SaveHub/github-token";

/// Прочитать токен только для использования внутри бэкенда.
/// Отсутствующая запись — None; ошибки доступа и формата возвращаются вызывающему.
#[cfg(windows)]
pub fn get_token() -> AdapterResult<Option<String>> {
    use std::{ffi::c_void, ptr, slice};
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredFree, CredReadW};

    let target: Vec<u16> = TARGET.encode_utf16().chain(std::iter::once(0)).collect();
    let mut credential = ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(1168) => Ok(None),
            _ => Err(AdapterError::Credentials(error.to_string())),
        };
    }

    // CredReadW возвращает буфер, действительный до CredFree. Копируем строку
    // перед освобождением; содержимое токена не включаем в ошибки.
    let result = unsafe {
        let entry = &*credential;
        if entry.CredentialBlobSize == 0 {
            Ok(Some(String::new()))
        } else if entry.CredentialBlob.is_null() {
            Err(AdapterError::Credentials("Пустой указатель токена".into()))
        } else {
            let bytes =
                slice::from_raw_parts(entry.CredentialBlob, entry.CredentialBlobSize as usize);
            std::str::from_utf8(bytes)
                .map(|token| Some(token.to_owned()))
                .map_err(|_| AdapterError::Credentials("Токен имеет неверный формат UTF-8".into()))
        }
    };
    unsafe { CredFree(credential as *const c_void) };
    result
}

#[cfg(windows)]
pub fn has_token() -> AdapterResult<bool> {
    use std::{ffi::c_void, ptr};
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredFree, CredReadW};
    let target: Vec<u16> = TARGET.encode_utf16().chain(std::iter::once(0)).collect();
    let mut credential = ptr::null_mut();
    let result = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if result == 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(1168) => Ok(false),
            _ => Err(AdapterError::Credentials(
                std::io::Error::last_os_error().to_string(),
            )),
        };
    }
    unsafe { CredFree(credential as *const c_void) };
    Ok(true)
}

#[cfg(windows)]
pub fn set_token(token: &str) -> AdapterResult<()> {
    use std::ptr;
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredWriteW,
    };
    let target: Vec<u16> = TARGET.encode_utf16().chain(std::iter::once(0)).collect();
    if token.is_empty() {
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 && std::io::Error::last_os_error().raw_os_error() != Some(1168) {
            return Err(AdapterError::Credentials(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        return Ok(());
    }
    let secret = token.as_bytes();
    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut u16,
        Comment: ptr::null_mut(),
        LastWritten: windows_sys::Win32::Foundation::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: secret.len() as u32,
        CredentialBlob: secret.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: ptr::null_mut(),
    };
    let ok = unsafe { CredWriteW(&credential, 0) };
    if ok == 0 {
        return Err(AdapterError::Credentials(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn get_token() -> AdapterResult<Option<String>> {
    Err(AdapterError::UnsupportedPlatform)
}

#[cfg(not(windows))]
pub fn has_token() -> AdapterResult<bool> {
    Ok(false)
}

#[cfg(not(windows))]
pub fn set_token(_token: &str) -> AdapterResult<()> {
    Err(AdapterError::UnsupportedPlatform)
}
