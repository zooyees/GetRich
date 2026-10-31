//! Background rt_k poller. Does not block HTTP or GUI threads.

use crate::db;
use crate::error::AppError;
use crate::events::EventHub;
use crate::import;
use crate::service::App;
use crate::session;
use crate::tushare::{self};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub struct IngestState {
    pub stop: AtomicBool,
    pub phase: Mutex<String>,
    pub last_attempt: Mutex<Option<String>>,
    pub duration_ms: AtomicU64,
    pub failures: AtomicU64,
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
            phase: Mutex::new("starting".into()),
            last_attempt: Mutex::new(None),
            duration_ms: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            started: AtomicBool::new(false),
            running: AtomicBool::new(false),
            in_session: AtomicBool::new(false),
            last_ok: Mutex::new(None),
            last_err: Mutex::new(None),
            last_api: Mutex::new(None),
        }
    }

    fn set_phase(&self, phase: &str) {
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = phase.into();
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
    let worker = Arc::clone(app);
    if let Err(error) = thread::Builder::new()
        .name("getrich-ingest".into())
        .spawn(move || run_loop(&worker))
    {
        app.ingest.started.store(false, Ordering::SeqCst);
        app.ingest.set_phase("error");
        *app.ingest
            .last_err
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
    }
}

fn sleep_interruptible(app: &App, cfg: &crate::config::TushareConfig, duration: Duration) {
    let until = std::time::Instant::now() + duration;
    while std::time::Instant::now() < until {
        if app.ingest.stop.load(Ordering::SeqCst) || app.tushare_cfg() != *cfg {
            break;
        }
        thread::sleep(
            until
                .saturating_duration_since(std::time::Instant::now())
                .min(Duration::from_secs(1)),
        );
    }
}
fn run_loop(app: &Arc<App>) {
    let mut backoff = Duration::from_secs(1);
    let mut last_open = None::<String>;
    let mut checked_date = None::<String>;
    let mut confirmed_date = None::<String>;
    let mut last_cal_check = std::time::Instant::now() - Duration::from_secs(3600);
    loop {
        if app.ingest.stop.load(Ordering::SeqCst) {
            break;
        }
        let cfg = app.tushare_cfg();
        let enabled = cfg.enabled && !cfg.token.trim().is_empty();
        app.ingest.running.store(enabled, Ordering::SeqCst);
        if !enabled {
            app.ingest.in_session.store(false, Ordering::SeqCst);
            app.ingest.set_phase(if cfg.enabled {
                "unconfigured"
            } else {
                "disabled"
            });
            sleep_interruptible(app, &cfg, Duration::from_secs(5));
            continue;
        }
        if !session::in_poll_window() {
            app.ingest.in_session.store(false, Ordering::SeqCst);
            app.ingest.set_phase("outside_session");
            sleep_interruptible(app, &cfg, Duration::from_secs(20));
            continue;
        }
        let today = session::yyyymmdd(session::shanghai_today());
        if checked_date.as_deref() != Some(&today)
            || last_cal_check.elapsed() >= Duration::from_secs(1800)
        {
            app.ingest.set_phase("checking_calendar");
            checked_date = Some(today.clone());
            last_cal_check = std::time::Instant::now();
            match app
                .tushare_client()
                .and_then(|client| client.poll_open_day())
            {
                Ok(day) => {
                    last_open = Some(day.clone());
                    confirmed_date = Some(today.clone());
                    if let Ok(conn) = app.db.connect() {
                        let _ = db::upsert_vendor(
                            &conn,
                            "trade_cal",
                            &session::iso_date(session::shanghai_today()),
                            Some("t0"),
                            "ok",
                            &day,
                        );
                    }
                }
                Err(error) => {
                    // Calendar failure must not permanently disable an independently authorized RT API.
                    // Only an actual source timestamp for today may confirm the fallback session.
                    tracing::warn!(%error, "实时采集日历暂不可用，将用有效当日报价校验");
                    if let Ok(conn) = app.db.connect() {
                        if let Ok(Some(cached)) = db::latest_vendor(&conn, "trade_cal") {
                            if cached.status == "ok"
                                && cached.as_of == session::iso_date(session::shanghai_today())
                            {
                                last_open = cached.payload_json.map(|s| s.trim_matches('"').into());
                                confirmed_date = Some(today.clone());
                            }
                        }
                    }
                }
            }
        }
        if confirmed_date.as_deref() == Some(&today) && last_open.as_deref() != Some(&today) {
            app.ingest.in_session.store(false, Ordering::SeqCst);
            app.ingest.set_phase("closed_day");
            sleep_interruptible(app, &cfg, Duration::from_secs(30));
            continue;
        }
        if confirmed_date.as_deref() == Some(&today) {
            app.ingest.in_session.store(true, Ordering::SeqCst);
        }
        let started = std::time::Instant::now();
        *app.ingest
            .last_attempt
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(chrono::Utc::now().to_rfc3339());
        app.ingest.set_phase("fetching");
        match poll_once(app) {
            Ok(()) => {
                app.ingest.in_session.store(true, Ordering::SeqCst);
                app.ingest.failures.store(0, Ordering::Relaxed);
                app.ingest
                    .set_phase(if confirmed_date.as_deref() == Some(&today) {
                        "live"
                    } else {
                        "live_quote_verified"
                    });
                backoff = Duration::from_secs(1);
                let interval = effective_interval(app, &cfg);
                let extra = now_jitter((interval / 10).max(1));
                app.ingest
                    .duration_ms
                    .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                sleep_interruptible(app, &cfg, poll_delay(interval, extra, started.elapsed()));
            }
            Err(error) => {
                let text = error.to_string();
                tracing::warn!(%error, "ingest rt_k failed");
                *app.ingest
                    .last_err
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
                app.ingest.failures.fetch_add(1, Ordering::Relaxed);
                app.ingest
                    .duration_ms
                    .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                app.ingest.set_phase(
                    if tushare::permission_denied(&text) || text.contains("接口权限不可用") {
                        "permission_denied"
                    } else if tushare::is_rate_limited(&error) {
                        "rate_limited"
                    } else {
                        "error"
                    },
                );
                mark_hot_error(app, &error);
                let retry = app
                    .tushare_client()
                    .and_then(|c| c.retry_after_ms("rt_k"))
                    .unwrap_or(0);
                sleep_interruptible(app, &cfg, backoff.max(Duration::from_millis(retry)));
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
    app.ingest.running.store(false, Ordering::SeqCst);
    app.ingest.in_session.store(false, Ordering::SeqCst);
    app.ingest.set_phase("stopped");
}

fn poll_delay(interval_ms: u64, jitter_ms: u64, elapsed: Duration) -> Duration {
    Duration::from_millis(interval_ms.saturating_add(jitter_ms)).saturating_sub(elapsed)
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
    if app.ingest.stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let bundle = client.live_board_bundle(&codes)?;
    let quotes = bundle.quotes;
    let bars = bundle.bars;
    let batch = app.cfg.system.db_commit_batch_size;
    let report = import::apply_quotes_hot(&app.db, &quotes, &bars, "tushare", batch)?;
    if report.accepted == 0 {
        return Err(AppError::Message(
            "实时接口已响应，但没有可入库的有效股票报价；请先同步股票列表并检查覆盖范围".into(),
        ));
    }
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
    tushare::quote_batches(&cfg.rt_k_codes)
}

fn chunk_codes(codes: Vec<String>, n: usize) -> Vec<String> {
    codes.chunks(n.max(1)).map(|c| c.join(",")).collect()
}
pub(crate) fn effective_interval(app: &App, cfg: &crate::config::TushareConfig) -> u64 {
    tushare::effective_poll_ms(cfg.poll_ms, rt_k_codes(app, cfg).len(), cfg.rate_per_min)
}

fn mark_hot_success(app: &App, report: &import::HotWriteReport) -> Result<(), AppError> {
    if report.accepted == 0 {
        return Ok(());
    }
    let conn = app.db.connect()?;
    let id = db::ensure_watermark_listing(&conn)?;
    db::mark_success(
        &conn,
        id,
        "quotes",
        "tushare.rt_k",
        report.watermark.as_deref(),
    )?;
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

#[cfg(test)]
mod tests;

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
