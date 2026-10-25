//! SQLite foundation store (WAL, migrations, typed access).

mod bars;
mod board;
mod freshness;
mod listings;
mod migrate;
mod pulse;
mod quotes;
mod realtime;
mod runs;

use crate::paths::db_path;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use bars::{bar_range, count_bars, list_bars, upsert_bars, upsert_bars_on};
pub use board::{
    count_quote_board, list_quote_board, store_stats, BoardScope, QuoteBoardQuery, QuoteBoardRow,
    QuoteSort,
};
pub use freshness::{
    latest_success, list_freshness, mark_attempt, mark_error, mark_success, watermark,
};
pub use listings::{
    ensure_watermark_listing, get_listing, get_listing_by_id, list_listing_ids, list_listings,
    list_metadata, upsert_listing, ListingQuery, WATERMARK_MARKET, WATERMARK_SYMBOL,
};
pub use migrate::{
    applied_migrations, ensure_current_schema, migrate, migration_status, schema_version,
};
pub use pulse::{
    daily_market_amounts, get_anchor, index_quotes, insert_anchor_if_missing, latest_vendor,
    list_post_amounts, pulse_tape_scan, replace_anchor, upsert_vendor, IndexQuote, PulseAnchor,
    PulseScanRow, PulseVendor,
};
pub use quotes::{get_quote, upsert_quote, upsert_quote_outcome, QuoteWriteOutcome};
pub use realtime::{insert_ingest_batch, latest_batch_version, latest_snapshot, persist_snapshot};
pub use runs::{finish_run, get_run, list_runs, start_run};

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("schema: {0}")]
    Schema(String),
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone)]
pub struct Database {
    pub path: PathBuf,
}

impl Database {
    pub fn from_config(db_name: &str) -> Self {
        Self {
            path: db_path(db_name),
        }
    }

    pub fn at(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn connect(&self) -> Result<Connection, DbError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| DbError::Message(e.to_string()))?;
            }
        }
        open_db(&self.path)
    }
}

