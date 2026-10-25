//! Asia/Shanghai session clock (no DST; UTC+8).

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc, Weekday,
};
use serde::{Deserialize, Serialize};

/// Beijing / Shanghai offset. China does not observe DST.
pub fn shanghai_tz() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("UTC+8")
}

pub fn shanghai_from_utc(utc: DateTime<Utc>) -> DateTime<FixedOffset> {
    utc.with_timezone(&shanghai_tz())
}

pub fn shanghai_now() -> DateTime<FixedOffset> {
    shanghai_from_utc(Utc::now())
}

pub fn shanghai_today() -> NaiveDate {
    shanghai_now().date_naive()
}

/// US equity timestamps use the federal DST rule effective since 2007.
/// Older US intraday records retain UTC rather than guessing historical rules.
pub fn market_offset(market: &str, utc: DateTime<Utc>) -> FixedOffset {
    if market != "US" {
        return shanghai_tz();
    }
    let year = utc.year();
    if year < 2007 {
        return FixedOffset::east_opt(0).expect("UTC");
    }
    let sunday = |month: u32, n: u32| {
        NaiveDate::from_ymd_opt(year, month, 1).and_then(|first| {
            first.checked_add_signed(Duration::days(
                ((7 - first.weekday().num_days_from_sunday()) % 7 + 7 * (n - 1)) as i64,
            ))
        })
    };
    let start = sunday(3, 2)
        .and_then(|d| d.and_hms_opt(7, 0, 0))
        .map(|d| Utc.from_utc_datetime(&d));
    let end = sunday(11, 1)
        .and_then(|d| d.and_hms_opt(6, 0, 0))
        .map(|d| Utc.from_utc_datetime(&d));
    let daylight = start.zip(end).is_some_and(|(a, b)| utc >= a && utc < b);
    FixedOffset::west_opt(if daylight { 4 * 3600 } else { 5 * 3600 }).expect("US offset")
}

pub fn market_date_at(market: &str, utc: DateTime<Utc>) -> NaiveDate {
    utc.with_timezone(&market_offset(market, utc)).date_naive()
}

pub fn market_timestamp(value: &str, market: &str) -> String {
    parse_cn_ts(value)
        .map(|t| {
            t.with_timezone(&market_offset(market, t))
                .format("%Y-%m-%d %H:%M:%S UTC%:z")
                .to_string()
        })
        .unwrap_or_else(|_| value.into())
}

#[cfg(test)]
mod market_time_tests {
    use super::*;
    #[test]
    fn new_york_dst_boundaries_and_cross_day() {
        for (text, seconds) in [
            ("2026-03-08T06:59:59Z", -18000),
            ("2026-03-08T07:00:00Z", -14400),
            ("2026-11-01T05:59:59Z", -14400),
            ("2026-11-01T06:00:00Z", -18000),
        ] {
            assert_eq!(
                market_offset("US", parse_cn_ts(text).unwrap()).local_minus_utc(),
                seconds
            );
        }
        let t = parse_cn_ts("2026-07-02T00:30:00Z").unwrap();
        assert_eq!(iso_date(market_date_at("US", t)), "2026-07-01");
        assert_eq!(iso_date(market_date_at("CN.SH", t)), "2026-07-02");
        assert_eq!(
            calendar_date_of("2026-10-22T23:30:00Z"),
            NaiveDate::from_ymd_opt(2026, 10, 23)
        );
    }
}

pub fn shanghai_date_at(utc: DateTime<Utc>) -> NaiveDate {
    shanghai_from_utc(utc).date_naive()
}

