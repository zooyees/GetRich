//! Application service: migrate, import, and read-path viewing.

use crate::config::{AppConfig, WatchItem};
use crate::db::{self, BoardScope, Database, ListingQuery, QuoteBoardQuery, QuoteSort};
use crate::error::AppError;
use crate::events::EventHub;
use crate::import::{self, ImportBundle, ImportKind};
use crate::ingest::IngestState;
use crate::models::{IngestRun, InstrumentKey, Timeframe};
use crate::paths;
use crate::pulse::MarketPulse;
use crate::realtime::{MarketSnapshot, RealtimeState};
use crate::session;
use crate::symbol::parse_instrument;
use crate::tushare;
use crate::view::{
    board_empty_message, quote_row_from_board, store_stats_from_counts, BarPage, BarStats,
    DetailEmpty, QuoteBoardPage, QuoteRow, StockDetail, StoreStats,
};
use rusqlite::{Connection, ErrorCode};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct App {
    pub cfg: AppConfig,
    pub db: Database,
    pub(crate) config_path: PathBuf,
    watchlist: Mutex<Vec<WatchItem>>,
    default_board: Mutex<String>,
    pub(crate) config_write: Mutex<()>,
    provider: Mutex<Option<(String, Arc<tushare::Client>)>>,
    pub events: EventHub,
    pub ingest: IngestState,
    pub realtime: RealtimeState,
    pub jobs: crate::jobs::JobState,
}

impl App {
    pub fn resolve_import_path(&self, path: &Path, server_only: bool) -> Result<PathBuf, AppError> {
        if path.as_os_str().is_empty() {
            return Err(AppError::InvalidArg("请选择文件或填写路径".into()));
        }
        let root = self.config_path.parent().unwrap_or_else(|| Path::new("."));
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        let canonical = resolved.canonicalize().map_err(|e| {
            AppError::InvalidArg(format!("无法读取导入文件 {}：{e}", resolved.display()))
        })?;
        if server_only && !canonical.starts_with(root.canonicalize()?) {
            return Err(AppError::InvalidArg(
                "服务端文件必须位于数据目录内；外部文件请使用选择文件或上传导入".into(),
            ));
        }
        Ok(canonical)
    }
    pub fn from_config(cfg: AppConfig) -> Self {
        let db = Database::from_config(&cfg.system.db_name);
        let watchlist = Mutex::new(cfg.universe.watchlist.clone());
        let default_board = Mutex::new(cfg.universe.default_board.clone());
        Self {
            cfg,
            db,
            config_path: paths::config_file(),
            watchlist,
            default_board,
            config_write: Mutex::new(()),
            provider: Mutex::new(None),
            events: EventHub::new(),
            ingest: IngestState::new(),
            realtime: RealtimeState::default(),
            jobs: Default::default(),
        }
    }

