use super::{now_rfc3339, DbError};
use crate::realtime::{CommittedBatch, MarketSnapshot};
use rusqlite::{params, Connection, OptionalExtension};

pub fn insert_ingest_batch(conn: &Connection, batch: &CommittedBatch) -> Result<u64, DbError> {
    conn.execute(
        r#"
        INSERT INTO ingest_batches (
            source, requested, accepted, changed, unchanged, stale, invalid,
            missing_listing, min_source_at, max_source_at, committed_at, detail_json
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
        "#,
        params![
            batch.source,
            batch.requested,
            batch.accepted,
            batch.changed,
            batch.unchanged,
            batch.stale,
            batch.invalid,
            batch.missing_listing,
            batch.min_source_at,
            batch.max_source_at,
            now_rfc3339(),
            batch.detail_json,
        ],
    )?;
    Ok(conn.last_insert_rowid() as u64)
}

pub fn latest_batch_version(conn: &Connection) -> Result<u64, DbError> {
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM ingest_batches",
        [],
        |row| row.get(0),
    )?;
    Ok(version.max(0) as u64)
}

pub fn persist_snapshot(conn: &Connection, snapshot: &MarketSnapshot) -> Result<(), DbError> {
    let payload = serde_json::to_string(snapshot).map_err(|e| DbError::Message(e.to_string()))?;
    conn.execute(
        r#"
        INSERT OR REPLACE INTO pulse_snapshots (
            trade_date, snapshot_version, phase, source_at, computed_at,
            quality, regime_code, payload_json, revision
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
        "#,
        params![
            snapshot.trade_date,
            snapshot.snapshot_version,
            snapshot.phase,
            snapshot.source_at,
            snapshot.computed_at,
            snapshot.quality.level,
            snapshot.pulse.regime.code,
            payload,
            snapshot.snapshot_revision,
        ],
    )?;
    Ok(())
}

pub fn latest_snapshot(conn: &Connection) -> Result<Option<MarketSnapshot>, DbError> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM pulse_snapshots ORDER BY revision DESC, computed_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    payload
        .map(|value| serde_json::from_str(&value).map_err(|e| DbError::Message(e.to_string())))
        .transpose()
}
