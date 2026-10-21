//! The two windows: the frameless popover anchored to the tray icon, and the Settings window.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

/// Prefix for window titles and the tray tooltip of a development build.
pub const DEV_MARK: &str = if cfg!(debug_assertions) { "[DEV] " } else { "" };

pub const POPOVER: &str = "popover";
pub const SETTINGS: &str = "settings";
pub const POPOVER_WIDTH: f64 = 380.0;
const POPOVER_MIN_HEIGHT: f64 = 120.0;
const EDGE_GAP: f64 = 8.0;

/// Physical-pixel rectangle of the tray icon.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Default)]
pub struct WindowState {
    anchor: Mutex<Option<Anchor>>,
    /// Epoch millis when the popover last hid itself because it lost focus.
    hidden_at: AtomicI64,
}

fn state(app: &AppHandle) -> tauri::State<'_, WindowState> {
    app.state::<WindowState>()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn create_popover(app: &AppHandle) -> tauri::Result<()> {
    app.manage(WindowState::default());
    let win = WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("index.html?w=popover".into()))
        .title(format!("{DEV_MARK}FlipOn"))
        .inner_size(POPOVER_WIDTH, 260.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        // macOS: open on the current Space, also over a full-screen app (no effect on Windows)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .build()?;

    let app_for_events = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            // The popover hides itself when you click anywhere else; remember when, so the tray click
            // that caused the blur doesn't immediately reopen it.
            if let Some(w) = app_for_events.get_webview_window(POPOVER) {
                if w.is_visible().unwrap_or(false) {
                    let _ = w.hide();
                    state(&app_for_events).hidden_at.store(now_ms(), Ordering::SeqCst);
                }
            }
        }
    });
    Ok(())
}

pub fn set_anchor(app: &AppHandle, anchor: Anchor) {
    *state(app).anchor.lock() = Some(anchor);
}

/// Left-click on the tray icon.
pub fn toggle_popover(app: &AppHandle, anchor: Option<Anchor>) {
    if let Some(a) = anchor {
        set_anchor(app, a);
    }
    let Some(win) = app.get_webview_window(POPOVER) else { return };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    if now_ms() - state(app).hidden_at.load(Ordering::SeqCst) < 350 {
        return; // click-through from the blur that just closed it
    }
    show_popover(app);
}

pub fn show_popover(app: &AppHandle) {
    let Some(win) = app.get_webview_window(POPOVER) else { return };
    place(app, &win);
    let _ = win.show();
    let _ = win.set_focus();
}

pub fn hide_popover(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(POPOVER) {
        let _ = win.hide();
    }
}

/// The frontend reports its content height (CSS px); fit the window and re-anchor it.
pub fn resize_popover(app: &AppHandle, height: f64) {
    let Some(win) = app.get_webview_window(POPOVER) else { return };
    let max = work_area_for(app, &win).map(|(_, _, _, h, scale)| h / scale - 2.0 * EDGE_GAP).unwrap_or(700.0);
    let h = height.clamp(POPOVER_MIN_HEIGHT, max.max(POPOVER_MIN_HEIGHT));
    let _ = win.set_size(LogicalSize::new(POPOVER_WIDTH, h));
    if win.is_visible().unwrap_or(false) {
        place(app, &win);
    }
}

/// (x, y, width, height, scale) of the monitor work area (physical px) the popover belongs on.
fn work_area_for(app: &AppHandle, win: &WebviewWindow) -> Option<(f64, f64, f64, f64, f64)> {
    let anchor = *state(app).anchor.lock();
    let monitor = anchor
        .and_then(|a| app.monitor_from_point(a.x + a.w / 2.0, a.y + a.h / 2.0).ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten())?;
    let wa = monitor.work_area();
    Some((
        wa.position.x as f64,
        wa.position.y as f64,
        wa.size.width as f64,
        wa.size.height as f64,
        monitor.scale_factor(),
    ))
}

fn place(app: &AppHandle, win: &WebviewWindow) {
    let Ok(size) = win.outer_size() else { return };
    let (w, h) = (size.width as f64, size.height as f64);
    let Some((wx, wy, ww, wh, scale)) = work_area_for(app, win) else { return };
    let gap = EDGE_GAP * scale;
    let anchor = *state(app).anchor.lock();

    let (x, y) = match anchor {
        Some(a) => {
            let cx = a.x + a.w / 2.0;
            let above = a.y + a.h / 2.0 > wy + wh / 2.0;
            let y = if above { a.y - h - gap } else { a.y + a.h + gap };
            (cx - w / 2.0, y)
        }
        // no tray position yet (e.g. opened from the menu): bottom-right corner
        None => (wx + ww - w - gap, wy + wh - h - gap),
    };
    let x = x.clamp(wx + gap, (wx + ww - w - gap).max(wx + gap));
    let y = y.clamp(wy + gap, (wy + wh - h - gap).max(wy + gap));
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

pub fn open_settings(app: &AppHandle) {
    hide_popover(app);
    if let Some(win) = app.get_webview_window(SETTINGS) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("index.html?w=settings".into()))
        .title(format!("{DEV_MARK}FlipOn Settings"))
        .inner_size(760.0, 560.0)
        .min_inner_size(760.0, 560.0)
        .center()
        .resizable(true)
        .maximizable(false)
        .build();
    match built {
        Ok(win) => {
            let _ = win.set_focus();
        }
        Err(e) => eprintln!("[flipon] could not open settings: {e}"),
    }
}
