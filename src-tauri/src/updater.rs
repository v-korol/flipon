//! Self-update from our own release server (api.korol.app), never from a third-party host.
//!
//! The endpoint and the public key live in `tauri.conf.json` (`plugins.updater`). Every update is
//! signed with the private key at build time and the plugin refuses anything whose signature does
//! not match, so even a compromised server cannot push a foreign build.
//!
//! Flow: check shortly after start and then every few hours → download and verify in the
//! background → "Version X is ready" in the popover and tray. It is installed when the user
//! clicks "Restart to update", or by itself once the app is idle if "Install updates
//! automatically" is on.

use crate::engine::{now_ms, Engine};
use std::sync::Arc;
use std::time::Duration;
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
const IDLE_INSTALL_POLL: Duration = Duration::from_secs(60);

/// A downloaded, verified update waiting to be installed.
#[derive(Default, Clone)]
pub struct Pending(Arc<Mutex<Option<(Update, Vec<u8>)>>>);

fn pending(engine: &Engine) -> Pending {
    use tauri::Manager;
    engine.app().state::<Pending>().inner().clone()
}

/// Ask the server whether there is a newer version and, if so, download it.
/// `manual` only changes how failures are reported (an error banner instead of a quiet status).
pub async fn check(engine: &Engine, manual: bool) {
    // A development copy must not ask the server: it would show up next to the installed app in the
    // usage numbers. Set FLIPON_CHECK_UPDATES=1 to test the updater from a development build.
    #[cfg(debug_assertions)]
    if std::env::var_os("FLIPON_CHECK_UPDATES").is_none() {
        if manual {
            engine.set_update(|u| {
                u.status = "error";
                u.error = Some("Update checks are off in development builds.".into());
            });
        }
        return;
    }
    let busy = matches!(engine.update_view().status, "checking" | "downloading" | "ready" | "installing");
    if busy {
        return;
    }
    engine.set_update(|u| {
        u.status = "checking";
        u.error = None;
        u.progress = None;
    });

    let result = async {
        // The server counts active copies from this token (new in 0.2.1); older versions are counted by address.
        let builder = engine
            .app()
            .updater_builder()
            .header("X-FlipOn-Day", engine.usage_token())
            .map_err(|e| e.to_string())?;
        // Development builds tell the server so, and it leaves them out of the usage numbers.
        #[cfg(debug_assertions)]
        let builder = builder.header("X-FlipOn-Dev", "1").map_err(|e| e.to_string())?;
        let updater = builder.build().map_err(|e| e.to_string())?;
        updater.check().await.map_err(|e| e.to_string())
    }
    .await;

    let update = match result {
        Ok(Some(update)) => update,
        Ok(None) => {
            engine.set_update(|u| {
                u.status = "up_to_date";
                u.version = None;
                u.notes = None;
                u.checked_at_ms = Some(now_ms());
            });
            return;
        }
        Err(e) => {
            fail(engine, format!("Update check failed: {e}"), manual);
            return;
        }
    };

    let version = update.version.clone();
    let notes = update.body.clone().filter(|n| !n.trim().is_empty());
    engine.set_update(|u| {
        u.status = "downloading";
        u.version = Some(version.clone());
        u.notes = notes.clone();
        u.progress = Some(0.0);
        u.checked_at_ms = Some(now_ms());
    });

    let progress_engine = engine.clone();
    let mut received: u64 = 0;
    let mut last_step: u64 = 0;
    let downloaded = update
        .download(
            move |chunk, total| {
                received += chunk as u64;
                if let Some(total) = total.filter(|t| *t > 0) {
                    // repaint in 5% steps, not on every chunk
                    let step = received * 20 / total;
                    if step != last_step {
                        last_step = step;
                        let fraction = (received as f64 / total as f64).min(1.0);
                        progress_engine.set_update(|u| u.progress = Some(fraction));
                    }
                }
            },
            || {},
        )
        .await;

    match downloaded {
        Ok(bytes) => {
            *pending(engine).0.lock().await = Some((update, bytes));
            engine.set_update(|u| {
                u.status = "ready";
                u.progress = None;
            });
        }
        Err(e) => fail(engine, format!("Could not download version {version}: {e}"), manual),
    }
}

fn fail(engine: &Engine, message: String, manual: bool) {
    eprintln!("[flipon] {message}");
    engine.set_update(|u| {
        u.status = "error";
        u.progress = None;
        u.error = Some(message.clone());
        u.checked_at_ms = Some(now_ms());
    });
    if manual {
        engine.set_error(message);
    }
}

/// Install the downloaded update and restart. On Windows the installer takes over and the app exits.
pub async fn install(engine: &Engine) {
    let Some((update, bytes)) = pending(engine).0.lock().await.take() else {
        engine.set_error("No update is ready to install yet.");
        return;
    };
    engine.set_update(|u| u.status = "installing");
    match update.install(bytes) {
        Ok(()) => engine.app().restart(),
        Err(e) => {
            let message = format!("Could not install the update: {e}");
            fail(engine, message, true);
        }
    }
}

/// Periodic checks, plus installing by itself when the user opted in and nothing is going on.
pub fn start_background(engine: &Engine) {
    let engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        let mut last_check = std::time::Instant::now();
        check(&engine, false).await;
        loop {
            tokio::time::sleep(IDLE_INSTALL_POLL).await;
            if last_check.elapsed() >= CHECK_EVERY {
                last_check = std::time::Instant::now();
                check(&engine, false).await;
            }
            let ready = engine.update_view().status == "ready";
            if ready && engine.settings().auto_update && engine.is_idle_for_restart() {
                install(&engine).await;
            }
        }
    });
}
