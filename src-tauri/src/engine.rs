//! The app's brain: account list, active-account tracking, usage polling, switching and the login flow.
//! The UI is a pure view over [`Snapshot`]s emitted from here.

use crate::activity::{self, Scanner};
use crate::api::{self, ApiError};
use crate::claude_creds::{self, CliLogin};
use crate::cli::{self, LoginEvent, LoginProc};
use crate::model::*;
use crate::store::Store;
use crate::strategy::{self, Candidate};
use crate::tray_icon::IconData;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

pub const STATE_EVENT: &str = "flipon://state";
const FIVE_HOURS_MS: i64 = 5 * 3_600_000;
const SEVEN_DAYS_MS: i64 = 7 * 24 * 3_600_000;
const POLL_EVERY: Duration = Duration::from_secs(90);
const TICK_EVERY: Duration = Duration::from_secs(15);
const MIN_SWITCH_GAP: Duration = Duration::from_secs(45);

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Copy, Debug)]
pub enum HoldSpec {
    None,
    Hours(f64),
    Indefinite,
}

struct Inner {
    settings: Settings,
    state: PersistedState,
    cli: CliInfo,
    cli_path: Option<PathBuf>,
    active_id: Option<String>,
    unknown_login: Option<String>,
    login: LoginView,
    login_proc: Option<LoginProc>,
    login_cancelled: bool,
    error: Option<ErrorView>,
    error_seq: u64,
    refreshing: bool,
    today_entries: Vec<activity::Entry>,
    warned: HashSet<String>,
    last_switch: Option<Instant>,
    last_poll: Option<Instant>,
    update: UpdateView,
}

struct Shared {
    app: AppHandle,
    store: Store,
    http: reqwest::Client,
    inner: Mutex<Inner>,
    switch_lock: tokio::sync::Mutex<()>,
    scanner: Mutex<Scanner>,
}

#[derive(Clone)]
pub struct Engine(Arc<Shared>);

impl Engine {
    pub fn new(app: AppHandle) -> Self {
        let store = Store::new();
        let settings = store.load_settings();
        let mut state = store.load_state();
        // accounts whose credentials are missing never completed authentication through FlipOn
        for a in &mut state.accounts {
            a.no_backup = store.read_cred(&a.id).is_none();
        }
        let cli_path = cli::find_claude(&settings.cli_path);
        let cli = CliInfo {
            found: cli_path.is_some(),
            version: None,
            path: cli_path.as_ref().map(|p| p.display().to_string()),
        };
        let inner = Inner {
            settings,
            state,
            cli,
            cli_path,
            active_id: None,
            unknown_login: None,
            login: LoginView::default(),
            login_proc: None,
            login_cancelled: false,
            error: None,
            error_seq: 0,
            refreshing: false,
            today_entries: Vec::new(),
            warned: HashSet::new(),
            last_switch: None,
            last_poll: None,
            update: UpdateView::default(),
        };
        Engine(Arc::new(Shared {
            app,
            store,
            http: api::client(),
            inner: Mutex::new(inner),
            switch_lock: tokio::sync::Mutex::new(()),
            scanner: Mutex::new(Scanner::default()),
        }))
    }

    pub fn app(&self) -> &AppHandle {
        &self.0.app
    }

    pub fn update_view(&self) -> UpdateView {
        self.0.inner.lock().update.clone()
    }

    /// Change the updater state and push it to the UI and tray.
    pub fn set_update(&self, change: impl FnOnce(&mut UpdateView)) {
        change(&mut self.0.inner.lock().update);
        self.emit();
    }

    /// Nothing is in progress that a restart would interrupt: no login, no switch, no open window.
    pub fn is_idle_for_restart(&self) -> bool {
        use tauri::Manager;
        if self.0.inner.lock().login.running || self.0.switch_lock.try_lock().is_err() {
            return false;
        }
        let popover_open = self
            .0
            .app
            .get_webview_window(crate::windows::POPOVER)
            .is_some_and(|w| w.is_visible().unwrap_or(false));
        let settings_open = self.0.app.get_webview_window(crate::windows::SETTINGS).is_some();
        !popover_open && !settings_open
    }

    /// The anonymous once-a-day token the update check sends (see `store::day_token`).
    pub fn usage_token(&self) -> String {
        crate::store::day_token(&self.0.store.install_id())
    }

    pub fn settings(&self) -> Settings {
        self.0.inner.lock().settings.clone()
    }

