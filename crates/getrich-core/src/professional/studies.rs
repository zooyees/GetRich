use super::*;
use crate::models::{Bar, InstrumentKey};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn members(app: &App, raw: &str) -> Result<Vec<InstrumentKey>, AppError> {
    let mut out = vec![];
    for line in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let (market, symbol) = line
            .split_once(':')
            .ok_or_else(|| invalid("证券格式应为 市场:代码，例如 CN.SH:600519"))?;
        let key = app.parse_key(symbol, Some(market))?;
        if !out.contains(&key) {
            out.push(key);
        }
    }
    if out.len() > 200 {
        return Err(invalid("单个研究池最多200只证券"));
    }
    Ok(out)
}
fn ema(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    let mut seed = vec![];
    let mut state = None;
    for (i, v) in values.iter().enumerate() {
        if let Some(v) = v {
            if let Some(previous) = state {
                let next = previous + (v - previous) * 2.0 / (period + 1) as f64;
                state = Some(next);
                out[i] = Some(next);
            } else {
                seed.push(*v);
                if seed.len() == period {
                    let next = seed.iter().sum::<f64>() / period as f64;
                    state = Some(next);
                    out[i] = Some(next);
                }
            }
        }
    }
    out
}
pub(super) fn indicators(bars: &[Bar]) -> BTreeMap<String, Vec<Option<f64>>> {
    let mut output = BTreeMap::new();
    let closes: Vec<_> = bars.iter().map(|b| Some(b.close)).collect();
    let n = bars.len();
    output.insert("收盘".into(), closes.clone());
    for p in crate::indicators::MA_PERIODS {
        let mut sum = 0.0;
        let mut values = vec![None; n];
        for (i, b) in bars.iter().enumerate() {
            sum += b.close;
            if i >= p {
                sum -= bars[i - p].close;
            }
            if i + 1 >= p {
                values[i] = Some(sum / p as f64);
            }
        }
        output.insert(format!("MA{p}"), values);
    }
    let fast = ema(&closes, 12);
    let slow = ema(&closes, 26);
    let dif: Vec<_> = fast
        .iter()
        .zip(slow)
        .map(|(a, b)| a.zip(b).map(|(a, b)| a - b))
        .collect();
    let dea = ema(&dif, 9);
    let hist = dif
        .iter()
        .zip(&dea)
        .map(|(a, b)| a.zip(*b).map(|(a, b)| 2.0 * (a - b)))
        .collect();
    output.insert("MACD DIF".into(), dif);
    output.insert("MACD DEA".into(), dea);
    output.insert("MACD 柱".into(), hist);
    let (mut gain, mut loss, mut tr) = (0.0, 0.0, 0.0);
    let mut rsi = vec![None; n];
    let mut atr = vec![None; n];
    for i in 1..n {
        let c = bars[i].close - bars[i - 1].close;
        let t = (bars[i].high - bars[i].low)
            .max((bars[i].high - bars[i - 1].close).abs())
            .max((bars[i].low - bars[i - 1].close).abs());
        if i <= 14 {
            gain += c.max(0.0) / 14.0;
            loss += (-c).max(0.0) / 14.0;
            tr += t / 14.0;
        } else {
            gain = (gain * 13.0 + c.max(0.0)) / 14.0;
            loss = (loss * 13.0 + (-c).max(0.0)) / 14.0;
            tr = (tr * 13.0 + t) / 14.0;
        }
        if i >= 14 {
            rsi[i] = Some(if gain + loss == 0.0 {
                50.0
            } else {
                100.0 * gain / (gain + loss)
            });
            atr[i] = Some(tr);
        }
    }
    output.insert("RSI14".into(), rsi);
    output.insert("ATR14".into(), atr);
    output
}
pub(super) fn chart(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = data::key(app, p)?;
    let (from, to) = range(p)?;
    let basis = if text(p, "basis").is_empty() {
        "raw"
    } else {
        text(p, "basis")
    };
    let timeframe = if text(p, "timeframe").is_empty() {
        "1d"
    } else {
        text(p, "timeframe")
    };
    let (bars, provenance) = match timeframe {
        "1d" => data::adjusted(app, &key, &to, basis, text(p, "factor_source"))?,
        "1m" if basis == "raw" => minute_study::load(app, &key, &to)?,
        "1m" => return Err(invalid("分钟指标当前只支持原始价格，请选择原始价格口径")),
        _ => return Err(invalid("指标周期应为1d或1m")),
    };
    let mut indicators = indicators(&bars);
    if timeframe == "1m" {
        let mut boundaries: Vec<usize> = provenance["resets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_u64().map(|v| v as usize))
            .collect();
        boundaries.push(bars.len());
        for range in boundaries.windows(2) {
            let segment = self::indicators(&bars[range[0]..range[1]]);
            for (name, values) in segment {
                indicators.get_mut(&name).unwrap()[range[0]..range[1]].copy_from_slice(&values);
            }
        }
    }
    let display = num(p, "display", 300.0)?.clamp(1.0, 2000.0) as usize;
    let valid: Vec<_> = bars
        .iter()
        .enumerate()
        .filter(|(_, b)| b.ts.as_str() >= from.as_str())
        .collect();
    let chosen: Vec<_> = valid
        .iter()
        .skip(valid.len().saturating_sub(display))
        .copied()
        .collect();
    let mut series:Vec<_>=indicators.iter().map(|(name,values)|json!({"name":name,"style":if name=="MACD 柱"{"histogram"}else{"line"},"pane":if name.starts_with("MACD"){"MACD"}else if name=="RSI14"{"RSI"}else if name=="ATR14"{"ATR"}else{"价格"},"points":chosen.iter().map(|(i,b)|json!({"date":b.ts,"value":values[*i]})).collect::<Vec<_>>()})).collect();
    let rows=chosen.iter().rev().take(100).map(|(i,b)|json!({"date":b.ts,"open":b.open,"high":b.high,"low":b.low,"close":b.close,"rsi":indicators["RSI14"][*i],"atr":indicators["ATR14"][*i]})).collect();
    let mut v=view("复权价格与指标副图",json!([{"label":"价格口径","value":basis},{"label":"完整预热样本","value":bars.len()},{"label":"展示根数","value":chosen.len()},{"label":"因子来源","value":provenance["source"]}]),vec![table("最近100根明细",&[("date","日期"),("open","开"),("high","高"),("low","低"),("close","收"),("rsi","RSI14"),("atr","ATR14")],rows)],vec![provenance["warning"].as_str().unwrap_or("").into(),"RSI/ATR使用Wilder14；MACD为SMA种子的EMA12/26/9，柱为2×差值；均线按可用序列预热，不随展示窗口截断重算。".into()]);
    series.sort_by_key(|s| {
        let pane = match text(s, "pane") {
            "价格" => 0,
            "MACD" => 1,
            "RSI" => 2,
            "ATR" => 3,
            _ => 4,
        };
        let name = text(s, "name");
        let order = if name == "收盘" {
            0
        } else if let Some(period) = name.strip_prefix("MA").and_then(|n| n.parse::<u32>().ok()) {
            period
        } else if name == "MACD DIF" {
            0
        } else if name == "MACD DEA" {
            1
        } else {
            2
        };
        (pane, order)
    });
    v["series"] = json!(series);
    v["provenance"] = provenance;
    v["timeframe"] = json!(timeframe);
    v["notices"].as_array_mut().unwrap().push(json!(if timeframe=="1m"{"本页均线/RSI/ATR/MACD的周期单位为分钟根数，不能解释为日均线；当前缓存的分钟日期和样本量见来源信息。"}else{"本页指标周期单位为日线根数。"}));
    Ok(v)
}
pub(super) fn compare(app: &App, p: &Value) -> Result<Value, AppError> {
    let keys = members(app, text(p, "members"))?;
    if keys.is_empty() || keys.len() > 12 {
        return Err(invalid("同期比较请提供1至12只证券"));
    }
    let currency = keys[0].market.default_currency();
    if keys.iter().any(|k| k.market.default_currency() != currency) {
        return Err(invalid("不同币种不能直接比较，请分别建立研究组"));
    }
    let (from, to) = range(p)?;
    let basis = if text(p, "basis").is_empty() {
        "raw"
    } else {
        text(p, "basis")
    };
    let mut data = vec![];
    let mut common = None::<BTreeSet<String>>;
    for key in &keys {
        let (bars, _) = data::adjusted(app, key, &to, basis, text(p, "factor_source"))?;
        let map: BTreeMap<_, _> = bars
            .into_iter()
            .filter(|b| b.ts.as_str() >= from.as_str())
            .map(|b| (b.ts[..10].to_string(), b.close))
            .collect();
        let dates: BTreeSet<_> = map.keys().cloned().collect();
        common = Some(
            common
                .map(|old| old.intersection(&dates).cloned().collect())
                .unwrap_or(dates),
        );
        data.push((key, map));
    }
    let common = common.unwrap();
    if common.len() < 2 {
        return Err(invalid("共同交易日不足2天，不能比较"));
    }
    let first = common.first().unwrap();
    let last = common.last().unwrap();
    let display = num(p, "display", 300.0)?.clamp(1.0, 2000.0) as usize;
    let mut series = vec![];
    let mut rows = vec![];
    for (key, map) in data {
        let base = map[first];
        let points: Vec<_> = common
            .iter()
            .skip(common.len().saturating_sub(display))
            .map(|d| json!({"date":d,"value":map[d]/base*100.0}))
            .collect();
        series.push(json!({"name":key.display(),"pane":"共同起点=100","points":points}));
        rows.push(json!({"stock":key.display(),"return_pct":(map[last]/base-1.0)*100.0,"from":first,"to":last}));
    }
    let mut v = view(
        "同期相对强弱",
        json!([{"label":"共同交易日","value":common.len()},{"label":"币种","value":currency},{"label":"价格口径","value":basis}]),
        vec![table(
            "共同区间收益",
            &[
                ("stock", "证券"),
                ("from", "起点"),
                ("to", "终点"),
                ("return_pct", "价格收益 %"),
            ],
            rows,
        )],
        vec!["比较区间仅使用共同可用日期；原始价格不含分红再投资，未复权时除权会影响结果。".into()],
    );
    v["series"] = json!(series);
    Ok(v)
}
pub(super) fn latest(
    app: &App,
    key: &InstrumentKey,
    kind: &str,
    to: &str,
) -> Result<Option<Value>, AppError> {
    let all = data::facts(app, kind, Some(key), to)?;
    let cache = app.stock_research(key)?;
    let mut candidates: Vec<Value> = all
        .into_iter()
        .map(|mut v| {
            let source = v["source"].clone();
            let observed = v["observed_at"].clone();
            let mut row = v["payload"].take();
            row["_source"] = source;
            row["_observed_at"] = observed;
            if kind == "valuation" {
                row["trade_date"] = v["day"].clone();
            }
            row
        })
        .collect();
    candidates.extend(
        cache["datasets"][kind]["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned()
            .map(|mut row| {
                row["_source"] = cache["datasets"][kind]["source"].clone();
                row["_observed_at"] = cache["datasets"][kind]["updated_at"].clone();
                row
            }),
    );
    candidates.retain(|r| {
        let d = if kind == "financial" {
            day(text(r, "ann_date")).ok()
        } else {
            day(text(r, "trade_date")).ok()
        };
        d.is_some_and(|d| d.as_str() <= to)
    });
    candidates.sort_by_key(|r| {
        if kind == "financial" {
            format!(
                "{}:{}:{}",
                text(r, "end_date").replace('-', ""),
                text(r, "ann_date").replace('-', ""),
                text(r, "_observed_at")
            )
        } else {
            format!(
                "{}:{}",
                text(r, "trade_date").replace('-', ""),
                text(r, "_observed_at")
            )
        }
    });
    Ok(candidates.pop())
}
pub(super) fn peers(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = data::key(app, p)?;
    let to = if text(p, "to").is_empty() {
        today()
    } else {
        day(text(p, "to"))?
    };
    if to > today() {
        return Err(invalid("不能比较未来日期"));
    }
    let detail = app.show_stock(&key)?;
    let industry = detail
        .listing
        .industry
        .ok_or_else(|| invalid("缺少行业分类，不能自动推定同行"))?;
    let conn = app.db.connect()?;
    let mut stmt=sql(conn.prepare("SELECT symbol,market,name FROM listings WHERE industry=?1 AND currency=?2 ORDER BY symbol LIMIT 200"))?;
    let listing = sql(stmt.query_map(
        rusqlite::params![industry, key.market.default_currency()],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    ))?
    .map(sql)
    .collect::<Result<Vec<_>, _>>()?;
    let mut rows = vec![];
    let mut pes = vec![];
    let mut roes = vec![];
    for (symbol, market, name) in &listing {
        let k = app.parse_key(symbol, Some(market))?;
        let f = latest(app, &k, "financial", &to)?.unwrap_or(Value::Null);
        let v = latest(app, &k, "valuation", &to)?.unwrap_or(Value::Null);
        if let Some(pe) = v["pe_ttm"].as_f64().filter(|v| *v > 0.0) {
            pes.push(pe);
        }
        if let Some(roe) = f["roe"].as_f64() {
            roes.push(roe);
        }
        rows.push(json!({"stock":k.display(),"name":name,"pe":v["pe_ttm"],"pb":v["pb"],"roe":f["roe"],"growth":f["netprofit_yoy"],"report":f["end_date"],"announced":f["ann_date"],"valuation_date":v["trade_date"]}));
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        if v.is_empty() {
            None
        } else {
            Some((v[(v.len() - 1) / 2] + v[v.len() / 2]) / 2.0)
        }
    };
    Ok(view("同行财务与估值",json!([{"label":"行业","value":industry},{"label":"同行数量（最多200）","value":listing.len()},{"label":"有PE样本","value":pes.len()},{"label":"正PE中位数","value":median(&mut pes)},{"label":"ROE中位数","value":median(&mut roes)}]),vec![table("按披露截止日比较",&[("stock","证券"),("name","名称"),("pe","PE TTM"),("pb","PB"),("roe","ROE %"),("growth","利润同比 %"),("report","报告期"),("announced","公告日"),("valuation_date","估值日")],rows)],vec!["财报期与估值日不一定相同；未知值不纳入中位数。当前缓存修订值不能证明其在历史时点已可得。".into()]))
}
pub(super) fn scenario(_app: &App, p: &Value) -> Result<Value, AppError> {
    let eps = num(p, "eps", 0.0)?;
    let growth = num(p, "growth_pct", 5.0)? / 100.0;
    let years = num(p, "years", 3.0)?;
    let pe = num(p, "pe", 15.0)?;
    let discount = num(p, "discount_pct", 10.0)? / 100.0;
    if eps <= 0.0
        || growth <= -1.0
        || discount <= -1.0
        || pe <= 0.0
        || !(1.0..=30.0).contains(&years)
        || years.fract() != 0.0
    {
        return Err(invalid(
            "情景参数无效；EPS与PE应为正数，年数1至30，增长和折现率大于-100%",
        ));
    }
    let mut rows = vec![];
    for dg in [-0.05, 0.0, 0.05] {
        for pm in [0.8, 1.0, 1.2] {
            let g = growth + dg;
            if g <= -1.0 {
                continue;
            }
            let value = eps * (1.0 + g).powf(years) * pe * pm / (1.0 + discount).powf(years);
            if !value.is_finite() {
                return Err(invalid("参数导致结果溢出"));
            }
            rows.push(json!({"growth_pct":g*100.0,"pe":pe*pm,"discount_pct":discount*100.0,"present_value":value}));
        }
    }
    Ok(view("估值假设敏感性",json!([{"label":"基期EPS假设","value":eps},{"label":"预测年数","value":years}]),vec![table("增长与期末倍数情景",&[("growth_pct","增长假设 %"),("pe","期末PE"),("discount_pct","折现率 %"),("present_value","折现后每股情景值")],rows)],vec!["公式：EPS×(1+增长)^年数×期末PE÷(1+折现率)^年数。全部输入均为用户假设，不是目标价或自动投资结论。".into()]))
}
pub(super) fn events(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = data::key(app, p)?;
    let (from, to) = range(p)?;
    let cached = app.stock_research(&key)?;
    let mut events = BTreeMap::new();
    for kind in ["announcements", "financial"] {
        let mut all: Vec<_> = data::facts(app, kind, Some(&key), &to)?
            .into_iter()
            .map(|r| r["payload"].clone())
            .collect();
        all.extend(
            cached["datasets"][kind]["rows"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
        for r in all {
            let Ok(d) = day(text(&r, "ann_date")) else {
                continue;
            };
            if d < from || d > to {
                continue;
            }
            let title = if kind == "financial" {
                format!("财务披露 · {}", text(&r, "end_date"))
            } else {
                text(&r, "title").to_string()
            };
            let identity = format!("{d}:{title}");
            let url = text(&r, "url");
            let safe =
                (url.starts_with("https://") || url.starts_with("http://")) && !url.contains('@');
            events.insert(identity,json!({"date":d,"type":if kind=="financial"{"财报"}else{"公告"},"title":title,"url":if safe{Some(url)}else{None}}));
        }
    }
    let rows = events.into_values().rev().take(500).collect();
    Ok(view(
        "公告与财报事件轴",
        json!([]),
        vec![table(
            "截止日前已披露事件（最多500）",
            &[
                ("date", "披露日"),
                ("type", "类型"),
                ("title", "事件"),
                ("url", "原文链接"),
            ],
            rows,
        )],
        vec!["公告原文跳转到提供商链接；未披露、无日期及不安全链接已排除。".into()],
    ))
}