pub fn open_db(path: impl AsRef<Path>) -> Result<Connection, DbError> {
    let conn = Connection::open(path)?;
    apply_pragmas(&conn)?;
    ensure_current_schema(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection, DbError> {
    let conn = Connection::open_in_memory()?;
    apply_pragmas(&conn)?;
    ensure_current_schema(&conn)?;
    Ok(conn)
}

pub fn apply_pragmas(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA temp_store = MEMORY;
        "#,
    )?;
    Ok(())
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn bar_ts_key(ts: chrono::DateTime<chrono::Utc>, daily: bool) -> String {
    if daily {
        ts.format("%Y-%m-%d").to_string()
    } else {
        ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        BarSnapshot, InstrumentKey, ListingSnapshot, Market, QuoteSnapshot, Timeframe,
    };
    use chrono::{TimeZone, Utc};

    fn sample_key() -> InstrumentKey {
        InstrumentKey::new("AAPL", Market::Us)
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        let first = migrate(&conn).unwrap();
        assert!(first.iter().any(|n| n == "001_init"));
        let second = migrate(&conn).unwrap();
        assert!(second.is_empty());
        let status = migration_status(&conn).unwrap();
        assert_eq!(status.current_version, schema_version());
        assert_eq!(schema_version(), 6);
        let user: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(user, schema_version());
        ensure_current_schema(&conn).unwrap();
        let user2: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(user2, schema_version());
    }

    #[test]
    fn rejects_future_schema_version() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", schema_version() + 1)
            .unwrap();
        let err = ensure_current_schema(&conn).unwrap_err().to_string();
        assert!(err.contains("newer than supported"));
    }

    #[test]
    fn listing_and_bar_roundtrip_idempotent() {
        let conn = open_memory().unwrap();
        let mut listing = ListingSnapshot::equity(sample_key(), "fixture", "AAPL");
        listing.name = Some("Apple Inc.".into());
        let id1 = upsert_listing(&conn, &listing).unwrap();
        listing.name = Some("Apple Inc.".into());
        let id2 = upsert_listing(&conn, &listing).unwrap();
        assert_eq!(id1, id2);

        let ts = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let bar = BarSnapshot {
            key: sample_key(),
            timeframe: Timeframe::D1,
            ts,
            open: 100.0,
            high: 110.0,
            low: 99.0,
            close: 105.0,
            adj_close: Some(105.0),
            volume: Some(1_000.0),
            turnover: None,
            source: "fixture".into(),
        };
        let n1 = upsert_bars(&conn, id1, std::slice::from_ref(&bar)).unwrap();
        let n2 = upsert_bars(&conn, id1, std::slice::from_ref(&bar)).unwrap();
        assert_eq!(n1, 1);
        assert_eq!(n2, 0);
        assert_eq!(count_bars(&conn, id1, Timeframe::D1).unwrap(), 1);

        let updated = BarSnapshot {
            close: 106.0,
            ..bar
        };
        upsert_bars(&conn, id1, &[updated]).unwrap();
        let rows = list_bars(&conn, id1, Timeframe::D1, None, None, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].close, 106.0);
    }

    #[test]
    fn list_bars_returns_latest_then_ascending() {
        let conn = open_memory().unwrap();
        let id = upsert_listing(
            &conn,
            &ListingSnapshot::equity(sample_key(), "fixture", "AAPL"),
        )
        .unwrap();
        let mk = |day, close| BarSnapshot {
            key: sample_key(),
            timeframe: Timeframe::D1,
            ts: Utc.with_ymd_and_hms(2024, 1, day, 0, 0, 0).unwrap(),
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            adj_close: None,
            volume: Some(1.0),
            turnover: None,
            source: "fixture".into(),
        };
        upsert_bars(&conn, id, &[mk(1, 10.0), mk(2, 20.0), mk(3, 30.0)]).unwrap();
        let rows = list_bars(&conn, id, Timeframe::D1, None, None, 2).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].close, 20.0);
        assert_eq!(rows[1].close, 30.0);
        assert_eq!(rows[0].ts, "2024-01-02");

        let live_bar = BarSnapshot {
            source: "tushare.rt_k".into(),
            close: 31.0,
            ..mk(3, 30.0)
        };
        upsert_bars(&conn, id, &[live_bar]).unwrap();
        let daily_same = BarSnapshot {
            source: "tushare.daily".into(),
            close: 20.0,
            low: 19.0,
            ..mk(3, 30.0)
        };
        upsert_bars(&conn, id, &[daily_same]).unwrap();
        let kept = list_bars(&conn, id, Timeframe::D1, None, None, 1).unwrap();
        assert_eq!(kept[0].close, 31.0);
        assert_eq!(kept[0].source, "tushare.rt_k");
    }

    #[test]
    fn quote_and_freshness() {
        let conn = open_memory().unwrap();
        let id = upsert_listing(
            &conn,
            &ListingSnapshot::equity(sample_key(), "fixture", "AAPL"),
        )
        .unwrap();
        let q = QuoteSnapshot {
            key: sample_key(),
            ts: Utc.with_ymd_and_hms(2024, 1, 2, 21, 0, 0).unwrap(),
            last: 105.0,
            open: Some(100.0),
            high: Some(110.0),
            low: Some(99.0),
            prev_close: Some(101.0),
            volume: Some(1_000.0),
            turnover: Some(105_000.0),
            change_pct: Some(3.96),
            currency: Some("USD".into()),
            source: "fixture".into(),
        };
        upsert_quote(&conn, id, &q).unwrap();
        let got = get_quote(&conn, id).unwrap().unwrap();
        assert_eq!(got.last, 105.0);

        let older = QuoteSnapshot {
            ts: Utc.with_ymd_and_hms(2024, 1, 2, 15, 0, 0).unwrap(),
            last: 90.0,
            low: Some(89.0),
            change_pct: None,
            ..q.clone()
        };
        upsert_quote(&conn, id, &older).unwrap();
        let still = get_quote(&conn, id).unwrap().unwrap();
        assert_eq!(still.last, 105.0);

        let newer = QuoteSnapshot {
            ts: Utc.with_ymd_and_hms(2024, 1, 2, 22, 0, 0).unwrap(),
            last: 108.0,
            change_pct: None,
            ..q
        };
        upsert_quote(&conn, id, &newer).unwrap();
        let got = get_quote(&conn, id).unwrap().unwrap();
        assert_eq!(got.last, 108.0);

        // Same-day date-only daily must not clobber live last.
        let live = QuoteSnapshot {
            key: sample_key(),
            ts: Utc.with_ymd_and_hms(2026, 9, 12, 2, 0, 0).unwrap(), // 10:00 Shanghai
            last: 10.0,
            open: Some(9.5),
            high: Some(10.2),
            low: Some(9.4),
            prev_close: Some(9.8),
            volume: Some(100.0),
            turnover: Some(1000.0),
            change_pct: Some(2.0),
            currency: Some("CNY".into()),
            source: "rt_k".into(),
        };
        upsert_quote(&conn, id, &live).unwrap();
        let daily_same = QuoteSnapshot {
            ts: Utc.with_ymd_and_hms(2026, 9, 12, 0, 0, 0).unwrap(),
            last: 9.0,
            low: Some(8.9),
            change_pct: None,
            source: "tushare.daily".into(),
            ..live.clone()
        };
        upsert_quote(&conn, id, &daily_same).unwrap();
        let still_live = get_quote(&conn, id).unwrap().unwrap();
        assert_eq!(still_live.last, 10.0);

        let id_eod = upsert_listing(
            &conn,
            &ListingSnapshot::equity(
                InstrumentKey::new("600519", Market::CnSh),
                "fixture",
                "600519",
            ),
        )
        .unwrap();
        let eod_first = QuoteSnapshot {
            key: InstrumentKey::new("600519", Market::CnSh),
            ts: Utc.with_ymd_and_hms(2026, 9, 12, 0, 0, 0).unwrap(),
            last: 9.2,
            open: Some(9.0),
            high: Some(9.5),
            low: Some(8.8),
            prev_close: Some(9.0),
            volume: Some(100.0),
            turnover: Some(1000.0),
            change_pct: Some(2.2),
            currency: Some("CNY".into()),
            source: "tushare.daily".into(),
        };
        upsert_quote(&conn, id_eod, &eod_first).unwrap();
        let live_after_eod = QuoteSnapshot {
            ts: Utc.with_ymd_and_hms(2026, 9, 12, 3, 0, 0).unwrap(),
            last: 10.8,
            high: Some(11.0),
            change_pct: None,
            source: "tushare.rt_k".into(),
            ..eod_first.clone()
        };
        upsert_quote(&conn, id_eod, &live_after_eod).unwrap();
        let refreshed = get_quote(&conn, id_eod).unwrap().unwrap();
        assert_eq!(refreshed.last, 10.8);

        // Cross-day: do not COALESCE yesterday's high onto today's last-only row.
        let next_day = QuoteSnapshot {
            key: sample_key(),
            ts: Utc.with_ymd_and_hms(2026, 9, 13, 0, 0, 0).unwrap(),
            last: 11.0,
            open: None,
            high: None,
            low: None,
            prev_close: Some(10.0),
            volume: None,
            turnover: None,
            change_pct: Some(10.0),
            currency: Some("CNY".into()),
            source: "daily".into(),
        };
        upsert_quote(&conn, id, &next_day).unwrap();
        let crossed = get_quote(&conn, id).unwrap().unwrap();
        assert_eq!(crossed.last, 11.0);
        assert!(crossed.high.is_none());
        assert!(crossed.open.is_none());

        mark_attempt(&conn, id, "bars:1d", "fixture").unwrap();
        mark_success(&conn, id, "bars:1d", "fixture", Some("2024-01-02")).unwrap();
        let wm = watermark(&conn, id, "bars:1d", "fixture").unwrap();
        assert_eq!(wm.as_deref(), Some("2024-01-02"));
    }
}
