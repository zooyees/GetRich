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
    let tx = conn.unchecked_transaction()?;
    let written = upsert_bars_on(&tx, instrument_id, bars)?;
    tx.commit()?;
    Ok(written)
}

/// Insert bars on an existing connection/transaction (no nested `BEGIN`).
pub fn upsert_bars_on(
    conn: &Connection,
    instrument_id: i64,
    bars: &[BarSnapshot],
) -> Result<u32, DbError> {
    if bars.is_empty() {
        return Ok(0);
    }
    let ingested_at = now_rfc3339();
    let mut written = 0u32;
    {
        let mut stmt = conn.prepare(
            r#"
            INSERT INTO bars (
                instrument_id, timeframe, ts, open, high, low, close, adj_close,
                volume, turnover, source, ingested_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
            ON CONFLICT(instrument_id, timeframe, ts) DO UPDATE SET
                open = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.open ELSE excluded.open END,
                high = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.high ELSE excluded.high END,
                low = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.low ELSE excluded.low END,
                close = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.close ELSE excluded.close END,
                adj_close = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.adj_close ELSE COALESCE(excluded.adj_close, bars.adj_close) END,
                volume = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.volume ELSE COALESCE(excluded.volume, bars.volume) END,
                turnover = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.turnover ELSE COALESCE(excluded.turnover, bars.turnover) END,
                source = CASE WHEN bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%' THEN bars.source ELSE excluded.source END,
                ingested_at = excluded.ingested_at
            WHERE NOT (bars.source LIKE '%rt_k%' AND excluded.source LIKE '%daily%')
              AND (bars.open IS NOT excluded.open OR bars.high IS NOT excluded.high
                OR bars.low IS NOT excluded.low OR bars.close IS NOT excluded.close
                OR bars.adj_close IS NOT COALESCE(excluded.adj_close,bars.adj_close)
                OR bars.volume IS NOT COALESCE(excluded.volume,bars.volume)
                OR bars.turnover IS NOT COALESCE(excluded.turnover,bars.turnover)
                OR bars.source IS NOT excluded.source)
            "#,
        )?;
        for bar in bars {
            if !bar.is_valid() {
                return Err(DbError::Message("K 线 OHLC/量额无效".into()));
            }
            let ts = bar_ts_key(bar.ts, !bar.timeframe.is_intraday());
            written += stmt.execute(params![
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
            ])? as u32;
        }
    }
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
    let limit = if limit == 0 { 500 } else { limit.min(20000) } as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, instrument_id, timeframe, ts, open, high, low, close, adj_close,
               volume, turnover, source, ingested_at
        FROM (
            SELECT id, instrument_id, timeframe, ts, open, high, low, close, adj_close,
                   volume, turnover, source, ingested_at
            FROM bars
            WHERE instrument_id = ?1
              AND timeframe = ?2
              AND (?3 IS NULL OR ts >= ?3)
              AND (?4 IS NULL OR ts <= ?4)
            ORDER BY ts DESC
            LIMIT ?5
        ) newest
        ORDER BY ts ASC
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
