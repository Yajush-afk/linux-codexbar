use crate::models::{CodexConfig, ProviderSnapshot, UsageWindowSnapshot};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{DateTime, Local, TimeZone, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const CODEX_REFRESH_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_REFRESH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_DEFAULT_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Clone)]
struct CodexCredentials {
    access_token: String,
    refresh_token: String,
    id_token: Option<String>,
    account_id: Option<String>,
    last_refresh: Option<DateTime<Utc>>,
    path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexUsageResponse {
    #[serde(rename = "plan_type")]
    plan_type: Option<String>,
    #[serde(rename = "rate_limit")]
    rate_limit: Option<CodexRateLimit>,
    credits: Option<CodexCredits>,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexRateLimit {
    #[serde(rename = "primary_window")]
    primary_window: Option<CodexWindow>,
    #[serde(rename = "secondary_window")]
    secondary_window: Option<CodexWindow>,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexWindow {
    #[serde(rename = "used_percent")]
    used_percent: f64,
    #[serde(rename = "reset_at")]
    reset_at: i64,
    #[serde(rename = "limit_window_seconds")]
    limit_window_seconds: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexCredits {
    balance: Option<f64>,
}

pub async fn fetch(config: &CodexConfig) -> ProviderSnapshot {
    if !config.enabled {
        return ProviderSnapshot::disabled("Codex");
    }

    let mut credentials = match load_credentials(config) {
        Ok(credentials) => credentials,
        Err(error) => return ProviderSnapshot::needs_setup("Codex", error),
    };

    if needs_refresh(credentials.last_refresh) && !credentials.refresh_token.is_empty() {
        if let Err(error) = refresh_access_token(&mut credentials).await {
            return ProviderSnapshot::error("Codex", error);
        }
    }

    match fetch_usage(&credentials).await {
        Ok(response) => {
            let mut snapshot = ProviderSnapshot::ready("Codex");
            snapshot.primary = response
                .rate_limit
                .as_ref()
                .and_then(|limits| limits.primary_window.as_ref())
                .map(to_window);
            snapshot.secondary = response
                .rate_limit
                .as_ref()
                .and_then(|limits| limits.secondary_window.as_ref())
                .map(to_window);
            snapshot.credits_remaining = response.credits.as_ref().and_then(|credits| credits.balance);
            snapshot.plan = response
                .plan_type
                .clone()
                .or_else(|| parse_id_token_claim(&credentials.id_token, "chatgpt_plan_type"));
            snapshot.detail = parse_id_token_email(&credentials.id_token).map(|email| format!("account {email}"));
            snapshot.source = Some("oauth".to_string());
            snapshot.updated_at = Some(Local::now());
            snapshot
        }
        Err(error) => ProviderSnapshot::error("Codex", error),
    }
}

fn load_credentials(config: &CodexConfig) -> Result<CodexCredentials, String> {
    let path = config
        .auth_file
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_auth_path);
    let text = fs::read_to_string(&path)
        .map_err(|_| format!("Codex auth.json not found at {}. Run `codex` first.", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Failed parsing Codex auth.json: {error}"))?;

    if let Some(api_key) = value.get("OPENAI_API_KEY").and_then(Value::as_str) {
        return Ok(CodexCredentials {
            access_token: api_key.to_string(),
            refresh_token: String::new(),
            id_token: None,
            account_id: None,
            last_refresh: None,
            path,
        });
    }

    let tokens = value
        .get("tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| "Codex auth.json exists but tokens are missing.".to_string())?;

    let access_token = tokens
        .get("access_token")
        .or_else(|| tokens.get("accessToken"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Codex access token missing from auth.json.".to_string())?
        .to_string();

    let refresh_token = tokens
        .get("refresh_token")
        .or_else(|| tokens.get("refreshToken"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let id_token = tokens
        .get("id_token")
        .or_else(|| tokens.get("idToken"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let account_id = tokens
        .get("account_id")
        .or_else(|| tokens.get("accountId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let last_refresh = value
        .get("last_refresh")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|parsed| parsed.with_timezone(&Utc));

    Ok(CodexCredentials {
        access_token,
        refresh_token,
        id_token,
        account_id,
        last_refresh,
        path,
    })
}

async fn refresh_access_token(credentials: &mut CodexCredentials) -> Result<(), String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed creating Codex refresh client: {error}"))?;
    let response = client
        .post(CODEX_REFRESH_URL)
        .json(&json!({
            "client_id": CODEX_REFRESH_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": credentials.refresh_token,
            "scope": "openid profile email",
        }))
        .send()
        .await
        .map_err(|error| format!("Codex token refresh failed: {error}"))?;

    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed decoding Codex refresh response: {error}"))?;
    if !status.is_success() {
        return Err(format!("Codex token refresh failed with HTTP {}", status.as_u16()));
    }

    credentials.access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or(&credentials.access_token)
        .to_string();
    credentials.refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or(&credentials.refresh_token)
        .to_string();
    credentials.id_token = value
        .get("id_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| credentials.id_token.clone());
    credentials.last_refresh = Some(Utc::now());

    persist_refreshed_credentials(credentials)?;
    Ok(())
}

fn persist_refreshed_credentials(credentials: &CodexCredentials) -> Result<(), String> {
    let contents = fs::read_to_string(&credentials.path)
        .map_err(|error| format!("Failed reading {}: {error}", credentials.path.display()))?;
    let mut root: Value = serde_json::from_str(&contents)
        .map_err(|error| format!("Failed parsing {}: {error}", credentials.path.display()))?;
    let tokens = root
        .get_mut("tokens")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "Codex auth.json tokens section is missing.".to_string())?;

    tokens.insert("access_token".to_string(), Value::String(credentials.access_token.clone()));
    tokens.insert("refresh_token".to_string(), Value::String(credentials.refresh_token.clone()));
    if let Some(id_token) = &credentials.id_token {
        tokens.insert("id_token".to_string(), Value::String(id_token.clone()));
    }
    root["last_refresh"] = Value::String(Utc::now().to_rfc3339());

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|error| format!("Failed serializing Codex auth.json: {error}"))?;
    fs::write(&credentials.path, serialized)
        .map_err(|error| format!("Failed writing {}: {error}", credentials.path.display()))
}

async fn fetch_usage(credentials: &CodexCredentials) -> Result<CodexUsageResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed creating Codex HTTP client: {error}"))?;

    let mut request = client
        .get(resolve_usage_url())
        .header(AUTHORIZATION, format!("Bearer {}", credentials.access_token))
        .header(USER_AGENT, "Linux CodexBar")
        .header(ACCEPT, "application/json");
    if let Some(account_id) = &credentials.account_id {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Codex usage request failed: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed reading Codex usage response: {error}"))?;

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("Codex token is invalid or expired. Run `codex` again.".to_string());
    }
    if !status.is_success() {
        return Err(format!("Codex usage API returned HTTP {}", status.as_u16()));
    }

    serde_json::from_str(&text).map_err(|error| format!("Failed decoding Codex usage response: {error}"))
}

fn resolve_usage_url() -> &'static str {
    CODEX_DEFAULT_USAGE_URL
}

fn to_window(window: &CodexWindow) -> UsageWindowSnapshot {
    let resets_at = Local.timestamp_opt(window.reset_at, 0).single();
    let label = if window.limit_window_seconds <= 5 * 60 * 60 {
        "5h"
    } else {
        "7d"
    };
    UsageWindowSnapshot {
        label,
        used_percent: window.used_percent,
        resets_at,
    }
}

fn needs_refresh(last_refresh: Option<DateTime<Utc>>) -> bool {
    match last_refresh {
        Some(last_refresh) => Utc::now().signed_duration_since(last_refresh).num_days() >= 8,
        None => true,
    }
}

fn default_auth_path() -> PathBuf {
    std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".codex"))
        .join("auth.json")
}

fn parse_id_token_email(id_token: &Option<String>) -> Option<String> {
    parse_jwt_payload(id_token)
        .and_then(|payload| payload.get("email").and_then(Value::as_str).map(str::to_string))
}

fn parse_id_token_claim(id_token: &Option<String>, claim: &str) -> Option<String> {
    parse_jwt_payload(id_token).and_then(|payload| {
        payload
            .get(claim)
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                payload
                    .get("https://api.openai.com/auth")
                    .and_then(Value::as_object)
                    .and_then(|auth| auth.get(claim))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    })
}

fn parse_jwt_payload(id_token: &Option<String>) -> Option<Value> {
    let token = id_token.as_ref()?;
    let encoded = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| URL_SAFE.decode(encoded))
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}
