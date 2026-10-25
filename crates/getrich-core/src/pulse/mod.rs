//! Market pulse: session → tape → regime → headline. Read path never calls the network.

mod headline;
mod limit;
mod regime;
mod tape;

use crate::config::{PulseConfig, WatchItem};
use crate::db::{self, PulseScanRow};
use crate::error::AppError;
use crate::session::{self, SessionPhase};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub use headline::contains_forbidden;
pub use limit::limit_pct;

const INDEX_KEYS: &[(&str, &str, &str, &str)] = &[
    ("000001", "CN.SH", "000001.SH", "上证"),
    ("399001", "CN.SZ", "399001.SZ", "深成"),
    ("399006", "CN.SZ", "399006.SZ", "创业"),
    ("000688", "CN.SH", "000688.SH", "科创50"),
    ("000300", "CN.SH", "000300.SH", "沪深300"),
    ("000852", "CN.SH", "000852.SH", "中证1000"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPulse {
    pub generated_at: String,
    pub ready: bool,
    pub message: String,
    pub headline: String,
    pub session: SessionInfo,
    pub as_of: AsOf,
    pub regime: Regime,
    pub tape: Tape,
    #[serde(default)]
    pub indices: Vec<IndexPrint>,
    #[serde(default)]
    pub styles: Vec<StyleSpread>,
    pub sectors: PulseSection<SectorMove>,
    pub extremes: Extremes,
    pub capital: PulseSection<FundFlow>,
    pub funds: PulseSection<FundFlow>,
    pub news: PulseSection<NewsItem>,
    pub watchlist: WatchPulse,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub data_quality: DataQuality,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataQuality {
    pub observed: u32,
    pub valid: u32,
    pub stale: u32,
    pub invalid: u32,
    pub missing: u32,
    pub universe_total: u32,
    pub universe_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub trade_date: String,
    pub phase: SessionPhase,
    pub is_open: bool,
    pub frozen: bool,
    pub calendar_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AsOf {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quotes_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_trade_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quotes_ingested_at: Option<String>,
    #[serde(default)]
    pub vendor: Vec<VendorStamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorStamp {
    pub dataset: String,
    pub vintage: Option<String>,
    pub as_of: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regime {
    pub code: String,
    pub label: String,
    pub confidence: String,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub subtypes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tape {
    pub universe: String,
    pub n_quoted: u32,
    pub n_up: u32,
    pub n_down: u32,
    pub n_flat: u32,
    pub advance_ratio: Option<f64>,
    pub median_pct: Option<f64>,
    pub mean_pct: Option<f64>,
    pub amount_weighted_pct: Option<f64>,
    pub amount: f64,
    pub amount_vs_20d: Option<f64>,
    pub up_amount_ratio: Option<f64>,
    pub n_limit_up_est: u32,
    pub n_limit_down_est: u32,
    pub top5_amount_share: Option<f64>,
    pub max_limit_times: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexPrint {
    pub ts_code: String,
    pub name: String,
    pub last: Option<f64>,
    pub change_pct: f64,
    pub amount: Option<f64>,
    pub trade_time: Option<String>,
    pub vintage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleSpread {
    pub name: String,
    pub left_pct: f64,
    pub right_pct: f64,
    pub spread_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseSection<T> {
    pub status: String,
    pub note: String,
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundFlow {
    pub name: String,
    pub value: Option<f64>,
    pub unit: String,
    pub direction: String,
    pub vintage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectorMove {
    pub name: String,
    pub change_pct: Option<f64>,
    pub median_pct: Option<f64>,
    pub amount: f64,
    pub amount_share: Option<f64>,
    pub n: u32,
    pub leaders: Vec<Leader>,
    pub rank: i32,
    pub rank_change: Option<i32>,
    pub direction: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unweighted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leader {
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub ts: Option<String>,
    pub source: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub industries: Vec<String>,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Extremes {
    pub n_limit_up_est: u32,
    pub n_limit_down_est: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_limit_up_official: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_limit_down_official: Option<u32>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchPulse {
    pub n: u32,
    pub vs_median_pct: Option<f64>,
    pub leaders: Vec<WatchName>,
    pub laggards: Vec<WatchName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchName {
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub pct: f64,
}

fn pending<T>(note: &str) -> PulseSection<T> {
    PulseSection {
        status: "pending".into(),
        note: note.into(),
        items: Vec::new(),
    }
}

pub fn error_pulse(msg: &str) -> MarketPulse {
    let now = Utc::now();
    let session = session_info(now, true, "fallback", None);
    let regime = Regime {
        code: "unknown".into(),
        label: "样本不足".into(),
        confidence: "low".into(),
        reasons: vec![msg.into()],
        subtypes: Vec::new(),
    };
    let tape = Tape {
        universe: "CN".into(),
        ..Tape::default()
    };
    let headline = headline::headline(&session, &regime, &tape, &[], &[], &[]);
    MarketPulse {
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ready: false,
        message: headline.clone(),
        headline,
        session,
        as_of: AsOf::default(),
        regime,
        tape,
        indices: Vec::new(),
        styles: Vec::new(),
        sectors: pending(msg),
        extremes: Extremes::default(),
        capital: pending("北向/两融为 T+1，尚未接入。"),
        funds: pending("北向/两融为 T+1，尚未接入。"),
        news: pending("快讯权限未接入。"),
        watchlist: WatchPulse::default(),
        stale: false,
        data_quality: DataQuality::default(),
    }
}

fn session_info(
    now: DateTime<Utc>,
    trading: bool,
    cal: &str,
    last_open: Option<&str>,
) -> SessionInfo {
    let today = session::iso_date(session::shanghai_date_at(now));
    let trade_date = if trading {
        today
    } else if let Some(d) = last_open.filter(|s| !s.is_empty()) {
        d.to_string()
    } else {
        session::iso_date(session::previous_weekday(session::shanghai_date_at(now)))
    };
    let phase = session::phase_at(now, trading);
    SessionInfo {
        trade_date,
        phase,
        is_open: phase.is_open(),
        frozen: phase.frozen(),
        calendar_source: cal.into(),
    }
}

pub fn compute(
    conn: &Connection,
    cfg: &PulseConfig,
    poll_ms: u64,
    watch: &[WatchItem],
    now: DateTime<Utc>,
) -> Result<MarketPulse, AppError> {
    let (trading, cal_src, last_open) = trading_day(conn, now)?;
    let session = session_info(now, trading, &cal_src, last_open.as_deref());

    let rows = db::pulse_tape_scan(conn)?;
    compute_from_rows(conn, cfg, poll_ms, watch, now, session, rows)
}

fn trading_day(
    conn: &Connection,
    now: DateTime<Utc>,
) -> Result<(bool, String, Option<String>), AppError> {
    let today = session::iso_date(session::shanghai_date_at(now));
    if let Ok(Some(v)) = db::latest_vendor(conn, "trade_cal") {
        if let Some(last) = v.payload_json.as_deref() {
            let last_iso = crate::tushare::yyyymmdd_to_iso(last.trim());
            if crate::validation::date(&last_iso).is_ok()
                && last_iso <= today
                && v.as_of == today
                && v.status == "ok"
                && session::calendar_date_of(&v.fetched_at)
                    .is_some_and(|d| session::iso_date(d) == today)
            {
                let trading = last_iso == today;
                return Ok((trading, "trade_cal".into(), Some(last_iso)));
            }
        }
    }
    Ok((
        session::is_weekday_date(session::shanghai_date_at(now)),
        "fallback".into(),
        None,
    ))
}

fn eligible_rows(
    conn: &Connection,
    cfg: &PulseConfig,
    poll_ms: u64,
    now: DateTime<Utc>,
    session: &SessionInfo,
    rows: Vec<PulseScanRow>,
) -> Result<(DataQuality, Vec<PulseScanRow>), AppError> {
    let observed = rows.len() as u32;
    let universe_total:u32=conn.query_row("SELECT COUNT(*) FROM listings WHERE market IN ('CN.SH','CN.SZ','CN.BJ') AND status='active' AND instrument_type='equity'",[],|r|r.get(0)).map_err(db::DbError::from)?;
    let max_age = poll_ms
        .saturating_mul(cfg.stale_poll_mult as u64)
        .max(15_000) as i64;
    let mut valid_rows = Vec::new();
    let mut historical = Vec::new();
    let mut invalid = 0;
    let mut stale = 0;
    for row in rows {
        if !row.last.is_finite()
            || row.last <= 0.0
            || row.volume.is_some_and(|v| !v.is_finite() || v < 0.0)
            || row.turnover.is_some_and(|v| !v.is_finite() || v < 0.0)
            || row.prev_close.is_some_and(|v| !v.is_finite() || v <= 0.0)
            || row.change_pct.is_some_and(|v| !v.is_finite())
        {
            invalid += 1;
            continue;
        }
        let source = row.ts.as_deref().and_then(|s| session::parse_cn_ts(s).ok());
        let received = row
            .ingested_at
            .as_deref()
            .and_then(|s| session::parse_cn_ts(s).ok());
        let current = source.is_some_and(|t| {
            session::iso_date(session::shanghai_date_at(t)) == session.trade_date
                && t <= now + chrono::Duration::minutes(1)
                && (!session.is_open || (now - t).num_milliseconds() <= max_age)
        }) && (!session.is_open
            || received.is_some_and(|t| {
                t <= now + chrono::Duration::minutes(1) && (now - t).num_milliseconds() <= max_age
            }));
        if current {
            valid_rows.push(row);
        } else {
            stale += 1;
            historical.push(row);
        }
    }
    let valid = valid_rows.len() as u32;
    let quality = DataQuality {
        observed,
        valid,
        stale,
        invalid,
        missing: universe_total.saturating_sub(observed),
        universe_total,
        universe_scope: "imported_sample".into(),
    };
    if valid > 0 {
        return Ok((quality, valid_rows));
    }
    let latest = historical
        .iter()
        .filter_map(|r| r.ts.as_deref())
        .filter_map(|s| session::parse_cn_ts(s).ok())
        .map(session::shanghai_date_at)
        .max();
    Ok((
        quality,
        historical
            .into_iter()
            .filter(|r| {
                r.ts.as_deref()
                    .and_then(|s| session::parse_cn_ts(s).ok())
                    .map(session::shanghai_date_at)
                    == latest
            })
            .collect(),
    ))
}

pub(crate) fn matching_ids(
    conn: &Connection,
    cfg: &PulseConfig,
    poll_ms: u64,
    filter: &str,
) -> Result<std::collections::HashSet<i64>, AppError> {
    let now = Utc::now();
    let (trading, source, last) = trading_day(conn, now)?;
    let session = session_info(now, trading, &source, last.as_deref());
    let (_, rows) = eligible_rows(
        conn,
        cfg,
        poll_ms,
        now,
        &session,
        db::pulse_tape_scan(conn)?,
    )?;
    Ok(tape::classify(rows, &session.trade_date, cfg)
        .into_iter()
        .filter(|c| match filter {
            "up" => c.pct.is_some_and(|p| p > cfg.flat_eps_pct),
            "down" => c.pct.is_some_and(|p| p < -cfg.flat_eps_pct),
            "flat" => c.pct.is_none_or(|p| p.abs() <= cfg.flat_eps_pct),
            "limit_up" => {
                !c.halted
                    && c.limit_pct
                        .zip(c.pct)
                        .is_some_and(|(l, p)| p >= l - cfg.limit_eps_pct)
            }
            "limit_down" => {
                !c.halted
                    && c.limit_pct
                        .zip(c.pct)
                        .is_some_and(|(l, p)| p <= -(l - cfg.limit_eps_pct))
            }
            _ => true,
        })
        .map(|c| c.row.id)
        .collect())
}

fn compute_from_rows(
    conn: &Connection,
    cfg: &PulseConfig,
    poll_ms: u64,
    watch: &[WatchItem],
    now: DateTime<Utc>,
    session: SessionInfo,
    rows: Vec<PulseScanRow>,
) -> Result<MarketPulse, AppError> {
    let (data_quality, rows) = eligible_rows(conn, cfg, poll_ms, now, &session, rows)?;
    let valid = data_quality.valid;
    let observed = data_quality.observed;
    let universe_total = data_quality.universe_total;
    let quotes_ts = rows.iter().filter_map(|r| r.ts.clone()).max();
    let quotes_ingested = rows.iter().filter_map(|r| r.ingested_at.clone()).max();
    let quote_trade_date = quotes_ts
        .as_deref()
        .and_then(session::calendar_date_of)
        .map(session::iso_date);
    let wrong_trade_date = quote_trade_date
        .as_deref()
        .map(|date| date != session.trade_date)
        .unwrap_or(false);
    let delayed_during_session = session.is_open
        && quotes_ingested
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                let age = now
                    .signed_duration_since(t.with_timezone(&Utc))
                    .num_milliseconds()
                    .max(0) as u64;
                age > poll_ms.saturating_mul(cfg.stale_poll_mult as u64)
            })
            .unwrap_or(true);
    let stale = valid == 0 && (wrong_trade_date || delayed_during_session || observed > 0);
    let classified = tape::classify(rows, &session.trade_date, cfg);
    let vs20 = amount_vs_20d(conn, &session.trade_date)?;
    let (tape, mut sectors_all, unlabeled_heavy, delist) = tape::build_tape(&classified, cfg, vs20);

    let anchor_phase = match session.phase {
        SessionPhase::Morning => Some("open"),
        SessionPhase::Afternoon | SessionPhase::Close => Some("lunch"),
        _ => None,
    };
    if let Some(ph) = anchor_phase {
        if let Ok(Some(a)) = db::get_anchor(conn, &session.trade_date, ph) {
            if let Some(js) = a.payload_json {
                if let Ok(prev) = serde_json::from_str::<MarketPulse>(&js) {
                    let ranks: Vec<(String, i32)> = prev
                        .sectors
                        .items
                        .iter()
                        .map(|s| (s.name.clone(), s.rank))
                        .collect();
                    tape::apply_rank_change(&mut sectors_all, &ranks);
                }
            }
        }
    }

    let board_sectors = tape::pick_sector_board(&sectors_all, cfg.sector_top_n);
    let indices = load_indices(conn);
    let styles = style_spread(&indices);

    let quotes_before_open = session.phase.is_open()
        && quotes_ingested
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                let sh = session::shanghai_from_utc(t.with_timezone(&Utc));
                sh.time() < chrono::NaiveTime::from_hms_opt(9, 30, 0).unwrap()
                    && sh.date_naive() == session::shanghai_date_at(now)
            })
            .unwrap_or(false);

    let mut regime = regime::classify(
        &tape,
        cfg,
        &indices,
        session.calendar_source == "fallback",
        quotes_before_open,
        unlabeled_heavy,
        board_sectors.iter().any(|s| s.name != "未分类"),
    );
    if regime.code == "rotation" {
        if let Some(r) = regime::rotation_reason(&board_sectors) {
            regime.reasons.insert(0, r);
        }
    }
    if regime.confidence == "high" && unlabeled_heavy {
        regime.confidence = "medium".into();
    }
    if data_quality.universe_scope == "imported_sample"
        || valid < universe_total
        || session.calendar_source != "trade_cal"
    {
        regime.confidence = "low".into();
        regime.reasons.push(format!(
            "已导入样本：有效 {valid}/{universe_total}，不代表全市场。"
        ));
    }
    if stale {
        let date = quote_trade_date.as_deref().unwrap_or("未知日期");
        regime = Regime {
            code: "stale".into(),
            label: "历史数据".into(),
            confidence: "low".into(),
            reasons: vec![format!(
                "行情日期 {date}，当前交易日 {}；该截面不能视为当前市场状态。",
                session.trade_date
            )],
            subtypes: Vec::new(),
        };
    }

    let capital = pending::<FundFlow>(
        "北向/两融为 T+1，暂未提供资金流数据，后续接入将标注数据日期与发布延迟。",
    );
    let news = pending::<NewsItem>("快讯权限未接入。");
    let watchlist = tape::watch_pulse(&classified, watch, tape.median_pct, cfg.watch_min);
    let mut notes = Vec::new();
    if delist {
        notes.push("含退市整理，阈值未细分".into());
    }
    let extremes = Extremes {
        n_limit_up_est: tape.n_limit_up_est,
        n_limit_down_est: tape.n_limit_down_est,
        n_limit_up_official: None,
        n_limit_down_official: None,
        notes,
    };
    let mut headline = headline::headline(
        &session,
        &regime,
        &tape,
        &board_sectors,
        &indices,
        &capital.items,
    );
    if stale {
        headline = format!(
            "历史快照 {} · {}",
            quote_trade_date.as_deref().unwrap_or("日期未知"),
            headline
        );
    }

    let sectors = if board_sectors.is_empty() {
        PulseSection {
            status: if tape.n_quoted == 0 { "pending" } else { "ok" }.into(),
            note: if tape.n_quoted == 0 {
                "无报价，无法分组行业".into()
            } else {
                "行业字段不足".into()
            },
            items: Vec::new(),
        }
    } else {
        PulseSection {
            status: "ok".into(),
            note: String::new(),
            items: board_sectors,
        }
    };

    let pulse = MarketPulse {
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ready: tape.n_quoted > 0,
        message: headline.clone(),
        headline,
        session: session.clone(),
        as_of: AsOf {
            quotes_ts,
            quote_trade_date,
            quotes_ingested_at: quotes_ingested,
            vendor: Vec::new(),
        },
        regime,
        tape: tape.clone(),
        indices,
        styles,
        sectors,
        extremes,
        funds: capital.clone(),
        capital,
        news,
        watchlist,
        stale,
        data_quality,
    };

    if !stale && session.phase == SessionPhase::Open {
        if let Ok(js) = serde_json::to_string(&pulse) {
            let _ = db::replace_anchor(
                conn,
                &session.trade_date,
                session.phase.as_str(),
                Some(tape.amount),
                tape.median_pct,
                &js,
            );
        }
    } else if !stale && session.phase.persist_anchor() {
        if let Ok(js) = serde_json::to_string(&pulse) {
            let _ = db::insert_anchor_if_missing(
                conn,
                &session.trade_date,
                session.phase.as_str(),
                Some(tape.amount),
                tape.median_pct,
                &js,
            );
        }
    }
    Ok(pulse)
}

/// Sparse 日 K 回填不能当全市场 20 日额。低于该阈值的交易日丢弃。
const AMOUNT_HIST_FLOOR: f64 = 1.0e11;

fn usable_amount_hist(vals: impl IntoIterator<Item = f64>) -> Vec<f64> {
    vals.into_iter()
        .filter(|a| *a >= AMOUNT_HIST_FLOOR)
        .take(20)
        .collect()
}

fn amount_vs_20d(conn: &Connection, trade_date: &str) -> Result<Option<f64>, AppError> {
    let mut hist = usable_amount_hist(db::list_post_amounts(conn, 40)?);
    if hist.len() < 20 {
        if let Ok(Some(v)) = db::latest_vendor(conn, "amount_hist") {
            if let Some(js) = v.payload_json {
                if let Ok(vals) = serde_json::from_str::<Vec<f64>>(&js) {
                    hist = usable_amount_hist(vals);
                }
            }
        }
    }
    if hist.len() < 20 {
        let daily = db::daily_market_amounts(conn, 40)?;
        hist = usable_amount_hist(
            daily
                .into_iter()
                .filter(|(d, _)| d != trade_date)
                .map(|(_, a)| a),
        );
        if hist.len() >= 20 {
            let js = serde_json::to_string(&hist).unwrap_or_else(|_| "[]".into());
            let _ = db::upsert_vendor(conn, "amount_hist", trade_date, Some("eod"), "ok", &js);
        }
    }
    if hist.len() < 20 {
        return Ok(None);
    }
    Ok(tape::median(hist))
}

fn load_indices(conn: &Connection) -> Vec<IndexPrint> {
    let keys: Vec<(&str, &str)> = INDEX_KEYS.iter().map(|k| (k.0, k.1)).collect();
    let Ok(found) = db::index_quotes(conn, &keys) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sym, mkt, ts_code, name) in INDEX_KEYS {
        if let Some(q) = found.iter().find(|q| q.symbol == *sym && q.market == *mkt) {
            if let Some(pct) = q.change_pct {
                out.push(IndexPrint {
                    ts_code: (*ts_code).into(),
                    name: q.name.clone().unwrap_or_else(|| (*name).into()),
                    last: q.last,
                    change_pct: pct,
                    amount: q.turnover,
                    trade_time: q.ts.clone(),
                    vintage: "t0".into(),
                });
            }
        }
    }
    out
}

fn style_spread(indices: &[IndexPrint]) -> Vec<StyleSpread> {
    let hs = indices.iter().find(|i| i.ts_code == "000300.SH");
    let zz = indices.iter().find(|i| i.ts_code == "000852.SH");
    match (hs, zz) {
        (Some(hs), Some(zz)) => vec![StyleSpread {
            name: "沪深300-中证1000".into(),
            left_pct: hs.change_pct,
            right_pct: zz.change_pct,
            spread_pct: hs.change_pct - zz.change_pct,
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PulseConfig;
    use chrono::TimeZone;

    fn cfg() -> PulseConfig {
        PulseConfig::default()
    }

    fn sample_a_tape() -> (Tape, Vec<SectorMove>, Vec<IndexPrint>) {
        let tape = Tape {
            universe: "CN".into(),
            n_quoted: 5124,
            n_up: 1840,
            n_down: 3120,
            n_flat: 164,
            advance_ratio: Some(1840.0 / 5124.0),
            median_pct: Some(-0.72),
            mean_pct: None,
            amount_weighted_pct: Some(0.21),
            amount: 9120e8,
            amount_vs_20d: Some(9120.0 / 8800.0),
            up_amount_ratio: None,
            n_limit_up_est: 38,
            n_limit_down_est: 9,
            top5_amount_share: Some(0.22),
            max_limit_times: None,
        };
        let sectors = vec![
            SectorMove {
                name: "电子".into(),
                change_pct: Some(2.8),
                median_pct: Some(2.0),
                amount: 0.0,
                amount_share: Some(0.11),
                n: 10,
                leaders: Vec::new(),
                rank: 1,
                rank_change: None,
                direction: "up".into(),
                unweighted: false,
            },
            SectorMove {
                name: "有色".into(),
                change_pct: Some(2.1),
                median_pct: Some(1.5),
                amount: 0.0,
                amount_share: Some(0.07),
                n: 8,
                leaders: Vec::new(),
                rank: 2,
                rank_change: None,
                direction: "up".into(),
                unweighted: false,
            },
            SectorMove {
                name: "房地产".into(),
                change_pct: Some(-2.4),
                median_pct: Some(-2.0),
                amount: 0.0,
                amount_share: Some(0.04),
                n: 6,
                leaders: Vec::new(),
                rank: 30,
                rank_change: None,
                direction: "down".into(),
                unweighted: false,
            },
        ];
        let indices = vec![IndexPrint {
            ts_code: "000001.SH".into(),
            name: "上证".into(),
            last: Some(3200.0),
            change_pct: 0.61,
            amount: None,
            trade_time: None,
            vintage: "t0".into(),
        }];
        (tape, sectors, indices)
    }

    #[test]
    fn empty_is_unknown_not_panic() {
        let tape = Tape {
            universe: "CN".into(),
            n_quoted: 0,
            ..Tape::default()
        };
        let r = regime::classify(&tape, &cfg(), &[], false, false, false, false);
        assert_eq!(r.code, "unknown");
        assert_ne!(r.code, "panic");
    }

    #[test]
    fn n_quoted_200_unknown() {
        let tape = Tape {
            universe: "CN".into(),
            n_quoted: 200,
            n_up: 100,
            advance_ratio: Some(0.5),
            median_pct: Some(0.1),
            ..Tape::default()
        };
        let r = regime::classify(&tape, &cfg(), &[], false, false, false, false);
        assert_eq!(r.code, "unknown");
    }

    #[test]
    fn sample_a_hollow() {
        let (tape, sectors, indices) = sample_a_tape();
        let r = regime::classify(&tape, &cfg(), &indices, false, false, false, true);
        assert_eq!(r.code, "hollow");
        let session = SessionInfo {
            trade_date: "2026-09-11".into(),
            phase: SessionPhase::Morning,
            is_open: true,
            frozen: false,
            calendar_source: "trade_cal".into(),
        };
        let h = headline::headline(&session, &r, &tape, &sectors, &indices, &[]);
        assert!(h.contains("指数失真"));
        assert!(h.contains("个股中位"));
        assert!(h.contains("上证"));
        assert!(contains_forbidden(&h).is_none());
    }

    #[test]
    fn sample_b_theme_no_index() {
        let tape = Tape {
            universe: "CN".into(),
            n_quoted: 5000,
            n_up: 2000,
            n_down: 2800,
            n_flat: 200,
            advance_ratio: Some(0.4),
            median_pct: Some(-0.15),
            amount: 8000e8,
            amount_vs_20d: Some(1.0),
            n_limit_up_est: 86,
            top5_amount_share: Some(0.33),
            max_limit_times: None,
            ..Tape::default()
        };
        let r = regime::classify(&tape, &cfg(), &[], false, false, false, true);
        assert_eq!(r.code, "theme");
        let session = SessionInfo {
            trade_date: "2026-09-11".into(),
            phase: SessionPhase::Morning,
            is_open: true,
            frozen: false,
            calendar_source: "trade_cal".into(),
        };
        let h = headline::headline(&session, &r, &tape, &[], &[], &[]);
        assert!(!h.contains("上证"));
        assert!(contains_forbidden(&h).is_none());
    }

    #[test]
    fn st_not_limit_up() {
        let rows = vec![PulseScanRow {
            id: 1,
            symbol: "600519".into(),
            market: "CN.SH".into(),
            name: Some("贵州茅台".into()),
            industry: Some("食品".into()),
            listing_date: None,
            last: 10.51,
            prev_close: Some(10.0),
            change_pct: Some(5.1),
            volume: Some(1.0),
            turnover: Some(1.0),
            ts: None,
            ingested_at: None,
        }];
        let c = tape::classify(rows, "2026-09-12", &cfg());
        let (tape, _, _, _) = tape::build_tape(&c, &cfg(), None);
        assert_eq!(tape.n_limit_up_est, 0);
    }

    #[test]
    fn chinext_limit() {
        let rows = vec![PulseScanRow {
            id: 1,
            symbol: "300001".into(),
            market: "CN.SZ".into(),
            name: Some("特锐德".into()),
            industry: Some("电子".into()),
            listing_date: None,
            last: 12.0,
            prev_close: Some(10.0),
            change_pct: Some(19.99),
            volume: Some(1.0),
            turnover: Some(1.0),
            ts: None,
            ingested_at: None,
        }];
        let c = tape::classify(rows, "2026-09-12", &cfg());
        let (tape, _, _, _) = tape::build_tape(&c, &cfg(), None);
        assert_eq!(tape.n_limit_up_est, 1);
    }

    #[test]
    fn star_limit() {
        let rows = vec![PulseScanRow {
            id: 1,
            symbol: "688001".into(),
            market: "CN.SH".into(),
            name: Some("华兴源创".into()),
            industry: Some("电子".into()),
            listing_date: None,
            last: 12.0,
            prev_close: Some(10.0),
            change_pct: Some(19.99),
            volume: Some(1.0),
            turnover: Some(1.0),
            ts: None,
            ingested_at: None,
        }];
        let c = tape::classify(rows, "2026-09-12", &cfg());
        let (tape, _, _, _) = tape::build_tape(&c, &cfg(), None);
        assert_eq!(tape.n_limit_up_est, 1);
    }

    #[test]
    fn ipo_not_limit() {
        let rows = vec![PulseScanRow {
            id: 1,
            symbol: "601234".into(),
            market: "CN.SH".into(),
            name: Some("新股".into()),
            industry: Some("电子".into()),
            listing_date: Some("2026-09-12".into()),
            last: 14.4,
            prev_close: Some(10.0),
            change_pct: Some(44.0),
            volume: Some(1.0),
            turnover: Some(1.0),
            ts: None,
            ingested_at: None,
        }];
        let c = tape::classify(rows, "2026-09-12", &cfg());
        let (tape, _, _, _) = tape::build_tape(&c, &cfg(), None);
        assert_eq!(tape.n_limit_up_est, 0);
    }

    #[test]
    fn halted_not_limit() {
        let rows = vec![PulseScanRow {
            id: 1,
            symbol: "600519".into(),
            market: "CN.SH".into(),
            name: Some("贵州茅台".into()),
            industry: Some("食品".into()),
            listing_date: None,
            last: 1400.0,
            prev_close: Some(1400.0),
            change_pct: Some(0.0),
            volume: Some(0.0),
            turnover: Some(0.0),
            ts: None,
            ingested_at: None,
        }];
        let c = tape::classify(rows, "2026-09-12", &cfg());
        assert!(c[0].halted);
        let (tape, _, _, _) = tape::build_tape(&c, &cfg(), None);
        assert_eq!(tape.n_limit_up_est, 0);
        assert_eq!(tape.n_flat, 1);
    }

    #[test]
    fn unlabeled_not_in_headline() {
        let tape = Tape {
            universe: "CN".into(),
            n_quoted: 4000,
            n_up: 2000,
            n_down: 1800,
            advance_ratio: Some(0.5),
            median_pct: Some(0.2),
            amount: 1000e8,
            ..Tape::default()
        };
        let sectors = vec![SectorMove {
            name: "未分类".into(),
            change_pct: Some(5.0),
            median_pct: Some(5.0),
            amount: 1.0,
            amount_share: Some(0.5),
            n: 10,
            leaders: Vec::new(),
            rank: 1,
            rank_change: None,
            direction: "up".into(),
            unweighted: false,
        }];
        let session = SessionInfo {
            trade_date: "2026-09-11".into(),
            phase: SessionPhase::Morning,
            is_open: true,
            frozen: false,
            calendar_source: "trade_cal".into(),
        };
        let r = regime::classify(&tape, &cfg(), &[], false, false, true, false);
        assert!(r.confidence != "high");
        let h = headline::headline(&session, &r, &tape, &sectors, &[], &[]);
        assert!(!h.contains("未分类"));
    }

    #[test]
    fn capital_requires_vintage() {
        let err = serde_json::from_str::<FundFlow>(
            r#"{"name":"北向","value":1.0,"unit":"亿","direction":"in"}"#,
        );
        assert!(err.is_err());
    }

    #[test]
    fn lunch_freeze_second_compute_stable() {
        let conn = crate::db::open_memory().unwrap();
        let cfg = cfg();
        let lunch = Utc.with_ymd_and_hms(2026, 9, 11, 4, 0, 0).unwrap();
        let p1 = compute(&conn, &cfg, 5000, &[], lunch).unwrap();
        assert!(p1.session.frozen);
        let p2 = compute(&conn, &cfg, 5000, &[], lunch).unwrap();
        assert_eq!(p1.tape.n_quoted, p2.tape.n_quoted);
        assert_eq!(p1.generated_at, p2.generated_at);
    }

    #[test]
    fn old_quote_is_explicitly_historical() {
        let conn = crate::db::open_memory().unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 23, 7, 0, 0).unwrap();
        let session = SessionInfo {
            trade_date: "2026-09-23".into(),
            phase: SessionPhase::Post,
            is_open: false,
            frozen: false,
            calendar_source: "fallback".into(),
        };
        let rows = vec![PulseScanRow {
            id: 1,
            symbol: "600519".into(),
            market: "CN.SH".into(),
            name: Some("贵州茅台".into()),
            industry: Some("食品".into()),
            listing_date: None,
            last: 10.5,
            prev_close: Some(10.0),
            change_pct: Some(5.0),
            volume: Some(1.0),
            turnover: Some(1.0),
            ts: Some("2024-03-07T07:00:00Z".into()),
            ingested_at: Some(now.to_rfc3339()),
        }];
        let pulse = compute_from_rows(&conn, &cfg(), 5_000, &[], now, session, rows).unwrap();
        assert!(pulse.stale);
        assert_eq!(pulse.as_of.quote_trade_date.as_deref(), Some("2024-03-07"));
        assert_eq!(pulse.regime.code, "stale");
        assert!(pulse.headline.starts_with("历史快照 2024-03-07"));
        assert!(crate::db::get_anchor(&conn, "2026-09-23", "post")
            .unwrap()
            .is_none());
    }

    #[test]
    fn holiday_trade_date_uses_last_open_not_weekday() {
        let conn = crate::db::open_memory().unwrap();
        crate::db::upsert_vendor(
            &conn,
            "trade_cal",
            "2026-10-08",
            Some("t0"),
            "ok",
            "20261002",
        )
        .unwrap();
        // Thursday 8 Oct 2026 10:00 Shanghai = 02:00 UTC; National Day week, last open Fri 2 Oct.
        conn.execute(
            "UPDATE pulse_vendor SET fetched_at='2026-10-08T01:00:00Z' WHERE dataset='trade_cal'",
            [],
        )
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 8, 2, 0, 0).unwrap();
        let p = compute(&conn, &cfg(), 5000, &[], now).unwrap();
        assert_eq!(p.session.trade_date, "2026-10-02");
        assert!(!p.session.is_open);
    }

    #[test]
    fn open_anchor_is_replaced_until_lunch() {
        let conn = crate::db::open_memory().unwrap();
        crate::db::insert_anchor_if_missing(
            &conn,
            "2026-09-11",
            "open",
            Some(1.0),
            Some(0.1),
            r#"{"n":1}"#,
        )
        .unwrap();
        crate::db::replace_anchor(
            &conn,
            "2026-09-11",
            "open",
            Some(9.0),
            Some(0.5),
            r#"{"n":9}"#,
        )
        .unwrap();
        let a = crate::db::get_anchor(&conn, "2026-09-11", "open")
            .unwrap()
            .unwrap();
        assert_eq!(a.amount, Some(9.0));
        assert_eq!(a.payload_json.as_deref(), Some(r#"{"n":9}"#));
    }

    #[test]
    fn n_up_matches_hand() {
        let mut rows = Vec::new();
        for i in 0..5 {
            rows.push(PulseScanRow {
                id: i,
                symbol: format!("60000{i}"),
                market: "CN.SH".into(),
                name: Some("测".into()),
                industry: Some("电子".into()),
                listing_date: None,
                last: 10.0 + i as f64,
                prev_close: Some(10.0),
                change_pct: Some(if i < 3 { 1.0 } else { -1.0 }),
                volume: Some(1.0),
                turnover: Some(1.0),
                ts: None,
                ingested_at: None,
            });
        }
        let c = tape::classify(rows, "2026-09-12", &cfg());
        let (tape, _, _, _) = tape::build_tape(&c, &cfg(), None);
        assert_eq!(tape.n_up, 3);
        assert_eq!(tape.n_down, 2);
    }
}
