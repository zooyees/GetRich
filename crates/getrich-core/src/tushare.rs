//! Tushare Pro HTTP client (https://tushare.pro).
//!
//! Token is read from config / `GETRICH_TUSHARE_TOKEN`. Never log the secret.

use crate::config::TushareConfig;
use crate::error::AppError;
use crate::import::{BarImport, ImportBundle, ListingImport, QuoteImport};
use crate::models::InstrumentKey;
use crate::session;
use crate::symbol::parse_instrument;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://api.tushare.pro";

fn collect_pages(
    api: &str,
    mut params: Value,
    mut fetch: impl FnMut(Value) -> Result<Table, AppError>,
) -> Result<Table, AppError> {
    let mut result: Option<Table> = None;
    let mut seen = std::collections::HashSet::new();
    for offset in (0..100_000).step_by(6000) {
        params["limit"] = json!(6000);
        params["offset"] = json!(offset);
        let page = fetch(params.clone())?;
        let count = page.items.len();
        if count == 0 {
            return Ok(result.unwrap_or(page));
        }
        let target = result.get_or_insert_with(|| Table {
            fields: page.fields.clone(),
            items: Vec::new(),
        });
        if target.fields != page.fields {
            return Err(AppError::Message(format!(
                "{api} 分页字段不一致，未提交不完整结果"
            )));
        }
        for row in page.items {
            if !seen.insert(row.to_string()) {
                return Err(AppError::Message(format!(
                    "{api} 分页重复，未提交不完整结果"
                )));
            }
            target.items.push(row);
        }
        if count < 6000 {
            return Ok(result.unwrap());
        }
    }
    Err(AppError::Message(format!(
        "{api} 触及分页上限，未提交不完整结果"
    )))
}

#[derive(Debug)]
struct TokenBucket {
    per_min: u32,
    hits: Mutex<VecDeque<u64>>,
}

impl TokenBucket {
    fn new(per_min: u32) -> Self {
        Self {
            per_min: per_min.max(1),
            hits: Mutex::new(VecDeque::new()),
        }
    }

    fn try_take_at(&self, now_ms: u64) -> Result<(), u64> {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        admit(&mut hits, now_ms, self.per_min)
    }

    fn wait(&self) {
        loop {
            let now = now_ms();
            match self.try_take_at(now) {
                Ok(()) => return,
                Err(wait_ms) => thread::sleep(Duration::from_millis(wait_ms.max(20))),
            }
        }
    }

    fn try_wait(&self) -> bool {
        self.try_take_at(now_ms()).is_ok()
    }
}

/// Sliding 60s window. Returns `Err(wait_ms)` when the bucket is empty.
pub fn admit(hits: &mut VecDeque<u64>, now_ms: u64, per_min: u32) -> Result<(), u64> {
    let window = 60_000u64;
    while hits
        .front()
        .map(|t| now_ms.saturating_sub(*t) >= window)
        .unwrap_or(false)
    {
        hits.pop_front();
    }
    if (hits.len() as u32) < per_min.max(1) {
        hits.push_back(now_ms);
        return Ok(());
    }
    let oldest = hits.front().copied().unwrap_or(now_ms);
    Err(window.saturating_sub(now_ms.saturating_sub(oldest)).max(1))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug)]
struct Backoff {
    until: Option<Instant>,
    delay: Duration,
}

impl Backoff {
    fn remaining(&self) -> Option<Duration> {
        let until = self.until?;
        until.checked_duration_since(Instant::now())
    }

    fn trip(&mut self) {
        let next = (self.delay * 2).min(Duration::from_secs(60));
        self.delay = if self.delay < Duration::from_secs(1) {
            Duration::from_secs(1)
        } else {
            next
        };
        self.until = Some(Instant::now() + self.delay);
    }

    fn reset(&mut self) {
        self.until = None;
        self.delay = Duration::from_millis(500);
    }
}

