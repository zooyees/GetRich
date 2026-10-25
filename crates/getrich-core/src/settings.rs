//! Cross-process settings updates are serialized by SQLite's write lock.
use crate::{
    config::{AppConfig, WatchItem},
    error::AppError,
    service::App,
};
use rusqlite::TransactionBehavior;
use serde_json::{json, Value};

pub fn validate_board(value: &str) -> Result<(), AppError> {
    if ![
        "ALL", "WATCH", "CN", "CN.SH", "CN.SZ", "CN.BJ", "STAR", "CHINEXT", "HK", "US",
    ]
    .contains(&value)
    {
        return Err(AppError::InvalidArg(format!("未知板块：{value}")));
    }
    Ok(())
}

impl App {
    pub fn record_view(&self, symbol: &str, market: Option<&str>) -> Result<Value, AppError> {
        let key = self.parse_key(symbol, market)?;
        let item = format!("{}:{}", key.market.as_str(), key.symbol);
        if self.read_settings_config()?.preferences.recent.first() == Some(&item) {
            return Ok(json!({"saved":false}));
        }
        self.update_settings(|cfg| {
            let recent = &mut cfg.preferences.recent;
            recent.retain(|r| r != &item);
            recent.insert(0, item);
            recent.truncate(30);
            Ok(())
        })?;
        Ok(json!({"saved":true}))
    }
    pub fn import_watch(&self, params: &Value) -> Result<Value, AppError> {
        let items: Vec<WatchItem> =
            serde_json::from_value(params.get("watchlist").cloned().unwrap_or(Value::Null))
                .map_err(|e| AppError::InvalidArg(e.to_string()))?;
        if items.len() > 10_000 {
            return Err(AppError::InvalidArg("自选最多导入 10000 条".into()));
        }
        let validated = items
            .into_iter()
            .map(|w| {
                self.parse_key(&w.symbol, Some(&w.market))
                    .map(|k| WatchItem {
                        symbol: k.symbol,
                        market: k.market.as_str().into(),
                        name: w.name,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.update_settings(|cfg| {
            for item in validated {
                if !cfg
                    .universe
                    .watchlist
                    .iter()
                    .any(|w| w.symbol == item.symbol && w.market == item.market)
                {
                    cfg.universe.watchlist.push(item);
                }
            }
            Ok(())
        })?;
        self.settings_view()
    }

    pub fn move_watch(&self, params: &Value) -> Result<Value, AppError> {
        let key = self.parse_key(
            params["symbol"].as_str().unwrap_or(""),
            params["market"].as_str(),
        )?;
        let delta = params["direction"].as_i64().unwrap_or(0).signum();
        self.update_settings(|cfg| {
            let list = &mut cfg.universe.watchlist;
            let index = list
                .iter()
                .position(|w| {
                    self.parse_key(&w.symbol, Some(&w.market)).ok().as_ref() == Some(&key)
                })
                .ok_or_else(|| AppError::NotFound("自选不存在".into()))?;
            let next =
                (index as i64 + delta).clamp(0, list.len().saturating_sub(1) as i64) as usize;
            list.swap(index, next);
            Ok(())
        })?;
        self.settings_view()
    }

    pub(crate) fn read_settings_config(&self) -> Result<AppConfig, AppError> {
        match std::fs::read_to_string(&self.config_path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| AppError::InvalidArg(format!("配置文件解析失败：{e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(self.cfg.clone()),
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn update_settings<T>(
        &self,
        update: impl FnOnce(&mut AppConfig) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let _guard = self.config_write.lock().unwrap_or_else(|e| e.into_inner());
        let mut conn = self.db.connect()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(crate::db::DbError::from)?;
        let mut cfg = self.read_settings_config()?;
        let value = update(&mut cfg)?;
        self.write_config(&cfg)?;
        tx.execute(
            "UPDATE app_state SET value=value+1 WHERE key='settings_version'",
            [],
        )
        .map_err(crate::db::DbError::from)?;
        tx.commit().map_err(crate::db::DbError::from)?;
        self.events.emit("settings.changed", json!({}));
        self.realtime.request_refresh();
        Ok(value)
    }

    pub(crate) fn change_watch(
        &self,
        symbol: &str,
        market: Option<&str>,
        name: Option<String>,
        desired: Option<bool>,
    ) -> Result<Value, AppError> {
        let key = self.parse_key(symbol, market)?;
        self.update_settings(|cfg| {
            let list = &mut cfg.universe.watchlist;
            let pos = list.iter().position(|w| self.parse_key(&w.symbol,Some(&w.market)).ok().as_ref() == Some(&key));
            let watched = desired.unwrap_or(pos.is_none());
            match (watched,pos) {
                (true,None) => list.push(WatchItem {symbol:key.symbol.clone(),market:key.market.as_str().into(),name}),
                (false,Some(i)) => { list.remove(i); },
                _=>{},
            }
            Ok(json!({"symbol":key.symbol,"market":key.market.as_str(),"watched":watched,"watchlist":list}))
        })
    }

    pub fn settings_view(&self) -> Result<Value, AppError> {
        let cfg = self.read_settings_config()?;
        Ok(
            json!({"default_board":cfg.universe.default_board,"watchlist":cfg.universe.watchlist,
            "provider":self.tushare_status(),"preferences":cfg.preferences,
            "config_path":self.config_path,"data_root":self.config_path.parent(),"db_path":self.db.path,
            "storage_change_requires_restart":true}),
        )
    }

    pub fn save_settings(&self, params: &Value) -> Result<Value, AppError> {
        self.update_settings(|cfg| {
            if let Some(board) = params.get("default_board").and_then(Value::as_str) {
                validate_board(board)?;
                cfg.universe.default_board = board.into();
            }
            if let Some(p) = params.get("provider") {
                if let Some(token) = p.get("token").and_then(Value::as_str) {
                    cfg.data.tushare.token = token.trim().into();
                }
                if let Some(user) = p.get("user_id").and_then(Value::as_str) {
                    cfg.data.tushare.user_id = user.trim().into();
                }
                if let Some(enabled) = p.get("enabled").and_then(Value::as_bool) {
                    cfg.data.tushare.enabled = enabled;
                }
                if let Some(poll) = p.get("poll_ms").and_then(Value::as_u64) {
                    if !(1000..=300_000).contains(&poll) {
                        return Err(AppError::InvalidArg("刷新间隔需为 1000–300000 毫秒".into()));
                    }
                    cfg.data.tushare.poll_ms = poll;
                }
                if let Some(universe) = p.get("universe").and_then(Value::as_str) {
                    if !["full", "watchlist"].contains(&universe) {
                        return Err(AppError::InvalidArg("无效同步范围".into()));
                    }
                    cfg.data.tushare.universe = universe.into();
                }
            }
            if let Some(prefs) = params.get("preferences") {
                let mut merged = serde_json::to_value(&cfg.preferences)
                    .map_err(|e| AppError::Message(e.to_string()))?;
                merged = crate::config::deep_merge(&merged, prefs);
                cfg.preferences = serde_json::from_value(merged)
                    .map_err(|e| AppError::InvalidArg(e.to_string()))?;
                if !["light", "dark", "system"].contains(&cfg.preferences.theme.as_str())
                    || !["red_up", "green_up"].contains(&cfg.preferences.colors.as_str())
                {
                    return Err(AppError::InvalidArg("无效外观设置".into()));
                }
                if !(280.0..=900.0).contains(&cfg.preferences.list_width) {
                    return Err(AppError::InvalidArg("列表宽度需为 280–900".into()));
                }
                let allowed = [
                    "symbol",
                    "name",
                    "last",
                    "change_pct",
                    "change_amt",
                    "turnover",
                    "amplitude",
                ];
                if cfg
                    .preferences
                    .columns
                    .iter()
                    .any(|c| !allowed.contains(&c.as_str()))
                {
                    return Err(AppError::InvalidArg("无效行情列".into()));
                }
                let mut seen = std::collections::HashSet::new();
                cfg.preferences.columns.retain(|c| seen.insert(c.clone()));
                for required in ["name", "symbol"] {
                    if !cfg.preferences.columns.iter().any(|c| c == required) {
                        cfg.preferences.columns.insert(0, required.into());
                    }
                }
                cfg.preferences.recent.truncate(30);
            }
            Ok(())
        })?;
        self.settings_view()
    }

    pub fn provider_test(&self) -> Result<Value, AppError> {
        self.data_provider("tushare")?.test_connection()
    }
}
