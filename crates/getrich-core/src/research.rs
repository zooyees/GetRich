//! Cached stock research. Vendor calls run only in the durable job worker.
use crate::{
    db,
    error::AppError,
    jobs,
    models::InstrumentKey,
    service::App,
    session,
    tushare::{Client, Table},
};
use chrono::{NaiveDate, NaiveDateTime, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

const DATASETS: &[&str] = &[
    "history",
    "intraday",
    "company",
    "financial",
    "valuation",
    "holders",
    "funds",
    "announcements",
];

fn cache(conn: &Connection, key: &InstrumentKey, dataset: &str) -> Result<Value, db::DbError> {
    let entry: Option<Value> = conn.query_row(
        "SELECT rows_json,source,as_of,updated_at,attempted_at,error FROM stock_research WHERE market=?1 AND symbol=?2 AND dataset=?3",
        params![key.market.as_str(),key.symbol,dataset], |r| {
            let rows: String = r.get(0)?;
            Ok(json!({"rows":serde_json::from_str::<Value>(&rows).unwrap_or(json!([])),"source":r.get::<_,String>(1)?,"as_of":r.get::<_,Option<String>>(2)?,"updated_at":r.get::<_,Option<String>>(3)?,"attempted_at":r.get::<_,String>(4)?,"error":r.get::<_,Option<String>>(5)?}))
        }).optional()?;
    let mut v = entry.unwrap_or(json!({"rows":[],"source":"Tushare","as_of":null,"updated_at":null,"attempted_at":null,"error":null}));
    let age = |field: &str| {
        v[field]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds())
    };
    let ttl = match dataset {
        "intraday" => 30,
        "announcements" => 300,
        _ => 86_400,
    };
    let fresh = age("updated_at").is_some_and(|s| s < ttl);
    let cooldown = age("attempted_at").is_some_and(|s| s < 60) && !v["error"].is_null();
    let has_rows = v["rows"].as_array().is_some_and(|a| !a.is_empty());
    v["needs_refresh"] = json!(!fresh && !cooldown);
    v["status"] = json!(if !v["error"].is_null() {
        if has_rows {
            "stale"
        } else {
            "error"
        }
    } else if v["updated_at"].is_null() {
        "missing"
    } else if has_rows {
        "ready"
    } else {
        "empty"
    });
    Ok(v)
}

fn save(
    conn: &Connection,
    key: &InstrumentKey,
    dataset: &str,
    source: &str,
    result: Result<Vec<Value>, AppError>,
) -> Result<Option<String>, AppError> {
    let now = db::now_rfc3339();
    let (rows, date, error) = match result {
        Ok(rows) => {
            let date = rows
                .iter()
                .filter_map(|v| {
                    ["trade_date", "end_date", "ann_date", "date"]
                        .iter()
                        .find_map(|f| v[*f].as_str())
                })
                .max()
                .map(str::to_string);
            (Some(json!(rows).to_string()), date, None)
        }
        Err(e) => (None, None, Some(e.to_string())),
    };
    conn.execute("INSERT INTO stock_research(market,symbol,dataset,rows_json,source,as_of,updated_at,attempted_at,error) VALUES (?1,?2,?3,COALESCE(?4,'[]'),?5,?6,CASE WHEN ?4 IS NULL THEN NULL ELSE ?7 END,?7,?8) ON CONFLICT(market,symbol,dataset) DO UPDATE SET rows_json=COALESCE(?4,stock_research.rows_json),source=CASE WHEN ?4 IS NULL THEN stock_research.source ELSE ?5 END,as_of=CASE WHEN ?4 IS NULL THEN stock_research.as_of ELSE ?6 END,updated_at=CASE WHEN ?4 IS NULL THEN stock_research.updated_at ELSE ?7 END,attempted_at=?7,error=?8",params![key.market.as_str(),key.symbol,dataset,rows,source,date,now,error]).map_err(db::DbError::from)?;
    Ok(error)
}