#[derive(Debug)]
pub struct Client {
    agent: ureq::Agent,
    shared_store: Option<crate::db::Database>,
    account: String,
    pub user_id: String,
    token: String,
    timeout: Duration,
    live_timeout: Duration,
    rt_k_limit: TokenBucket,
    backoff: Mutex<Backoff>,
    last_quote_api: Mutex<Option<String>>,
}

impl Client {
    pub fn new(
        user_id: String,
        token: String,
        timeout_ms: u64,
        live_timeout_ms: u64,
        rate_per_min: u32,
    ) -> Result<Self, AppError> {
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(AppError::InvalidArg(
                "未配置 Tushare token。请写入 config.json data.tushare.token 或环境变量 GETRICH_TUSHARE_TOKEN。"
                    .into(),
            ));
        }
        Ok(Self {
            agent: ureq::AgentBuilder::new().build(),
            shared_store: None,
            account: {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                token.hash(&mut h);
                format!("{:016x}", h.finish())
            },
            user_id: user_id.trim().to_string(),
            token,
            timeout: Duration::from_millis(timeout_ms.max(90_000)),
            live_timeout: Duration::from_millis(live_timeout_ms.max(1_000)),
            rt_k_limit: TokenBucket::new(rate_per_min.max(1)),
            backoff: Mutex::new(Backoff {
                until: None,
                delay: Duration::from_millis(500),
            }),
            last_quote_api: Mutex::new(None),
        })
    }

    pub fn from_cfg(cfg: &TushareConfig, read_timeout_ms: u64) -> Result<Self, AppError> {
        Self::new(
            cfg.user_id.clone(),
            cfg.token.clone(),
            read_timeout_ms,
            cfg.live_timeout_ms,
            cfg.rate_per_min,
        )
    }

    pub fn last_quote_api(&self) -> Option<String> {
        self.last_quote_api.lock().ok().and_then(|g| g.clone())
    }

    pub(crate) fn with_shared_store(mut self, store: crate::db::Database) -> Self {
        self.shared_store = Some(store);
        self
    }

    fn shared_take_at(&self, now: u64) -> Result<u64, AppError> {
        let Some(store) = &self.shared_store else {
            return Ok(0);
        };
        let mut conn = store.connect()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(crate::db::DbError::from)?;
        let blocked: u64 = tx
            .query_row(
                "SELECT COALESCE(MAX(until_ms),0) FROM provider_backoff WHERE account=?1",
                [&self.account],
                |r| r.get(0),
            )
            .map_err(crate::db::DbError::from)?;
        if blocked > now {
            return Ok(blocked - now);
        }
        tx.execute(
            "DELETE FROM provider_requests WHERE account=?1 AND requested_at_ms<=?2",
            rusqlite::params![self.account, now.saturating_sub(60_000)],
        )
        .map_err(crate::db::DbError::from)?;
        let (count,oldest):(u32,u64)=tx.query_row("SELECT COUNT(*),COALESCE(MIN(requested_at_ms),0) FROM provider_requests WHERE account=?1",[&self.account],|r|Ok((r.get(0)?,r.get(1)?))).map_err(crate::db::DbError::from)?;
        if count >= self.rt_k_limit.per_min {
            return Ok(oldest.saturating_add(60_000).saturating_sub(now).max(1));
        }
        tx.execute(
            "INSERT INTO provider_requests(account,requested_at_ms) VALUES (?1,?2)",
            rusqlite::params![self.account, now],
        )
        .map_err(crate::db::DbError::from)?;
        tx.commit().map_err(crate::db::DbError::from)?;
        Ok(0)
    }

    fn shared_block(&self, until: u64) -> Result<(), AppError> {
        if let Some(store) = &self.shared_store {
            store.connect()?.execute("INSERT INTO provider_backoff(account,until_ms) VALUES (?1,?2) ON CONFLICT(account) DO UPDATE SET until_ms=MAX(until_ms,excluded.until_ms)",rusqlite::params![self.account,until]).map_err(crate::db::DbError::from)?;
        }
        Ok(())
    }

    pub fn backoff_remaining(&self) -> Option<Duration> {
        self.backoff.lock().ok().and_then(|g| g.remaining())
    }

    pub fn call(&self, api_name: &str, params: Value, fields: &str) -> Result<Table, AppError> {
        self.call_inner(api_name, params, fields, false, false)
    }

    pub fn call_live(
        &self,
        api_name: &str,
        params: Value,
        fields: &str,
    ) -> Result<Table, AppError> {
        self.call_inner(api_name, params, fields, true, true)
    }

    #[allow(dead_code)]
    fn call_optional(&self, api_name: &str, params: Value, fields: &str) -> Option<Table> {
        match self.call_inner(api_name, params, fields, true, false) {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::debug!(api = api_name, error = %e, "Tushare optional call skipped");
                None
            }
        }
    }

    fn call_inner(
        &self,
        api_name: &str,
        params: Value,
        fields: &str,
        live: bool,
        wait_rate: bool,
    ) -> Result<Table, AppError> {
        if live {
            if let Some(wait) = self.backoff_remaining() {
                return Err(AppError::Message(format!(
                    "Tushare {api_name}: 限流退避 {}s",
                    wait.as_secs().max(1)
                )));
            }
            if wait_rate {
                self.rt_k_limit.wait();
            } else if !self.rt_k_limit.try_wait() {
                return Err(AppError::Message(format!(
                    "Tushare {api_name}: 客户端频次已满"
                )));
            }
        }
        let wait = self.shared_take_at(now_ms())?;
        if wait > 0 {
            return Err(AppError::Message(format!(
                "Tushare 账号共享频次/退避预算已满，请在 {} 秒后重试",
                wait.div_ceil(1000)
            )));
        }
        let body = json!({
            "api_name": api_name,
            "token": self.token,
            "params": params,
            "fields": fields,
        });
        let timeout = if live {
            self.live_timeout
        } else {
            self.timeout
        };
        let resp: Value = self
            .agent
            .post(ENDPOINT)
            .timeout(timeout)
            .send_json(body)
            .map_err(|e| AppError::Message(format!("Tushare 请求失败: {e}")))?
            .into_json()
            .map_err(|e| AppError::Message(format!("Tushare 响应不是 JSON: {e}")))?;
        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = resp
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("Tushare 返回错误");
            let err = AppError::Message(format!("Tushare {api_name}: {msg}"));
            if is_rate_limited(&err) {
                self.shared_block(now_ms().saturating_add(60_000))?;
                if let Ok(mut b) = self.backoff.lock() {
                    b.trip();
                }
            }
            return Err(err);
        }
        if let Ok(mut b) = self.backoff.lock() {
            b.reset();
        }
        let data = resp.get("data").cloned().unwrap_or(json!({}));
        let fields = data
            .get("fields")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let items = data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(Table { fields, items })
    }
}

