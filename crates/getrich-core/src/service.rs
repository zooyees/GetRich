//! Application service: migrate, import, and read-path viewing.

use crate::config::AppConfig;
use crate::db::{self, Database, ListingQuery, QuoteBoardQuery, QuoteSort};
use crate::error::AppError;
use crate::import::{self, ImportBundle, ImportKind};
use crate::models::{InstrumentKey, Timeframe};
use crate::symbol::{board_matches, parse_instrument};
use crate::view::{
    board_empty_message, quote_row_from_board, store_stats_from_counts, BarPage, BarStats,
    DetailEmpty, QuoteBoardPage, QuoteRow, StockDetail, StoreStats,
};
use rusqlite::{Connection, ErrorCode};
use std::path::Path;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct App {
    pub cfg: AppConfig,
    pub db: Database,
}

impl App {
    pub fn from_config(cfg: AppConfig) -> Self {
        let db = Database::from_config(&cfg.system.db_name);
        Self { cfg, db }
    }

    pub fn at_path(cfg: AppConfig, db_path: impl AsRef<Path>) -> Self {
        Self {
            cfg,
            db: Database::at(db_path),
        }
    }

    pub fn migrate(&self) -> Result<serde_json::Value, AppError> {
        self.with_retry(|conn| {
            let applied = db::migrate(conn)?;
            let status = db::migration_status(conn)?;
            Ok(serde_json::json!({
                "applied_now": applied,
                "current_version": status.current_version,
                "pending": status.pending,
                "applied": status.applied,
                "db_path": self.db.path.display().to_string(),
            }))
        })
    }

    pub fn db_status(&self) -> Result<serde_json::Value, AppError> {
        self.with_retry(|conn| {
            let status = db::migration_status(conn)?;
            let stats = self.stats_on(conn)?;
            Ok(serde_json::json!({
                "db_path": self.db.path.display().to_string(),
                "current_version": status.current_version,
                "pending": status.pending,
                "applied": status.applied,
                "stats": stats,
            }))
        })
    }

    pub fn stats(&self) -> Result<StoreStats, AppError> {
        self.with_retry(|conn| self.stats_on(conn))
    }

    fn stats_on(&self, conn: &Connection) -> Result<StoreStats, db::DbError> {
        let (listings, quotes, bars) = db::store_stats(conn)?;
        Ok(store_stats_from_counts(listings, quotes, bars))
    }

    pub fn import_file(&self, path: &Path, kind: ImportKind) -> Result<crate::models::IngestReport, AppError> {
        let bundle = import::load_file(path, kind)?;
        self.import_bundle(&bundle, &path.display().to_string())
    }

