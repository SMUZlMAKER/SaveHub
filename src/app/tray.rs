//! Иконка и меню в трее.
//!
//! Состав меню взят из макета интерфейса: активный профиль и переключение профилей,
//! синхронизация, переход к снапшотам и настройкам, статус синхронизации, выход.
//! Меню нативное (Win32), поэтому оно не стилизуется: только текст, галочки и разделители.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::api::events::NAVIGATE;
use tauri::{
    AppHandle, Emitter, Manager, Wry,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub const MAIN_WINDOW: &str = "main";


const TRAY_ID: &str = "main";
const ID_SYNC: &str = "sync";
const ID_SNAPSHOTS: &str = "snapshots";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";
const PROFILE_PREFIX: &str = "profile:";

/// Как часто обновлять строку «Синхронизировано N мин назад».
const STATUS_REFRESH: Duration = Duration::from_secs(30);

/// Всё, что нужно меню, чтобы себя нарисовать.
pub struct TrayView {
    pub profiles: Vec<String>,
    pub active_profile: Option<String>,
    /// Игра, которая сейчас запущена: пока она идет, профили переключать нельзя.
    pub running_game: Option<String>,
    pub last_sync: Option<SystemTime>,
}

impl TrayView {
    // TODO: убрать заглушку, когда появятся ProfileService и RuntimeState — брать данные из хранилищ.
    fn placeholder() -> Self {
        Self {
            profiles: vec!["Основной".into(), "Гость".into()],
            active_profile: Some("Основной".into()),
            running_game: None,
            last_sync: None,
        }
    }
}

struct TrayState {
    view: Mutex<TrayView>,
    /// Строка статуса синхронизации: ее текст обновляем по таймеру, не пересобирая меню.
    status_item: Mutex<Option<MenuItem<Wry>>>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(TrayState {
        view: Mutex::new(TrayView::placeholder()),
        status_item: Mutex::new(None),
    });

    let menu = build_menu(app)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("SaveHub")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    spawn_status_refresher(app.clone());
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<TrayState>();
    let view = state.view.lock().unwrap();
    let menu = Menu::new(app)?;

    let header = match &view.active_profile {
        Some(profile) => format!("Активный профиль: {profile}"),
        None => "Профиль не выбран".to_owned(),
    };
    menu.append(&MenuItem::new(app, header, false, None::<&str>)?)?;

    let can_switch = view.running_game.is_none();
    for profile in &view.profiles {
        let is_active = view.active_profile.as_deref() == Some(profile.as_str());
        let id = format!("{PROFILE_PREFIX}{profile}");
        menu.append(&CheckMenuItem::with_id(app, id, profile, can_switch, is_active, None::<&str>)?)?;
    }
    if let Some(game) = &view.running_game {
        let text = format!("Запущена {game}: переключение недоступно");
        menu.append(&MenuItem::new(app, text, false, None::<&str>)?)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, ID_SYNC, "Синхронизировать сейчас", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, ID_SNAPSHOTS, "Снапшоты и откат…", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, ID_SETTINGS, "Настройки…", true, None::<&str>)?)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let status = MenuItem::new(app, sync_status(view.last_sync, SystemTime::now()), false, None::<&str>)?;
    menu.append(&status)?;
    *state.status_item.lock().unwrap() = Some(status);

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, ID_QUIT, "Выход", true, None::<&str>)?)?;
    Ok(menu)
}

/// Пересобрать меню после изменения состояния.
fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    match build_menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(err) => eprintln!("tray: не удалось пересобрать меню: {err}"),
    }
    let tooltip = match &app.state::<TrayState>().view.lock().unwrap().active_profile {
        Some(profile) => format!("SaveHub — {profile}"),
        None => "SaveHub".to_owned(),
    };
    let _ = tray.set_tooltip(Some(tooltip));
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        ID_SYNC => sync_now(app),
        ID_SNAPSHOTS => open_screen(app, "snapshots"),
        ID_SETTINGS => open_screen(app, "settings"),
        ID_QUIT => app.exit(0),
        id => {
            if let Some(profile) = id.strip_prefix(PROFILE_PREFIX) {
                switch_profile(app, profile);
            }
        }
    }
}

fn switch_profile(app: &AppHandle, profile: &str) {
    {
        let state = app.state::<TrayState>();
        let mut view = state.view.lock().unwrap();
        // Пункты и так выключены, пока идет игра, но проверяем и здесь.
        if view.running_game.is_none() {
            // TODO: вызвать workflows::SwitchProfile вместо простой смены поля
            view.active_profile = Some(profile.to_owned());
        }
    }
    // Пересобираем даже без изменений: Windows сам переключает галочку у нажатого пункта.
    refresh(app);
}

fn sync_now(app: &AppHandle) {
    // TODO: попросить RepoWorker синхронизировать репозиторий и обновить статус по результату
    app.state::<TrayState>().view.lock().unwrap().last_sync = Some(SystemTime::now());
    refresh(app);
}

fn open_screen(app: &AppHandle, screen: &str) {
    show_main_window(app);
    let _ = app.emit_to(MAIN_WINDOW, NAVIGATE, screen);
}

fn spawn_status_refresher(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(STATUS_REFRESH);
            let state = app.state::<TrayState>();
            let text = sync_status(state.view.lock().unwrap().last_sync, SystemTime::now());
            if let Some(item) = state.status_item.lock().unwrap().as_ref() {
                let _ = item.set_text(text);
            }
        }
    });
}

fn sync_status(last_sync: Option<SystemTime>, now: SystemTime) -> String {
    let Some(last) = last_sync else {
        return "○ Ещё не синхронизировано".to_owned();
    };
    let ago = now.duration_since(last).unwrap_or_default().as_secs();
    let when = match ago {
        0..60 => "только что".to_owned(),
        60..3600 => format!("{} мин назад", ago / 60),
        3600..86400 => format!("{} ч назад", ago / 3600),
        _ => "больше суток назад".to_owned(),
    };
    format!("● Синхронизировано {when} · GitHub")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_after(secs: u64) -> String {
        let now = SystemTime::now();
        sync_status(Some(now - Duration::from_secs(secs)), now)
    }

    #[test]
    fn sync_status_formats_elapsed_time() {
        assert_eq!(sync_status(None, SystemTime::now()), "○ Ещё не синхронизировано");
        assert_eq!(status_after(5), "● Синхронизировано только что · GitHub");
        assert_eq!(status_after(3 * 60 + 10), "● Синхронизировано 3 мин назад · GitHub");
        assert_eq!(status_after(2 * 3600), "● Синхронизировано 2 ч назад · GitHub");
        assert_eq!(status_after(3 * 86400), "● Синхронизировано больше суток назад · GitHub");
    }

    #[test]
    fn sync_status_tolerates_clock_going_back() {
        let now = SystemTime::now();
        let future = now + Duration::from_secs(120);
        assert_eq!(sync_status(Some(future), now), "● Синхронизировано только что · GitHub");
    }
}
