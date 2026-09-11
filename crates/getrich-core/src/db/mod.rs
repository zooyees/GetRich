//! SQLite foundation store (WAL, migrations, typed access).

mod bars;
mod board;
mod freshness;
mod listings;
mod migrate;
mod quotes;
mod runs;

use crate::paths::db_path;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use bars::{bar_range, count_bars, list_bars, upsert_bars};
pub use board::{list_quote_board, store_stats, QuoteBoardQuery, QuoteBoardRow, QuoteSort};
pub use freshness::{list_freshness, mark_attempt, mark_error, mark_success, watermark};
pub use listings::{
    get_listing, get_listing_by_id, list_listings, list_metadata, upsert_listing, ListingQuery,
};
pub use migrate::{applied_migrations, migrate, migration_status};
pub use quotes::{get_quote, upsert_quote};
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
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection, DbError> {
    let conn = Connection::open_in_memory()?;
    apply_pragmas(&conn)?;
    migrate(&conn)?;
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
        assert_eq!(status.current_version, 2);
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
        let n1 = upsert_bars(&conn, id1, &[bar.clone()]).unwrap();
        let n2 = upsert_bars(&conn, id1, &[bar.clone()]).unwrap();
        assert_eq!(n1, 1);
        assert_eq!(n2, 1);
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

        mark_attempt(&conn, id, "bars:1d", "fixture").unwrap();
        mark_success(&conn, id, "bars:1d", "fixture", Some("2024-01-02")).unwrap();
        let wm = watermark(&conn, id, "bars:1d", "fixture").unwrap();
        assert_eq!(wm.as_deref(), Some("2024-01-02"));
    }
}
