//! Quote-board read model (listings LEFT JOIN quotes).

use super::DbError;
use crate::models::{Instrument, InstrumentKey, Market, Quote};
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};
use std::str::FromStr;

#[derive(Debug, Clone, Default)]
pub struct QuoteBoardQuery {
    pub scope: BoardScope,
    pub query: Option<String>,
    pub industry: Option<String>,
    pub status: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub sort: QuoteSort,
}

/// SQL-side board filter (科创板 / 创业板 / 沪深京 / 自选 are not separate markets).
#[derive(Debug, Clone, Default)]
pub enum BoardScope {
    #[default]
    All,
    Market(Market),
    /// A-shares: 沪 / 深 / 京
    CnA,
    /// STAR: CN.SH 688/689
    Star,
    /// ChiNext: CN.SZ 300/301/302
    Chinext,
    Watch(Vec<InstrumentKey>),
}

impl BoardScope {
    pub fn parse(board: &str, watch: Vec<InstrumentKey>) -> Self {
        let b = board.trim().to_ascii_uppercase();
        match b.as_str() {
            "" | "ALL" | "全部" => Self::All,
            "US" | "美股" => Self::Market(Market::Us),
            "HK" | "港股" => Self::Market(Market::Hk),
            "CN.SH" | "SH" | "沪市" => Self::Market(Market::CnSh),
            "CN.SZ" | "SZ" | "深市" => Self::Market(Market::CnSz),
            "CN.BJ" | "BJ" | "北证" => Self::Market(Market::CnBj),
            "STAR" | "KCB" | "科创板" => Self::Star,
            "CHINEXT" | "CYB" | "创业板" => Self::Chinext,
            "CN" | "A" | "沪深京" => Self::CnA,
            "WATCH" | "WATCHLIST" => Self::Watch(watch),
            _ if board.trim() == "自选" => Self::Watch(watch),
            _ => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum QuoteSort {
    #[default]
    Symbol,
    Name,
    Last,
    Change,
    ChangePct,
    ChangePctAsc,
    Volume,
    Turnover,
    Amplitude,
    LastAsc,
    ChangeAsc,
    TurnoverAsc,
    VolumeAsc,
    AmplitudeAsc,
    SymbolDesc,
    NameDesc,
}

impl QuoteSort {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "name" | "名称" => Self::Name,
            "last" | "price" | "最新" => Self::Last,
            "chg" | "change_amt" | "涨跌额" => Self::Change,
            "change_pct" | "pct" | "涨跌幅" | "涨幅" => Self::ChangePct,
            "change_pct_asc" | "pct_asc" | "涨跌幅升序" => Self::ChangePctAsc,
            "volume" | "vol" | "成交量" => Self::Volume,
            "turnover" | "amount" | "成交额" => Self::Turnover,
            "amplitude" | "amp" | "振幅" => Self::Amplitude,
            "last_asc" => Self::LastAsc,
            "change_amt_asc" => Self::ChangeAsc,
            "turnover_asc" => Self::TurnoverAsc,
            "volume_asc" => Self::VolumeAsc,
            "amplitude_asc" => Self::AmplitudeAsc,
            "symbol_desc" => Self::SymbolDesc,
            "name_desc" => Self::NameDesc,
            _ => Self::Symbol,
        }
    }

    fn sql(self) -> &'static str {
        match self {
            Self::Symbol => "l.market ASC, l.market ASC, l.symbol ASC",
            Self::Name => "LOWER(IFNULL(l.name, l.symbol)) ASC, l.market ASC, l.symbol ASC",
            Self::Last => "q.last IS NULL, q.last DESC, l.market ASC, l.symbol ASC",
            Self::Change => "computed_chg IS NULL, computed_chg DESC, l.market ASC, l.symbol ASC",
            Self::ChangePct => {
                "computed_pct IS NULL, computed_pct DESC, l.market ASC, l.symbol ASC"
            }
            Self::ChangePctAsc => {
                "computed_pct IS NULL, computed_pct ASC, l.market ASC, l.symbol ASC"
            }
            Self::Volume => "q.volume IS NULL, q.volume DESC, l.market ASC, l.symbol ASC",
            Self::Turnover => "q.turnover IS NULL, q.turnover DESC, l.market ASC, l.symbol ASC",
            Self::Amplitude => {
                "computed_amp IS NULL, computed_amp DESC, l.market ASC, l.symbol ASC"
            }
            Self::LastAsc => "q.last IS NULL, q.last ASC, l.market ASC, l.symbol ASC",
            Self::ChangeAsc => "computed_chg IS NULL, computed_chg ASC, l.market ASC, l.symbol ASC",
            Self::TurnoverAsc => "q.turnover IS NULL, q.turnover ASC, l.market ASC, l.symbol ASC",
            Self::VolumeAsc => "q.volume IS NULL, q.volume ASC, l.market ASC, l.symbol ASC",
            Self::AmplitudeAsc => {
                "computed_amp IS NULL, computed_amp ASC, l.market ASC, l.symbol ASC"
            }
            Self::SymbolDesc => "l.market DESC, l.symbol DESC",
            Self::NameDesc => "LOWER(IFNULL(l.name,l.symbol)) DESC,l.market ASC,l.symbol ASC",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuoteBoardRow {
    pub instrument: Instrument,
    pub quote: Option<Quote>,
}

fn board_where(q: &QuoteBoardQuery) -> (String, Vec<Value>) {
    let mut sql = String::from(" WHERE l.status != 'internal'");
    let mut binds: Vec<Value> = Vec::new();
    match &q.scope {
        BoardScope::All => {}
        BoardScope::Market(m) => {
            sql.push_str(" AND l.market = ?");
            binds.push(Value::Text(m.as_str().into()));
        }
        BoardScope::CnA => {
            sql.push_str(" AND l.market IN (?, ?, ?)");
            binds.push(Value::Text(Market::CnSh.as_str().into()));
            binds.push(Value::Text(Market::CnSz.as_str().into()));
            binds.push(Value::Text(Market::CnBj.as_str().into()));
        }
        BoardScope::Star => {
            sql.push_str(" AND l.market = ? AND (l.symbol LIKE ? OR l.symbol LIKE ?)");
            binds.push(Value::Text(Market::CnSh.as_str().into()));
            binds.push(Value::Text("688%".into()));
            binds.push(Value::Text("689%".into()));
        }
        BoardScope::Chinext => {
            sql.push_str(
                " AND l.market = ? AND (l.symbol LIKE ? OR l.symbol LIKE ? OR l.symbol LIKE ?)",
            );
            binds.push(Value::Text(Market::CnSz.as_str().into()));
            binds.push(Value::Text("300%".into()));
            binds.push(Value::Text("301%".into()));
            binds.push(Value::Text("302%".into()));
        }
        BoardScope::Watch(keys) => {
            if keys.is_empty() {
                sql.push_str(" AND 0");
            } else {
                sql.push_str(" AND (");
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        sql.push_str(" OR ");
                    }
                    sql.push_str("(l.symbol = ? AND l.market = ?)");
                    binds.push(Value::Text(k.symbol.clone()));
                    binds.push(Value::Text(k.market.as_str().into()));
                }
                sql.push(')');
            }
        }
    }

    if let Some(status) = &q.status {
        sql.push_str(" AND l.status = ?");
        binds.push(Value::Text(status.clone()));
    }

    if let Some(industry) = q
        .industry
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        sql.push_str(" AND l.industry = ?");
        binds.push(Value::Text(industry.to_string()));
    }