impl App {
    pub fn stock_research(&self, key: &InstrumentKey) -> Result<Value, AppError> {
        let conn = self.db.connect()?;
        let mut datasets = Map::new();
        for dataset in DATASETS {
            let mut value = cache(&conn, key, dataset)?;
            if *dataset == "history" {
                let quote_date = self
                    .show_stock(key)
                    .ok()
                    .and_then(|d| d.quote.trade_date)
                    .map(|s| s.replace('-', ""));
                let through = value["as_of"].as_str().map(|s| s.replace('-', ""));
                if quote_date.zip(through).is_some_and(|(q, b)| q > b) && value["error"].is_null() {
                    value["needs_refresh"] = json!(true);
                }
            }
            datasets.insert((*dataset).into(), value);
        }
        if let Some(intraday) = datasets.get_mut("intraday") {
            let date = intraday["as_of"]
                .as_str()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y%m%d").ok());
            if let (Some(date), Some(listing)) = (date, db::get_listing(&conn, key)?) {
                let previous:Option<f64>=conn.query_row("SELECT close FROM bars WHERE instrument_id=?1 AND timeframe=\'1d\' AND ts<?2 ORDER BY ts DESC LIMIT 1",params![listing.id,date.format("%Y-%m-%d").to_string()],|r|r.get(0)).optional().map_err(db::DbError::from)?;
                let fallback = self
                    .show_stock(key)
                    .ok()
                    .filter(|d| {
                        d.quote.trade_date.as_deref() == Some(&date.format("%Y-%m-%d").to_string())
                    })
                    .and_then(|d| d.quote.prev_close);
                intraday["prev_close"] = json!(fallback.or(previous));
            }
        }
        Ok(
            json!({"schema_version":1,"symbol":key.symbol,"market":key.market.as_str(),"supported":key.market.as_str().starts_with("CN."),"datasets":datasets}),
        )
    }
}

fn objects(table: Table, code: &str) -> Result<Vec<Value>, AppError> {
    let mut rows = Vec::new();
    for row in &table.items {
        if table
            .str(row, "ts_code")
            .or_else(|| table.str(row, "code"))
            .is_some_and(|c| c != code)
        {
            continue;
        }
        let Some(cells) = row.as_array() else {
            return Err(AppError::Message("数据源返回了无效行".into()));
        };
        if cells.len() != table.fields.len() {
            return Err(AppError::Message("数据源字段与行不匹配".into()));
        }
        rows.push(Value::Object(
            table
                .fields
                .iter()
                .cloned()
                .zip(cells.iter().cloned())
                .collect(),
        ));
    }
    Ok(rows)
}

pub fn safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(char::is_control)
        && url
            .split("://")
            .nth(1)
            .is_some_and(|s| !s.is_empty() && !s.starts_with('/') && !s.contains('@'))
}

fn number(v: &Value, field: &str) -> Option<f64> {
    v[field]
        .as_f64()
        .or_else(|| v[field].as_str()?.parse().ok())
        .filter(|v| v.is_finite())
}

