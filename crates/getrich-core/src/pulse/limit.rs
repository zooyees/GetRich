use crate::models::InstrumentKey;
use crate::models::Market;
use crate::symbol::board_id;

/// Limit-up threshold in percent. `None` = IPO day, do not estimate.
pub fn limit_pct(
    name: Option<&str>,
    market: &str,
    symbol: &str,
    listing_date: Option<&str>,
    trade_date: &str,
) -> Option<f64> {
    if let Some(ld) = listing_date {
        let iso = crate::validation::date(ld).ok()?;
        if iso == trade_date {
            return None;
        }
    }
    let name_u = name.unwrap_or("").to_ascii_uppercase();
    if name_u.contains("ST") {
        return Some(5.0);
    }
    let market: Market = market.parse().ok()?;
    let key = InstrumentKey::new(symbol, market);
    match board_id(&key) {
        "STAR" | "CHINEXT" => Some(20.0),
        "CN.BJ" => Some(30.0),
        _ => Some(10.0),
    }
}

pub fn name_has_delist(name: Option<&str>) -> bool {
    name.unwrap_or("").contains('退')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn st_is_five() {
        assert_eq!(
            limit_pct(Some("*ST宁科"), "CN.SH", "600001", None, "2026-09-12"),
            Some(5.0)
        );
    }

    #[test]
    fn chinext_is_twenty() {
        assert_eq!(
            limit_pct(Some("宁德时代"), "CN.SZ", "300750", None, "2026-09-12"),
            Some(20.0)
        );
    }

    #[test]
    fn star_is_twenty() {
        assert_eq!(
            limit_pct(Some("中芯国际"), "CN.SH", "688981", None, "2026-09-12"),
            Some(20.0)
        );
    }

    #[test]
    fn ipo_excluded() {
        assert_eq!(
            limit_pct(
                Some("新股"),
                "CN.SH",
                "601234",
                Some("2026-09-12"),
                "2026-09-12"
            ),
            None
        );
    }

    #[test]
    fn main_board_ten() {
        assert_eq!(
            limit_pct(Some("贵州茅台"), "CN.SH", "600519", None, "2026-09-12"),
            Some(10.0)
        );
    }
}
