//! System tray: dynamic icon, tooltip, and the native right-click menu.

use crate::engine::{Engine, HoldSpec};
use crate::tray_icon;
use crate::windows::{self, Anchor};
use parking_lot::Mutex;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Position, Size};

const TRAY_ID: &str = "flipon";

/// Fingerprint of the last menu we built, so we only rebuild it when something changed.
static LAST_MENU: Mutex<String> = Mutex::new(String::new());

pub fn build(app: &AppHandle, engine: &Engine) -> tauri::Result<()> {
    let rendered = tray_icon::render(None, &engine.settings(), tray_icon::light_taskbar());
    let image = Image::new_owned(rendered.rgba, rendered.size, rendered.size);
    let menu = build_menu(app, engine)?;

    let engine_for_menu = engine.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(image)
        .tooltip(crate::windows::DEV_MARK.to_string() + "FlipOn")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                let app = tray.app_handle();
                let scale = app
                    .primary_monitor()
                    .ok()
                    .flatten()
                    .map(|m| m.scale_factor())
                    .unwrap_or(1.0);
                let (x, y) = match rect.position {
                    Position::Physical(p) => (p.x as f64, p.y as f64),
                    Position::Logical(p) => (p.x * scale, p.y * scale),
                };
                let (w, h) = match rect.size {
                    Size::Physical(s) => (s.width as f64, s.height as f64),
                    Size::Logical(s) => (s.width * scale, s.height * scale),
                };
                windows::toggle_popover(app, Some(Anchor { x, y, w, h }));
            }
        })
        .on_menu_event(move |app, event| handle_menu(app, &engine_for_menu, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

fn handle_menu(app: &AppHandle, engine: &Engine, id: &str) {
    match id {
        "open" => windows::show_popover(app),
        // window creation inside a menu event handler (main thread) deadlocks on Windows
        "settings" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { windows::open_settings(&app) });
        }
        "refresh" => {
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move { engine.poll_all(true).await });
        }
        "add" => {
            windows::show_popover(app);
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                let result = engine.start_login(None).await;
                engine.report(result);
            });
        }
        "quit" => app.exit(0),
        "update" => {
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                if engine.update_view().status == "ready" {
                    crate::updater::install(&engine).await;
                } else {
                    crate::updater::check(&engine, true).await;
                }
            });
        }
        other => {
            if let Some(account_id) = other.strip_prefix("acct:") {
                // one-click switch from the menu pins the account until the next manual switch
                let engine = engine.clone();
                let account_id = account_id.to_string();
                tauri::async_runtime::spawn(async move {
                    let result = engine.switch_to(&account_id, HoldSpec::Indefinite, None, None).await;
                    engine.report(result);
                });
            }
        }
    }
}

fn build_menu(app: &AppHandle, engine: &Engine) -> tauri::Result<Menu<tauri::Wry>> {
    let snap = engine.snapshot();
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "open", "Open FlipOn", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for a in &snap.accounts {
        let name = a.alias.clone().unwrap_or_else(|| a.email.clone());
        let usage = a
            .windows
            .iter()
            .filter(|w| w.key == "five_hour" || w.key == "seven_day")
            .map(|w| w.utilization)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.max(v))));
        let label = match usage {
            Some(p) => format!("{name} — {p:.0}%"),
            None => name,
        };
        let active = snap.active_id.as_deref() == Some(a.id.as_str());
        menu.append(&CheckMenuItem::with_id(app, format!("acct:{}", a.id), label, true, active, None::<&str>)?)?;
    }
    if !snap.accounts.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(app, "add", "Add account…", snap.cli.found && !snap.login.running && !snap.login.adding, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "refresh", "Refresh usage", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let (update_label, update_enabled) = update_menu_item(&snap.update, &snap.app.version);
    menu.append(&MenuItem::with_id(app, "update", update_label, update_enabled, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit FlipOn", true, None::<&str>)?)?;
    Ok(menu)
}

/// The tray's update line: what is going on, and whether clicking it does something.
fn update_menu_item(u: &crate::model::UpdateView, current: &str) -> (String, bool) {
    let v = u.version.as_deref().unwrap_or("");
    match u.status {
        "checking" => ("Checking for updates…".into(), false),
        "downloading" => (format!("Downloading version {v}…"), false),
        "ready" => (format!("Restart to update to {v}"), true),
        "installing" => ("Installing update…".into(), false),
        "up_to_date" => (format!("Up to date (v{current}) — check again"), true),
        "error" => ("Update check failed — try again".into(), true),
        _ => ("Check for updates".into(), true),
    }
}

fn fingerprint(engine: &Engine) -> String {
    let snap = engine.snapshot();
    let mut s = format!(
        "{:?}|{}|{}|{}|{}|{:?}",
        snap.active_id, snap.cli.found, snap.login.running, snap.login.adding, snap.update.status, snap.update.version
    );
    for a in &snap.accounts {
        let p = a
            .windows
            .iter()
            .filter(|w| w.key == "five_hour" || w.key == "seven_day")
            .map(|w| w.utilization.round() as i64)
            .max()
            .unwrap_or(-1);
        s.push_str(&format!("|{}:{}:{}", a.id, a.alias.as_deref().unwrap_or(&a.email), p));
    }
    s
}

/// Re-render the icon, tooltip and (if needed) the menu from the engine's current state.
pub fn update(app: &AppHandle, engine: &Engine) {
    let Some(tray): Option<TrayIcon> = app.tray_by_id(TRAY_ID) else { return };
    let settings = engine.settings();
    let data = engine.icon_data();
    let rendered = tray_icon::render(data.as_ref(), &settings, tray_icon::light_taskbar());
    let _ = tray.set_icon(Some(Image::new_owned(rendered.rgba, rendered.size, rendered.size)));

    let tooltip = match &data {
        Some(d) => {
            let part = |label: &str, v: Option<f64>| v.map(|v| format!(" · {label} {v:.0}%")).unwrap_or_default();
            format!("FlipOn — {}{}{}", d.name, part("5h", d.five), part("wk", d.seven))
        }
        None => "FlipOn".to_string(),
    };
    let _ = tray.set_tooltip(Some(format!("{}{tooltip}", crate::windows::DEV_MARK)));

    let fp = fingerprint(engine);
    let changed = {
        let mut last = LAST_MENU.lock();
        if *last != fp {
            *last = fp;
            true
        } else {
            false
        }
    };
    if changed {
        if let Ok(menu) = build_menu(app, engine) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