    // ------------------------------------------------------------------ snapshot

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.0.inner.lock();
        let now = now_ms();
        let accounts = inner
            .state
            .accounts
            .iter()
            .map(|a| self.account_view(&inner, a, now))
            .collect();
        Snapshot {
            accounts,
            active_id: inner.active_id.clone(),
            settings: inner.settings.clone(),
            cli: inner.cli.clone(),
            login: inner.login.clone(),
            hold: inner.state.hold.clone().filter(|h| h.until_ms.is_none_or(|t| t > now)),
            unknown_login: inner.unknown_login.clone(),
            refreshing: inner.refreshing,
            error: inner.error.clone(),
            app: AppInfo {
                version: env!("CARGO_PKG_VERSION").to_string(),
                platform: std::env::consts::OS.to_string(),
                dev: cfg!(debug_assertions),
            },
            update: inner.update.clone(),
            now_ms: now,
        }
    }

    fn account_view(&self, inner: &Inner, a: &AccountMeta, now: i64) -> AccountView {
        let usage = inner.state.last_usage.get(&a.id);
        let samples = inner.state.history.get(&a.id).map(Vec::as_slice).unwrap_or(&[]);
        let is_active = inner.active_id.as_deref() == Some(a.id.as_str());
        let th = inner.settings.switch_threshold as f64;

        let windows: Vec<WindowView> = usage
            .map(|u| {
                u.windows
                    .iter()
                    .map(|w| {
                        // a window whose reset time has passed since we last looked is empty again
                        let expired = w.resets_at_ms.is_some_and(|r| r <= now);
                        let utilization = if expired { 0.0 } else { w.utilization };
                        let (label, window_ms) = match w.key.as_str() {
                            "five_hour" => ("Session", FIVE_HOURS_MS),
                            "seven_day" => ("Weekly", SEVEN_DAYS_MS),
                            "seven_day_opus" => ("Opus", SEVEN_DAYS_MS),
                            "seven_day_sonnet" => ("Sonnet", SEVEN_DAYS_MS),
                            _ => ("Other", SEVEN_DAYS_MS),
                        };
                        WindowView {
                            key: w.key.clone(),
                            label: label.into(),
                            utilization,
                            resets_at_ms: if expired { None } else { w.resets_at_ms },
                            window_ms,
                            trend: if expired { None } else { trend_for(samples, &w.key, utilization, now) },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let util = |key: &str| windows.iter().find(|w| w.key == key);
        let five = util("five_hour");
        let seven = util("seven_day");
        let binding = five.map_or(0.0, |w| w.utilization).max(seven.map_or(0.0, |w| w.utilization));

        let (status, status_resets) = if a.needs_login {
            ("needs_login", None)
        } else if a.no_backup && !is_active {
            ("no_backup", None)
        } else if is_active {
            ("active", None)
        } else if binding >= th {
            let week_binding = seven.is_some_and(|w| w.utilization >= th);
            if week_binding {
                ("week_full", seven.and_then(|w| w.resets_at_ms))
            } else {
                ("five_hour_full", five.and_then(|w| w.resets_at_ms))
            }
        } else {
            ("ready", None)
        };

        // Every account gets its own share of today's activity, attributed by who was active when it
        // happened. Without a switch log nothing can be attributed to an inactive account.
        let today = if is_active || !inner.state.switch_log.is_empty() {
            self.today_view(inner, &a.id)
        } else {
            None
        };

        AccountView {
            id: a.id.clone(),
            email: a.email.clone(),
            alias: a.alias.clone(),
            plan: plan_label(&a.subscription, &a.rate_limit_tier),
            status,
            status_resets_at_ms: status_resets,
            windows,
            today,
        }
    }

    fn today_view(&self, inner: &Inner, id: &str) -> Option<TodayView> {
        if inner.today_entries.is_empty() {
            return None;
        }
        let view = activity::aggregate(&inner.today_entries, id, &inner.state.switch_log);
        (view.turns > 0 || view.cost > 0.0 || view.lines > 0).then_some(view)
    }

    /// Data for the tray icon: the active account's readings.
    pub fn icon_data(&self) -> Option<IconData> {
        let inner = self.0.inner.lock();
        let id = inner.active_id.as_ref()?;
        let acc = inner.state.accounts.iter().find(|a| &a.id == id)?;
        let usage = inner.state.last_usage.get(id)?;
        let now = now_ms();
        let live = |key: &str| {
            usage.windows.iter().find(|w| w.key == key).map(|w| {
                if w.resets_at_ms.is_some_and(|r| r <= now) { (0.0, None) } else { (w.utilization, w.resets_at_ms) }
            })
        };
        let five = live("five_hour");
        let seven = live("seven_day");
        let samples = inner.state.history.get(id).map(Vec::as_slice).unwrap_or(&[]);
        Some(IconData {
            name: acc.alias.clone().unwrap_or_else(|| acc.email.split('@').next().unwrap_or("").to_string()),
            five: five.map(|x| x.0),
            seven: seven.map(|x| x.0),
            five_reset_ms: five.and_then(|x| x.1),
            seven_reset_ms: seven.and_then(|x| x.1),
            cost_today: self.today_view(&inner, id).map(|t| t.cost),
            history: history_points(samples, now),
            now_ms: now,
            critical_at: inner.settings.switch_threshold as f64,
        })
    }

    pub fn emit(&self) {
        let snap = self.snapshot();
        let _ = self.0.app.emit(STATE_EVENT, &snap);
        crate::tray::update(&self.0.app, self);
    }

    fn persist(&self) {
        let state = self.0.inner.lock().state.clone();
        self.0.store.save_state(&state);
    }

    // ------------------------------------------------------------------ errors

    pub fn set_error(&self, message: impl Into<String>) {
        let id = {
            let mut inner = self.0.inner.lock();
            inner.error_seq += 1;
            let id = inner.error_seq;
            inner.error = Some(ErrorView { id, message: message.into() });
            id
        };
        self.emit();
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(8)).await;
            let cleared = {
                let mut inner = this.0.inner.lock();
                if inner.error.as_ref().is_some_and(|e| e.id == id) {
                    inner.error = None;
                    true
                } else {
                    false
                }
            };
            if cleared {
                this.emit();
            }
        });
    }

    pub fn dismiss_error(&self) {
        self.0.inner.lock().error = None;
        self.emit();
    }

    /// Run a fallible action; failures become the popover's error banner.
    pub fn report(&self, result: Result<(), String>) {
        if let Err(e) = result {
            self.set_error(e);
        }
    }

    // ------------------------------------------------------------------ CLI login <-> saved accounts

    /// Make the CLI's current login the source of truth for "which account is active".
    pub fn sync_with_cli(&self) {
        let login = claude_creds::read_cli_login().ok().flatten();
        let email = login.as_ref().and_then(CliLogin::email);

        // keep the stored copy of the active account fresh: the CLI refreshes tokens on its own
        if let (Some(login), Some(email)) = (&login, &email) {
            let id = self
                .0
                .inner
                .lock()
                .state
                .accounts
                .iter()
                .find(|a| a.email.to_lowercase() == *email)
                .map(|a| a.id.clone());
            if let Some(id) = id {
                self.store_login(&id, login);
            }
        }

        let (changed, persist_needed) = {
            let mut inner = self.0.inner.lock();
            let previous = inner.active_id.clone();
            match &email {
                Some(e) => {
                    let found = inner.state.accounts.iter().find(|a| a.email.to_lowercase() == *e).map(|a| a.id.clone());
                    match found {
                        Some(id) => {
                            inner.active_id = Some(id);
                            inner.unknown_login = None;
                        }
                        None => {
                            inner.active_id = None;
                            inner.unknown_login = if inner.state.ignored_unknown.as_deref() == Some(e.as_str()) {
                                None
                            } else {
                                Some(e.clone())
                            };
                        }
                    }
                }
                None => {
                    inner.active_id = None;
                    inner.unknown_login = None;
                }
            }
            let mut dirty = false;
            if inner.state.ignored_unknown.is_some() && inner.unknown_login.is_none() && email.is_none() {
                inner.state.ignored_unknown = None;
                dirty = true;
            }
            let changed = previous != inner.active_id;
            if changed {
                if let Some(id) = inner.active_id.clone() {
                    inner.state.switch_log.push(SwitchEvent { t: now_ms(), account_id: id.clone() });
                    let excess = inner.state.switch_log.len().saturating_sub(200);
                    inner.state.switch_log.drain(..excess);
                    inner.state.last_active = Some(id.clone());
                    if inner.state.hold.as_ref().is_some_and(|h| h.account_id != id) {
                        inner.state.hold = None;
                    }
                }
                inner.warned.clear();
                dirty = true;
            }
            // expire timed holds
            let now = now_ms();
            if inner.state.hold.as_ref().is_some_and(|h| h.until_ms.is_some_and(|t| t <= now)) {
                inner.state.hold = None;
                dirty = true;
            }
            (changed, dirty)
        };
        if persist_needed {
            self.persist();
        }
        if changed {
            self.emit();
        }
    }

    /// Save the CLI's tokens into the slot of the account they belong to.
    fn store_login(&self, id: &str, login: &CliLogin) {
        let stored = self.0.store.read_cred(id);
        let same = stored.as_ref().and_then(|s| s.get("accessToken")) == login.oauth.get("accessToken")
            && stored.as_ref().and_then(|s| s.get("refreshToken")) == login.oauth.get("refreshToken");
        if same {
            return;
        }
        if let Err(e) = self.0.store.write_cred(id, &login.oauth) {
            eprintln!("[flipon] {e}");
            return;
        }
        let mut inner = self.0.inner.lock();
        if let Some(a) = inner.state.accounts.iter_mut().find(|a| a.id == id) {
            a.subscription = login.subscription();
            a.rate_limit_tier = login.rate_limit_tier();
            if login.oauth_account.is_some() {
                a.oauth_account = login.oauth_account.clone();
            }
            a.needs_login = false;
            a.no_backup = false;
        }
    }

    /// Capture whatever the CLI currently holds into its saved account (before it is replaced).
    fn capture_active(&self) {
        if let Ok(Some(login)) = claude_creds::read_cli_login() {
            if let Some(email) = login.email() {
                let id = self
                    .0
                    .inner
                    .lock()
                    .state
                    .accounts
                    .iter()
                    .find(|a| a.email.to_lowercase() == email)
                    .map(|a| a.id.clone());
                if let Some(id) = id {
                    self.store_login(&id, &login);
                    self.persist();
                }
            }
        }
    }

    /// Add (or refresh) the account the CLI is currently logged in as, and make it the active one.
    async fn adopt_cli_login(&self, expected_target: Option<&str>) -> Result<(), String> {
        self.0.inner.lock().login.adding = true;
        self.emit();
        let result = self.adopt_cli_login_inner(expected_target).await;
        self.0.inner.lock().login.adding = false;
        self.emit();
        result
    }

    async fn adopt_cli_login_inner(&self, expected_target: Option<&str>) -> Result<(), String> {
        let mut login = claude_creds::read_cli_login()?
            .ok_or("The Claude CLI is not logged in.")?;
        if login.email().is_none() {
            // fall back to `claude auth status`
            let path = self.0.inner.lock().cli_path.clone();
            if let Some(email) = match path {
                Some(p) => cli_status_email(&p).await,
                None => None,
            } {
                let mut acct = login.oauth_account.take().unwrap_or_else(|| json!({}));
                acct["emailAddress"] = Value::String(email);
                login.oauth_account = Some(acct);
            }
        }
        let email = login.email().ok_or("Could not determine which account the CLI is logged in as.")?;

        let (id, expected_email) = {
            let mut inner = self.0.inner.lock();
            let expected_email = expected_target
                .and_then(|t| inner.state.accounts.iter().find(|a| a.id == t))
                .map(|a| a.email.clone());
            let existing = inner.state.accounts.iter().find(|a| a.email.to_lowercase() == email).map(|a| a.id.clone());
            let id = match existing {
                Some(id) => id,
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    inner.state.accounts.push(AccountMeta {
                        id: id.clone(),
                        email: email.clone(),
                        alias: None,
                        subscription: login.subscription(),
                        rate_limit_tier: login.rate_limit_tier(),
                        oauth_account: login.oauth_account.clone(),
                        needs_login: false,
                        no_backup: false,
                    });
                    id
                }
            };
            (id, expected_email)
        };
        self.0.store.write_cred(&id, &login.oauth)?;
        {
            let mut inner = self.0.inner.lock();
            if let Some(a) = inner.state.accounts.iter_mut().find(|a| a.id == id) {
                a.subscription = login.subscription();
                a.rate_limit_tier = login.rate_limit_tier();
                if login.oauth_account.is_some() {
                    a.oauth_account = login.oauth_account.clone();
                }
                a.needs_login = false;
                a.no_backup = false;
            }
            if inner.state.ignored_unknown.as_deref() == Some(email.as_str()) {
                inner.state.ignored_unknown = None;
            }
            inner.state.hold = None;
            inner.unknown_login = None;
        }
        self.persist();
        self.sync_with_cli();
        self.emit();

        // refresh this account's usage right away
        let this = self.clone();
        tauri::async_runtime::spawn(async move { this.poll_all(true).await });

        if let Some(expected) = expected_email {
            if expected.to_lowercase() != email {
                return Err(format!(
                    "You signed in as {email}, not {expected}. {email} was saved; {expected} still needs login."
                ));
            }
        }
        Ok(())
    }

    pub async fn save_unknown(&self) {
        let result = self.adopt_cli_login(None).await;
        self.report(result);
    }

    // ------------------------------------------------------------------ login flow

    pub async fn start_login(&self, target: Option<String>) -> Result<(), String> {
        let (path, email) = {
            let inner = self.0.inner.lock();
            if inner.login.running || inner.login.adding {
                return Err("A login is already in progress.".into());
            }
            let path = inner.cli_path.clone().ok_or("Claude Code CLI not found. Set its path in Settings.")?;
            let email = target
                .as_ref()
                .and_then(|t| inner.state.accounts.iter().find(|a| &a.id == t))
                .map(|a| a.email.clone());
            (path, email)
        };
        // the CLI login is about to be replaced: keep the current account's latest tokens
        self.capture_active();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let proc = cli::spawn_login(&path, email.as_deref(), tx)?;
        {
            let mut inner = self.0.inner.lock();
            inner.login = LoginView { running: true, adding: false, needs_code: false, url: None, target_id: target.clone() };
            inner.login_proc = Some(proc);
            inner.login_cancelled = false;
        }
        self.emit();

        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    LoginEvent::Url(url) => {
                        this.0.inner.lock().login.url = Some(url);
                        this.emit();
                    }
                    LoginEvent::NeedsCode => {
                        this.0.inner.lock().login.needs_code = true;
                        this.emit();
                    }
                    LoginEvent::Exited { success, message } => {
                        let (cancelled, target) = {
                            let mut inner = this.0.inner.lock();
                            let t = inner.login.target_id.clone();
                            // on success the account is saved next; keep "Add" disabled meanwhile
                            inner.login = LoginView { adding: success, ..LoginView::default() };
                            inner.login_proc = None;
                            (std::mem::take(&mut inner.login_cancelled), t)
                        };
                        this.emit();
                        if success {
                            let result = this.adopt_cli_login(target.as_deref()).await;
                            this.report(result);
                        } else if !cancelled {
                            let detail = if message.is_empty() { String::new() } else { format!(": {message}") };
                            this.set_error(format!("Login did not finish{detail}"));
                        }
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    pub fn cancel_login(&self) {
        let mut inner = self.0.inner.lock();
        inner.login_cancelled = true;
        if let Some(proc) = inner.login_proc.as_mut() {
            proc.cancel();
        }
    }

    pub async fn submit_login_code(&self, code: String) -> Result<(), String> {
        let writer = {
            let mut inner = self.0.inner.lock();
            inner.login.needs_code = false;
            inner.login_proc.as_ref().map(LoginProc::writer)
        };
        self.emit();
        match writer {
            Some(w) => w.submit(&code).await,
            None => Err("No login is in progress.".into()),
        }
    }

    // ------------------------------------------------------------------ account management

    pub fn rename(&self, id: &str, alias: &str) {
        {
            let mut inner = self.0.inner.lock();
            if let Some(a) = inner.state.accounts.iter_mut().find(|a| a.id == id) {
                let alias = alias.trim();
                a.alias = (!alias.is_empty()).then(|| alias.to_string());
            }
        }
        self.persist();
        self.emit();
    }

    pub fn remove(&self, id: &str) {
        {
            let mut inner = self.0.inner.lock();
            let removed = inner.state.accounts.iter().find(|a| a.id == id).map(|a| a.email.to_lowercase());
            inner.state.accounts.retain(|a| a.id != id);
            inner.state.history.remove(id);
            inner.state.last_usage.remove(id);
            if inner.state.hold.as_ref().is_some_and(|h| h.account_id == id) {
                inner.state.hold = None;
            }
            // removing only forgets FlipOn's copy; the CLI stays logged in and must not nag about it
            if inner.active_id.as_deref() == Some(id) {
                inner.state.ignored_unknown = removed;
                inner.active_id = None;
            }
        }
        self.0.store.delete_cred(id);
        self.persist();
        self.emit();
    }

    pub fn reorder(&self, id: &str, delta: i32) {
        {
            let mut inner = self.0.inner.lock();
            let accounts = &mut inner.state.accounts;
            if let Some(i) = accounts.iter().position(|a| a.id == id) {
                let j = i as i32 + delta;
                if j >= 0 && (j as usize) < accounts.len() {
                    accounts.swap(i, j as usize);
                }
            }
        }
        self.persist();
        self.emit();
    }

    pub fn release_hold(&self) {
        self.0.inner.lock().state.hold = None;
        self.persist();
        self.emit();
    }

    // ------------------------------------------------------------------ settings

    pub fn update_settings(&self, mut settings: Settings) {
        // flipping the switch is the user's choice from now on (see `Settings::auto_update_chosen`)
        let previous = self.0.inner.lock().settings.auto_update;
        settings.auto_update_chosen |= previous != settings.auto_update;
        settings.sanitize();
        let (autostart_changed, cli_changed) = {
            let mut inner = self.0.inner.lock();
            let a = inner.settings.start_on_login != settings.start_on_login;
            let c = inner.settings.cli_path != settings.cli_path;
            inner.settings = settings.clone();
            (a, c)
        };
        self.0.store.save_settings(&settings);
        if autostart_changed {
            use tauri_plugin_autostart::ManagerExt;
            let manager = self.0.app.autolaunch();
            let result = if settings.start_on_login { manager.enable() } else { manager.disable() };
            if let Err(e) = result {
                self.set_error(format!("Could not change the login item: {e}"));
            }
        }
        if cli_changed {
            let this = self.clone();
            tauri::async_runtime::spawn(async move {
                this.detect_cli().await;
            });
        }
        self.emit();
    }

    pub async fn detect_cli(&self) -> CliInfo {
        let custom = self.0.inner.lock().settings.cli_path.clone();
        let path = cli::find_claude(&custom);
        let version = match &path {
            Some(p) => cli::version(p).await,
            None => None,
        };
        let info = CliInfo {
            found: path.is_some(),
            version,
            path: path.as_ref().map(|p| p.display().to_string()),
        };
        {
            let mut inner = self.0.inner.lock();
            inner.cli_path = path;
            inner.cli = info.clone();
        }
        self.emit();
        info
    }

    pub fn sync_autostart_setting(&self) {
        use tauri_plugin_autostart::ManagerExt;
        if let Ok(enabled) = self.0.app.autolaunch().is_enabled() {
            let changed = {
                let mut inner = self.0.inner.lock();
                let changed = inner.settings.start_on_login != enabled;
                inner.settings.start_on_login = enabled;
                changed
            };
            if changed {
                let s = self.settings();
                self.0.store.save_settings(&s);
            }
        }
    }

    // ------------------------------------------------------------------ switching

    pub async fn switch_to(&self, id: &str, hold: HoldSpec, auto_reason: Option<String>, new_lap: Option<u32>) -> Result<(), String> {
        let _guard = self.0.switch_lock.lock().await;
        if self.0.inner.lock().login.running {
            return Err("Finish or cancel the login before switching.".into());
        }
        let meta = {
            let inner = self.0.inner.lock();
            inner.state.accounts.iter().find(|a| a.id == id).cloned()
        }
        .ok_or("That account is no longer saved.")?;

        // 1. don't lose tokens the CLI refreshed since we last looked at the outgoing account
        self.capture_active();

        // 2. credentials for the target — refresh them first if they are about to expire
        let mut oauth = self.0.store.read_cred(id).ok_or("No saved credentials for this account. Log in again.")?;
        let expires = oauth.get("expiresAt").and_then(Value::as_i64).unwrap_or(0);
        if expires - now_ms() < 120_000 {
            let refresh = oauth.get("refreshToken").and_then(Value::as_str).unwrap_or_default().to_string();
            match api::refresh_token(&self.0.http, &refresh).await {
                Ok(tokens) => {
                    oauth["accessToken"] = Value::String(tokens.access_token);
                    oauth["refreshToken"] = Value::String(tokens.refresh_token);
                    oauth["expiresAt"] = json!(tokens.expires_at_ms);
                    self.0.store.write_cred(id, &oauth)?;
                }
                Err(ApiError::InvalidGrant) => {
                    self.mark_needs_login(id);
                    return Err("That account's session expired. Choose \"Log in again\".".into());
                }
                Err(e) if expires <= now_ms() => {
                    return Err(format!("Could not refresh this account's session: {}", e.message()));
                }
                Err(_) => {} // still valid for a couple of minutes; switch anyway
            }
        }

        // 3. point the CLI at it
        claude_creds::write_cli_login(&CliLogin { oauth, oauth_account: meta.oauth_account.clone() })?;

        // 4. bookkeeping
        let now = now_ms();
        let name = display_name(&meta);
        {
            let mut inner = self.0.inner.lock();
            inner.active_id = Some(id.to_string());
            inner.unknown_login = None;
            inner.state.last_active = Some(id.to_string());
            inner.state.switch_log.push(SwitchEvent { t: now, account_id: id.to_string() });
            let excess = inner.state.switch_log.len().saturating_sub(200);
            inner.state.switch_log.drain(..excess);
            inner.state.hold = match hold {
                HoldSpec::None => None,
                HoldSpec::Indefinite => Some(Hold { account_id: id.to_string(), until_ms: None }),
                HoldSpec::Hours(h) => Some(Hold { account_id: id.to_string(), until_ms: Some(now + (h * 3_600_000.0) as i64) }),
            };
            if let Some(l) = new_lap {
                inner.state.rr_lap = l;
            }
            inner.warned.clear();
            inner.last_switch = Some(Instant::now());
        }
        self.persist();
        self.emit();

        if let Some(reason) = auto_reason {
            if self.settings().notify_on_switch {
                self.notify(&format!("Switched to {name}"), &reason);
            }
        }
        Ok(())
    }

    fn mark_needs_login(&self, id: &str) {
        {
            let mut inner = self.0.inner.lock();
            if let Some(a) = inner.state.accounts.iter_mut().find(|a| a.id == id) {
                a.needs_login = true;
            }
        }
        self.persist();
        self.emit();
    }

    fn notify(&self, title: &str, body: &str) {
        if let Err(e) = self.0.app.notification().builder().title(title).body(body).show() {
            eprintln!("[flipon] notification failed: {e}");
        }
    }

    // ------------------------------------------------------------------ usage polling

    /// Valid access token for an account, refreshing when needed.
    async fn access_token(&self, id: &str, force_refresh: bool) -> Result<String, ApiError> {
        let is_active = self.0.inner.lock().active_id.as_deref() == Some(id);
        let cli_login = if is_active { claude_creds::read_cli_login().ok().flatten() } else { None };
        let oauth = match &cli_login {
            Some(l) if l.email().is_some() => l.oauth.clone(),
            _ => self.0.store.read_cred(id).ok_or(ApiError::Other("No saved credentials".into()))?,
        };
        let access = oauth.get("accessToken").and_then(Value::as_str).unwrap_or_default().to_string();
        let expires = oauth.get("expiresAt").and_then(Value::as_i64).unwrap_or(0);
        let now = now_ms();

        let fresh_enough = expires - now > 60_000;
        // The CLI refreshes the active login itself while it's in use. Only step in once the token has
        // been expired for a while, i.e. no CLI session is running — otherwise we could race it.
        let active_and_recent = is_active && now - expires < 150_000;
        if !access.is_empty() && !force_refresh && (fresh_enough || active_and_recent) {
            return Ok(access);
        }

        let refresh = oauth.get("refreshToken").and_then(Value::as_str).unwrap_or_default().to_string();
        if refresh.is_empty() {
            return Err(ApiError::InvalidGrant);
        }
        if is_active {
            // someone (the CLI) may have refreshed in the meantime
            if let Ok(Some(l)) = claude_creds::read_cli_login() {
                if l.access_token().is_some_and(|t| t != access) && l.expires_at_ms().unwrap_or(0) - now > 60_000 {
                    return Ok(l.access_token().unwrap_or_default().to_string());
                }
            }
        }
        let tokens = api::refresh_token(&self.0.http, &refresh).await?;
        let mut updated = oauth.clone();
        updated["accessToken"] = Value::String(tokens.access_token.clone());
        updated["refreshToken"] = Value::String(tokens.refresh_token);
        updated["expiresAt"] = json!(tokens.expires_at_ms);
        if let Err(e) = self.0.store.write_cred(id, &updated) {
            eprintln!("[flipon] {e}");
        }
        if is_active {
            // keep the CLI in step with the rotated refresh token
            if let Some(mut l) = cli_login {
                l.oauth = updated;
                if let Err(e) = claude_creds::write_cli_login(&l) {
                    eprintln!("[flipon] {e}");
                }
            }
        }
        Ok(tokens.access_token)
    }

    async fn fetch_account_usage(&self, id: &str) -> Result<Vec<UsageWindow>, ApiError> {
        let token = self.access_token(id, false).await?;
        match api::fetch_usage(&self.0.http, &token).await {
            Err(ApiError::Unauthorized) => {
                let token = self.access_token(id, true).await?;
                api::fetch_usage(&self.0.http, &token).await
            }
            other => other,
        }
    }

    pub async fn poll_all(&self, force: bool) {
        {
            let mut inner = self.0.inner.lock();
            if inner.refreshing {
                return;
            }
            if !force && inner.last_poll.is_some_and(|t| t.elapsed() < POLL_EVERY - Duration::from_secs(5)) {
                return;
            }
            inner.refreshing = true;
            inner.last_poll = Some(Instant::now());
        }
        self.emit();

        let ids: Vec<String> = self.0.inner.lock().state.accounts.iter().map(|a| a.id.clone()).collect();
        let results = futures_join(ids.iter().map(|id| {
            let this = self.clone();
            let id = id.clone();
            async move {
                let r = this.fetch_account_usage(&id).await;
                (id, r)
            }
        }))
        .await;

        let now = now_ms();
        for (id, result) in results {
            match result {
                Ok(windows) => {
                    let mut inner = self.0.inner.lock();
                    let five = windows.iter().find(|w| w.key == "five_hour").map(|w| w.utilization);
                    let seven = windows.iter().find(|w| w.key == "seven_day").map(|w| w.utilization);
                    let samples = inner.state.history.entry(id.clone()).or_default();
                    samples.push(Sample { t: now, five, seven });
                    samples.retain(|s| now - s.t < 6 * 3_600_000);
                    let excess = samples.len().saturating_sub(300);
                    samples.drain(..excess);
                    inner.state.last_usage.insert(id.clone(), StoredUsage { windows, fetched_at_ms: now });
                    if let Some(a) = inner.state.accounts.iter_mut().find(|a| a.id == id) {
                        a.needs_login = false;
                    }
                }
                Err(ApiError::InvalidGrant) => self.mark_needs_login(&id),
                Err(e) => eprintln!("[flipon] usage for {id}: {}", e.message()),
            }
        }

        self.refresh_activity().await;
        self.0.inner.lock().refreshing = false;
        self.persist();
        self.emit();

        self.check_warnings();
        self.maybe_auto_switch().await;
    }

    async fn refresh_activity(&self) {
        let this = self.clone();
        let entries = tauri::async_runtime::spawn_blocking(move || {
            let since = activity::local_midnight_ms();
            this.0.scanner.lock().scan(&claude_creds::config_dir(), since)
        })
        .await
        .unwrap_or_default();
        self.0.inner.lock().today_entries = entries;
    }

    fn check_warnings(&self) {
        let notices: Vec<(String, String)> = {
            let mut inner = self.0.inner.lock();
            if !inner.settings.warn_enabled {
                return;
            }
            let Some(id) = inner.active_id.clone() else { return };
            let Some(usage) = inner.state.last_usage.get(&id).cloned() else { return };
            let name = inner.state.accounts.iter().find(|a| a.id == id).map(display_name).unwrap_or_default();
            let th = inner.settings.warn_threshold as f64;
            let now = now_ms();
            let mut out = Vec::new();
            for (key, label) in [("five_hour", "5-hour"), ("seven_day", "weekly")] {
                let Some(w) = usage.windows.iter().find(|w| w.key == key) else { continue };
                if w.resets_at_ms.is_some_and(|r| r <= now) {
                    continue;
                }
                let flag = format!("{id}:{key}");
                if w.utilization >= th {
                    if inner.warned.insert(flag) {
                        out.push((
                            "Approaching the limit".to_string(),
                            format!("{name} is at {:.0}% of its {label} limit.", w.utilization),
                        ));
                    }
                } else if w.utilization < th - 5.0 {
                    inner.warned.remove(&flag);
                }
            }
            out
        };
        for (title, body) in notices {
            self.notify(&title, &body);
        }
    }

    async fn maybe_auto_switch(&self) {
        let decision = {
            let mut inner = self.0.inner.lock();
            if !inner.settings.auto_switch || inner.login.running {
                return;
            }
            if inner.last_switch.is_some_and(|t| t.elapsed() < MIN_SWITCH_GAP) {
                return;
            }
            let Some(active) = inner.active_id.clone() else { return };
            // never judge the active account before we have a reading for it
            if !inner.state.last_usage.contains_key(&active) {
                return;
            }
            let now = now_ms();
            let candidates: Vec<Candidate> = inner
                .state
                .accounts
                .iter()
                .map(|a| {
                    let usage = inner.state.last_usage.get(&a.id);
                    let reading = |key: &str| {
                        usage
                            .and_then(|u| u.windows.iter().find(|w| w.key == key))
                            .map(|w| if w.resets_at_ms.is_some_and(|r| r <= now) { (0.0, None) } else { (w.utilization, w.resets_at_ms) })
                    };
                    let five = reading("five_hour");
                    let seven = reading("seven_day");
                    Candidate {
                        id: a.id.clone(),
                        usable: !a.needs_login && !a.no_backup && usage.is_some(),
                        five: five.map(|x| x.0),
                        seven: seven.map(|x| x.0),
                        five_reset: five.and_then(|x| x.1),
                        seven_reset: seven.and_then(|x| x.1),
                    }
                })
                .collect();
            let hold_active = inner
                .state
                .hold
                .as_ref()
                .is_some_and(|h| h.account_id == active && h.until_ms.is_none_or(|t| t > now));
            // start a new round-robin lap from 1 once every account is idle again
            let mut lap = inner.state.rr_lap.max(1);
            let step = inner.settings.round_robin_step as f64;
            if candidates.iter().all(|c| c.binding() < step) {
                lap = 1;
            }
            inner.state.rr_lap = lap;
            let input = strategy::Input {
                accounts: &candidates,
                active: Some(&active),
                threshold: inner.settings.switch_threshold as f64,
                strategy: inner.settings.strategy,
                round_robin_step: step,
                rr_lap: lap,
                hold_active,
            };
            let decision = strategy::decide(&input);
            let from = candidates.iter().find(|c| c.id == active).map(|c| c.binding());
            decision.map(|d| (d, from))
        };
        let Some((d, from_pct)) = decision else { return };
        let reason = if d.exhausted {
            match from_pct {
                Some(p) => format!("The previous account reached {p:.0}% of its limit."),
                None => "The previous account is unavailable.".into(),
            }
        } else {
            "Automatic switch based on your strategy.".to_string()
        };
        let result = self.switch_to(&d.target, HoldSpec::None, Some(reason), Some(d.new_lap)).await;
        if let Err(e) = result {
            self.set_error(format!("Automatic switch failed: {e}"));
        }
    }

    // ------------------------------------------------------------------ background loop

    pub fn start_background(&self) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            this.detect_cli().await;
            this.sync_with_cli();
            this.emit();
            this.poll_all(true).await;
            let mut ticker = tokio::time::interval(TICK_EVERY);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                this.sync_with_cli();
                // keep countdowns / holds / tray fresh even between polls
                this.emit();
                this.poll_all(false).await;
            }
        });
    }
}

