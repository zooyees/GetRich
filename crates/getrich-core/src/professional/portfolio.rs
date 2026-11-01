use super::*;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Default)]
struct Position {
    quantity: f64,
    cost: f64,
    realized: f64,
    dividends: f64,
}
#[derive(Default)]
struct Book {
    cash: f64,
    external: f64,
    positions: BTreeMap<String, Position>,
}
fn cents(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}
fn instrument(row: &Value) -> String {
    format!("{}:{}", text(row, "market"), text(row, "symbol"))
}
impl Book {
    fn apply(&mut self, r: &Value) -> Result<f64, AppError> {
        let kind = text(r, "kind");
        let qty = num(r, "quantity", 0.0)?;
        let price = num(r, "price", 0.0)?;
        let fees = num(r, "fees", 0.0)?;
        let cash = num(r, "cash", 0.0)?;
        let mut flow = 0.0;
        match kind {
            "deposit" => {
                self.cash += cash;
                self.external += cash;
                flow = cash;
            }
            "withdraw" => {
                self.cash -= cash;
                self.external -= cash;
                flow = -cash;
            }
            "buy" => {
                let pos = self.positions.entry(instrument(r)).or_default();
                let cost = cents(qty * price) + fees;
                pos.quantity += qty;
                pos.cost += cost;
                self.cash -= cost;
            }
            "sell" => {
                let pos = self.positions.entry(instrument(r)).or_default();
                if qty > pos.quantity + 1e-8 {
                    return Err(invalid(&format!(
                        "流水 {} 卖出数量超过持仓",
                        text(r, "reference")
                    )));
                }
                let cost = if pos.quantity > 0.0 {
                    pos.cost * qty / pos.quantity
                } else {
                    0.0
                };
                let proceeds = cents(qty * price) - fees;
                pos.quantity -= qty;
                pos.cost -= cost;
                pos.realized += proceeds - cost;
                self.cash += proceeds;
                if pos.quantity.abs() < 1e-8 {
                    pos.quantity = 0.0;
                    pos.cost = 0.0;
                }
            }
            "dividend" => {
                self.cash += cash - fees;
                self.positions.entry(instrument(r)).or_default().dividends += cash - fees;
            }
            "split" => {
                let pos = self.positions.entry(instrument(r)).or_default();
                if pos.quantity + qty < -1e-8 {
                    return Err(invalid("公司行为调整后持仓不能为负"));
                }
                pos.quantity += qty;
            }
            _ => return Err(invalid("不支持的流水类型")),
        }
        self.cash = cents(self.cash);
        if self.cash < -0.005 {
            return Err(invalid(&format!(
                "流水 {} 后现金为负，请先导入资金流入或核对顺序",
                text(r, "reference")
            )));
        }
        Ok(flow)
    }
}
fn entries(conn: &Connection, account: &str, to: &str) -> Result<Vec<Value>, AppError> {
    let mut stmt=sql(conn.prepare("SELECT reference,currency,day,kind,market,symbol,quantity,price,cash,fees,memo,id FROM portfolio_entries e WHERE account=?1 AND day<=?2 AND COALESCE((SELECT voided FROM portfolio_corrections c WHERE c.account=e.account AND c.reference=e.reference ORDER BY c.id DESC LIMIT 1),0)=0 ORDER BY day,id"))?;
    let rows=sql(stmt.query_map(params![account,to],|r|Ok(json!({"reference":r.get::<_,String>(0)?,"currency":r.get::<_,String>(1)?,"date":r.get::<_,String>(2)?,"kind":r.get::<_,String>(3)?,"market":r.get::<_,String>(4)?,"symbol":r.get::<_,String>(5)?,"quantity":r.get::<_,f64>(6)?,"price":r.get::<_,f64>(7)?,"cash":r.get::<_,f64>(8)?,"fees":r.get::<_,f64>(9)?,"memo":r.get::<_,String>(10)?,"id":r.get::<_,i64>(11)?}))))?.map(sql).collect();
    rows
}
pub(super) fn import(app: &App, p: &Value) -> Result<Value, AppError> {
    let account = text(p, "account");
    let currency = text(p, "currency");
    if account.is_empty()
        || account.chars().count() > 80
        || !["CNY", "HKD", "USD"].contains(&currency)
    {
        return Err(invalid("账户名称或币种无效"));
    }
    let mut rows = data::csv(text(p, "csv"))?;
    for (i, r) in rows.iter_mut().enumerate() {
        let result = (|| {
            let date = day(text(r, "date"))?;
            if date > today() {
                return Err(invalid("不能导入未来成交"));
            }
            r["date"] = json!(date);
            r["currency"] = json!(currency);
            if text(r, "reference").is_empty() || text(r, "reference").chars().count() > 100 {
                return Err(invalid("流水reference不能为空或超过100字"));
            }
            let kind = text(r, "kind").to_string();
            if !["deposit", "withdraw", "buy", "sell", "dividend", "split"].contains(&kind.as_str())
            {
                return Err(invalid("流水类型无效"));
            }
            for name in ["quantity", "price", "cash", "fees"] {
                r[name] = json!(num(r, name, 0.0)?);
            }
            let qty = num(r, "quantity", 0.0)?;
            let price = num(r, "price", 0.0)?;
            let cash = num(r, "cash", 0.0)?;
            let fees = num(r, "fees", 0.0)?;
            if qty.abs() > 1e12 || price > 1e9 || cash > 1e15 || fees > 1e9 {
                return Err(invalid("流水数值超出记账范围"));
            }
            if fees < 0.0 || cash < 0.0 || cents(fees) != fees || cents(cash) != cash {
                return Err(invalid("现金与费用应为非负金额，最多两位小数"));
            }
            if matches!(kind.as_str(), "deposit" | "withdraw") && fees != 0.0 {
                return Err(invalid(
                    "入出金请填写净现金额，fees填0；交易费用在买卖流水中记录",
                ));
            }
            if matches!(kind.as_str(), "buy" | "sell" | "dividend" | "split") {
                let key = data::key(app, r)?;
                if key.market.default_currency() != currency {
                    return Err(invalid("证券币种与账户记账币种不同，请分别记账"));
                }
                r["symbol"] = json!(key.symbol);
                r["market"] = json!(key.market.as_str());
            } else {
                r["symbol"] = json!("");
                r["market"] = json!("");
            }
            match kind.as_str() {
                "buy" | "sell" if qty <= 0.0 || price <= 0.0 || cash != 0.0 => {
                    return Err(invalid("买卖须填写正数量、正价格，cash填0"))
                }
                "deposit" | "withdraw" | "dividend"
                    if cash <= 0.0 || qty != 0.0 || price != 0.0 =>
                {
                    return Err(invalid("现金类须填写正cash，quantity与price填0"))
                }
                "split" if qty == 0.0 || price != 0.0 || cash != 0.0 || fees != 0.0 => {
                    return Err(invalid("split填写净增加或减少股数，其余金额填0"))
                }
                _ => {}
            }
            Ok(())
        })();
        result.map_err(|e| invalid(&format!("第{}行：{e}", i + 2)))?;
    }
    let mut conn = app.db.connect()?;
    let tx = sql(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let existing_currency: Option<String> = sql(tx
        .query_row(
            "SELECT currency FROM portfolio_entries WHERE account=?1 LIMIT 1",
            [account],
            |r| r.get(0),
        )
        .optional())?;
    if existing_currency.as_deref().is_some_and(|c| c != currency) {
        return Err(invalid("已有账户不能更换币种"));
    }
    let mut added = 0;
    for r in &rows {
        let old:Option<Value>=sql(tx.query_row("SELECT day,kind,market,symbol,quantity,price,cash,fees,memo FROM portfolio_entries WHERE account=?1 AND reference=?2",params![account,text(r,"reference")],|q|Ok(json!({"date":q.get::<_,String>(0)?,"kind":q.get::<_,String>(1)?,"market":q.get::<_,String>(2)?,"symbol":q.get::<_,String>(3)?,"quantity":q.get::<_,f64>(4)?,"price":q.get::<_,f64>(5)?,"cash":q.get::<_,f64>(6)?,"fees":q.get::<_,f64>(7)?,"memo":q.get::<_,String>(8)?}))).optional())?;
        if let Some(old) = old {
            for name in [
                "date", "kind", "market", "symbol", "quantity", "price", "cash", "fees", "memo",
            ] {
                let expected = if name == "memo" && r[name].is_null() {
                    json!("")
                } else {
                    r[name].clone()
                };
                if old[name] != expected {
                    return Err(invalid(
                        "相同reference的流水内容不同，整批未写入；请使用冲正审计流程",
                    ));
                }
            }
            continue;
        }
        sql(tx.execute("INSERT INTO portfolio_entries(account,reference,currency,day,kind,market,symbol,quantity,price,cash,fees,memo,recorded_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![account,text(r,"reference"),currency,text(r,"date"),text(r,"kind"),text(r,"market"),text(r,"symbol"),num(r,"quantity",0.0)?,num(r,"price",0.0)?,num(r,"cash",0.0)?,num(r,"fees",0.0)?,text(r,"memo"),db::now_rfc3339()]))?;
        added += 1;
    }
    let mut book = Book::default();
    for r in entries(&tx, account, &today())? {
        book.apply(&r)?;
    }
    sql(tx.commit())?;
    Ok(view(
        "组合流水对账完成",
        json!([{"label":"新增流水","value":added},{"label":"重复跳过","value":rows.len()-added},{"label":"重建现金","value":book.cash},{"label":"币种","value":currency}]),
        vec![],
        vec!["整批经过持仓与现金校验后原子落盘；此操作只记录本地账本，不向券商提交交易。".into()],
    ))
}
pub(super) fn correct(app: &App, p: &Value) -> Result<Value, AppError> {
    let account = text(p, "account");
    let reference = text(p, "reference");
    if text(p, "reason").is_empty() {
        return Err(invalid("请填写纠错原因"));
    }
    let mut conn = app.db.connect()?;
    let tx = sql(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let exists: i64 = sql(tx.query_row(
        "SELECT count(*) FROM portfolio_entries WHERE account=?1 AND reference=?2",
        params![account, reference],
        |r| r.get(0),
    ))?;
    if exists != 1 {
        return Err(invalid("待纠错流水不存在"));
    }
    sql(tx.execute("INSERT INTO portfolio_corrections(account,reference,voided,reason,recorded_at) VALUES(?1,?2,?3,?4,?5)",params![account,reference,flag(p,"voided"),text(p,"reason"),db::now_rfc3339()]))?;
    let mut book = Book::default();
    for r in entries(&tx, account, &today())? {
        book.apply(&r)?;
    }
    sql(tx.commit())?;
    Ok(view(
        "流水纠错已记录",
        json!([]),
        vec![],
        vec!["原流水保留；报表按最新纠错状态重建，历史报表可能因此重述。".into()],
    ))
}
pub(super) fn report(app: &App, p: &Value) -> Result<Value, AppError> {
    let account = text(p, "account");
    let to = if text(p, "to").is_empty() {
        today()
    } else {
        day(text(p, "to"))?
    };
    if to > today() {
        return Err(invalid("报告日期不能超过今天"));
    }
    let conn = app.db.connect()?;
    let rows = entries(&conn, account, &to)?;
    if rows.is_empty() {
        return Err(invalid("该账户在截止日前没有有效流水"));
    }
    let currency = text(&rows[0], "currency");
    let first = text(&rows[0], "date");
    let mut keys = BTreeSet::new();
    for r in &rows {
        if !text(r, "symbol").is_empty() {
            keys.insert(instrument(r));
        }
    }
    let mut prices = BTreeMap::<String, BTreeMap<String, f64>>::new();
    let mut industries = BTreeMap::new();
    let mut dates: BTreeSet<_> = rows.iter().map(|r| text(r, "date").to_string()).collect();
    dates.insert(to.clone());
    for name in &keys {
        let k = studies::members(app, name)?.remove(0);
        let bars = data::raw_bars(app, &k, &to)?;
        let map: BTreeMap<_, _> = bars
            .into_iter()
            .map(|b| (b.ts[..10].to_string(), b.close))
            .collect();
        dates.extend(map.keys().filter(|d| d.as_str() >= first).cloned());
        prices.insert(name.clone(), map);
        industries.insert(
            name.clone(),
            app.show_stock(&k)
                .ok()
                .and_then(|d| d.listing.industry)
                .unwrap_or("未分类".into()),
        );
    }
    let (mut book, mut index, mut rowindex, mut prev_nav, mut peak, mut drawdown) = (
        Book::default(),
        100.0,
        0usize,
        None::<f64>,
        100.0f64,
        0.0f64,
    );
    let mut nav_points = vec![];
    let mut return_points = vec![];
    let mut incomplete = 0;
    for d in &dates {
        let mut flow = 0.0;
        while rowindex < rows.len() && text(&rows[rowindex], "date") <= d.as_str() {
            flow += book.apply(&rows[rowindex])?;
            rowindex += 1;
        }
        let mut nav = Some(book.cash);
        for (name, pos) in &book.positions {
            if pos.quantity == 0.0 {
                continue;
            }
            let price = prices
                .get(name)
                .and_then(|m| m.range(..=d.clone()).next_back())
                .map(|(_, p)| *p);
            nav = nav.zip(price).map(|(n, p)| n + pos.quantity * p);
        }
        if let Some(value) = nav {
            if let Some(previous) = prev_nav {
                let denom = previous + flow;
                if denom > 0.0 {
                    index *= value / denom;
                } else if value.abs() > 0.005 {
                    incomplete += 1;
                }
            } else if flow > 0.0 {
                index *= value / flow;
            }
            peak = peak.max(index);
            drawdown = drawdown.min((index / peak - 1.0) * 100.0);
        } else {
            incomplete += 1;
        }
        prev_nav = nav;
        nav_points.push(json!({"date":d,"value":nav}));
        return_points
            .push(json!({"date":d,"value":if nav.is_some()&&incomplete==0{Some(index)}else{None}}));
    }
    let mut holdings = vec![];
    let mut sectors = BTreeMap::<String, f64>::new();
    let (mut realized, mut income, mut unrealized, mut market_value) =
        (0.0, 0.0, Some(0.0), Some(0.0));
    for (name, pos) in &book.positions {
        let price = prices
            .get(name)
            .and_then(|m| m.range(..=to.clone()).next_back());
        let value = if pos.quantity == 0.0 {
            Some(0.0)
        } else {
            price.map(|(_, p)| pos.quantity * p)
        };
        let profit = value.map(|v| v - pos.cost);
        realized += pos.realized;
        income += pos.dividends;
        unrealized = unrealized.zip(profit).map(|(a, b)| a + b);
        market_value = market_value.zip(value).map(|(a, b)| a + b);
        let industry = industries.get(name).cloned().unwrap_or("未分类".into());
        if let Some(value) = value {
            *sectors.entry(industry.clone()).or_default() += value;
        }
        holdings.push(json!({"stock":name,"quantity":pos.quantity,"cost":pos.cost,"average_cost":if pos.quantity>0.0{Some(pos.cost/pos.quantity)}else{None},"price":price.map(|(_,p)|*p),"price_date":price.map(|(d,_)|d),"value":value,"realized":pos.realized,"dividends":pos.dividends,"unrealized":profit,"contribution":profit.map(|v|v+pos.realized+pos.dividends),"industry":industry}));
    }
    let nav = market_value.map(|v| v + book.cash);
    let reconciled = nav
        .zip(unrealized)
        .map(|(nav, unrealized)| cents(nav - (book.external + realized + income + unrealized)));
    let sector_rows=sectors.into_iter().map(|(industry,value)|json!({"industry":industry,"value":value,"weight_pct":market_value.filter(|v|*v>0.0).map(|v|value/v*100.0)})).collect();
    let mut v=view("组合对账、收益与归因",json!([{"label":"账户","value":account},{"label":"币种","value":currency},{"label":"现金","value":book.cash},{"label":"总资产","value":nav},{"label":"净投入","value":book.external},{"label":"已实现价差收益","value":realized},{"label":"分红收入（已扣记录费用）","value":income},{"label":"未实现收益","value":unrealized},{"label":"按日初资金流假设的TWR %","value":if incomplete==0{Some(index-100.0)}else{None}},{"label":"单位净值最大回撤 %","value":if incomplete==0{Some(drawdown)}else{None}},{"label":"账本对账差额","value":reconciled}]),vec![table("持仓与累计贡献",&[("stock","证券"),("quantity","数量"),("cost","含费用成本"),("average_cost","均价"),("price","估值价"),("price_date","价格日期"),("value","市值"),("realized","已实现"),("dividends","分红"),("unrealized","未实现"),("contribution","累计贡献"),("industry","行业")],holdings),table("持仓行业集中度",&[("industry","行业"),("value","市值"),("weight_pct","持仓占比 %")],sector_rows),table("有效流水（最近200）",&[("reference","流水号"),("date","日期"),("kind","类型"),("symbol","证券"),("quantity","数量"),("price","价格"),("cash","现金"),("fees","费用"),("memo","备注")],rows.into_iter().rev().take(200).collect())],vec!["估值采用截止日前最近可用日线，价格日期逐项显示；缺少任何持仓价格时，总资产与收益保持未知。".into(),"按移动平均成本法对账，费用计入成本/收益；公司行为以实际流水记录。现金流只有日期，时间加权收益采用日初现金流假设，不宣称精确的盘中TWR。".into()]);
    v["series"] = json!([{"name":"总资产","pane":"账户资产","points":nav_points},{"name":"单位净值","pane":"单位净值 / 初始100","points":return_points}]);
    Ok(v)
}
pub(super) fn accounts(app: &App) -> Result<Value, AppError> {
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT account,currency,count(*),min(day),max(day) FROM portfolio_entries GROUP BY account,currency ORDER BY account"))?;
    let rows=sql(s.query_map([],|r|{let account:String=r.get(0)?;Ok(json!({"account":account,"currency":r.get::<_,String>(1)?,"entries":r.get::<_,i64>(2)?,"from":r.get::<_,String>(3)?,"to":r.get::<_,String>(4)?,"load":{"tool":"portfolio.report","values":{"account":account,"to":today()}}}))}))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    Ok(view(
        "本地组合账户",
        json!([]),
        vec![table(
            "按币种独立记账",
            &[
                ("account", "账户"),
                ("currency", "币种"),
                ("entries", "流水数"),
                ("from", "首日"),
                ("to", "最近流水"),
            ],
            rows,
        )],
        vec![],
    ))
}
