//! Daily SMA values, aligned to the final daily observation in each display bar.
use crate::models::Bar;
use serde::{Deserialize, Serialize};
pub const MA_PERIODS: [usize; 6] = [5, 10, 20, 30, 120, 250];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovingAverage {
    pub period: usize,
    pub values: Vec<Option<f64>>,
}
pub fn daily_averages(daily: &[Bar], display: &[Bar]) -> Vec<MovingAverage> {
    MA_PERIODS
        .into_iter()
        .map(|period| {
            let mut sum = 0.0;
            let mut series = Vec::with_capacity(daily.len());
            for (i, bar) in daily.iter().enumerate() {
                sum += bar.close;
                if i >= period {
                    sum -= daily[i - period].close;
                }
                series.push((i + 1 >= period).then_some(sum / period as f64));
            }
            let values = display
                .iter()
                .map(|bar| {
                    let date = bar.ts.get(..10).unwrap_or(&bar.ts);
                    let end = daily.partition_point(|b| b.ts.get(..10).unwrap_or(&b.ts) <= date);
                    end.checked_sub(1).and_then(|i| series[i])
                })
                .collect();
            MovingAverage { period, values }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bars() -> Vec<Bar> {
        (0..300)
            .map(|i| Bar {
                id: i,
                instrument_id: 1,
                timeframe: crate::models::Timeframe::D1,
                ts: (chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                    + chrono::Duration::days(i))
                .to_string(),
                open: i as f64 + 1.0,
                high: i as f64 + 2.0,
                low: i as f64 + 0.5,
                close: i as f64 + 1.0,
                adj_close: None,
                volume: None,
                turnover: None,
                source: "fixture".into(),
                ingested_at: String::new(),
            })
            .collect()
    }
    #[test]
    fn all_six_periods_use_complete_daily_history_and_never_look_ahead() {
        let daily = bars();
        let display: Vec<_> = [0, 28, 29, 119, 249, 299]
            .into_iter()
            .map(|i| daily[i].clone())
            .collect();
        let ma = daily_averages(&daily, &display);
        assert_eq!(ma.len(), 6);
        let values = |p| &ma.iter().find(|m| m.period == p).unwrap().values;
        assert_eq!(values(30)[1], None);
        assert_eq!(values(30)[2], Some(15.5));
        assert_eq!(values(120)[3], Some(60.5));
        assert_eq!(values(250)[4], Some(125.5));
        assert_eq!(values(250)[5], Some(175.5));
        assert_eq!(values(5)[2], Some(28.0));
        assert_eq!(values(250)[0], None);
        let weekly: Vec<_> = daily.iter().step_by(7).cloned().collect();
        let aligned = daily_averages(&daily, &weekly);
        let year = &aligned.iter().find(|m| m.period == 250).unwrap().values;
        assert_eq!(year[36], Some(128.5));
    }
}