    pub fn at_path(cfg: AppConfig, db_path: impl AsRef<Path>) -> Self {
        let path = db_path.as_ref().to_path_buf();
        let config_path = path
            .parent()
            .map(|p| p.join("config.json"))
            .unwrap_or_else(|| PathBuf::from("config.json"));
        let watchlist = Mutex::new(cfg.universe.watchlist.clone());
        let default_board = Mutex::new(cfg.universe.default_board.clone());
        Self {
            cfg,
            db: Database::at(path),
            config_path,
            watchlist,
            default_board,
            config_write: Mutex::new(()),
            provider: Mutex::new(None),
            events: EventHub::new(),
            ingest: IngestState::new(),
            realtime: RealtimeState::default(),
            jobs: Default::default(),
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

    pub fn market_pulse(&self) -> MarketPulse {
        if let Some(snapshot) = self.realtime_snapshot() {
            return snapshot.pulse.clone();
        }
        match self.with_retry(|conn| {
            self.compute_market_pulse_on(conn)
                .map_err(|e| db::DbError::Message(e.to_string()))
        }) {
            Ok(p) => p,
            Err(e) => crate::pulse::error_pulse(&e.to_string()),
        }
    }

    pub fn realtime_snapshot(&self) -> Option<Arc<MarketSnapshot>> {
        if let Err(error) = self.realtime.refresh_from_store(&self.db) {
            tracing::debug!(%error,"跨实例快照同步失败");
        }
        self.realtime.snapshot()
    }

    pub(crate) fn compute_market_pulse_on(
        &self,
        conn: &Connection,
    ) -> Result<MarketPulse, AppError> {
        crate::pulse::compute(
            conn,
            &self.cfg.data.pulse,
            self.cfg.data.tushare.poll_ms.max(1),
            &self.watchlist(),
            chrono::Utc::now(),
        )
    }

    fn stats_on(&self, conn: &Connection) -> Result<StoreStats, db::DbError> {
        let (listings, quotes, bars, last_quote_ts) = db::store_stats(conn)?;
        let last_success_at = db::latest_success(conn, "quotes", "tushare")?;
        let mut stats =
            store_stats_from_counts(listings, quotes, bars, last_quote_ts, last_success_at);
        let (daily,weekly,monthly)=conn.query_row("SELECT COALESCE(SUM(timeframe='1d'),0),COALESCE(SUM(timeframe='1w'),0),COALESCE(SUM(timeframe='1mo'),0) FROM bars",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        stats.daily_bars = daily;
        stats.weekly_bars = weekly;
        stats.monthly_bars = monthly;
        Ok(stats)
    }

    pub fn import_file(
        &self,
        path: &Path,
        kind: ImportKind,
    ) -> Result<crate::models::IngestReport, AppError> {
        let resolved = self.resolve_import_path(path, false)?;
        let path = resolved.as_path();
        let bundle = import::load_file(path, kind)?;
        let source = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("import.file");
        self.import_bundle(&bundle, source)
    }

    pub fn import_bundle(
        &self,
        bundle: &ImportBundle,
        source: &str,
    ) -> Result<crate::models::IngestReport, AppError> {
        self.import_bundle_with_event(bundle, source, true)
    }

    fn import_bundle_with_event(
        &self,
        bundle: &ImportBundle,
        source: &str,
        emit_event: bool,
    ) -> Result<crate::models::IngestReport, AppError> {
        let mut last = None;
        let attempts = self.cfg.data.read_retries.max(1);
        for i in 1..=attempts {
            match import::apply_bundle(&self.db, bundle, source) {
                Ok(v) => {
                    if emit_event {
                        crate::ingest::emit_ingest_done(
                            &self.events,
                            &v.kind,
                            v.quotes_upserted,
                            v.bars_upserted,
                            v.instruments_upserted,
                            &v.status,
                        );
                    }
                    return Ok(v);
                }
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

    pub(crate) fn tushare_cfg(&self) -> crate::config::TushareConfig {
        let mut cfg = self
            .read_settings_config()
            .unwrap_or_else(|_| self.cfg.clone());
        if let Ok(token) = std::env::var("GETRICH_TUSHARE_TOKEN") {
            if !token.trim().is_empty() {
                cfg.data.tushare.token = token.trim().to_string();
            }
        }
        if let Ok(user) = std::env::var("GETRICH_TUSHARE_USER") {
            if !user.trim().is_empty() {
                cfg.data.tushare.user_id = user.trim().to_string();
            }
        }
        cfg.data.tushare
    }

    pub(crate) fn tushare_client(&self) -> Result<Arc<tushare::Client>, AppError> {
        let c = self.tushare_cfg();
        let key = serde_json::to_string(&c).map_err(|e| AppError::Message(e.to_string()))?;
        let mut provider = self.provider.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((old, client)) = provider.as_ref() {
            if old == &key {
                return Ok(Arc::clone(client));
            }
        }
        let client = Arc::new(
            tushare::Client::from_cfg(&c, self.cfg.data.read_timeout_ms)?
                .with_shared_store(self.db.clone()),
        );
        *provider = Some((key, Arc::clone(&client)));
        Ok(client)
    }

    pub fn tushare_status(&self) -> serde_json::Value {
        let c = self.tushare_cfg();
        let last_ok = self.ingest.last_ok.lock().ok().and_then(|g| g.clone());
        let last_err = self.ingest.last_err.lock().ok().and_then(|g| g.clone());
        let last_api = self.ingest.last_api.lock().ok().and_then(|g| g.clone());
        let freshness = self
            .with_retry(|conn| db::latest_success(conn, "quotes", "tushare"))
            .ok()
            .flatten();
        serde_json::json!({
            "configured": !c.token.trim().is_empty(),
            "user_id": c.user_id,
            "token_mask": tushare::mask_token(&c.token),
            "source": "tushare",
            "enabled": c.enabled,
            "poll_ms": c.poll_ms,
            "universe": c.universe,
            "rt_k_codes": c.rt_k_codes,
            "live_timeout_ms": c.live_timeout_ms,
            "rate_per_min": c.rate_per_min,
            "worker_running": c.enabled && !c.token.trim().is_empty() && self.ingest.worker_running(),
            "worker_started": self.ingest.started(),
            "in_session": self.ingest.in_session(),
            "source_api": last_api,
            "last_success_at": last_ok.or(freshness),
            "last_error": last_err,
        })
    }

    pub fn ingest_tushare(&self) -> Result<crate::models::IngestReport, AppError> {
        let client = self.tushare_client()?;
        let today = session::yyyymmdd(session::shanghai_today());
        let conn = self.db.connect()?;
        let wm_id = db::ensure_watermark_listing(&conn)?;
        let skip_listings =
            db::watermark(&conn, wm_id, "listings", "tushare")?.as_deref() == Some(today.as_str());

        let mut instruments = 0u32;
        let mut quotes = 0u32;
        let mut bars = 0u32;
        let mut errors = Vec::new();
        let mut run_id = 0i64;
        let mut status = "ok".to_string();
        let mut counts = crate::models::ImportCounts::default();

        if !skip_listings {
            let listings = client.listings_bundle()?;
            let r = self.import_bundle_with_event(&listings, "tushare", false)?;
            counts.requested += r.counts.requested;
            counts.inserted += r.counts.inserted;
            counts.updated += r.counts.updated;
            counts.unchanged += r.counts.unchanged;
            counts.stale += r.counts.stale;
            counts.invalid += r.counts.invalid;
            counts.failed += r.counts.failed;
            instruments += r.instruments_upserted;
            errors.extend(r.errors);
            run_id = r.run_id;
            if r.status == "ok" && r.instruments_upserted > 0 {
                db::mark_success(&conn, wm_id, "listings", "tushare", Some(&today))?;
            }
            status = r.status;
        }

        let daily = client.board_bundle(false)?;
        let r = self.import_bundle_with_event(&daily, "tushare", false)?;
        counts.requested += r.counts.requested;
        counts.inserted += r.counts.inserted;
        counts.updated += r.counts.updated;
        counts.unchanged += r.counts.unchanged;
        counts.stale += r.counts.stale;
        counts.invalid += r.counts.invalid;
        counts.failed += r.counts.failed;
        quotes += r.quotes_upserted;
        bars += r.bars_upserted;
        instruments += r.instruments_upserted;
        errors.extend(r.errors);
        if run_id == 0 {
            run_id = r.run_id;
        }
        if r.status != "ok" {
            status = r.status;
        }
        let wm = session::iso_date(session::shanghai_today());
        if status == "ok" && quotes > 0 {
            db::mark_success(&conn, wm_id, "quotes", "tushare", Some(&wm))?;
        }
        if status == "ok" && bars > 0 {
            db::mark_success(&conn, wm_id, "bars.1d", "tushare", Some(&wm))?;
        }
        crate::ingest::emit_ingest_done(
            &self.events,
            "tushare.sync",
            quotes,
            bars,
            instruments,
            &status,
        );
        Ok(crate::models::IngestReport {
            run_id,
            kind: "tushare.sync".into(),
            source: "tushare".into(),
            status,
            instruments_upserted: instruments,
            bars_upserted: bars,
            quotes_upserted: quotes,
            errors,
            counts,
        })
    }

    pub fn sync_tushare_bars(
        &self,
        key: &InstrumentKey,
        days: i64,
    ) -> Result<crate::models::IngestReport, AppError> {
        let client = self.tushare_client()?;
        let today = session::shanghai_today();
        let end = session::yyyymmdd(today);
        let start = session::yyyymmdd(session::days_before(today, days.max(400)));
        let bundle = client.bars_bundle(key, &start, &end)?;
        self.import_bundle(&bundle, "tushare")
    }

    pub fn sync_tushare_quote(
        &self,
        key: &InstrumentKey,
    ) -> Result<crate::models::IngestReport, AppError> {
        let client = self.tushare_client()?;
        let bundle = client.quote_bundle(key)?;
        let report = import::apply_quotes_hot(
            &self.db,
            &bundle.quotes,
            &bundle.bars,
            "tushare",
            self.cfg.system.db_commit_batch_size,
        )?;
        if let Some(api) = client.last_quote_api() {
            if let Ok(mut g) = self.ingest.last_api.lock() {
                *g = Some(api);
            }
        }
        if let Ok(conn) = self.db.connect() {
            if let Ok(id) = db::ensure_watermark_listing(&conn) {
                let _ =
                    db::mark_success(&conn, id, "quotes", "tushare", report.watermark.as_deref());
            }
        }
        crate::ingest::emit_quote_updated(&self.events, &report);
        Ok(report.as_ingest("tushare"))
    }

    pub fn list_freshness(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::models::FreshnessRow>, AppError> {
        self.with_retry(|conn| db::list_freshness(conn, limit))
    }

    pub fn quote_board(
        &self,
        board: Option<&str>,
        query: Option<&str>,
        sort: Option<&str>,
        limit: usize,
        offset: usize,
        industry: Option<&str>,
    ) -> Result<QuoteBoardPage, AppError> {
        self.quote_board_filtered(board, query, sort, limit, offset, industry, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quote_board_filtered(
        &self,
        board: Option<&str>,
        query: Option<&str>,
        sort: Option<&str>,
        limit: usize,
        offset: usize,
        industry: Option<&str>,
        change_filter: Option<&str>,
    ) -> Result<QuoteBoardPage, AppError> {
        if change_filter
            .is_some_and(|f| !["", "up", "down", "flat", "limit_up", "limit_down"].contains(&f))
        {
            return Err(AppError::InvalidArg("无效涨跌筛选".into()));
        }
        let board_s = board.unwrap_or("ALL").to_string();
        let query_s = query.unwrap_or("").to_string();
        let sort_s = sort.unwrap_or("symbol").to_string();
        let industry_s = industry.unwrap_or("").trim().to_string();
        let watch = self.watch_keys();

        self.with_retry(|conn| {
            let stats = self.stats_on(conn)?;
            let mut q = QuoteBoardQuery {
                scope: BoardScope::parse(&board_s, watch.clone()),
                query: if query_s.trim().is_empty() {
                    None
                } else {
                    Some(query_s.clone())
                },
                industry: if industry_s.is_empty() {
                    None
                } else {
                    Some(industry_s.clone())
                },
                status: None,
                limit: if limit == 0 { 200 } else { limit },
                offset,
                sort: QuoteSort::parse(&sort_s),
            };
            let is_watch = matches!(&q.scope, BoardScope::Watch(_));
            let has_filter = change_filter.is_some_and(|f| !f.is_empty());
            if is_watch || has_filter {
                q.limit = 100_000;
                q.offset = 0;
            }
            let mut raw = db::list_quote_board(conn, &q)?;
            if has_filter {
                let ids = crate::pulse::matching_ids(
                    conn,
                    &self.cfg.data.pulse,
                    self.tushare_cfg().poll_ms,
                    change_filter.unwrap_or(""),
                )
                .map_err(|e| db::DbError::Message(e.to_string()))?;
                raw.retain(|r| ids.contains(&r.instrument.id));
            }
            let mut rows: Vec<QuoteRow> = raw.into_iter().map(quote_row_from_board).collect();
            let matched = if is_watch {
                if industry_s.is_empty() && change_filter.unwrap_or("").is_empty() {
                    let absent: Vec<_> = self
                        .watchlist()
                        .into_iter()
                        .filter(|w| {
                            self.parse_key(&w.symbol, Some(&w.market))
                                .ok()
                                .is_some_and(|k| db::get_listing(conn, &k).ok().flatten().is_none())
                        })
                        .collect();
                    rows.extend(merge_watch_rows(Vec::new(), &absent, &query_s));
                }
                crate::read_model::sort_rows(&mut rows, &sort_s);
                rows.len()
            } else if has_filter {
                rows.len()
            } else {
                db::count_quote_board(conn, &q)? as usize
            };
            if is_watch || has_filter {
                let start = offset.min(matched);
                let take = if limit == 0 { 200 } else { limit };
                let end = (start + take).min(matched);
                rows = rows[start..end].to_vec();
            }
            let empty = rows.is_empty();
            let watch_empty = matches!(&q.scope, BoardScope::Watch(w) if w.is_empty());
            let (empty_kind, mut empty_message) = if watch_empty {
                (
                    "no_watch".to_string(),
                    "还没有自选。在详情点星号，或到设置添加。".to_string(),
                )
            } else {
                board_empty_message(&stats, empty && !stats.empty)
            };
            if !industry_s.is_empty() {
                empty_message = format!("行业={industry_s} · {matched} 只");
            }
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
                industry: industry_s.clone(),
                count: rows.len(),
                matched,
                truncated: matched > rows.len(),
                empty,
                empty_kind,
                empty_message,
                rows,
            })
        })
    }

    pub fn show_stock(&self, key: &InstrumentKey) -> Result<StockDetail, AppError> {
        let found = self.with_retry(|conn| {
            let Some(listing) = db::get_listing(conn, key)? else {
                return Ok(None);
            };
            let quote = db::get_quote(conn, listing.id)?;
            let metadata = db::list_metadata(conn, listing.id)?;
            let tf = Timeframe::D1;
            let count = db::count_bars(conn, listing.id, tf)?;
            let range = db::bar_range(conn, listing.id, tf)?;
            let row = quote_row_from_board(db::QuoteBoardRow {
                instrument: listing.clone(),
                quote,
            });
            Ok(Some(StockDetail {
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
                watched: self.is_watched(key),
            }))
        })?;
        match found {
            Some(detail) => Ok(detail),
            None if self.is_watched(key) => Ok(self.ghost_stock(key)),
            None => Err(AppError::NotFound(key.display())),
        }
    }

    fn ghost_stock(&self, key: &InstrumentKey) -> StockDetail {
        let name = self.watchlist().into_iter().find_map(|w| {
            parse_instrument(&w.symbol, Some(&w.market))
                .ok()
                .filter(|k| k.symbol == key.symbol && k.market == key.market)
                .and(w.name)
        });
        let listing = crate::models::Instrument {
            id: 0,
            key: key.clone(),
            exchange: Some(key.market.default_exchange().into()),
            name: name.clone(),
            name_en: None,
            currency: Some(key.market.default_currency().into()),
            instrument_type: "equity".into(),
            isin: None,
            listing_date: None,
            delisting_date: None,
            status: "active".into(),
            sector: None,
            industry: None,
            lot_size: None,
            source: Some("watchlist".into()),
            source_symbol: Some(key.symbol.clone()),
            extra_json: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let row = quote_row_from_board(db::QuoteBoardRow {
            instrument: listing.clone(),
            quote: None,
        });
        StockDetail {
            empty: DetailEmpty {
                no_quote: true,
                no_bars: true,
            },
            quote: row,
            listing,
            metadata: Vec::new(),
            bars: BarStats {
                timeframe: Timeframe::D1.as_str().into(),
                count: 0,
                from: None,
                to: None,
            },
            watched: true,
        }
    }

    pub fn list_bars(
        &self,
        key: &InstrumentKey,
        timeframe: Timeframe,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> Result<BarPage, AppError> {
        use chrono::Datelike;
        let limit = if limit == 0 { 500 } else { limit.min(20000) };
        self.with_retry(|conn| {
            let Some(listing) = db::get_listing(conn, key)? else {
                return Ok(BarPage {
                    basis_timeframe: timeframe.as_str().into(),
                    partial_last_period: false,
                    data_version: db::latest_batch_version(conn)?,
                    symbol: key.symbol.clone(),
                    market: key.market.as_str().into(),
                    timeframe: timeframe.as_str().into(),
                    count: 0,
                    from: None,
                    to: None,
                    empty: true,
                    empty_message: if self.is_watched(key) {
                        "自选尚未导入 listings，暂无 K 线。".into()
                    } else {
                        format!("未找到 {}。请确认已导入 listings。", key.display())
                    },
                    rows: Vec::new(),
                });
            };
            let mut basis_timeframe = timeframe.as_str();
            let mut rows = db::list_bars(conn, listing.id, timeframe, from, to, limit)?;
            if matches!(timeframe, Timeframe::W1 | Timeframe::Mo1) {
                let daily = db::list_bars(conn, listing.id, Timeframe::D1, from, to, 20000)?;
                if !daily.is_empty() {
                    basis_timeframe = "1d";
                    rows = crate::resample::resample_bars(&daily, timeframe);
                }
                if rows.len() > limit {
                    rows = rows[rows.len() - limit..].to_vec();
                }
            }
            let empty = rows.is_empty();
            let range = if rows.is_empty() {
                db::bar_range(conn, listing.id, timeframe)?
            } else {
                Some((
                    rows.first().map(|b| b.ts.clone()).unwrap_or_default(),
                    rows.last().map(|b| b.ts.clone()).unwrap_or_default(),
                ))
            };
            Ok(BarPage {
                basis_timeframe: basis_timeframe.into(),
                partial_last_period: rows
                    .last()
                    .and_then(|b| session::calendar_date_of(&b.ts))
                    .is_some_and(|day| {
                        let today = session::shanghai_today();
                        match timeframe {
                            Timeframe::W1 => day.iso_week() == today.iso_week(),
                            Timeframe::Mo1 => {
                                day.year() == today.year() && day.month() == today.month()
                            }
                            _ => false,
                        }
                    }),
                data_version: db::latest_batch_version(conn)?,
                symbol: key.symbol.clone(),
                market: key.market.as_str().into(),
                timeframe: timeframe.as_str().into(),
                count: rows.len(),
                from: range.as_ref().map(|r| r.0.clone()),
                to: range.as_ref().map(|r| r.1.clone()),
                empty,
                empty_message: if empty {
                    "暂无该周期 K 线。导入日 K 后将自动合成周/月线，或从 Tushare 拉取。".to_string()
                } else {
                    String::new()
                },
                rows,
            })
        })
        .map_err(|e| match e {
            AppError::Db(db::DbError::Message(m)) if m.starts_with("not found:") => {
                AppError::NotFound(m.trim_start_matches("not found:").trim().to_string())
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
                    query: query
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| s.to_string()),
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

    pub fn ingest_runs(&self, limit: usize) -> Result<Vec<IngestRun>, AppError> {
        self.with_retry(|conn| db::list_runs(conn, limit))
    }

    pub fn watchlist(&self) -> Vec<WatchItem> {
        if let Ok(cfg) = self.read_settings_config() {
            return cfg.universe.watchlist;
        }
        self.watchlist
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn is_watched(&self, key: &InstrumentKey) -> bool {
        self.watch_keys()
            .iter()
            .any(|k| k.symbol == key.symbol && k.market == key.market)
    }

    pub fn toggle_watch(
        &self,
        symbol: &str,
        market: Option<&str>,
        name: Option<String>,
    ) -> Result<serde_json::Value, AppError> {
        self.change_watch(symbol, market, name, None)
    }

    pub fn set_watch(
        &self,
        symbol: &str,
        market: Option<&str>,
        name: Option<String>,
        watched: bool,
    ) -> Result<serde_json::Value, AppError> {
        self.change_watch(symbol, market, name, Some(watched))
    }

    pub fn default_board(&self) -> String {
        if let Ok(cfg) = self.read_settings_config() {
            return cfg.universe.default_board;
        }
        self.default_board
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_default_board(&self, board: &str) -> Result<serde_json::Value, AppError> {
        let board = if board.trim().is_empty() {
            "ALL".to_string()
        } else {
            board.trim().to_string()
        };
        crate::settings::validate_board(&board)?;
        self.update_settings(|cfg| {
            cfg.universe.default_board = board.clone();
            Ok(())
        })?;
        *self.default_board.lock().unwrap_or_else(|e| e.into_inner()) = board.clone();
        Ok(serde_json::json!({ "default_board": board }))
    }

    pub(crate) fn write_config(&self, cfg: &AppConfig) -> Result<(), AppError> {
        let mut out = cfg.clone();
        let disk = self.disk_config();
        if let Ok(token) = std::env::var("GETRICH_TUSHARE_TOKEN") {
            let token = token.trim();
            if !token.is_empty() {
                out.data.tushare.token = disk
                    .as_ref()
                    .map(|c| c.data.tushare.token.clone())
                    .unwrap_or_default();
            }
        }
        if let Ok(uid) = std::env::var("GETRICH_TUSHARE_USER") {
            let uid = uid.trim();
            if !uid.is_empty() {
                out.data.tushare.user_id = disk
                    .as_ref()
                    .map(|c| c.data.tushare.user_id.clone())
                    .unwrap_or_default();
            }
        }
        if out.data.tushare.token.trim().is_empty() {
            out.data.tushare.token.clear();
        }
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text =
            serde_json::to_string_pretty(&out).map_err(|e| AppError::Message(e.to_string()))?;
        atomic_write(&self.config_path, text.as_bytes())?;
        Ok(())
    }

    fn disk_config(&self) -> Option<AppConfig> {
        let text = std::fs::read_to_string(&self.config_path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub(crate) fn watch_keys(&self) -> Vec<InstrumentKey> {
        self.watchlist()
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

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config.json");
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    replace_file(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let existing: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let new: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
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

fn merge_watch_rows(rows: Vec<QuoteRow>, watch: &[WatchItem], query: &str) -> Vec<QuoteRow> {
    let q = query.trim().to_ascii_lowercase();
    let mut out = Vec::new();
    for w in watch {
        let Ok(key) = parse_instrument(&w.symbol, Some(&w.market)) else {
            continue;
        };
        let name = w.name.clone().unwrap_or_default();
        if !q.is_empty() {
            let hit = key.symbol.to_ascii_lowercase().contains(&q)
                || name.to_ascii_lowercase().contains(&q)
                || key.market.as_str().to_ascii_lowercase().contains(&q);
            if !hit {
                continue;
            }
        }
        if let Some(existing) = rows
            .iter()
            .find(|r| r.symbol == key.symbol && r.market == key.market.as_str())
        {
            out.push(existing.clone());
        } else {
            let board = crate::symbol::board_id(&key).to_string();
            out.push(QuoteRow {
                trade_date: None,
                received_at: None,
                source_kind: "missing".into(),
                data_status: "未导入".into(),
                instrument_id: 0,
                symbol: key.symbol.clone(),
                market: key.market.as_str().into(),
                market_label: key.market.label_zh().into(),
                board: board.clone(),
                board_label: crate::symbol::board_label_zh(&board).into(),
                name: w.name.clone(),
                exchange: Some(key.market.default_exchange().into()),
                currency: Some(key.market.default_currency().into()),
                status: "active".into(),
                last: None,
                change: None,
                change_pct: None,
                open: None,
                high: None,
                low: None,
                prev_close: None,
                amplitude: None,
                volume: None,
                turnover: None,
                ts: None,
                has_quote: false,
                direction: "none".into(),
                source: None,
            });
        }
    }
    out
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
        let page = app.quote_board(None, None, None, 50, 0, None).unwrap();
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
        let page = app
            .quote_board(Some("沪市"), None, None, 50, 0, None)
            .unwrap();
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
    fn star_and_chinext_filter_in_sql() {
        let app = temp_app();
        let json = r#"{
          "listings":[
            {"symbol":"600519","market":"CN.SH","name":"贵州茅台"},
            {"symbol":"688981","market":"CN.SH","name":"中芯国际"},
            {"symbol":"300750","market":"CN.SZ","name":"宁德时代"},
            {"symbol":"000001","market":"CN.SZ","name":"平安银行"}
          ]
        }"#;
        let bundle = parse_json(json, ImportKind::Bundle).unwrap();
        app.import_bundle(&bundle, "test").unwrap();

        let star = app
            .quote_board(Some("科创板"), None, None, 50, 0, None)
            .unwrap();
        assert_eq!(star.rows.len(), 1);
        assert_eq!(star.rows[0].symbol, "688981");

        let cyb = app
            .quote_board(Some("CHINEXT"), None, None, 50, 0, None)
            .unwrap();
        assert_eq!(cyb.rows.len(), 1);
        assert_eq!(cyb.rows[0].symbol, "300750");

        let cn = app
            .quote_board(Some("沪深京"), None, None, 50, 0, None)
            .unwrap();
        assert_eq!(cn.rows.len(), 4);
    }

    #[test]
    fn industry_filter_is_exact() {
        let app = temp_app();
        let json = r#"{
          "listings":[
            {"symbol":"600519","market":"CN.SH","name":"贵州茅台","industry":"食品饮料"},
            {"symbol":"000001","market":"CN.SZ","name":"平安银行","industry":"银行"}
          ],
          "quotes":[
            {"symbol":"600519","market":"CN.SH","ts":"2024-01-02","last":1488,"prev_close":1470},
            {"symbol":"000001","market":"CN.SZ","ts":"2024-01-02","last":10,"prev_close":10.2}
          ]
        }"#;
        let bundle = parse_json(json, ImportKind::Bundle).unwrap();
        app.import_bundle(&bundle, "test").unwrap();
        let page = app
            .quote_board(
                Some("CN"),
                None,
                Some("change_pct"),
                50,
                0,
                Some("食品饮料"),
            )
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].symbol, "600519");
        assert_eq!(page.industry, "食品饮料");
        assert!(page.empty_message.contains("食品饮料"));
        let miss = app
            .quote_board(Some("CN"), None, None, 50, 0, Some("半导体"))
            .unwrap();
        assert!(miss.empty);
        assert_eq!(miss.matched, 0);

        let pulse = app.market_pulse();
        assert!(pulse.ready);
        assert_eq!(pulse.tape.n_quoted, 2);
        assert_eq!(pulse.tape.n_up, 1);
        assert_eq!(pulse.tape.n_down, 1);
        assert_eq!(pulse.funds.status, pulse.capital.status);
        assert_eq!(pulse.capital.status, "pending");
        assert!(!pulse.headline.contains("看好"));
        assert!(!pulse.headline.contains("实时北向"));
    }

    #[test]
    fn missing_stock_is_not_found() {
        let app = temp_app();
        let key = app.parse_key("NOSUCH", Some("US")).unwrap();
        let err = app.show_stock(&key).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        assert!(!err.to_string().contains("not found: not found"));
    }

    #[test]
    fn watch_only_symbol_opens_without_listing() {
        let app = temp_app();
        let added = app
            .toggle_watch("MSFT", Some("US"), Some("Microsoft".into()))
            .unwrap();
        assert_eq!(added["watched"], true, "watchlist={:?}", app.watchlist());
        let key = app.parse_key("MSFT", Some("US")).unwrap();
        assert!(app.is_watched(&key), "watchlist={:?}", app.watchlist());
        let detail = app.show_stock(&key).unwrap();
        assert!(detail.watched);
        assert!(detail.empty.no_quote);
        assert_eq!(detail.quote.symbol, "MSFT");
        let off = app
            .toggle_watch("MSFT", Some("US"), Some("Microsoft".into()))
            .unwrap();
        assert_eq!(off["watched"], false);
        assert!(!app.is_watched(&key));
        let err = app.show_stock(&key).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn watch_toggle_roundtrip() {
        let app = temp_app();
        let json = r#"{"listings":[{"symbol":"600519","market":"CN.SH","name":"贵州茅台"}]}"#;
        app.import_bundle(&parse_json(json, ImportKind::Bundle).unwrap(), "test")
            .unwrap();
        let key = app.parse_key("600519", Some("CN.SH")).unwrap();
        let first = app.toggle_watch("600519", Some("CN.SH"), None).unwrap();
        let was = first["watched"].as_bool().unwrap();
        assert_eq!(app.is_watched(&key), was);
        let second = app
            .toggle_watch("600519", Some("CN.SH"), Some("贵州茅台".into()))
            .unwrap();
        assert_ne!(second["watched"], first["watched"]);
        let detail = app.show_stock(&key).unwrap();
        assert_eq!(detail.watched, second["watched"].as_bool().unwrap());
    }

    #[test]
    fn watch_set_is_idempotent() {
        let app = temp_app();
        app.set_watch("600519", Some("CN.SH"), Some("贵州茅台".into()), true)
            .unwrap();
        app.set_watch("600519", Some("CN.SH"), Some("贵州茅台".into()), true)
            .unwrap();
        assert_eq!(
            app.watchlist()
                .iter()
                .filter(|item| item.symbol == "600519" && item.market == "CN.SH")
                .count(),
            1
        );
        app.set_watch("600519", Some("CN.SH"), None, false).unwrap();
        app.set_watch("600519", Some("CN.SH"), None, false).unwrap();
        assert!(!app
            .watchlist()
            .iter()
            .any(|item| item.symbol == "600519" && item.market == "CN.SH"));
    }

    #[test]
    fn watch_board_lists_every_starred_symbol() {
        let app = temp_app();
        for w in app.watchlist() {
            let _ = app.toggle_watch(&w.symbol, Some(&w.market), None);
        }
        let json = r#"{"listings":[{"symbol":"600519","market":"CN.SH","name":"贵州茅台"}]}"#;
        app.import_bundle(&parse_json(json, ImportKind::Bundle).unwrap(), "test")
            .unwrap();
        app.toggle_watch("600519", Some("CN.SH"), Some("贵州茅台".into()))
            .unwrap();
        app.toggle_watch("688484", Some("CN.SH"), Some("南芯科技".into()))
            .unwrap();
        app.toggle_watch("000001", Some("CN.SZ"), Some("平安银行".into()))
            .unwrap();
        let page = app
            .quote_board(Some("WATCH"), None, None, 50, 0, None)
            .unwrap();
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.rows[0].symbol, "600519");
        assert_eq!(page.rows[1].symbol, "688484");
        assert_eq!(page.rows[2].symbol, "000001");
        assert_eq!(page.matched, 3);

        let page = app
            .quote_board(Some("WATCH"), None, None, 2, 2, None)
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].symbol, "000001");
        assert_eq!(page.matched, 3);
        assert!(page.truncated);
    }

    #[test]
    fn settings_board_updates_memory_without_restart() {
        let app = temp_app();
        app.set_default_board("CN.SZ").unwrap();
        assert_eq!(app.default_board(), "CN.SZ");
        let list = crate::invoke::invoke(&app, "watch.list", serde_json::json!({})).unwrap();
        assert_eq!(list["default_board"], "CN.SZ");
    }

    #[test]
    fn watch_persist_does_not_copy_env_token() {
        let dir = std::env::temp_dir().join(format!(
            "getrich-tok-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = AppConfig::default();
        cfg.data.tushare.token = "env-secret-token".into();
        let app = App::at_path(cfg, dir.join("t.db"));
        let prev = std::env::var("GETRICH_TUSHARE_TOKEN");
        std::env::set_var("GETRICH_TUSHARE_TOKEN", "env-secret-token");
        let result = app.toggle_watch("600519", Some("CN.SH"), Some("贵州茅台".into()));
        match prev {
            Ok(v) => std::env::set_var("GETRICH_TUSHARE_TOKEN", v),
            Err(_) => std::env::remove_var("GETRICH_TUSHARE_TOKEN"),
        }
        result.unwrap();
        let text = std::fs::read_to_string(dir.join("config.json")).unwrap();
        assert!(
            !text.contains("env-secret-token"),
            "env token leaked into config.json: {text}"
        );
    }

    #[test]
    fn weekly_bars_resample_from_daily() {
        let app = temp_app();
        let json = r#"{
          "listings":[{"symbol":"600519","market":"CN.SH","name":"贵州茅台"}],
          "bars":[
            {"symbol":"600519","market":"CN.SH","ts":"2024-01-02","open":10,"high":11,"low":9,"close":10.5,"volume":1},
            {"symbol":"600519","market":"CN.SH","ts":"2024-01-03","open":10.5,"high":12,"low":10,"close":11,"volume":1},
            {"symbol":"600519","market":"CN.SH","ts":"2024-01-08","open":11,"high":11.5,"low":10.5,"close":11.2,"volume":1}
          ]
        }"#;
        app.import_bundle(&parse_json(json, ImportKind::Bundle).unwrap(), "test")
            .unwrap();
        let key = app.parse_key("600519", Some("CN.SH")).unwrap();
        let week = app.list_bars(&key, Timeframe::W1, None, None, 100).unwrap();
        assert!(!week.empty);
        assert_eq!(week.rows.len(), 2);
        let month = app
            .list_bars(&key, Timeframe::Mo1, None, None, 100)
            .unwrap();
        assert_eq!(month.rows.len(), 1);
    }
}
