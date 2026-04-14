use crate::models::AppConfig;
use dirs::config_dir;
use std::fs;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "linux-codexbar";
const CONFIG_FILE: &str = "config.json";

pub fn config_dir_path() -> Result<PathBuf, String> {
    let base = config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(base.join(APP_DIR))
}

pub fn config_file_path() -> Result<PathBuf, String> {
    Ok(config_dir_path()?.join(CONFIG_FILE))
}

pub fn load_or_create() -> Result<AppConfig, String> {
    let path = config_file_path()?;
    if !path.exists() {
        let config = AppConfig::default();
        save_to_path(&path, &config)?;
        return Ok(config);
    }

    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed reading config file {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("Failed parsing config file {}: {error}", path.display()))
}

pub fn reload() -> Result<AppConfig, String> {
    let path = config_file_path()?;
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed reading config file {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("Failed parsing config file {}: {error}", path.display()))
}

pub fn save(config: &AppConfig) -> Result<(), String> {
    let path = config_file_path()?;
    save_to_path(&path, config)
}

fn save_to_path(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed creating config directory {}: {error}", parent.display()))?;
    }

    let contents = serde_json::to_string_pretty(config)
        .map_err(|error| format!("Failed serializing config: {error}"))?;
    fs::write(path, contents)
        .map_err(|error| format!("Failed writing config file {}: {error}", path.display()))
}
