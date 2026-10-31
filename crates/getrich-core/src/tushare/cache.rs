//! Persistent local acquisition store. Live quotes are deliberately never cached.
use super::*;
use chrono::{Days, NaiveDate};
use rusqlite::{params, OptionalExtension};

fn date(s: &str) -> Option<NaiveDate> {
    let normalized = if s.get(4..5) == Some("-") {
        s.get(..10)?.replace('-', "")
    } else {
        s.get(..8)?.to_string()
    };
    NaiveDate::parse_from_str(&normalized, "%Y%m%d").ok()
}
fn missing(
    from: NaiveDate,
    to: NaiveDate,
    mut covered: Vec<(NaiveDate, NaiveDate)>,
) -> Vec<(NaiveDate, NaiveDate)> {
    covered.sort_unstable();
    let mut next = from;
    let mut out = Vec::new();
    for (a, b) in covered {
        if b < next || a > to {
            continue;
        }
        if a > next {
            out.push((next, a.pred_opt().unwrap().min(to)));
        }
        next = next.max(b.succ_opt().unwrap());
        if next > to {
            return out;
        }
    }
    if next <= to {
        out.push((next, to));
    }
    out
}

fn confirmed_end(
    table: &Table,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
) -> Option<NaiveDate> {
    // Recent nonempty replies can still be missing the not-yet-published tail.
    let end = if to < today - Days::new(7) {
        to
    } else {
        table
            .items
            .iter()
            .filter_map(|r| table.str(r, "trade_date").and_then(|s| date(&s)))
            .max()?
            .min(today.pred_opt()?)
            .min(to)
    };
    (end >= from).then_some(end)
}

impl Client {
    pub(super) fn response_cache_key(
        &self,
        api: &str,
        p: &Value,
        fields: &str,
        live: bool,
    ) -> Option<String> {
        if self.shared_store.is_none() || (live && api != "trade_cal") || api.starts_with("rt_") {
            return None;
        }
        Some(json!([self.endpoint, self.account, api, p, fields]).to_string())
    }
    pub(super) fn cached_response(&self, key: &str) -> Result<Option<Table>, AppError> {
        let Some(db) = &self.shared_store else {
            return Ok(None);
        };
        let conn = db.connect()?;
        let body: Option<String> = conn
            .query_row(
                "SELECT response_json FROM provider_cache WHERE cache_key=?1 AND expires_ms>?2",
                params![key, now_ms()],
                |r| r.get(0),
            )
            .optional()
            .map_err(crate::db::DbError::from)?;
        Ok(body.and_then(|s| serde_json::from_str(&s).ok()))
    }
    pub(super) fn store_response(
        &self,
        key: &str,
        api: &str,
        p: &Value,
        table: &Table,
    ) -> Result<(), AppError> {
        let Some(db) = &self.shared_store else {
            return Ok(());
        };
        if matches!(api, "daily" | "index_daily")
            && table.items.iter().any(|row| {
                let Some(key) = table
                    .str(row, "ts_code")
                    .and_then(|code| ts_code_to_key(&code).ok())
                else {
                    return true;
                };
                !map_daily_row(table, row, &key)
                    .and_then(|(_, bar)| bar)
                    .is_some_and(|bar| {
                        bar.low > 0.0
                            && bar.high >= bar.open
                            && bar.high >= bar.close
                            && bar.low <= bar.open
                            && bar.low <= bar.close
                            && bar.volume.is_none_or(|n| n.is_finite() && n >= 0.0)
                            && bar.turnover.is_none_or(|n| n.is_finite() && n >= 0.0)
                    })
            })
        {
            return Ok(());
        }
        let today = session::yyyymmdd(session::shanghai_today());
        let end = p["trade_date"].as_str().or_else(|| p["end_date"].as_str());
        let immutable = matches!(
            api,
            "daily" | "index_daily" | "daily_basic" | "moneyflow" | "stk_mins"
        ) && end
            .and_then(date)
            .is_some_and(|d| session::yyyymmdd(d) < today)
            && !table.items.is_empty();
        let ttl = if table.items.is_empty() {
            60_000
        } else if immutable {
            100 * 365 * 86_400_000u64
        } else if matches!(
            api,
            "trade_cal" | "stock_basic" | "stock_company" | "fina_indicator" | "top10_holders"
        ) {
            86_400_000
        } else {
            300_000
        };
        let conn = db.connect()?;
        conn.execute("INSERT INTO provider_cache VALUES(?1,?2,?3,?4) ON CONFLICT(cache_key) DO UPDATE SET response_json=excluded.response_json,fetched_ms=excluded.fetched_ms,expires_ms=excluded.expires_ms",params![key,serde_json::to_string(table).map_err(|e|AppError::Message(e.to_string()))?,now_ms(),now_ms().saturating_add(ttl)]).map_err(crate::db::DbError::from)?;
        Ok(())
    }

