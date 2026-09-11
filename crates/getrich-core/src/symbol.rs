//! Symbol / market normalization (US / HK / A-share / 北证).

use crate::models::{InstrumentKey, Market};

/// Parse a user-supplied ticker into a canonical key.
///
/// Accepts `AAPL`, `US:AAPL`, `AAPL.US`, `600519`, `600519.SS`, `SH600519`,
/// `000001.SZ`, `0700.HK`, `00700`, `HK:700`, `830001.BJ`.
pub fn parse_instrument(raw: &str, market_hint: Option<&str>) -> Result<InstrumentKey, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty symbol".into());
    }

    if let Some((left, right)) = raw.split_once(':') {
        let market: Market = left.parse()?;
        return Ok(InstrumentKey::new(
            canonicalize_symbol(right, market)?,
            market,
        ));
    }

    let upper = raw.to_ascii_uppercase();
    if let Some(key) = parse_suffixed(&upper) {
        return Ok(key);
    }
    if let Some(key) = parse_prefixed(&upper) {
        return Ok(key);
    }

    let market = if let Some(hint) = market_hint {
        hint.parse()?
    } else {
        infer_market(&upper)?
    };
    Ok(InstrumentKey::new(canonicalize_symbol(&upper, market)?, market))
}

fn parse_suffixed(upper: &str) -> Option<InstrumentKey> {
    let (stem, market) = if let Some(s) = upper.strip_suffix(".SS") {
        (s, Market::CnSh)
    } else if let Some(s) = upper.strip_suffix(".SH") {
        (s, Market::CnSh)
    } else if let Some(s) = upper.strip_suffix(".SZ") {
        (s, Market::CnSz)
    } else if let Some(s) = upper.strip_suffix(".BJ") {
        (s, Market::CnBj)
    } else if let Some(s) = upper.strip_suffix(".HK") {
        (s, Market::Hk)
    } else if let Some(s) = upper.strip_suffix(".US") {
        (s, Market::Us)
    } else {
        return None;
    };
    canonicalize_symbol(stem, market)
        .ok()
        .map(|symbol| InstrumentKey::new(symbol, market))
}

fn parse_prefixed(upper: &str) -> Option<InstrumentKey> {
    let (stem, market) = if let Some(s) = upper.strip_prefix("SH") {
        if s.chars().all(|c| c.is_ascii_digit()) && s.len() == 6 {
            (s, Market::CnSh)
        } else {
            return None;
        }
    } else if let Some(s) = upper.strip_prefix("SZ") {
        if s.chars().all(|c| c.is_ascii_digit()) && s.len() == 6 {
            (s, Market::CnSz)
        } else {
            return None;
        }
    } else if let Some(s) = upper.strip_prefix("BJ") {
        if s.chars().all(|c| c.is_ascii_digit()) && s.len() == 6 {
            (s, Market::CnBj)
        } else {
            return None;
        }
    } else {
        return None;
    };
    canonicalize_symbol(stem, market)
        .ok()
        .map(|symbol| InstrumentKey::new(symbol, market))
}

fn infer_market(upper: &str) -> Result<Market, String> {
    if upper.chars().all(|c| c.is_ascii_digit()) {
        match upper.len() {
            6 if upper.starts_with('6') || upper.starts_with("900") => Ok(Market::CnSh),
            6 if upper.starts_with('8') || upper.starts_with('4') => Ok(Market::CnBj),
            6 => Ok(Market::CnSz),
            1..=5 => Ok(Market::Hk),
            _ => Err(format!("cannot infer market for numeric symbol {upper}")),
        }
    } else {
        Ok(Market::Us)
    }
}

