//! Quote-board read model (listings LEFT JOIN quotes).

use super::DbError;
use crate::models::{Instrument, InstrumentKey, Market, Quote};
use rusqlite::{params, Connection};
use std::str::FromStr;

#[derive(Debug, Clone, Default)]
pub struct QuoteBoardQuery {
    pub market: Option<Market>,
    pub query: Option<String>,
    pub status: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub sort: QuoteSort,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum QuoteSort {
    #[default]
    Symbol,
    ChangePct,
    Volume,
    Turnover,
}

impl QuoteSort {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "change" | "change_pct" | "pct" | "涨跌幅" => Self::ChangePct,
            "volume" | "vol" | "成交量" => Self::Volume,
            "turnover" | "amount" | "成交额" => Self::Turnover,
            _ => Self::Symbol,
        }
    }

    fn sql(self) -> &'static str {
        match self {
            Self::Symbol => "l.market ASC, l.symbol ASC",
            Self::ChangePct => "q.change_pct IS NULL, q.change_pct DESC, l.symbol ASC",
            Self::Volume => "q.volume IS NULL, q.volume DESC, l.symbol ASC",
            Self::Turnover => "q.turnover IS NULL, q.turnover DESC, l.symbol ASC",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuoteBoardRow {
    pub instrument: Instrument,
    pub quote: Option<Quote>,
}

pub fn list_quote_board(
    conn: &Connection,
    q: &QuoteBoardQuery,
) -> Result<Vec<QuoteBoardRow>, DbError> {
    let limit = if q.limit == 0 { 200 } else { q.limit.min(2000) } as i64;
    let offset = q.offset as i64;
    let market = q.market.map(|m| m.as_str().to_string());
    let status = q.status.clone();
    let query = q.query.as_ref().map(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(format!("%{t}%"))
        }
    });
    let query = query.flatten();
    let order = q.sort.sql();
    let sql = format!(
        r#"
        SELECT
            l.id, l.symbol, l.market, l.exchange, l.name, l.name_en, l.currency,
            l.instrument_type, l.isin, l.listing_date, l.delisting_date, l.status,
            l.sector, l.industry, l.lot_size, l.source, l.source_symbol, l.extra_json,
            l.created_at, l.updated_at,
            q.instrument_id, q.ts, q.last, q.open, q.high, q.low, q.prev_close,
            q.volume, q.turnover, q.change_pct, q.currency, q.source, q.ingested_at
        FROM listings l
        LEFT JOIN quotes q ON q.instrument_id = l.id
        WHERE (?1 IS NULL OR l.market = ?1)
          AND (?2 IS NULL OR l.status = ?2)
          AND (
                ?3 IS NULL
                OR l.symbol LIKE ?3
                OR IFNULL(l.name, '') LIKE ?3
                OR IFNULL(l.name_en, '') LIKE ?3
                OR IFNULL(l.source_symbol, '') LIKE ?3
              )
        ORDER BY {order}
        LIMIT ?4 OFFSET ?5
        "#
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![market, status, query, limit, offset], |r| {
        let market_s: String = r.get(2)?;
        let market = Market::from_str(&market_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?;
        let instrument = Instrument {
            id: r.get(0)?,
            key: InstrumentKey {
                symbol: r.get(1)?,
                market,
            },
            exchange: r.get(3)?,
            name: r.get(4)?,
            name_en: r.get(5)?,
            currency: r.get(6)?,
            instrument_type: r.get(7)?,
            isin: r.get(8)?,
            listing_date: r.get(9)?,
            delisting_date: r.get(10)?,
            status: r.get(11)?,
            sector: r.get(12)?,
            industry: r.get(13)?,
            lot_size: r.get(14)?,
            source: r.get(15)?,
            source_symbol: r.get(16)?,
            extra_json: r.get(17)?,
            created_at: r.get(18)?,
            updated_at: r.get(19)?,
        };
        let quote_id: Option<i64> = r.get(20)?;
        let quote = match quote_id {
            Some(_) => Some(Quote {
                instrument_id: r.get(20)?,
                ts: r.get(21)?,
                last: r.get(22)?,
                open: r.get(23)?,
                high: r.get(24)?,
                low: r.get(25)?,
                prev_close: r.get(26)?,
                volume: r.get(27)?,
                turnover: r.get(28)?,
                change_pct: r.get(29)?,
                currency: r.get(30)?,
                source: r.get(31)?,
                ingested_at: r.get(32)?,
            }),
            None => None,
        };
        Ok(QuoteBoardRow { instrument, quote })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn store_stats(conn: &Connection) -> Result<(i64, i64, i64), DbError> {
    let listings: i64 = conn.query_row("SELECT COUNT(*) FROM listings", [], |r| r.get(0))?;
    let quotes: i64 = conn.query_row("SELECT COUNT(*) FROM quotes", [], |r| r.get(0))?;
    let bars: i64 = conn.query_row("SELECT COUNT(*) FROM bars", [], |r| r.get(0))?;
    Ok((listings, quotes, bars))
}
