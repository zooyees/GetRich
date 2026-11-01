use getrich_core::{
    analysis,
    import::{parse_json, ImportKind},
    models::{Bar, Timeframe},
    service::App,
};
use serde_json::json;
fn bars(values: &[f64]) -> Vec<Bar> {
    values
        .iter()
        .enumerate()
        .map(|(i, p)| Bar {
            id: i as i64,
            instrument_id: 1,
            timeframe: Timeframe::D1,
            ts: (chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                + chrono::Duration::days(i as i64))
            .to_string(),
            open: *p,
            high: p + 1.0,
            low: p - 1.0,
            close: *p,
            adj_close: None,
            volume: Some(100.0),
            turnover: None,
            source: "fixture".into(),
            ingested_at: String::new(),
        })
        .collect()
}
fn workspace() -> (App, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "getrich-workbench-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    (App::at_path(Default::default(), root.join("test.db")), root)
}
fn cache_dataset(app: &App, root: &std::path::Path, dataset: &str, rows: serde_json::Value) {
    app.migrate().unwrap();
    let conn = rusqlite::Connection::open(root.join("test.db")).unwrap();
    conn.execute("INSERT OR REPLACE INTO stock_research(market,symbol,dataset,rows_json,source,as_of,updated_at,attempted_at,error) VALUES('CN.SH','600519',?1,?2,'synthetic.qa','20240103','2024-01-03T08:00:00Z','2024-01-03T08:00:00Z',NULL)",rusqlite::params![dataset,rows.to_string()]).unwrap();
}
#[test]
fn three_modes_keep_independent_revisions_evidence_and_preferences() {
    let (app, root) = workspace();
    let key = app.parse_key("600519", Some("CN.SH")).unwrap();
    for mode in ["intraday", "swing", "value"] {
        let p = json!({"symbol":"600519","market":"CN.SH","mode":mode,"revision":0,"thesis":mode,"risks":"","plan":"","review":"","analysis_as_of":"2024-01-03"});
        let saved = app.save_research_note(&p).unwrap();
        assert_eq!(saved["revision"], 1);
        assert_eq!(saved["mode"], mode);
        assert_eq!(saved["analysis_snapshot"]["mode"], mode);
        assert_eq!(saved["analysis_as_of"], "2024-01-03");
        assert!(app.save_research_note(&p).is_err());
        assert_eq!(
            app.research_notes(&p, true).unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            app.analysis_workspace(&key, None, Some(mode)).unwrap()["note"]["thesis"],
            mode
        );
    }
    assert_eq!(
        app.stock_analysis(&key, None).unwrap()["note"]["thesis"],
        "swing"
    );
    assert!(app.analysis_workspace(&key, None, Some("invalid")).is_err());
    app.save_settings(&json!({"preferences":{"research_mode":"value"}}))
        .unwrap();
    assert_eq!(
        app.settings_view().unwrap()["preferences"]["research_mode"],
        "value"
    );
    assert!(app
        .save_settings(&json!({"preferences":{"research_mode":"invalid"}}))
        .is_err());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn minute_workspace_rejects_future_and_bad_clocks_and_never_fills_gaps_with_daily_data() {
    let (app, root) = workspace();
    let key = app.parse_key("600519", Some("CN.SH")).unwrap();
    let missing = app
        .analysis_workspace(&key, Some("2024-01-03"), Some("intraday"))
        .unwrap();
    assert_eq!(missing["workspace"]["sample_count"], 0);
    assert!(missing["workspace"]["metrics"][0]["value"].is_null());
    let mut rows:Vec<_>=(0..16).map(|i|json!({"time":format!("2024-01-03 09:{:02}:00",30+i),"trade_date":"20240103","minute":i,"close":10.0+i as f64,"volume":100,"amount":(10+i)*100})).collect();
    rows.push(json!({"time":"2099-01-03 09:30:00","trade_date":"20990103","minute":0,"close":999}));
    rows.push(
        json!({"time":"2024-01-03 09:46:00","trade_date":"20240103","minute":180,"close":999}),
    );
    cache_dataset(&app, &root, "intraday", json!(rows));
    let a = app
        .analysis_workspace(&key, Some("2024-01-03"), Some("intraday"))
        .unwrap();
    assert_eq!(a["workspace"]["sample_count"], 16);
    assert_eq!(a["workspace"]["excluded"], 2);
    assert_eq!(a["workspace"]["fresh"], false);
    assert_eq!(a["workspace"]["metrics"][1]["value"], 17.5);
    assert_eq!(a["workspace"]["metrics"][3]["value"], 150.0);
    rows.remove(5);
    cache_dataset(&app, &root, "intraday", json!(rows));
    let a = app
        .analysis_workspace(&key, Some("2024-01-03"), Some("intraday"))
        .unwrap();
    assert_eq!(a["workspace"]["missing_minutes"], 1);
    assert!(a["workspace"]["metrics"][3]["value"].is_null());
    assert_eq!(
        app.analysis_workspace(&key, Some("2024-01-02"), Some("intraday"))
            .unwrap()["workspace"]["sample_count"],
        0
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn fundamentals_separate_disclosure_period_and_valuation_dates() {
    let (app, root) = workspace();
    cache_dataset(
        &app,
        &root,
        "financial",
        json!([
            {"end_date":"20230930","ann_date":"20231031","roe":8,"eps":2,"ocfps":3},
            {"end_date":"20231231","ann_date":"20240401","roe":99,"eps":4},
            {"end_date":"20231231","roe":888}
        ]),
    );
    cache_dataset(
        &app,
        &root,
        "valuation",
        json!([
            {"trade_date":"20240103","pe_ttm":12,"pb":1.5},
            {"trade_date":"20240104","pe_ttm":999}
        ]),
    );
    let key = app.parse_key("600519", Some("CN.SH")).unwrap();
    let a = app
        .analysis_workspace(&key, Some("2024-01-03"), Some("value"))
        .unwrap();
    let w = &a["workspace"];
    assert_eq!(w["report_period"], "2023-09-30");
    assert_eq!(w["disclosed_at"], "2023-10-31");
    assert_eq!(w["as_of"], "2024-01-03");
    assert_eq!(w["excluded_financial"], 2);
    assert_eq!(w["metrics"][0]["value"], 12.0);
    assert_eq!(w["metrics"][3]["value"], 8.0);
    assert_eq!(w["metrics"][11]["value"], 1.5);
    assert!(app
        .analysis_workspace(&key, Some("2023-01-01"), Some("value"))
        .unwrap()["workspace"]["metrics"][3]["value"]
        .is_null());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn schema8_journal_migrates_to_swing_without_losing_its_history() {
    let (app, root) = workspace();
    let conn = rusqlite::Connection::open(root.join("test.db")).unwrap();
    conn.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE,applied_at TEXT NOT NULL);").unwrap();
    for (i, sql) in [
        include_str!("../migrations/001_init.sql"),
        include_str!("../migrations/002_quote_turnover.sql"),
        include_str!("../migrations/003_pulse.sql"),
        include_str!("../migrations/004_realtime_pulse.sql"),
        include_str!("../migrations/005_workstation.sql"),
        include_str!("../migrations/006_stock_research.sql"),
        include_str!("../migrations/007_provider_cache.sql"),
        include_str!("../migrations/008_research_journal.sql"),
    ]
    .iter()
    .enumerate()
    {
        conn.execute_batch(sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2,'2024-01-03')",
            rusqlite::params![i + 1, format!("fixture-{}", i + 1)],
        )
        .unwrap();
    }
    conn.execute_batch("INSERT INTO research_journal VALUES('CN.SH','600519',1,'原始研究','风险','计划','复盘','2024-01-03',NULL,'{}'); PRAGMA user_version=8;").unwrap();
    drop(conn);
    app.migrate().unwrap();
    assert_eq!(
        app.research_notes(&json!({"symbol":"600519","market":"CN.SH"}), false)
            .unwrap()["thesis"],
        "原始研究"
    );
    assert_eq!(
        app.research_notes(
            &json!({"symbol":"600519","market":"CN.SH","mode":"value"}),
            false
        )
        .unwrap()["revision"],
        0
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn indicators_have_explicit_warmup_and_known_linear_and_flat_results() {
    let linear = bars(&(0..80).map(|i| 100.0 + i as f64).collect::<Vec<_>>());
    let m = analysis::metrics(&linear);
    assert_eq!(m["rsi14"], 100.0);
    assert_eq!(m["atr14"], 2.0);
    assert_eq!(m["ma20"], 169.5);
    assert!((m["macd_dif"].as_f64().unwrap() - 7.0).abs() < 1e-10);
    assert!(m["macd_histogram"].as_f64().unwrap().abs() < 1e-10);
    assert_eq!(m["volume_ratio20"], 1.0);
    assert_eq!(m["max_drawdown60_pct"], 0.0);
    let short = analysis::metrics(&linear[..14]);
    assert!(short["rsi14"].is_null());
    assert!(short["atr14"].is_null());
    assert!(short["volatility20_annual_pct"].is_null());
    let flat = analysis::metrics(&bars(&[100.0; 80]));
    assert_eq!(flat["rsi14"], 50.0);
    assert_eq!(flat["volatility20_annual_pct"], 0.0);
    let down = analysis::metrics(&bars(
        &(0..80).map(|i| 180.0 - i as f64).collect::<Vec<_>>(),
    ));
    assert_eq!(down["rsi14"], 0.0);
}
#[test]
fn drawdown_uses_path_peaks_and_missing_volume_stays_unknown() {
    let mut rows = bars(&[vec![100.0; 30], vec![80.0; 30]].concat());
    rows[59].volume = None;
    let m = analysis::metrics(&rows);
    assert!((m["max_drawdown60_pct"].as_f64().unwrap() + 20.0).abs() < 1e-9);
    assert!(m["volume_ratio20"].is_null());
    assert!(analysis::metrics(&[])["rsi14"].is_null());
}
#[test]
fn historical_analysis_excludes_later_prices_and_never_requires_a_vendor() {
    let (app, root) = workspace();
    let rows:Vec<_>=bars(&(0..80).map(|i|100.0+i as f64).collect::<Vec<_>>()).iter().map(|b|json!({"symbol":"600519","market":"CN.SH","ts":b.ts,"open":b.open,"high":b.high,"low":b.low,"close":b.close})).collect();
    app.import_bundle(
        &parse_json(
            &json!({"listings":[{"symbol":"600519","market":"CN.SH"}],"bars":rows}).to_string(),
            ImportKind::Bundle,
        )
        .unwrap(),
        "fixture",
    )
    .unwrap();
    let key = app.parse_key("600519", Some("CN.SH")).unwrap();
    let a = app.stock_analysis(&key, Some("2024-02-29")).unwrap();
    assert_eq!(a["sample_count"], 60);
    assert_eq!(a["as_of"], "2024-02-29");
    assert!(a["metrics"]["return_60_pct"].is_null());
    let extra=parse_json(&json!({"bars":[{"symbol":"600519","market":"CN.SH","ts":"2024-03-01","open":1000,"high":1001,"low":999,"close":1000}]}).to_string(),ImportKind::Bundle).unwrap();
    app.import_bundle(&extra, "fixture").unwrap();
    assert_eq!(
        a["metrics"],
        app.stock_analysis(&key, Some("2024-02-29")).unwrap()["metrics"]
    );
    assert!(app.stock_analysis(&key, Some("2099-01-01")).is_err());
    let p = json!({"symbol":"600519","market":"CN.SH","revision":0,"thesis":"研究假设","risks":"反证","plan":"观察","review":"","analysis_as_of":"2024-02-29"});
    let saved = app.save_research_note(&p).unwrap();
    assert_eq!(saved["analysis_snapshot"]["metrics"], a["metrics"]);
    let revised=parse_json(&json!({"bars":[{"symbol":"600519","market":"CN.SH","ts":"2024-02-29","open":500,"high":501,"low":499,"close":500}]}).to_string(),ImportKind::Bundle).unwrap();
    app.import_bundle(&revised, "fixture").unwrap();
    assert_ne!(
        app.stock_analysis(&key, Some("2024-02-29")).unwrap()["metrics"],
        a["metrics"]
    );
    assert_eq!(
        app.research_notes(&p, true).unwrap()["rows"][0]["analysis_snapshot"]["metrics"],
        a["metrics"]
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn journal_survives_restart_retains_revisions_and_rejects_stale_writers() {
    let (app, root) = workspace();
    let mut p = json!({"symbol":"600519","market":"CN.SH","revision":0,"thesis":"假设一","risks":"反证","plan":"观察","review":"复盘"});
    assert_eq!(app.save_research_note(&p).unwrap()["revision"], 1);
    assert!(app.save_research_note(&p).is_err());
    p["revision"] = json!(1);
    p["thesis"] = json!("假设二");
    assert_eq!(app.save_research_note(&p).unwrap()["revision"], 2);
    let receipt = app.backup_store().unwrap();
    assert_eq!(receipt["integrity"], "ok");
    let restored = App::at_path(Default::default(), receipt["path"].as_str().unwrap());
    assert_eq!(
        restored.research_notes(&p, false).unwrap()["thesis"],
        "假设二"
    );
    assert_eq!(
        restored.research_notes(&p, true).unwrap()["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(restored);
    drop(app);
    let app = App::at_path(Default::default(), root.join("test.db"));
    assert_eq!(app.research_notes(&p, false).unwrap()["thesis"], "假设二");
    assert_eq!(
        app.research_notes(&p, true).unwrap()["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    p["revision"] = json!(2);
    p["thesis"] = json!("x".repeat(10001));
    assert!(app.save_research_note(&p).is_err());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn screener_uses_one_date_one_currency_and_descending_turnover() {
    let (app, root) = workspace();
    let data = json!({"listings":[{"symbol":"600001","market":"CN.SH"},{"symbol":"600002","market":"CN.SH"},{"symbol":"600003","market":"CN.SH"},{"symbol":"AAPL","market":"US"}],"quotes":[{"symbol":"600001","market":"CN.SH","ts":"2024-01-03","last":11,"prev_close":10,"turnover":200000000},{"symbol":"600002","market":"CN.SH","ts":"2024-01-03","last":12,"prev_close":10,"turnover":300000000},{"symbol":"600003","market":"CN.SH","ts":"2024-01-02","last":13,"prev_close":10,"turnover":900000000},{"symbol":"AAPL","market":"US","currency":"USD","ts":"2024-01-03","last":20,"prev_close":10,"turnover":900000000}]});
    app.import_bundle(
        &parse_json(&data.to_string(), ImportKind::Bundle).unwrap(),
        "fixture",
    )
    .unwrap();
    let s = app.research_screen(&json!({})).unwrap();
    assert_eq!(s["matched"], 2);
    assert_eq!(s["excluded"], 1);
    assert_eq!(s["rows"][0]["symbol"], "600002");
    assert_eq!(s["as_of"], "2024-01-03");
    assert!(app.research_screen(&json!({"board":"US"})).is_err());
    assert!(app.research_screen(&json!({"min_turnover":-1})).is_err());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
