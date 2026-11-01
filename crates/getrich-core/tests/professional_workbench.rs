use getrich_core::{
    import::{parse_json, ImportKind},
    professional,
    service::App,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
static ID: AtomicU64 = AtomicU64::new(0);
fn workspace() -> (App, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "getrich-pro04-{}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap(),
        ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    (App::at_path(Default::default(), root.join("test.db")), root)
}
fn run(app: &App, tool: &str, p: Value) -> Value {
    professional::execute(app, tool, &p).unwrap()
}
fn import(app: &App, kind: &str, csv: &str) -> Value {
    run(
        app,
        "data.import",
        json!({"dataset":kind,"source":"fixture.authorized","csv":csv}),
    )
}
fn fixture(app: &App, prices: &[f64]) {
    let rows:Vec<_>=prices.iter().enumerate().map(|(i,p)|json!({"symbol":"600519","market":"CN.SH","ts":(chrono::NaiveDate::from_ymd_opt(2024,1,1).unwrap()+chrono::Duration::days(i as i64)).to_string(),"open":p,"high":p+2.0,"low":p-2.0,"close":p,"volume":100000,"turnover":p*100000.0})).collect();
    app.import_bundle(&parse_json(&json!({"listings":[{"symbol":"600519","market":"CN.SH","industry":"测试行业"}],"bars":rows}).to_string(),ImportKind::Bundle).unwrap(),"fixture").unwrap();
}
fn summary(v: &Value, label: &str) -> Value {
    v["summary"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["label"] == label)
        .unwrap()["value"]
        .clone()
}
#[test]
fn imports_are_atomic_csv_is_quoted_and_reimports_do_not_duplicate_versions() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11.]);
    let bad = "symbol,market,date,factor\n600519,CN.SH,2024-01-01,1\n600519,CN.SH,2024-01-02,-1";
    assert!(professional::execute(
        &app,
        "data.import",
        &json!({"dataset":"factors","source":"x","csv":bad})
    )
    .is_err());
    let csv =
        "symbol,market,date,factor\r\n\"600519\",CN.SH,2024-01-01,1\r\n600519,CN.SH,2024-01-02,2";
    assert_eq!(summary(&import(&app, "factors", csv), "新增版本"), 2);
    assert_eq!(summary(&import(&app, "factors", csv), "新增版本"), 0);
    let restored = csv.replace("2024-01-02,2", "2024-01-02,3");
    assert_eq!(summary(&import(&app, "factors", &restored), "新增版本"), 1);
    assert_eq!(summary(&import(&app, "factors", csv), "新增版本"), 1);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn main_daily_weekly_monthly_candles_and_saved_evidence_share_adjustment_basis() {
    let (app, root) = workspace();
    let values: Vec<_> = (0..270)
        .map(|i| {
            if i < 130 {
                100. + i as f64
            } else {
                (100. + i as f64) / 2.0
            }
        })
        .collect();
    fixture(&app, &values);
    let mut csv = String::from("symbol,market,date,factor\n");
    for i in 0..270 {
        let date = (chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            + chrono::Duration::days(i))
        .to_string();
        csv += &format!("600519,CN.SH,{date},{}\n", if i < 130 { 1 } else { 2 });
    }
    import(&app, "factors", &csv);
    let key = app.parse_key("600519", Some("CN.SH")).unwrap();
    let end = "2024-09-26";
    for tf in [
        getrich_core::models::Timeframe::D1,
        getrich_core::models::Timeframe::W1,
        getrich_core::models::Timeframe::Mo1,
    ] {
        let page = app
            .list_bars_price(&key, tf, None, Some(end), 1, "qfq", "")
            .unwrap();
        assert_eq!(page.price_basis, "qfq");
        assert_eq!(page.rows.len(), 1);
        assert!(
            (page
                .moving_averages
                .iter()
                .find(|m| m.period == 250)
                .unwrap()
                .values[0]
                .unwrap()
                - 122.25)
                .abs()
                < 1e-10
        );
    }
    let report = app
        .analysis_workspace_basis(&key, Some(end), Some("swing"), "qfq", "")
        .unwrap();
    let saved=app.save_research_note(&json!({"symbol":"600519","market":"CN.SH","mode":"swing","basis":"qfq","analysis_as_of":end,"revision":0,"thesis":"复权依据","risks":"","plan":"","review":""})).unwrap();
    assert_eq!(saved["analysis_snapshot"]["price_basis"], "qfq");
    assert!(
        (saved["analysis_snapshot"]["metrics"]["ma20"]
            .as_f64()
            .unwrap()
            - report["metrics"]["ma20"].as_f64().unwrap())
        .abs()
            < 1e-10
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn imported_financial_versions_reach_the_stock_workspace_and_exclude_future_disclosure() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11.]);
    import(&app,"financial","symbol,market,end_date,ann_date,eps,ocfps,roe\n600519,CN.SH,2023-09-30,2023-10-31,2,3,8\n600519,CN.SH,2023-12-31,2024-04-01,4,5,99");
    let key = app.parse_key("600519", Some("CN.SH")).unwrap();
    let report = app
        .analysis_workspace(&key, Some("2024-01-03"), Some("value"))
        .unwrap();
    assert_eq!(report["workspace"]["metrics"][3]["value"], 8.0);
    assert_eq!(report["workspace"]["report_period"], "2023-09-30");
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn named_screen_filters_the_complete_universe_before_applying_the_result_limit() {
    let (app, root) = workspace();
    let listings: Vec<_> = (0..102)
        .map(|i| json!({"symbol":format!("{:06}",600000+i),"market":"CN.SH"}))
        .collect();
    let quotes:Vec<_>=(0..102).map(|i|json!({"symbol":format!("{:06}",600000+i),"market":"CN.SH","ts":"2024-01-03","last":11,"prev_close":10,"turnover":1000000-i})).collect();
    app.import_bundle(
        &parse_json(
            &json!({"listings":listings,"quotes":quotes}).to_string(),
            ImportKind::Bundle,
        )
        .unwrap(),
        "fixture",
    )
    .unwrap();
    import(
        &app,
        "financial",
        "symbol,market,end_date,ann_date,roe\n600101,CN.SH,2023-09-30,2023-10-31,20",
    );
    run(
        &app,
        "screen.save",
        json!({"name":"完整截面","revision":0,"min_change_pct":0,"max_change_pct":100,"min_turnover":0,"min_roe":"10"}),
    );
    let v = run(&app, "screen.run", json!({"name":"完整截面"}));
    assert_eq!(summary(&v, "匹配"), 1);
    assert_eq!(v["tables"][0]["rows"][0]["stock"], "CN.SH:600101");
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn missing_portfolio_price_stays_unknown_instead_of_becoming_zero() {
    let (app, root) = workspace();
    let csv="reference,date,kind,market,symbol,quantity,price,cash,fees\nd,2024-01-01,deposit,,,0,0,10000,0\nb,2024-01-01,buy,CN.SH,600001,100,10,0,5";
    run(
        &app,
        "portfolio.import",
        json!({"account":"缺价","currency":"CNY","csv":csv}),
    );
    let v = run(
        &app,
        "portfolio.report",
        json!({"account":"缺价","to":"2024-01-03"}),
    );
    assert!(summary(&v, "总资产").is_null());
    assert!(summary(&v, "按日初资金流假设的TWR %").is_null());
    assert_eq!(summary(&v, "现金"), 8995.0);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn source_comparison_excludes_unaligned_clocks() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11.]);
    let csv="symbol,market,timestamp,last,volume,turnover,currency\n600519,CN.SH,2024-01-03 10:00:00,10,100,1000,CNY";
    run(
        &app,
        "data.import",
        json!({"dataset":"observations","source":"独立源甲","csv":csv}),
    );
    let other = csv.replace("10:00:00,10,", "10:00:20,11,");
    run(
        &app,
        "data.import",
        json!({"dataset":"observations","source":"独立源乙","csv":other}),
    );
    let v = run(
        &app,
        "data.sources",
        json!({"symbol":"600519","market":"CN.SH","tolerance_seconds":30,"deviation_pct":0.5}),
    );
    assert_eq!(
        v["tables"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["status"] == "偏差需核对")
            .count(),
        1
    );
    let v = run(
        &app,
        "data.sources",
        json!({"symbol":"600519","market":"CN.SH","tolerance_seconds":10}),
    );
    assert_eq!(
        v["tables"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["status"] == "时间或币种未对齐" && r["source"] == "独立源甲")
            .count(),
        1
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn adjustment_preserves_raw_prices_and_uses_only_the_requested_end_anchor() {
    let (app, root) = workspace();
    fixture(&app, &[100., 110., 55., 60.]);
    import(&app,"factors","symbol,market,date,factor\n600519,CN.SH,2024-01-01,1\n600519,CN.SH,2024-01-02,1\n600519,CN.SH,2024-01-03,2\n600519,CN.SH,2024-01-04,2");
    for (basis, expected) in [
        ("raw", vec![100., 110., 55., 60.]),
        ("qfq", vec![50., 55., 55., 60.]),
        ("hfq", vec![100., 110., 110., 120.]),
    ] {
        let v = run(
            &app,
            "study.chart",
            json!({"symbol":"600519","market":"CN.SH","from":"2024-01-01","to":"2024-01-04","basis":basis}),
        );
        let series = v["series"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "收盘")
            .unwrap();
        assert_eq!(
            series["points"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["value"].as_f64().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let v = run(
        &app,
        "study.chart",
        json!({"symbol":"600519","market":"CN.SH","from":"2024-01-01","to":"2024-01-02","basis":"qfq"}),
    );
    assert_eq!(v["provenance"]["anchor_factor"], 1.0);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn indicator_subplots_match_reference_metrics_and_keep_warmup() {
    let (app, root) = workspace();
    fixture(&app, &(0..80).map(|i| 100. + i as f64).collect::<Vec<_>>());
    let v = run(
        &app,
        "study.chart",
        json!({"symbol":"600519","market":"CN.SH","from":"2024-01-01","to":"2024-03-20","display":80}),
    );
    let rsi = v["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "RSI14")
        .unwrap();
    assert!(rsi["points"][13]["value"].is_null());
    assert_eq!(rsi["points"][79]["value"], 100.0);
    let atr = v["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "ATR14")
        .unwrap();
    assert!((atr["points"][79]["value"].as_f64().unwrap() - 4.0).abs() < 1e-10);
    let q = json!({"members":"CN.SH:600519\nUS:AAPL","from":"2024-01-01","to":"2024-03-20"});
    assert!(professional::execute(&app, "study.compare", &q).is_err());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn definitions_are_versioned_pools_validated_and_templates_only_create_drafts() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11.]);
    let p = json!({"name":"研究池一","revision":0,"members":"CN.SH:600519","tags":"半导体,观察"});
    run(&app, "pool.save", p.clone());
    assert!(professional::execute(&app, "pool.save", &p).is_err());
    run(
        &app,
        "template.save",
        json!({"name":"长线模板","revision":0,"thesis":"商业逻辑","risks":"负债","plan":"财报跟踪","review":"复盘"}),
    );
    let v = run(
        &app,
        "template.apply",
        json!({"name":"长线模板","symbol":"600519","market":"CN.SH","mode":"value"}),
    );
    assert_eq!(v["open_research"]["draft"]["thesis"], "商业逻辑");
    assert_eq!(v["open_research"]["draft"]["_dirty"], true);
    assert_eq!(
        app.research_notes(
            &json!({"symbol":"600519","market":"CN.SH","mode":"value"}),
            false
        )
        .unwrap()["revision"],
        0
    );
    assert_eq!(
        run(&app, "definition.list", json!({"query":"半导体"}))["tables"][0]["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn portfolio_cash_positions_fees_dividends_splits_and_corrections_reconcile() {
    let (app, root) = workspace();
    fixture(&app, &[10., 12., 7.]);
    let csv="reference,date,kind,market,symbol,quantity,price,cash,fees,memo\nd1,2024-01-01,deposit,,,0,0,10000,0,入金\nb1,2024-01-01,buy,CN.SH,600519,100,10,0,5,买入\ns1,2024-01-02,sell,CN.SH,600519,40,12,0,5,卖出\nv1,2024-01-03,dividend,CN.SH,600519,0,0,50,0,分红\nx1,2024-01-03,split,CN.SH,600519,60,0,0,0,送股";
    let p = json!({"account":"测试","currency":"CNY","csv":csv});
    assert_eq!(
        summary(&run(&app, "portfolio.import", p.clone()), "新增流水"),
        5
    );
    assert_eq!(summary(&run(&app, "portfolio.import", p), "新增流水"), 0);
    let v = run(
        &app,
        "portfolio.report",
        json!({"account":"测试","to":"2024-01-03"}),
    );
    assert_eq!(summary(&v, "现金"), 9520.0);
    assert_eq!(summary(&v, "总资产"), 10360.0);
    assert_eq!(summary(&v, "已实现价差收益"), 73.0);
    assert_eq!(summary(&v, "未实现收益"), 237.0);
    assert_eq!(summary(&v, "账本对账差额"), 0.0);
    assert!(professional::execute(
        &app,
        "portfolio.correct",
        &json!({"account":"测试","reference":"b1","voided":true,"reason":"测试"})
    )
    .is_err());
    run(
        &app,
        "portfolio.correct",
        json!({"account":"测试","reference":"v1","voided":true,"reason":"重复登记"}),
    );
    assert_eq!(
        summary(
            &run(
                &app,
                "portfolio.report",
                json!({"account":"测试","to":"2024-01-03"})
            ),
            "总资产"
        ),
        10310.0
    );
    run(
        &app,
        "portfolio.correct",
        json!({"account":"测试","reference":"v1","voided":false,"reason":"复核有效"}),
    );
    assert_eq!(
        summary(
            &run(
                &app,
                "portfolio.report",
                json!({"account":"测试","to":"2024-01-03"})
            ),
            "总资产"
        ),
        10360.0
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn stale_quotes_cannot_trigger_alerts_and_fresh_breaches_are_deduplicated() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11.]);
    let quote = |ts: String, last: f64| {
        app.import_bundle(
            &parse_json(
                &json!({"quotes":[{"symbol":"600519","market":"CN.SH","ts":ts,"last":last}]})
                    .to_string(),
                ImportKind::Bundle,
            )
            .unwrap(),
            "fixture",
        )
        .unwrap();
    };
    run(
        &app,
        "alerts.save",
        json!({"name":"突破","symbol":"600519","market":"CN.SH","kind":"price_above","threshold":12,"enabled":true,"cooldown_seconds":60,"revision":0}),
    );
    quote("2024-01-03".into(), 20.);
    assert_eq!(
        summary(&run(&app, "alerts.evaluate", json!({})), "新增事件"),
        0
    );
    quote(chrono::Utc::now().to_rfc3339(), 20.);
    assert_eq!(
        summary(&run(&app, "alerts.evaluate", json!({})), "新增事件"),
        1
    );
    assert_eq!(
        summary(&run(&app, "alerts.evaluate", json!({})), "新增事件"),
        0
    );
    run(&app, "alerts.ack", json!({"id":1}));
    assert_eq!(
        run(&app, "alerts.list", json!({}))["tables"][1]["rows"][0]["acknowledged"],
        true
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
fn backtest_data(app: &App) {
    fixture(app, &(0..60).map(|i| 100. + i as f64).collect::<Vec<_>>());
    let mut calendar = String::from("date,exchange,is_open\n");
    let mut constraints = String::from(
        "symbol,market,date,up_limit,down_limit,suspended,lot_size,t_plus_one,actions_checked\n",
    );
    for i in 0..60 {
        let date = (chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            + chrono::Duration::days(i))
        .to_string();
        calendar += &format!("{date},SSE,1\n");
        constraints += &format!("600519,CN.SH,{date},999,1,0,100,1,1\n");
    }
    import(app, "calendar", &calendar);
    import(app, "constraints", &constraints);
}
#[test]
fn minute_subplots_use_minute_ohlc_and_restart_after_a_known_gap() {
    let (app, root) = workspace();
    fixture(&app, &[999.0]);
    let rows:Vec<_>=(0..40).filter(|i|*i!=35).map(|i|{
        let time=chrono::NaiveDate::from_ymd_opt(2024,1,2).unwrap().and_hms_opt(9,30,0).unwrap()+chrono::Duration::minutes(i);
        let c=100.0+i as f64;
        json!({"time":time.to_string(),"trade_date":"20240102","minute":i,"open":c,"high":c+1.0,"low":c-1.0,"close":c,"volume":100,"amount":100.0*c})
    }).collect();
    let db = root.join("test.db");
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute("INSERT INTO stock_research(market,symbol,dataset,rows_json,source,as_of,updated_at,attempted_at) VALUES('CN.SH','600519','intraday',?1,'fixture.minute','20240102','2024-01-02T16:00:00+08:00','2024-01-02T16:00:00+08:00')",[json!(rows).to_string()]).unwrap();
    let p = json!({"symbol":"600519","market":"CN.SH","from":"2024-01-02","to":"2024-01-02","timeframe":"1m","basis":"raw"});
    let v = run(&app, "study.chart", p.clone());
    assert_eq!(v["timeframe"], "1m");
    assert_eq!(summary(&v, "完整预热样本"), 39);
    assert_eq!(v["provenance"]["missing_minutes"], 1);
    let rsi = v["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "RSI14")
        .unwrap();
    assert_eq!(rsi["points"][34]["value"], 100.0);
    assert!(rsi["points"][35]["value"].is_null());
    assert_eq!(v["tables"][0]["rows"][0]["close"], 139.0);
    let mut adjusted = p;
    adjusted["basis"] = json!("qfq");
    assert!(professional::execute(&app, "study.chart", &adjusted).is_err());
    drop(conn);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn backtest_does_not_borrow_cash_to_pay_fees_on_a_small_sale() {
    let (app, root) = workspace();
    backtest_data(&app);
    let prices: Vec<f64> = (0..60)
        .map(|i| {
            if i < 20 {
                100.0
            } else if i < 22 {
                103.0
            } else {
                3.0
            }
        })
        .collect();
    fixture(&app, &prices);
    let mut constraints = String::from(
        "symbol,market,date,up_limit,down_limit,suspended,lot_size,t_plus_one,actions_checked\n",
    );
    for i in 0..60 {
        let d = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap() + chrono::Duration::days(i);
        constraints += &format!("600519,CN.SH,{d},999,1,0,1,1,1\n");
    }
    import(&app, "constraints", &constraints);
    let p = json!({"symbol":"600519","market":"CN.SH","from":"2024-01-21","to":"2024-01-24","fast":1,"slow":2,"initial_cash":108,"commission_bps":0,"min_commission":5,"sell_tax_bps":0,"slippage_bps":0,"strict_pit":false});
    let v = professional::run_backtest(&app, &p).unwrap();
    assert!(v["blocked"].is_null());
    assert_eq!(summary(&v, "期末现金"), 0.0);
    assert_eq!(summary(&v, "期末股数"), 1.0);
    assert_eq!(v["tables"][0]["rows"].as_array().unwrap().len(), 1);
    assert!(v["tables"][1]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["reason"].as_str().unwrap().contains("费用")));
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn relative_strength_accepts_imported_indices_and_uses_common_trading_dates() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11., 12.]);
    app.import_bundle(&parse_json(&json!({"listings":[{"symbol":"000001","market":"CN.SH","instrument_type":"index","name":"上证指数"}],"bars":[{"symbol":"000001","market":"CN.SH","ts":"2024-01-01","open":100,"high":101,"low":99,"close":100},{"symbol":"000001","market":"CN.SH","ts":"2024-01-03","open":110,"high":111,"low":109,"close":110}]}).to_string(),ImportKind::Bundle).unwrap(),"index.fixture").unwrap();
    let v = run(
        &app,
        "study.compare",
        json!({"members":"CN.SH:600519\nCN.SH:000001","from":"2024-01-01","to":"2024-01-03","basis":"raw"}),
    );
    assert_eq!(summary(&v, "共同交易日"), 2);
    assert_eq!(v["series"].as_array().unwrap().len(), 2);
    assert_eq!(v["series"][0]["points"][0]["value"], 100.0);
    assert!((v["series"][1]["points"][1]["value"].as_f64().unwrap() - 110.0).abs() < 1e-8);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn strict_point_in_time_requires_archives_and_rejects_late_knowledge_or_delisting() {
    let (app, root) = workspace();
    backtest_data(&app);
    let p = json!({"symbol":"600519","market":"CN.SH","from":"2024-01-21","to":"2024-02-29","fast":5,"slow":20,"name":"时点校验"});
    assert_eq!(
        professional::run_backtest(&app, &p).unwrap()["blocked"],
        true
    );
    let mut bars = String::from("symbol,market,date,known_at,open,high,low,close,volume\n");
    let mut calendar = String::from("date,exchange,is_open,known_at\n");
    let mut constraints=String::from("symbol,market,date,up_limit,down_limit,suspended,lot_size,t_plus_one,actions_checked,known_at\n");
    for i in 0..60 {
        let d = (chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap() + chrono::Duration::days(i))
            .to_string();
        let close = 100 + i;
        bars += &format!(
            "600519,CN.SH,{d},{d}T15:05:00+08:00,{close},{},{},{close},100000\n",
            close + 1,
            close - 1
        );
        calendar += &format!("{d},SSE,1,2023-12-01T00:00:00+08:00\n");
        constraints += &format!("600519,CN.SH,{d},999,1,0,100,1,1,{d}T09:00:00+08:00\n");
    }
    import(&app, "bar_history", &bars);
    import(&app, "calendar", &calendar);
    import(&app, "constraints", &constraints);
    import(&app,"security_history","symbol,market,date,known_at,status,list_date,delist_date\n600519,CN.SH,2020-01-01,2019-12-31T12:00:00+08:00,listed,2010-01-01,");
    let first = professional::run_backtest(&app, &p).unwrap();
    assert!(first["blocked"].is_null());
    assert_eq!(first["strict_pit"], true);
    import(&app,"bar_history","symbol,market,date,known_at,open,high,low,close,volume\n600519,CN.SH,2024-01-05,2024-02-01T16:00:00+08:00,500,501,499,500,100000");
    let revised = professional::run_backtest(&app, &p).unwrap();
    assert_eq!(revised["summary"], first["summary"]);
    let replay = professional::replay_backtest(&app, &json!({"id":first["run_id"]})).unwrap();
    assert_eq!(replay["summary"], first["summary"]);
    import(&app,"constraints","symbol,market,date,up_limit,down_limit,suspended,lot_size,t_plus_one,actions_checked,known_at\n600519,CN.SH,2024-02-01,999,1,0,100,1,1,2024-02-02T16:00:00+08:00");
    assert_eq!(
        professional::run_backtest(&app, &p).unwrap()["blocked"],
        true
    );
    import(&app, "constraints", &constraints);
    import(&app,"security_history","symbol,market,date,known_at,status,list_date,delist_date\n600519,CN.SH,2024-02-01,2024-01-20T12:00:00+08:00,delisted,2010-01-01,2024-02-01");
    let blocked = professional::run_backtest(&app, &p).unwrap();
    assert_eq!(blocked["blocked"], true);
    assert!(blocked["tables"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["issue"].as_str().unwrap().contains("退市")));
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn backtest_blocks_missing_coverage_and_freezes_inputs_with_next_day_execution() {
    let (app, root) = workspace();
    fixture(&app, &[10., 11.]);
    let p = json!({"symbol":"600519","market":"CN.SH","from":"2024-01-21","to":"2024-02-29","fast":5,"slow":20,"name":"基准测试","strict_pit":false});
    let blocked = professional::run_backtest(&app, &p).unwrap();
    assert_eq!(blocked["blocked"], true);
    assert!(blocked["series"].is_null());
    backtest_data(&app);
    let first = professional::run_backtest(&app, &p).unwrap();
    assert!(first["blocked"].is_null());
    let trades = &first["tables"][0]["rows"];
    assert!(!trades.as_array().unwrap().is_empty());
    for trade in trades.as_array().unwrap() {
        assert!(trade["signal_date"].as_str().unwrap() < trade["date"].as_str().unwrap());
        assert!(trade["cash_after"].as_f64().unwrap() >= -1e-8);
    }
    let second = professional::run_backtest(&app, &p).unwrap();
    assert_eq!(first["fingerprint"], second["fingerprint"]);
    assert_eq!(first["summary"], second["summary"]);
    let old = run(&app, "backtest.history", json!({"id":first["run_id"]}));
    let revised = json!({"bars":[{"symbol":"600519","market":"CN.SH","ts":"2024-02-29","open":500,"high":501,"low":499,"close":500,"volume":100000}]});
    app.import_bundle(
        &parse_json(&revised.to_string(), ImportKind::Bundle).unwrap(),
        "revision",
    )
    .unwrap();
    let replay = professional::replay_backtest(&app, &json!({"id":first["run_id"]})).unwrap();
    assert_eq!(replay["fingerprint"], first["fingerprint"]);
    assert_eq!(replay["summary"], first["summary"]);
    let snapshot = run(&app, "backtest.inputs", json!({"id":first["run_id"]}));
    assert_eq!(
        snapshot["export_context"]["data"]["bars"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["close"],
        159.0
    );
    for (a, b) in old["summary"]
        .as_array()
        .unwrap()
        .iter()
        .zip(first["summary"].as_array().unwrap())
    {
        assert_eq!(a["label"], b["label"]);
        if let (Some(a), Some(b)) = (a["value"].as_f64(), b["value"].as_f64()) {
            assert!((a - b).abs() < 1e-8);
        } else {
            assert_eq!(a["value"], b["value"]);
        }
    }
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
