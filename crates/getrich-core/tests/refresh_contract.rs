use chrono::{Datelike, Duration, NaiveDate, Weekday};
use getrich_core::{
    bar_refresh,
    import::{self, ImportKind},
    models::Timeframe,
    service::App,
};
use serde_json::json;
use std::{
    hash::{Hash, Hasher},
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn concurrent_bar_reads_do_not_attach_a_newer_revision_to_older_ohlc() {
    use std::sync::{atomic::AtomicBool, Arc};
    let (app, root) = workspace();
    let app = Arc::new(app);
    let bar = |close: f64| json!({"symbol":"600001","market":"CN.SH","timeframe":"1d","ts":"2026-10-23","open":10.0,"high":11.0,"low":9.0,"close":close});
    app.import_bundle(
        &import::parse_json(
            &json!({"listings":[{"symbol":"600001","market":"CN.SH"}],"bars":[bar(10.0)]})
                .to_string(),
            ImportKind::Bundle,
        )
        .unwrap(),
        "fixture",
    )
    .unwrap();
    let key = app.parse_key("600001", Some("CN.SH")).unwrap();
    let version = app
        .list_bars(&key, Timeframe::D1, None, None, 20000)
        .unwrap()
        .data_version;
    let done = Arc::new(AtomicBool::new(false));
    let writer_app = Arc::clone(&app);
    let finished = Arc::clone(&done);
    let writer = std::thread::spawn(move || {
        for i in 1..=40 {
            let close = 10.0 + i as f64 / 100.0;
            let bundle=import::parse_json(&json!({"quotes":[{"symbol":"600001","market":"CN.SH","ts":format!("2026-10-23T02:00:{i:02}Z"),"last":close,"prev_close":10.0}],"bars":[bar(close)]}).to_string(),ImportKind::Bundle).unwrap();
            import::apply_quotes_hot(
                &writer_app.db,
                &bundle.quotes,
                &bundle.bars,
                "fixture.rt_k",
                1000,
            )
            .unwrap();
        }
        finished.store(true, Ordering::SeqCst);
    });
    let mut reads = 0;
    while !done.load(Ordering::SeqCst) || reads < 2 {
        let page = app
            .list_bars(&key, Timeframe::D1, None, None, 20000)
            .unwrap();
        let expected = 10.0 + (page.data_version - version) as f64 / 100.0;
        assert!(
            (page.rows[0].close - expected).abs() < 1e-8,
            "OHLC and data version must come from the same transaction"
        );
        reads += 1;
    }
    writer.join().unwrap();
    assert!(reads >= 2);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
fn workspace() -> (App, std::path::PathBuf) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "getrich-refresh-contract-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    let app = App::at_path(Default::default(), root.join("test.db"));
    app.migrate().unwrap();
    (app, root)
}
#[test]
fn daily_weekly_monthly_tails_match_a_complete_reload_including_six_averages() {
    let (app, root) = workspace();
    let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let dates: Vec<_> = (0..600)
        .map(|n| start + Duration::days(n))
        .filter(|d| !matches!(d.weekday(), Weekday::Sat | Weekday::Sun))
        .take(302)
        .collect();
    let row = |i: usize| json!({"symbol":"600001","market":"CN.SH","timeframe":"1d","ts":dates[i].to_string(),"open":100.0+i as f64,"high":102.0+i as f64,"low":99.0+i as f64,"close":101.0+i as f64,"volume":1000.0});
    let bundle=import::parse_json(&json!({"listings":[{"symbol":"600001","market":"CN.SH"}],"bars":(0..301).map(row).collect::<Vec<_>>()} ).to_string(),ImportKind::Bundle).unwrap();
    app.import_bundle(&bundle, "fixture").unwrap();
    let key = app.parse_key("600001", Some("CN.SH")).unwrap();
    let mut pages: Vec<_> = [Timeframe::D1, Timeframe::W1, Timeframe::Mo1]
        .into_iter()
        .map(|tf| (tf, app.list_bars(&key, tf, None, None, 20000).unwrap()))
        .collect();
    let before = app
        .quote_board(Some("CN"), None, None, 20, 0, None)
        .unwrap()
        .stats
        .bars;
    app.import_bundle(
        &import::parse_json(&json!({"bars":[row(301)]}).to_string(), ImportKind::Bundle).unwrap(),
        "fixture",
    )
    .unwrap();
    assert_eq!(
        app.quote_board(Some("CN"), None, None, 20, 0, None)
            .unwrap()
            .stats
            .bars,
        before + 1
    );
    for (tf, base) in &mut pages {
        let from = bar_refresh::tail_from(tf.as_str(), &base.rows.last().unwrap().ts).unwrap();
        let tail = app.list_bars(&key, *tf, Some(&from), None, 20000).unwrap();
        assert!(
            tail.rows.len() <= 23,
            "tail must not return years of history"
        );
        let old = tail.clone();
        assert!(bar_refresh::merge_tail(base, tail, &from));
        let full = app.list_bars(&key, *tf, None, None, 20000).unwrap();
        assert_eq!(
            serde_json::to_value(&*base).unwrap(),
            serde_json::to_value(full).unwrap(),
            "{tf:?}"
        );
        let mut stale = old;
        stale.data_version = 0;
        assert!(!bar_refresh::merge_tail(base, stale, &from));
    }
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn cached_maps_are_reused_but_a_committed_quote_invalidates_the_result() {
    let (app, root) = workspace();
    let demo = import::parse_json(
        include_str!("../../../docs/examples/demo.board.json"),
        ImportKind::Bundle,
    )
    .unwrap();
    app.import_bundle(&demo, "fixture").unwrap();
    let query = getrich_core::market_map::MapQuery {
        metric: "equal".into(),
        ..Default::default()
    };
    let first = app.market_map(query.clone()).unwrap();
    let second = app.market_map(query.clone()).unwrap();
    assert_eq!(first.computed_at, second.computed_at);
    let next=import::parse_json(&json!({"quotes":[{"symbol":"600519","market":"CN.SH","ts":(chrono::Utc::now()-Duration::seconds(1)).to_rfc3339(),"last":42.0,"prev_close":40.0}]}).to_string(),ImportKind::Bundle).unwrap();
    let report =
        import::apply_quotes_hot(&app.db, &next.quotes, &[], "fixture.rt_k", 1000).unwrap();
    assert_eq!(report.accepted, 1);
    let latest = app.market_map(query).unwrap();
    assert!(latest.data_version > first.data_version);
    assert_eq!(
        latest
            .groups
            .iter()
            .flat_map(|g| &g.tiles)
            .find(|t| t.quote.symbol == "600519")
            .unwrap()
            .quote
            .last,
        Some(42.0)
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn a_configured_token_and_historical_sync_do_not_claim_live_permission_or_success() {
    let (app, root) = workspace();
    let mut cfg = app.cfg.clone();
    cfg.data.tushare.token = "fixture-live-permission".into();
    cfg.data.tushare.enabled = true;
    drop(app);
    let app = App::at_path(cfg, root.join("test.db"));
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    "fixture-live-permission".hash(&mut hash);
    let conn = app.db.connect().unwrap();
    conn.execute(
        "INSERT INTO provider_backoff(account,until_ms) VALUES(?1,?2)",
        rusqlite::params![
            format!("{:016x}:rt_k:permission", hash.finish()),
            chrono::Utc::now().timestamp_millis() + 600000
        ],
    )
    .unwrap();
    let id = getrich_core::db::ensure_watermark_listing(&conn).unwrap();
    getrich_core::db::mark_success(&conn, id, "quotes", "tushare", Some("2026-10-23")).unwrap();
    drop(conn);
    let status = app.tushare_status();
    assert_eq!(status["configured"], true);
    assert_eq!(status["realtime_status"]["state"], "permission_denied");
    assert!(status["realtime_status"]["last_success_at"].is_null());
    assert!(!status["last_success_at"].is_null());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
