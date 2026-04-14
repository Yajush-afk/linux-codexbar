use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub refresh_interval_seconds: u64,
    pub providers: ProvidersConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: 60,
            providers: ProvidersConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    pub opencode: OpenCodeConfig,
    pub codex: CodexConfig,
    pub claude: ClaudeConfig,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            opencode: OpenCodeConfig::default(),
            codex: CodexConfig::default(),
            claude: ClaudeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeConfig {
    pub enabled: bool,
    pub cookie_header: String,
    pub workspace_id: Option<String>,
}

impl Default for OpenCodeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cookie_header: String::new(),
            workspace_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfig {
    pub enabled: bool,
    pub auth_file: Option<String>,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auth_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub enabled: bool,
    pub credentials_file: Option<String>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            credentials_file: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProviderState {
    Disabled,
    NeedsSetup(String),
    Error(String),
    Ready,
}

#[derive(Debug, Clone)]
pub struct UsageWindowSnapshot {
    pub label: &'static str,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Local>>,
}

impl UsageWindowSnapshot {
    pub fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    pub name: &'static str,
    pub state: ProviderState,
    pub primary: Option<UsageWindowSnapshot>,
    pub secondary: Option<UsageWindowSnapshot>,
    pub tertiary: Option<UsageWindowSnapshot>,
    pub source: Option<String>,
    pub detail: Option<String>,
    pub plan: Option<String>,
    pub credits_remaining: Option<f64>,
    pub updated_at: Option<DateTime<Local>>,
}

impl ProviderSnapshot {
    pub fn disabled(name: &'static str) -> Self {
        Self {
            name,
            state: ProviderState::Disabled,
            primary: None,
            secondary: None,
            tertiary: None,
            source: None,
            detail: None,
            plan: None,
            credits_remaining: None,
            updated_at: None,
        }
    }

    pub fn needs_setup(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            state: ProviderState::NeedsSetup(message.into()),
            primary: None,
            secondary: None,
            tertiary: None,
            source: None,
            detail: None,
            plan: None,
            credits_remaining: None,
            updated_at: None,
        }
    }

    pub fn error(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            state: ProviderState::Error(message.into()),
            primary: None,
            secondary: None,
            tertiary: None,
            source: None,
            detail: None,
            plan: None,
            credits_remaining: None,
            updated_at: Some(Local::now()),
        }
    }

    pub fn ready(name: &'static str) -> Self {
        Self {
            name,
            state: ProviderState::Ready,
            primary: None,
            secondary: None,
            tertiary: None,
            source: None,
            detail: None,
            plan: None,
            credits_remaining: None,
            updated_at: Some(Local::now()),
        }
    }
}
