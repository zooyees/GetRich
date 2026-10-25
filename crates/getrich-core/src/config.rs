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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub preferences: UiPreferences,
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
#[serde(default)]
pub struct UiPreferences {
    pub theme: String,
    pub colors: String,
    pub list_width: f32,
    pub columns: Vec<String>,
    pub recent: Vec<String>,
}
impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            colors: "red_up".into(),
            list_width: 560.0,
            columns: vec![
                "symbol".into(),
                "name".into(),
                "last".into(),
                "change_pct".into(),
                "turnover".into(),
            ],
            recent: Vec::new(),
        }
    }
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
    #[serde(default = "default_board_all")]
    pub default_board: String,
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
    #[serde(default)]
    pub tushare: TushareConfig,
    #[serde(default)]
    pub pulse: PulseConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TushareConfig {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub token: String,
    /// When false (default), the live rt_k worker does not start.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_tushare_poll_ms")]
    pub poll_ms: u64,
    /// `full` or `watchlist`.
    #[serde(default = "default_tushare_universe")]
    pub universe: String,
    #[serde(default = "default_rt_k_codes")]
    pub rt_k_codes: String,
    #[serde(default = "default_live_timeout_ms")]
    pub live_timeout_ms: u64,
    #[serde(default = "default_rate_per_min")]
    pub rate_per_min: u32,
}

