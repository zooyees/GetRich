//! Shared realtime pulse snapshot and background recomputation runtime.

use crate::db;
use crate::pulse::MarketPulse;
use crate::service::App;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommittedBatch {
    #[serde(default)]
    pub version: u64,
    pub source: String,
    pub requested: u32,
    pub accepted: u32,
    pub changed: u32,
    pub unchanged: u32,
    pub stale: u32,
    pub invalid: u32,
    pub missing_listing: u32,
    pub min_source_at: Option<String>,
    pub max_source_at: Option<String>,
    #[serde(default)]
    pub changed_ids: Vec<i64>,
    #[serde(default)]
    pub committed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotQuality {
    pub level: String,
    pub universe_total: u32,
    pub quoted_valid: u32,
    pub coverage: f64,
    pub fresh_ratio_3x: f64,
    pub source_lag_ms: Option<u64>,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub observed: u32,
    #[serde(default)]
    pub stale: u32,
    #[serde(default)]
    pub invalid: u32,
    #[serde(default)]
    pub missing: u32,
    #[serde(default)]
    pub universe_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub schema_version: u32,
    pub snapshot_version: u64,
    pub trade_date: String,
    pub phase: String,
    pub source_at: Option<String>,
    pub received_at: Option<String>,
    pub computed_at: String,
    pub quality: SnapshotQuality,
    pub pulse: MarketPulse,
    #[serde(default)]
    pub snapshot_revision: u64,
    #[serde(default)]
    pub config_version: u64,
    #[serde(default)]
    pub calendar_version: String,
}

#[derive(Debug, Default)]
pub struct RealtimeState {
    compute_lock: std::sync::Mutex<()>,
    heartbeat_ms: AtomicU64,
    started: AtomicBool,
    alive: AtomicBool,
    requested: AtomicBool,
    last_error: RwLock<Option<String>>,
    snapshot: RwLock<Option<Arc<MarketSnapshot>>>,
    compute_ms: AtomicU64,
    compute_failures: AtomicU64,
}

impl RealtimeState {
    pub fn snapshot(&self) -> Option<Arc<MarketSnapshot>> {
        let snapshot = self.snapshot.read().ok().and_then(|value| value.clone())?;
        let age = DateTime::parse_from_rfc3339(&snapshot.computed_at)
            .ok()
            .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds())
            .unwrap_or(i64::MAX);
        let error = self.last_error.read().ok().and_then(|e| e.clone());
        if age > 90
            || error.is_some()
            || (self.started.load(Ordering::SeqCst) && !self.alive.load(Ordering::SeqCst))
        {
            let mut expired = (*snapshot).clone();
            expired.quality.level = "poor".into();
            expired
                .quality
                .reasons
                .push("compute_unavailable_or_expired".into());
            expired.pulse.stale = true;
            expired.pulse.regime.confidence = "low".into();
            expired
                .pulse
                .regime
                .reasons
                .push(error.unwrap_or_else(|| "计算服务不可用或快照超过 90 秒未更新".into()));
            return Some(Arc::new(expired));
        }
        Some(snapshot)
    }

    pub fn request_refresh(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    pub(crate) fn refresh_from_store(
        &self,
        db: &crate::db::Database,
    ) -> Result<(), crate::db::DbError> {
        let conn = db.connect()?;
        let revision: u64 = conn.query_row(
            "SELECT COALESCE(MAX(revision),0) FROM pulse_snapshots",
            [],
            |r| r.get(0),
        )?;
        let current = self
            .snapshot
            .read()
            .ok()
            .and_then(|v| v.as_ref().map(|s| s.snapshot_revision))
            .unwrap_or(0);
        if revision > current {
            if let Some(snapshot) = crate::db::latest_snapshot(&conn)? {
                self.publish(snapshot);
            }
        }
        Ok(())
    }
    fn publish(&self, value: MarketSnapshot) -> Arc<MarketSnapshot> {
        let value = Arc::new(value);
        if let Ok(mut current) = self.snapshot.write() {
            if let Some(newer) = current
                .as_ref()
                .filter(|s| s.snapshot_revision > value.snapshot_revision)
            {
                return Arc::clone(newer);
            }
            *current = Some(Arc::clone(&value));
        }
        value
    }

    pub fn health(&self) -> serde_json::Value {
        let snapshot = self.snapshot();
        json!({
            "worker_started": self.started.load(Ordering::SeqCst),
            "worker_alive": self.alive.load(Ordering::SeqCst),
            "healthy": self.alive.load(Ordering::SeqCst) && self.last_error.read().map(|e| e.is_none()).unwrap_or(false)
                && Utc::now().timestamp_millis().saturating_sub(self.heartbeat_ms.load(Ordering::Relaxed) as i64)<90_000,
            "heartbeat_at_ms":self.heartbeat_ms.load(Ordering::Relaxed),
            "last_error": self.last_error.read().ok().and_then(|e| e.clone()),
            "snapshot_version": snapshot.as_ref().map(|s| s.snapshot_version),
            "computed_at": snapshot.as_ref().map(|s| s.computed_at.clone()),
            "quality": snapshot.as_ref().map(|s| s.quality.level.clone()),
            "coverage": snapshot.as_ref().map(|s| s.quality.coverage),
            "compute_ms": self.compute_ms.load(Ordering::Relaxed),
            "compute_failures": self.compute_failures.load(Ordering::Relaxed),
        })
    }
}

pub fn start(app: &Arc<App>) {
    if app
        .realtime
        .started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let app = Arc::clone(app);
    let worker_app = Arc::clone(&app);
    let spawned = thread::Builder::new()
        .name("getrich-pulse".into())
        .spawn(move || run(worker_app));
    if let Err(e) = spawned {
        app.realtime.started.store(false, Ordering::SeqCst);
        *app.realtime
            .last_error
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
    }
}

fn run(app: Arc<App>) {
    app.realtime.alive.store(true, Ordering::SeqCst);
    let subscription = app.events.subscribe_since(None);
    let mut last_periodic = Instant::now() - Duration::from_secs(60);
    app.realtime.request_refresh();
    loop {
        app.realtime
            .heartbeat_ms
            .store(Utc::now().timestamp_millis() as u64, Ordering::Relaxed);
        if app.ingest.stop.load(Ordering::SeqCst) {
            break;
        }
        let event_due = match subscription.receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(message) => matches!(
                message.get("type").and_then(|value| value.as_str()),
                Some("quote.updated" | "ingest.done" | "settings.changed")
            ),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                last_periodic.elapsed() >= Duration::from_secs(30)
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        if event_due
            || last_periodic.elapsed() >= Duration::from_secs(30)
            || app.realtime.requested.swap(false, Ordering::SeqCst)
        {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recompute(&app)))
                    .unwrap_or_else(|_| Err("市场计算遇到无效数据，已隔离本次失败".into()));
            match outcome {
                Ok(()) => {
                    last_periodic = Instant::now();
                    *app.realtime
                        .last_error
                        .write()
                        .unwrap_or_else(|e| e.into_inner()) = None;
                }
                Err(error) => {
                    last_periodic = Instant::now();
                    *app.realtime
                        .last_error
                        .write()
                        .unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
                    app.realtime
                        .compute_failures
                        .fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(error = %error, "realtime pulse recompute failed");
                }
            }
        }
    }
    app.realtime.alive.store(false, Ordering::SeqCst);
}