fn normalize(mut rows: Vec<Value>, dataset: &str, date: &str) -> Result<Vec<Value>, AppError> {
    if dataset == "intraday" {
        let mut ordered = std::collections::BTreeMap::new();
        for mut row in rows {
            let time = row["trade_time"]
                .as_str()
                .or_else(|| row["time"].as_str())
                .unwrap_or("");
            let Ok(dt) = NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S") else {
                continue;
            };
            if dt.format("%Y%m%d").to_string() != date {
                continue;
            }
            let minute = dt.hour() * 60 + dt.minute();
            let x = match minute {
                570..=690 => minute - 570,
                780..=900 => minute - 660,
                _ => continue,
            };
            let (Some(price), Some(vol), Some(amount)) = (
                number(&row, "close"),
                number(&row, "vol"),
                number(&row, "amount"),
            ) else {
                continue;
            };
            if price <= 0.0 || vol < 0.0 || amount < 0.0 {
                continue;
            }
            row["time"] = json!(time);
            row["trade_date"] = json!(date);
            row["minute"] = json!(x);
            row["close"] = json!(price);
            row["volume"] = json!(vol);
            row["amount"] = json!(amount);
            ordered.insert(dt, row);
        }
        let (mut volume, mut amount) = (0.0, 0.0);
        rows = ordered
            .into_values()
            .map(|mut row| {
                volume += row["volume"].as_f64().unwrap_or(0.0);
                amount += row["amount"].as_f64().unwrap_or(0.0);
                row["vwap"] = json!(if volume > 0.0 {
                    Some(amount / volume)
                } else {
                    None
                });
                row
            })
            .collect();
        if rows.is_empty() {
            return Err(AppError::Message(format!(
                "{date} 未返回有效分钟数据；请核对 A 股分钟接口权限或稍后重试"
            )));
        }
    } else {
        rows.retain(|row| {
            row["ann_date"].as_str().is_none_or(|d| d <= date)
                && row["trade_date"].as_str().is_none_or(|d| d <= date)
        });
        rows.sort_by(|a, b| {
            let f = if dataset == "financial" || dataset == "holders" {
                "end_date"
            } else if dataset == "announcements" {
                "ann_date"
            } else {
                "trade_date"
            };
            b[f].as_str()
                .cmp(&a[f].as_str())
                .then_with(|| b["ann_date"].as_str().cmp(&a["ann_date"].as_str()))
        });
        if dataset == "funds" {
            for row in &mut rows {
                let fields: Vec<String> = row
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                for field in fields {
                    if field.ends_with("_amount") {
                        row[&field] = json!(number(row, &field).map(|n| n * 10000.0));
                    } else if field.ends_with("_vol") {
                        row[&field] = json!(number(row, &field).map(|n| n * 100.0));
                    }
                }
                row["large_net"] = json!(number(row, "buy_lg_amount")
                    .zip(number(row, "sell_lg_amount"))
                    .map(|(a, b)| a - b));
                row["extra_large_net"] = json!(number(row, "buy_elg_amount")
                    .zip(number(row, "sell_elg_amount"))
                    .map(|(a, b)| a - b));
            }
            rows.truncate(60);
        }
        if dataset == "valuation" {
            for row in &mut rows {
                for field in [
                    "total_mv",
                    "circ_mv",
                    "total_share",
                    "float_share",
                    "free_share",
                ] {
                    row[field] = json!(number(row, field).map(|v| v * 10000.0));
                }
            }
        }
        if dataset == "announcements" {
            rows.retain(|r| r["title"].as_str().is_some_and(|t| !t.trim().is_empty()));
            for row in &mut rows {
                if !row["url"].as_str().is_some_and(safe_url) {
                    row["url"] = Value::Null;
                }
            }
            rows.dedup_by(|a, b| {
                a["ann_date"] == b["ann_date"] && a["title"] == b["title"] && a["url"] == b["url"]
            });
        }
        if dataset == "holders" {
            if let Some(period) = rows
                .first()
                .and_then(|r| r["end_date"].as_str())
                .map(str::to_string)
            {
                rows.retain(|r| r["end_date"].as_str() == Some(&period));
                rows.sort_by(|a, b| {
                    number(b, "hold_ratio")
                        .partial_cmp(&number(a, "hold_ratio"))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                rows.truncate(10);
            }
        }
        if dataset == "financial" {
            rows.dedup_by(|a, b| a["end_date"] == b["end_date"]);
            rows.truncate(8);
        }
    }
    Ok(rows)
}

fn fetch(
    client: &Client,
    key: &InstrumentKey,
    dataset: &str,
    date: &str,
) -> Result<(String, Vec<Value>), AppError> {
    let code = crate::tushare::key_to_ts_code(key)
        .ok_or_else(|| AppError::InvalidArg("当前资料源仅支持 A 股".into()))?;
    let day = NaiveDate::parse_from_str(date, "%Y%m%d")
        .map_err(|e| AppError::InvalidArg(e.to_string()))?;
    let from = session::yyyymmdd(session::days_before(day, 365));
    let (api,params,fields)=match dataset {
        "company"=>("stock_company",json!({"ts_code":code}),"ts_code,com_name,chairman,manager,secretary,reg_capital,setup_date,province,city,introduction,website,employees,main_business,office"),
        "financial"=>("fina_indicator",json!({"ts_code":code,"start_date":session::yyyymmdd(session::days_before(day,1000)),"end_date":date}),"ts_code,ann_date,end_date,eps,bps,roe,roa,ocfps,grossprofit_margin,netprofit_margin,debt_to_assets,or_yoy,netprofit_yoy,roic"),
        "valuation"=>("daily_basic",json!({"ts_code":code,"start_date":session::yyyymmdd(session::days_before(day,21)),"end_date":date}),"ts_code,trade_date,close,turnover_rate,volume_ratio,pe,pe_ttm,pb,dv_ttm,total_share,float_share,free_share,total_mv,circ_mv"),
        "holders"=>("top10_holders",json!({"ts_code":code,"start_date":from,"end_date":date}),"ts_code,ann_date,end_date,holder_name,hold_amount,hold_ratio,hold_change,holder_type"),
        "funds"=>("moneyflow",json!({"ts_code":code,"start_date":session::yyyymmdd(session::days_before(day,120)),"end_date":date}),"ts_code,trade_date,buy_sm_amount,sell_sm_amount,buy_md_amount,sell_md_amount,buy_lg_amount,sell_lg_amount,buy_elg_amount,sell_elg_amount,net_mf_amount"),
        "announcements"=>("anns_d",json!({"ts_code":code,"start_date":from,"end_date":session::yyyymmdd(session::shanghai_today())}),"ann_date,ts_code,name,title,url"),
        "intraday" if day==session::shanghai_today()=>("rt_min_daily",json!({"ts_code":code,"freq":"1MIN"}),""),
        "intraday"=>("stk_mins",json!({"ts_code":code,"freq":"1min","start_date":format!("{} 09:00:00",day.format("%Y-%m-%d")),"end_date":format!("{} 15:30:00",day.format("%Y-%m-%d"))}),"ts_code,trade_time,open,close,high,low,vol,amount"),
        _=>return Err(AppError::InvalidArg("未知资料集".into())),
    };
    let table = client.call(api, params, fields)?;
    Ok((
        format!("Tushare · {api}"),
        normalize(objects(table, &code)?, dataset, date)?,
    ))
}

pub(crate) fn execute(app: &App, id: i64, p: &Value) -> Result<Value, AppError> {
    let key = app.parse_key(p["symbol"].as_str().unwrap_or(""), p["market"].as_str())?;
    if !key.market.as_str().starts_with("CN.") {
        return Err(AppError::InvalidArg(
            "当前资料源仅支持 A 股，港美股可查看已导入日线".into(),
        ));
    }
    let section = p["section"].as_str().unwrap_or("history");
    let datasets: &[&str] = match section {
        "history" => &["history"],
        "intraday" => &["intraday"],
        "f10" => &["company", "financial", "valuation", "holders"],
        "funds" => &["funds"],
        "announcements" => &["announcements"],
        _ => return Err(AppError::InvalidArg("未知资料分区".into())),
    };
    let detail = app.show_stock(&key)?;
    let client = app.tushare_client()?;
    let date = if section == "intraday" {
        client.last_open_day()?
    } else {
        session::yyyymmdd(session::shanghai_today())
    };
    let mut warnings = Vec::new();
    for (index, dataset) in datasets.iter().enumerate() {
        jobs::stage(
            app,
            id,
            &format!("获取 {dataset}"),
            10 + (index * 75 / datasets.len()) as u32,
        )?;
        let source;
        let result = if *dataset == "history" {
            let history_api = if detail.listing.instrument_type == "index" {
                "index_daily"
            } else {
                "daily"
            };
            source = format!("Tushare · {history_api}");
            let start = detail
                .listing
                .listing_date
                .as_deref()
                .unwrap_or("19900101")
                .replace('-', "");
            (|| {
                let bundle = client.bars_bundle_api(&key, &start, &date, history_api)?;
                if bundle.bars.is_empty() {
                    return Err(AppError::Message(
                        "未返回历史日线，请检查证券代码、权限与日期".into(),
                    ));
                }
                jobs::stage(app, id, "正在原子写入", 75)?;
                let report = app.import_bundle(&bundle, "tushare.daily.history")?;
                let bars = app.show_stock(&key)?.bars;
                Ok(vec![
                    json!({"trade_date":bars.to,"from":bars.from,"count":bars.count,"report":report,"price_basis":"原始价格，未复权"}),
                ])
            })()
        } else {
            source = format!(
                "Tushare · {}",
                match *dataset {
                    "company" => "stock_company",
                    "financial" => "fina_indicator",
                    "valuation" => "daily_basic",
                    "holders" => "top10_holders",
                    "funds" => "moneyflow",
                    "announcements" => "anns_d",
                    "intraday" =>
                        if date == session::yyyymmdd(session::shanghai_today()) {
                            "rt_min_daily"
                        } else {
                            "stk_mins"
                        },
                    _ => "unknown",
                }
            );
            fetch(&client, &key, dataset, &date).map(|(_, rows)| rows)
        };
        if let Some(error) = save(&app.db.connect()?, &key, dataset, &source, result)? {
            warnings.push(format!("{dataset}: {error}"));
        }
    }
    app.events.emit(
        "stock.updated",
        json!({"symbol":key.symbol,"market":key.market.as_str(),"section":section}),
    );
    Ok(
        json!({"status":if warnings.is_empty(){"succeeded"}else{"partial"},"warnings":warnings,"research":app.stock_research(&key)?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minute_series_is_sorted_deduplicated_filtered_and_vwap_uses_shares() {
        let rows = normalize(
            vec![
                json!({"trade_time":"2026-10-23 13:01:00","close":12,"vol":100,"amount":1200}),
                json!({"trade_time":"2026-10-23 09:31:00","close":10,"vol":200,"amount":2000}),
                json!({"trade_time":"2026-10-22 09:31:00","close":9,"vol":100,"amount":900}),
                json!({"trade_time":"2026-10-23 12:01:00","close":99,"vol":100,"amount":9900}),
            ],
            "intraday",
            "20261023",
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["minute"], 1);
        assert_eq!(rows[1]["minute"], 121);
        assert!((rows[1]["vwap"].as_f64().unwrap() - 3200.0 / 300.0).abs() < 1e-8);
    }
    #[test]
    fn money_units_and_unsafe_announcement_links() {
        let rows = normalize(
            vec![json!({"buy_lg_amount":5,"sell_lg_amount":2,"net_mf_amount":-1})],
            "funds",
            "20261023",
        )
        .unwrap();
        assert_eq!(rows[0]["large_net"], 30000.0);
        assert_eq!(rows[0]["net_mf_amount"], -10000.0);
        let rows = normalize(
            vec![json!({"title":"公告","url":"javascript:alert(1)"})],
            "announcements",
            "20261023",
        )
        .unwrap();
        assert!(rows[0]["url"].is_null());
        assert!(safe_url("https://example.com/report.pdf"));
        assert!(!safe_url("file:///c:/secret"));
    }
    #[test]
    fn failed_refresh_preserves_last_valid_dataset_and_cooldown() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/006_stock_research.sql"))
            .unwrap();
        let key = crate::symbol::parse_instrument("600519", Some("CN.SH")).unwrap();
        save(
            &c,
            &key,
            "funds",
            "test",
            Ok(vec![json!({"trade_date":"20261023","net_mf_amount":123.0})]),
        )
        .unwrap();
        save(
            &c,
            &key,
            "funds",
            "test",
            Err(AppError::Message("权限不足".into())),
        )
        .unwrap();
        let v = cache(&c, &key, "funds").unwrap();
        assert_eq!(v["status"], "stale");
        assert_eq!(v["rows"][0]["net_mf_amount"], 123.0);
        assert_eq!(v["needs_refresh"], false);
        assert_eq!(v["as_of"], "20261023");
    }
}