// --------------------------------------------------------------------------- helpers

pub fn display_name(a: &AccountMeta) -> String {
    a.alias.clone().unwrap_or_else(|| a.email.clone())
}

pub fn plan_label(subscription: &str, tier: &str) -> String {
    let t = tier.to_lowercase();
    match subscription.to_lowercase().as_str() {
        "" => "—".into(),
        "max" if t.contains("20x") => "Max 20×".into(),
        "max" if t.contains("5x") => "Max 5×".into(),
        "max" => "Max".into(),
        "pro" => "Pro".into(),
        "team" => "Team".into(),
        "enterprise" => "Enterprise".into(),
        "free" => "Free".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn trend_for(samples: &[Sample], key: &str, current: f64, now: i64) -> Option<Trend> {
    if key != "five_hour" && key != "seven_day" {
        return None;
    }
    let min_age = 12 * 60_000;
    let max_age = 30 * 60_000;
    let target = 20 * 60_000;
    let pick = samples
        .iter()
        .filter(|s| (min_age..=max_age).contains(&(now - s.t)))
        .min_by_key(|s| ((now - s.t) - target).abs())?;
    let prev = if key == "five_hour" { pick.five } else { pick.seven }?;
    let delta = current - prev;
    if delta < -2.0 {
        return None; // the window reset in between
    }
    Some(Trend {
        dir: if delta >= 2.0 { "up" } else { "flat" },
        delta,
        minutes: ((now - pick.t) / 60_000).max(1),
    })
}

/// Up to five binding-usage points, at least ~20 minutes apart, oldest first.
fn history_points(samples: &[Sample], now: i64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut last_t = now;
    for s in samples.iter().rev() {
        if last_t - s.t >= 20 * 60_000 {
            out.push(s.five.unwrap_or(0.0).max(s.seven.unwrap_or(0.0)));
            last_t = s.t;
            if out.len() == 5 {
                break;
            }
        }
    }
    out.reverse();
    out
}

async fn cli_status_email(path: &std::path::Path) -> Option<String> {
    let out = cli::command(path)
        .args(["auth", "status", "--json"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("email").and_then(Value::as_str).map(|s| s.trim().to_lowercase())
}

/// Minimal join_all so we don't need the futures crate.
async fn futures_join<F, T>(iter: impl Iterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = iter.map(|f| tauri::async_runtime::spawn(f)).collect();
    let mut out = Vec::new();
    for h in handles {
        if let Ok(v) = h.await {
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_labels() {
        assert_eq!(plan_label("pro", "default_claude_ai"), "Pro");
        assert_eq!(plan_label("max", "default_claude_max_20x"), "Max 20×");
        assert_eq!(plan_label("max", "default_claude_max_5x"), "Max 5×");
        assert_eq!(plan_label("team", ""), "Team");
    }

    #[test]
    fn trend_detects_rise_and_flat() {
        let now = 100 * 60_000;
        let samples = vec![Sample { t: now - 20 * 60_000, five: Some(10.0), seven: Some(30.0) }];
        let up = trend_for(&samples, "five_hour", 15.0, now).unwrap();
        assert_eq!((up.dir, up.minutes), ("up", 20));
        assert_eq!(trend_for(&samples, "seven_day", 30.5, now).unwrap().dir, "flat");
        assert!(trend_for(&samples, "five_hour", 2.0, now).is_none(), "window reset hides the trend");
        assert!(trend_for(&samples, "seven_day_opus", 2.0, now).is_none());
    }

    #[test]
    fn history_points_are_spaced_and_ordered() {
        let now = 300 * 60_000;
        let samples: Vec<Sample> = (0..10)
            .map(|i| Sample { t: now - (10 - i) * 10 * 60_000, five: Some(i as f64), seven: Some(0.0) })
            .collect();
        let pts = history_points(&samples, now);
        assert!(pts.len() <= 5 && pts.windows(2).all(|w| w[0] <= w[1]));
    }
}
