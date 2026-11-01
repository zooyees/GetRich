use super::*;
use crate::models::{Bar, InstrumentKey, Timeframe};
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};

fn before(row: &Value, cutoff: &str) -> bool {
    let raw = if text(&row["payload"], "known_at").is_empty() {
        text(row, "observed_at")
    } else {
        text(&row["payload"], "known_at")
    };
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .zip(chrono::DateTime::parse_from_rfc3339(cutoff).ok())
        .is_some_and(|(known, limit)| known <= limit)
}
fn preopen(d: &str) -> String {
    format!("{d}T09:25:00+08:00")
}
fn next_preopen(d: &str) -> String {
    let next =
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap() + chrono::Duration::days(1);
    preopen(&next.to_string())
}

// The strategy deliberately freezes the first available daily version. It does
// not retroactively replace a signal's history with a subsequently revised bar.
pub(super) fn bars(
    conn: &Connection,
    key: &InstrumentKey,
    instrument_id: i64,
    to: &str,
    p: &Value,
) -> Result<(Vec<Bar>, Value), AppError> {
    let mut rows = data::facts_on(conn, "bar_history", Some(key), to)?;
    let sources: BTreeSet<_> = rows.iter().map(|r| text(r, "source").to_string()).collect();
    let source = text(p, "archive_source");
    if source.is_empty() && sources.len() > 1 {
        return Err(invalid("时点日线有多个来源，请指定历史档案来源"));
    }
    if !source.is_empty() {
        rows.retain(|r| text(r, "source") == source);
    }
    rows.sort_by(|a, b| {
        let time =
            |r: &Value| chrono::DateTime::parse_from_rfc3339(text(&r["payload"], "known_at")).ok();
        time(a)
            .cmp(&time(b))
            .then(a["id"].as_i64().cmp(&b["id"].as_i64()))
    });
    let mut selected = BTreeMap::new();
    for r in &rows {
        let d = text(r, "day");
        if !before(r, &next_preopen(d)) {
            continue;
        }
        let v = &r["payload"];
        selected.entry(d.to_string()).or_insert_with(|| Bar {
            id: r["id"].as_i64().unwrap_or(0),
            instrument_id,
            timeframe: Timeframe::D1,
            ts: d.into(),
            open: v["open"].as_f64().unwrap(),
            high: v["high"].as_f64().unwrap(),
            low: v["low"].as_f64().unwrap(),
            close: v["close"].as_f64().unwrap(),
            adj_close: None,
            volume: v["volume"].as_f64(),
            turnover: v["turnover"].as_f64(),
            source: text(r, "source").into(),
            ingested_at: text(r, "observed_at").into(),
        });
    }
    let mut securities = data::facts_on(conn, "security_history", Some(key), to)?;
    let security_source = text(p, "security_source");
    if !security_source.is_empty() {
        securities.retain(|r| text(r, "source") == security_source);
    }
    Ok((
        selected.into_values().collect(),
        json!({"bar_archive":rows,"securities":securities}),
    ))
}

pub(super) fn validate(snapshot: &Value) -> Vec<Value> {
    let mut issues = vec![];
    let from = text(snapshot, "from");
    let to = text(snapshot, "to");
    if !text(snapshot, "market").starts_with("CN.") {
        issues.push(json!({"date":from,"issue":"严格时点模式当前仅支持沪深京；其他市场须使用明确标注的历史研究模式"}));
    }
    for kind in ["calendar", "constraints", "factors"] {
        if let Some(rows) = snapshot[kind].as_object() {
            for (d, r) in rows {
                if d.as_str() > to || (kind != "factors" && d.as_str() < from) {
                    continue;
                }
                let cutoff = if kind == "factors" {
                    next_preopen(d)
                } else {
                    preopen(d)
                };
                if !before(r, &cutoff) {
                    issues.push(json!({"date":d,"issue":format!("{kind}缺少决策前可得版本；请提供来源known_at，当前获取时间不能回填为历史时间")}));
                }
            }
        }
    }
    if let Some(actions) = snapshot["actions"].as_array() {
        for r in actions.iter().filter(|r| text(r, "day") >= from) {
            if !before(r, &preopen(text(r, "day"))) {
                issues.push(json!({"date":r["day"],"issue":"公司行为缺少除权日前可得版本"}));
            }
        }
    }
    let securities = snapshot["point_in_time"]["securities"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let sources: BTreeSet<_> = securities.iter().map(|r| text(r, "source")).collect();
    if sources.len() > 1 {
        issues.push(json!({"date":from,"issue":"证券历史存在多个来源，请指定上市状态来源"}));
    }
    if let Some(calendar) = snapshot["calendar"].as_object() {
        for (d, _) in calendar.iter().filter(|(d, c)| {
            d.as_str() >= from && d.as_str() <= to && flag(&c["payload"], "is_open")
        }) {
            let state = securities
                .iter()
                .filter(|r| text(r, "day") <= d.as_str() && before(r, &preopen(d)))
                .max_by_key(|r| {
                    (
                        text(r, "day"),
                        chrono::DateTime::parse_from_rfc3339(text(&r["payload"], "known_at")).ok(),
                    )
                });
            let valid = state.is_some_and(|r| {
                let s = &r["payload"];
                text(s, "status") == "listed"
                    && text(s, "list_date") <= d.as_str()
                    && (text(s, "delist_date").is_empty() || d.as_str() < text(s, "delist_date"))
            });
            if !valid {
                issues.push(json!({"date":d,"issue":"缺少当时可得的上市状态，或区间跨越退市；不能假设退市后可成交/自动清算"}));
            }
        }
    }
    issues
}
