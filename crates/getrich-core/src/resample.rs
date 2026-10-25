//! Resample daily OHLCV into weekly / monthly bars.

use crate::models::{Bar, Timeframe};
use chrono::{Datelike, NaiveDate};

#[derive(Debug, Clone)]
struct Ohlcv {
    ts: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: Option<f64>,
    turnover: Option<f64>,
}

fn parse_date(ts: &str) -> Option<NaiveDate> {
    let s = ts.trim();
    let day = s.get(..10).unwrap_or(s);
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(s, "%Y%m%d").ok())
}

fn group_key(date: NaiveDate, tf: Timeframe) -> Option<i64> {
    match tf {
        Timeframe::W1 => {
            let iso = date.iso_week();
            Some(i64::from(iso.year()) * 100 + i64::from(iso.week()))
        }
        Timeframe::Mo1 => Some(i64::from(date.year()) * 100 + i64::from(date.month())),
        _ => None,
    }
}

fn resample_ohlcv(daily: &[Ohlcv], tf: Timeframe) -> Vec<Ohlcv> {
    if daily.is_empty() || group_key(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), tf).is_none() {
        return Vec::new();
    }
    let mut indexed: Vec<(i64, &Ohlcv)> = daily
        .iter()
        .filter_map(|b| {
            parse_date(&b.ts)
                .and_then(|d| group_key(d, tf))
                .map(|k| (k, b))
        })
        .collect();
    indexed.sort_by_key(|(k, b)| (*k, b.ts.clone()));

    let mut out = Vec::new();
    let mut cur_key: Option<i64> = None;
    let mut acc: Option<Ohlcv> = None;
    for (key, bar) in indexed {
        if cur_key != Some(key) {
            if let Some(done) = acc.take() {
                out.push(done);
            }
            cur_key = Some(key);
            acc = Some(Ohlcv {
                ts: bar.ts.clone(),
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                volume: bar.volume,
                turnover: bar.turnover,
            });
        } else if let Some(a) = acc.as_mut() {
            a.high = a.high.max(bar.high);
            a.low = a.low.min(bar.low);
            a.close = bar.close;
            a.ts = bar.ts.clone();
            a.volume = match (a.volume, bar.volume) {
                (Some(x), Some(y)) => Some(x + y),
                (Some(x), None) => Some(x),
                (None, Some(y)) => Some(y),
                (None, None) => None,
            };
            a.turnover = match (a.turnover, bar.turnover) {
                (Some(x), Some(y)) => Some(x + y),
                (Some(x), None) => Some(x),
                (None, Some(y)) => Some(y),
                (None, None) => None,
            };
        }
    }
    if let Some(done) = acc {
        out.push(done);
    }
    out
}

pub fn resample_bars(daily: &[Bar], tf: Timeframe) -> Vec<Bar> {
    let src: Vec<Ohlcv> = daily
        .iter()
        .map(|b| Ohlcv {
            ts: b.ts.clone(),
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
            turnover: b.turnover,
        })
        .collect();
    resample_ohlcv(&src, tf)
        .into_iter()
        .map(|o| Bar {
            id: 0,
            instrument_id: daily.first().map(|b| b.instrument_id).unwrap_or(0),
            timeframe: tf,
            ts: o.ts,
            open: o.open,
            high: o.high,
            low: o.low,
            close: o.close,
            adj_close: None,
            volume: o.volume,
            turnover: o.turnover,
            source: "resample".into(),
            ingested_at: String::new(),
        })
        .collect()
}

pub fn resample_imports(
    daily: &[crate::import::BarImport],
    tf: Timeframe,
) -> Vec<crate::import::BarImport> {
    let src: Vec<Ohlcv> = daily
        .iter()
        .map(|b| Ohlcv {
            ts: b.ts.clone(),
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
            turnover: b.turnover,
        })
        .collect();
    let tf_s = tf.as_str().to_string();
    resample_ohlcv(&src, tf)
        .into_iter()
        .map(|o| {
            let template = daily.first();
            crate::import::BarImport {
                symbol: template.map(|b| b.symbol.clone()).unwrap_or_default(),
                market: template.and_then(|b| b.market.clone()),
                timeframe: Some(tf_s.clone()),
                ts: o.ts,
                open: o.open,
                high: o.high,
                low: o.low,
                close: o.close,
                adj_close: None,
                volume: o.volume,
                turnover: o.turnover,
                source: Some("resample".into()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::BarImport;

    fn d(ts: &str, open: f64, high: f64, low: f64, close: f64) -> BarImport {
        BarImport {
            symbol: "000001".into(),
            market: Some("CN.SZ".into()),
            timeframe: Some("1d".into()),
            ts: ts.into(),
            open,
            high,
            low,
            close,
            adj_close: None,
            volume: Some(10.0),
            turnover: Some(100.0),
            source: Some("t".into()),
        }
    }

    #[test]
    fn weekly_groups_iso_week() {
        // 2024-01-02 Tue .. 2024-01-05 Fri = same ISO week; 2024-01-08 Mon = next.
        let daily = vec![
            d("2024-01-02", 10.0, 11.0, 9.0, 10.5),
            d("2024-01-03", 10.5, 12.0, 10.0, 11.0),
            d("2024-01-08", 11.0, 11.5, 10.5, 11.2),
        ];
        let w = resample_imports(&daily, Timeframe::W1);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].open, 10.0);
        assert_eq!(w[0].high, 12.0);
        assert_eq!(w[0].low, 9.0);
        assert_eq!(w[0].close, 11.0);
        assert_eq!(w[0].volume, Some(20.0));
        assert_eq!(w[1].open, 11.0);
        assert_eq!(w[1].close, 11.2);
    }

    #[test]
    fn monthly_groups_calendar_month() {
        let daily = vec![
            d("2024-01-31", 1.0, 2.0, 1.0, 1.5),
            d("2024-02-01", 1.5, 3.0, 1.4, 2.0),
        ];
        let m = resample_imports(&daily, Timeframe::Mo1);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].close, 1.5);
        assert_eq!(m[1].open, 1.5);
        assert_eq!(m[1].high, 3.0);
    }
}