impl Default for TushareConfig {
    fn default() -> Self {
        Self {
            user_id: String::new(),
            token: String::new(),
            enabled: false,
            poll_ms: default_tushare_poll_ms(),
            universe: default_tushare_universe(),
            rt_k_codes: default_rt_k_codes(),
            live_timeout_ms: default_live_timeout_ms(),
            rate_per_min: default_rate_per_min(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseConfig {
    #[serde(default = "default_min_quoted")]
    pub min_quoted: u32,
    #[serde(default = "default_flat_eps")]
    pub flat_eps_pct: f64,
    #[serde(default = "default_limit_eps")]
    pub limit_eps_pct: f64,
    #[serde(default = "default_hollow_gap")]
    pub hollow_gap_pct: f64,
    #[serde(default = "default_hollow_gap_no_index")]
    pub hollow_gap_pct_no_index: f64,
    #[serde(default = "default_beta_advance")]
    pub beta_advance_ratio: f64,
    #[serde(default = "default_beta_down")]
    pub beta_down_advance_ratio: f64,
    #[serde(default = "default_chop_abs")]
    pub chop_abs_pct: f64,
    #[serde(default = "default_amount_hot")]
    pub amount_hot: f64,
    #[serde(default = "default_amount_chop")]
    pub amount_chop: f64,
    #[serde(default = "default_theme_height")]
    pub theme_height: u32,
    #[serde(default = "default_theme_top5")]
    pub theme_top5_share: f64,
    #[serde(default = "default_panic_ld")]
    pub panic_limit_down: u32,
    #[serde(default = "default_sector_top_n")]
    pub sector_top_n: usize,
    #[serde(default = "default_leader_n")]
    pub leader_n: usize,
    #[serde(default = "default_stale_mult")]
    pub stale_poll_mult: u32,
    #[serde(default = "default_watch_min")]
    pub watch_min: usize,
}

impl Default for PulseConfig {
    fn default() -> Self {
        Self {
            min_quoted: default_min_quoted(),
            flat_eps_pct: default_flat_eps(),
            limit_eps_pct: default_limit_eps(),
            hollow_gap_pct: default_hollow_gap(),
            hollow_gap_pct_no_index: default_hollow_gap_no_index(),
            beta_advance_ratio: default_beta_advance(),
            beta_down_advance_ratio: default_beta_down(),
            chop_abs_pct: default_chop_abs(),
            amount_hot: default_amount_hot(),
            amount_chop: default_amount_chop(),
            theme_height: default_theme_height(),
            theme_top5_share: default_theme_top5(),
            panic_limit_down: default_panic_ld(),
            sector_top_n: default_sector_top_n(),
            leader_n: default_leader_n(),
            stale_poll_mult: default_stale_mult(),
            watch_min: default_watch_min(),
        }
    }
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
fn default_board_all() -> String {
    "ALL".into()
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
fn default_tushare_poll_ms() -> u64 {
    5_000
}
fn default_tushare_universe() -> String {
    "full".into()
}
fn default_rt_k_codes() -> String {
    "6*.SH,0*.SZ,3*.SZ,1*.SH,8*.BJ,4*.BJ,9*.BJ".into()
}
fn default_live_timeout_ms() -> u64 {
    8_000
}
fn default_rate_per_min() -> u32 {
    50
}
fn default_min_quoted() -> u32 {
    3000
}
fn default_flat_eps() -> f64 {
    0.01
}
fn default_limit_eps() -> f64 {
    0.01
}
fn default_hollow_gap() -> f64 {
    1.2
}
fn default_hollow_gap_no_index() -> f64 {
    1.5
}
fn default_beta_advance() -> f64 {
    0.60
}
fn default_beta_down() -> f64 {
    0.40
}
fn default_chop_abs() -> f64 {
    0.40
}
fn default_amount_hot() -> f64 {
    1.15
}
fn default_amount_chop() -> f64 {
    0.85
}
fn default_theme_height() -> u32 {
    4
}
fn default_theme_top5() -> f64 {
    0.28
}
fn default_panic_ld() -> u32 {
    30
}
fn default_sector_top_n() -> usize {
    8
}
fn default_leader_n() -> usize {
    3
}
fn default_stale_mult() -> u32 {
    3
}
fn default_watch_min() -> usize {
    3
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
            default_board: default_board_all(),
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
            tushare: TushareConfig::default(),
            pulse: PulseConfig::default(),
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
    Ok(upgrade_legacy_rt_k_codes(apply_tushare_env(cfg)))
}

/// Old shipped defaults omitted BJ 8/4 and SH 1. Custom lists are left alone.
fn upgrade_legacy_rt_k_codes(mut cfg: AppConfig) -> AppConfig {
    let cur = cfg
        .data
        .tushare
        .rt_k_codes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    const LEGACY: &[&str] = &["6*.SH,0*.SZ,3*.SZ,9*.BJ", "6*.SH,0*.SZ,3*.SZ"];
    if LEGACY.iter().any(|s| s.eq_ignore_ascii_case(&cur)) {
        cfg.data.tushare.rt_k_codes = default_rt_k_codes();
    }
    cfg
}

fn apply_tushare_env(mut cfg: AppConfig) -> AppConfig {
    if let Ok(token) = std::env::var("GETRICH_TUSHARE_TOKEN") {
        if !token.trim().is_empty() {
            cfg.data.tushare.token = token.trim().to_string();
        }
    }
    if let Ok(uid) = std::env::var("GETRICH_TUSHARE_USER") {
        if !uid.trim().is_empty() {
            cfg.data.tushare.user_id = uid.trim().to_string();
        }
    }
    cfg
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
        let cfg: AppConfig =
            serde_json::from_value(json!({"system": {"log_level": "DEBUG"}})).unwrap();
        assert_eq!(cfg.system.log_level, "DEBUG");
        assert_eq!(cfg.system.db_name, "getrich.db");
        assert_eq!(cfg.data.read_retries, 3);
        assert!(cfg.data.tushare.token.is_empty());
        assert!(!cfg.data.tushare.enabled);
        assert_eq!(cfg.data.tushare.poll_ms, 5_000);
        assert_eq!(cfg.data.tushare.live_timeout_ms, 8_000);
        assert_eq!(cfg.data.tushare.rate_per_min, 50);
        assert_eq!(cfg.data.tushare.universe, "full");
    }

    #[test]
    fn upgrades_legacy_rt_k_codes() {
        let mut cfg = AppConfig::default();
        cfg.data.tushare.rt_k_codes = "6*.SH,0*.SZ,3*.SZ,9*.BJ".into();
        cfg = upgrade_legacy_rt_k_codes(cfg);
        assert!(cfg.data.tushare.rt_k_codes.contains("8*.BJ"));
        assert!(cfg.data.tushare.rt_k_codes.contains("1*.SH"));
        cfg.data.tushare.rt_k_codes = "600519.SH".into();
        cfg = upgrade_legacy_rt_k_codes(cfg);
        assert_eq!(cfg.data.tushare.rt_k_codes, "600519.SH");
    }
}
