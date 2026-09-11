//! Application config with deep-merge defaults (WiParse `config` parity).

use crate::paths::{config_file, default_config_file};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub system: SystemConfig,
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default)]
    pub universe: UniverseConfig,
    #[serde(default)]
    pub data: DataConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    #[serde(default = "default_db_name")]
    pub db_name: String,
    #[serde(default = "default_log_file")]
    pub log_file: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_commit_interval")]
    pub db_commit_interval_sec: f64,
    #[serde(default = "default_batch")]
    pub db_commit_batch_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseConfig {
    #[serde(default)]
    pub watchlist: Vec<WatchItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchItem {
    pub symbol: String,
    #[serde(default = "default_market")]
    pub market: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    #[serde(default = "default_timeframe")]
    pub default_timeframe: String,
    #[serde(default = "default_retries", alias = "max_retries")]
    pub read_retries: u32,
    #[serde(default = "default_backoff", alias = "retry_backoff_ms")]
    pub read_retry_backoff_ms: u64,
    #[serde(default = "default_timeout", alias = "request_timeout_ms")]
    pub read_timeout_ms: u64,
}

fn default_db_name() -> String {
    "getrich.db".into()
}
fn default_log_file() -> String {
    "getrich.log".into()
}
fn default_log_level() -> String {
    "INFO".into()
}
fn default_commit_interval() -> f64 {
    1.0
}
fn default_batch() -> u32 {
    500
}
fn default_bind() -> String {
    "127.0.0.1:7878".into()
}
fn default_market() -> String {
    "US".into()
}
fn default_timeframe() -> String {
    "1d".into()
}
fn default_timeout() -> u64 {
    15_000
}
fn default_retries() -> u32 {
    3
}
fn default_backoff() -> u64 {
    500
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            db_name: default_db_name(),
            log_file: default_log_file(),
            log_level: default_log_level(),
            db_commit_interval_sec: default_commit_interval(),
            db_commit_batch_size: default_batch(),
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
        }
    }
}

impl Default for UniverseConfig {
    fn default() -> Self {
        Self {
            watchlist: vec![
                WatchItem {
                    symbol: "AAPL".into(),
                    market: "US".into(),
                    name: Some("Apple Inc.".into()),
                },
                WatchItem {
                    symbol: "600519".into(),
                    market: "CN.SH".into(),
                    name: Some("贵州茅台".into()),
                },
                WatchItem {
                    symbol: "00700".into(),
                    market: "HK".into(),
                    name: Some("腾讯控股".into()),
                },
            ],
        }
    }
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            default_timeframe: default_timeframe(),
            read_retries: default_retries(),
            read_retry_backoff_ms: default_backoff(),
            read_timeout_ms: default_timeout(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            system: SystemConfig::default(),
            http: HttpConfig::default(),
            universe: UniverseConfig::default(),
            data: DataConfig::default(),
        }
    }
}

/// Deep-merge JSON objects (`overlay` wins on leaf keys).
pub fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            let mut out = Map::new();
            for (k, v) in b {
                out.insert(k.clone(), v.clone());
            }
            for (k, v) in o {
                if let Some(existing) = out.get(k) {
                    out.insert(k.clone(), deep_merge(existing, v));
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            Value::Object(out)
        }
        (_, o) => o.clone(),
    }
}

fn default_config_value() -> Value {
    serde_json::to_value(AppConfig::default()).unwrap_or_else(|_| json!({}))
}

pub fn load_config() -> Result<AppConfig, ConfigError> {
    let mut merged = default_config_value();

    if let Ok(text) = fs::read_to_string(default_config_file()) {
        if let Ok(file_cfg) = serde_json::from_str::<Value>(&text) {
            merged = deep_merge(&merged, &file_cfg);
        }
    }

    let path = config_file();
    if path.is_file() {
        let text = fs::read_to_string(&path)?;
        let file_cfg: Value = serde_json::from_str(&text)?;
        merged = deep_merge(&merged, &file_cfg);
    }

    let cfg: AppConfig = serde_json::from_value(merged)?;
    Ok(cfg)
}

pub fn save_config(cfg: &AppConfig) -> Result<(), ConfigError> {
    let path = config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cfg)?;
    fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge_nested() {
        let base = json!({"a": {"x": 1, "y": 2}, "b": 3});
        let over = json!({"a": {"y": 9}, "c": 4});
        let m = deep_merge(&base, &over);
        assert_eq!(m["a"]["x"], 1);
        assert_eq!(m["a"]["y"], 9);
        assert_eq!(m["c"], 4);
    }

    #[test]
    fn default_loads() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.system.db_name, "getrich.db");
        assert_eq!(cfg.http.bind, "127.0.0.1:7878");
        assert!(!cfg.universe.watchlist.is_empty());
    }

    #[test]
    fn missing_fields_default() {
        let cfg: AppConfig = serde_json::from_value(json!({"system": {"log_level": "DEBUG"}}))
            .unwrap();
        assert_eq!(cfg.system.log_level, "DEBUG");
        assert_eq!(cfg.system.db_name, "getrich.db");
        assert_eq!(cfg.data.read_retries, 3);
    }
}
