mod activity;
mod api;
mod claude_creds;
mod cli;
mod commands;
mod engine;
mod model;
mod store;
mod strategy;
mod tray;
mod tray_icon;
mod updater;
mod windows;

use engine::Engine;
use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // launching FlipOn again just brings the popover up
            windows::show_popover(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let engine = Engine::new(handle.clone());
            app.manage(engine.clone());
            app.manage(updater::Pending::default());
            windows::create_popover(&handle)?;
            tray::build(&handle, &engine)?;
            engine.sync_autostart_setting();
            engine.start_background();
            updater::start_background(&engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::add_account,
            commands::relogin_account,
            commands::cancel_login,
            commands::submit_login_code,
            commands::switch_account,
            commands::rename_account,
            commands::remove_account,
            commands::reorder_account,
            commands::refresh_usage,
            commands::update_settings,
            commands::detect_cli,
            commands::save_unknown_login,
            commands::release_hold,
            commands::dismiss_error,
            commands::open_settings,
            commands::hide_popover,
            commands::resize_popover,
            commands::open_url,
            commands::check_updates,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building FlipOn");

    app.run(|_app, event| {
        // FlipOn lives in the tray: closing the Settings window must not quit the app.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
