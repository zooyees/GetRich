use super::{now_rfc3339, DbError};
use crate::models::IngestRun;
use rusqlite::{params, Connection, OptionalExtension};

pub fn start_run(conn: &Connection, kind: &str, source: &str) -> Result<i64, DbError> {
    conn.execute(
        r#"
        INSERT INTO ingest_runs (kind, source, status, started_at)
        VALUES (?1, ?2, 'running', ?3)
        "#,
        params![kind, source, now_rfc3339()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_run(
    conn: &Connection,
    run_id: i64,
    status: &str,
    instruments: u32,
    bars: u32,
    quotes: u32,
    error: Option<&str>,
    detail_json: Option<&str>,
) -> Result<(), DbError> {
    conn.execute(
        r#"
        UPDATE ingest_runs SET
            status = ?1,
            finished_at = ?2,
            instruments_upserted = ?3,
            bars_upserted = ?4,
            quotes_upserted = ?5,
            error = ?6,
            detail_json = ?7
        WHERE id = ?8
        "#,
        params![
            status,
            now_rfc3339(),
            instruments as i64,
            bars as i64,
            quotes as i64,
            error,
            detail_json,
            run_id,
        ],
    )?;
    Ok(())
}

pub fn get_run(conn: &Connection, run_id: i64) -> Result<Option<IngestRun>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, kind, source, status, started_at, finished_at,
               instruments_upserted, bars_upserted, quotes_upserted, error
        FROM ingest_runs WHERE id = ?1
        "#,
    )?;
    let row = stmt
        .query_row(params![run_id], map_run)
        .optional()?;
    Ok(row)
}

pub fn list_runs(conn: &Connection, limit: usize) -> Result<Vec<IngestRun>, DbError> {
    let limit = if limit == 0 { 20 } else { limit } as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, kind, source, status, started_at, finished_at,
               instruments_upserted, bars_upserted, quotes_upserted, error
        FROM ingest_runs ORDER BY id DESC LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit], map_run)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn map_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<IngestRun> {
    Ok(IngestRun {
        id: r.get(0)?,
        kind: r.get(1)?,
        source: r.get(2)?,
        status: r.get(3)?,
        started_at: r.get(4)?,
        finished_at: r.get(5)?,
        instruments_upserted: r.get(6)?,
        bars_upserted: r.get(7)?,
        quotes_upserted: r.get(8)?,
        error: r.get(9)?,
    })
}
