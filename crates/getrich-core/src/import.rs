//! File import into the foundation store (JSON bundle / JSON arrays / CSV).
//!
//! Live network acquisition is out of scope. This module is the contract for
//! later database loads: upsert listings, quotes, and bars idempotently.

use crate::db::{self, Database};
use crate::error::AppError;
use crate::models::{
    BarSnapshot, IngestReport, InstrumentKey, ListingSnapshot, MetadataEntry, QuoteSnapshot,
    Timeframe,
};
use crate::symbol::parse_instrument;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rusqlite::Connection;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImportBundle {
    #[serde(default)]
    pub listings: Vec<ListingImport>,
    #[serde(default)]
    pub quotes: Vec<QuoteImport>,
    #[serde(default)]
    pub bars: Vec<BarImport>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListingImport {
    pub symbol: String,
    #[serde(default)]
    pub market: Option<String>,
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub instrument_type: Option<String>,
    #[serde(default)]
    pub isin: Option<String>,
    #[serde(default)]
    pub listing_date: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub sector: Option<String>,
    #[serde(default)]
    pub industry: Option<String>,
    #[serde(default)]
    pub lot_size: Option<i64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_symbol: Option<String>,
    #[serde(default)]
    pub metadata: Vec<MetadataEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuoteImport {
    pub symbol: String,
    #[serde(default)]
    pub market: Option<String>,
    pub ts: String,
    pub last: f64,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    #[serde(default)]
    pub prev_close: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub turnover: Option<f64>,
    #[serde(default)]
    pub change_pct: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BarImport {
    pub symbol: String,
    #[serde(default)]
    pub market: Option<String>,
    #[serde(default)]
    pub timeframe: Option<String>,
    pub ts: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    #[serde(default)]
    pub adj_close: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub turnover: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Bundle,
    Listings,
    Quotes,
    Bars,
}

impl ImportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bundle => "bundle",
            Self::Listings => "listings",
            Self::Quotes => "quotes",
            Self::Bars => "bars",
        }
    }
}

pub fn load_file(path: &Path, kind: ImportKind) -> Result<ImportBundle, AppError> {
    let text = fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "csv" {
        parse_csv(&text, kind)
    } else {
        parse_json(&text, kind)
    }
}

pub fn parse_json(text: &str, kind: ImportKind) -> Result<ImportBundle, AppError> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| AppError::InvalidArg(format!("invalid JSON: {e}")))?;
    match kind {
        ImportKind::Bundle => serde_json::from_value(value)
            .map_err(|e| AppError::InvalidArg(format!("invalid bundle: {e}"))),
        ImportKind::Listings => {
            let listings: Vec<ListingImport> = if value.is_array() {
                serde_json::from_value(value)
            } else {
                serde_json::from_value(value.get("listings").cloned().unwrap_or(value))
            }
            .map_err(|e| AppError::InvalidArg(format!("invalid listings: {e}")))?;
            Ok(ImportBundle {
                listings,
                ..Default::default()
            })
        }
        ImportKind::Quotes => {
            let quotes: Vec<QuoteImport> = if value.is_array() {
                serde_json::from_value(value)
            } else {
                serde_json::from_value(value.get("quotes").cloned().unwrap_or(value))
            }
            .map_err(|e| AppError::InvalidArg(format!("invalid quotes: {e}")))?;
            Ok(ImportBundle {
                quotes,
                ..Default::default()
            })
        }
        ImportKind::Bars => {
            let bars: Vec<BarImport> = if value.is_array() {
                serde_json::from_value(value)
            } else {
                serde_json::from_value(value.get("bars").cloned().unwrap_or(value))
            }
            .map_err(|e| AppError::InvalidArg(format!("invalid bars: {e}")))?;
            Ok(ImportBundle {
                bars,
                ..Default::default()
            })
        }
    }
}