    pub fn import_bundle(
        &self,
        bundle: &ImportBundle,
        source: &str,
    ) -> Result<crate::models::IngestReport, AppError> {
        let mut last = None;
        let attempts = self.cfg.data.read_retries.max(1);
        for i in 1..=attempts {
            match import::apply_bundle(&self.db, bundle, source) {
                Ok(v) => return Ok(v),
                Err(e) if is_busy_err(&e) && i < attempts => {
                    tracing::warn!(attempt = i, error = %e, "import retry after busy");
                    thread::sleep(Duration::from_millis(
                        self.cfg.data.read_retry_backoff_ms * u64::from(i),
                    ));
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| AppError::Message("import failed".into())))
    }

    pub fn quote_board(
        &self,
        board: Option<&str>,
        query: Option<&str>,
        sort: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<QuoteBoardPage, AppError> {
        let board_s = board.unwrap_or("ALL").to_string();
        let query_s = query.unwrap_or("").to_string();
        let sort_s = sort.unwrap_or("symbol").to_string();
        let watch = self.watch_keys();

        self.with_retry(|conn| {
            let stats = self.stats_on(conn)?;
            let fetch_limit = if needs_post_filter(&board_s) {
                2000
            } else if limit == 0 {
                200
            } else {
                limit
            };
            let q = QuoteBoardQuery {
                market: market_for_board(&board_s),
                query: if query_s.trim().is_empty() {
                    None
                } else {
                    Some(query_s.clone())
                },
                status: None,
                limit: fetch_limit,
                offset: if needs_post_filter(&board_s) { 0 } else { offset },
                sort: QuoteSort::parse(&sort_s),
            };
            let raw = db::list_quote_board(conn, &q)?;
            let mut rows: Vec<QuoteRow> = raw.into_iter().map(quote_row_from_board).collect();
            if needs_post_filter(&board_s) {
                rows.retain(|r| row_matches_board(r, &board_s, &watch));
                let start = offset.min(rows.len());
                let take = if limit == 0 { 200 } else { limit };
                rows = rows.into_iter().skip(start).take(take).collect();
            }
            let empty = rows.is_empty();
            let (empty_kind, empty_message) = board_empty_message(&stats, empty && !stats.empty);
            let empty_message = if stats.empty {
                stats.message.clone()
            } else {
                empty_message
            };
            Ok(QuoteBoardPage {
                stats,
                board: board_s.clone(),
                query: query_s.clone(),
                sort: sort_s.clone(),
                count: rows.len(),
                empty,
                empty_kind,
                empty_message,
                rows,
            })
        })
    }

    pub fn show_stock(&self, key: &InstrumentKey) -> Result<StockDetail, AppError> {
        self.with_retry(|conn| {
            let listing = db::get_listing(conn, key)?.ok_or_else(|| {
                db::DbError::Message(format!("not found: {}", key.display()))
            })?;
            let quote = db::get_quote(conn, listing.id)?;
            let metadata = db::list_metadata(conn, listing.id)?;
            let tf = Timeframe::D1;
            let count = db::count_bars(conn, listing.id, tf)?;
            let range = db::bar_range(conn, listing.id, tf)?;
            let row = quote_row_from_board(db::QuoteBoardRow {
                instrument: listing.clone(),
                quote,
            });
            Ok(StockDetail {
                empty: DetailEmpty {
                    no_quote: !row.has_quote,
                    no_bars: count == 0,
                },
                quote: row,
                listing,
                metadata,
                bars: BarStats {
                    timeframe: tf.as_str().into(),
                    count,
                    from: range.as_ref().map(|r| r.0.clone()),
                    to: range.as_ref().map(|r| r.1.clone()),
                },
            })
        })
        .map_err(|e| match e {
            AppError::Db(db::DbError::Message(m)) if m.starts_with("not found:") => {
                AppError::NotFound(m)
            }
            other => other,
        })
    }

    pub fn list_bars(
        &self,
        key: &InstrumentKey,
        timeframe: Timeframe,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> Result<BarPage, AppError> {
        self.with_retry(|conn| {
            let listing = db::get_listing(conn, key)?.ok_or_else(|| {
                db::DbError::Message(format!("not found: {}", key.display()))
            })?;
            let rows = db::list_bars(conn, listing.id, timeframe, from, to, limit)?;
            let range = db::bar_range(conn, listing.id, timeframe)?;
            let empty = rows.is_empty();
            Ok(BarPage {
                symbol: key.symbol.clone(),
                market: key.market.as_str().into(),
                timeframe: timeframe.as_str().into(),
                count: rows.len(),
                from: range.as_ref().map(|r| r.0.clone()),
                to: range.as_ref().map(|r| r.1.clone()),
                empty,
                empty_message: if empty {
                    format!(
                        "暂无 {} K 线。导入 bars（timeframe={}）后将在此绘制。",
                        timeframe.as_str(),
                        timeframe.as_str()
                    )
                } else {
                    String::new()
                },
                rows,
            })
        })
        .map_err(|e| match e {
            AppError::Db(db::DbError::Message(m)) if m.starts_with("not found:") => {
                AppError::NotFound(m)
            }
            other => other,
        })
    }

    pub fn list_listings(
        &self,
        market: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::models::Instrument>, AppError> {
        let market = match market {
            Some(m) if !m.trim().is_empty() && !m.eq_ignore_ascii_case("ALL") => {
                Some(m.parse().map_err(AppError::InvalidArg)?)
            }
            _ => None,
        };
        self.with_retry(|conn| {
            db::list_listings(
                conn,
                &ListingQuery {
                    market,
                    query: query.filter(|s| !s.trim().is_empty()).map(|s| s.to_string()),
                    status: None,
                    limit,
                    offset: 0,
                },
            )
        })
    }

    pub fn parse_key(&self, symbol: &str, market: Option<&str>) -> Result<InstrumentKey, AppError> {
        parse_instrument(symbol, market).map_err(AppError::InvalidArg)
    }

    fn watch_keys(&self) -> Vec<InstrumentKey> {
        self.cfg
            .universe
            .watchlist
            .iter()
            .filter_map(|w| parse_instrument(&w.symbol, Some(&w.market)).ok())
            .collect()
    }

    fn with_retry<T, F>(&self, op: F) -> Result<T, AppError>
    where
        F: Fn(&Connection) -> Result<T, db::DbError>,
    {
        let attempts = self.cfg.data.read_retries.max(1);
        let mut last = None;
        for i in 1..=attempts {
            match self.db.connect().and_then(|c| op(&c)) {
                Ok(v) => return Ok(v),
                Err(e) if is_busy_db(&e) && i < attempts => {
                    tracing::warn!(attempt = i, error = %e, "db busy; retrying read");
                    thread::sleep(Duration::from_millis(
                        self.cfg.data.read_retry_backoff_ms * u64::from(i),
                    ));
                    last = Some(e);
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(last.unwrap().into())
    }
}

fn is_busy_db(err: &db::DbError) -> bool {
    match err {
        db::DbError::Sqlite(rusqlite::Error::SqliteFailure(e, _)) => {
            e.code == ErrorCode::DatabaseBusy || e.code == ErrorCode::DatabaseLocked
        }
        _ => false,
    }
}

fn is_busy_err(err: &AppError) -> bool {
    match err {
        AppError::Db(e) => is_busy_db(e),
        _ => false,
    }
}

fn needs_post_filter(board: &str) -> bool {
    let b = board.trim().to_ascii_uppercase();
    matches!(
        b.as_str(),
        "STAR"
            | "KCB"
            | "科创板"
            | "CHINEXT"
            | "CYB"
            | "创业板"
            | "WATCH"
            | "WATCHLIST"
            | "自选"
            | "CN"
            | "A"
            | "沪深京"
    )
}

fn market_for_board(board: &str) -> Option<crate::models::Market> {
    let b = board.trim().to_ascii_uppercase();
    match b.as_str() {
        "US" | "美股" => Some(crate::models::Market::Us),
        "HK" | "港股" => Some(crate::models::Market::Hk),
        "CN.SH" | "SH" | "沪市" => Some(crate::models::Market::CnSh),
        "CN.SZ" | "SZ" | "深市" => Some(crate::models::Market::CnSz),
        "CN.BJ" | "BJ" | "北证" => Some(crate::models::Market::CnBj),
        _ => None,
    }
}

fn row_matches_board(row: &QuoteRow, board: &str, watch: &[InstrumentKey]) -> bool {
    let b = board.trim().to_ascii_uppercase();
    if b == "WATCH" || b == "WATCHLIST" || board.trim() == "自选" {
        return watch.iter().any(|k| k.symbol == row.symbol && k.market.as_str() == row.market);
    }
    let Ok(key) = parse_instrument(&row.symbol, Some(&row.market)) else {
        return false;
    };
    board_matches(&key, board)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::import::{parse_json, ImportKind};

    fn temp_app() -> App {
        let dir = std::env::temp_dir().join(format!(
            "getrich-svc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = AppConfig::default();
        cfg.data.read_retries = 2;
        cfg.data.read_retry_backoff_ms = 1;
        App::at_path(cfg, dir.join("t.db"))
    }

    #[test]
    fn empty_board_has_stable_empty_state() {
        let app = temp_app();
        let page = app.quote_board(None, None, None, 50, 0).unwrap();
        assert!(page.empty);
        assert_eq!(page.empty_kind, "no_data");
        assert_eq!(page.stats.listings, 0);
        assert!(!page.empty_message.is_empty());
    }

    #[test]
    fn import_then_list_and_detail() {
        let app = temp_app();
        let json = r#"{
          "listings":[{"symbol":"600519","market":"CN.SH","name":"贵州茅台"}],
          "quotes":[{"symbol":"600519","market":"CN.SH","ts":"2024-01-02","last":1488,"open":1470,"high":1499,"low":1460,"prev_close":1470,"volume":1000,"turnover":1488000}],
          "bars":[
            {"symbol":"600519","market":"CN.SH","ts":"2024-01-02","open":1470,"high":1499,"low":1460,"close":1488,"volume":1000},
            {"symbol":"600519","market":"CN.SH","ts":"2024-01-03","open":1488,"high":1500,"low":1480,"close":1490,"volume":1100}
          ]
        }"#;
        let bundle = parse_json(json, ImportKind::Bundle).unwrap();
        app.import_bundle(&bundle, "test").unwrap();
        let page = app.quote_board(Some("沪市"), None, None, 50, 0).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].symbol, "600519");
        assert_eq!(page.rows[0].direction, "up");
        assert!(page.rows[0].change.unwrap() > 0.0);

        let key = app.parse_key("600519", Some("CN.SH")).unwrap();
        let detail = app.show_stock(&key).unwrap();
        assert_eq!(detail.listing.name.as_deref(), Some("贵州茅台"));
        assert!(!detail.empty.no_quote);
        assert!(!detail.empty.no_bars);

        let bars = app.list_bars(&key, Timeframe::D1, None, None, 100).unwrap();
        assert_eq!(bars.count, 2);
        assert!(!bars.empty);
    }

    #[test]
    fn missing_stock_is_not_found() {
        let app = temp_app();
        let key = app.parse_key("AAPL", Some("US")).unwrap();
        let err = app.show_stock(&key).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
