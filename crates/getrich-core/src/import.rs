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
use crate::session;
use crate::symbol::parse_instrument;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ImportBundle {
    #[serde(default)]
    pub listings: Vec<ListingImport>,
    #[serde(default)]
    pub quotes: Vec<QuoteImport>,
    #[serde(default)]
    pub bars: Vec<BarImport>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

pub fn apply_bundle(
    db: &Database,
    bundle: &ImportBundle,
    source: &str,
) -> Result<IngestReport, AppError> {
    let conn = db.connect()?;
    apply_bundle_conn(&conn, bundle, source)
}

pub fn apply_bundle_conn(
    conn: &Connection,
    bundle: &ImportBundle,
    source: &str,
) -> Result<IngestReport, AppError> {
    let tx = conn.unchecked_transaction().map_err(db::DbError::from)?;
    let report = apply_bundle_in(&tx, bundle, source)?;
    let source_times: Vec<DateTime<Utc>> = bundle
        .quotes
        .iter()
        .filter_map(|quote| parse_ts(&quote.ts).ok())
        .collect();
    let batch = crate::realtime::CommittedBatch {
        source: source.into(),
        requested: bundle.quotes.len() as u32,
        accepted: report.quotes_upserted,
        changed: report.quotes_upserted,
        invalid: report.errors.len() as u32,
        min_source_at: source_times.iter().min().map(DateTime::to_rfc3339),
        max_source_at: source_times.iter().max().map(DateTime::to_rfc3339),
        committed_at: db::now_rfc3339(),
        ..crate::realtime::CommittedBatch::default()
    };
    db::insert_ingest_batch(&tx, &batch)?;
    tx.commit().map_err(db::DbError::from)?;
    Ok(report)
}

#[derive(Debug, Clone, Default)]
pub struct HotWriteReport {
    pub batch_version: u64,
    pub requested: u32,
    pub accepted: u32,
    pub changed: u32,
    pub unchanged: u32,
    pub stale: u32,
    pub invalid: u32,
    pub missing_listing: u32,
    pub quotes_upserted: u32,
    pub bars_upserted: u32,
    pub skipped_missing: u32,
    pub watermark: Option<String>,
}

/// Incremental quotes/bars for listings that already exist. No ingest_runs, no ensure_listing.
pub fn apply_quotes_hot(
    db: &Database,
    quotes: &[QuoteImport],
    bars: &[BarImport],
    source: &str,
    batch_size: u32,
) -> Result<HotWriteReport, AppError> {
    let conn = db.connect()?;
    apply_quotes_hot_conn(&conn, quotes, bars, source, batch_size)
}

pub fn apply_quotes_hot_conn(
    conn: &Connection,
    quotes: &[QuoteImport],
    bars: &[BarImport],
    source: &str,
    _batch_size: u32,
) -> Result<HotWriteReport, AppError> {
    let ids = db::list_listing_ids(conn)?;
    let mut report = HotWriteReport {
        requested: quotes.len().max(bars.len()) as u32,
        ..HotWriteReport::default()
    };
    let mut max_ts: Option<DateTime<Utc>> = None;

    let mut quote_snaps: Vec<(i64, QuoteSnapshot)> = Vec::new();
    for row in quotes {
        match quote_snapshot(row, source) {
            Ok(snap) => {
                let key = (
                    snap.key.symbol.clone(),
                    snap.key.market.as_str().to_string(),
                );
                if let Some(id) = ids.get(&key) {
                    if max_ts.map(|t| snap.ts > t).unwrap_or(true) {
                        max_ts = Some(snap.ts);
                    }
                    quote_snaps.push((*id, snap));
                } else {
                    report.skipped_missing += 1;
                    report.missing_listing += 1;
                }
            }
            Err(_) => {
                report.skipped_missing += 1;
                report.invalid += 1;
            }
        }
    }

    let mut bar_snaps: Vec<(i64, BarSnapshot)> = Vec::new();
    for row in bars {
        match bar_snapshot(row, source) {
            Ok(snap) => {
                let key = (
                    snap.key.symbol.clone(),
                    snap.key.market.as_str().to_string(),
                );
                if let Some(id) = ids.get(&key) {
                    bar_snaps.push((*id, snap));
                } else {
                    report.skipped_missing += 1;
                    report.missing_listing += 1;
                }
            }
            Err(_) => {
                report.skipped_missing += 1;
                report.invalid += 1;
            }
        }
    }
    use std::collections::HashMap;
    let mut grouped: HashMap<i64, Vec<BarSnapshot>> = HashMap::new();
    for (id, snap) in bar_snaps {
        grouped.entry(id).or_default().push(snap);
    }
    let tx = conn.unchecked_transaction().map_err(db::DbError::from)?;
    let mut changed_ids = Vec::new();
    for (id, snap) in &quote_snaps {
        match db::upsert_quote_outcome(&tx, *id, snap)? {
            db::QuoteWriteOutcome::Inserted | db::QuoteWriteOutcome::Changed => {
                report.accepted += 1;
                report.changed += 1;
                report.quotes_upserted += 1;
                changed_ids.push(*id);
            }
            db::QuoteWriteOutcome::Unchanged => {
                report.accepted += 1;
                report.unchanged += 1;
            }
            db::QuoteWriteOutcome::Stale => report.stale += 1,
        }
    }
    for (id, snaps) in grouped {
        report.bars_upserted += db::upsert_bars_on(&tx, id, &snaps)?;
    }
    let batch = crate::realtime::CommittedBatch {
        source: source.into(),
        requested: report.requested,
        accepted: report.accepted,
        changed: report.changed,
        unchanged: report.unchanged,
        stale: report.stale,
        invalid: report.invalid,
        missing_listing: report.missing_listing,
        min_source_at: quote_snaps
            .iter()
            .map(|(_, q)| q.ts)
            .min()
            .map(|ts| ts.to_rfc3339()),
        max_source_at: quote_snaps
            .iter()
            .map(|(_, q)| q.ts)
            .max()
            .map(|ts| ts.to_rfc3339()),
        changed_ids,
        committed_at: db::now_rfc3339(),
        ..crate::realtime::CommittedBatch::default()
    };
    report.batch_version = db::insert_ingest_batch(&tx, &batch)?;
    tx.commit().map_err(db::DbError::from)?;

    if let Some(ts) = max_ts {
        report.watermark = Some(ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }
    Ok(report)
}

impl HotWriteReport {
    pub fn as_ingest(&self, source: &str) -> IngestReport {
        IngestReport {
            run_id: 0,
            kind: "quotes_hot".into(),
            source: source.into(),
            status: if self.skipped_missing == 0 {
                "ok".into()
            } else {
                "partial".into()
            },
            instruments_upserted: 0,
            bars_upserted: self.bars_upserted,
            quotes_upserted: self.quotes_upserted,
            errors: if self.skipped_missing == 0 {
                Vec::new()
            } else {
                vec![format!("skipped_missing={}", self.skipped_missing)]
            },
            counts: crate::models::ImportCounts {
                requested: self.requested,
                updated: self.changed,
                unchanged: self.unchanged,
                stale: self.stale,
                invalid: self.invalid,
                ..Default::default()
            },
        }
    }
}

fn apply_bundle_in(
    conn: &Connection,
    bundle: &ImportBundle,
    source: &str,
) -> Result<IngestReport, AppError> {
    let kind = if !bundle.listings.is_empty() && bundle.quotes.is_empty() && bundle.bars.is_empty()
    {
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
    let mut counts = crate::models::ImportCounts {
        requested: (bundle.listings.len() + bundle.quotes.len() + bundle.bars.len()) as u32,
        ..Default::default()
    };

    let reject = |dataset: &str,
                  index: usize,
                  symbol: &str,
                  reason: String,
                  errors: &mut Vec<String>|
     -> Result<(), AppError> {
        let message = format!("{dataset} 第 {} 行 {symbol}：{reason}", index + 1);
        conn.execute("INSERT INTO ingest_rejections(run_id,dataset,row_number,symbol,reason,created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![run_id,dataset,index+1,symbol,reason,db::now_rfc3339()]).map_err(db::DbError::from)?;
        errors.push(message);
        Ok(())
    };
    for (index, row) in bundle.listings.iter().enumerate() {
        match listing_snapshot(row, source) {
            Ok(snap) => {
                let before = db::get_listing(conn, &snap.key)?
                    .map(|listing| listing_content(conn, &listing))
                    .transpose()?;
                let id = db::upsert_listing(conn, &snap)?;
                let after = db::get_listing_by_id(conn, id)?
                    .ok_or_else(|| AppError::Message("写入标的后未找到记录".into()))?;
                let after = listing_content(conn, &after)?;
                if let Some(before) = before {
                    if before == after {
                        counts.unchanged += 1;
                    } else {
                        instruments += 1;
                        counts.updated += 1;
                    }
                } else {
                    instruments += 1;
                    counts.inserted += 1;
                }
            }
            Err(e) => reject("listings", index, &row.symbol, e, &mut errors)?,
        }
    }
    for (index, row) in bundle.quotes.iter().enumerate() {
        match quote_snapshot(row, source) {
            Ok(snap) => {
                let id = ensure_listing(conn, &snap.key, source)?;
                match db::upsert_quote_outcome(conn, id, &snap)? {
                    db::QuoteWriteOutcome::Inserted => {
                        quotes += 1;
                        counts.inserted += 1;
                    }
                    db::QuoteWriteOutcome::Changed => {
                        quotes += 1;
                        counts.updated += 1;
                    }
                    db::QuoteWriteOutcome::Unchanged => counts.unchanged += 1,
                    db::QuoteWriteOutcome::Stale => counts.stale += 1,
                }
            }
            Err(e) => reject("quotes", index, &row.symbol, e, &mut errors)?,
        }
    }
    for (index, row) in bundle.bars.iter().enumerate() {
        match bar_snapshot(row, source) {
            Ok(snap) => {
                let id = ensure_listing(conn, &snap.key, source)?;
                let ts = db::bar_ts_key(snap.ts, !snap.timeframe.is_intraday());
                let existed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM bars WHERE instrument_id=?1 AND timeframe=?2 AND ts=?3)",
                    rusqlite::params![id,snap.timeframe.as_str(),ts],|r|r.get(0)).map_err(db::DbError::from)?;
                let n = db::upsert_bars_on(conn, id, &[snap])?;
                bars += n;
                if n == 0 {
                    counts.unchanged += 1;
                } else if existed {
                    counts.updated += 1;
                } else {
                    counts.inserted += 1;
                }
            }
            Err(e) => reject("bars", index, &row.symbol, e, &mut errors)?,
        }
    }
    counts.invalid = errors.len() as u32;
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
        counts,
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
    if let Some(value) = row.listing_date.as_deref() {
        snap.listing_date = Some(crate::validation::date(value)?);
    }
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
    validate_market_timestamp(&row.ts, &key)?;
    let snap = QuoteSnapshot {
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
    };
    crate::validation::quote(&snap)?;
    Ok(snap)
}

fn bar_snapshot(row: &BarImport, default_source: &str) -> Result<BarSnapshot, String> {
    let key = parse_instrument(&row.symbol, row.market.as_deref())?;
    validate_market_timestamp(&row.ts, &key)?;
    let timeframe = row
        .timeframe
        .as_deref()
        .unwrap_or("1d")
        .parse::<Timeframe>()?;
    let snap = BarSnapshot {
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
    };
    if !snap.is_valid() {
        return Err(format!("{}: K 线 OHLC/量额无效", snap.key.display()));
    }
    Ok(snap)
}

fn listing_content(
    conn: &Connection,
    listing: &crate::models::Instrument,
) -> Result<serde_json::Value, AppError> {
    let mut content =
        serde_json::to_value(listing).map_err(|e| AppError::Message(e.to_string()))?;
    if let Some(object) = content.as_object_mut() {
        object.remove("created_at");
        object.remove("updated_at");
    }
    content["metadata"] = serde_json::to_value(db::list_metadata(conn, listing.id)?)
        .map_err(|e| AppError::Message(e.to_string()))?;
    Ok(content)
}

pub fn preview(
    db: &Database,
    bundle: &ImportBundle,
    source: &str,
) -> Result<serde_json::Value, AppError> {
    use std::hash::{Hash, Hasher};
    let mut conn = db.connect()?;
    let transaction = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db::DbError::from)?;
    let report = apply_bundle_in(&transaction, bundle, source)?;
    let errors = {
        let mut stmt=transaction.prepare("SELECT dataset,row_number,symbol,reason FROM ingest_rejections WHERE run_id=?1 ORDER BY id").map_err(db::DbError::from)?;
        let rows=stmt.query_map([report.run_id],|r|Ok(serde_json::json!({"dataset":r.get::<_,String>(0)?,"row":r.get::<_,u32>(1)?,"symbol":r.get::<_,Option<String>>(2)?,"message":r.get::<_,String>(3)?}))).map_err(db::DbError::from)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(db::DbError::from)?
    };
    transaction.rollback().map_err(db::DbError::from)?;
    let text = serde_json::to_string(bundle).map_err(|e| AppError::Message(e.to_string()))?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hash);
    Ok(
        serde_json::json!({"can_apply":report.counts.requested>report.counts.invalid,"counts":report.counts,"errors":errors,"fingerprint":format!("{:016x}",hash.finish()),"preview":true}),
    )
}