pub fn parse_csv(text: &str, kind: ImportKind) -> Result<ImportBundle, AppError> {
    match kind {
        ImportKind::Listings => Ok(ImportBundle {
            listings: csv_listings(text)?,
            ..Default::default()
        }),
        ImportKind::Quotes => Ok(ImportBundle {
            quotes: csv_quotes(text)?,
            ..Default::default()
        }),
        ImportKind::Bars => Ok(ImportBundle {
            bars: csv_bars(text)?,
            ..Default::default()
        }),
        ImportKind::Bundle => Err(AppError::InvalidArg(
            "CSV import requires listings, quotes, or bars (not bundle)".into(),
        )),
    }
}

pub fn apply_bundle(db: &Database, bundle: &ImportBundle, source: &str) -> Result<IngestReport, AppError> {
    let conn = db.connect()?;
    apply_bundle_conn(&conn, bundle, source)
}

pub fn apply_bundle_conn(
    conn: &Connection,
    bundle: &ImportBundle,
    source: &str,
) -> Result<IngestReport, AppError> {
    let kind = if !bundle.listings.is_empty() && bundle.quotes.is_empty() && bundle.bars.is_empty() {
        "listings"
    } else if bundle.listings.is_empty() && !bundle.quotes.is_empty() && bundle.bars.is_empty() {
        "quotes"
    } else if bundle.listings.is_empty() && bundle.quotes.is_empty() && !bundle.bars.is_empty() {
        "bars"
    } else {
        "bundle"
    };
    let run_id = db::start_run(conn, kind, source)?;
    let mut errors = Vec::new();
    let mut instruments = 0u32;
    let mut quotes = 0u32;
    let mut bars = 0u32;

    for row in &bundle.listings {
        match listing_snapshot(row, source) {
            Ok(snap) => match db::upsert_listing(conn, &snap) {
                Ok(_) => instruments += 1,
                Err(e) => errors.push(format!("{}: {e}", snap.key.display())),
            },
            Err(e) => errors.push(e),
        }
    }

    for row in &bundle.quotes {
        match quote_snapshot(row, source) {
            Ok(snap) => match ensure_listing(conn, &snap.key, source) {
                Ok(id) => match db::upsert_quote(conn, id, &snap) {
                    Ok(()) => quotes += 1,
                    Err(e) => errors.push(format!("{}: {e}", snap.key.display())),
                },
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(e),
        }
    }

    let mut bar_snaps: Vec<(i64, BarSnapshot)> = Vec::new();
    for row in &bundle.bars {
        match bar_snapshot(row, source) {
            Ok(snap) => match ensure_listing(conn, &snap.key, source) {
                Ok(id) => bar_snaps.push((id, snap)),
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(e),
        }
    }
    use std::collections::HashMap;
    let mut grouped: HashMap<i64, Vec<BarSnapshot>> = HashMap::new();
    for (id, snap) in bar_snaps {
        grouped.entry(id).or_default().push(snap);
    }
    for (id, snaps) in grouped {
        match db::upsert_bars(conn, id, &snaps) {
            Ok(n) => bars += n,
            Err(e) => errors.push(e.to_string()),
        }
    }

    let status = if errors.is_empty() { "ok" } else { "partial" };
    let detail = serde_json::json!({ "errors": errors }).to_string();
    db::finish_run(
        conn,
        run_id,
        status,
        instruments,
        bars,
        quotes,
        errors.first().map(String::as_str),
        Some(&detail),
    )?;

    Ok(IngestReport {
        run_id,
        kind: kind.into(),
        source: source.into(),
        status: status.into(),
        instruments_upserted: instruments,
        bars_upserted: bars,
        quotes_upserted: quotes,
        errors,
    })
}

fn ensure_listing(conn: &Connection, key: &InstrumentKey, source: &str) -> Result<i64, AppError> {
    if let Some(existing) = db::get_listing(conn, key)? {
        return Ok(existing.id);
    }
    let snap = ListingSnapshot::equity(key.clone(), source, key.symbol.clone());
    Ok(db::upsert_listing(conn, &snap)?)
}

fn listing_snapshot(row: &ListingImport, default_source: &str) -> Result<ListingSnapshot, String> {
    let key = parse_instrument(&row.symbol, row.market.as_deref())?;
    let mut snap = ListingSnapshot::equity(
        key.clone(),
        row.source.clone().unwrap_or_else(|| default_source.into()),
        row.source_symbol.clone().unwrap_or(key.symbol.clone()),
    );
    snap.exchange = row.exchange.clone().or(snap.exchange);
    snap.name = row.name.clone();
    snap.name_en = row.name_en.clone();
    snap.currency = row.currency.clone().or(snap.currency);
    if let Some(t) = &row.instrument_type {
        snap.instrument_type = t.clone();
    }
    snap.isin = row.isin.clone();
    snap.listing_date = row.listing_date.clone();
    if let Some(s) = &row.status {
        snap.status = s.clone();
    }
    snap.sector = row.sector.clone();
    snap.industry = row.industry.clone();
    snap.lot_size = row.lot_size;
    snap.metadata = row.metadata.clone();
    Ok(snap)
}

fn quote_snapshot(row: &QuoteImport, default_source: &str) -> Result<QuoteSnapshot, String> {
    let key = parse_instrument(&row.symbol, row.market.as_deref())?;
    Ok(QuoteSnapshot {
        key,
        ts: parse_ts(&row.ts)?,
        last: row.last,
        open: row.open,
        high: row.high,
        low: row.low,
        prev_close: row.prev_close,
        volume: row.volume,
        turnover: row.turnover,
        change_pct: row.change_pct,
        currency: row.currency.clone(),
        source: row.source.clone().unwrap_or_else(|| default_source.into()),
    })
}

fn bar_snapshot(row: &BarImport, default_source: &str) -> Result<BarSnapshot, String> {
    let key = parse_instrument(&row.symbol, row.market.as_deref())?;
    let timeframe = row
        .timeframe
        .as_deref()
        .unwrap_or("1d")
        .parse::<Timeframe>()?;
    Ok(BarSnapshot {
        key,
        timeframe,
        ts: parse_ts(&row.ts)?,
        open: row.open,
        high: row.high,
        low: row.low,
        close: row.close,
        adj_close: row.adj_close,
        volume: row.volume,
        turnover: row.turnover,
        source: row.source.clone().unwrap_or_else(|| default_source.into()),
    })
}

pub fn parse_ts(raw: &str) -> Result<DateTime<Utc>, String> {
    let s = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ndt = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("invalid date: {s}"))?;
        return Ok(Utc.from_utc_datetime(&ndt));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&dt));
    }
    Err(format!("invalid timestamp (use RFC3339 or YYYY-MM-DD): {s}"))
}

