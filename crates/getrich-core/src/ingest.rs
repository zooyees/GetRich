//! Background rt_k poller. Does not block HTTP or GUI threads.

use crate::db;
use crate::error::AppError;
use crate::events::EventHub;
use crate::import;
use crate::service::App;
use crate::session;
use crate::tushare::{self};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub struct IngestState {
    pub stop: AtomicBool,
    started: AtomicBool,
    pub running: AtomicBool,
    pub in_session: AtomicBool,
    pub last_ok: Mutex<Option<String>>,
    pub last_err: Mutex<Option<String>>,
    pub last_api: Mutex<Option<String>>,
}

impl Default for IngestState {
    fn default() -> Self {
        Self::new()
    }
}

impl IngestState {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
            running: AtomicBool::new(false),
            in_session: AtomicBool::new(false),
            last_ok: Mutex::new(None),
            last_err: Mutex::new(None),
            last_api: Mutex::new(None),
        }
    }

    pub fn started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub fn worker_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn in_session(&self) -> bool {
        self.in_session.load(Ordering::SeqCst)
    }
}

pub fn start(app: &Arc<App>) {
    if app
        .ingest
        .started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let app = Arc::clone(app);
    let _ = thread::Builder::new()
        .name("getrich-ingest".into())
        .spawn(move || run_loop(&app));
}

