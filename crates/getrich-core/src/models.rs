//! Domain types for the stock foundation store.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Market {
    Us,
    Hk,
    #[serde(rename = "CN.SH")]
    CnSh,
    #[serde(rename = "CN.SZ")]
    CnSz,
    #[serde(rename = "CN.BJ")]
    CnBj,
}

impl Market {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Us => "US",
            Self::Hk => "HK",
            Self::CnSh => "CN.SH",
            Self::CnSz => "CN.SZ",
            Self::CnBj => "CN.BJ",
        }
    }

    pub fn default_currency(self) -> &'static str {
        match self {
            Self::Us => "USD",
            Self::Hk => "HKD",
            Self::CnSh | Self::CnSz | Self::CnBj => "CNY",
        }
    }

    pub fn default_exchange(self) -> &'static str {
        match self {
            Self::Us => "US",
            Self::Hk => "HKEX",
            Self::CnSh => "SSE",
            Self::CnSz => "SZSE",
            Self::CnBj => "BSE",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Us => "美股",
            Self::Hk => "港股",
            Self::CnSh => "沪市",
            Self::CnSz => "深市",
            Self::CnBj => "北证",
        }
    }
}

impl fmt::Display for Market {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Market {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            "US" | "USA" | "NASDAQ" | "NYSE" | "AMEX" => Ok(Self::Us),
            "HK" | "HKG" | "HKEX" => Ok(Self::Hk),
            "CN.SH" | "SH" | "SSE" | "SHA" | "XSHG" => Ok(Self::CnSh),
            "CN.SZ" | "SZ" | "SZSE" | "SHE" | "XSHE" => Ok(Self::CnSz),
            "CN.BJ" | "BJ" | "BSE" | "BSEJ" | "NEEQ" => Ok(Self::CnBj),
            "CN" => Err("ambiguous market CN; use CN.SH, CN.SZ, or CN.BJ".into()),
            other => Err(format!("unknown market: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe {
    #[serde(rename = "1m")]
    M1,
    #[serde(rename = "5m")]
    M5,
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "30m")]
    M30,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "1d")]
    D1,
    #[serde(rename = "1w")]
    W1,
    #[serde(rename = "1mo")]
    Mo1,
}

impl Timeframe {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::M1 => "1m",
            Self::M5 => "5m",
            Self::M15 => "15m",
            Self::M30 => "30m",
            Self::H1 => "1h",
            Self::D1 => "1d",
            Self::W1 => "1w",
            Self::Mo1 => "1mo",
        }
    }

    pub fn is_intraday(self) -> bool {
        matches!(self, Self::M1 | Self::M5 | Self::M15 | Self::M30 | Self::H1)
    }

    pub fn dataset_key(self) -> String {
        format!("bars:{}", self.as_str())
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Timeframe {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "1m" | "1min" => Ok(Self::M1),
            "5m" | "5min" => Ok(Self::M5),
            "15m" | "15min" => Ok(Self::M15),
            "30m" | "30min" => Ok(Self::M30),
            "1h" | "60m" | "60min" => Ok(Self::H1),
            "1d" | "d" | "day" | "daily" => Ok(Self::D1),
            "1w" | "1wk" | "w" | "week" | "weekly" => Ok(Self::W1),
            "1mo" | "1mth" | "month" | "monthly" => Ok(Self::Mo1),
            other => Err(format!("unknown timeframe: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentKey {
    pub symbol: String,
    pub market: Market,
}

impl InstrumentKey {
    pub fn new(symbol: impl Into<String>, market: Market) -> Self {
        Self {
            symbol: symbol.into(),
            market,
        }
    }

    pub fn display(&self) -> String {
        format!("{}:{}", self.market.as_str(), self.symbol)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    pub id: i64,
    pub key: InstrumentKey,
    pub exchange: Option<String>,
    pub name: Option<String>,
    pub name_en: Option<String>,
    pub currency: Option<String>,
    pub instrument_type: String,
    pub isin: Option<String>,
    pub listing_date: Option<String>,
    pub delisting_date: Option<String>,
    pub status: String,
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub lot_size: Option<i64>,
    pub source: Option<String>,
    pub source_symbol: Option<String>,
    pub extra_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingSnapshot {
    pub key: InstrumentKey,
    pub exchange: Option<String>,
    pub name: Option<String>,
    pub name_en: Option<String>,
    pub currency: Option<String>,
    pub instrument_type: String,
    pub isin: Option<String>,
    pub listing_date: Option<String>,
    pub status: String,
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub lot_size: Option<i64>,
    pub source: String,
    pub source_symbol: String,
    pub metadata: Vec<MetadataEntry>,
}

impl ListingSnapshot {
    pub fn equity(key: InstrumentKey, source: impl Into<String>, source_symbol: impl Into<String>) -> Self {
        let market = key.market;
        Self {
            key,
            exchange: Some(market.default_exchange().into()),
            name: None,
            name_en: None,
            currency: Some(market.default_currency().into()),
            instrument_type: "equity".into(),
            isin: None,
            listing_date: None,
            status: "active".into(),
            sector: None,
            industry: None,
            lot_size: None,
            source: source.into(),
            source_symbol: source_symbol.into(),
            metadata: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub key: String,
    pub value: Option<String>,
    pub source: Option<String>,
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarSnapshot {
    pub key: InstrumentKey,
    pub timeframe: Timeframe,
    pub ts: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub adj_close: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub source: String,
}

impl BarSnapshot {
    pub fn is_valid(&self) -> bool {
        self.open.is_finite()
            && self.high.is_finite()
            && self.low.is_finite()
            && self.close.is_finite()
            && self.high >= self.low
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub id: i64,
    pub instrument_id: i64,
    pub timeframe: Timeframe,
    pub ts: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub adj_close: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub source: String,
    pub ingested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteSnapshot {
    pub key: InstrumentKey,
    pub ts: DateTime<Utc>,
    pub last: f64,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub prev_close: Option<f64>,
    pub volume: Option<f64>,
    #[serde(default)]
    pub turnover: Option<f64>,
    pub change_pct: Option<f64>,
    pub currency: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub instrument_id: i64,
    pub ts: String,
    pub last: f64,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub prev_close: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub change_pct: Option<f64>,
    pub currency: Option<String>,
    pub source: String,
    pub ingested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreshnessRow {
    pub instrument_id: i64,
    pub symbol: String,
    pub market: String,
    pub dataset: String,
    pub source: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub watermark_ts: Option<String>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRun {
    pub id: i64,
    pub kind: String,
    pub source: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub instruments_upserted: i64,
    pub bars_upserted: i64,
    pub quotes_upserted: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestReport {
    pub run_id: i64,
    pub kind: String,
    pub source: String,
    pub status: String,
    pub instruments_upserted: u32,
    pub bars_upserted: u32,
    pub quotes_upserted: u32,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_parse() {
        assert_eq!("CN.SH".parse::<Market>().unwrap(), Market::CnSh);
        assert_eq!("sz".parse::<Market>().unwrap(), Market::CnSz);
        assert_eq!("CN.BJ".parse::<Market>().unwrap(), Market::CnBj);
        assert!("CN".parse::<Market>().is_err());
    }

    #[test]
    fn timeframe_parse() {
        assert_eq!("1d".parse::<Timeframe>().unwrap(), Timeframe::D1);
        assert_eq!("1wk".parse::<Timeframe>().unwrap(), Timeframe::W1);
        assert_eq!(Timeframe::D1.dataset_key(), "bars:1d");
    }
}
