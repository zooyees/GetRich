//! Local, style-specific research views. Dataset clocks are deliberately separate.
use crate::{
    analysis::{note_on, research_mode},
    error::AppError,
    models::InstrumentKey,
    service::App,
    session,
};
use chrono::{NaiveDate, Timelike, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn number(row: &Value, field: &str) -> Option<f64> {
    row[field].as_f64().filter(|v| v.is_finite())
}
fn date(value: &Value) -> Option<String> {
    let raw = value.as_str()?;
    NaiveDate::parse_from_str(raw, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(raw, "%Y-%m-%d"))
        .ok()
        .map(|d| d.to_string())
}
fn item(label: &str, value: Option<f64>) -> Value {
    json!({"label":label,"value":value})
}
fn provenance(dataset: &Value) -> Value {
    json!({"source":dataset["source"],"updated_at":dataset["updated_at"],"status":dataset["status"],"error":dataset["error"]})
}
fn intraday(dataset: &Value, cutoff: &str) -> Value {
    let now = Utc::now();
    let today = session::iso_date(session::shanghai_today());
    let mut days = BTreeMap::<String, BTreeMap<chrono::DateTime<Utc>, &Value>>::new();
    let mut excluded = 0;
    for row in dataset["rows"].as_array().into_iter().flatten() {
        let ts = row["time"]
            .as_str()
            .and_then(|t| session::parse_cn_ts(t).ok());
        let day = date(&row["trade_date"]);
        let minute = row["minute"].as_i64().filter(|m| (0..=240).contains(m));
        if let (Some(ts), Some(day), Some(minute), Some(_)) =
            (ts, day, minute, number(row, "close").filter(|p| *p > 0.0))
        {
            let clock = ts.with_timezone(&session::shanghai_tz());
            let clock_day = clock.date_naive().to_string();
            let clock_minute = clock.hour() * 60 + clock.minute();
            let expected = match clock_minute {
                570..=690 => Some((clock_minute - 570) as i64),
                780..=900 => Some((clock_minute - 660) as i64),
                _ => None,
            };
            if ts <= now && day.as_str() <= cutoff && clock_day == day && expected == Some(minute) {
                days.entry(day).or_default().insert(ts, row);
                continue;
            }
        }
        excluded += 1;
    }
    let latest = days.last_key_value();
    let day = latest.map(|(day, _)| day.as_str());
    let rows: Vec<_> = latest
        .into_iter()
        .flat_map(|(_, rows)| rows.values().copied())
        .collect();
    let last = rows.last().copied().unwrap_or(&Value::Null);
    let last_ts = last["time"]
        .as_str()
        .and_then(|t| session::parse_cn_ts(t).ok());
    let fresh = day == Some(today.as_str())
        && last_ts.is_some_and(|t| (now - t).num_seconds() <= 120)
        && dataset["error"].is_null();
    let gaps: i64 = rows
        .windows(2)
        .map(|w| (w[1]["minute"].as_i64().unwrap() - w[0]["minute"].as_i64().unwrap() - 1).max(0))
        .sum();
    let sum = |field: &str| -> Option<f64> {
        if rows.is_empty() {
            return None;
        }
        rows.iter()
            .map(|r| number(r, field).filter(|v| *v >= 0.0))
            .collect::<Option<Vec<_>>>()
            .map(|v| v.iter().sum())
    };
    let volume = sum("volume");
    let cash = sum("amount");
    let vwap = volume
        .zip(cash)
        .filter(|(v, _)| *v > 0.0)
        .map(|(v, c)| c / v);
    let last_price = number(last, "close");
    let ret15 = if rows.len() >= 16 {
        let window = &rows[rows.len() - 16..];
        window
            .windows(2)
            .all(|w| w[1]["minute"].as_i64().unwrap() - w[0]["minute"].as_i64().unwrap() == 1)
            .then(|| {
                last_price
                    .zip(number(window[0], "close"))
                    .map(|(a, b)| (a / b - 1.0) * 100.0)
            })
            .flatten()
    } else {
        None
    };
    let low = rows
        .iter()
        .filter_map(|r| number(r, "close"))
        .reduce(f64::min);
    let high = rows
        .iter()
        .filter_map(|r| number(r, "close"))
        .reduce(f64::max);
    let mut warnings = vec![
        "分钟统计仅覆盖本地已缓存样本；样本高低使用分钟收盘价，不等同全天最高最低价。".to_string(),
    ];
    if !fresh {
        warnings.push("当前为历史或延迟分钟数据，不标记为实时；可在行情分时页更新，接口权限以数据源返回为准。".into());
    }
    if rows.is_empty() {
        warnings.push("缺少有效分钟数据：不会用日线报价替代分时。".into());
    }
    if gaps > 0 {
        warnings.push(format!(
            "分钟序列缺口 {gaps} 个；15分钟收益仅在最近16个样本连续时计算。"
        ));
    }
    json!({"title":"盘中短线","as_of":last["time"],"trade_date":day,"sample_count":rows.len(),"excluded":excluded,"missing_minutes":gaps,"fresh":fresh,"provenance":{"intraday":provenance(dataset)},"warnings":warnings,"methodology":"成交均价=样本成交额(元)/样本成交量(股)；15分钟收益按连续交易分钟计算，午休不计入；不使用日线填补分钟缺口。","metrics":[item("最新分钟收盘",last_price),item("样本成交均价",vwap),item("偏离样本均价 %",last_price.zip(vwap).map(|(p,v)|(p/v-1.0)*100.0)),item("连续15交易分钟收益 %",ret15),item("样本收盘最高",high),item("样本收盘最低",low),item("样本成交量 / 股",volume),item("样本成交额 / 元",cash)]})
}
fn fundamental(datasets: &Value, cutoff: &str) -> Value {
    let financial = &datasets["financial"];
    let valuation = &datasets["valuation"];
    let rows: Vec<_> = financial["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            date(&r["ann_date"]).is_some_and(|d| d.as_str() <= cutoff)
                && date(&r["end_date"]).is_some_and(|d| d.as_str() <= cutoff)
        })
        .collect();
    let f = rows
        .iter()
        .max_by_key(|r| (date(&r["end_date"]), date(&r["ann_date"])))
        .copied()
        .unwrap_or(&Value::Null);
    let v = valuation["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| date(&r["trade_date"]).is_some_and(|d| d.as_str() <= cutoff))
        .max_by_key(|r| date(&r["trade_date"]))
        .unwrap_or(&Value::Null);
    let cash_eps = number(f, "ocfps")
        .zip(number(f, "eps"))
        .filter(|(_, eps)| *eps > 0.0)
        .map(|(cash, eps)| cash / eps);
    let mut warnings = vec![
        "财务报表期、公告日与估值交易日分别标注；仅展示截止日期前已披露的缓存记录。".to_string(),
        "缓存可能包含后续修订值，尚不具备完整历史版本，不能作为无前视偏差回测的数据源。".into(),
        "每股收益与经营现金流通常为报告期累计口径；不将季度数值直接当作全年数值。".into(),
    ];
    if f.is_null() {
        warnings.push("缺少截止日期前有效披露的财务数据；未知值显示 —。".into());
    }
    if v.is_null() {
        warnings.push("缺少截止日期前的估值数据；不会推算或填零。".into());
    }
    json!({"title":"基本面长线","as_of":date(&v["trade_date"]),"report_period":date(&f["end_date"]),"disclosed_at":date(&f["ann_date"]),"sample_count":rows.len(),"excluded_financial":financial["rows"].as_array().map_or(0,|r|r.len())-rows.len(),"provenance":{"financial":provenance(financial),"valuation":provenance(valuation)},"warnings":warnings,"methodology":"ROE/毛利率/净利率/资产负债率/同比增长率直接采用提供商字段；现金收益比=同一报告期每股经营现金流/每股收益，仅EPS>0时计算；估值日期与财报日期不混用。","metrics":[item("市盈率 TTM",number(v,"pe_ttm")),item("市净率",number(v,"pb")),item("股息率 TTM %",number(v,"dv_ttm")),item("ROE %",number(f,"roe")),item("营收同比 %",number(f,"or_yoy")),item("净利润同比 %",number(f,"netprofit_yoy")),item("资产负债率 %",number(f,"debt_to_assets")),item("毛利率 %",number(f,"grossprofit_margin")),item("净利率 %",number(f,"netprofit_margin")),item("每股收益",number(f,"eps")),item("每股经营现金流",number(f,"ocfps")),item("现金收益比",cash_eps)]})
}
impl App {
    pub fn analysis_workspace(
        &self,
        key: &InstrumentKey,
        as_of: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Value, AppError> {
        self.analysis_workspace_basis(key, as_of, mode, "raw", "")
    }
    pub fn analysis_workspace_basis(
        &self,
        key: &InstrumentKey,
        as_of: Option<&str>,
        mode: Option<&str>,
        basis: &str,
        source: &str,
    ) -> Result<Value, AppError> {
        let mode = research_mode(mode)?;
        let mut report = self.stock_analysis(key, as_of)?;
        if mode == "swing" && basis != "raw" {
            let page = self.list_bars_price(
                key,
                crate::models::Timeframe::D1,
                None,
                as_of,
                20000,
                basis,
                source,
            )?;
            report["metrics"] = crate::analysis::metrics(&page.rows);
            report["price_basis"] = json!(basis);
            report["adjustment"] = page.adjustment.clone();
            report["warnings"] = json!([
                page.adjustment["warning"],
                "指标口径与当前复权K线一致；资料不足的指标仍显示未知。"
            ]);
            report["sample_count"] = json!(page.count);
            report["from"] = json!(page.from);
            report["as_of"] = json!(page.to);
            report["data_version"] = json!(page.data_version);
        }
        let cutoff = report["evaluation_date"].as_str().unwrap();
        let workspace = if mode == "swing" {
            json!({"title":"日线波段","adjustment":report["adjustment"],"as_of":report["as_of"],"sample_count":report["sample_count"],"warnings":[],"methodology":report["methodology"]})
        } else {
            let cached = self.stock_research(key)?;
            if mode == "intraday" {
                intraday(&cached["datasets"]["intraday"], cutoff)
            } else {
                fundamental(&cached["datasets"], cutoff)
            }
        };
        report["mode"] = json!(mode);
        report["workspace"] = workspace;
        report["note"] = note_on(&self.db.connect()?, key, mode)?;
        Ok(report)
    }
}
