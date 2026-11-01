use super::*;
use crate::models::Bar;
use rusqlite::{params, OptionalExtension};
use std::collections::BTreeMap;
#[derive(Clone)]
struct Parameters {
    fast: usize,
    slow: usize,
    initial: f64,
    commission: f64,
    min_fee: f64,
    tax: f64,
    slippage: f64,
    participation: f64,
}
impl Parameters {
    fn read(p: &Value) -> Result<Self, AppError> {
        let f = num(p, "fast", 5.0)?;
        let s = num(p, "slow", 20.0)?;
        let initial = num(p, "initial_cash", 100000.0)?;
        let commission = num(p, "commission_bps", 3.0)?;
        let minimum = num(p, "min_commission", 5.0)?;
        let tax = num(p, "sell_tax_bps", 5.0)?;
        let slippage = num(p, "slippage_bps", 5.0)?;
        let part = num(p, "participation_pct", 5.0)?;
        if !(1.0..=250.0).contains(&f)
            || s <= f
            || s > 1000.0
            || f.fract() != 0.0
            || s.fract() != 0.0
            || !(100.0..=1e9).contains(&initial)
            || [commission, minimum, tax, slippage]
                .iter()
                .any(|n| *n < 0.0 || *n > 1000.0)
            || part <= 0.0
            || part > 100.0
        {
            return Err(invalid(
                "回测参数无效：周期为整数且快线<慢线，资金100至10亿，参与率(0,100]",
            ));
        }
        Ok(Self {
            fast: f as usize,
            slow: s as usize,
            initial,
            commission: commission / 10000.0,
            min_fee: minimum,
            tax: tax / 10000.0,
            slippage: slippage / 10000.0,
            participation: part / 100.0,
        })
    }
    fn scale(&self, f: f64) -> Self {
        let mut p = self.clone();
        p.commission *= f;
        p.min_fee *= f;
        p.tax *= f;
        p.slippage *= f;
        p
    }
}
fn latest_by_day(rows: &[Value]) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for r in rows {
        out.insert(text(r, "day").to_string(), r.clone());
    }
    out
}
fn select_source(rows: Vec<Value>, source: &str, label: &str) -> Result<Vec<Value>, AppError> {
    let sources: std::collections::BTreeSet<_> = rows.iter().map(|r| text(r, "source")).collect();
    if !source.is_empty() {
        if !rows.is_empty() && !sources.contains(source) {
            return Err(invalid(&format!("{label}来源不存在")));
        }
        return Ok(rows
            .into_iter()
            .filter(|r| text(r, "source") == source)
            .collect());
    }
    if sources.len() > 1 {
        return Err(invalid(&format!(
            "{label}有多个来源，请明确指定，避免静默混用"
        )));
    }
    Ok(rows)
}
struct Input {
    bars: Vec<Bar>,
    days: Vec<String>,
    constraints: BTreeMap<String, Value>,
    actions: Vec<Value>,
    factors: BTreeMap<String, Value>,
    from: String,
    to: String,
}
fn simulate(input: &Input, p: &Parameters) -> Value {
    let mut closes = vec![];
    for b in &input.bars {
        let f = input
            .factors
            .get(&b.ts[..10])
            .and_then(|r| r["payload"]["factor"].as_f64())
            .unwrap_or(1.0);
        closes.push(b.close * f);
    }
    let mut prefix = vec![0.0];
    for c in &closes {
        prefix.push(prefix.last().unwrap() + c);
    }
    let bars: BTreeMap<_, _> = input
        .bars
        .iter()
        .enumerate()
        .map(|(i, b)| (b.ts[..10].to_string(), (i, b)))
        .collect();
    let (mut cash, mut shares, mut last_price, mut total_fees, mut turnover, mut peak, mut dd) =
        (p.initial, 0.0, 0.0, 0.0, 0.0, p.initial, 0.0f64);
    let mut pending = Vec::<(String, f64)>::new();
    let mut at_close = BTreeMap::<String, f64>::new();
    let mut transactions = vec![];
    let mut skipped = vec![];
    let mut curve = vec![];
    let mut last_buy = String::new();
    for d in &input.days {
        let mut retained = vec![];
        for (pay, amount) in pending {
            if pay <= *d {
                cash += amount;
            } else {
                retained.push((pay, amount));
            }
        }
        pending = retained;
        for a in input.actions.iter().filter(|r| text(r, "day") == d) {
            let a = &a["payload"];
            let entitlement = at_close.get(text(a, "record_date")).copied().unwrap_or(0.0);
            let new = entitlement * a["share_ratio"].as_f64().unwrap_or(1.0);
            shares += new.floor() - entitlement;
            let amount = entitlement * a["cash_per_share"].as_f64().unwrap_or(0.0);
            if text(a, "pay_date") <= d.as_str() {
                cash += amount;
            } else if amount > 0.0 {
                pending.push((text(a, "pay_date").into(), amount));
            }
        }
        let constraint = &input.constraints[d]["payload"];
        if let Some((i, b)) = bars.get(d) {
            let n = *i;
            let signal = if n >= p.slow {
                let fast = (prefix[n] - prefix[n - p.fast]) / p.fast as f64;
                let slow = (prefix[n] - prefix[n - p.slow]) / p.slow as f64;
                Some(fast > slow)
            } else {
                None
            };
            if let Some(target) = signal {
                let direction = if target { "buy" } else { "sell" };
                let needs = if target { cash > b.open } else { shares > 0.0 };
                if needs {
                    let up = constraint["up_limit"].as_f64();
                    let down = constraint["down_limit"].as_f64();
                    let lot = constraint["lot_size"].as_f64().unwrap_or(100.0);
                    let min_buy = constraint["min_buy"].as_f64().unwrap_or(lot);
                    let price = b.open * (1.0 + if target { p.slippage } else { -p.slippage });
                    let previous_volume = n
                        .checked_sub(1)
                        .and_then(|j| input.bars[j].volume)
                        .unwrap_or(0.0);
                    let capacity = previous_volume.min(b.volume.unwrap_or(0.0)) * p.participation;
                    let capacity = (capacity / lot).floor() * lot;
                    let reason = if flag(constraint, "suspended") {
                        Some("停牌")
                    } else if target && up.is_some_and(|v| b.open >= v - 0.005 || price > v) {
                        Some("开盘涨停或滑点价格越过涨停")
                    } else if !target && down.is_some_and(|v| b.open <= v + 0.005 || price < v) {
                        Some("开盘跌停或滑点价格越过跌停")
                    } else if price > b.high + 1e-8 || price < b.low - 1e-8 {
                        Some("滑点后价格超出当日成交范围")
                    } else if !target && flag(constraint, "t_plus_one") && last_buy == *d {
                        Some("T+1持仓不可卖")
                    } else if capacity <= 0.0 {
                        Some("成交量约束或量能未知")
                    } else {
                        None
                    };
                    if let Some(reason) = reason {
                        skipped.push(json!({"date":d,"direction":direction,"reason":reason}));
                    } else {
                        let affordable = (cash / (price * (1.0 + p.commission)))
                            .min((cash - p.min_fee).max(0.0) / price);
                        let qty = if target {
                            ((affordable / lot).floor() * lot).min(capacity)
                        } else {
                            shares.min(capacity)
                        };
                        if qty > 0.0 && (!target || qty >= min_buy) {
                            let amount = qty * price;
                            let fees = (amount * p.commission).max(p.min_fee)
                                + if target { 0.0 } else { amount * p.tax };
                            if !target && cash + amount < fees {
                                skipped.push(json!({"date":d,"direction":direction,"reason":"现金与卖出收入不足以支付费用，不能产生隐性借款"}));
                            } else {
                                if target {
                                    cash -= amount + fees;
                                    shares += qty;
                                    last_buy = d.clone();
                                } else {
                                    cash += amount - fees;
                                    shares -= qty;
                                }
                                total_fees += fees;
                                turnover += amount;
                                transactions.push(json!({"date":d,"signal_date":input.bars[n-1].ts,"direction":if target{"买入"}else{"卖出"},"quantity":qty,"price":price,"fees":fees,"cash_after":cash,"shares_after":shares}));
                            }
                        }
                    }
                }
            }
            last_price = b.close;
        } else if last_price == 0.0 {
            last_price = input
                .bars
                .iter()
                .rev()
                .find(|b| b.ts.as_str() < d.as_str())
                .map(|b| b.close)
                .unwrap_or(0.0);
        }
        let receivable = pending.iter().map(|(_, v)| v).sum::<f64>();
        let nav = cash + shares * last_price + receivable;
        peak = peak.max(nav);
        dd = dd.min((nav / peak - 1.0) * 100.0);
        curve.push(
            json!({"date":d,"value":nav,"cash":cash,"shares":shares,"receivable":receivable}),
        );
        at_close.insert(d.clone(), shares);
    }
    let final_nav = curve
        .last()
        .and_then(|r| r["value"].as_f64())
        .unwrap_or(p.initial);
    json!({"return_pct":(final_nav/p.initial-1.0)*100.0,"final_nav":final_nav,"cash":cash,"shares":shares,"receivable":pending.iter().map(|(_,n)|n).sum::<f64>(),"fees":total_fees,"drawdown_pct":dd,"turnover_multiple":turnover/p.initial,"transactions":transactions,"skipped":skipped,"curve":curve})
}
fn save(app: &App, p: &Value, snapshot: &Value, mut result: Value) -> Result<Value, AppError> {
    let fingerprint = hash(&snapshot.to_string());
    let mut conn = app.db.connect()?;
    let tx = sql(conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    sql(tx.execute(
        "INSERT INTO strategy_runs(created_at,fingerprint,inputs,result) VALUES(?1,?2,?3,?4)",
        params![
            db::now_rfc3339(),
            fingerprint,
            json!({"parameters":p,"data":snapshot}).to_string(),
            result.to_string()
        ],
    ))?;
    let id = tx.last_insert_rowid();
    result["run_id"] = json!(id);
    result["fingerprint"] = json!(fingerprint);
    sql(tx.execute(
        "UPDATE strategy_runs SET result=?2 WHERE id=?1",
        params![id, result.to_string()],
    ))?;
    sql(tx.commit())?;
    Ok(result)
}
pub(super) fn run(app: &App, p: &Value) -> Result<Value, AppError> {
    let key = data::key(app, p)?;
    let (from, to) = range(p)?;
    let parameters = Parameters::read(p)?;
    let count = (chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d").unwrap()
        - chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").unwrap())
    .num_days();
    if count > 10000 {
        return Err(invalid("单次回测不超过10000个自然日"));
    }
    let conn = app.db.connect()?;
    let snapshot_tx = sql(conn.unchecked_transaction())?;
    let listing =
        crate::db::get_listing(&snapshot_tx, &key)?.ok_or_else(|| invalid("该证券尚未导入"))?;
    let strict = p["strict_pit"].as_bool().unwrap_or(true);
    let (all, point_in_time) = if strict {
        pit::bars(&snapshot_tx, &key, listing.id, &to, p)?
    } else {
        (
            crate::db::list_bars(
                &snapshot_tx,
                listing.id,
                crate::models::Timeframe::D1,
                None,
                Some(&to),
                20000,
            )?,
            Value::Null,
        )
    };
    let begin = all.partition_point(|b| b.ts.as_str() < from.as_str());
    let bars = all[begin.saturating_sub(parameters.slow)..].to_vec();
    let calendar = select_source(
        data::facts_on(&snapshot_tx, "calendar", None, &to)?
            .into_iter()
            .filter(|r| text(&r["payload"], "exchange") == key.market.default_exchange())
            .collect(),
        text(p, "calendar_source"),
        "交易日历",
    )?;
    let calendar: BTreeMap<_, _> = calendar
        .into_iter()
        .filter(|r| text(&r["payload"], "exchange") == key.market.default_exchange())
        .map(|r| (text(&r, "day").to_string(), r))
        .collect();
    let constraints_all = select_source(
        data::facts_on(&snapshot_tx, "constraints", Some(&key), &to)?
            .into_iter()
            .filter(|r| flag(&r["payload"], "actions_checked"))
            .collect(),
        text(p, "constraint_source"),
        "交易约束",
    )?;
    let constraints = latest_by_day(
        &constraints_all
            .iter()
            .filter(|r| flag(&r["payload"], "actions_checked"))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let actions = select_source(
        data::facts_on(&snapshot_tx, "actions", Some(&key), &to)?,
        text(p, "action_source"),
        "公司行为",
    )?;
    let factor_rows = select_source(
        data::facts_on(&snapshot_tx, "factors", Some(&key), &to)?,
        text(p, "factor_source"),
        "复权因子",
    )?;
    let factors = latest_by_day(&factor_rows);
    let bar_map: BTreeMap<_, _> = bars.iter().map(|b| (b.ts[..10].to_string(), b)).collect();
    let mut days = vec![];
    let mut issues = vec![];
    let mut date = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").unwrap();
    while date.to_string() <= to {
        let d = date.to_string();
        if let Some(c) = calendar.get(&d) {
            if flag(&c["payload"], "is_open") {
                days.push(d.clone());
                if let Some(constraint) = constraints.get(&d) {
                    let c = &constraint["payload"];
                    if num(c, "lot_size", 0.0)? <= 0.0 {
                        issues.push(json!({"date":d,"issue":"最小交易单位未知"}));
                    }
                    if !flag(c, "suspended") && !bar_map.contains_key(&d) {
                        issues.push(json!({"date":d,"issue":"非停牌交易日缺少日线"}));
                    }
                } else {
                    issues.push(json!({"date":d,"issue":"缺少已核验公司行为的逐日交易约束"}));
                }
            }
        } else {
            issues.push(json!({"date":d,"issue":"缺少官方交易日历"}));
        }
        date += chrono::Duration::days(1);
    }
    if days.is_empty() {
        issues.push(json!({"date":from,"issue":"没有可验证的开市日"}));
    }
    if bars.len() < parameters.slow + 1 {
        issues.push(json!({"date":from,"issue":"历史不足以预热慢均线并执行下一日成交"}));
    }
    if !actions.is_empty() {
        for b in &bars {
            if !factors.contains_key(&b.ts[..10]) {
                issues.push(
                    json!({"date":b.ts,"issue":"公司行为区间缺复权因子，不能生成连续价格信号"}),
                );
            }
        }
    }
    for b in &bars {
        if b.volume.is_none() || b.volume.is_some_and(|v| v < 0.0) {
            issues.push(json!({"date":b.ts,"issue":"日成交量未知或无效，不能核定成交容量"}));
        }
    }
    let snapshot = json!({"point_in_time":point_in_time,"strict_pit":strict,"symbol":key.symbol,"market":key.market.as_str(),"from":from,"to":to,"bars":bars,"calendar":calendar,"constraints":constraints,"actions":actions,"factors":factors,"engine_version":1});
    if strict {
        issues.extend(pit::validate(&snapshot));
    }
    drop(snapshot_tx);
    drop(conn);
    if !issues.is_empty() {
        let count = issues.len();
        let mut v=view("回测已阻止：数据覆盖不完整",json!([{"label":"待补齐项目","value":count}]),vec![table("缺口（最多500）",&[("date","日期"),("issue","原因")],issues.into_iter().take(500).collect())],vec!["请先补齐授权交易日历、逐日涨跌停/停牌/交易单位和公司行为检查。没有生成收益或交易。".into()]);
        v["blocked"] = json!(true);
        return save(app, p, &snapshot, v);
    }
    let input = Input {
        bars,
        days,
        constraints,
        actions,
        factors,
        from: from.clone(),
        to: to.clone(),
    };
    let result = simulate(&input, &parameters);
    let mut costs = vec![];
    for scale in [0.5, 1.0, 2.0] {
        let r = simulate(&input, &parameters.scale(scale));
        costs.push(json!({"multiplier":scale,"return_pct":r["return_pct"],"fees":r["fees"],"drawdown_pct":r["drawdown_pct"]}));
    }
    let mut segments = vec![];
    if !text(p, "validation_from").is_empty() {
        let split = day(text(p, "validation_from"))?;
        if split <= from || split > to {
            return Err(invalid("样本外起点应晚于开始日且不超过截止日"));
        }
        let warm = Input {
            days: input
                .days
                .iter()
                .filter(|d| *d >= &split)
                .cloned()
                .collect(),
            bars: input.bars.clone(),
            constraints: input.constraints.clone(),
            actions: input.actions.clone(),
            factors: input.factors.clone(),
            from: split.clone(),
            to: to.clone(),
        };
        let oos = simulate(&warm, &parameters);
        let train_nav = result["curve"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|r| text(r, "date") < split.as_str())
            .and_then(|r| r["value"].as_f64());
        segments.push(json!({"segment":"训练观察区间（未优化参数）","return_pct":train_nav.map(|n|(n/parameters.initial-1.0)*100.0)}));
        segments
            .push(json!({"segment":"样本外区间（独立初始现金）","return_pct":oos["return_pct"]}));
    }
    let first = input.bars.iter().find(|b| b.ts.as_str() >= from.as_str());
    let last = input.bars.last();
    let adjusted = |b: &Bar| {
        b.close
            * input
                .factors
                .get(&b.ts[..10])
                .and_then(|r| r["payload"]["factor"].as_f64())
                .unwrap_or(1.0)
    };
    let benchmark = first
        .zip(last)
        .map(|(a, b)| (adjusted(b) / adjusted(a) - 1.0) * 100.0);
    let mut v=view("可追溯的单证券策略回测",json!([{"label":"策略","value":text(p,"name")},{"label":"策略总收益 %","value":result["return_pct"]},{"label":"期末总资产（含应收分红）","value":result["final_nav"]},{"label":"实际费用","value":result["fees"]},{"label":"最大回撤 %","value":result["drawdown_pct"]},{"label":"同证券价格基准 %","value":benchmark},{"label":"超额百分点","value":benchmark.map(|b|result["return_pct"].as_f64().unwrap()-b)},{"label":"成交金额/初始资金","value":result["turnover_multiple"]},{"label":"期末现金","value":result["cash"]},{"label":"期末股数","value":result["shares"]},{"label":"未到账应收分红","value":result["receivable"]}]),vec![table("逐笔成交",&[("date","成交日"),("signal_date","决策依据日"),("direction","方向"),("quantity","数量"),("price","成交价"),("fees","费用"),("cash_after","成交后现金"),("shares_after","成交后股数")],result["transactions"].as_array().unwrap().clone()),table("未成交说明",&[("date","日期"),("direction","方向"),("reason","原因")],result["skipped"].as_array().unwrap().clone()),table("交易成本敏感性",&[("multiplier","成本及滑点倍数"),("return_pct","收益 %"),("fees","费用"),("drawdown_pct","回撤 %")],costs),table("训练与样本外区间",&[("segment","区间"),("return_pct","收益 %")],segments)],vec!["每次决策仅使用前一日及更早收盘数据，下一开盘加用户设定滑点；不使用当日收盘决定当日交易。".into(),"成交量上限采用前一日成交量，同时以当日总量作事后可行性上限；日线无法还原开盘盘口与排队，开盘涨跌停保守跳过，结果是有约束的研究近似。".into(),"公司行为以核验的股权登记日、净现金、派发日和股数比例处理；应收股息计入资产但到账前不能用于买入。费用参数由用户指定，不自动推断历史税制。".into(),"冻结的是当前保存的历史版本；缺少当时观测版本时不宣称点时数据可得。单证券运行不代表无幸存者偏差的全市场选股回测。".into()]);
    v["series"] = json!([{"name":"策略资产","pane":"资产曲线","points":result["curve"]}]);
    v["strict_pit"] = json!(strict);
    v["notices"].as_array_mut().unwrap().push(json!(if strict {"严格时点模式：按来源声明的known_at或真实观测时间核验；日线信号冻结首个在次日09:25前可得的版本，不回填后续修订。此模式不核证供应商声明的真实性，也不代表全市场无幸存者偏差。"}else{"历史研究模式：未验证当时可得性，不应将结果标为严格点时回测。"}));
    v["methodology"] = json!({"signal":"previous_close_fast_ma_above_slow_ma","execution":"next_open_with_slippage","initial_cash":parameters.initial,"from":input.from,"to":input.to});
    save(app, p, &snapshot, v)
}
pub(super) fn history(app: &App, p: &Value) -> Result<Value, AppError> {
    let id = num(p, "id", 0.0)?;
    if id < 0.0 || id.fract() != 0.0 {
        return Err(invalid("运行编号无效"));
    }
    let conn = app.db.connect()?;
    if id > 0.0 {
        let row: Option<String> = sql(conn
            .query_row(
                "SELECT result FROM strategy_runs WHERE id=?1",
                [id as i64],
                |r| r.get(0),
            )
            .optional())?;
        return row
            .map(|s| serde_json::from_str(&s).map_err(|e| invalid(&e.to_string())))
            .unwrap_or_else(|| Err(invalid("回测记录不存在")));
    }
    let mut s=sql(conn.prepare("SELECT id,created_at,fingerprint,inputs,result FROM strategy_runs ORDER BY id DESC LIMIT 100"))?;
    let rows=sql(s.query_map([],|r|{let input:String=r.get(3)?;let result:String=r.get(4)?;let p:Value=serde_json::from_str(&input).unwrap_or(Value::Null);let v:Value=serde_json::from_str(&result).unwrap_or(Value::Null);let id:i64=r.get(0)?;Ok(json!({"id":id,"created_at":r.get::<_,String>(1)?,"fingerprint":r.get::<_,String>(2)?,"name":p["parameters"]["name"],"stock":p["data"]["symbol"],"status":if v["blocked"]==true{"资料不足，未运行"}else{"已完成"},"load":{"tool":"backtest.history","values":{"id":id}}}))}))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    Ok(view(
        "回测历史",
        json!([]),
        vec![table(
            "最近100次运行",
            &[
                ("id", "编号"),
                ("name", "名称"),
                ("stock", "证券"),
                ("created_at", "时间"),
                ("fingerprint", "输入指纹"),
                ("status", "状态"),
            ],
            rows,
        )],
        vec!["完整输入快照随结果保存在本地数据库中，后续修订不会改写旧运行。".into()],
    ))
}
fn frozen(app: &App, p: &Value) -> Result<(Value, Value), AppError> {
    let id = num(p, "id", 0.0)?;
    if id < 1.0 || id.fract() != 0.0 {
        return Err(invalid("请填写有效运行编号"));
    }
    let conn = app.db.connect()?;
    let row: Option<(String, String)> = sql(conn
        .query_row(
            "SELECT inputs,result FROM strategy_runs WHERE id=?1",
            [id as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    let (inputs, result) = row.ok_or_else(|| invalid("运行记录不存在"))?;
    Ok((
        serde_json::from_str(&inputs).map_err(|_| invalid("输入快照损坏"))?,
        serde_json::from_str(&result).map_err(|_| invalid("结果记录损坏"))?,
    ))
}
pub(super) fn inputs(app: &App, p: &Value) -> Result<Value, AppError> {
    let (inputs, _) = frozen(app, p)?;
    let d = &inputs["data"];
    let mut v=view("冻结输入快照",json!([{"label":"证券","value":d["symbol"]},{"label":"开始","value":d["from"]},{"label":"截止","value":d["to"]},{"label":"引擎版本","value":d["engine_version"]},{"label":"日线根数","value":d["bars"].as_array().map_or(0,|a|a.len())},{"label":"输入指纹","value":hash(&d.to_string())}]),vec![],vec!["导出完整结果与口径将包含不可变参数、日线、交易日历、逐日约束、公司行为与复权因子的输入版本。".into()]);
    v["export_context"] = inputs;
    Ok(v)
}
pub(super) fn replay(app: &App, p: &Value) -> Result<Value, AppError> {
    let (inputs, mut saved) = frozen(app, p)?;
    let d = &inputs["data"];
    if saved["blocked"] == true {
        return Err(invalid("该记录因资料不足未执行，请补齐后创建新的运行"));
    }
    if d["engine_version"] != 1 {
        return Err(invalid("存档引擎版本不受当前重放器支持"));
    }
    let decode = |field: &str| -> Result<BTreeMap<String, Value>, AppError> {
        serde_json::from_value(d[field].clone()).map_err(|_| invalid("输入快照字段损坏"))
    };
    let calendar = decode("calendar")?;
    let from = text(d, "from").to_string();
    let to = text(d, "to").to_string();
    let input = Input {
        bars: serde_json::from_value(d["bars"].clone()).map_err(|_| invalid("日线快照损坏"))?,
        days: calendar
            .iter()
            .filter(|(day, r)| *day >= &from && *day <= &to && flag(&r["payload"], "is_open"))
            .map(|(day, _)| day.clone())
            .collect(),
        constraints: decode("constraints")?,
        actions: serde_json::from_value(d["actions"].clone())
            .map_err(|_| invalid("公司行为快照损坏"))?,
        factors: decode("factors")?,
        from,
        to,
    };
    let parameters = Parameters::read(&inputs["parameters"])?;
    let reproduced = simulate(&input, &parameters);
    let expected = saved["summary"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["label"] == "期末总资产（含应收分红）")
        .and_then(|r| r["value"].as_f64())
        .ok_or_else(|| invalid("存档缺少期末资产"))?;
    if (reproduced["final_nav"].as_f64().unwrap() - expected).abs() > 0.000001 {
        return Err(invalid(
            "当前引擎与存档结果不一致，已停止重放；请核对引擎版本",
        ));
    }
    saved["title"] = json!("冻结输入重放通过");
    saved["replayed_from"] = p["id"].clone();
    saved["export_context"] = inputs.clone();
    save(app, &inputs["parameters"], d, saved)
}
