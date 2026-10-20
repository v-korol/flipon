//! Tauri commands exposed to the frontend. Failures become the popover's error banner.

use crate::engine::{Engine, HoldSpec};
use crate::model::{CliInfo, Settings, Snapshot};
use crate::windows;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn get_snapshot(engine: State<'_, Engine>) -> Snapshot {
    engine.snapshot()
}

#[tauri::command]
pub async fn add_account(engine: State<'_, Engine>) -> Result<(), String> {
    let result = engine.start_login(None).await;
    engine.report(result);
    Ok(())
}

#[tauri::command]
pub async fn relogin_account(engine: State<'_, Engine>, id: String) -> Result<(), String> {
    let result = engine.start_login(Some(id)).await;
    engine.report(result);
    Ok(())
}

#[tauri::command]
pub fn cancel_login(engine: State<'_, Engine>) {
    engine.cancel_login();
}

#[tauri::command]
pub async fn submit_login_code(engine: State<'_, Engine>, code: String) -> Result<(), String> {
    let result = engine.submit_login_code(code).await;
    engine.report(result);
    Ok(())
}

/// `hours: null` is the "No timer" choice: pin the account until the next manual switch.
#[tauri::command]
pub async fn switch_account(engine: State<'_, Engine>, id: String, hours: Option<f64>) -> Result<(), String> {
    let hold = match hours {
        Some(h) if h > 0.0 => HoldSpec::Hours(h),
        _ => HoldSpec::Indefinite,
    };
    let result = engine.switch_to(&id, hold, None, None).await;
    engine.report(result);
    Ok(())
}

#[tauri::command]
pub fn rename_account(engine: State<'_, Engine>, id: String, alias: String) {
    engine.rename(&id, &alias);
}

#[tauri::command]
pub fn remove_account(engine: State<'_, Engine>, id: String) {
    engine.remove(&id);
}

#[tauri::command]
pub fn reorder_account(engine: State<'_, Engine>, id: String, delta: i32) {
    engine.reorder(&id, delta);
}

#[tauri::command]
pub async fn refresh_usage(engine: State<'_, Engine>) -> Result<(), String> {
    engine.poll_all(true).await;
    Ok(())
}

#[tauri::command]
pub fn update_settings(engine: State<'_, Engine>, settings: Settings) {
    engine.update_settings(settings);
}

#[tauri::command]
pub async fn detect_cli(engine: State<'_, Engine>) -> Result<CliInfo, String> {
    Ok(engine.detect_cli().await)
}

#[tauri::command]
pub async fn save_unknown_login(engine: State<'_, Engine>) -> Result<(), String> {
    engine.save_unknown().await;
    Ok(())
}

#[tauri::command]
pub fn release_hold(engine: State<'_, Engine>) {
    engine.release_hold();
}

#[tauri::command]
pub fn dismiss_error(engine: State<'_, Engine>) {
    engine.dismiss_error();
}

/// Async on purpose: building a window inside a *sync* command deadlocks the main thread on Windows.
#[tauri::command]
pub async fn open_settings(app: AppHandle) {
    windows::open_settings(&app);
}

#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    windows::hide_popover(&app);
}

#[tauri::command]
pub fn resize_popover(app: AppHandle, height: f64) {
    windows::resize_popover(&app, height);
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_updates(engine: State<'_, Engine>) -> Result<(), String> {
    crate::updater::check(&engine, true).await;
    Ok(())
}

#[tauri::command]
pub async fn install_update(engine: State<'_, Engine>) -> Result<(), String> {
    crate::updater::install(&engine).await;
    Ok(())
}