pub fn is_rate_limited(err: &AppError) -> bool {
    let s = err.to_string();
    let lower = s.to_ascii_lowercase();
    lower.contains("429")
        || s.contains("频率")
        || s.contains("限流")
        || lower.contains("too many")
        || lower.contains("freq")
        || lower.contains("rate limit")
}

#[derive(Debug, Clone)]
pub struct Table {
    pub fields: Vec<String>,
    pub items: Vec<Value>,
}

impl Table {
    pub fn col(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f == name)
    }

    pub fn cell(&self, row: &Value, name: &str) -> Option<Value> {
        let i = self.col(name)?;
        row.as_array()?.get(i).cloned()
    }

    pub fn str(&self, row: &Value, name: &str) -> Option<String> {
        match self.cell(row, name)? {
            Value::String(s) if !s.is_empty() => Some(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn f64(&self, row: &Value, name: &str) -> Option<f64> {
        match self.cell(row, name)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}

/// `600519.SH` → CN.SH / 600519.
pub fn ts_code_to_key(ts_code: &str) -> Result<InstrumentKey, String> {
    let ts = ts_code.trim().to_ascii_uppercase();
    let (stem, market) = if let Some(s) = ts.strip_suffix(".SH") {
        (s, "CN.SH")
    } else if let Some(s) = ts.strip_suffix(".SZ") {
        (s, "CN.SZ")
    } else if let Some(s) = ts.strip_suffix(".BJ") {
        (s, "CN.BJ")
    } else if let Some(s) = ts.strip_suffix(".SS") {
        (s, "CN.SH")
    } else {
        return Err(format!("unsupported ts_code {ts_code}"));
    };
    parse_instrument(stem, Some(market))
}

pub fn key_to_ts_code(key: &InstrumentKey) -> Option<String> {
    let suffix = match key.market.as_str() {
        "CN.SH" => "SH",
        "CN.SZ" => "SZ",
        "CN.BJ" => "BJ",
        _ => return None,
    };
    Some(format!("{}.{}", key.symbol, suffix))
}

pub fn yyyymmdd_to_iso(raw: &str) -> String {
    let s = raw.trim();
    if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8])
    } else {
        s.to_string()
    }
}

