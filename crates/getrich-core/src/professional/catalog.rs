use super::*;
fn field(id: &str, label: &str, kind: &str, default: Value) -> Value {
    json!({"id":id,"label":label,"kind":kind,"default":default})
}
fn s(id: &str, label: &str, default: &str) -> Value {
    field(id, label, "text", json!(default))
}
fn n(id: &str, label: &str, default: f64) -> Value {
    field(id, label, "number", json!(default))
}
fn options(id: &str, label: &str, choices: &[(&str, &str)], default: &str) -> Value {
    let mut f = field(id, label, "select", json!(default));
    f["options"] = json!(choices
        .iter()
        .map(|(v, l)| json!({"value":v,"label":l}))
        .collect::<Vec<_>>());
    f
}
fn stock() -> Vec<Value> {
    vec![
        s("symbol", "证券代码", "600519"),
        options(
            "market",
            "市场",
            &[
                ("CN.SH", "沪市"),
                ("CN.SZ", "深市"),
                ("CN.BJ", "北证"),
                ("HK", "港股"),
                ("US", "美股"),
            ],
            "CN.SH",
        ),
    ]
}
fn dates() -> Vec<Value> {
    vec![
        field(
            "from",
            "开始日期",
            "date",
            json!((session::shanghai_today() - chrono::Duration::days(365)).to_string()),
        ),
        field("to", "截止日期", "date", json!(today())),
    ]
}
fn tool(id: &str, label: &str, group: &str, help: &str, fields: Vec<Value>) -> Value {
    json!({"id":id,"label":label,"group":group,"help":help,"fields":fields})
}
fn extend(mut a: Vec<Value>, b: Vec<Value>) -> Vec<Value> {
    a.extend(b);
    a
}
pub(super) fn catalog() -> Value {
    let basis = options(
        "basis",
        "价格口径",
        &[("raw", "原始价格"), ("qfq", "前复权"), ("hfq", "后复权")],
        "raw",
    );
    let mut tools=vec![
 tool("overview","工作台概览","概览","查看本地资料、研究记录和待处理预警。",vec![]),
 tool("data.quality","数据质量检查","数据","检查价格、量额、官方交易日缺口、数据时钟与复权覆盖。",extend(stock(),dates())),
 tool("data.sources","多源报价核对","数据","只比较同币种、时间差不超过指定秒数的独立来源，不以历史报价代替实时。",extend(stock(),vec![n("tolerance_seconds","来源最大时间差 / 秒",30.0),n("deviation_pct","偏差提示阈值 / %",0.5)])),
 tool("data.revisions","历史修订记录","数据","保留日线和资料的每次观测版本；旧库首次纳入版本管理的时间单独标注。",stock()),
 tool("data.sync","补齐专业数据","数据","后台下载复权因子、交易日历、每日涨跌停及公司行为，成功数据本地保存。",extend(stock(),dates())),
 tool("data.import","导入专业资料","数据","粘贴带表头的 CSV；先校验全部行再原子写入。相同来源与内容重复导入不会新增版本。日期用YYYY-MM-DD，空字段表示未知。",vec![options("dataset","资料类型",&[("factors","复权因子"),("calendar","交易日历"),("observations","独立报价"),("constraints","逐日交易约束"),("actions","公司行为"),("financial","财务版本"),("valuation","估值版本"),("bar_history","时点日线档案"),("security_history","上市/退市历史")],"factors"),s("source","来源名称","本地授权资料"),field("csv","CSV 内容","csv",json!(""))]),
 tool("study.chart","复权与指标副图","分析","显示日线或分钟的MACD、RSI、ATR与均线；按本地可用序列预热，缺因子阻止复权，分钟缺口后重新预热。",extend(extend(stock(),dates()),vec![options("timeframe","指标周期",&[("1d","日线"),("1m","分钟")],"1d"),basis.clone(),s("factor_source","复权来源（留空自动选择单一来源）",""),n("display","显示最近根数",300.0)])),
 tool("study.compare","同期相对强弱","分析","多股按共同交易日期对齐、同币种比较；每条曲线以共同起点归一为100。",extend(dates(),vec![field("members","证券列表（每行 市场:代码）","multiline",json!("CN.SH:600519\nCN.SZ:000001")),basis,s("factor_source","复权来源",""),n("display","显示最近根数",300.0)])),
 tool("study.peers","同行财务比较","分析","限定同一行业和币种，按披露截止日比较财务与估值，报告样本覆盖及中位数。",extend(stock(),vec![field("to","研究截止日","date",json!(today()))])),
 tool("study.scenario","估值情景分析","分析","基于明确的每股收益假设、增长、估值倍数和折现率；展示假设敏感性，不代替投资结论。",vec![n("eps","基期每股收益假设",1.0),n("growth_pct","年增长假设 / %",5.0),n("years","预测年数",3.0),n("pe","期末市盈率假设",15.0),n("discount_pct","折现率 / %",10.0)]),
 tool("study.events","公告与财报事件轴","分析","按披露日期排序，不混入截止日期之后的公告。",extend(stock(),dates())),
 tool("definition.list","规则、研究池与模板","研究","载入已有版本编辑；保存时检查版本冲突，旧版本永久保留。",vec![s("query","名称或标签关键词","")]),
 tool("screen.run","运行命名选股规则","研究","同日人民币截面，支持涨幅、成交额、ROE、PE与研究池限制；缺少必要资料的股票排除并报告原因。",vec![s("name","已保存的规则名称","")]),
 tool("notes.search","跨股票研究检索","研究","检索各股票、研究方式最新版本中的假设、风险、计划及复盘。",vec![s("query","检索关键词",""),options("mode","研究方式",&[("","全部"),("intraday","短线"),("swing","波段"),("value","长线")],"")]),
 tool("template.apply","应用研究模板","研究","将模板载入指定股票的未保存草稿，进入研究分析后核对并保存。",extend(stock(),vec![s("name","模板名称",""),options("mode","研究方式",&[("intraday","短线"),("swing","波段"),("value","长线")],"swing")])),
 tool("alerts.save","建立或修订预警","预警","仅生成本地观察事件，保持条件不重复提醒；旧报价不能触发实时价格预警。",extend(stock(),vec![s("name","规则名称",""),field("revision","版本","hidden",json!(0)),options("kind","条件",&[("price_above","价格高于"),("price_below","价格低于"),("volume_multiple","成交量达到前20日均量倍数"),("announcement","公告含关键词")],"price_above"),n("threshold","价格或量能倍数",100.0),s("keyword","公告关键词",""),n("cooldown_seconds","最短提醒间隔 / 秒",300.0),field("enabled","启用","checkbox",json!(true))])),
 tool("alerts.list","预警规则与事件","预警","查看来源、报价时钟、触发条件和确认状态；载入规则可停用或修订。",vec![]),
 tool("alerts.evaluate","检查当前预警","预警","按当前服务器时钟评估本地资料，不重放历史报价为实时事件。",vec![]),
 tool("alerts.ack","确认已读预警","预警","只改变本地已读状态，保留事件与证据。",vec![n("id","事件编号",1.0)]),
 tool("portfolio.import","导入组合流水","组合","CSV列：reference,date,kind,market,symbol,quantity,price,cash,fees,memo。kind为deposit/withdraw/buy/sell/dividend/split。买卖cash填0，现金类quantity与price填0；split的quantity为新增股数。重复reference校验后跳过，整批对账通过才入库。",vec![s("account","账户名称","我的组合"),options("currency","记账币种",&[("CNY","人民币"),("HKD","港币"),("USD","美元")],"CNY"),field("csv","成交、现金流与公司行为 CSV","csv",json!(""))]),
 tool("portfolio.correct","纠正错误流水","组合","不删除原始流水，追加作废或恢复记录，并重新验证整个账户；会导致负持仓或负现金的纠错被拒绝。",vec![s("account","账户名称","我的组合"),s("reference","待核对的流水reference",""),field("voided","作废此流水（关闭则恢复）","checkbox",json!(true)),s("reason","纠错原因","")]),
 tool("portfolio.report","组合收益与归因","组合","按流水重建现金、成本与已实现收益；只按截止日前可用行情估值，币种不直接合并。",vec![s("account","账户名称","我的组合"),field("to","报告截止日","date",json!(today()))]),
 tool("backtest.history","回测历史与输入快照","回测","结果保存完整参数、数据指纹、原始输入与交易明细；输入不齐时报告缺口而不生成收益曲线。",vec![n("id","结果编号（0列出历史）",0.0)]),
 ];
    for (kind, label, fields) in [
        (
            "screen",
            "保存选股规则",
            vec![
                n("min_change_pct", "最低涨幅 / %", 0.0),
                n("max_change_pct", "最高涨幅 / %", 100.0),
                n("min_turnover", "最低成交额 / 元", 100000000.0),
                s("min_roe", "最低ROE / %（留空不限）", ""),
                s("max_pe", "最高TTM市盈率（留空不限）", ""),
                s("pool", "限定研究池名称（留空不限）", ""),
            ],
        ),
        (
            "pool",
            "保存研究池",
            vec![
                field(
                    "members",
                    "证券列表（每行 市场:代码）",
                    "multiline",
                    json!(""),
                ),
                s("tags", "标签（逗号分隔）", ""),
            ],
        ),
        (
            "template",
            "保存研究模板",
            vec![
                field("thesis", "研究假设提示", "multiline", json!("")),
                field("risks", "风险反证提示", "multiline", json!("")),
                field("plan", "跟踪计划提示", "multiline", json!("")),
                field("review", "复盘提示", "multiline", json!("")),
                s("tags", "标签", ""),
            ],
        ),
    ] {
        let mut f = vec![
            field("kind", "类型", "hidden", json!(kind)),
            s("name", "名称", ""),
            field("revision", "版本", "hidden", json!(0)),
        ];
        f.extend(fields);
        tools.push(tool(
            &format!("{kind}.save"),
            label,
            "研究",
            "按名称保存版本；修改已有记录请先从列表载入。",
            f,
        ));
    }
    tools.push(tool("backtest.run","运行可审查回测","回测","单证券均线策略，前一日收盘决策、下一开盘成交。必须有逐日交易约束和公司行为检查覆盖；历史修订可得性另行披露。费用是用户提供的情景参数，不自动推断历史税制。",extend(extend(stock(),dates()),vec![s("name","运行名称","均线研究"),field("strict_pit","严格时点校验（关闭为历史研究模拟）","checkbox",json!(true)),s("archive_source","时点日线档案来源（多源时指定）",""),s("security_source","上市状态来源（多源时指定）",""),n("fast","快均线周期",5.0),n("slow","慢均线周期",20.0),n("initial_cash","初始资金",100000.0),n("commission_bps","佣金 / 万分之",3.0),n("min_commission","最低佣金 / 元",5.0),n("sell_tax_bps","卖出税费 / 万分之",5.0),n("slippage_bps","单边滑点 / 万分之",5.0),n("participation_pct","单日成交量参与上限 / %",5.0),s("validation_from","样本外开始日期（可选）",""),s("factor_source","复权因子来源（多源时指定）",""),s("constraint_source","已核验约束来源（多源时指定）",""),s("calendar_source","交易日历来源（多源时指定）",""),s("action_source","公司行为来源（多源时指定）","")])));
    tools.push(tool(
        "backtest.inputs",
        "查看与导出输入快照",
        "回测",
        "完整参数与数据版本可随结果导出，便于独立核对和归档。",
        vec![n("id", "运行编号", 1.0)],
    ));
    tools.push(tool(
        "backtest.replay",
        "重放冻结输入",
        "回测",
        "完全使用旧运行保存的输入，重放与存档不一致时停止。",
        vec![n("id", "运行编号", 1.0)],
    ));
    for (id, label, base) in [
        ("study.save", "保存分析参数", "study.chart"),
        ("strategy.save", "保存策略参数", "backtest.run"),
    ] {
        let mut fields = vec![
            s("name", "参数模板名称", ""),
            field("revision", "版本", "hidden", json!(0)),
        ];
        if let Some(definition) = tools.iter().find(|t| t["id"] == base) {
            fields.extend(
                definition["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| f["id"] != "name")
                    .cloned(),
            );
        }
        tools.push(tool(
            id,
            label,
            "研究",
            "复用完整参数；从定义列表可载入修订，或使用参数进入对应分析功能。",
            fields,
        ));
    }
    tools.push(tool(
        "definition.history",
        "定义历史与恢复",
        "研究",
        "载入旧内容后保存为新版本，原版本与恢复来源保留。",
        vec![
            options(
                "kind",
                "定义类型",
                &[
                    ("screen", "选股规则"),
                    ("pool", "研究池"),
                    ("template", "研究模板"),
                    ("study", "分析参数"),
                    ("strategy", "策略参数"),
                    ("alert", "预警"),
                ],
                "screen",
            ),
            s("name", "名称", ""),
        ],
    ));
    tools.push(tool(
        "portfolio.accounts",
        "组合账户列表",
        "组合",
        "载入账户查看收益与流水，币种分别记账。",
        vec![],
    ));
    json!({"groups":["概览","数据","分析","研究","预警","组合","回测"],"tools":tools,"csv_headers":{
 "bar_history":"symbol,market,date,known_at,open,high,low,close,volume,turnover", "security_history":"symbol,market,date,known_at,status,list_date,delist_date", "factors":"symbol,market,date,factor,known_at", "calendar":"date,exchange,is_open,known_at", "observations":"symbol,market,timestamp,last,volume,turnover,currency", "constraints":"symbol,market,date,up_limit,down_limit,suspended,lot_size,min_buy,t_plus_one,actions_checked,no_limit,known_at", "actions":"symbol,market,date,record_date,pay_date,cash_per_share,share_ratio,known_at", "financial":"symbol,market,end_date,ann_date,eps,ocfps,roe,or_yoy,netprofit_yoy,debt_to_assets,grossprofit_margin,netprofit_margin", "valuation":"symbol,market,date,pe_ttm,pb,dv_ttm"}})
}