fn run_loop(app: &Arc<App>) {
    let mut backoff = Duration::from_secs(1);
    let mut last_open: Option<String> = None;
    let mut last_cal_check = std::time::Instant::now() - Duration::from_secs(3_600);
    loop {
        if app.ingest.stop.load(Ordering::SeqCst) {
            app.ingest.running.store(false, Ordering::SeqCst);
            app.ingest.in_session.store(false, Ordering::SeqCst);
            break;
        }
        let cfg = app.tushare_cfg();
        let enabled = cfg.enabled && !cfg.token.trim().is_empty();
        if !enabled {
            app.ingest.running.store(false, Ordering::SeqCst);
            app.ingest.in_session.store(false, Ordering::SeqCst);
            thread::sleep(Duration::from_secs(5));
            continue;
        }
        app.ingest.running.store(true, Ordering::SeqCst);
        let in_window = session::in_poll_window();
        app.ingest.in_session.store(in_window, Ordering::SeqCst);
        if !in_window {
            thread::sleep(Duration::from_secs(20));
            continue;
        }

        let today = session::yyyymmdd(session::shanghai_today());
        if last_cal_check.elapsed() > Duration::from_secs(30 * 60) || last_open.is_none() {
            if let Ok(client) = app.tushare_client() {
                match client.last_open_day() {
                    Ok(d) => {
                        last_open = Some(d.clone());
                        last_cal_check = std::time::Instant::now();
                        if let Ok(conn) = app.db.connect() {
                            let as_of = session::iso_date(session::shanghai_today());
                            let _ = db::upsert_vendor(
                                &conn,
                                "trade_cal",
                                &as_of,
                                Some("t0"),
                                "ok",
                                d.trim(),
                            );
                        }
                    }
                    Err(e) => tracing::debug!(error = %e, "trade_cal for ingest window skipped"),
                }
            }
        }
        // Unknown calendar or holiday: do not treat as an open session.
        if last_open.as_deref() != Some(today.as_str()) {
            app.ingest.in_session.store(false, Ordering::SeqCst);
            thread::sleep(Duration::from_secs(30));
            continue;
        }

        match poll_once(app) {
            Ok(()) => {
                backoff = Duration::from_secs(1);
                let jitter = cfg.poll_ms / 10;
                let extra = if jitter == 0 { 0 } else { now_jitter(jitter) };
                thread::sleep(Duration::from_millis(cfg.poll_ms.max(1_000) + extra));
            }
            Err(e) => {
                tracing::warn!(error = %e, "ingest rt_k failed");
                if let Ok(mut g) = app.ingest.last_err.lock() {
                    *g = Some(e.to_string());
                }
                mark_hot_error(app, &e);
                thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

fn now_jitter(max: u64) -> u64 {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis() as u64)
        .unwrap_or(0);
    n % (max.max(1))
}

fn poll_once(app: &App) -> Result<(), AppError> {
    let cfg = app.tushare_cfg();
    let client = app.tushare_client()?;
    let codes = rt_k_codes(app, &cfg);
    let mut quotes = Vec::new();
    let mut bars = Vec::new();
    for code in codes {
        if app.ingest.stop.load(Ordering::SeqCst) {
            break;
        }
        let table = client.rt_k_table(&code)?;
        let mapped = tushare::map_rt_k(&table);
        quotes.extend(mapped.quotes);
        bars.extend(mapped.bars);
    }
    if quotes.is_empty() && bars.is_empty() {
        return Ok(());
    }
    let batch = app.cfg.system.db_commit_batch_size;
    let report = import::apply_quotes_hot(&app.db, &quotes, &bars, "tushare", batch)?;
    mark_hot_success(app, &report)?;
    if let Ok(mut g) = app.ingest.last_ok.lock() {
        *g = Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    }
    if let Ok(mut g) = app.ingest.last_err.lock() {
        *g = None;
    }
    if let Ok(mut g) = app.ingest.last_api.lock() {
        *g = Some("rt_k".into());
    }
    emit_quote_updated(&app.events, &report);
    Ok(())
}

fn rt_k_codes(app: &App, cfg: &crate::config::TushareConfig) -> Vec<String> {
    if cfg.universe.trim().eq_ignore_ascii_case("watchlist") {
        let mut out = Vec::new();
        for k in app.watch_keys() {
            if let Some(code) = tushare::key_to_ts_code(&k) {
                out.push(code);
            }
        }
        return chunk_codes(out, 50);
    }
    cfg.rt_k_codes
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn chunk_codes(codes: Vec<String>, n: usize) -> Vec<String> {
    if codes.is_empty() {
        return codes;
    }
    codes.chunks(n.max(1)).map(|c| c.join(",")).collect()
}

fn mark_hot_success(app: &App, report: &import::HotWriteReport) -> Result<(), AppError> {
    if report.accepted == 0 {
        return Ok(());
    }
    let conn = app.db.connect()?;
    let id = db::ensure_watermark_listing(&conn)?;
    db::mark_success(&conn, id, "quotes", "tushare", report.watermark.as_deref())?;
    db::mark_success(
        &conn,
        id,
        "bars.1d",
        "tushare",
        Some(&session::iso_date(session::shanghai_today())),
    )?;
    Ok(())
}

fn mark_hot_error(app: &App, err: &AppError) {
    if let Ok(conn) = app.db.connect() {
        if let Ok(id) = db::ensure_watermark_listing(&conn) {
            let _ = db::mark_error(&conn, id, "quotes", "tushare", &err.to_string());
        }
    }
}

pub fn emit_quote_updated(events: &EventHub, report: &import::HotWriteReport) {
    events.emit(
        "quote.updated",
        json!({
            "quotes_upserted": report.quotes_upserted,
            "bars_upserted": report.bars_upserted,
            "skipped_missing": report.skipped_missing,
            "batch_version": report.batch_version,
            "accepted": report.accepted,
            "changed": report.changed,
            "unchanged": report.unchanged,
            "stale": report.stale,
            "invalid": report.invalid,
            "missing_listing": report.missing_listing,
            "watermark": report.watermark,
        }),
    );
}

pub fn emit_ingest_done(
    events: &EventHub,
    kind: &str,
    quotes: u32,
    bars: u32,
    instruments: u32,
    status: &str,
) {
    events.emit(
        "ingest.done",
        json!({
            "kind": kind,
            "quotes_upserted": quotes,
            "bars_upserted": bars,
            "instruments_upserted": instruments,
            "status": status,
        }),
    );
}