pub fn mask_token(token: &str) -> String {
    let t = token.trim();
    if t.len() <= 8 {
        return if t.is_empty() {
            "未配置".into()
        } else {
            "已配置".into()
        };
    }
    format!("····{}", &t[t.len() - 4..])
}

#[derive(Debug, Clone, Copy)]
enum VolUnit {
    Daily,
    Realtime,
}

fn scale_vol(v: Option<f64>, unit: VolUnit) -> Option<f64> {
    match unit {
        VolUnit::Daily => v.map(|x| x * 100.0),
        VolUnit::Realtime => v,
    }
}

fn scale_amount(v: Option<f64>, unit: VolUnit) -> Option<f64> {
    match unit {
        VolUnit::Daily => v.map(|x| x * 1000.0),
        VolUnit::Realtime => v,
    }
}

pub struct MappedQuotes {
    pub quotes: Vec<QuoteImport>,
    pub bars: Vec<BarImport>,
}

/// Map `rt_k` rows: volume/amount already in 股/元; keep full trade_time.
pub fn map_rt_k(table: &Table) -> MappedQuotes {
    let mut quotes = Vec::new();
    let mut bars = Vec::new();
    for row in &table.items {
        let Some(ts_code) = table.str(row, "ts_code") else {
            continue;
        };
        let Ok(key) = ts_code_to_key(&ts_code) else {
            continue;
        };
        let Some(last) = table.f64(row, "close") else {
            continue;
        };
        let trade_time = table
            .str(row, "trade_time")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                session::shanghai_now()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            });
        let prev = table.f64(row, "pre_close");
        let change_pct = table
            .f64(row, "pct_chg")
            .or_else(|| prev.filter(|p| *p != 0.0).map(|p| (last - p) / p * 100.0));
        let volume = scale_vol(table.f64(row, "vol"), VolUnit::Realtime);
        let turnover = scale_amount(table.f64(row, "amount"), VolUnit::Realtime);
        quotes.push(QuoteImport {
            symbol: key.symbol.clone(),
            market: Some(key.market.as_str().into()),
            ts: trade_time.clone(),
            last,
            open: table.f64(row, "open"),
            high: table.f64(row, "high"),
            low: table.f64(row, "low"),
            prev_close: prev,
            volume,
            turnover,
            change_pct,
            currency: Some("CNY".into()),
            source: Some("tushare.rt_k".into()),
        });
        if let (Some(open), Some(high), Some(low)) = (
            table.f64(row, "open"),
            table.f64(row, "high"),
            table.f64(row, "low"),
        ) {
            let day = session::calendar_date_of(&trade_time)
                .map(session::iso_date)
                .unwrap_or_else(|| session::iso_date(session::shanghai_today()));
            bars.push(BarImport {
                symbol: key.symbol,
                market: Some(key.market.as_str().into()),
                timeframe: Some("1d".into()),
                ts: day,
                open,
                high,
                low,
                close: last,
                adj_close: None,
                volume,
                turnover,
                source: Some("tushare.rt_k".into()),
            });
        }
    }
    MappedQuotes { quotes, bars }
}

