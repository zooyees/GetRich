//! Refresh whole display periods at the tail, preserving historical bars and SMA warmup.
use crate::view::BarPage;
use chrono::{Datelike, Duration, NaiveDate};

pub fn tail_from(timeframe: &str, last: &str) -> Option<String> {
    let date = NaiveDate::parse_from_str(last.get(..10)?, "%Y-%m-%d").ok()?;
    Some(
        match timeframe {
            "1w" => date - Duration::days(i64::from(date.weekday().num_days_from_monday())),
            "1mo" => date.with_day(1)?,
            "1d" => date,
            _ => return None,
        }
        .to_string(),
    )
}
pub fn merge_tail(base: &mut BarPage, tail: BarPage, from: &str) -> bool {
    if base.symbol != tail.symbol
        || base.market != tail.market
        || base.timeframe != tail.timeframe
        || tail.rows.is_empty()
        || tail.data_version < base.data_version
    {
        return false;
    }
    let prefix = base.rows.partition_point(|bar| bar.ts.as_str() < from);
    base.rows.truncate(prefix);
    let added = tail.rows.len();
    base.rows.extend(tail.rows);
    for ma in &mut base.moving_averages {
        ma.values.truncate(prefix);
        if let Some(next) = tail.moving_averages.iter().find(|m| m.period == ma.period) {
            ma.values.extend(next.values.iter().copied());
        } else {
            ma.values.extend(std::iter::repeat_n(None, added));
        }
    }
    base.data_version = tail.data_version;
    base.basis_timeframe = tail.basis_timeframe;
    base.partial_last_period = tail.partial_last_period;
    base.from = base.rows.first().map(|b| b.ts.clone());
    base.to = base.rows.last().map(|b| b.ts.clone());
    base.count = base.rows.len();
    base.empty = false;
    base.empty_message.clear();
    true
}
