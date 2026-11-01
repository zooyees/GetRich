//! Deterministic local analysis and append-only personal research. No vendor I/O.
use crate::{
    db,
    error::AppError,
    models::{Bar, InstrumentKey, Timeframe},
    service::App,
    session,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

fn ema(values: &[f64], n: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if values.len() < n {
        return out;
    }
    let mut v = values[..n].iter().sum::<f64>() / n as f64;
    out[n - 1] = Some(v);
    for i in n..values.len() {
        v += (values[i] - v) * 2.0 / (n + 1) as f64;
        out[i] = Some(v);
    }
    out
}
pub fn metrics(rows: &[Bar]) -> Value {
    let prices: Vec<_> = rows.iter().map(|b| b.close).collect();
    let n = prices.len();
    let ret = |p: usize| {
        if n > p {
            Some((prices[n - 1] / prices[n - 1 - p] - 1.0) * 100.0)
        } else {
            None
        }
    };
    let ma = |p: usize| {
        if n >= p {
            Some(prices[n - p..].iter().sum::<f64>() / p as f64)
        } else {
            None
        }
    };
    let mut rsi = None;
    let mut atr = None;
    if n >= 15 {
        let changes: Vec<_> = prices.windows(2).map(|w| w[1] - w[0]).collect();
        let mut gain = changes[..14].iter().map(|v| v.max(0.0)).sum::<f64>() / 14.0;
        let mut loss = changes[..14].iter().map(|v| (-v).max(0.0)).sum::<f64>() / 14.0;
        for c in &changes[14..] {
            gain = (gain * 13.0 + c.max(0.0)) / 14.0;
            loss = (loss * 13.0 + (-c).max(0.0)) / 14.0;
        }
        rsi = Some(if gain + loss == 0.0 {
            50.0
        } else {
            100.0 * gain / (gain + loss)
        });
        let tr: Vec<_> = rows
            .windows(2)
            .map(|w| {
                (w[1].high - w[1].low)
                    .max((w[1].high - w[0].close).abs())
                    .max((w[1].low - w[0].close).abs())
            })
            .collect();
        let mut v = tr[..14].iter().sum::<f64>() / 14.0;
        for t in &tr[14..] {
            v = (v * 13.0 + t) / 14.0;
        }
        atr = Some(v);
    }
    let e12 = ema(&prices, 12);
    let e26 = ema(&prices, 26);
    let dif: Vec<_> = e12
        .iter()
        .zip(&e26)
        .filter_map(|(a, b)| a.zip(*b).map(|(a, b)| a - b))
        .collect();
    let dea = ema(&dif, 9).last().copied().flatten();
    let last_dif = dif.last().copied();
    let vol = if n >= 21 {
        let v: Vec<_> = prices[n - 21..]
            .windows(2)
            .map(|w| (w[1] / w[0]).ln())
            .collect();
        let mean = v.iter().sum::<f64>() / 20.0;
        Some(
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / 19.0).sqrt()
                * 252f64.sqrt()
                * 100.0,
        )
    } else {
        None
    };
    let dd = if n >= 60 {
        let mut peak = prices[n - 60];
        let mut worst = 0.0f64;
        for p in &prices[n - 60..] {
            peak = peak.max(*p);
            worst = worst.min((p / peak - 1.0) * 100.0);
        }
        Some(worst)
    } else {
        None
    };
    let vr = if n >= 21 {
        rows[n - 21..n - 1]
            .iter()
            .map(|b| b.volume)
            .collect::<Option<Vec<_>>>()
            .and_then(|v| {
                let avg = v.iter().sum::<f64>() / 20.0;
                (avg > 0.0)
                    .then(|| rows[n - 1].volume.map(|v| v / avg))
                    .flatten()
            })
    } else {
        None
    };
    json!({"return_5_pct":ret(5),"return_20_pct":ret(20),"return_60_pct":ret(60),"ma20":ma(20),"ma60":ma(60),"rsi14":rsi,"macd_dif":last_dif,"macd_dea":dea,"macd_histogram":last_dif.zip(dea).map(|(d,s)|2.0*(d-s)),"atr14":atr,"volatility20_annual_pct":vol,"max_drawdown60_pct":dd,"volume_ratio20":vr})
}
pub fn research_mode(mode: Option<&str>) -> Result<&str, AppError> {
    let mode = mode.unwrap_or("swing");
    if !["intraday", "swing", "value"].contains(&mode) {
        return Err(AppError::InvalidArg(
            "研究方式只支持 intraday / swing / value".into(),
        ));
    }
    Ok(mode)
}
pub(crate) fn note_on(
    conn: &rusqlite::Connection,
    key: &InstrumentKey,
    mode: &str,
) -> Result<Value, AppError> {
    let result=conn.query_row("SELECT revision,thesis,risks,plan,review,updated_at,analysis_as_of,analysis_snapshot FROM research_journal WHERE market=?1 AND symbol=?2 AND mode=?3 ORDER BY revision DESC LIMIT 1",params![key.market.as_str(),key.symbol,mode],|r|Ok(json!({"mode":mode,"revision":r.get::<_,i64>(0)?,"thesis":r.get::<_,String>(1)?,"risks":r.get::<_,String>(2)?,"plan":r.get::<_,String>(3)?,"review":r.get::<_,String>(4)?,"updated_at":r.get::<_,String>(5)?,"analysis_as_of":r.get::<_,Option<String>>(6)?,"analysis_snapshot":serde_json::from_str::<Value>(&r.get::<_,String>(7)?).unwrap_or(Value::Null)}))).optional().map_err(db::DbError::from)?;
    Ok(result
        .unwrap_or(json!({"mode":mode,"revision":0,"thesis":"","risks":"","plan":"","review":""})))
}
impl App {
    pub fn stock_analysis(
        &self,
        key: &InstrumentKey,
        as_of: Option<&str>,
    ) -> Result<Value, AppError> {
        if let Some(d) = as_of {
            let parsed = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map_err(|_| AppError::InvalidArg("分析日期应为 YYYY-MM-DD".into()))?;
            if parsed > session::shanghai_today() {
                return Err(AppError::InvalidArg("不能分析未来日期".into()));
            }
        }
        let end = as_of
            .map(str::to_string)
            .unwrap_or_else(|| session::iso_date(session::shanghai_today()));
        let bars = self.list_bars(key, Timeframe::D1, None, Some(&end), 20000)?;
        let mut warnings = vec!["原始价格 / 未复权；跨除权区间的收益和波动可能失真".to_string()];
        if bars.count < 61 {
            warnings.push("历史不足：部分指标显示 —，不补零或推测".into());
        }
        if bars.to.as_deref().is_some_and(|d| d < &end) {
            warnings.push("分析基于本地最近可用日线，非实时行情承诺".into());
        }
        if bars.rows.last().is_some_and(|b| {
            b.ts.get(..10) == Some(session::iso_date(session::shanghai_today()).as_str())
        }) {
            warnings.push("最后一根可能尚未收盘，指标会随行情更新".into());
        }
        let sources: std::collections::BTreeSet<_> =
            bars.rows.iter().map(|b| b.source.clone()).collect();
        Ok(
            json!({"symbol":key.symbol,"market":key.market.as_str(),"as_of":bars.to,"from":bars.from,"sample_count":bars.count,"data_version":bars.data_version,"price_basis":"raw_unadjusted","sources":sources,"warnings":warnings,"metrics":metrics(&bars.rows),"methodology":"RSI/ATR: Wilder14；MACD: SMA种子EMA12/26/9，柱=2×(DIF-DEA)；波动：20个对数收益样本标准差×√252；回撤：最近60根收盘价；量比：最新量/此前20根均量（不是交易所实时量比）","evaluation_date":end,"mode":"swing","note":note_on(&self.db.connect()?,key,"swing")?}),
        )
    }
    pub fn research_notes(&self, p: &Value, history: bool) -> Result<Value, AppError> {
        let key = self.parse_key(p["symbol"].as_str().unwrap_or(""), p["market"].as_str())?;
        let mode = research_mode(p["mode"].as_str())?;
        let conn = self.db.connect()?;
        if !history {
            return note_on(&conn, &key, mode);
        }
        let mut stmt=conn.prepare("SELECT revision,thesis,risks,plan,review,updated_at,analysis_as_of,analysis_snapshot FROM research_journal WHERE market=?1 AND symbol=?2 AND mode=?3 ORDER BY revision DESC LIMIT 100").map_err(db::DbError::from)?;
        let rows=stmt.query_map(params![key.market.as_str(),key.symbol,mode],|r|Ok(json!({"mode":mode,"revision":r.get::<_,i64>(0)?,"thesis":r.get::<_,String>(1)?,"risks":r.get::<_,String>(2)?,"plan":r.get::<_,String>(3)?,"review":r.get::<_,String>(4)?,"updated_at":r.get::<_,String>(5)?,"analysis_as_of":r.get::<_,Option<String>>(6)?,"analysis_snapshot":serde_json::from_str::<Value>(&r.get::<_,String>(7)?).unwrap_or(Value::Null)}))).map_err(db::DbError::from)?.collect::<Result<Vec<_>,_>>().map_err(db::DbError::from)?;
        Ok(json!({"mode":mode,"rows":rows,"limit":100}))
    }
    pub fn save_research_note(&self, p: &Value) -> Result<Value, AppError> {
        let key = self.parse_key(p["symbol"].as_str().unwrap_or(""), p["market"].as_str())?;
        let revision = p["revision"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| AppError::InvalidArg("缺少笔记版本".into()))?;
        let mut fields = Vec::new();
        for name in ["thesis", "risks", "plan", "review"] {
            let v = p[name]
                .as_str()
                .ok_or_else(|| AppError::InvalidArg(format!("缺少 {name}")))?;
            if v.chars().count() > 10000 {
                return Err(AppError::InvalidArg("每段研究记录最多10000字".into()));
            }
            fields.push(v);
        }
        let mode = research_mode(p["mode"].as_str())?;
        let analysis = self.analysis_workspace_basis(
            &key,
            p["analysis_as_of"].as_str(),
            Some(mode),
            p["basis"].as_str().unwrap_or("raw"),
            p["factor_source"].as_str().unwrap_or(""),
        )?;
        let evidence = json!({"mode":mode,"workspace":analysis["workspace"],"evaluation_date":analysis["evaluation_date"],"as_of":analysis["as_of"],"data_version":analysis["data_version"],"sample_count":analysis["sample_count"],"price_basis":analysis["price_basis"],"metrics":analysis["metrics"],"sources":analysis["sources"],"warnings":analysis["warnings"]});
        let mut conn = self.db.connect()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db::DbError::from)?;
        if note_on(&tx, &key, mode)?["revision"].as_i64() != Some(revision) {
            return Err(AppError::InvalidArg(
                "研究记录已在另一窗口更新，请先核对最新版本；草稿未覆盖已保存记录".into(),
            ));
        }
        tx.execute(
            "INSERT INTO research_journal(market,symbol,mode,revision,thesis,risks,plan,review,updated_at,analysis_as_of,analysis_snapshot) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                key.market.as_str(),
                key.symbol,
                mode,
                revision + 1,
                fields[0],
                fields[1],
                fields[2],
                fields[3],
                db::now_rfc3339(),
                analysis["evaluation_date"].as_str(),
                evidence.to_string()
            ],
        )
        .map_err(db::DbError::from)?;
        let note = note_on(&tx, &key, mode)?;
        tx.commit().map_err(db::DbError::from)?;
        Ok(note)
    }
    pub fn research_screen(&self, p: &Value) -> Result<Value, AppError> {
        let board = p["board"].as_str().unwrap_or("CN");
        if !["CN", "WATCH"].contains(&board) {
            return Err(AppError::InvalidArg(
                "选股范围只支持 CN / WATCH，成交额统一使用人民币".into(),
            ));
        }
        let bound = |name: &str, default: f64| -> Result<f64, AppError> {
            if p[name].is_null() {
                return Ok(default);
            }
            p[name]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| AppError::InvalidArg(format!("{name} 必须为有限数字")))
        };
        let min = bound("min_change_pct", 2.0)?;
        let max = bound("max_change_pct", 100.0)?;
        let amount = bound("min_turnover", 1e8)?;
        if min > max || amount < 0.0 {
            return Err(AppError::InvalidArg("选股阈值范围无效".into()));
        }
        let page = self.quote_board(Some(board), None, Some("turnover"), 20000, 0, None)?;
        let now = chrono::Utc::now();
        let usable = |r: &crate::view::QuoteRow| {
            r.market.starts_with("CN.")
                && r.has_quote
                && r.currency.as_deref() == Some("CNY")
                && r.ts
                    .as_deref()
                    .and_then(|ts| session::parse_cn_ts(ts).ok())
                    .is_some_and(|ts| ts <= now)
                && r.trade_date.is_some()
        };
        let date = page
            .rows
            .iter()
            .filter(|r| usable(r))
            .filter_map(|r| r.trade_date.as_deref())
            .filter(|d| *d <= session::iso_date(session::shanghai_today()).as_str())
            .max()
            .map(str::to_string);
        let mut valid = 0;
        let mut excluded = 0;
        let mut rows = Vec::new();
        for r in page.rows {
            if !usable(&r) || r.trade_date != date {
                excluded += 1;
                continue;
            }
            valid += 1;
            if r.change_pct
                .zip(r.turnover)
                .is_some_and(|(pct, cash)| pct >= min && pct <= max && cash >= amount)
            {
                rows.push(r);
            }
        }
        let matched = rows.len();
        rows.truncate(100);
        Ok(
            json!({"as_of":date,"board":board,"currency":"CNY","eligible":valid,"excluded":excluded,"matched":matched,"truncated":matched>100,"rows":rows,"criteria":{"min_change_pct":min,"max_change_pct":max,"min_turnover":amount},"notice":"仅筛选本地同日人民币报价，按成交额降序；结果不构成收益预测"}),
        )
    }
}
