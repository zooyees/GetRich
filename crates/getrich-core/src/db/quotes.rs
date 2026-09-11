use super::{now_rfc3339, DbError};
use crate::models::{Quote, QuoteSnapshot};
use rusqlite::{params, Connection, OptionalExtension};

pub fn upsert_quote(
    conn: &Connection,
    instrument_id: i64,
    quote: &QuoteSnapshot,
) -> Result<(), DbError> {
    let ts = quote.ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    conn.execute(
        r#"
        INSERT INTO quotes (
            instrument_id, ts, last, open, high, low, prev_close, volume,
            turnover, change_pct, currency, source, ingested_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
        ON CONFLICT(instrument_id) DO UPDATE SET
            ts = excluded.ts,
            last = excluded.last,
            open = COALESCE(excluded.open, quotes.open),
            high = COALESCE(excluded.high, quotes.high),
            low = COALESCE(excluded.low, quotes.low),
            prev_close = COALESCE(excluded.prev_close, quotes.prev_close),
            volume = COALESCE(excluded.volume, quotes.volume),
            turnover = COALESCE(excluded.turnover, quotes.turnover),
            change_pct = COALESCE(excluded.change_pct, quotes.change_pct),
            currency = COALESCE(excluded.currency, quotes.currency),
            source = excluded.source,
            ingested_at = excluded.ingested_at
        "#,
        params![
            instrument_id,
            ts,
            quote.last,
            quote.open,
            quote.high,
            quote.low,
            quote.prev_close,
            quote.volume,
            quote.turnover,
            quote.change_pct,
            quote.currency,
            quote.source,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn get_quote(conn: &Connection, instrument_id: i64) -> Result<Option<Quote>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT instrument_id, ts, last, open, high, low, prev_close, volume,
               turnover, change_pct, currency, source, ingested_at
        FROM quotes WHERE instrument_id = ?1
        "#,
    )?;
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