    if let Some(query) = q.query.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sql.push_str(
            " AND (l.symbol LIKE ? OR IFNULL(l.name, '') LIKE ? OR IFNULL(l.name_en, '') LIKE ? OR IFNULL(l.source_symbol, '') LIKE ?)",
        );
        let pat = format!("%{query}%");
        for _ in 0..4 {
            binds.push(Value::Text(pat.clone()));
        }
    }

    (sql, binds)
}

pub fn count_quote_board(conn: &Connection, q: &QuoteBoardQuery) -> Result<i64, DbError> {
    let (where_sql, binds) = board_where(q);
    let sql = format!("SELECT COUNT(*) FROM listings l LEFT JOIN canonical_quotes q ON q.instrument_id=l.id{where_sql}");
    let n = conn.query_row(&sql, params_from_iter(binds), |r| r.get(0))?;
    Ok(n)
}

pub fn list_quote_board(
    conn: &Connection,
    q: &QuoteBoardQuery,
) -> Result<Vec<QuoteBoardRow>, DbError> {
    let limit = if q.limit == 0 {
        200
    } else {
        q.limit.min(100_000)
    } as i64;
    let offset = q.offset as i64;
    let order = q.sort.sql();
    let (where_sql, mut binds) = board_where(q);
    let sql = format!(
        r#"
        SELECT
            l.id, l.symbol, l.market, l.exchange, l.name, l.name_en, l.currency,
            l.instrument_type, l.isin, l.listing_date, l.delisting_date, l.status,
            l.sector, l.industry, l.lot_size, l.source, l.source_symbol, l.extra_json,
            l.created_at, l.updated_at,
            q.instrument_id, q.ts, q.last, q.open, q.high, q.low, q.prev_close,
            q.volume, q.turnover, q.change_pct, q.currency, q.source, q.ingested_at,
            CASE
              WHEN q.change_pct IS NOT NULL THEN q.change_pct
              WHEN q.prev_close IS NOT NULL AND q.prev_close != 0 THEN (q.last - q.prev_close) * 100.0 / q.prev_close
              ELSE NULL
            END AS computed_pct,
            CASE WHEN q.prev_close IS NOT NULL THEN q.last - q.prev_close ELSE NULL END AS computed_chg,
            CASE
              WHEN q.high IS NOT NULL AND q.low IS NOT NULL AND q.prev_close IS NOT NULL AND q.prev_close != 0
              THEN (q.high - q.low) * 100.0 / q.prev_close
              ELSE NULL
            END AS computed_amp
        FROM listings l
        LEFT JOIN canonical_quotes q ON q.instrument_id = l.id
        {where_sql}
        ORDER BY {order}
        LIMIT ? OFFSET ?
        "#
    );
    binds.push(Value::Integer(limit));
    binds.push(Value::Integer(offset));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(binds), map_board_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn map_board_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<QuoteBoardRow> {
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
}

pub fn store_stats(conn: &Connection) -> Result<(i64, i64, i64, Option<String>), DbError> {
    let listings: i64 = conn.query_row(
        "SELECT COUNT(*) FROM listings WHERE status IS NULL OR status != 'internal'",
        [],
        |r| r.get(0),
    )?;
    let quotes: i64 = conn.query_row("SELECT COUNT(*) FROM quotes", [], |r| r.get(0))?;
    let bars: i64 = conn.query_row("SELECT COUNT(*) FROM bars", [], |r| r.get(0))?;
    let last_quote_ts: Option<String> =
        conn.query_row("SELECT MAX(ts) FROM quotes", [], |r| r.get(0))?;
    Ok((listings, quotes, bars, last_quote_ts))
}