pub fn yyyymmdd(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

pub fn iso_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Continuous auction plus a small poll buffer: 09:10–11:35 and 12:55–15:10, weekdays.
pub fn in_poll_window_at(utc: DateTime<Utc>) -> bool {
    let sh = shanghai_from_utc(utc);
    if !is_weekday(sh.weekday()) {
        return false;
    }
    let t = sh.time();
    in_range(t, 9, 10, 11, 35) || in_range(t, 12, 55, 15, 10)
}

/// Official continuous auction: 09:15–11:30 / 13:00–15:00, weekdays.
pub fn in_auction_at(utc: DateTime<Utc>) -> bool {
    let sh = shanghai_from_utc(utc);
    if !is_weekday(sh.weekday()) {
        return false;
    }
    let t = sh.time();
    in_range(t, 9, 15, 11, 30) || in_range(t, 13, 0, 15, 0)
}

pub fn in_poll_window() -> bool {
    in_poll_window_at(Utc::now())
}

fn is_weekday(d: Weekday) -> bool {
    !matches!(d, Weekday::Sat | Weekday::Sun)
}

fn in_range(t: NaiveTime, h0: u32, m0: u32, h1: u32, m1: u32) -> bool {
    let start = NaiveTime::from_hms_opt(h0, m0, 0).unwrap();
    let end = NaiveTime::from_hms_opt(h1, m1, 0).unwrap();
    t >= start && t <= end
}

/// Parse a China-naive datetime as Shanghai, then convert to UTC.
/// Date-only values stay on that calendar day (UTC midnight) so daily bar keys do not shift.
pub fn parse_cn_ts(raw: &str) -> Result<DateTime<Utc>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty timestamp".into());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return midnight_utc(date, s);
    }
    if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(date) = NaiveDate::parse_from_str(s, "%Y%m%d") {
            return midnight_utc(date, s);
        }
    }
    let naive = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d %H:%M:%S"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .map_err(|_| format!("invalid timestamp (use RFC3339 or YYYY-MM-DD): {s}"))?;
    shanghai_tz()
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| format!("ambiguous Shanghai timestamp: {s}"))
}

fn midnight_utc(date: NaiveDate, raw: &str) -> Result<DateTime<Utc>, String> {
    let ndt = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| format!("invalid date: {raw}"))?;
    Ok(Utc.from_utc_datetime(&ndt))
}

/// Calendar date of a Shanghai-naive or RFC3339 timestamp (for today's 1d bar key).
pub fn calendar_date_of(raw: &str) -> Option<NaiveDate> {
    let s = raw.trim();
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(s) {
        return Some(shanghai_date_at(timestamp.with_timezone(&Utc)));
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(date);
    }
    if let Some(prefix) = s.get(..10) {
        if let Ok(date) = NaiveDate::parse_from_str(prefix, "%Y-%m-%d") {
            return Some(date);
        }
    }
    if s.len() >= 8 && s.as_bytes()[..8].iter().all(|b| b.is_ascii_digit()) {
        return NaiveDate::parse_from_str(&s[..8], "%Y%m%d").ok();
    }
    parse_cn_ts(s).ok().map(shanghai_date_at)
}

pub fn days_before(date: NaiveDate, days: i64) -> NaiveDate {
    date - Duration::days(days)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Pre,
    Auction,
    Open,
    Morning,
    Lunch,
    Afternoon,
    Close,
    Post,
}

impl SessionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pre => "pre",
            Self::Auction => "auction",
            Self::Open => "open",
            Self::Morning => "morning",
            Self::Lunch => "lunch",
            Self::Afternoon => "afternoon",
            Self::Close => "close",
            Self::Post => "post",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Pre => "未开盘",
            Self::Auction => "竞价",
            Self::Open => "开盘",
            Self::Morning => "午前",
            Self::Lunch => "午休",
            Self::Afternoon => "午后",
            Self::Close => "尾盘",
            Self::Post => "收盘",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Open | Self::Morning | Self::Afternoon | Self::Close
        )
    }

    pub fn frozen(self) -> bool {
        matches!(self, Self::Lunch)
    }

    /// Persist once per trade date for rank_change / 20d amount.
    /// Open uses a replaceable snapshot (see `replace_anchor`); Lunch/Post freeze.
    pub fn persist_anchor(self) -> bool {
        matches!(self, Self::Lunch | Self::Post)
    }
}

