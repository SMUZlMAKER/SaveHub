//! Global keyboard registration owns a dedicated Windows message queue.
use std::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub modifiers: u32,
    pub key: u32,
}

pub fn parse(value: &str) -> Result<Option<Shortcut>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let mut modifiers = 0;
    let mut key = None;
    for part in value.split('+').map(|s| s.trim().to_ascii_uppercase()) {
        let modifier = match part.as_str() {
            "ALT" => 1,
            "CTRL" | "CONTROL" => 2,
            "SHIFT" => 4,
            _ => 0,
        };
        if modifier != 0 {
            if modifiers & modifier != 0 {
                return Err("Модификатор указан дважды".into());
            }
            modifiers |= modifier;
        } else {
            let parsed = if part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric() {
                Some(part.as_bytes()[0] as u32)
            } else {
                part.strip_prefix('F')
                    .and_then(|n| n.parse::<u32>().ok())
                    .filter(|n| (1..=24).contains(n))
                    .map(|n| 0x6f + n)
            };
            if key.is_some() || parsed.is_none() {
                return Err("Используйте Ctrl/Alt/Shift и одну клавишу A–Z, 0–9 или F1–F24".into());
            }
            key = parsed;
        }
    }
    if modifiers & 3 == 0 || key.is_none() {
        return Err("Нужны Ctrl или Alt и одна основная клавиша".into());
    }
    Ok(Some(Shortcut {
        modifiers,
        key: key.unwrap(),
    }))
}

type Request = (Option<Shortcut>, mpsc::Sender<Result<(), String>>);
pub struct Hotkey {
    sender: mpsc::Sender<Request>,
}
impl Hotkey {
    pub fn start(callback: impl Fn() + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel::<Request>();
        std::thread::spawn(move || run(receiver, callback));
        Self { sender }
    }
    pub fn set(&self, shortcut: Option<Shortcut>) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send((shortcut, sender))
            .map_err(|_| "Обработчик сочетаний клавиш остановлен".to_string())?;
        receiver
            .recv()
            .map_err(|_| "Обработчик сочетаний клавиш остановлен".to_string())?
    }
}
#[cfg(windows)]
fn run(receiver: mpsc::Receiver<Request>, callback: impl Fn()) {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey},
        WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, WM_HOTKEY},
    };
    let mut current: Option<Shortcut> = None;
    let mut active_slot = 1;
    loop {
        match receiver.recv_timeout(std::time::Duration::from_millis(30)) {
            Ok((next, reply)) => {
                let result = if next == current {
                    Ok(())
                } else {
                    // Reserve the replacement first: a conflict leaves the old shortcut usable.
                    let slot = if active_slot == 1 { 2 } else { 1 };
                    if next.is_some_and(|key| unsafe { RegisterHotKey(std::ptr::null_mut(), slot, key.modifiers | MOD_NOREPEAT, key.key) } == 0) {
                        Err(format!("Сочетание занято или недоступно: {}", std::io::Error::last_os_error()))
                    } else {
                        if current.is_some() { unsafe { UnregisterHotKey(std::ptr::null_mut(), active_slot); } }
                        active_slot = slot;
                        Ok(())
                    }
                };
                if result.is_ok() {
                    current = next;
                }
                let _ = reply.send(result);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while unsafe {
            PeekMessageW(
                &mut message,
                std::ptr::null_mut(),
                WM_HOTKEY,
                WM_HOTKEY,
                PM_REMOVE,
            )
        } != 0
        {
            if current.is_some() && message.wParam == active_slot as usize {
                callback();
            }
        }
    }
    unsafe {
        UnregisterHotKey(std::ptr::null_mut(), 1);
        UnregisterHotKey(std::ptr::null_mut(), 2);
    }
}
#[cfg(not(windows))]
fn run(receiver: mpsc::Receiver<Request>, _callback: impl Fn()) {
    while let Ok((shortcut, reply)) = receiver.recv() {
        let _ = reply.send(if shortcut.is_none() {
            Ok(())
        } else {
            Err("Глобальные сочетания поддерживаются только в Windows".into())
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn manual_hotkey_replacement_preserves_old_registration_on_conflict_and_disables() {
        let first = Hotkey::start(|| {});
        let second = Hotkey::start(|| {});
        let third = Hotkey::start(|| {});
        let a = parse("Ctrl+Alt+Shift+F23").unwrap();
        let b = parse("Ctrl+Alt+Shift+F24").unwrap();
        first.set(a).unwrap();
        assert!(second.set(a).is_err());
        first.set(b).unwrap();
        second.set(a).unwrap();
        assert!(first.set(a).is_err());
        assert!(third.set(b).is_err());
        first.set(None).unwrap();
        third.set(b).unwrap();
        second.set(None).unwrap();
        third.set(None).unwrap();
    }

    #[test]
    fn rejects_ambiguous_shortcuts_and_accepts_disabled_and_function_keys() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(
            parse("Ctrl+Alt+F8").unwrap(),
            Some(Shortcut {
                modifiers: 3,
                key: 0x77
            })
        );
        for bad in [
            "F8",
            "Shift+A",
            "Ctrl+Ctrl+A",
            "Ctrl+A+B",
            "Ctrl+F25",
            "Ctrl+",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
