//! Shared data types: persisted settings/state and the snapshot sent to the UI.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Headroom,
    Order,
    Balanced,
    #[serde(rename = "roundrobin")]
    RoundRobin,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrayMode {
    Presets,
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrayPreset {
    Bar,
    Percent,
    Ring,
    Dot,
    Face,
    History,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasedOn {
    #[serde(rename = "highest")]
    Highest,
    #[serde(rename = "5h")]
    FiveHour,
    #[serde(rename = "7d")]
    SevenDay,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TrayModule {
    /// "name" | "5h" | "7d" | "cost" | "reset"
    pub id: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub auto_switch: bool,
    pub notify_on_switch: bool,
    pub switch_threshold: u8,
    pub warn_enabled: bool,
    pub warn_threshold: u8,
    pub strategy: Strategy,
    pub round_robin_step: u8,
    pub tray_mode: TrayMode,
    pub tray_preset: TrayPreset,
    pub tray_modules: Vec<TrayModule>,
    pub tray_based_on: BasedOn,
    pub start_on_login: bool,
    pub cli_path: String,
    pub auto_update: bool,
    /// True once the user has switched automatic updates on or off themselves. Until then automatic
    /// updates are on: this also turns them on for people who installed before they became the default.
    pub auto_update_chosen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_switch: true,
            notify_on_switch: true,
            switch_threshold: 90,
            warn_enabled: true,
            warn_threshold: 80,
            strategy: Strategy::Balanced,
            round_robin_step: 40,
            tray_mode: TrayMode::Presets,
            tray_preset: TrayPreset::Bar,
            tray_modules: default_modules(),
            tray_based_on: BasedOn::Highest,
            start_on_login: false,
            cli_path: String::new(),
            auto_update: true,
            auto_update_chosen: false,
        }
    }
}

pub fn default_modules() -> Vec<TrayModule> {
    [("name", true), ("5h", true), ("7d", true), ("cost", false), ("reset", false)]
        .into_iter()
        .map(|(id, enabled)| TrayModule { id: id.into(), enabled })
        .collect()
}

impl Settings {
    /// Clamp values into the ranges the UI offers and make sure every tray module exists exactly once.
    pub fn sanitize(&mut self) {
        if !self.auto_update_chosen {
            self.auto_update = true;
        }
        self.switch_threshold = self.switch_threshold.clamp(50, 100);
        self.warn_threshold = self.warn_threshold.clamp(50, 100);
        self.round_robin_step = self.round_robin_step.clamp(10, 90);
        let mut seen = Vec::<String>::new();
        self.tray_modules.retain(|m| {
            let known = matches!(m.id.as_str(), "name" | "5h" | "7d" | "cost" | "reset");
            if known && !seen.contains(&m.id) {
                seen.push(m.id.clone());
                true
            } else {
                false
            }
        });
        for m in default_modules() {
            if !seen.contains(&m.id) {
                self.tray_modules.push(m);
            }
        }
    }
}

/// One saved Claude account (credentials live separately in the credential store).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountMeta {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub alias: Option<String>,
    /// Raw `subscriptionType` from the OAuth blob ("pro", "max", "team", ...).
    #[serde(default)]
    pub subscription: String,
    #[serde(default)]
    pub rate_limit_tier: String,
    /// The `oauthAccount` object Claude Code keeps in ~/.claude.json (email, org, plan details).
    #[serde(default)]
    pub oauth_account: Option<Value>,
    /// Stored credentials were rejected by Anthropic.
    #[serde(default)]
    pub needs_login: bool,
    /// Never completed authentication through FlipOn (no credentials stored).
    #[serde(default)]
    pub no_backup: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// "five_hour" | "seven_day" | "seven_day_opus" | "seven_day_sonnet"
    pub key: String,
    pub utilization: f64,
    pub resets_at_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredUsage {
    pub windows: Vec<UsageWindow>,
    pub fetched_at_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub t: i64,
    pub five: Option<f64>,
    pub seven: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Hold {
    pub account_id: String,
    /// `None` means "until you switch again".
    pub until_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SwitchEvent {
    pub t: i64,
    pub account_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedState {
    pub accounts: Vec<AccountMeta>,
    pub hold: Option<Hold>,
    pub rr_lap: u32,
    pub history: HashMap<String, Vec<Sample>>,
    pub last_usage: HashMap<String, StoredUsage>,
    pub switch_log: Vec<SwitchEvent>,
    pub last_active: Option<String>,
    pub ignored_unknown: Option<String>,
}

// ---------------------------------------------------------------------------
// Snapshot sent to the frontend
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Trend {
    /// "up" | "flat"
    pub dir: &'static str,
    pub delta: f64,
    pub minutes: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WindowView {
    pub key: String,
    pub label: String,
    pub utilization: f64,
    pub resets_at_ms: Option<i64>,
    pub window_ms: i64,
    pub trend: Option<Trend>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TodayView {
    pub cost: f64,
    pub turns: u32,
    pub lines: u32,
    pub models: Vec<(String, f64)>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub email: String,
    pub alias: Option<String>,
    pub plan: String,
    /// "active" | "ready" | "needs_login" | "no_backup" | "week_full" | "five_hour_full"
    pub status: &'static str,
    /// For *_full statuses: when the binding window resets.
    pub status_resets_at_ms: Option<i64>,
    pub windows: Vec<WindowView>,
    pub today: Option<TodayView>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub found: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoginView {
    pub running: bool,
    /// A finished login is being saved into the account list.
    pub adding: bool,
    pub needs_code: bool,
    pub url: Option<String>,
    pub target_id: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ErrorView {
    pub id: u64,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    /// A development build (`pnpm tauri dev`): marked as DEV everywhere so it is never mistaken for the installed app.
    pub dev: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub accounts: Vec<AccountView>,
    pub active_id: Option<String>,
    pub settings: Settings,
    pub cli: CliInfo,
    pub login: LoginView,
    pub hold: Option<Hold>,
    pub unknown_login: Option<String>,
    pub refreshing: bool,
    pub error: Option<ErrorView>,
    pub app: AppInfo,
    pub update: UpdateView,
    pub now_ms: i64,
}

/// Where the self-updater is. The update itself is checked, downloaded and signature-verified
/// in the background; installing it restarts the app.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    /// "idle" | "checking" | "up_to_date" | "downloading" | "ready" | "installing" | "error"
    pub status: &'static str,
    pub version: Option<String>,
    pub notes: Option<String>,
    /// 0..1 while downloading, when the size is known.
    pub progress: Option<f64>,
    pub error: Option<String>,
    pub checked_at_ms: Option<i64>,
}

impl Default for UpdateView {
    fn default() -> Self {
        Self { status: "idle", version: None, notes: None, progress: None, error: None, checked_at_ms: None }
    }
}

#[cfg(test)]
mod auto_update_default_tests {
    use super::Settings;

    #[test]
    fn on_for_new_and_older_installs_until_the_user_chooses() {
        let mut fresh = Settings::default();
        fresh.sanitize();
        assert!(fresh.auto_update);

        // settings.json written by 0.2.0 and older: autoUpdate false, no autoUpdateChosen
        let mut old: Settings = serde_json::from_str(r#"{"autoUpdate":false}"#).unwrap();
        old.sanitize();
        assert!(old.auto_update);

        let mut off: Settings = serde_json::from_str(r#"{"autoUpdate":false,"autoUpdateChosen":true}"#).unwrap();
        off.sanitize();
        assert!(!off.auto_update);
    }
}
