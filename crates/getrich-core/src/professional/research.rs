use super::*;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn definition(app: &App, kind: &str, name: &str) -> Result<Option<Value>, AppError> {
    let conn = app.db.connect()?;
    sql(conn.query_row("SELECT revision,payload,updated_at FROM professional_definitions WHERE kind=?1 AND name=?2 ORDER BY revision DESC LIMIT 1",params![kind,name],|r|{let raw:String=r.get(1)?;let mut p:Value=serde_json::from_str(&raw).unwrap_or(json!({}));p["revision"]=json!(r.get::<_,i64>(0)?);p["updated_at"]=json!(r.get::<_,String>(2)?);Ok(p)}).optional())
}
pub(super) fn history(app: &App, p: &Value) -> Result<Value, AppError> {
    let kind = text(p, "kind");
    let name = text(p, "name");
    if !["screen", "pool", "template", "study", "strategy", "alert"].contains(&kind)
        || name.is_empty()
    {
        return Err(invalid("请选择类型并填写名称"));
    }
    let current = definition(app, kind, name)?.ok_or_else(|| invalid("定义不存在"))?;
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT revision,payload,updated_at FROM professional_definitions WHERE kind=?1 AND name=?2 ORDER BY revision DESC LIMIT 100"))?;
    let rows=sql(s.query_map(params![kind,name],|r|{let revision:i64=r.get(0)?;let raw:String=r.get(1)?;let mut values:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);values["revision"]=current["revision"].clone();values["restored_from"]=json!(revision);Ok(json!({"revision":revision,"updated_at":r.get::<_,String>(2)?,"content":raw,"load":{"tool":if kind=="alert"{"alerts.save".to_string()}else{format!("{kind}.save")},"values":values}}))}))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    Ok(view("定义历史与恢复基线",json!([{"label":"名称","value":name},{"label":"当前版本","value":current["revision"]}]),vec![table("载入旧内容后保存为新版本",&[("revision","原版本"),("updated_at","保存时间"),("content","完整内容")],rows)],vec!["载入旧内容不会自动恢复；保存时仍按本次载入的当前版本检查冲突，并记录 restored_from 来源。".into()]))
}
pub(super) fn save_definition(app: &App, kind: &str, p: &Value) -> Result<Value, AppError> {
    let name = text(p, "name");
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("名称应为1至80字"));
    }
    let expected = num(p, "revision", 0.0)?;
    if expected < 0.0 || expected.fract() != 0.0 {
        return Err(invalid("版本号无效"));
    }
    let mut payload = p.clone();
    payload["definition_kind"] = json!(kind);
    payload["name"] = json!(name);
    let mut conn = app.db.connect()?;
    let tx = sql(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let revision = sql(tx.query_row(
        "SELECT max(revision) FROM professional_definitions WHERE kind=?1 AND name=?2",
        params![kind, name],
        |r| r.get::<_, Option<i64>>(0),
    ))?
    .unwrap_or(0);
    if revision != expected as i64 {
        return Err(invalid(
            "该名称已有新版本，请从列表载入后再修改；现有记录未覆盖",
        ));
    }
    payload["revision"] = json!(revision + 1);
    sql(tx.execute("INSERT INTO professional_definitions(kind,name,revision,payload,updated_at) VALUES(?1,?2,?3,?4,?5)",params![kind,name,revision+1,payload.to_string(),db::now_rfc3339()]))?;
    sql(tx.commit())?;
    app.events
        .emit("professional.updated", json!({"kind":kind,"name":name}));
    Ok(payload)
}
pub(super) fn save(app: &App, p: &Value) -> Result<Value, AppError> {
    let kind = text(p, "kind");
    if !["screen", "pool", "template", "study", "strategy"].contains(&kind) {
        return Err(invalid("未知研究定义类型"));
    }
    if kind == "pool" && studies::members(app, text(p, "members"))?.is_empty() {
        return Err(invalid("研究池不能为空"));
    }
    if kind == "screen" {
        if num(p, "min_change_pct", 0.0)? > num(p, "max_change_pct", 100.0)?
            || num(p, "min_turnover", 0.0)? < 0.0
        {
            return Err(invalid("选股阈值范围无效"));
        }
        for k in ["min_roe", "max_pe"] {
            if !text(p, k).is_empty() {
                num(p, k, 0.0)?;
            }
        }
        if !text(p, "pool").is_empty() && definition(app, "pool", text(p, "pool"))?.is_none() {
            return Err(invalid("限定研究池不存在"));
        }
    }
    for name in ["thesis", "risks", "plan", "review", "members", "tags"] {
        if text(p, name).chars().count() > 10000 {
            return Err(invalid("研究字段不能超过10000字"));
        }
    }
    let saved = save_definition(app, kind, p)?;
    let mut v = view(
        "研究定义已保存",
        json!([{"label":"名称","value":saved["name"]},{"label":"版本","value":saved["revision"]}]),
        vec![],
        vec!["历史版本保留；修改已有定义时需使用当前版本。".into()],
    );
    v["form_patch"] = json!({"revision":saved["revision"]});
    Ok(v)
}
pub(super) fn list(app: &App, p: &Value) -> Result<Value, AppError> {
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT kind,name,revision,payload,updated_at FROM (SELECT *,row_number() OVER(PARTITION BY kind,name ORDER BY revision DESC) rn FROM professional_definitions) WHERE rn=1 AND kind!='alert' ORDER BY updated_at DESC LIMIT 500"))?;
    let mut rows=sql(s.query_map([],|r|{let kind:String=r.get(0)?;let raw:String=r.get(3)?;let values:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);Ok(json!({"kind":match kind.as_str(){"screen"=>"选股规则","pool"=>"研究池","template"=>"研究模板","study"=>"分析参数","strategy"=>"策略参数",_=>"其他"},"name":r.get::<_,String>(1)?,"revision":r.get::<_,i64>(2)?,"updated_at":r.get::<_,String>(4)?,"tags":values["tags"],"load":{"tool":format!("{kind}.save"),"values":values},"apply":if kind=="study"||kind=="strategy"{json!({"tool":if kind=="study"{"study.chart"}else{"backtest.run"},"values":values})}else{Value::Null}}))}))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    let query = text(p, "query");
    if !query.is_empty() {
        rows.retain(|r| text(r, "name").contains(query) || text(r, "tags").contains(query));
    }
    Ok(view(
        "已保存的研究规则与模板",
        json!([{"label":"记录数量","value":rows.len()}]),
        vec![table(
            "载入当前版本编辑",
            &[
                ("kind", "类型"),
                ("name", "名称"),
                ("revision", "版本"),
                ("tags", "标签"),
                ("updated_at", "更新时间"),
            ],
            rows,
        )],
        vec![],
    ))
}
pub(super) fn facts_map(
    app: &App,
    kind: &str,
    to: &str,
) -> Result<BTreeMap<String, Value>, AppError> {
    let mut all: Vec<(String, Value)> = data::facts(app, kind, None, to)?
        .into_iter()
        .map(|r| {
            (
                format!("{}:{}", text(&r, "market"), text(&r, "symbol")),
                r["payload"].clone(),
            )
        })
        .collect();
    let conn = app.db.connect()?;
    let mut s =
        sql(conn.prepare("SELECT market,symbol,rows_json FROM stock_research WHERE dataset=?1"))?;
    for row in sql(s.query_map([kind], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }))? {
        let (market, symbol, raw) = sql(row)?;
        let rows: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
        for r in rows {
            all.push((format!("{market}:{symbol}"), r));
        }
    }
    let mut out = BTreeMap::new();
    let mut clocks = BTreeMap::new();
    for (key, row) in all {
        let raw = if kind == "financial" {
            text(&row, "ann_date")
        } else {
            let d = text(&row, "trade_date");
            if d.is_empty() {
                text(&row, "date")
            } else {
                d
            }
        };
        let Ok(date) = day(raw) else { continue };
        if date.as_str() > to {
            continue;
        }
        let clock = if kind == "financial" {
            format!("{}:{}", text(&row, "end_date").replace('-', ""), date)
        } else {
            date
        };
        if clocks.get(&key).is_none_or(|old| old <= &clock) {
            clocks.insert(key.clone(), clock);
            out.insert(key, row);
        }
    }
    Ok(out)
}
pub(super) fn screen(app: &App, p: &Value) -> Result<Value, AppError> {
    let rule = definition(app, "screen", text(p, "name"))?
        .ok_or_else(|| invalid("请先保存选股规则，或从列表确认名称"))?;
    let base=app.research_screen(&json!({"min_change_pct":num(&rule,"min_change_pct",0.0)?,"max_change_pct":num(&rule,"max_change_pct",100.0)?,"min_turnover":num(&rule,"min_turnover",0.0)?}))?;
    // Fetch the complete eligible universe; the basic screen's top100 is only a presentation limit.
    let date = base["as_of"]
        .as_str()
        .ok_or_else(|| invalid("没有可比较的本地报价"))?;
    let board = app.quote_board(Some("CN"), None, Some("turnover"), 20000, 0, None)?;
    let finance = facts_map(app, "financial", date)?;
    let valuation = facts_map(app, "valuation", date)?;
    let pool = if !text(&rule, "pool").is_empty() {
        Some(
            studies::members(
                app,
                text(
                    &definition(app, "pool", text(&rule, "pool"))?
                        .ok_or_else(|| invalid("研究池已不可用"))?,
                    "members",
                ),
            )?
            .into_iter()
            .map(|k| k.display())
            .collect::<BTreeSet<_>>(),
        )
    } else {
        None
    };
    let (mut matched, mut excluded) = (0, 0);
    let mut rows = vec![];
    for r in board.rows {
        let key = format!("{}:{}", r.market, r.symbol);
        if r.trade_date.as_deref() != Some(date)
            || r.currency.as_deref() != Some("CNY")
            || !r.has_quote
            || r.ts
                .as_deref()
                .and_then(|t| session::parse_cn_ts(t).ok())
                .is_none_or(|t| t > chrono::Utc::now())
            || pool.as_ref().is_some_and(|pool| !pool.contains(&key))
        {
            excluded += 1;
            continue;
        }
        let (Some(pct), Some(amount)) = (r.change_pct, r.turnover) else {
            excluded += 1;
            continue;
        };
        if pct < num(&rule, "min_change_pct", 0.0)?
            || pct > num(&rule, "max_change_pct", 100.0)?
            || amount < num(&rule, "min_turnover", 0.0)?
        {
            continue;
        }
        let roe = finance.get(&key).and_then(|r| r["roe"].as_f64());
        let pe = valuation.get(&key).and_then(|r| r["pe_ttm"].as_f64());
        if !text(&rule, "min_roe").is_empty() {
            let Some(v) = roe else {
                excluded += 1;
                continue;
            };
            if v < num(&rule, "min_roe", 0.0)? {
                continue;
            }
        }
        if !text(&rule, "max_pe").is_empty() {
            let Some(v) = pe.filter(|v| *v > 0.0) else {
                excluded += 1;
                continue;
            };
            if v > num(&rule, "max_pe", 0.0)? {
                continue;
            }
        }
        matched += 1;
        if rows.len() < 500 {
            rows.push(json!({"stock":key,"name":r.name,"change_pct":pct,"turnover":amount,"roe":roe,"pe":pe,"open_stock":{"symbol":r.symbol,"market":r.market}}));
        }
    }
    let mut v=view("命名规则选股结果",json!([{"label":"规则","value":rule["name"]},{"label":"规则版本","value":rule["revision"]},{"label":"统一日期","value":date},{"label":"匹配","value":matched},{"label":"不可比较或资料不足","value":excluded}]),vec![table("成交额降序（最多500）",&[("stock","证券"),("name","名称"),("change_pct","涨幅 %"),("turnover","成交额 / 元"),("roe","ROE %"),("pe","PE TTM")],rows)],vec!["仅反映条件匹配，不产生自动交易；财务字段按已披露截止日过滤，但旧修订版本可得性尚须来源核验。".into()]);
    v["export_context"] =
        json!({"rule":rule,"trade_date":date,"matched":matched,"excluded":excluded});
    Ok(v)
}
pub(super) fn notes(app: &App, p: &Value) -> Result<Value, AppError> {
    let query = text(p, "query");
    if query.chars().count() > 200 {
        return Err(invalid("检索词最多200字"));
    }
    let mode = text(p, "mode");
    if !mode.is_empty() {
        crate::analysis::research_mode(Some(mode))?;
    }
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT market,symbol,mode,revision,thesis,risks,plan,review,updated_at FROM (SELECT *,row_number() OVER(PARTITION BY market,symbol,mode ORDER BY revision DESC) rn FROM research_journal) WHERE rn=1 AND (?1='' OR mode=?1) AND (?2='' OR instr(thesis,?2)>0 OR instr(risks,?2)>0 OR instr(plan,?2)>0 OR instr(review,?2)>0 OR instr(symbol,?2)>0) ORDER BY updated_at DESC LIMIT 100"))?;
    let rows=sql(s.query_map(params![mode,query],|r|Ok(json!({"market":r.get::<_,String>(0)?,"symbol":r.get::<_,String>(1)?,"mode":r.get::<_,String>(2)?,"revision":r.get::<_,i64>(3)?,"thesis":r.get::<_,String>(4)?,"risks":r.get::<_,String>(5)?,"plan":r.get::<_,String>(6)?,"review":r.get::<_,String>(7)?,"updated_at":r.get::<_,String>(8)?}))))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    Ok(view(
        "跨股票研究检索",
        json!([{"label":"结果（最多100）","value":rows.len()}]),
        vec![table(
            "最新研究版本",
            &[
                ("symbol", "证券"),
                ("market", "市场"),
                ("mode", "研究方式"),
                ("revision", "版本"),
                ("thesis", "研究假设"),
                ("risks", "风险"),
                ("plan", "计划"),
                ("review", "复盘"),
                ("updated_at", "保存时间"),
            ],
            rows,
        )],
        vec![],
    ))
}
pub(super) fn apply_template(app: &App, p: &Value) -> Result<Value, AppError> {
    let template =
        definition(app, "template", text(p, "name"))?.ok_or_else(|| invalid("模板不存在"))?;
    let mode = crate::analysis::research_mode(Some(text(p, "mode")))?;
    let key = data::key(app, p)?;
    let mut draft = app.research_notes(
        &json!({"symbol":key.symbol,"market":key.market.as_str(),"mode":mode}),
        false,
    )?;
    for f in ["thesis", "risks", "plan", "review"] {
        draft[f] = template[f].clone();
    }
    draft["_dirty"] = json!(true);
    Ok(
        json!({"title":"模板已载入草稿","open_research":{"symbol":key.symbol,"market":key.market.as_str(),"mode":mode,"draft":draft},"notices":["模板只载入未保存草稿，核对内容后在研究分析中保存。"]}),
    )
}
pub(super) fn overview(app: &App) -> Result<Value, AppError> {
    let conn = app.db.connect()?;
    let mut counts = vec![];
    for (table, label) in [
        ("professional_facts", "资料版本"),
        ("bar_revisions", "日线版本"),
        ("research_journal", "研究记录版本"),
        ("portfolio_entries", "组合流水"),
        ("strategy_runs", "回测运行"),
    ] {
        let n: i64 =
            sql(conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)))?;
        counts.push(json!({"label":label,"value":n}));
    }
    let unread: i64 = sql(conn.query_row(
        "SELECT count(*) FROM professional_alert_events WHERE acknowledged=0",
        [],
        |r| r.get(0),
    ))?;
    counts.push(json!({"label":"未读预警","value":unread}));
    let mut v=view("专业研究工作台",json!(counts),vec![],vec!["从左侧选择数据、分析、研究、预警、组合或回测。所有研究记录与结果保存到当前本地数据库，行情刷新独立运行。".into(),"行情、分钟和财务的实际可用范围取决于已配置数据源授权。回测只在明确的数据与交易约束覆盖内执行。".into()]);
    v["modules"] = json!([
        "数据质量与修订",
        "复权与技术副图",
        "同期与同行比较",
        "命名规则和研究池",
        "去重预警",
        "组合对账与归因",
        "输入冻结的策略回测"
    ]);
    Ok(v)
}
