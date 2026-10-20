//! Talking to Anthropic: the (undocumented) OAuth usage endpoint and OAuth token refresh.
//! Credentials are only ever sent to Anthropic's own hosts.

use crate::model::UsageWindow;
use chrono::DateTime;
use serde_json::{json, Value};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TOKEN_URLS: [&str; 2] = [
    "https://platform.claude.com/v1/oauth/token",
    "https://console.anthropic.com/v1/oauth/token",
];
/// Claude Code's public OAuth client id.
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

#[derive(Debug)]
pub enum ApiError {
    /// 401/403 — the access token is expired or revoked.
    Unauthorized,
    /// The refresh token was rejected; the user must log in again.
    InvalidGrant,
    RateLimited,
    Network(String),
    Other(String),
}

impl ApiError {
    pub fn message(&self) -> String {
        match self {
            ApiError::Unauthorized => "Session expired".into(),
            ApiError::InvalidGrant => "Session expired. Log in again.".into(),
            ApiError::RateLimited => "Anthropic is rate limiting usage checks; will retry".into(),
            ApiError::Network(e) => format!("Network error: {e}"),
            ApiError::Other(e) => e.clone(),
        }
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("flipon/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub async fn fetch_usage(http: &reqwest::Client, access_token: &str) -> Result<Vec<UsageWindow>, ApiError> {
    let resp = http
        .get(USAGE_URL)
        .bearer_auth(access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
    match resp.status().as_u16() {
        200 => {}
        401 | 403 => return Err(ApiError::Unauthorized),
        429 => return Err(ApiError::RateLimited),
        s => return Err(ApiError::Other(format!("Usage request failed (HTTP {s})"))),
    }
    let body: Value = resp.json().await.map_err(|e| ApiError::Other(format!("Unreadable usage response: {e}")))?;
    Ok(parse_usage(&body))
}

fn parse_time(v: &Value) -> Option<i64> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
}

/// Accepts both response shapes seen in the wild: flat `five_hour` / `seven_day` / `seven_day_*`
/// objects, and the newer structured `limits` array.
pub fn parse_usage(body: &Value) -> Vec<UsageWindow> {
    let mut out: Vec<UsageWindow> = Vec::new();

    for key in ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"] {
        if let Some(w) = body.get(key).filter(|v| v.is_object()) {
            if let Some(u) = w.get("utilization").and_then(Value::as_f64) {
                out.push(UsageWindow {
                    key: key.into(),
                    utilization: u,
                    resets_at_ms: w.get("resets_at").and_then(parse_time),
                });
            }
        }
    }

    if let Some(limits) = body.get("limits").and_then(Value::as_array) {
        for l in limits {
            let kind = l.get("kind").and_then(Value::as_str).unwrap_or_default();
            let scope = l
                .get("scope")
                .and_then(|s| s.as_str().map(str::to_lowercase).or_else(|| s.get("name").and_then(Value::as_str).map(str::to_lowercase)));
            let key = match (kind, scope.as_deref()) {
                ("session", _) => "five_hour",
                ("weekly_all", _) => "seven_day",
                ("weekly_scoped", Some(s)) if s.contains("opus") => "seven_day_opus",
                ("weekly_scoped", Some(s)) if s.contains("sonnet") => "seven_day_sonnet",
                _ => continue,
            };
            if out.iter().any(|w| w.key == key) {
                continue;
            }
            if let Some(p) = l.get("percent").and_then(Value::as_f64) {
                out.push(UsageWindow {
                    key: key.into(),
                    utilization: p,
                    resets_at_ms: l.get("resets_at").and_then(parse_time),
                });
            }
        }
    }
    out
}

pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
}

pub async fn refresh_token(http: &reqwest::Client, refresh_token: &str) -> Result<TokenSet, ApiError> {
    let mut last_err = ApiError::Other("Token refresh failed".into());
    for url in TOKEN_URLS {
        let resp = match http
            .post(url)
            .json(&json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": CLIENT_ID,
            }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = ApiError::Network(e.without_url().to_string());
                continue;
            }
        };
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        match status {
            200 => {
                let access = body.get("access_token").and_then(Value::as_str);
                let Some(access) = access else {
                    last_err = ApiError::Other("Token response had no access_token".into());
                    continue;
                };
                // The server may or may not rotate the refresh token.
                let refresh = body
                    .get("refresh_token")
                    .and_then(Value::as_str)
                    .unwrap_or(refresh_token);
                let expires_in = body.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
                return Ok(TokenSet {
                    access_token: access.to_string(),
                    refresh_token: refresh.to_string(),
                    expires_at_ms: chrono::Utc::now().timestamp_millis() + expires_in * 1000,
                });
            }
            400 | 401 => {
                let err = body
                    .get("error")
                    .and_then(|e| e.as_str().map(str::to_string).or_else(|| e.get("type").and_then(Value::as_str).map(str::to_string)))
                    .unwrap_or_default();
                // Only an explicit invalid_grant means "log in again"; any other 4xx may just be a
                // request-format problem and must not lock the user out of an account.
                if err.contains("invalid_grant") {
                    return Err(ApiError::InvalidGrant);
                }
                last_err = ApiError::Other(format!("Token refresh rejected (HTTP {status})"));
            }
            429 => last_err = ApiError::RateLimited,
            s => last_err = ApiError::Other(format!("Token refresh failed (HTTP {s})")),
        }
    }
    Err(last_err)
}
