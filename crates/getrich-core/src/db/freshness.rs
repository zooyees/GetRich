use super::{now_rfc3339, DbError};
use crate::models::FreshnessRow;
use rusqlite::{params, Connection, OptionalExtension};

pub fn mark_attempt(
    conn: &Connection,
    instrument_id: i64,
    dataset: &str,
    source: &str,
) -> Result<(), DbError> {
    conn.execute(
        r#"
        INSERT INTO data_freshness (
            instrument_id, dataset, source, last_attempt_at, status
        ) VALUES (?1,?2,?3,?4,'running')
        ON CONFLICT(instrument_id, dataset, source) DO UPDATE SET
            last_attempt_at = excluded.last_attempt_at,
            status = 'running',
            error = NULL
        "#,
        params![instrument_id, dataset, source, now_rfc3339()],
    )?;
    Ok(())
}

pub fn mark_success(
    conn: &Connection,
    instrument_id: i64,
    dataset: &str,
    source: &str,
    watermark_ts: Option<&str>,
) -> Result<(), DbError> {
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO data_freshness (
            instrument_id, dataset, source, last_attempt_at, last_success_at,
            watermark_ts, status, error
        ) VALUES (?1,?2,?3,?4,?4,?5,'ok',NULL)
        ON CONFLICT(instrument_id, dataset, source) DO UPDATE SET
            last_success_at = excluded.last_success_at,
            watermark_ts = COALESCE(excluded.watermark_ts, data_freshness.watermark_ts),
            status = 'ok',
            error = NULL
        "#,
        params![instrument_id, dataset, source, now, watermark_ts],
    )?;
    Ok(())
}

pub fn mark_error(
    conn: &Connection,
    instrument_id: i64,
    dataset: &str,
    source: &str,
    error: &str,
) -> Result<(), DbError> {
    conn.execute(
        r#"
        INSERT INTO data_freshness (
            instrument_id, dataset, source, last_attempt_at, status, error
        ) VALUES (?1,?2,?3,?4,'error',?5)
        ON CONFLICT(instrument_id, dataset, source) DO UPDATE SET
            last_attempt_at = excluded.last_attempt_at,
            status = 'error',
            error = excluded.error
        "#,
        params![instrument_id, dataset, source, now_rfc3339(), error],
    )?;
    Ok(())
}

pub fn watermark(
    conn: &Connection,
    instrument_id: i64,
    dataset: &str,
    source: &str,
) -> Result<Option<String>, DbError> {
    let row: Option<String> = conn
        .query_row(
            r#"
            SELECT watermark_ts FROM data_freshness
            WHERE instrument_id = ?1 AND dataset = ?2 AND source = ?3
            "#,
            params![instrument_id, dataset, source],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(row)
}

pub fn list_freshness(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<FreshnessRow>, DbError> {
    let limit = if limit == 0 { 200 } else { limit } as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT f.instrument_id, l.symbol, l.market, f.dataset, f.source,
               f.last_attempt_at, f.last_success_at, f.watermark_ts, f.status, f.error
        FROM data_freshness f
        JOIN listings l ON l.id = f.instrument_id
        ORDER BY f.last_attempt_at DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(FreshnessRow {
            instrument_id: r.get(0)?,
            symbol: r.get(1)?,
            market: r.get(2)?,
            dataset: r.get(3)?,
            source: r.get(4)?,
            last_attempt_at: r.get(5)?,
            last_success_at: r.get(6)?,
            watermark_ts: r.get(7)?,
            status: r.get(8)?,
            error: r.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
