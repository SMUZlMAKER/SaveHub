//! Иконка и меню в трее.
//!
//! Состав меню взят из макета интерфейса: активный профиль и переключение профилей,
//! синхронизация, переход к сохранениям и настройкам, статус синхронизации, выход.
//! Меню нативное (Win32), поэтому оно не стилизуется: только текст, галочки и разделители.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::api::events::NAVIGATE;
use tauri::{
    AppHandle, Emitter, Listener, Manager, Wry,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub const MAIN_WINDOW: &str = "main";

const TRAY_ID: &str = "main";
const ID_SYNC: &str = "sync";
const ID_SAVES: &str = "saves";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";
const PROFILE_PREFIX: &str = "profile:";

/// Как часто обновлять строку «Синхронизировано N мин назад».
const STATUS_REFRESH: Duration = Duration::from_secs(30);

/// Всё, что нужно меню, чтобы себя нарисовать.
pub struct TrayView {
    pub profiles: Vec<(crate::domain::ProfileId, String)>,
    pub active_profile: Option<crate::domain::ProfileId>,
    /// Игра, которая сейчас запущена: пока она идет, профили переключать нельзя.
    pub running_game: Option<String>,
}

impl TrayView {
    fn load(app: &AppHandle) -> Self {
        let catalog = app.state::<std::sync::Arc<crate::domain::Catalog>>();
        let runtime = app.state::<std::sync::Arc<crate::domain::RuntimeState>>();
        let mut profiles: Vec<_> = catalog
            .profiles()
            .into_values()
            .map(|p| (p.id, p.name))
            .collect();
        profiles.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        let running_game = match runtime.running_games() {
            None => Some("состояние процессов неизвестно".into()),
            Some(games) if !games.is_empty() => Some(
                games
                    .iter()
                    .map(|id| {
                        catalog
                            .games()
                            .get(id)
                            .map(|g| g.name.clone())
                            .unwrap_or_else(|| id.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            _ => None,
        };
        Self {
            profiles,
            active_profile: runtime.active_profile(),
            running_game,
        }
    }
}

struct TrayState {
    /// Строка статуса синхронизации: ее текст обновляем по таймеру, не пересобирая меню.
    status_item: Mutex<Option<MenuItem<Wry>>>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(TrayState {
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

    for event in [
        crate::api::events::PROFILE_CHANGED,
        crate::api::events::GAMES_CHANGED,
    ] {
        let handle = app.clone();
        app.listen(event, move |_| {
            refresh_menu(&handle);
        });
    }
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
    let view = TrayView::load(app);
    let menu = Menu::new(app)?;

    let header = match &view.active_profile {
        Some(profile) => format!(
            "Активный профиль: {}",
            view.profiles
                .iter()
                .find(|(id, _)| id == profile)
                .map(|(_, name)| name.as_str())
                .unwrap_or("не найден")
        ),
        None => "Профили — в окне приложения".to_owned(),
    };
    menu.append(&MenuItem::new(app, header, false, None::<&str>)?)?;

    let can_switch = view.running_game.is_none();
    for (profile_id, profile) in &view.profiles {
        let is_active = view.active_profile == Some(*profile_id);
        let id = format!("{PROFILE_PREFIX}{profile_id}");
        menu.append(&CheckMenuItem::with_id(
            app,
            id,
            profile,
            can_switch,
            is_active,
            None::<&str>,
        )?)?;
    }
    if let Some(game) = &view.running_game {
        let text = format!("Запущена {game}: переключение недоступно");
        menu.append(&MenuItem::new(app, text, false, None::<&str>)?)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_SYNC,
        "Синхронизация пока недоступна",
        false,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_SAVES,
        "Сохранения и откат…",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_SETTINGS,
        "Настройки…",
        true,
        None::<&str>,
    )?)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let status = MenuItem::new(app, current_sync_status(app), false, None::<&str>)?;
    menu.append(&status)?;
    *state.status_item.lock().unwrap() = Some(status);

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_QUIT,
        "Выход",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        ID_SYNC => sync_now(app),
        ID_SAVES => open_screen(app, "saves"),
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
    let Ok(profile) = profile.parse() else {
        return;
    };
    let workflow = app
        .state::<std::sync::Arc<crate::workflows::SwitchProfile>>()
        .inner()
        .clone();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = workflow.execute(&profile);
        for event in [
            crate::api::events::PROFILE_CHANGED,
            crate::api::events::SAVES_CHANGED,
            crate::api::events::CONFLICTS_CHANGED,
        ] {
            let _ = app.emit(event, ());
        }
        if let Err(error) = result {
            use tauri_plugin_dialog::DialogExt;
            let error = crate::api::ApiError::from(error);
            let message = match error {
                crate::api::ApiError::Rejected(m) | crate::api::ApiError::Internal(m) => m,
                _ => "Переключение недоступно".into(),
            };
            app.dialog().message(message).title("SaveHub").show(|_| {});
        }
    });
}

fn refresh_menu(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let (Some(tray), Ok(menu)) = (handle.tray_by_id(TRAY_ID), build_menu(&handle)) {
            let _ = tray.set_menu(Some(menu));
        }
    });
}

fn sync_now(app: &AppHandle) {
    if let Err(error) = app
        .state::<crate::background::repo::RepoWorkerHandle>()
        .sync_now()
    {
        eprintln!("Не удалось запустить синхронизацию: {error:?}");
    }
}

fn open_screen(app: &AppHandle, screen: &str) {
    show_main_window(app);
    let _ = app.emit_to(MAIN_WINDOW, NAVIGATE, screen);
}

pub fn refresh_sync_status(app: &AppHandle) {
    let state = app.state::<TrayState>();
    let item = state.status_item.lock().unwrap().clone();
    if let Some(item) = item {
        let _ = item.set_text(current_sync_status(app));
    }
}

fn current_sync_status(app: &AppHandle) -> String {
    use crate::domain::{RuntimeState, SyncState};
    let status = app.state::<std::sync::Arc<RuntimeState>>().sync_status();
    match status.state {
        SyncState::NotConfigured => "○ Репозиторий не настроен".into(),
        SyncState::Syncing => "↻ Синхронизация…".into(),
        SyncState::Error => "○ Ошибка синхронизации".into(),
        SyncState::Idle => sync_status(
            status
                .last_sync_at
                .map(|ms| SystemTime::UNIX_EPOCH + Duration::from_millis(ms)),
            SystemTime::now(),
        ),
    }
}

fn spawn_status_refresher(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(STATUS_REFRESH);
            refresh_sync_status(&app);
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
        assert_eq!(
            sync_status(None, SystemTime::now()),
            "○ Ещё не синхронизировано"
        );
        assert_eq!(status_after(5), "● Синхронизировано только что · GitHub");
        assert_eq!(
            status_after(3 * 60 + 10),
            "● Синхронизировано 3 мин назад · GitHub"
        );
        assert_eq!(
            status_after(2 * 3600),
            "● Синхронизировано 2 ч назад · GitHub"
        );
        assert_eq!(
            status_after(3 * 86400),
            "● Синхронизировано больше суток назад · GitHub"
        );
    }

    #[test]
    fn sync_status_tolerates_clock_going_back() {
        let now = SystemTime::now();
        let future = now + Duration::from_secs(120);
        assert_eq!(
            sync_status(Some(future), now),
            "● Синхронизировано только что · GitHub"
        );
    }
}
