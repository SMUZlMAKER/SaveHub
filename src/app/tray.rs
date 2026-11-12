//! Нативное меню трея, данные из общих хранилищ; действия общие с API окна.
use crate::{
    api::{ApiError, events},
    domain::{Catalog, ProfileId, RuntimeState, SyncState},
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tauri::{
    AppHandle, Emitter, Listener, Manager, Wry,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

pub const MAIN_WINDOW: &str = "main";
const TRAY_ID: &str = "main";
const PROFILE_PREFIX: &str = "profile:";

#[derive(Clone, PartialEq, Eq)]
struct TrayView {
    profiles: Vec<(ProfileId, String)>,
    active: Option<ProfileId>,
    blocker: Option<&'static str>,
    sync_enabled: bool,
    sync_label: &'static str,
    sync_status: String,
}
struct TrayState(Mutex<Option<TrayView>>);

fn view(app: &AppHandle) -> TrayView {
    let runtime = app.state::<Arc<RuntimeState>>();
    let (active, running) = runtime.profile_and_running_games();
    let mut profiles: Vec<_> = app
        .state::<Arc<Catalog>>()
        .profiles()
        .into_values()
        .map(|p| (p.id, p.name))
        .collect();
    profiles.sort_by_key(|(id, name)| (name.to_lowercase(), *id));
    let status = runtime.sync_status();
    let (sync_enabled, sync_label, sync_text) = match status.state {
        SyncState::NotConfigured => (
            false,
            "Сначала настройте репозиторий",
            "○ Репозиторий не настроен".into(),
        ),
        SyncState::Syncing => (false, "Синхронизация…", "↻ Синхронизация…".into()),
        SyncState::Error => (
            true,
            "Повторить синхронизацию",
            "○ Ошибка синхронизации".into(),
        ),
        SyncState::Idle => (
            true,
            "Синхронизировать",
            sync_status(
                status
                    .last_sync_at
                    .map(|ms| SystemTime::UNIX_EPOCH + Duration::from_millis(ms)),
                SystemTime::now(),
            ),
        ),
    };
    TrayView {
        profiles,
        active,
        blocker: super::actions::profile_switch_blocker(
            crate::api::features::get_app_features(None).profile_switch,
            running.as_deref(),
        ),
        sync_enabled,
        sync_label,
        sync_status: sync_text,
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let initial = view(app);
    app.manage(TrayState(Mutex::new(Some(initial.clone()))));
    let menu = build_menu(app, &initial)?;
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
        events::PROFILE_CHANGED,
        events::GAME_PROCESS_CHANGED,
        events::GAMES_CHANGED,
    ] {
        let handle = app.clone();
        app.listen(event, move |_| refresh_sync_status(&handle));
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            refresh_sync_status(&handle);
        }
    });
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
fn build_menu(app: &AppHandle, view: &TrayView) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    let active = view
        .profiles
        .iter()
        .find(|(id, _)| Some(*id) == view.active);
    let header = active
        .map(|(_, name)| format!("Активный профиль: {name}"))
        .unwrap_or_else(|| "Активный профиль не выбран".into());
    menu.append(&MenuItem::new(app, header, false, None::<&str>)?)?;
    for (id, name) in &view.profiles {
        let active = view.active == Some(*id);
        menu.append(&CheckMenuItem::with_id(
            app,
            format!("{PROFILE_PREFIX}{id}"),
            name,
            view.blocker.is_none() && !active,
            active,
            None::<&str>,
        )?)?;
    }
    if let Some(reason) = view.blocker {
        menu.append(&MenuItem::new(app, reason, false, None::<&str>)?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "profiles",
        "Управление профилями…",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for (id, label, enabled) in [
        ("sync", view.sync_label, view.sync_enabled),
        ("saves", "Сохранения…", true),
        ("settings", "Настройки…", true),
    ] {
        menu.append(&MenuItem::with_id(app, id, label, enabled, None::<&str>)?)?;
    }
    menu.append(&MenuItem::new(app, &view.sync_status, false, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Выход",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    // IDs are fixed action names or profile UUIDs; never menu labels.
    crate::adapters::diagnostics::event("tray_click", &format!("action={}", event.id.as_ref()));
    match event.id.as_ref() {
        "sync" => sync_from_tray(app),
        "profiles" => open_screen(app, "profiles"),
        "saves" => open_screen(app, "saves"),
        "settings" => open_screen(app, "settings"),
        "quit" => app.exit(0),
        id => {
            if let Some(id) = id.strip_prefix(PROFILE_PREFIX) {
                let id = id.to_owned();
                run_action(app, move |app| {
                    let id = id.parse().map_err(|_| {
                        ApiError::Rejected(
                            "Профиль больше недоступен. Откройте список профилей заново.".into(),
                        )
                    })?;
                    super::actions::switch_profile(&app, id)
                });
            }
        }
    }
}
// Manual sync runs off the UI thread and returns its actual result to the tray.
fn sync_from_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let action_app = app.clone();
        let result =
            crate::app::diagnostics::spawn_blocking(move || super::actions::sync_now(&action_app))
                .await
                .unwrap_or_else(|error| Err(ApiError::Internal(error.to_string())));
        if let Err(error) = result {
            show_error(&app, error);
        }
        refresh_sync_status(&app);
    });
}
fn run_action(
    app: &AppHandle,
    action: impl FnOnce(AppHandle) -> Result<(), ApiError> + Send + 'static,
) {
    let app = app.clone();
    crate::app::diagnostics::spawn_blocking(move || {
        if let Err(error) = action(app.clone()) {
            show_error(&app, error);
        }
        refresh_sync_status(&app);
    });
}
fn show_error(app: &AppHandle, error: ApiError) {
    let message: String = match error {
        ApiError::NotImplemented(_) => {
            "Эта функция пока недоступна в установленной версии SaveHub.".into()
        }
        ApiError::Rejected(message) => message,
        ApiError::Internal(_) => {
            "Не удалось выполнить действие из-за внутренней ошибки. Попробуйте ещё раз.".into()
        }
    };
    app.dialog()
        .message(message)
        .title("SaveHub — действие не выполнено")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}
fn open_screen(app: &AppHandle, screen: &str) {
    show_main_window(app);
    if let Err(error) = app.emit_to(MAIN_WINDOW, events::NAVIGATE, screen) {
        show_error(app, ApiError::Internal(error.to_string()));
    }
}

// Все операции с меню — на UI-потоке. Mutex никогда не удерживается при обращении к UI.
pub fn refresh_sync_status(app: &AppHandle) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let next = view(&handle);
        let state = handle.state::<TrayState>();
        if state.0.lock().unwrap().as_ref() == Some(&next) {
            return;
        }
        let result = build_menu(&handle, &next).and_then(|menu| match handle.tray_by_id(TRAY_ID) {
            Some(tray) => tray.set_menu(Some(menu)),
            None => Ok(()),
        });
        match result {
            Ok(()) => *state.0.lock().unwrap() = Some(next),
            Err(error) => show_error(&handle, ApiError::Internal(error.to_string())),
        }
    }) {
        show_error(app, ApiError::Internal(error.to_string()));
    }
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