    pub(crate) fn history_table(
        &self,
        api: &str,
        code: &str,
        start: &str,
        end: &str,
        fields: &str,
    ) -> Result<Table, AppError> {
        let from = date(start).ok_or_else(|| AppError::InvalidArg("历史开始日期无效".into()))?;
        let to = date(end).ok_or_else(|| AppError::InvalidArg("历史结束日期无效".into()))?;
        if from > to {
            return Err(AppError::InvalidArg("历史日期区间倒置".into()));
        }
        let fetch = |a: NaiveDate, b: NaiveDate| {
            self.paged_table(api,json!({"ts_code":code,"start_date":session::yyyymmdd(a),"end_date":session::yyyymmdd(b)}),fields)
        };
        let Some(db) = &self.shared_store else {
            return fetch(from, to);
        };
        let scope = json!([self.endpoint, self.account, api, code, fields]).to_string();
        let conn = db.connect()?;
        // Reuse pre-existing local daily bars too. A lone imported bar does not
        // prove the surrounding dates are complete, so mark only known dates.
        let key = ts_code_to_key(code).map_err(AppError::InvalidArg)?;
        if let Some(listing) =
            crate::db::get_listing(&conn, &key)?.filter(|_| matches!(api, "daily" | "index_daily"))
        {
            let mut stmt=conn.prepare("SELECT replace(ts,'-',''),open,high,low,close,volume,turnover FROM bars WHERE instrument_id=?1 AND timeframe='1d' AND ts<?2 AND source NOT LIKE '%rt_k%' ORDER BY ts").map_err(crate::db::DbError::from)?;
            let rows = stmt
                .query_map(
                    params![listing.id, session::iso_date(session::shanghai_today())],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            json!([
                                code,
                                r.get::<_, String>(0)?,
                                r.get::<_, f64>(1)?,
                                r.get::<_, f64>(2)?,
                                r.get::<_, f64>(3)?,
                                r.get::<_, f64>(4)?,
                                null,
                                r.get::<_, Option<f64>>(5)?.map(|n| n / 100.0),
                                r.get::<_, Option<f64>>(6)?.map(|n| n / 1000.0)
                            ]),
                        ))
                    },
                )
                .map_err(crate::db::DbError::from)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(crate::db::DbError::from)?;
            drop(stmt);
            let tx = conn
                .unchecked_transaction()
                .map_err(crate::db::DbError::from)?;
            let manifest:Option<String>=tx.query_row("SELECT rows_json FROM stock_research WHERE market=?1 AND symbol=?2 AND dataset='history' AND updated_at IS NOT NULL AND error IS NULL AND source LIKE '%daily%'",params![key.market.as_str(),key.symbol],|r|r.get(0)).optional().map_err(crate::db::DbError::from)?;
            if let Some(proof) = manifest
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_array().and_then(|a| a.first()).cloned())
            {
                if let Some((a, b)) = proof["from"]
                    .as_str()
                    .and_then(date)
                    .zip(proof["trade_date"].as_str().and_then(date))
                {
                    let n = rows
                        .iter()
                        .filter(|(day, _)| date(day).is_some_and(|d| d >= a && d <= b))
                        .count();
                    if proof["count"].as_u64() == Some(n as u64)
                        && n > 0
                        && b < session::shanghai_today()
                    {
                        tx.execute(
                            "INSERT OR IGNORE INTO provider_coverage VALUES(?1,?2,?3)",
                            params![scope, session::yyyymmdd(a), session::yyyymmdd(b)],
                        )
                        .map_err(crate::db::DbError::from)?;
                    }
                }
            }
            for (day, row) in rows {
                tx.execute(
                    "INSERT OR IGNORE INTO provider_history VALUES(?1,?2,?3)",
                    params![scope, day, row.to_string()],
                )
                .map_err(crate::db::DbError::from)?;
                tx.execute(
                    "INSERT OR IGNORE INTO provider_coverage VALUES(?1,?2,?2)",
                    params![scope, day],
                )
                .map_err(crate::db::DbError::from)?;
            }
            tx.commit().map_err(crate::db::DbError::from)?;
        }
        let covered = {
            let mut stmt = conn
                .prepare("SELECT from_date,to_date FROM provider_coverage WHERE scope=?1")
                .map_err(crate::db::DbError::from)?;
            let rows = stmt
                .query_map([&scope], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(crate::db::DbError::from)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(crate::db::DbError::from)?
                .into_iter()
                .filter_map(|(a, b)| Some((date(&a)?, date(&b)?)))
                .collect()
        };
        drop(conn);
        let today = session::shanghai_today();
        // Persist completed intervals only. The current candle remains refreshable.
        for (a, b) in missing(from, to, covered) {
            let mut next = a;
            while next <= b {
                let stop =
                    next.checked_add_days(Days::new(365))
                        .unwrap()
                        .min(b)
                        .min(if next < today {
                            today.pred_opt().unwrap()
                        } else {
                            b
                        });
                let table = fetch(next, stop)?;
                if table.fields != fields.split(',').map(str::to_string).collect::<Vec<_>>() {
                    return Err(AppError::Message("历史字段不完整，未标记本地区间".into()));
                }
                let key = ts_code_to_key(code).map_err(AppError::InvalidArg)?;
                for row in &table.items {
                    let row_date = table.str(row, "trade_date").and_then(|s| date(&s));
                    let valid = !matches!(api, "daily" | "index_daily")
                        || map_daily_row(&table, row, &key)
                            .and_then(|(_, b)| b)
                            .is_some_and(|b| {
                                b.low > 0.0
                                    && b.low <= b.open
                                    && b.low <= b.close
                                    && b.high >= b.open
                                    && b.high >= b.close
                                    && b.volume.is_none_or(|n| n.is_finite() && n >= 0.0)
                                    && b.turnover.is_none_or(|n| n.is_finite() && n >= 0.0)
                            });
                    if table.str(row, "ts_code").as_deref() != Some(code)
                        || !row_date.is_some_and(|d| d >= next && d <= stop)
                        || !valid
                    {
                        return Err(AppError::Message("历史记录无效，未标记本地区间".into()));
                    }
                }
                let conn = db.connect()?;
                let tx = conn
                    .unchecked_transaction()
                    .map_err(crate::db::DbError::from)?;
                for row in &table.items {
                    tx.execute("INSERT INTO provider_history VALUES(?1,?2,?3) ON CONFLICT(scope,trade_date) DO UPDATE SET row_json=excluded.row_json",params![scope,table.str(row,"trade_date"),row.to_string()]).map_err(crate::db::DbError::from)?;
                }
                if let Some(through) = confirmed_end(&table, next, stop, today) {
                    tx.execute(
                        "INSERT OR IGNORE INTO provider_coverage VALUES(?1,?2,?3)",
                        params![scope, session::yyyymmdd(next), session::yyyymmdd(through)],
                    )
                    .map_err(crate::db::DbError::from)?;
                }
                tx.commit().map_err(crate::db::DbError::from)?;
                next = stop.succ_opt().unwrap();
            }
        }
        let conn = db.connect()?;
        let mut stmt=conn.prepare("SELECT row_json FROM provider_history WHERE scope=?1 AND trade_date>=?2 AND trade_date<=?3 ORDER BY trade_date").map_err(crate::db::DbError::from)?;
        let rows = stmt
            .query_map(
                params![scope, session::yyyymmdd(from), session::yyyymmdd(to)],
                |r| r.get::<_, String>(0),
            )
            .map_err(crate::db::DbError::from)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(
                serde_json::from_str(&row.map_err(crate::db::DbError::from)?)
                    .map_err(|e| AppError::Message(e.to_string()))?,
            );
        }
        Ok(Table {
            fields: fields.split(',').map(str::to_string).collect(),
            items,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_partial_publication_does_not_mark_the_missing_tail_complete() {
        let d = |s| date(s).unwrap();
        let table = Table {
            fields: vec!["trade_date".into()],
            items: vec![json!(["20261029"])],
        };
        assert_eq!(
            confirmed_end(&table, d("20261001"), d("20261030"), d("20261031")),
            Some(d("20261029"))
        );
        assert_eq!(
            confirmed_end(&table, d("20261030"), d("20261031"), d("20261031")),
            None
        );
    }
    fn store() -> crate::db::Database {
        crate::db::Database::at(std::env::temp_dir().join(format!(
            "getrich-local-cache-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        )))
    }
    #[test]
    fn history_survives_restart_and_fetches_only_new_dates() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for days in [vec!["20240102", "20240105"], vec!["20240108"]] {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut data = Vec::new();
                let mut byte = [0];
                while !data.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    data.push(byte[0]);
                }
                let header = String::from_utf8(data).unwrap();
                let len = header
                    .lines()
                    .find_map(|s| {
                        s.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                let mut body = vec![0; len];
                stream.read_exact(&mut body).unwrap();
                requests.push(serde_json::from_slice::<Value>(&body).unwrap());
                let items: Vec<_> = days
                    .into_iter()
                    .map(|d| json!(["600519.SH", d, 10, 12, 9, 11, 10, 10, 100]))
                    .collect();
                let body=json!({"code":0,"data":{"fields":["ts_code","trade_date","open","high","low","close","pre_close","vol","amount"],"items":items}}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
            requests
        });
        let db = store();
        let make = || {
            let mut c = Client::new("".into(), "fixture-token".into(), 1000, 1000, 50)
                .unwrap()
                .with_shared_store(db.clone());
            c.endpoint = endpoint.clone();
            c
        };
        let key = ts_code_to_key("600519.SH").unwrap();
        assert_eq!(
            make()
                .bars_bundle(&key, "20240102", "20240105")
                .unwrap()
                .bars
                .iter()
                .filter(|b| b.timeframe.as_deref() == Some("1d"))
                .count(),
            2
        );
        // A new client reads covered dates from disk, including an overlapping subset.
        assert_eq!(
            make()
                .bars_bundle(&key, "20240103", "20240105")
                .unwrap()
                .bars
                .iter()
                .filter(|b| b.timeframe.as_deref() == Some("1d"))
                .count(),
            1
        );
        make().bars_bundle(&key, "20240102", "20240108").unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["params"]["start_date"], "20240106");
        // Exact successful responses also survive client recreation (before budget/network).
        let replay = make()
            .call(
                "daily",
                requests[0]["params"].clone(),
                requests[0]["fields"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(replay.items.len(), 2);
        // Server is gone: the full range must still load without a network request.
        let result = make().bars_bundle(&key, "20240102", "20240108").unwrap();
        assert_eq!(
            result
                .bars
                .iter()
                .filter(|b| b.timeframe.as_deref() == Some("1d"))
                .count(),
            3
        );
        assert!(make()
            .response_cache_key("rt_k", &json!({}), "", true)
            .is_none());
        assert!(make()
            .response_cache_key("rt_min_daily", &json!({}), "", false)
            .is_none());
        let conn = db.connect().unwrap();
        let all: String = conn
            .query_row(
                "SELECT group_concat(cache_key) FROM provider_cache",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!all.contains("fixture-token"));
    }
    #[test]
    fn imported_local_daily_bars_are_reused_without_a_server() {
        let db = store();
        let app = crate::service::App::at_path(Default::default(), &db.path);
        app.import_bundle(&crate::import::parse_json(&json!({"listings":[{"symbol":"600519","market":"CN.SH"}],"bars":[{"symbol":"600519","market":"CN.SH","ts":"2024-01-02","open":10,"high":12,"low":9,"close":11,"volume":1000}]}).to_string(),crate::import::ImportKind::Bundle).unwrap(),"fixture").unwrap();
        let mut c = Client::new("".into(), "fixture".into(), 1000, 1000, 50)
            .unwrap()
            .with_shared_store(db);
        c.endpoint = "http://127.0.0.1:1".into();
        let bars = c
            .bars_bundle(
                &ts_code_to_key("600519.SH").unwrap(),
                "20240102",
                "20240102",
            )
            .unwrap();
        assert_eq!(bars.bars[0].close, 11.0);
        assert_eq!(bars.bars[0].volume, Some(1000.0));
    }
    #[test]
    fn coverage_subtracts_disjoint_and_overlapping_ranges_without_hiding_holes() {
        let d = |s| date(s).unwrap();
        let c = vec![
            (d("20200102"), d("20200103")),
            (d("20200103"), d("20200105")),
            (d("20200108"), d("20200109")),
        ];
        assert_eq!(
            missing(d("20200101"), d("20200110"), c),
            vec![
                (d("20200101"), d("20200101")),
                (d("20200106"), d("20200107")),
                (d("20200110"), d("20200110"))
            ]
        );
    }
}