pub(crate) fn recompute(app: &App) -> Result<(), String> {
    let _guard = app
        .realtime
        .compute_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let started = Instant::now();
    let mut conn = app.db.connect().map_err(|e| e.to_string())?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let pulse = app
        .compute_market_pulse_on(&tx)
        .map_err(|e| e.to_string())?;
    let version = db::latest_batch_version(&tx).map_err(|e| e.to_string())?;
    let universe_total: u32 = tx
        .query_row(
            r#"SELECT COUNT(*) FROM listings
               WHERE market IN ('CN.SH','CN.SZ','CN.BJ')
                 AND status = 'active'
                 AND IFNULL(instrument_type, 'equity') = 'equity'"#,
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut snapshot = build_snapshot(pulse, version, universe_total, Utc::now());
    snapshot.config_version = tx
        .query_row(
            "SELECT value FROM app_state WHERE key='settings_version'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    snapshot.calendar_version = snapshot.pulse.session.calendar_source.clone();
    tx.execute(
        "UPDATE app_state SET value=value+1 WHERE key='snapshot_revision'",
        [],
    )
    .map_err(|e| e.to_string())?;
    snapshot.snapshot_revision = tx
        .query_row(
            "SELECT value FROM app_state WHERE key='snapshot_revision'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    db::persist_snapshot(&tx, &snapshot).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    let version = snapshot.snapshot_version;
    let computed_at = snapshot.computed_at.clone();
    app.realtime.publish(snapshot);
    *app.realtime
        .last_error
        .write()
        .unwrap_or_else(|e| e.into_inner()) = None;
    app.realtime
        .compute_ms
        .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
    app.events.emit(
        "snapshot.available",
        json!({"snapshot_version": version, "computed_at": computed_at}),
    );
    Ok(())
}

fn build_snapshot(
    pulse: MarketPulse,
    version: u64,
    universe_total: u32,
    computed: DateTime<Utc>,
) -> MarketSnapshot {
    let q = pulse.data_quality.clone();
    let quoted_valid = if q.observed > 0 {
        q.valid
    } else if pulse.stale {
        0
    } else {
        pulse.tape.n_quoted
    };
    let coverage = if universe_total == 0 {
        0.0
    } else {
        quoted_valid as f64 / universe_total as f64
    };
    let fresh_ratio_3x = if pulse.stale { 0.0 } else { coverage };
    let mut reasons = Vec::new();
    if universe_total == 0 {
        reasons.push("universe_empty".into());
    }
    if coverage < 0.80 {
        reasons.push("coverage_below_80pct".into());
    }
    if pulse.stale {
        reasons.push("quotes_stale".into());
    }
    if q.stale > 0 {
        reasons.push("some_quotes_stale".into());
    }
    if q.invalid > 0 {
        reasons.push("some_quotes_invalid".into());
    }
    if q.universe_scope == "imported_sample" {
        reasons.push("local_sample_not_full_market".into());
    }
    let level = if coverage >= 0.95 && fresh_ratio_3x >= 0.95 {
        "good"
    } else if coverage >= 0.80 {
        "degraded"
    } else {
        "poor"
    };
    let source_at = pulse.as_of.quotes_ts.clone();
    let received_at = pulse.as_of.quotes_ingested_at.clone();
    let source_lag_ms = source_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            computed
                .signed_duration_since(value.with_timezone(&Utc))
                .num_milliseconds()
                .max(0) as u64
        });
    MarketSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_version: version,
        trade_date: pulse.session.trade_date.clone(),
        phase: pulse.session.phase.as_str().into(),
        source_at,
        received_at,
        computed_at: computed.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        quality: SnapshotQuality {
            level: level.into(),
            universe_total,
            quoted_valid,
            coverage,
            fresh_ratio_3x,
            source_lag_ms,
            reasons,
            observed: q.observed,
            stale: q.stale,
            invalid: q.invalid,
            missing: q.missing,
            universe_scope: q.universe_scope,
        },
        pulse,
        snapshot_revision: 0,
        config_version: 0,
        calendar_version: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_instance_recompute_cannot_leave_readers_on_an_older_revision() {
        let root = std::env::temp_dir().join(format!(
            "getrich-snapshot-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let first = App::at_path(Default::default(), root.join("test.db"));
        first.migrate().unwrap();
        let second = App::at_path(Default::default(), root.join("test.db"));
        recompute(&first).unwrap();
        let before = first.realtime_snapshot().unwrap();
        recompute(&second).unwrap();
        let after = first.realtime_snapshot().unwrap();
        assert!(after.snapshot_revision > before.snapshot_revision);
        first.realtime.publish((*before).clone());
        assert_eq!(
            first.realtime_snapshot().unwrap().snapshot_revision,
            after.snapshot_revision
        );
        drop((first, second));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quality_degrades_with_low_coverage() {
        let mut pulse = crate::pulse::error_pulse("fixture");
        pulse.tape.n_quoted = 79;
        let snapshot = build_snapshot(pulse, 7, 100, Utc::now());
        assert_eq!(snapshot.snapshot_version, 7);
        assert_eq!(snapshot.quality.level, "poor");
        assert_eq!(snapshot.quality.coverage, 0.79);
    }

    #[test]
    fn stale_quotes_are_never_counted_as_valid_coverage() {
        let mut pulse = crate::pulse::error_pulse("fixture");
        pulse.tape.n_quoted = 100;
        pulse.stale = true;
        let snapshot = build_snapshot(pulse, 8, 100, Utc::now());
        assert_eq!(snapshot.quality.level, "poor");
        assert_eq!(snapshot.quality.quoted_valid, 0);
        assert_eq!(snapshot.quality.coverage, 0.0);
        assert!(snapshot
            .quality
            .reasons
            .iter()
            .any(|reason| reason == "quotes_stale"));
    }
}
