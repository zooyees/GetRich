use super::*;
use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
pub(super) fn save(app: &App, p: &Value) -> Result<Value, AppError> {
    data::key(app, p)?;
    let kind = text(p, "kind");
    if ![
        "price_above",
        "price_below",
        "volume_multiple",
        "announcement",
    ]
    .contains(&kind)
    {
        return Err(invalid("预警类型无效"));
    }
    if kind == "announcement" {
        if text(p, "keyword").is_empty() || text(p, "keyword").chars().count() > 80 {
            return Err(invalid("公告关键词应为1至80字"));
        }
    } else if num(p, "threshold", 0.0)? <= 0.0 {
        return Err(invalid("预警阈值应大于0"));
    }
    if !(60.0..=86400.0).contains(&num(p, "cooldown_seconds", 300.0)?) {
        return Err(invalid("提醒间隔应为60至86400秒"));
    }
    let rule = research::save_definition(app, "alert", p)?;
    let mut v = view(
        "预警规则已保存",
        json!([{"label":"名称","value":rule["name"]},{"label":"版本","value":rule["revision"]}]),
        vec![],
        vec!["仅生成本地观察事件，不提交订单或向外部发送消息。".into()],
    );
    v["form_patch"] = json!({"revision":rule["revision"]});
    Ok(v)
}
fn rules(app: &App) -> Result<Vec<Value>, AppError> {
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT payload FROM (SELECT *,row_number() OVER(PARTITION BY name ORDER BY revision DESC) rn FROM professional_definitions WHERE kind='alert') WHERE rn=1 ORDER BY name LIMIT 1000"))?;
    let rows = sql(s.query_map([], |r| {
        let raw: String = r.get(0)?;
        Ok(serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null))
    }))?
    .map(sql)
    .collect();
    rows
}
pub(super) fn evaluate(app: &App) -> Result<Value, AppError> {
    let mut checked = 0;
    let mut fired = 0;
    let mut unknown = 0;
    for rule in rules(app)?.iter().filter(|r| flag(r, "enabled")) {
        checked += 1;
        let key = data::key(app, rule)?;
        let name = text(rule, "name");
        let revision = num(rule, "revision", 0.0)? as i64;
        let now = Utc::now();
        let kind = text(rule, "kind");
        if kind == "announcement" {
            let research = app.stock_research(&key)?;
            for row in research["datasets"]["announcements"]["rows"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Ok(date) = day(text(row, "ann_date")) else {
                    continue;
                };
                let earliest = (session::shanghai_today() - chrono::Duration::days(1)).to_string();
                if date < earliest
                    || date > today()
                    || !text(row, "title").contains(text(rule, "keyword"))
                {
                    continue;
                }
                let identity = hash(&format!("{}:{}:{}", date, row["title"], row["url"]));
                let evidence = json!({"stock":key.display(),"kind":"公告关键词","condition":rule["keyword"],"date":date,"title":row["title"],"source":research["datasets"]["announcements"]["source"]});
                let conn = app.db.connect()?;
                fired+=sql(conn.execute("INSERT OR IGNORE INTO professional_alert_events(rule,revision,event_key,occurred_at,payload) VALUES(?1,?2,?3,?4,?5)",params![name,revision,identity,db::now_rfc3339(),evidence.to_string()]))?;
            }
            continue;
        }
        let detail = app.show_stock(&key).ok();
        let quote = detail.as_ref().map(|d| &d.quote);
        let fresh = quote
            .and_then(|q| q.ts.as_deref())
            .and_then(|ts| session::parse_cn_ts(ts).ok())
            .is_some_and(|ts| ts <= now && (now - ts).num_seconds() <= 120);
        let condition = if fresh {
            quote.and_then(|q| match kind {
                "price_above" => q
                    .last
                    .map(|v| v > num(rule, "threshold", 0.0).unwrap_or(f64::INFINITY)),
                "price_below" => q
                    .last
                    .map(|v| v < num(rule, "threshold", 0.0).unwrap_or(f64::NEG_INFINITY)),
                "volume_multiple" => {
                    let date = q.trade_date.as_deref().unwrap_or("");
                    let bars = data::raw_bars(app, &key, date).ok()?;
                    let volumes: Option<Vec<_>> = bars
                        .iter()
                        .rev()
                        .filter(|b| b.ts.get(..10).is_some_and(|d| d < date))
                        .take(20)
                        .map(|b| b.volume)
                        .collect();
                    volumes.filter(|v| v.len() == 20).and_then(|v| {
                        let avg = v.iter().sum::<f64>() / 20.0;
                        q.volume
                            .filter(|_| avg > 0.0)
                            .map(|n| n / avg > num(rule, "threshold", 0.0).unwrap_or(f64::INFINITY))
                    })
                }
                _ => None,
            })
        } else {
            None
        };
        let mut conn = app.db.connect()?;
        let tx = sql(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let old: Option<(i64, bool, Option<String>)> = sql(tx
            .query_row(
                "SELECT revision,active,fired_at FROM professional_alert_state WHERE rule=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?;
        let active = old
            .as_ref()
            .is_some_and(|(rev, active, _)| *rev == revision && *active);
        let last = old.as_ref().and_then(|(_, _, s)| s.clone());
        let cooled = last
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|t| {
                (now - t.with_timezone(&Utc)).num_seconds() as f64
                    >= num(rule, "cooldown_seconds", 300.0).unwrap_or(300.0)
            });
        let mut next_active = active;
        let mut last = last;
        if let Some(condition) = condition {
            if !condition {
                next_active = false;
            } else if !active && cooled {
                let q = quote.unwrap();
                let evidence = json!({"stock":key.display(),"kind":kind,"threshold":rule["threshold"],"price":q.last,"volume":q.volume,"quote_time":q.ts,"source":q.source,"freshness_seconds":120});
                let identity = hash(&format!(
                    "{}:{}:{}",
                    q.ts.clone().unwrap_or_default(),
                    q.last.map(|v| v.to_string()).unwrap_or_default(),
                    now.timestamp_millis()
                ));
                let changed=sql(tx.execute("INSERT OR IGNORE INTO professional_alert_events(rule,revision,event_key,occurred_at,payload) VALUES(?1,?2,?3,?4,?5)",params![name,revision,identity,now.to_rfc3339(),evidence.to_string()]))?;
                fired += changed;
                next_active = true;
                last = Some(now.to_rfc3339());
            }
        } else {
            unknown += 1;
        }
        sql(tx.execute("INSERT INTO professional_alert_state(rule,revision,active,fired_at,checked_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(rule) DO UPDATE SET revision=excluded.revision,active=excluded.active,fired_at=excluded.fired_at,checked_at=excluded.checked_at",params![name,revision,next_active,last,now.to_rfc3339()]))?;
        sql(tx.commit())?;
    }
    if fired > 0 {
        app.events
            .emit("professional.alert", json!({"count":fired}));
    }
    Ok(view("本地预警检查",json!([{"label":"启用规则","value":checked},{"label":"新增事件","value":fired},{"label":"时效或资料不足","value":unknown}]),vec![],vec!["价格/量能只接受120秒内的有效报价；未知状态不视为解除条件，持续越界不会重复提醒。公告按披露日及内容去重。".into()]))
}
pub(super) fn list(app: &App) -> Result<Value, AppError> {
    let rules:Vec<_>=rules(app)?.into_iter().map(|r|json!({"name":r["name"],"revision":r["revision"],"stock":format!("{}:{}",text(&r,"market"),text(&r,"symbol")),"condition":r["kind"],"threshold":r["threshold"],"keyword":r["keyword"],"enabled":flag(&r,"enabled"),"load":{"tool":"alerts.save","values":r}})).collect();
    let conn = app.db.connect()?;
    let mut s=sql(conn.prepare("SELECT id,rule,revision,occurred_at,payload,acknowledged FROM professional_alert_events ORDER BY id DESC LIMIT 200"))?;
    let events=sql(s.query_map([],|r|{let raw:String=r.get(4)?;let p:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);Ok(json!({"id":r.get::<_,i64>(0)?,"rule":r.get::<_,String>(1)?,"revision":r.get::<_,i64>(2)?,"time":r.get::<_,String>(3)?,"stock":p["stock"],"price":p["price"],"quote_time":p["quote_time"],"title":p["title"],"source":p["source"],"acknowledged":r.get::<_,bool>(5)?}))}))?.map(sql).collect::<Result<Vec<_>,_>>()?;
    Ok(view(
        "预警规则与触发证据",
        json!([]),
        vec![
            table(
                "规则（载入可修订/停用）",
                &[
                    ("name", "名称"),
                    ("revision", "版本"),
                    ("stock", "证券"),
                    ("condition", "条件"),
                    ("threshold", "阈值"),
                    ("keyword", "公告关键词"),
                    ("enabled", "启用"),
                ],
                rules,
            ),
            table(
                "最近200条事件",
                &[
                    ("id", "编号"),
                    ("rule", "规则"),
                    ("revision", "版本"),
                    ("time", "触发时间"),
                    ("stock", "证券"),
                    ("price", "报价"),
                    ("quote_time", "报价时钟"),
                    ("title", "公告"),
                    ("source", "来源"),
                    ("acknowledged", "已确认"),
                ],
                events,
            ),
        ],
        vec![],
    ))
}
pub(super) fn ack(app: &App, p: &Value) -> Result<Value, AppError> {
    let id = num(p, "id", 0.0)?;
    if id < 1.0 || id.fract() != 0.0 {
        return Err(invalid("事件编号无效"));
    }
    let conn = app.db.connect()?;
    let n = sql(conn.execute(
        "UPDATE professional_alert_events SET acknowledged=1 WHERE id=?1",
        [id as i64],
    ))?;
    if n == 0 {
        return Err(invalid("事件不存在"));
    }
    Ok(view("预警已确认", json!([]), vec![], vec![]))
}
pub(super) fn badge(app: &App) -> Result<Value, AppError> {
    let conn = app.db.connect()?;
    let (count, id): (i64, Option<i64>) = sql(conn.query_row(
        "SELECT count(*),max(id) FROM professional_alert_events WHERE acknowledged=0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    Ok(json!({"unread":count,"latest_id":id.unwrap_or(0)}))
}
