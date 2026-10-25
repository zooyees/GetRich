//! Stable provider capabilities and acquisition contract.
use crate::{
    error::AppError, import::ImportBundle, models::InstrumentKey, service::App, tushare::Client,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub trait DataProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn markets(&self) -> &'static [&'static str];
    fn test_connection(&self) -> Result<Value, AppError>;
    fn listings(&self) -> Result<ImportBundle, AppError>;
    fn snapshot(&self) -> Result<ImportBundle, AppError>;
    fn valuations(&self) -> Result<ImportBundle, AppError>;
    fn indices(&self) -> Result<ImportBundle, AppError>;
    fn valuations_at(&self, _date: &str) -> Result<ImportBundle, AppError> {
        self.valuations()
    }
    fn indices_at(&self, _date: &str) -> Result<ImportBundle, AppError> {
        self.indices()
    }
    fn history(&self, key: &InstrumentKey, from: &str, to: &str) -> Result<ImportBundle, AppError>;
}
impl DataProvider for Client {
    fn id(&self) -> &'static str {
        "tushare"
    }
    fn markets(&self) -> &'static [&'static str] {
        &["CN.SH", "CN.SZ", "CN.BJ"]
    }
    fn test_connection(&self) -> Result<Value, AppError> {
        Ok(
            json!({"ok":true,"last_open_day":self.last_open_day()?,"message":"连接成功，交易日历权限可用；其他接口权限以同步任务结果为准。"}),
        )
    }
    fn listings(&self) -> Result<ImportBundle, AppError> {
        self.listings_bundle()
    }
    fn snapshot(&self) -> Result<ImportBundle, AppError> {
        self.board_bundle(false)
    }
    fn valuations(&self) -> Result<ImportBundle, AppError> {
        self.valuations_bundle()
    }
    fn indices(&self) -> Result<ImportBundle, AppError> {
        self.indices_bundle()
    }
    fn valuations_at(&self, date: &str) -> Result<ImportBundle, AppError> {
        self.valuations_bundle_at(date)
    }
    fn indices_at(&self, date: &str) -> Result<ImportBundle, AppError> {
        self.indices_bundle_at(date)
    }
    fn history(&self, key: &InstrumentKey, from: &str, to: &str) -> Result<ImportBundle, AppError> {
        self.bars_bundle(key, from, to)
    }
}
impl App {
    pub fn data_provider(&self, id: &str) -> Result<Arc<dyn DataProvider>, AppError> {
        match id {
            "tushare" => Ok(self.tushare_client()?),
            _ => Err(AppError::InvalidArg(format!("未知数据源：{id}"))),
        }
    }
    pub fn provider_capabilities(&self) -> Value {
        json!({"schema_version":1,"providers":[{"id":"tushare","name":"Tushare","markets":["CN.SH","CN.SZ","CN.BJ"],"datasets":["quotes","bars:1d","calendar","valuations:daily","indices:daily","intraday:1m","company","financial","valuation","holders","funds","announcements"],"configured":self.tushare_status()["configured"],"configuration_fields":[{"id":"user_id","type":"text"},{"id":"token","type":"secret"},{"id":"poll_ms","type":"integer"}]}]})
    }
}
