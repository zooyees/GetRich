use super::{now_rfc3339, DbError};
use crate::models::{Quote, QuoteSnapshot};
use crate::session;
use chrono::{DateTime, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteWriteOutcome {
    Inserted,
    Changed,
    Unchanged,
    Stale,
}

pub fn upsert_quote(
    conn: &Connection,
    instrument_id: i64,
    quote: &QuoteSnapshot,
) -> Result<(), DbError> {
    upsert_quote_outcome(conn, instrument_id, quote).map(|_| ())
}

pub fn upsert_quote_outcome(
    conn: &Connection,
    instrument_id: i64,
    quote: &QuoteSnapshot,
) -> Result<QuoteWriteOutcome, DbError> {
    crate::validation::quote(quote).map_err(DbError::Message)?;
    let mut incoming = quote.clone();
    let existing = get_stored_quote(conn, instrument_id)?;
    if let Some(old) = &existing {
        if let Ok(old_ts) = session::parse_cn_ts(&old.ts) {
            let same_day =
                session::shanghai_date_at(old_ts) == session::shanghai_date_at(incoming.ts);
            let new_eod = is_eod_quote(&incoming.source, incoming.ts);
            let old_live = is_live_quote(&old.source, old_ts);
            if same_day && new_eod && old_live {
                return Ok(QuoteWriteOutcome::Stale);
            }
            let same_day_promotion = same_day
                && is_live_quote(&incoming.source, incoming.ts)
                && is_eod_quote(&old.source, old_ts);
            if incoming.ts < old_ts && !same_day_promotion {
                return Ok(QuoteWriteOutcome::Stale);
            }
            if same_day {
                incoming.prev_close = incoming.prev_close.or(old.prev_close);
            }
        }
        incoming.currency = incoming.currency.or_else(|| old.currency.clone());
    }
    // Missing OHLC/flow fields remain unknown. Carrying old bounds onto a new
    // last-only quote can create an impossible candle, even on the same day.
    if let Some(previous) = incoming.prev_close {
        incoming.change_pct = Some((incoming.last - previous) * 100.0 / previous);
    }
    crate::validation::quote(&incoming).map_err(DbError::Message)?;
    if existing
        .as_ref()
        .is_some_and(|old| quote_equals(old, &incoming))
    {
        return Ok(QuoteWriteOutcome::Unchanged);
    }
    let outcome = if existing.is_some() {
        QuoteWriteOutcome::Changed
    } else {
        QuoteWriteOutcome::Inserted
    };
    conn.execute(r#"
        INSERT INTO quotes(instrument_id,ts,last,open,high,low,prev_close,volume,turnover,change_pct,currency,source,ingested_at)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
        ON CONFLICT(instrument_id) DO UPDATE SET ts=excluded.ts,last=excluded.last,open=excluded.open,high=excluded.high,
          low=excluded.low,prev_close=excluded.prev_close,volume=excluded.volume,turnover=excluded.turnover,
          change_pct=excluded.change_pct,currency=excluded.currency,source=excluded.source,ingested_at=excluded.ingested_at
    "#,params![instrument_id,incoming.ts.to_rfc3339_opts(chrono::SecondsFormat::Secs,true),incoming.last,incoming.open,incoming.high,incoming.low,incoming.prev_close,incoming.volume,incoming.turnover,incoming.change_pct,incoming.currency,incoming.source,now_rfc3339()])?;
    Ok(outcome)
}

fn quote_equals(existing: &Quote, quote: &QuoteSnapshot) -> bool {
    session::parse_cn_ts(&existing.ts).ok() == Some(quote.ts)
        && existing.last == quote.last
        && existing.open == quote.open
        && existing.high == quote.high
        && existing.low == quote.low
        && existing.prev_close == quote.prev_close
        && existing.volume == quote.volume
        && existing.turnover == quote.turnover
        && existing.change_pct == quote.change_pct
        && existing.currency == quote.currency
        && existing.source == quote.source
}

fn is_date_only(ts: DateTime<Utc>) -> bool {
    ts.time().num_seconds_from_midnight() == 0 && ts.nanosecond() == 0
}

fn is_eod_quote(source: &str, ts: DateTime<Utc>) -> bool {
    source.contains("daily") || (is_date_only(ts) && !source.contains("rt_k"))
}

fn is_live_quote(source: &str, ts: DateTime<Utc>) -> bool {
    source.contains("rt_k") || !is_date_only(ts)
}

pub fn get_quote(conn: &Connection, instrument_id: i64) -> Result<Option<Quote>, DbError> {
    read_quote(conn, instrument_id, "canonical_quotes")
}

fn get_stored_quote(conn: &Connection, instrument_id: i64) -> Result<Option<Quote>, DbError> {
    read_quote(conn, instrument_id, "quotes")
}

fn read_quote(
    conn: &Connection,
    instrument_id: i64,
    table: &str,
) -> Result<Option<Quote>, DbError> {
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT instrument_id, ts, last, open, high, low, prev_close, volume,
               turnover, change_pct, currency, source, ingested_at
        FROM {table} WHERE instrument_id = ?1
        "#
    ))?;
    let row = stmt
        .query_row(params![instrument_id], map_quote)
        .optional()?;
    Ok(row)
}

pub(crate) fn map_quote(r: &rusqlite::Row<'_>) -> rusqlite::Result<Quote> {
    Ok(Quote {
        instrument_id: r.get(0)?,
        ts: r.get(1)?,
        last: r.get(2)?,
        open: r.get(3)?,
        high: r.get(4)?,
        low: r.get(5)?,
        prev_close: r.get(6)?,
        volume: r.get(7)?,
        turnover: r.get(8)?,
        change_pct: r.get(9)?,
        currency: r.get(10)?,
        source: r.get(11)?,
        ingested_at: r.get(12)?,
    })
}
