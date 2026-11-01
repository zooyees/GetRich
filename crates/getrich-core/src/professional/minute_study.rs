use super::*;
use crate::models::{Bar, InstrumentKey, Timeframe};
use chrono::Timelike;
use std::collections::BTreeMap;

pub(super) fn load(
    app: &App,
    key: &InstrumentKey,
    to: &str,
) -> Result<(Vec<Bar>, Value), AppError> {
    if !key.market.as_str().starts_with("CN.") {
        return Err(invalid("当前分钟资料源仅支持沪深京"));
    }
    let research = app.stock_research(key)?;
    let dataset = &research["datasets"]["intraday"];
    let mut ordered = BTreeMap::new();
    let mut excluded = 0;
    for row in dataset["rows"].as_array().into_iter().flatten() {
        let parsed = (|| {
            let ts = session::parse_cn_ts(text(row, "time")).ok()?;
            let clock = ts.with_timezone(&session::shanghai_tz());
            let day = clock.date_naive().to_string();
            let minute = clock.hour() * 60 + clock.minute();
            let slot = match minute {
                570..=690 => minute - 570,
                780..=900 => minute - 660,
                _ => return None,
            };
            if ts > chrono::Utc::now()
                || day.as_str() > to
                || row["minute"].as_u64() != Some(slot as u64)
            {
                return None;
            }
            let number = |f| row[f].as_f64().filter(|n| n.is_finite());
            let open = number("open")?;
            let high = number("high")?;
            let low = number("low")?;
            let close = number("close")?;
            if low <= 0.0 || low > open.min(close) || high < open.max(close) {
                return None;
            }
            if number("volume").is_some_and(|v| v < 0.0)
                || number("amount").is_some_and(|v| v < 0.0)
            {
                return None;
            }
            Some((
                ts,
                day,
                slot,
                Bar {
                    id: 0,
                    instrument_id: 0,
                    timeframe: Timeframe::M1,
                    ts: clock.format("%Y-%m-%d %H:%M:%S").to_string(),
                    open,
                    high,
                    low,
                    close,
                    adj_close: None,
                    volume: number("volume"),
                    turnover: number("amount"),
                    source: text(dataset, "source").into(),
                    ingested_at: text(dataset, "updated_at").into(),
                },
            ))
        })();
        if let Some((ts, day, slot, bar)) = parsed {
            ordered.insert(ts, (day, slot, bar));
        } else {
            excluded += 1;
        }
    }
    let mut bars = vec![];
    let mut resets = vec![0usize];
    let mut gaps = 0u64;
    let mut previous = None::<(String, u32)>;
    for (_, (day, slot, bar)) in ordered {
        if let Some((last_day, last_slot)) = &previous {
            if *last_day == day && slot > last_slot + 1 {
                gaps += (slot - last_slot - 1) as u64;
                resets.push(bars.len());
            }
        }
        previous = Some((day, slot));
        bars.push(bar);
    }
    if bars.is_empty() {
        return Err(invalid(
            "没有有效分钟OHLC。请先在个股分时更新已授权分钟资料；日线不会代替分钟。",
        ));
    }
    let meta = json!({"source":dataset["source"],"updated_at":dataset["updated_at"],"as_of":dataset["as_of"],"status":dataset["status"],"timeframe":"1m","basis":"raw","excluded":excluded,"missing_minutes":gaps,"resets":resets,"warning":format!("分钟原始价格，使用已落盘的分钟资料；排除{excluded}条无效/未来数据，已知日内缺口{gaps}分钟。缺口后指标重新预热，不填日线；隔夜按有效分钟序列延续。")});
    Ok((bars, meta))
}
