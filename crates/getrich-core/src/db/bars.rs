use super::{bar_ts_key, now_rfc3339, DbError};
use crate::models::{Bar, BarSnapshot, Timeframe};
use rusqlite::{params, Connection};
use std::str::FromStr;

pub fn upsert_bars(
    conn: &Connection,
    instrument_id: i64,
    bars: &[BarSnapshot],
) -> Result<u32, DbError> {
    if bars.is_empty() {
        return Ok(0);
    }
    let ingested_at = now_rfc3339();
    let tx = conn.unchecked_transaction()?;
    let mut written = 0u32;
    {
        let mut stmt = tx.prepare(
            r#"
            INSERT INTO bars (
                instrument_id, timeframe, ts, open, high, low, close, adj_close,
                volume, turnover, source, ingested_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
            ON CONFLICT(instrument_id, timeframe, ts) DO UPDATE SET
                open = excluded.open,
                high = excluded.high,
                low = excluded.low,
                close = excluded.close,
                adj_close = COALESCE(excluded.adj_close, bars.adj_close),
                volume = COALESCE(excluded.volume, bars.volume),
                turnover = COALESCE(excluded.turnover, bars.turnover),
                source = excluded.source,
                ingested_at = excluded.ingested_at
            "#,
        )?;
        for bar in bars {
            if !bar.is_valid() {
                continue;
            }
            let ts = bar_ts_key(bar.ts, !bar.timeframe.is_intraday());
            stmt.execute(params![
                instrument_id,
                bar.timeframe.as_str(),
                ts,
                bar.open,
                bar.high,
                bar.low,
                bar.close,
                bar.adj_close,
                bar.volume,
                bar.turnover,
                bar.source,
                ingested_at,
            ])?;
            written += 1;
        }
    }
    tx.commit()?;
    Ok(written)
}

pub fn list_bars(
    conn: &Connection,
    instrument_id: i64,
    timeframe: Timeframe,
    from: Option<&str>,
    to: Option<&str>,
    limit: usize,
) -> Result<Vec<Bar>, DbError> {
    let limit = if limit == 0 { 500 } else { limit } as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, instrument_id, timeframe, ts, open, high, low, close, adj_close,
               volume, turnover, source, ingested_at
        FROM bars
        WHERE instrument_id = ?1
          AND timeframe = ?2
          AND (?3 IS NULL OR ts >= ?3)
          AND (?4 IS NULL OR ts <= ?4)
        ORDER BY ts ASC
        LIMIT ?5
        "#,
    )?;
    let rows = stmt.query_map(
        params![instrument_id, timeframe.as_str(), from, to, limit],
        map_bar,
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn bar_range(
    conn: &Connection,
    instrument_id: i64,
    timeframe: Timeframe,
) -> Result<Option<(String, String)>, DbError> {
    conn.query_row(
        "SELECT MIN(ts), MAX(ts) FROM bars WHERE instrument_id = ?1 AND timeframe = ?2",
        params![instrument_id, timeframe.as_str()],
        |r| {
            let min: Option<String> = r.get(0)?;
            let max: Option<String> = r.get(1)?;
            Ok(min.zip(max))
        },
    )
    .map_err(DbError::from)
}

pub fn count_bars(
    conn: &Connection,
    instrument_id: i64,
    timeframe: Timeframe,
) -> Result<i64, DbError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bars WHERE instrument_id = ?1 AND timeframe = ?2",
        params![instrument_id, timeframe.as_str()],
        |r| r.get(0),
    )?;
    Ok(n)
}

fn map_bar(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bar> {
    let tf: String = r.get(2)?;
    let timeframe = Timeframe::from_str(&tf).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
    })?;
    Ok(Bar {
        id: r.get(0)?,
        instrument_id: r.get(1)?,
        timeframe,
        ts: r.get(3)?,
        open: r.get(4)?,
        high: r.get(5)?,
        low: r.get(6)?,
        close: r.get(7)?,
        adj_close: r.get(8)?,
        volume: r.get(9)?,
        turnover: r.get(10)?,
        source: r.get(11)?,
        ingested_at: r.get(12)?,
    })
}
