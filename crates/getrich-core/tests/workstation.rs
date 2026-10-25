use chrono::{TimeZone, Utc};
use getrich_core::{
    config::AppConfig,
    db,
    events::EventHub,
    import::{self, ImportKind},
    models::Timeframe,
    pulse,
    service::App,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

#[test]
fn partial_quote_does_not_inherit_impossible_bounds_and_is_idempotent() {
    let w = Workspace::new();
    w.apply(json!({"quotes":[{"symbol":"600001","market":"CN.SH","ts":"2024-01-02T02:00:00Z","last":10.0,"open":10.0,"high":11.0,"low":9.0,"prev_close":10.0}]}));
    let next = json!({"quotes":[{"symbol":"600001","market":"CN.SH","ts":"2024-01-02T02:01:00Z","last":12.0}]});
    let applied = w.apply(next.clone());
    assert_eq!(applied.quotes_upserted, 1);
    let page = w
        .app
        .quote_board(Some("CN"), None, None, 200, 0, None)
        .unwrap();
    assert_eq!(page.rows[0].high, None);
    assert_eq!(page.rows[0].last, Some(12.0));
    assert_eq!(page.rows[0].change_pct, Some(20.0));
    assert_eq!(w.apply(next).counts.unchanged, 1);
}

struct Workspace {
    root: PathBuf,
    app: Arc<App>,
}
impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "getrich-workstation-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut cfg = AppConfig::default();
        cfg.universe.watchlist.clear();
        let app = Arc::new(App::at_path(cfg, root.join("test.db")));
        app.migrate().unwrap();
        Self { root, app }
    }
    fn apply(&self, value: Value) -> getrich_core::models::IngestReport {
        self.app
            .import_bundle(
                &import::parse_json(&value.to_string(), ImportKind::Bundle).unwrap(),
                "fixture",
            )
            .unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        self.app.ingest.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn quote(symbol: &str, ts: &str, last: f64) -> Value {
    json!({"symbol":symbol,"market":"CN.SH","ts":ts,"last":last,"prev_close":10.0})
}

#[test]
fn market_map_uses_one_date_and_never_substitutes_turnover_for_market_cap() {
    use getrich_core::market_map::MapQuery;
    let w = Workspace::new();
    w.apply(json!({"listings":[
        {"symbol":"600001","market":"CN.SH","industry":"银行","metadata":[{"key":"float_market_cap","value":"200000000","as_of":"2024-01-03","source":"fixture.cap"}]},
        {"symbol":"600002","market":"CN.SH","industry":"银行","metadata":[{"key":"float_market_cap","value":"900000000","as_of":"2024-01-02"}]},
        {"symbol":"600003","market":"CN.SH","industry":"半导体"},
        {"symbol":"600004","market":"CN.SH"},
        {"symbol":"000001","market":"CN.SH","instrument_type":"index"},
        {"symbol":"AAPL","market":"US"}
    ],"quotes":[
        {"symbol":"600001","market":"CN.SH","ts":"2024-01-03","last":11.0,"prev_close":10.0,"turnover":1000000.0},
        {"symbol":"600002","market":"CN.SH","ts":"2024-01-03","last":9.0,"prev_close":10.0,"turnover":3000000.0},
        {"symbol":"600003","market":"CN.SH","ts":"2024-01-02","last":10.0},
        {"symbol":"000001","market":"CN.SH","ts":"2024-01-03","last":3000.0},
        {"symbol":"AAPL","market":"US","ts":"2024-01-03","last":200.0}
    ]}));
    let map = w.app.market_map(MapQuery::default()).unwrap();
    assert_eq!(map.trade_date.as_deref(), Some("2024-01-03"));
    assert_eq!(map.metric, "turnover");
    assert_eq!(map.coverage.universe, 4);
    assert_eq!(map.coverage.other_date, 1);
    assert_eq!(map.coverage.missing_quote, 1);
    assert_eq!(map.coverage.displayed, 2);
    assert!(map.historical);
    assert_eq!(map.indices.len(), 5);
    assert_eq!(map.indices[0].quote.as_ref().unwrap().last, Some(3000.0));
    assert!(map.indices[1].quote.is_none());
    let cap = w
        .app
        .market_map(MapQuery {
            metric: "float_market_cap".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cap.coverage.missing_weight, 1);
    assert_eq!(cap.groups[0].tiles[0].weight, 200000000.0);
    assert_eq!(
        cap.groups[0].tiles[0].weight_source.as_deref(),
        Some("fixture.cap")
    );
    assert_eq!(cap.coverage.displayed, 1);
    let equal = w
        .app
        .market_map(MapQuery {
            metric: "equal".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(equal.coverage.displayed, 2);
    assert_eq!(equal.coverage.up, 1);
    assert_eq!(equal.coverage.down, 1);
}

#[test]
fn market_map_keeps_unknown_change_and_market_watch_membership_explicit() {
    use getrich_core::market_map::MapQuery;
    let w = Workspace::new();
    w.apply(json!({"quotes":[{"symbol":"600001","market":"CN.SH","ts":"2024-01-03","last":11.0},{"symbol":"600002","market":"CN.SH","ts":"2024-01-03","last":10.0,"prev_close":10.0},{"symbol":"AAPL","market":"US","ts":"2024-01-03","last":200.0}]}));
    w.app
        .set_watch("600001", Some("CN.SH"), None, true)
        .unwrap();
    w.app.set_watch("AAPL", Some("US"), None, true).unwrap();
    let map = w
        .app
        .market_map(MapQuery {
            watch_only: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(map.coverage.displayed, 1);
    assert_eq!(map.coverage.unknown_change, 1);
    assert_eq!(map.coverage.flat, 0);
    assert_eq!(map.currency, "CNY");
    assert!(map.groups[0].tiles[0].watched);
    let us = w
        .app
        .market_map(MapQuery {
            board: "US".into(),
            watch_only: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(us.coverage.displayed, 1);
    assert_eq!(us.currency, "USD");
    assert!(w
        .app
        .market_map(MapQuery {
            board: "ALL".into(),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn dated_weights_cannot_be_replaced_by_older_data_or_inherit_a_missing_date() {
    use getrich_core::market_map::MapQuery;
    let w = Workspace::new();
    let listing = |value: &str, date: Option<&str>| json!({"symbol":"600001","market":"CN.SH","metadata":[{"key":"float_market_cap","value":value,"as_of":date,"source":"fixture.cap"}]});
    w.apply(json!({"listings":[listing("200000000",Some("2024-01-03"))],"quotes":[{"symbol":"600001","market":"CN.SH","ts":"2024-01-03","last":10.0}]}));
    w.apply(json!({"listings":[listing("100000000",Some("2024-01-02"))]}));
    let map = w
        .app
        .market_map(MapQuery {
            metric: "float_market_cap".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(map.groups[0].tiles[0].weight, 200000000.0);
    w.apply(json!({"listings":[listing("300000000",None)]}));
    let undated = w
        .app
        .market_map(MapQuery {
            metric: "float_market_cap".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(undated.coverage.displayed, 0);
    assert_eq!(undated.coverage.missing_weight, 1);
}
fn bar(day: &str, close: f64) -> Value {
    json!({"symbol":"600001","market":"CN.SH","ts":day,"open":10.0,"high":close.max(12.0),"low":9.0,"close":close,"volume":10.0})
}

#[test]
fn invalid_rows_are_quarantined_with_original_positions() {
    let w = Workspace::new();
    let report=w.apply(json!({"listings":[{"symbol":"600001","market":"CN.SH","listing_date":"中文ab"}],"quotes":[quote("600002","2024-01-02",11.0),quote("600003","2024-01-02",-10.0)],"bars":[{"symbol":"600004","market":"CN.SH","ts":"2024-01-02","open":10,"high":9,"low":11,"close":10}]}));
    assert_eq!(report.status, "partial");
    assert_eq!(report.counts.requested, 4);
    assert_eq!(report.counts.invalid, 3);
    assert_eq!(report.quotes_upserted, 1);
    let conn = w.app.db.connect().unwrap();
    let rejected: (String, i64) = conn
        .query_row(
            "SELECT dataset,row_number FROM ingest_rejections WHERE symbol='600003'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(rejected, ("quotes".into(), 2));
    assert_eq!(w.app.stats().unwrap().quotes, 1);
}
#[test]
fn preview_rolls_back_and_matches_applied_counts() {
    let w = Workspace::new();
    let bundle = import::parse_json(
        &json!({"quotes":[quote("600001","2024-01-02",11.0)],"bars":[bar("2024-01-02",11.0)]})
            .to_string(),
        ImportKind::Bundle,
    )
    .unwrap();
    let before = import::preview(&w.app.db, &bundle, "fixture").unwrap();
    assert!(w.app.stats().unwrap().empty);
    let actual = w.app.import_bundle(&bundle, "fixture").unwrap();
    assert_eq!(
        before["counts"],
        serde_json::to_value(actual.counts).unwrap()
    );
    let repeat = w.app.import_bundle(&bundle, "fixture").unwrap();
    assert_eq!(repeat.counts.unchanged, 2);
    assert_eq!(repeat.quotes_upserted, 0);
    assert_eq!(repeat.bars_upserted, 0);
    let stale = w.apply(json!({"quotes":[quote("600001","2024-01-01",12.0)]}));
    assert_eq!(stale.counts.stale, 1);
    assert_eq!(stale.quotes_upserted, 0);
}
#[test]
fn quote_projection_matches_list_detail_sort_and_pulse() {
    let w = Workspace::new();
    w.apply(json!({"quotes":[quote("600001","2024-01-02",10.0)],"bars":[bar("2024-01-03",12.0)]}));
    let page = w
        .app
        .quote_board(Some("CN"), None, Some("last"), 200, 0, None)
        .unwrap();
    let key = w.app.parse_key("600001", Some("CN.SH")).unwrap();
    let detail = w.app.show_stock(&key).unwrap();
    assert_eq!(page.rows[0].last, Some(12.0));
    assert_eq!(page.rows[0].last, detail.quote.last);
    let rows = db::pulse_tape_scan(&w.app.db.connect().unwrap()).unwrap();
    assert_eq!(rows[0].last, 12.0);
}
#[test]
fn daily_revision_invalidates_saved_monthly_and_weekly() {
    let w = Workspace::new();
    let mut old = bar("2024-01-01", 10.0);
    old["timeframe"] = json!("1mo");
    w.apply(json!({"bars":[old,bar("2024-01-02",11.0)]}));
    let key = w.app.parse_key("600001", Some("CN.SH")).unwrap();
    let first = w
        .app
        .list_bars(&key, Timeframe::Mo1, None, None, 800)
        .unwrap();
    assert_eq!(first.rows[0].close, 11.0);
    w.apply(json!({"bars":[bar("2024-01-03",12.0),bar("2024-01-02",10.5)]}));
    for tf in [Timeframe::Mo1, Timeframe::W1] {
        let page = w.app.list_bars(&key, tf, None, None, 800).unwrap();
        assert_eq!(page.rows.last().unwrap().close, 12.0);
        assert_eq!(page.rows.last().unwrap().volume, Some(20.0));
    }
}
#[test]
fn one_current_quote_does_not_hide_103_historical_quotes() {
    let w = Workspace::new();
    let mut quotes = vec![quote("600000", "2026-10-23T02:00:00Z", 11.0)];
    for i in 1..104 {
        quotes.push(quote(&format!("{:06}", 600000 + i), "2024-01-02", 11.0));
    }
    w.apply(json!({"quotes":quotes}));
    let conn = w.app.db.connect().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 10, 23, 2, 0, 1).unwrap();
    conn.execute("UPDATE quotes SET ingested_at='2026-10-23T02:00:00Z'", [])
        .unwrap();
    let p = pulse::compute(&conn, &w.app.cfg.data.pulse, 5000, &[], now).unwrap();
    assert_eq!(p.data_quality.observed, 104);
    assert_eq!(p.data_quality.valid, 1);
    assert_eq!(p.data_quality.stale, 103);
    assert_eq!(p.tape.n_quoted, 1);
    assert_eq!(p.data_quality.universe_scope, "imported_sample");
}
#[test]
fn invalid_unicode_dates_cannot_panic_any_date_parser() {
    for text in ["中文ab", "中文中文abcd", "🦀🦀🦀", "20261399", "x", ""] {
        assert!(std::panic::catch_unwind(|| {
            getrich_core::session::calendar_date_of(text);
            let _ = getrich_core::validation::date(text);
            getrich_core::tushare::yyyymmdd_to_iso(text);
        })
        .is_ok());
    }
}
#[test]
fn stale_calendar_is_not_authoritative() {
    let w = Workspace::new();
    let conn = w.app.db.connect().unwrap();
    db::upsert_vendor(
        &conn,
        "trade_cal",
        "2026-10-22",
        Some("t0"),
        "ok",
        "20261022",
    )
    .unwrap();
    let p = pulse::compute(
        &conn,
        &w.app.cfg.data.pulse,
        5000,
        &[],
        Utc.with_ymd_and_hms(2026, 10, 23, 2, 0, 0).unwrap(),
    )
    .unwrap();
    assert_eq!(p.session.calendar_source, "fallback");
    assert_eq!(p.session.trade_date, "2026-10-23");
}
#[test]
fn watch_sort_filter_and_pagination_preserve_membership() {
    let w = Workspace::new();
    w.apply(json!({"listings":[{"symbol":"600001","market":"CN.SH","industry":"A"},{"symbol":"600002","market":"CN.SH","industry":"B"}],"quotes":[quote("600001","2024-01-02",11.0),quote("600002","2024-01-02",12.0)]}));
    for s in ["600001", "600002", "600003"] {
        w.app.set_watch(s, Some("CN.SH"), None, true).unwrap();
    }
    let sorted = w
        .app
        .quote_board(Some("WATCH"), None, Some("change_pct"), 1, 0, None)
        .unwrap();
    assert_eq!(sorted.rows[0].symbol, "600002");
    assert_eq!(sorted.matched, 3);
    let second = w
        .app
        .quote_board(Some("WATCH"), None, Some("change_pct"), 1, 1, None)
        .unwrap();
    assert_eq!(second.rows[0].symbol, "600001");
    let filtered = w
        .app
        .quote_board(Some("WATCH"), None, Some("change_pct"), 200, 0, Some("A"))
        .unwrap();
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0].symbol, "600001");
    assert!(filtered.rows[0].has_quote);
}
#[test]
fn independent_instances_merge_concurrent_settings_updates() {
    let w = Workspace::new();
    let other = Arc::new(App::at_path(w.app.cfg.clone(), w.root.join("test.db")));
    let first = Arc::clone(&w.app);
    let thread = std::thread::spawn(move || {
        first
            .set_watch("600001", Some("CN.SH"), None, true)
            .unwrap()
    });
    other
        .set_watch("600002", Some("CN.SH"), None, true)
        .unwrap();
    thread.join().unwrap();
    assert_eq!(w.app.watchlist().len(), 2);
    assert_eq!(other.watchlist().len(), 2);
    other
        .save_settings(&json!({"default_board":"WATCH","preferences":{"theme":"dark"}}))
        .unwrap();
    assert_eq!(w.app.default_board(), "WATCH");
    assert_eq!(
        w.app.settings_view().unwrap()["preferences"]["theme"],
        "dark"
    );
    assert!(other
        .save_settings(&json!({"default_board":"INVALID"}))
        .is_err());
    assert_eq!(w.app.watchlist().len(), 2);
}
#[test]
fn relative_import_uses_data_root_in_all_entry_points() {
    let w = Workspace::new();
    std::fs::create_dir(w.root.join("examples")).unwrap();
    std::fs::write(
        w.root.join("examples/demo.json"),
        json!({"quotes":[quote("600001","2024-01-02",11.0)]}).to_string(),
    )
    .unwrap();
    let params = json!({"path":"examples/demo.json"});
    let preview = getrich_core::invoke::invoke(&w.app, "import.preview", params).unwrap();
    assert_eq!(preview["counts"]["inserted"], 1);
    w.app
        .import_file(
            std::path::Path::new("examples/demo.json"),
            ImportKind::Bundle,
        )
        .unwrap();
    assert_eq!(w.app.stats().unwrap().quotes, 1);
}
#[test]
fn durable_jobs_deduplicate_cancel_and_retry() {
    let w = Workspace::new();
    let params = json!({"data":{"quotes":[quote("600001","2024-01-02",11.0)]}});
    let one = w.app.submit_job("import.json", params.clone()).unwrap();
    let duplicate = w.app.submit_job("import.json", params).unwrap();
    assert_eq!(one["id"], duplicate["id"]);
    let id = one["id"].as_i64().unwrap();
    assert_eq!(w.app.cancel_job(id).unwrap()["status"], "cancelled");
    assert_eq!(w.app.stats().unwrap().quotes, 0);
    let retry = w.app.retry_job(id).unwrap();
    getrich_core::jobs::start(&w.app);
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let job = w.app.job(retry["id"].as_i64().unwrap()).unwrap();
        if job["status"] == "succeeded" {
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(w.app.stats().unwrap().quotes, 1);
    w.app.ingest.stop.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(300));
}
#[test]
fn events_have_monotonic_replay_and_gap_detection() {
    let hub = Arc::new(EventHub::new());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let hub = Arc::clone(&hub);
            std::thread::spawn(move || {
                for _ in 0..100 {
                    hub.emit("changed", json!({}));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let replay = hub.subscribe_since(Some(0));
    assert!(replay.resync_required);
    assert_eq!(replay.cursor, 400);
    assert!(replay
        .replay
        .windows(2)
        .all(|p| p[0]["seq"].as_u64().unwrap() + 1 == p[1]["seq"].as_u64().unwrap()));
    assert_ne!(hub.epoch(), EventHub::new().epoch());
}
