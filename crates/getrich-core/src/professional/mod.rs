//! Professional workstation services. Local records and computations, no order routing.
mod alerts;
mod backtest;
mod catalog;
mod data;
mod minute_study;
mod pit;
mod portfolio;
mod research;
mod studies;
use crate::{db, error::AppError, service::App, session};
use serde_json::{json, Value};

pub(super) fn sql<T>(r: rusqlite::Result<T>) -> Result<T, AppError> {
    r.map_err(db::DbError::from).map_err(AppError::from)
}
pub(super) fn text<'a>(p: &'a Value, k: &str) -> &'a str {
    p[k].as_str().unwrap_or("").trim()
}
pub(super) fn num(p: &Value, k: &str, default: f64) -> Result<f64, AppError> {
    let raw = &p[k];
    if raw.is_null() || raw.as_str() == Some("") {
        return Ok(default);
    }
    raw.as_f64()
        .or_else(|| raw.as_str().and_then(|s| s.parse().ok()))
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(&format!("{k} 必须是有效数字")))
}
pub(super) fn flag(p: &Value, k: &str) -> bool {
    p[k].as_bool()
        .unwrap_or_else(|| matches!(text(p, k), "true" | "1" | "是"))
}
pub(super) fn invalid(message: &str) -> AppError {
    AppError::InvalidArg(message.into())
}
pub(super) fn day(raw: &str) -> Result<String, AppError> {
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .or_else(|_| chrono::NaiveDate::parse_from_str(raw, "%Y%m%d"))
        .map(|d| d.to_string())
        .map_err(|_| invalid("日期应为 YYYY-MM-DD"))
}
pub(super) fn today() -> String {
    session::iso_date(session::shanghai_today())
}
pub(super) fn range(p: &Value) -> Result<(String, String), AppError> {
    let to = if text(p, "to").is_empty() {
        today()
    } else {
        day(text(p, "to"))?
    };
    let from = if text(p, "from").is_empty() {
        (session::shanghai_today() - chrono::Duration::days(365)).to_string()
    } else {
        day(text(p, "from"))?
    };
    if from > to || to > today() {
        return Err(invalid("日期区间无效或超过今天"));
    }
    Ok((from, to))
}
pub(super) fn hash(s: &str) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}
pub(super) fn view(title: &str, summary: Value, tables: Vec<Value>, notices: Vec<String>) -> Value {
    json!({"title":title,"summary":summary,"tables":tables,"notices":notices})
}
pub(super) fn table(title: &str, columns: &[(&str, &str)], rows: Vec<Value>) -> Value {
    json!({"title":title,"columns":columns.iter().map(|(key,label)|json!({"key":key,"label":label})).collect::<Vec<_>>(),"rows":rows})
}
pub fn execute(app: &App, tool: &str, p: &Value) -> Result<Value, AppError> {
    match tool {
        "catalog" => Ok(catalog::catalog()),
        "overview" => research::overview(app),
        "data.import" => data::import(app, p),
        "data.quality" => data::quality(app, p),
        "data.sources" => data::sources(app, p),
        "data.revisions" => data::revisions(app, p),
        "data.sync" => {
            let job = app.submit_job("professional.sync", p.clone())?;
            Ok(
                json!({"title":"资料更新已提交","job_id":job["id"],"summary":[{"label":"任务","value":job["id"]}],"notices":["在后台补齐复权、交易日历、涨跌停与公司行为；接口无权限会保留错误，不生成假数据。"]}),
            )
        }
        "study.chart" => studies::chart(app, p),
        "study.compare" => studies::compare(app, p),
        "study.peers" => studies::peers(app, p),
        "study.scenario" => studies::scenario(app, p),
        "study.events" => studies::events(app, p),
        "definition.save" => research::save(app, p),
        "screen.save" | "pool.save" | "template.save" | "study.save" | "strategy.save" => {
            let mut p = p.clone();
            p["kind"] = json!(tool.split('.').next().unwrap());
            research::save(app, &p)
        }
        "definition.list" => research::list(app, p),
        "definition.history" => research::history(app, p),
        "screen.run" => research::screen(app, p),
        "notes.search" => research::notes(app, p),
        "template.apply" => research::apply_template(app, p),
        "portfolio.import" => portfolio::import(app, p),
        "portfolio.accounts" => portfolio::accounts(app),
        "portfolio.report" => portfolio::report(app, p),
        "portfolio.correct" => portfolio::correct(app, p),
        "alerts.save" => alerts::save(app, p),
        "alerts.evaluate" => alerts::evaluate(app),
        "alerts.list" => alerts::list(app),
        "alerts.badge" => alerts::badge(app),
        "alerts.ack" => alerts::ack(app, p),
        "backtest.run" => {
            let job = app.submit_job("professional.backtest", p.clone())?;
            Ok(
                json!({"title":"回测已进入后台队列","job_id":job["id"],"notices":["冻结输入数据、参数和约束，完成后显示交易明细与成本敏感性。"]}),
            )
        }
        "backtest.history" => backtest::history(app, p),
        "backtest.inputs" => backtest::inputs(app, p),
        "backtest.replay" => {
            let job = app.submit_job("professional.replay", p.clone())?;
            Ok(json!({"title":"冻结输入重放已提交","job_id":job["id"]}))
        }
        _ => Err(invalid("未知专业工作台功能")),
    }
}
pub fn run_backtest(app: &App, p: &Value) -> Result<Value, AppError> {
    backtest::run(app, p)
}
pub fn replay_backtest(app: &App, p: &Value) -> Result<Value, AppError> {
    backtest::replay(app, p)
}
impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn list_bars_price(
        &self,
        key: &crate::models::InstrumentKey,
        timeframe: crate::models::Timeframe,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
        basis: &str,
        source: &str,
    ) -> Result<crate::view::BarPage, AppError> {
        use crate::models::Timeframe;
        if basis == "raw" || basis.is_empty() {
            return self.list_bars(key, timeframe, from, to, limit);
        }
        if !matches!(timeframe, Timeframe::D1 | Timeframe::W1 | Timeframe::Mo1) {
            return Err(invalid("复权图表仅支持日、周、月线"));
        }
        let to = to.map(day).transpose()?.unwrap_or_else(today);
        if to > today() {
            return Err(invalid("图表截止日不能超过今天"));
        }
        let from = from.map(day).transpose()?;
        let (daily, adjustment, version) = data::adjusted_snapshot(self, key, &to, basis, source)?;
        let display: Vec<_> = daily
            .iter()
            .filter(|b| from.as_ref().is_none_or(|f| b.ts.as_str() >= f.as_str()))
            .cloned()
            .collect();
        let mut rows = if timeframe == Timeframe::D1 {
            display
        } else {
            crate::resample::resample_bars(&display, timeframe)
        };
        let limit = if limit == 0 { 500 } else { limit.min(20000) };
        if rows.len() > limit {
            rows = rows[rows.len() - limit..].to_vec();
        }
        let today = session::shanghai_today();
        let partial = rows
            .last()
            .and_then(|b| session::calendar_date_of(&b.ts))
            .is_some_and(|d| {
                use chrono::Datelike;
                match timeframe {
                    Timeframe::W1 => d.iso_week() == today.iso_week(),
                    Timeframe::Mo1 => d.year() == today.year() && d.month() == today.month(),
                    _ => false,
                }
            });
        Ok(crate::view::BarPage {
            price_basis: basis.into(),
            adjustment,
            moving_averages: crate::indicators::daily_averages(&daily, &rows),
            basis_timeframe: "1d".into(),
            partial_last_period: partial,
            data_version: version,
            symbol: key.symbol.clone(),
            market: key.market.as_str().into(),
            timeframe: timeframe.as_str().into(),
            count: rows.len(),
            from: rows.first().map(|b| b.ts.clone()),
            to: rows.last().map(|b| b.ts.clone()),
            empty: rows.is_empty(),
            empty_message: if rows.is_empty() {
                "该区间没有日线".into()
            } else {
                String::new()
            },
            rows,
        })
    }
}
pub(crate) fn overlay_research(
    app: &App,
    key: &crate::models::InstrumentKey,
    datasets: &mut serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    data::overlay_research(app, key, datasets)
}
pub(crate) fn sync(app: &App, id: i64, p: &Value) -> Result<Value, AppError> {
    data::sync(app, id, p)
}
pub(crate) fn capture_research(
    conn: &rusqlite::Connection,
    key: &crate::models::InstrumentKey,
    dataset: &str,
    source: &str,
    rows: &[Value],
) -> Result<(), AppError> {
    data::capture_research(conn, key, dataset, source, rows)
}
pub(crate) fn start(app: &std::sync::Arc<App>) {
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("getrich-professional".into())
        .spawn(move || {
            while !app.ingest.stop.load(std::sync::atomic::Ordering::SeqCst) {
                if let Err(e) = alerts::evaluate(&app) {
                    tracing::warn!(%e, "professional alerts");
                }
                if let Ok(conn) = app.db.connect() {
                    // The expression index keeps retention bounded as the archive grows.
                    let expired = conn.query_row("SELECT EXISTS(SELECT 1 FROM quote_samples WHERE julianday(ts)<julianday('now','-7 days'))", [], |r|r.get::<_,bool>(0)).unwrap_or(false);
                    if expired {
                        let _ = conn.execute("DELETE FROM quote_samples WHERE rowid IN (SELECT rowid FROM quote_samples WHERE julianday(ts)<julianday('now','-7 days') LIMIT 10000)", []);
                    }
                }
                for _ in 0..50 {
                    if app.ingest.stop.load(std::sync::atomic::Ordering::SeqCst) { return; }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        })
    {
        tracing::error!(%e, "professional worker start");
    }
}
