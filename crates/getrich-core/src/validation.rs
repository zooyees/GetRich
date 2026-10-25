//! Shared input validation. Invalid vendor/file rows never reach analytics.
use crate::models::QuoteSnapshot;
use chrono::NaiveDate;

pub fn date(value: &str) -> Result<String, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y%m%d"))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| format!("无效日期：{value}（使用 YYYY-MM-DD）"))
}

pub fn quote(q: &QuoteSnapshot) -> Result<(), String> {
    if !q.last.is_finite() || q.last <= 0.0 {
        return Err("last 必须为正数".into());
    }
    for (name, value) in [
        ("open", q.open),
        ("high", q.high),
        ("low", q.low),
        ("prev_close", q.prev_close),
    ] {
        if value.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err(format!("{name} 必须为正数或留空"));
        }
    }
    for (name, value) in [("volume", q.volume), ("turnover", q.turnover)] {
        if value.is_some_and(|v| !v.is_finite() || v < 0.0) {
            return Err(format!("{name} 必须非负"));
        }
    }
    if q.high
        .is_some_and(|h| h < q.last || q.open.is_some_and(|o| h < o))
        || q.low
            .is_some_and(|l| l > q.last || q.open.is_some_and(|o| l > o))
        || matches!((q.high,q.low),(Some(h),Some(l)) if h < l)
    {
        return Err("OHLC 关系无效：最低 ≤ 开盘/最新 ≤ 最高".into());
    }
    if let Some(pct) = q.change_pct {
        if !pct.is_finite() {
            return Err("change_pct 不是有限数值".into());
        }
        if let Some(prev) = q.prev_close {
            let calculated = (q.last - prev) * 100.0 / prev;
            if (pct - calculated).abs() > 0.1 {
                return Err(format!(
                    "change_pct 与最新价/昨收不一致（计算值 {calculated:.4}%）"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn malformed_dates_are_safe() {
        for value in ["中文ab", "2026-99-99", "", "🙂ab"] {
            assert!(super::date(value).is_err());
        }
        assert_eq!(super::date("20261024").unwrap(), "2026-10-24");
    }
}
