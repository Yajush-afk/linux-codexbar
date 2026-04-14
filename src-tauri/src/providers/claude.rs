use crate::models::{ClaudeConfig, ProviderSnapshot, UsageWindowSnapshot};
use chrono::{DateTime, Local, TimeZone, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLAUDE_BETA_HEADER: &str = "oauth-2025-04-20";
const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

#[derive(Debug, Clone)]
struct ClaudeCredentials {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    scopes: Vec<String>,
    rate_limit_tier: Option<String>,
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOauthFileSection>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOauthFileSection {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<f64>,
    scopes: Option<Vec<String>>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeUsageResponse {
    #[serde(rename = "five_hour")]
    five_hour: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day")]
    seven_day: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day_sonnet")]
    seven_day_sonnet: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day_opus")]
    seven_day_opus: Option<ClaudeUsageWindow>,
    #[serde(rename = "extra_usage")]
    extra_usage: Option<ClaudeExtraUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeUsageWindow {
    utilization: Option<f64>,
    #[serde(rename = "resets_at")]
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeExtraUsage {
    #[serde(rename = "used_credits")]
    used_credits: Option<f64>,
    #[serde(rename = "monthly_limit")]
    monthly_limit: Option<f64>,
}

pub async fn fetch(config: &ClaudeConfig) -> ProviderSnapshot {
    if !config.enabled {
        return ProviderSnapshot::disabled("Claude");
    }

    let mut credentials = match load_credentials(config) {
        Ok(credentials) => credentials,
        Err(error) => return ProviderSnapshot::needs_setup("Claude", error),
    };

    if credentials.scopes.iter().all(|scope| scope != "user:profile") {
        return ProviderSnapshot::error(
            "Claude",
            "Claude OAuth token is missing user:profile scope. Run `claude login` again.",
        );
    }

    if is_expired(credentials.expires_at) {
        if let Err(error) = refresh_access_token(&mut credentials).await {
            return ProviderSnapshot::error("Claude", error);
        }
    }

    match fetch_usage(&credentials).await {
        Ok(response) => {
            let mut snapshot = ProviderSnapshot::ready("Claude");
            snapshot.primary = response.five_hour.as_ref().and_then(|window| to_window("5h", window));
            snapshot.secondary = response.seven_day.as_ref().and_then(|window| to_window("7d", window));
            snapshot.tertiary = response
                .seven_day_sonnet
                .as_ref()
                .or(response.seven_day_opus.as_ref())
                .and_then(|window| to_window("model", window));
            snapshot.source = Some("oauth".to_string());
            snapshot.plan = credentials.rate_limit_tier.clone();
            snapshot.detail = response.extra_usage.as_ref().and_then(|extra| {
                match (extra.used_credits, extra.monthly_limit) {
                    (Some(used), Some(limit)) => Some(format!("extra usage ${:.2}/${:.2}", used / 100.0, limit / 100.0)),
                    _ => None,
                }
            });
            snapshot.updated_at = Some(Local::now());
            snapshot
        }
        Err(error) => ProviderSnapshot::error("Claude", error),
    }
}

fn load_credentials(config: &ClaudeConfig) -> Result<ClaudeCredentials, String> {
    let path = config
        .credentials_file
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_credentials_path);
    let text = fs::read_to_string(&path)
        .map_err(|_| format!("Claude credentials not found at {}. Run `claude login` first.", path.display()))?;
    let parsed: ClaudeCredentialsFile = serde_json::from_str(&text)
        .map_err(|error| format!("Failed parsing Claude credentials file: {error}"))?;
    let oauth = parsed
        .claude_ai_oauth
        .ok_or_else(|| "Claude OAuth credentials missing. Run `claude login`.".to_string())?;
    let access_token = oauth
        .access_token
        .unwrap_or_default()
        .trim()
        .to_string();
    if access_token.is_empty() {
        return Err("Claude access token missing. Run `claude login`.".to_string());
    }

    let expires_at = oauth
        .expires_at
        .and_then(|millis| Utc.timestamp_millis_opt(millis as i64).single());

    Ok(ClaudeCredentials {
        access_token,
        refresh_token: oauth.refresh_token,
        expires_at,
        scopes: oauth.scopes.unwrap_or_default(),
        rate_limit_tier: oauth.rate_limit_tier,
        path,
    })
}

async fn refresh_access_token(credentials: &mut ClaudeCredentials) -> Result<(), String> {
    let refresh_token = credentials
        .refresh_token
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Claude token expired and no refresh token is available. Run `claude login`.".to_string())?;

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed creating Claude refresh client: {error}"))?;
    let response = client
        .post(CLAUDE_REFRESH_URL)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ACCEPT, "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("client_id", CLAUDE_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|error| format!("Claude token refresh failed: {error}"))?;

    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed decoding Claude refresh response: {error}"))?;
    if !status.is_success() {
        return Err(format!("Claude token refresh failed with HTTP {}", status.as_u16()));
    }

    credentials.access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or(&credentials.access_token)
        .to_string();
    credentials.refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| credentials.refresh_token.clone());
    credentials.expires_at = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .and_then(|seconds| Utc.timestamp_opt(Utc::now().timestamp() + seconds, 0).single());

    persist_refreshed_credentials(credentials)?;
    Ok(())
}

fn persist_refreshed_credentials(credentials: &ClaudeCredentials) -> Result<(), String> {
    let text = fs::read_to_string(&credentials.path)
        .map_err(|error| format!("Failed reading {}: {error}", credentials.path.display()))?;
    let mut root: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Failed parsing {}: {error}", credentials.path.display()))?;
    let oauth = root
        .get_mut("claudeAiOauth")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "Claude credentials file is missing claudeAiOauth.".to_string())?;

    oauth.insert("accessToken".to_string(), Value::String(credentials.access_token.clone()));
    if let Some(refresh_token) = &credentials.refresh_token {
        oauth.insert("refreshToken".to_string(), Value::String(refresh_token.clone()));
    }
    if let Some(expires_at) = credentials.expires_at {
        oauth.insert(
            "expiresAt".to_string(),
            Value::Number(serde_json::Number::from(expires_at.timestamp_millis())),
        );
    }

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|error| format!("Failed serializing Claude credentials file: {error}"))?;
    fs::write(&credentials.path, serialized)
        .map_err(|error| format!("Failed writing {}: {error}", credentials.path.display()))
}

async fn fetch_usage(credentials: &ClaudeCredentials) -> Result<ClaudeUsageResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed creating Claude HTTP client: {error}"))?;
    let response = client
        .get(CLAUDE_USAGE_URL)
        .header(AUTHORIZATION, format!("Bearer {}", credentials.access_token))
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .header("anthropic-beta", CLAUDE_BETA_HEADER)
        .header(USER_AGENT, "claude-code/2.1.0")
        .send()
        .await
        .map_err(|error| format!("Claude usage request failed: {error}"))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed reading Claude usage response: {error}"))?;
    if status.as_u16() == 401 {
        return Err("Claude token is invalid or expired. Run `claude login`.".to_string());
    }
    if !status.is_success() {
        return Err(format!("Claude usage API returned HTTP {}", status.as_u16()));
    }

    serde_json::from_str(&text).map_err(|error| format!("Failed decoding Claude usage response: {error}"))
}

fn to_window(label: &'static str, window: &ClaudeUsageWindow) -> Option<UsageWindowSnapshot> {
    let used_percent = window.utilization?;
    let resets_at = window
        .resets_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Local));
    Some(UsageWindowSnapshot {
        label,
        used_percent,
        resets_at,
    })
}

fn default_credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".credentials.json")
}

fn is_expired(expires_at: Option<DateTime<Utc>>) -> bool {
    match expires_at {
        Some(expires_at) => Utc::now() >= expires_at,
        None => true,
    }
}