pub fn canonicalize_symbol(raw: &str, market: Market) -> Result<String, String> {
    let s = raw.trim().to_ascii_uppercase();
    if s.is_empty() {
        return Err("empty symbol".into());
    }
    match market {
        Market::Us => {
            if !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            {
                return Err(format!("invalid US symbol: {s}"));
            }
            Ok(s)
        }
        Market::Hk => {
            let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return Err(format!("invalid HK symbol: {s}"));
            }
            let n: u32 = digits
                .parse()
                .map_err(|_| format!("invalid HK symbol: {s}"))?;
            Ok(format!("{n:05}"))
        }
        Market::CnSh | Market::CnSz | Market::CnBj => {
            let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() != 6 {
                return Err(format!("A-share symbols must be 6 digits, got {s}"));
            }
            Ok(digits)
        }
    }
}

/// Tonghuashun-style board id inferred from stored market + symbol (display filter, not a data source).
pub fn board_id(key: &InstrumentKey) -> &'static str {
    match key.market {
        Market::Us => "US",
        Market::Hk => "HK",
        Market::CnBj => "CN.BJ",
        Market::CnSh if key.symbol.starts_with("688") || key.symbol.starts_with("689") => "STAR",
        Market::CnSz
            if key.symbol.starts_with("300")
                || key.symbol.starts_with("301")
                || key.symbol.starts_with("302") =>
        {
            "CHINEXT"
        }
        Market::CnSh => "CN.SH",
        Market::CnSz => "CN.SZ",
    }
}

pub fn board_label_zh(board: &str) -> &'static str {
    match board {
        "US" => "美股",
        "HK" => "港股",
        "CN.SH" => "沪市",
        "CN.SZ" => "深市",
        "CN.BJ" => "北证",
        "STAR" => "科创板",
        "CHINEXT" => "创业板",
        "WATCH" => "自选",
        _ => "全部",
    }
}

pub fn board_matches(key: &InstrumentKey, board: &str) -> bool {
    let b = board.trim().to_ascii_uppercase();
    if b.is_empty() || b == "ALL" || b == "全部" {
        return true;
    }
    match b.as_str() {
        "US" | "美股" => key.market == Market::Us,
        "HK" | "港股" => key.market == Market::Hk,
        "CN.SH" | "SH" | "沪市" => key.market == Market::CnSh,
        "CN.SZ" | "SZ" | "深市" => key.market == Market::CnSz,
        "CN.BJ" | "BJ" | "北证" => key.market == Market::CnBj,
        "STAR" | "KCB" | "科创板" => board_id(key) == "STAR",
        "CHINEXT" | "CYB" | "创业板" => board_id(key) == "CHINEXT",
        "CN" | "A" | "沪深京" => matches!(
            key.market,
            Market::CnSh | Market::CnSz | Market::CnBj
        ),
        _ => board_id(key).eq_ignore_ascii_case(&b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_us_and_cn_and_hk() {
        let a = parse_instrument("AAPL", None).unwrap();
        assert_eq!(a.symbol, "AAPL");
        assert_eq!(a.market, Market::Us);

        let mt = parse_instrument("600519", None).unwrap();
        assert_eq!(mt.symbol, "600519");
        assert_eq!(mt.market, Market::CnSh);

        let ping = parse_instrument("000001.SZ", None).unwrap();
        assert_eq!(ping.symbol, "000001");
        assert_eq!(ping.market, Market::CnSz);

        let tencent = parse_instrument("700.HK", None).unwrap();
        assert_eq!(tencent.symbol, "00700");
        assert_eq!(tencent.market, Market::Hk);

        let sh = parse_instrument("SH600519", None).unwrap();
        assert_eq!(sh.market, Market::CnSh);

        let bj = parse_instrument("830001.BJ", None).unwrap();
        assert_eq!(bj.market, Market::CnBj);
        assert_eq!(board_id(&bj), "CN.BJ");
    }

    #[test]
    fn star_and_chinext_boards() {
        let star = parse_instrument("688981", None).unwrap();
        assert_eq!(board_id(&star), "STAR");
        let cyb = parse_instrument("300750", None).unwrap();
        assert_eq!(board_id(&cyb), "CHINEXT");
        assert!(board_matches(&star, "科创板"));
        assert!(!board_matches(&cyb, "STAR"));
    }
}