fn validate_market_timestamp(raw: &str, key: &InstrumentKey) -> Result<(), String> {
    if key.market == crate::models::Market::Us
        && raw.trim().len() > 10
        && DateTime::parse_from_rfc3339(raw.trim()).is_err()
    {
        return Err("美股盘中时间必须使用带时区偏移的 RFC3339，例如 2026-10-23T16:00:00-04:00；日线可使用 YYYY-MM-DD".into());
    }
    Ok(())
}

pub fn parse_ts(raw: &str) -> Result<DateTime<Utc>, String> {
    session::parse_cn_ts(raw)
}

fn csv_listings(text: &str) -> Result<Vec<ListingImport>, AppError> {
    let (header, rows) = csv_rows(text)?;
    let idx = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let mut out = Vec::new();
    for cols in rows {
        let get = |name: &str| {
            idx(name)
                .and_then(|i| cols.get(i))
                .and_then(|value| nonempty(value))
        };
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
        let get = |name: &str| {
            idx(name)
                .and_then(|i| cols.get(i))
                .and_then(|value| nonempty(value))
        };
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
        let get = |name: &str| {
            idx(name)
                .and_then(|i| cols.get(i))
                .and_then(|value| nonempty(value))
        };
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

fn nonempty(s: &str) -> Option<String> {
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
        assert_eq!(r2.instruments_upserted, 0);
        assert_eq!(r2.quotes_upserted, 0);
        assert_eq!(r2.bars_upserted, 0);
        assert_eq!(r2.counts.unchanged, 3);
        assert_eq!(db::count_bars(&conn, 1, Timeframe::D1).unwrap(), 1);
    }

    #[test]
    fn naive_datetime_is_shanghai() {
        let ts = parse_ts("2026-09-11 10:00:00").unwrap();
        assert_eq!(
            ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-09-11T02:00:00Z"
        );
    }

    #[test]
    fn hot_write_skips_missing_and_skips_ingest_runs() {
        let json = r#"{
          "listings":[{"symbol":"600519","market":"CN.SH","name":"贵州茅台"}],
          "quotes":[{"symbol":"600519","market":"CN.SH","ts":"2024-01-02","last":10}]
        }"#;
        let bundle = parse_json(json, ImportKind::Bundle).unwrap();
        let conn = open_memory().unwrap();
        apply_bundle_conn(&conn, &bundle, "test").unwrap();
        let runs_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM ingest_runs", [], |r| r.get(0))
            .unwrap();

        let quotes = vec![
            QuoteImport {
                symbol: "600519".into(),
                market: Some("CN.SH".into()),
                ts: "2026-09-11 10:00:00".into(),
                last: 12.0,
                open: Some(11.0),
                high: Some(12.5),
                low: Some(11.0),
                prev_close: Some(10.0),
                volume: Some(55_845_293.0),
                turnover: Some(1.0),
                change_pct: Some(20.0),
                currency: Some("CNY".into()),
                source: Some("tushare".into()),
            },
            QuoteImport {
                symbol: "999999".into(),
                market: Some("CN.SH".into()),
                ts: "2026-09-11 10:00:00".into(),
                last: 1.0,
                open: None,
                high: None,
                low: None,
                prev_close: None,
                volume: None,
                turnover: None,
                change_pct: None,
                currency: None,
                source: None,
            },
        ];
        let bars = vec![BarImport {
            symbol: "600519".into(),
            market: Some("CN.SH".into()),
            timeframe: Some("1d".into()),
            ts: "2026-09-11".into(),
            open: 11.0,
            high: 12.5,
            low: 11.0,
            close: 12.0,
            adj_close: None,
            volume: Some(55_845_293.0),
            turnover: Some(1.0),
            source: Some("tushare".into()),
        }];
        let hot = apply_quotes_hot_conn(&conn, &quotes, &bars, "tushare", 500).unwrap();
        assert_eq!(hot.quotes_upserted, 1);
        assert_eq!(hot.bars_upserted, 1);
        assert_eq!(hot.skipped_missing, 1);
        assert_eq!(hot.accepted, 1);
        assert_eq!(hot.changed, 1);
        assert_eq!(hot.missing_listing, 1);
        assert!(hot.batch_version > 0);
        let runs_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM ingest_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs_after, runs_before);
        let vol: f64 = conn
            .query_row(
                "SELECT volume FROM quotes q JOIN listings l ON l.id = q.instrument_id WHERE l.symbol='600519'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(vol, 55_845_293.0);
        let again = apply_quotes_hot_conn(&conn, &quotes[..1], &bars, "tushare", 500).unwrap();
        assert_eq!(again.quotes_upserted, 0);
        assert_eq!(again.unchanged, 1);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM quotes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn csv_listings_parse() {
        let csv = "symbol,market,name\n600519,CN.SH,贵州茅台\n";
        let bundle = parse_csv(csv, ImportKind::Listings).unwrap();
        assert_eq!(bundle.listings.len(), 1);
        assert_eq!(bundle.listings[0].name.as_deref(), Some("贵州茅台"));
    }
}