/// Clock phase. `trading_day` false → always `post` (use previous session's tape).
pub fn phase_at(utc: DateTime<Utc>, trading_day: bool) -> SessionPhase {
    if !trading_day {
        return SessionPhase::Post;
    }
    let t = shanghai_from_utc(utc).time();
    if t < t_hms(9, 15) {
        SessionPhase::Pre
    } else if t < t_hms(9, 30) {
        SessionPhase::Auction
    } else if t < t_hms(10, 0) {
        SessionPhase::Open
    } else if t < t_hms(11, 30) {
        SessionPhase::Morning
    } else if t < t_hms(13, 0) {
        SessionPhase::Lunch
    } else if t < t_hms(14, 45) {
        SessionPhase::Afternoon
    } else if t < t_hms(15, 0) {
        SessionPhase::Close
    } else {
        SessionPhase::Post
    }
}

fn t_hms(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

pub fn is_weekday_date(date: NaiveDate) -> bool {
    is_weekday(date.weekday())
}

pub fn previous_weekday(mut date: NaiveDate) -> NaiveDate {
    date -= Duration::days(1);
    while !is_weekday_date(date) {
        date -= Duration::days(1);
    }
    date
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn shanghai_morning_is_same_trade_date() {
        let utc = Utc.with_ymd_and_hms(2026, 9, 11, 1, 20, 0).unwrap();
        assert_eq!(shanghai_date_at(utc).to_string(), "2026-09-11");
        assert!(in_poll_window_at(utc));
        assert!(in_auction_at(utc));
    }

    #[test]
    fn utc_calendar_would_be_wrong_before_eight() {
        let utc = Utc.with_ymd_and_hms(2026, 9, 11, 1, 20, 0).unwrap();
        assert_eq!(utc.date_naive().to_string(), "2026-09-11");
        let early = Utc.with_ymd_and_hms(2026, 9, 10, 16, 30, 0).unwrap();
        assert_eq!(early.date_naive().to_string(), "2026-09-10");
        assert_eq!(shanghai_date_at(early).to_string(), "2026-09-11");
    }

    #[test]
    fn weekend_is_closed() {
        let sat = Utc.with_ymd_and_hms(2026, 9, 12, 2, 0, 0).unwrap();
        assert!(!in_poll_window_at(sat));
        assert!(!in_auction_at(sat));
    }

    #[test]
    fn lunch_is_closed() {
        let lunch = Utc.with_ymd_and_hms(2026, 9, 11, 4, 0, 0).unwrap();
        assert_eq!(shanghai_from_utc(lunch).time().to_string(), "12:00:00");
        assert!(!in_poll_window_at(lunch));
    }

    #[test]
    fn naive_trade_time_is_shanghai() {
        let ts = parse_cn_ts("2026-09-11 10:00:00").unwrap();
        assert_eq!(
            ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-09-11T02:00:00Z"
        );
    }

    #[test]
    fn date_only_keeps_calendar_day() {
        let ts = parse_cn_ts("2024-01-02").unwrap();
        assert_eq!(ts.format("%Y-%m-%d").to_string(), "2024-01-02");
    }

    #[test]
    fn phases_on_trade_day() {
        let open = Utc.with_ymd_and_hms(2026, 9, 11, 1, 40, 0).unwrap();
        assert_eq!(phase_at(open, true), SessionPhase::Open);
        assert!(phase_at(open, true).is_open());
        let lunch = Utc.with_ymd_and_hms(2026, 9, 11, 4, 0, 0).unwrap();
        assert_eq!(phase_at(lunch, true), SessionPhase::Lunch);
        assert!(phase_at(lunch, true).frozen());
        let post = Utc.with_ymd_and_hms(2026, 9, 11, 8, 0, 0).unwrap();
        assert_eq!(phase_at(post, true), SessionPhase::Post);
        let weekend = Utc.with_ymd_and_hms(2026, 9, 12, 2, 0, 0).unwrap();
        assert_eq!(phase_at(weekend, false), SessionPhase::Post);
    }
}