fn csv_listings(text: &str) -> Result<Vec<ListingImport>, AppError> {
    let (header, rows) = csv_rows(text)?;
    let idx = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let mut out = Vec::new();
    for cols in rows {
        let get = |name: &str| idx(name).and_then(|i| cols.get(i)).and_then(nonempty);
        let Some(symbol) = get("symbol") else {
            continue;
        };
        out.push(ListingImport {
            symbol,
            market: get("market"),
            exchange: get("exchange"),
            name: get("name"),
            name_en: get("name_en"),
            currency: get("currency"),
            instrument_type: get("instrument_type"),
            isin: get("isin"),
            listing_date: get("listing_date"),
            status: get("status"),
            sector: get("sector"),
            industry: get("industry"),
            lot_size: get("lot_size").and_then(|s| s.parse().ok()),
            source: get("source"),
            source_symbol: get("source_symbol"),
            metadata: Vec::new(),
        });
    }
    Ok(out)
}

fn csv_quotes(text: &str) -> Result<Vec<QuoteImport>, AppError> {
    let (header, rows) = csv_rows(text)?;
    let idx = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let mut out = Vec::new();
    for cols in rows {
        let get = |name: &str| idx(name).and_then(|i| cols.get(i)).and_then(nonempty);
        let num = |name: &str| get(name).and_then(|s| s.parse().ok());
        let Some(symbol) = get("symbol") else {
            continue;
        };
        let Some(ts) = get("ts").or_else(|| get("date")) else {
            continue;
        };
        let Some(last) = num("last").or_else(|| num("close")) else {
            continue;
        };
        out.push(QuoteImport {
            symbol,
            market: get("market"),
            ts,
            last,
            open: num("open"),
            high: num("high"),
            low: num("low"),
            prev_close: num("prev_close").or_else(|| num("pre_close")),
            volume: num("volume"),
            turnover: num("turnover").or_else(|| num("amount")),
            change_pct: num("change_pct"),
            currency: get("currency"),
            source: get("source"),
        });
    }
    Ok(out)
}

