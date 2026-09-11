//! Quote-board and detail view models (Tonghuashun-style columns).
//!
//! Derived fields (涨跌额 / 振幅 / 方向) are computed from stored quotes.
//! Missing quotes or bars stay null — the UI renders empty states.

use crate::db::QuoteBoardRow;
use crate::models::{Bar, Instrument, MetadataEntry};
use crate::symbol::{board_id, board_label_zh};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct QuoteRow {
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
}

#[derive(Debug, Clone, Serialize)]
pub struct DetailEmpty {
    pub no_quote: bool,
    pub no_bars: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreStats {
    pub listings: i64,
    pub quotes: i64,
    pub bars: i64,
    pub empty: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuoteBoardPage {
    pub stats: StoreStats,
    pub board: String,
    pub query: String,
    pub sort: String,
    pub count: usize,
    pub rows: Vec<QuoteRow>,
    pub empty: bool,
    pub empty_kind: String,
    pub empty_message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BarPage {
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
    };
    if let Some(q) = row.quote {
        view.has_quote = true;
        view.last = Some(q.last);
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

pub fn empty_store_stats() -> StoreStats {
    StoreStats {
        listings: 0,
        quotes: 0,
        bars: 0,
        empty: true,
        message: "数据库为空。导入 listings 后即可在行情列表中查看；导入 quotes / bars 后显示报价与 K 线。"
            .into(),
    }
}

pub fn store_stats_from_counts(listings: i64, quotes: i64, bars: i64) -> StoreStats {
    let empty = listings == 0;
    let message = if empty {
        "数据库为空。请按导入约定写入 listings / quotes / bars 后再查看。"
            .into()
    } else if quotes == 0 {
        format!("已有 {listings} 只股票，但尚无报价。导入 quotes 后行情列表将显示最新价与涨跌。")
    } else {
        format!("listings={listings} quotes={quotes} bars={bars}")
    };
    StoreStats {
        listings,
        quotes,
        bars,
        empty,
        message,
    }
}

pub fn board_empty_message(stats: &StoreStats, filtered_empty: bool) -> (String, String) {
    if stats.empty {
        (
            "no_data".into(),
            "暂无股票数据。后续导入 listings 后将出现在此列表。".into(),
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
        let listing = ListingSnapshot::equity(InstrumentKey::new("AAPL", Market::Us), "import", "AAPL");
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
}
