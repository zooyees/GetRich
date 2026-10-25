//! Quote-board and detail view models (Tonghuashun-style columns).
//!
//! Derived fields (涨跌额 / 振幅 / 方向) are computed from stored quotes.
//! Missing quotes or bars stay null — the UI renders empty states.

use crate::db::QuoteBoardRow;
use crate::models::{Bar, Instrument, MetadataEntry};
use crate::session;
use crate::symbol::{board_id, board_label_zh};
use chrono::Timelike;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct QuoteRow {
    pub trade_date: Option<String>,
    pub received_at: Option<String>,
    pub source_kind: String,
    pub data_status: String,
    pub instrument_id: i64,
    pub symbol: String,
    pub market: String,
    pub market_label: String,
    pub board: String,
    pub board_label: String,
    pub name: Option<String>,
    pub exchange: Option<String>,
    pub currency: Option<String>,
    pub status: String,
    pub last: Option<f64>,
    pub change: Option<f64>,
    pub change_pct: Option<f64>,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub prev_close: Option<f64>,
    pub amplitude: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub ts: Option<String>,
    pub has_quote: bool,
    /// `up` | `down` | `flat` | `none`
    pub direction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BarStats {
    pub timeframe: String,
    pub count: i64,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StockDetail {
    pub listing: Instrument,
    pub quote: QuoteRow,
    pub metadata: Vec<MetadataEntry>,
    pub bars: BarStats,
    pub empty: DetailEmpty,
    pub watched: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetailEmpty {
    pub no_quote: bool,
    pub no_bars: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreStats {
    pub daily_bars: i64,
    pub weekly_bars: i64,
    pub monthly_bars: i64,
    pub listings: i64,
    pub quotes: i64,
    pub bars: i64,
    pub empty: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_quote_ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuoteBoardPage {
    pub stats: StoreStats,
    pub board: String,
    pub query: String,
    pub sort: String,
    #[serde(default)]
    pub industry: String,
    pub count: usize,
    pub matched: usize,
    pub truncated: bool,
    pub rows: Vec<QuoteRow>,
    pub empty: bool,
    pub empty_kind: String,
    pub empty_message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BarPage {
    pub basis_timeframe: String,
    pub partial_last_period: bool,
    pub data_version: u64,
    pub symbol: String,
    pub market: String,
    pub timeframe: String,
    pub count: usize,
    pub from: Option<String>,
    pub to: Option<String>,
    pub empty: bool,
    pub empty_message: String,
    pub rows: Vec<Bar>,
}

pub fn quote_row_from_board(row: QuoteBoardRow) -> QuoteRow {
    let inst = row.instrument;
    let board = board_id(&inst.key).to_string();
    let mut view = QuoteRow {
        trade_date: None,
        received_at: None,
        source_kind: "missing".into(),
        data_status: "无报价".into(),
        instrument_id: inst.id,
        symbol: inst.key.symbol.clone(),
        market: inst.key.market.as_str().to_string(),
        market_label: inst.key.market.label_zh().to_string(),
        board: board.clone(),
        board_label: board_label_zh(&board).to_string(),
        name: inst.name.or(inst.name_en.clone()),
        exchange: inst.exchange,
        currency: inst.currency,
        status: inst.status,
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
    };
    if let Some(q) = row.quote {
        view.received_at = Some(q.ingested_at.clone());
        view.source_kind = if q.source.contains("rt_k") {
            "live"
        } else if q.source.contains("daily") || q.ts.contains("T00:00:00") {
            "daily"
        } else {
            "snapshot"
        }
        .into();
        let source_time = session::parse_cn_ts(&q.ts).ok();
        view.trade_date = if view.source_kind == "daily" {
            q.ts.get(..10).map(str::to_string)
        } else {
            source_time
                .map(|t| session::market_date_at(&view.market, t))
                .map(session::iso_date)
        };
        view.data_status = if view.trade_date.as_deref()
            != Some(
                session::iso_date(session::market_date_at(&view.market, chrono::Utc::now()))
                    .as_str(),
            ) {
            "历史报价"
        } else if source_time.is_some_and(|t| (chrono::Utc::now() - t).num_seconds() > 60)
            && view.market.starts_with("CN.")
            && session::in_poll_window()
            && view.source_kind != "daily"
        {
            "报价延迟"
        } else {
            "已保存报价"
        }
        .into();
        view.has_quote = true;
        view.last = Some(q.last);
        view.source = if q.source.is_empty() {
            None
        } else {
            Some(q.source.clone())
        };
        view.open = q.open;
        view.high = q.high;
        view.low = q.low;
        view.prev_close = q.prev_close;
        view.volume = q.volume;
        view.turnover = q.turnover;
        view.ts = Some(q.ts);
        if view.currency.is_none() {
            view.currency = q.currency;
        }
        let change = q.prev_close.map(|p| q.last - p);
        view.change = change;
        view.change_pct = q.change_pct.or_else(|| {
            q.prev_close
                .filter(|p| *p != 0.0)
                .map(|p| (q.last - p) / p * 100.0)
        });
        view.amplitude = match (q.high, q.low, q.prev_close) {
            (Some(h), Some(l), Some(p)) if p != 0.0 => Some((h - l) / p * 100.0),
            _ => None,
        };
        view.direction = match view.change {
            Some(c) if c > 0.0 => "up".into(),
            Some(c) if c < 0.0 => "down".into(),
            Some(_) => "flat".into(),
            None => match view.change_pct {
                Some(p) if p > 0.0 => "up".into(),
                Some(p) if p < 0.0 => "down".into(),
                Some(_) => "flat".into(),
                None => "none".into(),
            },
        };
    }
    view
}

/// Align `last` with the session 1d bar when that bar is the live print, without
/// letting a date-only / daily bar clobber an `rt_k` quote.
pub fn prefer_session_last(row: &mut QuoteRow, bar: Option<&Bar>) {
    let Some(bar) = bar else { return };
    if !bar.close.is_finite() {
        return;
    }
    let Some(bar_day) = session::calendar_date_of(&bar.ts) else {
        return;
    };
    let quote_live = row.source.as_deref().is_some_and(|s| s.contains("rt_k"));
    let bar_live = bar.source.contains("rt_k");
    let bar_daily = bar.source.contains("daily");
    if quote_live && bar_daily {
        return;
    }
    let quote_dt = row.ts.as_deref().and_then(|t| session::parse_cn_ts(t).ok());
    let quote_day = quote_dt
        .map(session::shanghai_date_at)
        .or_else(|| row.ts.as_deref().and_then(session::calendar_date_of));
    // Unparsable ts must not be treated as EOD — that used to overwrite live last.
    let quote_is_eod = quote_dt
        .map(|dt| dt.time().num_seconds_from_midnight() == 0 && dt.nanosecond() == 0)
        .unwrap_or(false);
    if let Some(qd) = quote_day {
        if bar_day < qd {
            return;
        }
        if bar_day == qd {
            if bar_daily && !quote_is_eod {
                return;
            }
            if !bar_live && !quote_is_eod && row.last.is_some() {
                return;
            }
        }
        if bar_day > qd && row.last.is_some() {
            row.prev_close = row.last;
        }
    } else if row.last.is_some() && !bar_live {
        return;
    }
    row.last = Some(bar.close);
    row.open = Some(bar.open);
    row.high = Some(bar.high);
    row.low = Some(bar.low);
    row.volume = bar.volume.or(row.volume);
    row.turnover = bar.turnover.or(row.turnover);
    row.has_quote = true;
    if quote_is_eod || quote_day.map(|qd| bar_day > qd).unwrap_or(true) {
        row.ts = Some(bar.ts.clone());
    }
    row.change = row.prev_close.map(|p| bar.close - p);
    row.change_pct = row
        .prev_close
        .filter(|p| *p != 0.0)
        .map(|p| (bar.close - p) / p * 100.0);
    row.amplitude = match (row.high, row.low, row.prev_close) {
        (Some(h), Some(l), Some(p)) if p != 0.0 => Some((h - l) / p * 100.0),
        _ => None,
    };
    row.direction = match row.change {
        Some(c) if c > 0.0 => "up".into(),
        Some(c) if c < 0.0 => "down".into(),
        Some(_) => "flat".into(),
        None => "none".into(),
    };
}

pub fn empty_store_stats() -> StoreStats {
    StoreStats {
        daily_bars: 0,
        weekly_bars: 0,
        monthly_bars: 0,
        listings: 0,
        quotes: 0,
        bars: 0,
        empty: true,
        message: "库是空的。导入 listings 后即可看盘；导入 quotes / bars 后显示报价与 K 线。"
            .into(),
        last_quote_ts: None,
        last_success_at: None,
    }
}

pub fn store_stats_from_counts(
    listings: i64,
    quotes: i64,
    bars: i64,
    last_quote_ts: Option<String>,
    last_success_at: Option<String>,
) -> StoreStats {
    let empty = listings == 0;
    let message = if empty {
        "库是空的。导入 listings / quotes / bars 后再查看。".into()
    } else if quotes == 0 {
        format!("已有 {listings} 只股票，但尚无报价。导入 quotes 后行情表将显示最新价与涨跌。")
    } else {
        format!("listings={listings} quotes={quotes} bars={bars}")
    };
    StoreStats {
        daily_bars: bars,
        weekly_bars: 0,
        monthly_bars: 0,
        listings,
        quotes,
        bars,
        empty,
        message,
        last_quote_ts,
        last_success_at,
    }
}

pub fn board_empty_message(stats: &StoreStats, filtered_empty: bool) -> (String, String) {
    if stats.empty {
        (
            "no_data".into(),
            "库是空的。到「数据」页导入 bundle 后即可看盘。".into(),
        )
    } else if filtered_empty {
        (
            "no_match".into(),
            "当前板块或搜索条件下没有股票。尝试切换「全部」或清空搜索。".into(),
        )
    } else {
        ("ok".into(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{InstrumentKey, ListingSnapshot, Market, QuoteSnapshot};
    use chrono::{TimeZone, Utc};

    #[test]
    fn computes_change_and_amplitude() {
        let listing =
            ListingSnapshot::equity(InstrumentKey::new("AAPL", Market::Us), "import", "AAPL");
        let inst = Instrument {
            id: 1,
            key: listing.key.clone(),
            exchange: listing.exchange,
            name: Some("Apple".into()),
            name_en: None,
            currency: Some("USD".into()),
            instrument_type: "equity".into(),
            isin: None,
            listing_date: None,
            delisting_date: None,
            status: "active".into(),
            sector: None,
            industry: None,
            lot_size: None,
            source: Some("import".into()),
            source_symbol: Some("AAPL".into()),
            extra_json: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let quote = QuoteSnapshot {
            key: listing.key,
            ts: Utc.with_ymd_and_hms(2024, 1, 2, 21, 0, 0).unwrap(),
            last: 110.0,
            open: Some(100.0),
            high: Some(112.0),
            low: Some(99.0),
            prev_close: Some(100.0),
            volume: Some(10.0),
            turnover: Some(1100.0),
            change_pct: None,
            currency: Some("USD".into()),
            source: "import".into(),
        };
        let row = quote_row_from_board(QuoteBoardRow {
            instrument: inst,
            quote: Some(crate::models::Quote {
                instrument_id: 1,
                ts: quote.ts.to_rfc3339(),
                last: quote.last,
                open: quote.open,
                high: quote.high,
                low: quote.low,
                prev_close: quote.prev_close,
                volume: quote.volume,
                turnover: quote.turnover,
                change_pct: quote.change_pct,
                currency: quote.currency,
                source: quote.source,
                ingested_at: String::new(),
            }),
        });
        assert_eq!(row.change, Some(10.0));
        assert_eq!(row.change_pct, Some(10.0));
        assert!((row.amplitude.unwrap() - 13.0).abs() < 1e-9);
        assert_eq!(row.direction, "up");
    }

    fn sample_row(ts: &str, last: f64) -> QuoteRow {
        QuoteRow {
            trade_date: None,
            received_at: None,
            source_kind: "test".into(),
            data_status: "test".into(),
            instrument_id: 1,
            symbol: "600519".into(),
            market: "CN.SH".into(),
            market_label: "沪市".into(),
            board: "SH".into(),
            board_label: "沪市".into(),
            name: Some("贵州茅台".into()),
            exchange: None,
            currency: Some("CNY".into()),
            status: "active".into(),
            last: Some(last),
            change: None,
            change_pct: None,
            open: Some(last),
            high: Some(last),
            low: Some(last),
            prev_close: Some(100.0),
            amplitude: None,
            volume: None,
            turnover: None,
            ts: Some(ts.into()),
            has_quote: true,
            direction: "none".into(),
            source: None,
        }
    }

    fn sample_bar(ts: &str, close: f64) -> Bar {
        Bar {
            id: 1,
            instrument_id: 1,
            timeframe: crate::models::Timeframe::D1,
            ts: ts.into(),
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            adj_close: None,
            volume: Some(1.0),
            turnover: None,
            source: "tushare.rt_k".into(),
            ingested_at: String::new(),
        }
    }

    #[test]
    fn session_bar_replaces_date_only_quote() {
        let mut row = sample_row("2026-09-12", 9.0);
        row.prev_close = Some(9.0);
        prefer_session_last(&mut row, Some(&sample_bar("2026-09-12", 10.5)));
        assert_eq!(row.last, Some(10.5));
        assert_eq!(row.change, Some(1.5));
    }

    #[test]
    fn daily_bar_does_not_clobber_timed_quote() {
        let mut row = sample_row("2026-09-12T02:15:00Z", 11.0);
        row.source = Some("tushare.rt_k".into());
        let mut bar = sample_bar("2026-09-12", 10.5);
        bar.source = "tushare.daily".into();
        prefer_session_last(&mut row, Some(&bar));
        assert_eq!(row.last, Some(11.0));
    }

    #[test]
    fn rt_k_bar_aligns_header_with_session_close() {
        let mut row = sample_row("2026-09-12T02:15:00Z", 11.0);
        row.source = Some("tushare.rt_k".into());
        prefer_session_last(&mut row, Some(&sample_bar("2026-09-12", 11.2)));
        assert_eq!(row.last, Some(11.2));
    }

    #[test]
    fn unparsable_quote_ts_keeps_live_last() {
        let mut row = sample_row("not-a-timestamp", 11.0);
        row.source = Some("tushare.rt_k".into());
        let mut bar = sample_bar("2026-09-12", 9.0);
        bar.source = "tushare.daily".into();
        prefer_session_last(&mut row, Some(&bar));
        assert_eq!(row.last, Some(11.0));
    }
}