fn csv_bars(text: &str) -> Result<Vec<BarImport>, AppError> {
    let (header, rows) = csv_rows(text)?;
    let idx = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let mut out = Vec::new();
    for cols in rows {
        let get = |name: &str| idx(name).and_then(|i| cols.get(i)).and_then(nonempty);
        let num = |name: &str| get(name).and_then(|s| s.parse().ok());
        let Some(symbol) = get("symbol") else {
            continue;
        };
        let Some(ts) = get("ts").or_else(|| get("date")) else {
            continue;
        };
        let (Some(open), Some(high), Some(low), Some(close)) =
            (num("open"), num("high"), num("low"), num("close"))
        else {
            continue;
        };
        out.push(BarImport {
            symbol,
            market: get("market"),
            timeframe: get("timeframe"),
            ts,
            open,
            high,
            low,
            close,
            adj_close: num("adj_close"),
            volume: num("volume"),
            turnover: num("turnover").or_else(|| num("amount")),
            source: get("source"),
        });
    }
    Ok(out)
}

fn csv_rows(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), AppError> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines
        .next()
        .ok_or_else(|| AppError::InvalidArg("CSV is empty".into()))?;
    let header = split_csv_line(header_line);
    if header.iter().all(|h| h.is_empty()) {
        return Err(AppError::InvalidArg("CSV header is empty".into()));
    }
    let rows = lines.map(split_csv_line).collect();
    Ok((header, rows))
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    cur.push('"');
                    i += 1;
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
        i += 1;
    }
    out.push(cur.trim().to_string());
    out
}

fn nonempty(s: &String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    #[test]
    fn json_bundle_roundtrip_idempotent() {
        let json = r#"{
          "listings": [
            {"symbol":"AAPL","market":"US","name":"Apple Inc.","currency":"USD"}
          ],
          "quotes": [
            {"symbol":"AAPL","market":"US","ts":"2024-01-02T21:00:00Z","last":110,"open":100,"high":112,"low":99,"prev_close":100,"volume":10,"turnover":1100}
          ],
          "bars": [
            {"symbol":"AAPL","market":"US","timeframe":"1d","ts":"2024-01-02","open":100,"high":112,"low":99,"close":110,"volume":10}
          ]
        }"#;
        let bundle = parse_json(json, ImportKind::Bundle).unwrap();
        let conn = open_memory().unwrap();
        let r1 = apply_bundle_conn(&conn, &bundle, "test").unwrap();
        let r2 = apply_bundle_conn(&conn, &bundle, "test").unwrap();
        assert_eq!(r1.instruments_upserted, 1);
        assert_eq!(r1.quotes_upserted, 1);
        assert_eq!(r1.bars_upserted, 1);
        assert_eq!(r2.instruments_upserted, 1);
        assert_eq!(r2.quotes_upserted, 1);
        assert_eq!(r2.bars_upserted, 1);
        assert_eq!(db::count_bars(&conn, 1, Timeframe::D1).unwrap(), 1);
    }

    #[test]
    fn csv_listings_parse() {
        let csv = "symbol,market,name\n600519,CN.SH,贵州茅台\n";
        let bundle = parse_csv(csv, ImportKind::Listings).unwrap();
        assert_eq!(bundle.listings.len(), 1);
        assert_eq!(bundle.listings[0].name.as_deref(), Some("贵州茅台"));
    }
}