pub fn map_daily_row(
    table: &Table,
    row: &Value,
    key: &InstrumentKey,
) -> Option<(QuoteImport, Option<BarImport>)> {
    let close = table.f64(row, "close")?;
    let iso = yyyymmdd_to_iso(&table.str(row, "trade_date").unwrap_or_default());
    if iso.is_empty() {
        return None;
    }
    let volume = scale_vol(table.f64(row, "vol"), VolUnit::Daily);
    let turnover = scale_amount(table.f64(row, "amount"), VolUnit::Daily);
    let quote = QuoteImport {
        symbol: key.symbol.clone(),
        market: Some(key.market.as_str().into()),
        ts: iso.clone(),
        last: close,
        open: table.f64(row, "open"),
        high: table.f64(row, "high"),
        low: table.f64(row, "low"),
        prev_close: table.f64(row, "pre_close"),
        volume,
        turnover,
        change_pct: table.f64(row, "pct_chg"),
        currency: Some("CNY".into()),
        source: Some("tushare.daily".into()),
    };
    let bar = match (
        table.f64(row, "open"),
        table.f64(row, "high"),
        table.f64(row, "low"),
    ) {
        (Some(open), Some(high), Some(low)) => Some(BarImport {
            symbol: key.symbol.clone(),
            market: Some(key.market.as_str().into()),
            timeframe: Some("1d".into()),
            ts: iso,
            open,
            high,
            low,
            close,
            adj_close: None,
            volume,
            turnover,
            source: Some("tushare.daily".into()),
        }),
        _ => None,
    };
    Some((quote, bar))
}

