use super::*;
use crate::models::{Bar, InstrumentKey, Timeframe};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn csv(raw: &str) -> Result<Vec<Value>, AppError> {
    if raw.len() > 4_000_000 {
        return Err(invalid("单次CSV最多4MB"));
    }
    let mut rows = Vec::<Vec<String>>::new();
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = raw.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(field.trim().to_string());
                field.clear();
                closed = false;
            }
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(field.trim().to_string());
                field.clear();
                closed = false;
                if row.iter().any(|s| !s.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ if closed && !c.is_whitespace() => return Err(invalid("CSV引号后的字符无效")),
            _ => field.push(c),
        }
        if rows.len() > 10001 {
            return Err(invalid("单次最多10000条记录"));
        }
    }
    if quoted {
        return Err(invalid("CSV引号未闭合"));
    }
    row.push(field.trim().into());
    if row.iter().any(|s| !s.is_empty()) {
        rows.push(row);
    }
    if rows.len() < 2 {
        return Err(invalid("CSV需要表头和至少一行记录"));
    }
    let header = rows.remove(0);
    let mut names = BTreeSet::new();
    if header
        .iter()
        .any(|h| h.is_empty() || !names.insert(h.clone()))
    {
        return Err(invalid("CSV表头为空或重复"));
    }
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            if r.len() != header.len() {
                return Err(invalid(&format!("CSV第{}行列数与表头不一致", i + 2)));
            }
            Ok(Value::Object(
                header
                    .iter()
                    .cloned()
                    .zip(r.into_iter().map(Value::String))
                    .collect(),
            ))
        })
        .collect()
}
pub(super) fn key(app: &App, p: &Value) -> Result<InstrumentKey, AppError> {
    app.parse_key(text(p, "symbol"), Some(text(p, "market")))
}
pub(super) fn store(
    conn: &Connection,
    kind: &str,
    key: Option<&InstrumentKey>,
    day: &str,
    identity: &str,
    source: &str,
    payload: &Value,
) -> Result<usize, AppError> {
    sql(conn.execute("INSERT INTO professional_facts(kind,market,symbol,day,identity,source,observed_at,payload) SELECT ?1,?2,?3,?4,?5,?6,?7,?8 WHERE COALESCE((SELECT payload FROM professional_facts WHERE kind=?1 AND market=?2 AND symbol=?3 AND day=?4 AND identity=?5 AND source=?6 ORDER BY id DESC LIMIT 1),'')<>?8",params![kind,key.map(|k|k.market.as_str()).unwrap_or(""),key.map(|k|k.symbol.as_str()).unwrap_or(""),day,identity,source,db::now_rfc3339(),payload.to_string()]))
}
pub(super) fn facts(
    app: &App,
    kind: &str,
    key: Option<&InstrumentKey>,
    to: &str,
) -> Result<Vec<Value>, AppError> {
    let conn = app.db.connect()?;
    facts_on(&conn, kind, key, to)
}
pub(super) fn facts_on(
    conn: &Connection,
    kind: &str,
    key: Option<&InstrumentKey>,
    to: &str,
) -> Result<Vec<Value>, AppError> {
    let mut s=sql(conn.prepare("SELECT id,day,identity,source,observed_at,payload,market,symbol FROM (SELECT *,ROW_NUMBER() OVER(PARTITION BY market,symbol,day,identity,source ORDER BY id DESC) rn FROM professional_facts WHERE kind=?1 AND day<=?2 AND (?3='' OR (market=?3 AND symbol=?4))) WHERE rn=1 ORDER BY day,id"))?;
    let rows=sql(s.query_map(params![kind,to,key.map(|k|k.market.as_str()).unwrap_or(""),key.map(|k|k.symbol.as_str()).unwrap_or("")],|r|{let p:String=r.get(5)?;Ok(json!({"id":r.get::<_,i64>(0)?,"day":r.get::<_,String>(1)?,"identity":r.get::<_,String>(2)?,"source":r.get::<_,String>(3)?,"observed_at":r.get::<_,String>(4)?,"payload":serde_json::from_str::<Value>(&p).unwrap_or(Value::Null),"market":r.get::<_,String>(6)?,"symbol":r.get::<_,String>(7)?}))}))?;
    rows.map(sql).collect()
}
pub(super) fn capture_research(
    conn: &Connection,
    key: &InstrumentKey,
    dataset: &str,
    source: &str,
    rows: &[Value],
) -> Result<(), AppError> {
    if !["financial", "valuation", "announcements", "company"].contains(&dataset) {
        return Ok(());
    }
    for row in rows {
        let raw = ["end_date", "trade_date", "ann_date"]
            .iter()
            .find_map(|f| row[*f].as_str())
            .unwrap_or("");
        let d = if dataset == "company" {
            today()
        } else {
            match day(raw) {
                Ok(d) => d,
                Err(_) => continue,
            }
        };
        let identity = if dataset == "announcements" {
            hash(&format!("{}{}", row["title"], row["url"]))
        } else {
            text(row, "ann_date").to_string()
        };
        store(conn, dataset, Some(key), &d, &identity, source, row)?;
    }
    Ok(())
}
pub(super) fn import(app: &App, p: &Value) -> Result<Value, AppError> {
    let kind = text(p, "dataset");
    if ![
        "factors",
        "calendar",
        "observations",
        "constraints",
        "actions",
        "financial",
        "valuation",
        "bar_history",
        "security_history",
    ]
    .contains(&kind)
    {
        return Err(invalid("未知资料类型"));
    }
    let source = text(p, "source");
    if source.is_empty() || source.chars().count() > 100 {
        return Err(invalid("请填写不超过100字的来源名称"));
    }
    let mut rows = csv(text(p, "csv"))?;
    let mut validated = Vec::new();
    for (i, r) in rows.iter_mut().enumerate() {
        let result = (|| {
            let instrument = if kind == "calendar" {
                None
            } else {
                Some(key(app, r)?)
            };
            let d = day(if kind == "financial" {
                text(r, "end_date")
            } else if kind == "observations" {
                text(r, "timestamp").get(..10).unwrap_or("")
            } else {
                text(r, "date")
            })?;
            if d > today() {
                return Err(invalid("不接受未来日期"));
            }
            let mut identity = String::new();
            let mut values = Vec::<(&str, f64)>::new();
            if !text(r, "known_at").is_empty() {
                let ts = chrono::DateTime::parse_from_rfc3339(text(r, "known_at"))
                    .map_err(|_| invalid("known_at须为带时区的RFC3339来源可得时间"))?;
                if ts.with_timezone(&chrono::Utc) > chrono::Utc::now() {
                    return Err(invalid("来源可得时间不能晚于现在"));
                }
                r["known_at"] = json!(ts.to_rfc3339());
            }
            match kind {
                "bar_history" => {
                    if text(r, "known_at").is_empty() {
                        return Err(invalid("历史时点日线必须包含known_at"));
                    }
                    let known = chrono::DateTime::parse_from_rfc3339(text(r, "known_at")).unwrap();
                    if !instrument
                        .as_ref()
                        .unwrap()
                        .market
                        .as_str()
                        .starts_with("CN.")
                    {
                        return Err(invalid("时点日线档案当前仅支持沪深京"));
                    }
                    let close =
                        chrono::DateTime::parse_from_rfc3339(&format!("{d}T15:00:00+08:00"))
                            .unwrap();
                    if known < close {
                        return Err(invalid("完整日线可得时间不能早于当日收盘"));
                    }
                    if known
                        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
                        .date_naive()
                        .to_string()
                        < d
                    {
                        return Err(invalid("完整日线不可能在交易日期之前可得"));
                    }
                    let o = num(r, "open", 0.0)?;
                    let h = num(r, "high", 0.0)?;
                    let l = num(r, "low", 0.0)?;
                    let c = num(r, "close", 0.0)?;
                    let v = num(r, "volume", -1.0)?;
                    if l <= 0.0 || l > o.min(c) || h < o.max(c) || v < 0.0 {
                        return Err(invalid("历史日线OHLC或成交量无效"));
                    }
                    values.extend([
                        ("open", o),
                        ("high", h),
                        ("low", l),
                        ("close", c),
                        ("volume", v),
                    ]);
                    if !text(r, "turnover").is_empty() {
                        let amount = num(r, "turnover", -1.0)?;
                        if amount < 0.0 {
                            return Err(invalid("成交额不能为负"));
                        }
                        values.push(("turnover", amount));
                    }
                    identity = text(r, "known_at").into();
                }
                "security_history" => {
                    if text(r, "known_at").is_empty()
                        || !["listed", "delisted"].contains(&text(r, "status"))
                    {
                        return Err(invalid("证券历史须有known_at及listed/delisted状态"));
                    }
                    r["list_date"] = json!(day(text(r, "list_date"))?);
                    if !text(r, "delist_date").is_empty() {
                        r["delist_date"] = json!(day(text(r, "delist_date"))?);
                        if text(r, "delist_date") < text(r, "list_date") {
                            return Err(invalid("退市日期早于上市日期"));
                        }
                    }
                    identity = text(r, "known_at").into();
                }
                "factors" => {
                    let v = num(r, "factor", 0.0)?;
                    if v <= 0.0 {
                        return Err(invalid("复权因子必须大于0"));
                    }
                    values.push(("factor", v));
                }
                "calendar" => {
                    if !["SSE", "SZSE", "BSE", "HKEX", "US"].contains(&text(r, "exchange")) {
                        return Err(invalid("交易所应为 SSE/SZSE/BSE/HKEX/US"));
                    }
                    identity = text(r, "exchange").into();
                    if !["0", "1", "true", "false"].contains(&text(r, "is_open")) {
                        return Err(invalid("is_open应为0或1"));
                    }
                    r["is_open"] = json!(flag(r, "is_open"));
                }
                "observations" => {
                    let ts = session::parse_cn_ts(text(r, "timestamp")).map_err(|e| invalid(&e))?;
                    if ts > chrono::Utc::now() {
                        return Err(invalid("报价时间超过当前时间"));
                    }
                    r["timestamp"] = json!(ts.to_rfc3339());
                    let last = num(r, "last", 0.0)?;
                    if last <= 0.0 {
                        return Err(invalid("报价必须大于0"));
                    }
                    values.push(("last", last));
                    for f in ["volume", "turnover"] {
                        if !text(r, f).is_empty() {
                            let v = num(r, f, 0.0)?;
                            if v < 0.0 {
                                return Err(invalid("成交量额不能为负"));
                            }
                            values.push((f, v));
                        }
                    }
                    if text(r, "currency") != instrument.as_ref().unwrap().market.default_currency()
                    {
                        return Err(invalid("报价币种与市场不匹配"));
                    }
                    identity = text(r, "timestamp").into();
                }
                "constraints" => {
                    if r.get("up_limit").is_none() || r.get("down_limit").is_none() {
                        return Err(invalid("交易约束须明确包含 up_limit 与 down_limit 列"));
                    }
                    let up = text(r, "up_limit");
                    let down = text(r, "down_limit");
                    if up.is_empty() != down.is_empty() || (up.is_empty() && !flag(r, "no_limit")) {
                        return Err(invalid("无涨跌幅限制时两列留空并明确填写 no_limit=1"));
                    }
                    for f in ["suspended", "t_plus_one", "actions_checked"] {
                        if !["0", "1", "true", "false"].contains(&text(r, f)) {
                            return Err(invalid(&format!("{f}必须明确填写0或1")));
                        }
                        r[f] = json!(flag(r, f));
                    }
                    let lot = num(r, "lot_size", 0.0)?;
                    if lot < 1.0 || lot.fract() != 0.0 {
                        return Err(invalid("最小交易单位必须为正整数"));
                    }
                    values.push(("lot_size", lot));
                    let min_buy = num(r, "min_buy", lot)?;
                    if min_buy < lot || min_buy.fract() != 0.0 {
                        return Err(invalid("最低买入股数无效"));
                    }
                    values.push(("min_buy", min_buy));
                    for f in ["up_limit", "down_limit"] {
                        if !text(r, f).is_empty() {
                            let v = num(r, f, 0.0)?;
                            if v <= 0.0 {
                                return Err(invalid("涨跌停价必须为正数；无价格限制填写空白"));
                            }
                            values.push((f, v));
                        }
                    }
                }
                "actions" => {
                    for f in ["record_date", "pay_date"] {
                        r[f] = json!(day(text(r, f))?);
                    }
                    if text(r, "record_date") > d.as_str() || text(r, "pay_date") < d.as_str() {
                        return Err(invalid("公司行为日期顺序错误"));
                    }
                    let cash = num(r, "cash_per_share", 0.0)?;
                    let ratio = num(r, "share_ratio", 1.0)?;
                    if cash < 0.0 || ratio <= 0.0 {
                        return Err(invalid("分红或股数比例无效"));
                    }
                    values.extend([("cash_per_share", cash), ("share_ratio", ratio)]);
                }
                "financial" => {
                    let announced = day(text(r, "ann_date"))?;
                    if announced < d || announced > today() {
                        return Err(invalid("财报公告日期无效"));
                    }
                    identity = announced;
                    for f in [
                        "eps",
                        "ocfps",
                        "roe",
                        "or_yoy",
                        "netprofit_yoy",
                        "debt_to_assets",
                        "grossprofit_margin",
                        "netprofit_margin",
                    ] {
                        if !text(r, f).is_empty() {
                            values.push((f, num(r, f, 0.0)?));
                        }
                    }
                }
                "valuation" => {
                    for f in ["pe_ttm", "pb", "dv_ttm"] {
                        if !text(r, f).is_empty() {
                            values.push((f, num(r, f, 0.0)?));
                        }
                    }
                }
                _ => {}
            }
            for (field, n) in values {
                r[field] = json!(n);
            }
            for field in ["volume", "turnover", "up_limit", "down_limit"] {
                if r[field].as_str() == Some("") {
                    r[field] = Value::Null;
                }
            }
            Ok((instrument, d, identity, r.clone()))
        })();
        validated.push(result.map_err(|e| invalid(&format!("第{}行：{e}", i + 2)))?);
    }
    let mut conn = app.db.connect()?;
    let tx = sql(conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    let mut inserted = 0;
    for (key, d, identity, row) in &validated {
        inserted += store(&tx, kind, key.as_ref(), d, identity, source, row)?;
        if kind == "observations" {
            let key = key.as_ref().unwrap();
            let ts = text(row, "timestamp");
            sql(tx.execute("INSERT INTO quote_samples(market,symbol,source,bucket,ts,last,volume,turnover,currency) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(market,symbol,source,bucket) DO UPDATE SET ts=excluded.ts,last=excluded.last,volume=excluded.volume,turnover=excluded.turnover,currency=excluded.currency WHERE excluded.ts>=quote_samples.ts",params![key.market.as_str(),key.symbol,source,&ts[..16],ts,num(row,"last",0.0)?,row["volume"].as_f64(),row["turnover"].as_f64(),text(row,"currency")]))?;
        }
    }
    sql(tx.commit())?;
    app.events
        .emit("professional.updated", json!({"dataset":kind}));
    Ok(view(
        "资料已落盘",
        json!([{"label":"有效行","value":rows.len()},{"label":"新增版本","value":inserted},{"label":"重复跳过","value":rows.len()-inserted}]),
        vec![],
        vec!["服务器观测时间与来源随版本保存；不会将导入时间伪装成历史可得时间。".into()],
    ))
}
pub(super) fn overlay_research(
    app: &App,
    key: &InstrumentKey,
    datasets: &mut serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    for kind in ["financial", "valuation", "announcements"] {
        let all = facts(app, kind, Some(key), &today())?;
        if all.is_empty() {
            continue;
        }
        let dataset = datasets
            .entry(kind.to_string())
            .or_insert(json!({"rows":[]}));
        let mut rows = dataset["rows"].as_array().cloned().unwrap_or_default();
        let mut sources = BTreeSet::new();
        let mut observed = dataset["updated_at"].as_str().unwrap_or("").to_string();
        if let Some(source) = dataset["source"].as_str() {
            if !rows.is_empty() {
                sources.insert(source.to_string());
            }
        }
        for fact in all {
            let mut row = fact["payload"].clone();
            row["_source"] = fact["source"].clone();
            row["_observed_at"] = fact["observed_at"].clone();
            if kind == "valuation" {
                row["trade_date"] = fact["day"].clone();
            }
            if observed.as_str() < text(&fact, "observed_at") {
                observed = text(&fact, "observed_at").into();
            }
            sources.insert(text(&fact, "source").to_string());
            rows.push(row);
        }
        dataset["rows"] = json!(rows);
        dataset["source"] = json!(sources.into_iter().collect::<Vec<_>>().join(" / "));
        dataset["updated_at"] = json!(observed);
        dataset["status"] = json!("ready");
    }
    Ok(())
}
pub(super) fn raw_bars(app: &App, key: &InstrumentKey, to: &str) -> Result<Vec<Bar>, AppError> {
    Ok(app
        .list_bars(key, Timeframe::D1, None, Some(to), 20000)?
        .rows)
}
pub(super) fn adjusted(
    app: &App,
    key: &InstrumentKey,
    to: &str,
    basis: &str,
    source: &str,
) -> Result<(Vec<Bar>, Value), AppError> {
    let (bars, meta, _) = adjusted_snapshot(app, key, to, basis, source)?;
    Ok((bars, meta))
}
pub(super) fn adjusted_snapshot(
    app: &App,
    key: &InstrumentKey,
    to: &str,
    basis: &str,
    source: &str,
) -> Result<(Vec<Bar>, Value, u64), AppError> {
    let conn = app.db.connect()?;
    let tx = sql(conn.unchecked_transaction())?;
    let listing = crate::db::get_listing(&tx, key)?.ok_or_else(|| invalid("证券尚未导入"))?;
    let mut bars = crate::db::list_bars(&tx, listing.id, Timeframe::D1, None, Some(to), 20000)?;
    let version = crate::db::latest_batch_version(&tx)?;
    if !["raw", "qfq", "hfq"].contains(&basis) {
        return Err(invalid("未知价格口径"));
    }
    if basis == "raw" {
        return Ok((
            bars,
            json!({"basis":basis,"source":null,"warning":"原始价格，跨除权收益可能失真"}),
            version,
        ));
    }
    let all = facts_on(&tx, "factors", Some(key), to)?;
    let sources: BTreeSet<_> = all.iter().map(|r| text(r, "source")).collect();
    let chosen = if !source.is_empty() {
        source
    } else if sources.len() == 1 {
        sources.iter().next().copied().unwrap()
    } else {
        return Err(invalid("复权来源缺失或不唯一，请先更新资料并指定来源"));
    };
    let factors: BTreeMap<_, _> = all
        .iter()
        .filter(|r| text(r, "source") == chosen)
        .map(|r| (text(r, "day"), r))
        .collect();
    let anchor = if basis == "qfq" {
        factors.last_key_value()
    } else {
        factors.first_key_value()
    }
    .and_then(|(_, r)| r["payload"]["factor"].as_f64())
    .ok_or_else(|| invalid("缺少复权基准因子"))?;
    let mut used = vec![];
    for bar in &mut bars {
        let date = bar.ts.get(..10).unwrap_or(&bar.ts);
        let factor = factors
            .get(date)
            .and_then(|r| r["payload"]["factor"].as_f64())
            .ok_or_else(|| invalid(&format!("{date} 缺少复权因子，已阻止混合价格口径")))?;
        let ratio = factor / anchor;
        bar.open *= ratio;
        bar.high *= ratio;
        bar.low *= ratio;
        bar.close *= ratio;
        used.push(factors[date]["id"].clone());
    }
    Ok((
        bars,
        json!({"basis":basis,"source":chosen,"anchor_factor":anchor,"factor_versions":used,"warning":"按当前保存版本计算；历史修订的可得性另行核对。成交量额保持原口径。"}),
        version,
    ))
}
pub(super) fn quality(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = key(app, p)?;
    let (from, to) = range(p)?;
    let bars = raw_bars(app, &key, &to)?;
    let rows: Vec<_> = bars
        .iter()
        .filter(|b| b.ts.as_str() >= from.as_str())
        .collect();
    let dates: BTreeSet<_> = rows.iter().filter_map(|b| b.ts.get(..10)).collect();
    let mut issues = vec![];
    for b in &rows {
        if ![b.open, b.high, b.low, b.close]
            .iter()
            .all(|n| n.is_finite() && *n > 0.0)
            || b.low > b.high
            || b.open < b.low
            || b.open > b.high
            || b.close < b.low
            || b.close > b.high
        {
            issues.push(json!({"date":b.ts,"kind":"价格区间无效","source":b.source}));
        }
        if b.volume.is_some_and(|v| v < 0.0) || b.turnover.is_some_and(|v| v < 0.0) {
            issues.push(json!({"date":b.ts,"kind":"成交量额为负","source":b.source}));
        }
    }
    let exchange = key.market.default_exchange();
    let calendar = facts(app, "calendar", None, &to)?;
    let official: BTreeMap<_, _> = calendar
        .iter()
        .filter(|r| text(r, "day") >= from.as_str() && text(&r["payload"], "exchange") == exchange)
        .map(|r| (text(r, "day"), flag(&r["payload"], "is_open")))
        .collect();
    let mut missing = 0;
    for (d, open) in &official {
        if *open && !dates.contains(d) {
            missing += 1;
            if issues.len() < 500 {
                issues.push(
                    json!({"date":d,"kind":"交易日日线缺失或停牌，需核对","source":"交易日历"}),
                );
            }
        }
    }
    let factors = facts(app, "factors", Some(&key), &to)?;
    let covered: BTreeSet<_> = factors.iter().map(|r| text(r, "day")).collect();
    let factor_count = dates.iter().filter(|d| covered.contains(**d)).count();
    let mut notices =
        vec!["缺口不会自动补零或推算；停牌与上市前区间需结合逐日交易约束核对。".into()];
    if official.is_empty() {
        notices.push("没有官方交易日历，无法判断完整交易日缺口；没有用工作日推测节假日。".into());
    }
    Ok(view(
        "数据质量报告",
        json!([{"label":"日线样本","value":rows.len()},{"label":"已知交易日缺口","value":missing},{"label":"复权因子覆盖","value":format!("{factor_count}/{}",dates.len())},{"label":"官方日历天数","value":official.len()}]),
        vec![table(
            "问题与待核对项目",
            &[("date", "日期"), ("kind", "项目"), ("source", "来源")],
            issues,
        )],
        notices,
    ))
}
pub(super) fn sources(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = key(app, p)?;
    let tolerance = num(p, "tolerance_seconds", 30.0)?;
    let threshold = num(p, "deviation_pct", 0.5)?;
    if tolerance < 0.0 || threshold < 0.0 {
        return Err(invalid("比较阈值不能为负"));
    }
    let conn = app.db.connect()?;
    let mut stmt=sql(conn.prepare("SELECT source,ts,last,currency FROM (SELECT *,row_number() OVER(PARTITION BY source ORDER BY ts DESC) rn FROM quote_samples WHERE market=?1 AND symbol=?2) WHERE rn=1"))?;
    let mut candidates = sql(
        stmt.query_map(params![key.market.as_str(), key.symbol], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        }),
    )?
    .map(sql)
    .collect::<Result<Vec<_>, _>>()?;
    if let Ok(detail) = app.show_stock(&key) {
        let q = detail.quote;
        if let (Some(source), Some(ts), Some(last)) = (q.source, q.ts, q.last) {
            if last.is_finite() && last > 0.0 {
                candidates.retain(|r| r.0 != source);
                candidates.push((source, ts, last, q.currency));
            }
        }
    }
    let mut usable: Vec<_> = candidates
        .iter()
        .filter_map(|r| {
            session::parse_cn_ts(&r.1)
                .ok()
                .filter(|ts| *ts <= chrono::Utc::now() && r.2.is_finite() && r.2 > 0.0)
                .map(|ts| (r, ts))
        })
        .collect();
    usable.sort_by_key(|(_, ts)| *ts);
    let anchor = usable.last().copied();
    let rows=usable.iter().map(|(r,ts)|{let a=anchor.unwrap();let aligned=(a.1-*ts).num_seconds() as f64<=tolerance&&r.3==a.0.3;let deviation=aligned.then(||(r.2/a.0.2-1.0)*100.0);json!({"source":r.0,"timestamp":r.1,"last":r.2,"currency":r.3,"deviation_pct":deviation,"status":if !aligned{"时间或币种未对齐"}else if deviation.unwrap().abs()>threshold{"偏差需核对"}else{"可比较"}})}).collect();
    Ok(view(
        "独立来源报价核对",
        json!([{"label":"来源数量","value":candidates.len()}]),
        vec![table(
            "同时间窗口比较",
            &[
                ("source", "来源"),
                ("timestamp", "报价时钟"),
                ("last", "价格"),
                ("currency", "币种"),
                ("deviation_pct", "偏差 %"),
                ("status", "结论"),
            ],
            rows,
        )],
        vec!["只有一个来源时不能证明准确性；缓存和同一源的不同接口不自动认定为独立来源。".into()],
    ))
}
pub(super) fn revisions(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = key(app, p)?;
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT r.id,r.ts,r.observed_at,r.payload FROM bar_revisions r JOIN listings l ON l.id=r.instrument_id WHERE l.market=?1 AND l.symbol=?2 ORDER BY r.id DESC LIMIT 200"))?;
    let rows=sql(s.query_map(params![key.market.as_str(),key.symbol],|r|{let raw:String=r.get(3)?;let v:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);Ok(json!({"revision":r.get::<_,i64>(0)?,"date":r.get::<_,String>(1)?,"observed_at":r.get::<_,String>(2)?,"close":v["close"],"source":v["source"]}))}))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    let mut f=sql(conn.prepare("SELECT id,kind,day,source,observed_at FROM professional_facts WHERE market=?1 AND symbol=?2 ORDER BY id DESC LIMIT 200"))?;
    let facts=sql(f.query_map(params![key.market.as_str(),key.symbol],|r|Ok(json!({"revision":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"date":r.get::<_,String>(2)?,"source":r.get::<_,String>(3)?,"observed_at":r.get::<_,String>(4)?}))))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    Ok(view(
        "历史修订与观测版本",
        json!([]),
        vec![
            table(
                "最近200个日线版本",
                &[
                    ("revision", "版本"),
                    ("date", "数据日期"),
                    ("observed_at", "本地观测时间"),
                    ("close", "收盘"),
                    ("source", "来源"),
                ],
                rows,
            ),
            table(
                "最近200个资料版本",
                &[
                    ("revision", "版本"),
                    ("kind", "资料"),
                    ("date", "数据日期"),
                    ("observed_at", "本地观测时间"),
                    ("source", "来源"),
                ],
                facts,
            ),
        ],
        vec!["旧库导入版本管理时的首个观测时间是升级时间，不反推为历史首次发布时间。".into()],
    ))
}
pub(super) fn sync(app: &App, id: i64, p: &Value) -> Result<Value, AppError> {
    let key = key(app, p)?;
    let (from, to) = range(p)?;
    let code = crate::tushare::key_to_ts_code(&key)
        .ok_or_else(|| invalid("当前自动资料源只支持A股，其他市场可导入授权资料"))?;
    let client = app.tushare_client()?;
    let start = from.replace('-', "");
    let end = to.replace('-', "");
    let mut outcomes = vec![];
    for (i, (api, fields)) in [
        ("adj_factor", "ts_code,trade_date,adj_factor"),
        ("stk_limit", "ts_code,trade_date,up_limit,down_limit"),
        (
            "dividend",
            "ts_code,ann_date,div_proc,record_date,ex_date,pay_date,cash_div_tax,stk_div",
        ),
        ("trade_cal", "exchange,cal_date,is_open"),
    ]
    .iter()
    .enumerate()
    {
        crate::jobs::stage(app, id, &format!("获取 {api}"), 10 + i as u32 * 20)?;
        let params = if *api == "trade_cal" {
            json!({"exchange":key.market.default_exchange(),"start_date":start,"end_date":end})
        } else if *api == "dividend" {
            json!({"ts_code":code})
        } else {
            json!({"ts_code":code,"start_date":start,"end_date":end})
        };
        let result = (|| {
            let table = if matches!(*api, "adj_factor" | "stk_limit") {
                let history_start = if *api == "adj_factor" {
                    app.show_stock(&key)?
                        .bars
                        .from
                        .as_deref()
                        .and_then(|d| d.get(..10))
                        .map(|s| s.replace('-', ""))
                        .unwrap_or(start.clone())
                } else {
                    start.clone()
                };
                client.history_table(api, &code, &history_start, &end, fields)?
            } else if *api == "trade_cal" {
                let known: BTreeSet<_> = facts(app, "calendar", None, &to)?
                    .into_iter()
                    .filter(|r| {
                        text(r, "source") == "Tushare.trade_cal"
                            && text(&r["payload"], "exchange") == key.market.default_exchange()
                    })
                    .map(|r| text(&r, "day").to_string())
                    .collect();
                let mut combined = crate::tushare::Table {
                    fields: vec![],
                    items: vec![],
                };
                let mut next = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").unwrap();
                let through = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d").unwrap();
                while next <= through {
                    if known.contains(&next.to_string()) {
                        next += chrono::Duration::days(1);
                        continue;
                    }
                    let begin = next;
                    while next < through
                        && (next - begin).num_days() < 365
                        && !known.contains(&(next + chrono::Duration::days(1)).to_string())
                    {
                        next += chrono::Duration::days(1);
                    }
                    let page=client.call(api,json!({"exchange":key.market.default_exchange(),"start_date":session::yyyymmdd(begin),"end_date":session::yyyymmdd(next)}),fields)?;
                    if !combined.fields.is_empty() && combined.fields != page.fields {
                        return Err(invalid("交易日历分段字段不一致"));
                    }
                    combined.fields = page.fields;
                    combined.items.extend(page.items);
                    next += chrono::Duration::days(1);
                }
                combined
            } else {
                client.call(api, params, fields)?
            };
            let mut conn = app.db.connect()?;
            let tx = sql(conn.transaction())?;
            let mut count = 0;
            for row in &table.items {
                let r: Value = Value::Object(
                    table
                        .fields
                        .iter()
                        .cloned()
                        .zip(
                            row.as_array()
                                .ok_or_else(|| invalid("供应商行格式无效"))?
                                .iter()
                                .cloned(),
                        )
                        .collect(),
                );
                let date_field = if *api == "trade_cal" {
                    "cal_date"
                } else if *api == "dividend" {
                    "ex_date"
                } else {
                    "trade_date"
                };
                let Ok(d) = day(text(&r, date_field)) else {
                    continue;
                };
                if d < from || d > to {
                    continue;
                }
                let (kind, instrument, mut payload) = match *api {
                    "adj_factor" => ("factors", Some(&key), json!({"factor":r["adj_factor"]})),
                    "stk_limit" => (
                        "constraints",
                        Some(&key),
                        json!({"up_limit":r["up_limit"],"down_limit":r["down_limit"],"suspended":false,"actions_checked":false,"lot_size":if key.symbol.starts_with("688")||key.market.as_str()=="CN.BJ"{1}else{100},"min_buy":if key.symbol.starts_with("688"){200}else{100},"t_plus_one":true}),
                    ),
                    "trade_cal" => (
                        "calendar",
                        None,
                        json!({"exchange":key.market.default_exchange(),"is_open":r["is_open"].as_i64()==Some(1)}),
                    ),
                    // Gross dividend/tax and suspension coverage are preserved as vendor evidence; not certified executable constraints.
                    _ => ("vendor_actions", Some(&key), r.clone()),
                };
                payload["vendor"] = r.clone();
                count += store(
                    &tx,
                    kind,
                    instrument,
                    &d,
                    if *api == "trade_cal" {
                        key.market.default_exchange()
                    } else {
                        ""
                    },
                    &format!("Tushare.{api}"),
                    &payload,
                )?;
            }
            sql(tx.commit())?;
            Ok::<_, AppError>(count)
        })();
        outcomes.push(match result{Ok(n)=>json!({"dataset":api,"status":"已落盘","versions":n,"error":null}),Err(e)=>json!({"dataset":api,"status":"失败，未伪造资料","versions":0,"error":e.to_string()})});
    }
    Ok(view("专业资料更新结果",json!([]),vec![table("接口执行情况",&[("dataset","接口"),("status","状态"),("versions","新增版本"),("error","原因")],outcomes)],vec!["供应商分红为原始税前字段，逐日停牌、净分红及公司行为完整性尚须授权资料核验；未核验日期不允许进入严格成交回测。".into()]))
}
