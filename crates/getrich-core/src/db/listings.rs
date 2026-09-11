use super::{now_rfc3339, DbError};
use crate::models::{Instrument, InstrumentKey, ListingSnapshot, Market};
use rusqlite::{params, Connection, OptionalExtension};
use std::str::FromStr;

#[derive(Debug, Clone, Default)]
pub struct ListingQuery {
    pub market: Option<Market>,
    pub query: Option<String>,
    pub status: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

pub fn upsert_listing(conn: &Connection, snap: &ListingSnapshot) -> Result<i64, DbError> {
    let now = now_rfc3339();
    let id: i64 = conn.query_row(
        r#"
        INSERT INTO listings (
            symbol, market, exchange, name, name_en, currency, instrument_type, isin,
            listing_date, status, sector, industry, lot_size, source, source_symbol,
            extra_json, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL,?16,?16)
        ON CONFLICT(symbol, market) DO UPDATE SET
            exchange = COALESCE(excluded.exchange, listings.exchange),
            name = COALESCE(excluded.name, listings.name),
            name_en = COALESCE(excluded.name_en, listings.name_en),
            currency = COALESCE(excluded.currency, listings.currency),
            instrument_type = excluded.instrument_type,
            isin = COALESCE(excluded.isin, listings.isin),
            listing_date = COALESCE(excluded.listing_date, listings.listing_date),
            status = excluded.status,
            sector = COALESCE(excluded.sector, listings.sector),
            industry = COALESCE(excluded.industry, listings.industry),
            lot_size = COALESCE(excluded.lot_size, listings.lot_size),
            source = COALESCE(excluded.source, listings.source),
            source_symbol = COALESCE(excluded.source_symbol, listings.source_symbol),
            updated_at = excluded.updated_at
        RETURNING id
        "#,
        params![
            snap.key.symbol,
            snap.key.market.as_str(),
            snap.exchange,
            snap.name,
            snap.name_en,
            snap.currency,
            snap.instrument_type,
            snap.isin,
            snap.listing_date,
            snap.status,
            snap.sector,
            snap.industry,
            snap.lot_size,
            snap.source,
            snap.source_symbol,
            now,
        ],
        |r| r.get(0),
    )?;

    for entry in &snap.metadata {
        conn.execute(
            r#"
            INSERT INTO instrument_metadata (instrument_id, key, value, source, as_of, updated_at)
            VALUES (?1,?2,?3,?4,?5,?6)
            ON CONFLICT(instrument_id, key) DO UPDATE SET
                value = excluded.value,
                source = COALESCE(excluded.source, instrument_metadata.source),
                as_of = COALESCE(excluded.as_of, instrument_metadata.as_of),
                updated_at = excluded.updated_at
            "#,
            params![
                id,
                entry.key,
                entry.value,
                entry.source,
                entry.as_of,
                now,
            ],
        )?;
    }
    Ok(id)
}

pub fn get_listing(conn: &Connection, key: &InstrumentKey) -> Result<Option<Instrument>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, symbol, market, exchange, name, name_en, currency, instrument_type, isin,
               listing_date, delisting_date, status, sector, industry, lot_size, source,
               source_symbol, extra_json, created_at, updated_at
        FROM listings WHERE symbol = ?1 AND market = ?2
        "#,
    )?;
    let row = stmt
        .query_row(params![key.symbol, key.market.as_str()], map_instrument)
        .optional()?;
    Ok(row)
}

pub fn get_listing_by_id(conn: &Connection, id: i64) -> Result<Option<Instrument>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, symbol, market, exchange, name, name_en, currency, instrument_type, isin,
               listing_date, delisting_date, status, sector, industry, lot_size, source,
               source_symbol, extra_json, created_at, updated_at
        FROM listings WHERE id = ?1
        "#,
    )?;
    let row = stmt.query_row(params![id], map_instrument).optional()?;
    Ok(row)
}

pub fn list_listings(conn: &Connection, q: &ListingQuery) -> Result<Vec<Instrument>, DbError> {
    let limit = if q.limit == 0 { 100 } else { q.limit } as i64;
    let offset = q.offset as i64;
    let market = q.market.map(|m| m.as_str().to_string());
    let status = q.status.clone();
    let query = q.query.as_ref().map(|s| format!("%{}%", s.trim()));

    let mut stmt = conn.prepare(
        r#"
        SELECT id, symbol, market, exchange, name, name_en, currency, instrument_type, isin,
               listing_date, delisting_date, status, sector, industry, lot_size, source,
               source_symbol, extra_json, created_at, updated_at
        FROM listings
        WHERE (?1 IS NULL OR market = ?1)
          AND (?2 IS NULL OR status = ?2)
          AND (
                ?3 IS NULL
                OR symbol LIKE ?3
                OR IFNULL(name, '') LIKE ?3
                OR IFNULL(name_en, '') LIKE ?3
                OR IFNULL(source_symbol, '') LIKE ?3
              )
        ORDER BY market, symbol
        LIMIT ?4 OFFSET ?5
        "#,
    )?;
    let rows = stmt.query_map(params![market, status, query, limit, offset], map_instrument)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn list_metadata(
    conn: &Connection,
    instrument_id: i64,
) -> Result<Vec<crate::models::MetadataEntry>, DbError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT key, value, source, as_of
        FROM instrument_metadata
        WHERE instrument_id = ?1
        ORDER BY key
        "#,
    )?;
    let rows = stmt.query_map(params![instrument_id], |r| {
        Ok(crate::models::MetadataEntry {
            key: r.get(0)?,
            value: r.get(1)?,
            source: r.get(2)?,
            as_of: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn map_instrument(r: &rusqlite::Row<'_>) -> rusqlite::Result<Instrument> {
    let market_s: String = r.get(2)?;
    let market = Market::from_str(&market_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
    })?;
    Ok(Instrument {
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
    })
}