impl Client {
    pub fn last_open_day(&self) -> Result<String, AppError> {
        self.recent_open_days()?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Message("交易日历未返回有效交易日".into()))
    }
    fn recent_open_days(&self) -> Result<Vec<String>, AppError> {
        let today = session::shanghai_today();
        let cal=self.call("trade_cal",json!({"exchange":"SSE","is_open":"1","start_date":session::yyyymmdd(session::days_before(today,21)),"end_date":session::yyyymmdd(today)}),"cal_date,is_open")?;
        let mut dates: Vec<_> = cal
            .items
            .iter()
            .filter_map(|row| cal.str(row, "cal_date"))
            .filter(|d| chrono::NaiveDate::parse_from_str(d, "%Y%m%d").is_ok())
            .collect();
        dates.sort_unstable_by(|a, b| b.cmp(a));
        dates.dedup();
        Ok(dates)
    }
    fn paged_table(&self, api: &str, params: Value, fields: &str) -> Result<Table, AppError> {
        collect_pages(api, params, |params| self.call(api, params, fields))
    }

    pub fn rt_k_table(&self, ts_code: &str) -> Result<Table, AppError> {
        self.call_live(
            "rt_k",
            json!({ "ts_code": ts_code }),
            "ts_code,pre_close,high,open,low,close,vol,amount,trade_time,pct_chg",
        )
    }

    pub fn board_bundle(&self, include_listings: bool) -> Result<ImportBundle, AppError> {
        let listings = if include_listings {
            self.listings_bundle()?.listings
        } else {
            Vec::new()
        };
        let mut latest = None;
        for trade_date in self.recent_open_days()?.into_iter().take(6) {
            let daily = self.paged_table(
                "daily",
                json!({"trade_date":trade_date}),
                "ts_code,trade_date,open,high,low,close,pre_close,change,pct_chg,vol,amount",
            )?;
            if !daily.items.is_empty() {
                latest = Some(daily);
                break;
            }
        }
        let daily = latest.ok_or_else(|| {
            AppError::Message("最近交易日均无日线行情，请检查 daily 权限或等待收盘数据发布".into())
        })?;
        let mut quotes = Vec::new();
        let mut bars = Vec::new();
        for row in &daily.items {
            let Some(ts) = daily.str(row, "ts_code") else {
                continue;
            };
            let Ok(key) = ts_code_to_key(&ts) else {
                continue;
            };
            if let Some((q, b)) = map_daily_row(&daily, row, &key) {
                quotes.push(q);
                if let Some(bar) = b {
                    bars.push(bar);
                }
            }
        }
        if quotes.is_empty() {
            return Err(AppError::Message(
                "daily 未返回可用报价，请检查数据格式与权限".into(),
            ));
        }
        Ok(ImportBundle {
            listings,
            quotes,
            bars,
        })
    }

    pub fn listings_bundle(&self) -> Result<ImportBundle, AppError> {
        let basics = self.paged_table(
            "stock_basic",
            json!({ "list_status": "L" }),
            "ts_code,symbol,name,list_status,list_date,industry",
        )?;
        let mut listings = Vec::new();
        for row in &basics.items {
            let Some(ts) = basics.str(row, "ts_code") else {
                continue;
            };
            let Ok(key) = ts_code_to_key(&ts) else {
                continue;
            };
            listings.push(ListingImport {
                symbol: key.symbol.clone(),
                market: Some(key.market.as_str().into()),
                exchange: Some(key.market.default_exchange().into()),
                name: basics.str(row, "name"),
                name_en: None,
                currency: Some("CNY".into()),
                instrument_type: Some("equity".into()),
                isin: None,
                listing_date: basics.str(row, "list_date").map(|s| yyyymmdd_to_iso(&s)),
                status: Some("active".into()),
                sector: None,
                industry: basics.str(row, "industry"),
                lot_size: Some(100),
                source: Some("tushare".into()),
                source_symbol: Some(ts),
                metadata: Vec::new(),
            });
        }
        if listings.is_empty() {
            return Err(AppError::Message(
                "stock_basic 未返回可用股票列表，请检查接口权限".into(),
            ));
        }
        Ok(ImportBundle {
            listings,
            ..ImportBundle::default()
        })
    }

    pub fn bars_bundle(
        &self,
        key: &InstrumentKey,
        start: &str,
        end: &str,
    ) -> Result<ImportBundle, AppError> {
        self.bars_bundle_api(key, start, end, "daily")
    }
    pub(crate) fn bars_bundle_api(
        &self,
        key: &InstrumentKey,
        start: &str,
        end: &str,
        api: &str,
    ) -> Result<ImportBundle, AppError> {
        let Some(ts_code) = key_to_ts_code(key) else {
            return Ok(ImportBundle::default());
        };
        let daily = self.paged_table(
            api,
            json!({
                "ts_code": ts_code,
                "start_date": start,
                "end_date": end,
            }),
            "ts_code,trade_date,open,high,low,close,pre_close,vol,amount",
        )?;
        let mut bars = Vec::new();
        for row in &daily.items {
            if let Some((_, Some(bar))) = map_daily_row(&daily, row, key) {
                bars.push(bar);
            }
        }
        let mut extra = crate::resample::resample_imports(&bars, crate::models::Timeframe::W1);
        extra.extend(crate::resample::resample_imports(
            &bars,
            crate::models::Timeframe::Mo1,
        ));
        bars.extend(extra);

        Ok(ImportBundle {
            listings: Vec::new(),
            quotes: Vec::new(),
            bars,
        })
    }

    pub fn quote_bundle(&self, key: &InstrumentKey) -> Result<ImportBundle, AppError> {
        let Some(ts_code) = key_to_ts_code(key) else {
            return Ok(ImportBundle::default());
        };
        match self.call_live(
            "rt_k",
            json!({ "ts_code": ts_code }),
            "ts_code,pre_close,high,open,low,close,vol,amount,trade_time,pct_chg",
        ) {
            Ok(table) => {
                let mapped = map_rt_k(&table);
                if !mapped.quotes.is_empty() {
                    if let Ok(mut api) = self.last_quote_api.lock() {
                        *api = Some("rt_k".into());
                    }
                    return Ok(ImportBundle {
                        quotes: mapped.quotes,
                        bars: mapped.bars,
                        listings: Vec::new(),
                    });
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "rt_k quote miss, falling back to daily");
            }
        }
        if let Ok(mut api) = self.last_quote_api.lock() {
            *api = Some("daily".into());
        }
        let today = session::shanghai_today();
        let end = session::yyyymmdd(today);
        let start = session::yyyymmdd(session::days_before(today, 12));
        let daily = self.call(
            "daily",
            json!({
                "ts_code": ts_code,
                "start_date": start,
                "end_date": end,
            }),
            "ts_code,trade_date,open,high,low,close,pre_close,vol,amount,pct_chg",
        )?;
        let quotes = latest_quote_from_daily(key, &daily);
        let mut bars = Vec::new();
        for row in &daily.items {
            if let Some((_, Some(bar))) = map_daily_row(&daily, row, key) {
                bars.push(bar);
            }
        }
        if bars.is_empty() {
            if let Some(q) = quotes.first() {
                if let (Some(open), Some(high), Some(low)) = (q.open, q.high, q.low) {
                    bars.push(BarImport {
                        symbol: key.symbol.clone(),
                        market: Some(key.market.as_str().into()),
                        timeframe: Some("1d".into()),
                        ts: q.ts.clone(),
                        open,
                        high,
                        low,
                        close: q.last,
                        adj_close: None,
                        volume: q.volume,
                        turnover: q.turnover,
                        source: Some("tushare.daily".into()),
                    });
                }
            }
        }
        Ok(ImportBundle {
            quotes,
            bars,
            listings: Vec::new(),
        })
    }
}

