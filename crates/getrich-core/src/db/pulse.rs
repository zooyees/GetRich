use super::{now_rfc3339, DbError};
use crate::models::Market;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct PulseScanRow {
    pub id: i64,
    pub symbol: String,
    pub market: String,
    pub name: Option<String>,
    pub industry: Option<String>,
    pub listing_date: Option<String>,
    pub last: f64,
    pub prev_close: Option<f64>,
    pub change_pct: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub ts: Option<String>,
    pub ingested_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PulseAnchor {
    pub trade_date: String,
    pub phase: String,
    pub amount: Option<f64>,
    pub median_pct: Option<f64>,
    pub payload_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PulseVendor {
    pub dataset: String,
    pub as_of: String,
    pub vintage: Option<String>,
    pub status: String,
    pub payload_json: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Clone)]
pub struct IndexQuote {
    pub symbol: String,
    pub market: String,
    pub name: Option<String>,
    pub last: Option<f64>,
    pub change_pct: Option<f64>,
    pub turnover: Option<f64>,
    pub ts: Option<String>,
}

pub fn pulse_tape_scan(conn: &Connection) -> Result<Vec<PulseScanRow>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT l.id, l.symbol, l.market, l.name, l.industry, l.listing_date,
               q.last, q.prev_close, q.change_pct, q.volume, q.turnover, q.ts, q.ingested_at
        FROM listings l
        JOIN canonical_quotes q ON q.instrument_id = l.id
        WHERE l.market IN (?, ?, ?)
          AND l.status = 'active'
          AND IFNULL(l.instrument_type, 'equity') = 'equity'
          AND q.last IS NOT NULL
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            Market::CnSh.as_str(),
            Market::CnSz.as_str(),
            Market::CnBj.as_str()
        ],
        |r| {
            Ok(PulseScanRow {
                id: r.get(0)?,
                symbol: r.get(1)?,
                market: r.get(2)?,
                name: r.get(3)?,
                industry: r.get(4)?,
                listing_date: r.get(5)?,
                last: r.get(6)?,
                prev_close: r.get(7)?,
                change_pct: r.get(8)?,
                volume: r.get(9)?,
                turnover: r.get(10)?,
                ts: r.get(11)?,
                ingested_at: r.get(12)?,
            })
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_anchor(
    conn: &Connection,
    trade_date: &str,
    phase: &str,
) -> Result<Option<PulseAnchor>, DbError> {
    conn.query_row(
        r#"
        SELECT trade_date, phase, amount, median_pct, payload_json, created_at
        FROM pulse_anchors WHERE trade_date = ?1 AND phase = ?2
        "#,
        params![trade_date, phase],
        |r| {
            Ok(PulseAnchor {
                trade_date: r.get(0)?,
                phase: r.get(1)?,
                amount: r.get(2)?,
                median_pct: r.get(3)?,
                payload_json: r.get(4)?,
                created_at: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn insert_anchor_if_missing(
    conn: &Connection,
    trade_date: &str,
    phase: &str,
    amount: Option<f64>,
    median_pct: Option<f64>,
    payload_json: &str,
) -> Result<bool, DbError> {
    let n = conn.execute(
        r#"
        INSERT OR IGNORE INTO pulse_anchors (
            trade_date, phase, amount, median_pct, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            trade_date,
            phase,
            amount,
            median_pct,
            payload_json,
            now_rfc3339()
        ],
    )?;
    Ok(n > 0)
}

pub fn replace_anchor(
    conn: &Connection,
    trade_date: &str,
    phase: &str,
    amount: Option<f64>,
    median_pct: Option<f64>,
    payload_json: &str,
) -> Result<(), DbError> {
    conn.execute(
        r#"
        INSERT INTO pulse_anchors (
            trade_date, phase, amount, median_pct, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(trade_date, phase) DO UPDATE SET
            amount = excluded.amount,
            median_pct = excluded.median_pct,
            payload_json = excluded.payload_json
        "#,
        params![
            trade_date,
            phase,
            amount,
            median_pct,
            payload_json,
            now_rfc3339()
        ],
    )?;
    Ok(())
}

pub fn list_post_amounts(conn: &Connection, limit: usize) -> Result<Vec<f64>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT amount FROM pulse_anchors
        WHERE phase = 'post' AND amount IS NOT NULL
        ORDER BY trade_date DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit as i64], |r| r.get::<_, f64>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn daily_market_amounts(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<(String, f64)>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT b.ts, SUM(IFNULL(b.turnover, 0))
        FROM bars b
        JOIN listings l ON l.id = b.instrument_id
        WHERE b.timeframe = '1d'
          AND l.market IN (?, ?, ?)
          AND l.status = 'active'
        GROUP BY b.ts
        ORDER BY b.ts DESC
        LIMIT ?4
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            Market::CnSh.as_str(),
            Market::CnSz.as_str(),
            Market::CnBj.as_str(),
            limit as i64
        ],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)),
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn upsert_vendor(
    conn: &Connection,
    dataset: &str,
    as_of: &str,
    vintage: Option<&str>,
    status: &str,
    payload_json: &str,
) -> Result<(), DbError> {
    conn.execute(
        r#"
        INSERT INTO pulse_vendor (dataset, as_of, vintage, status, payload_json, fetched_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(dataset, as_of) DO UPDATE SET
            vintage = excluded.vintage,
            status = excluded.status,
            payload_json = excluded.payload_json,
            fetched_at = excluded.fetched_at
        "#,
        params![dataset, as_of, vintage, status, payload_json, now_rfc3339()],
    )?;
    Ok(())
}

pub fn latest_vendor(conn: &Connection, dataset: &str) -> Result<Option<PulseVendor>, DbError> {
    conn.query_row(
        r#"
        SELECT dataset, as_of, vintage, status, payload_json, fetched_at
        FROM pulse_vendor WHERE dataset = ?1
        ORDER BY as_of DESC
        LIMIT 1
        "#,
        params![dataset],
        |r| {
            Ok(PulseVendor {
                dataset: r.get(0)?,
                as_of: r.get(1)?,
                vintage: r.get(2)?,
                status: r.get(3)?,
                payload_json: r.get(4)?,
                fetched_at: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn index_quotes(conn: &Connection, keys: &[(&str, &str)]) -> Result<Vec<IndexQuote>, DbError> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = String::from(
        r#"
        SELECT l.symbol, l.market, l.name, q.last, q.change_pct, q.turnover, q.ts
        FROM listings l
        JOIN canonical_quotes q ON q.instrument_id = l.id
        WHERE
        "#,
    );
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    for (i, (sym, mkt)) in keys.iter().enumerate() {
        if i > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str("(l.symbol = ? AND l.market = ?)");
        binds.push(rusqlite::types::Value::Text((*sym).into()));
        binds.push(rusqlite::types::Value::Text((*mkt).into()));
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds), |r| {
        Ok(IndexQuote {
            symbol: r.get(0)?,
            market: r.get(1)?,
            name: r.get(2)?,
            last: r.get(3)?,
            change_pct: r.get(4)?,
            turnover: r.get(5)?,
            ts: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
