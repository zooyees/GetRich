//! Tushare EOD valuation and index acquisition; vendor units converted at the boundary.
use crate::{
    error::AppError,
    import::{ImportBundle, ListingImport},
    models::MetadataEntry,
    tushare::{self, Client, Table},
};
use serde_json::{json, Value};

fn listing(value: Value) -> Result<ListingImport, AppError> {
    serde_json::from_value(value).map_err(|e| AppError::Message(e.to_string()))
}
pub fn valuation_row(table: &Table, row: &Value) -> Option<ListingImport> {
    let code = table.str(row, "ts_code")?;
    let key = tushare::ts_code_to_key(&code).ok()?;
    let raw = table.str(row, "trade_date")?;
    let date = chrono::NaiveDate::parse_from_str(&raw, "%Y%m%d")
        .ok()?
        .to_string();
    let metadata = [
        ("circ_mv", "float_market_cap", 10000.0),
        ("total_mv", "market_cap", 10000.0),
        ("turnover_rate", "turnover_rate", 1.0),
    ]
    .into_iter()
    .filter_map(|(field, key, scale)| {
        let value = table.f64(row, field)? * scale;
        (value.is_finite() && value >= 0.0).then(|| MetadataEntry {
            key: key.into(),
            value: Some(value.to_string()),
            source: Some("tushare.daily_basic".into()),
            as_of: Some(date.clone()),
        })
    })
    .collect::<Vec<_>>();
    if metadata.is_empty() {
        return None;
    }
    listing(json!({"symbol":key.symbol,"market":key.market,"currency":"CNY","source":"tushare.daily_basic","source_symbol":code,"metadata":metadata})).ok()
}
impl Client {
    pub fn valuations_bundle(&self) -> Result<ImportBundle, AppError> {
        self.valuations_bundle_at(&self.last_open_day()?)
    }
    pub fn valuations_bundle_at(&self, date: &str) -> Result<ImportBundle, AppError> {
        validate_date(date)?;
        let mut bundle = ImportBundle::default();
        for offset in (0..100_000).step_by(6000) {
            let table = self.call(
                "daily_basic",
                json!({"trade_date":date,"limit":6000,"offset":offset}),
                "ts_code,trade_date,total_mv,circ_mv,turnover_rate",
            )?;
            for row in &table.items {
                let listing = valuation_row(&table, row).ok_or_else(|| {
                    AppError::Message("市值接口返回无效代码、日期或指标，未提交本批次".into())
                })?;
                if table.str(row, "trade_date").as_deref() != Some(date) {
                    return Err(AppError::Message(
                        "市值接口返回其他日期，未提交本批次".into(),
                    ));
                }
                bundle.listings.push(listing);
            }
            if table.items.len() < 6000 {
                if bundle.listings.is_empty() {
                    return Err(AppError::Message(format!(
                        "{date} 的市值尚未返回，请核对接口权限或等待数据发布"
                    )));
                }
                return Ok(bundle);
            }
        }
        Err(AppError::Message(
            "估值数据触及分页上限，未提交不完整批次".into(),
        ))
    }
    pub fn indices_bundle(&self) -> Result<ImportBundle, AppError> {
        self.indices_bundle_at(&self.last_open_day()?)
    }
    pub fn indices_bundle_at(&self, date: &str) -> Result<ImportBundle, AppError> {
        validate_date(date)?;
        let mut bundle = ImportBundle::default();
        for (code, name) in [
            ("000001.SH", "上证指数"),
            ("399001.SZ", "深证成指"),
            ("399006.SZ", "创业板指"),
            ("000688.SH", "科创50"),
            ("000300.SH", "沪深300"),
            ("000852.SH", "中证1000"),
        ] {
            let key = tushare::ts_code_to_key(code).map_err(AppError::InvalidArg)?;
            let table = self.call(
                "index_daily",
                json!({"ts_code":code,"trade_date":date}),
                "ts_code,trade_date,open,high,low,close,pre_close,pct_chg,vol,amount",
            )?;
            if table.items.is_empty() {
                return Err(AppError::Message(format!(
                    "指数 {name} 在 {date} 无日线，未提交本批次"
                )));
            }
            bundle.listings.push(listing(json!({"symbol":key.symbol,"market":key.market,"name":name,"currency":"CNY","instrument_type":"index","source":"tushare.index_daily","source_symbol":code}))?);
            for row in &table.items {
                if table.str(row, "trade_date").as_deref() != Some(date) {
                    return Err(AppError::Message(format!(
                        "指数 {name} 返回其他交易日，未提交本批次"
                    )));
                }
                if let Some((mut quote, bar)) = tushare::map_daily_row(&table, row, &key) {
                    quote.source = Some("tushare.index_daily".into());
                    bundle.quotes.push(quote);
                    if let Some(mut bar) = bar {
                        bar.source = Some("tushare.index_daily".into());
                        bundle.bars.push(bar);
                    }
                } else {
                    return Err(AppError::Message(format!(
                        "指数 {name} 返回无效行情，未提交本批次"
                    )));
                }
            }
        }
        Ok(bundle)
    }
}
fn validate_date(date: &str) -> Result<(), AppError> {
    chrono::NaiveDate::parse_from_str(date, "%Y%m%d")
        .map(|_| ())
        .map_err(|_| AppError::InvalidArg("无效的同步交易日期".into()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vendor_caps_are_ten_thousand_yuan_and_have_dates() {
        let t = Table {
            fields: [
                "ts_code",
                "trade_date",
                "circ_mv",
                "total_mv",
                "turnover_rate",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            items: vec![json!(["600519.SH", "20261023", 250.5, 300.0, 0.7])],
        };
        let listing = valuation_row(&t, &t.items[0]).unwrap();
        assert_eq!(listing.metadata[0].value.as_deref(), Some("2505000"));
        assert_eq!(listing.metadata[0].as_of.as_deref(), Some("2026-10-23"));
        assert_eq!(listing.metadata[2].value.as_deref(), Some("0.7"));
    }
}