fn latest_quote_from_daily(key: &InstrumentKey, daily: &Table) -> Vec<QuoteImport> {
    let mut best: Option<QuoteImport> = None;
    for row in &daily.items {
        let Some((q, _)) = map_daily_row(daily, row, key) else {
            continue;
        };
        if best
            .as_ref()
            .map(|b| b.ts.as_str() < q.ts.as_str())
            .unwrap_or(true)
        {
            best = Some(q);
        }
    }
    best.into_iter().collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn upstream_pagination_keeps_6001_rows_and_accepts_empty_final_schema() {
        let fields = vec!["ts_code".to_string()];
        let result = super::collect_pages("daily", serde_json::json!({}), |p| {
            let offset = p["offset"].as_u64().unwrap() as usize;
            Ok(super::Table {
                fields: fields.clone(),
                items: (offset..(offset + 6000).min(6001))
                    .map(|n| serde_json::json!([format!("stock-{n}")]))
                    .collect(),
            })
        })
        .unwrap();
        assert_eq!(result.items.len(), 6001);
        let full = super::collect_pages("stock_basic", serde_json::json!({}), |p| {
            Ok(if p["offset"] == 0 {
                super::Table {
                    fields: fields.clone(),
                    items: (0..6000).map(|n| serde_json::json!([n])).collect(),
                }
            } else {
                super::Table {
                    fields: vec![],
                    items: vec![],
                }
            })
        })
        .unwrap();
        assert_eq!(full.items.len(), 6000);
    }
    #[test]
    fn upstream_repeated_page_is_an_error_instead_of_a_partial_success() {
        assert!(
            super::collect_pages("daily", serde_json::json!({}), |_| Ok(super::Table {
                fields: vec!["ts_code".into()],
                items: (0..6000).map(|n| serde_json::json!([n])).collect()
            }))
            .unwrap_err()
            .to_string()
            .contains("分页重复")
        );
    }

    use super::*;

    #[test]
    fn maps_sh_sz_bj() {
        let sh = ts_code_to_key("600519.SH").unwrap();
        assert_eq!(sh.symbol, "600519");
        assert_eq!(sh.market.as_str(), "CN.SH");
        let sz = ts_code_to_key("000001.SZ").unwrap();
        assert_eq!(sz.market.as_str(), "CN.SZ");
        let bj = ts_code_to_key("920002.BJ").unwrap();
        assert_eq!(bj.market.as_str(), "CN.BJ");
        assert_eq!(key_to_ts_code(&sh).as_deref(), Some("600519.SH"));
    }

    #[test]
    fn formats_trade_date() {
        assert_eq!(yyyymmdd_to_iso("20240115"), "2024-01-15");
    }

    #[test]
    fn rt_k_does_not_scale_volume() {
        let table = Table {
            fields: vec![
                "ts_code".into(),
                "pre_close".into(),
                "high".into(),
                "open".into(),
                "low".into(),
                "close".into(),
                "vol".into(),
                "amount".into(),
                "trade_time".into(),
            ],
            items: vec![json!([
                "600519.SH",
                1400.0,
                1500.0,
                1410.0,
                1405.0,
                1490.0,
                55_845_293.0,
                8.2e9,
                "2026-09-11 10:00:00"
            ])],
        };
        let mapped = map_rt_k(&table);
        assert_eq!(mapped.quotes[0].volume, Some(55_845_293.0));
        assert_eq!(mapped.quotes[0].turnover, Some(8.2e9));
        assert_eq!(mapped.quotes[0].ts, "2026-09-11 10:00:00");
        assert_eq!(mapped.bars[0].ts, "2026-09-11");
        assert_eq!(mapped.bars[0].volume, Some(55_845_293.0));
    }

    #[test]
    fn daily_scales_volume() {
        let key = ts_code_to_key("600519.SH").unwrap();
        let table = Table {
            fields: vec![
                "ts_code".into(),
                "trade_date".into(),
                "open".into(),
                "high".into(),
                "low".into(),
                "close".into(),
                "pre_close".into(),
                "vol".into(),
                "amount".into(),
                "pct_chg".into(),
            ],
            items: vec![json!([
                "600519.SH",
                "20260911",
                1410.0,
                1500.0,
                1405.0,
                1490.0,
                1400.0,
                558_452.93,
                8_200_000.0,
                6.43
            ])],
        };
        let (q, b) = map_daily_row(&table, &table.items[0], &key).unwrap();
        assert!((q.volume.unwrap() - 55_845_293.0).abs() < 1.0);
        assert!((q.turnover.unwrap() - 8.2e9).abs() < 1.0);
        assert_eq!(b.unwrap().ts, "2026-09-11");
    }

    #[test]
    fn token_bucket_caps_per_minute() {
        let mut hits = VecDeque::new();
        for i in 0..50 {
            admit(&mut hits, i * 10, 50).unwrap();
        }
        assert!(admit(&mut hits, 400, 50).is_err());
        admit(&mut hits, 60_000, 50).unwrap();
    }

    #[test]
    fn separate_clients_share_account_budget_and_backoff() {
        let path = std::env::temp_dir().join(format!(
            "getrich-provider-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let store = crate::db::Database::at(&path);
        let first = Client::new("test".into(), "fixture-token".into(), 1000, 1000, 2)
            .unwrap()
            .with_shared_store(store.clone());
        let second = Client::new("test".into(), "fixture-token".into(), 1000, 1000, 2)
            .unwrap()
            .with_shared_store(store);
        assert_eq!(first.shared_take_at(100_000).unwrap(), 0);
        assert_eq!(second.shared_take_at(100_001).unwrap(), 0);
        assert!(first.shared_take_at(100_002).unwrap() > 0);
        first.shared_block(200_000).unwrap();
        assert_eq!(second.shared_take_at(150_000).unwrap(), 50_000);
        assert_eq!(second.shared_take_at(200_000).unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }
}
