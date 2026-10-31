use super::*;

#[test]
fn pinned_stock_outside_the_board_refreshes_quotes_and_tail_without_resetting_the_workspace() {
    let (mut app, root) = workspace();
    let initial_stats = app.core.stats().unwrap();
    for _ in 0..100 {
        app.apply_io();
        if !app.loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    app.board = "US".into();
    app.navigate_stock("600519", "CN.SH");
    for _ in 0..200 {
        app.apply_io();
        if app
            .detail
            .as_ref()
            .is_some_and(|d| d.quote.symbol == "600519")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let old_bars = app.bars.clone().unwrap();
    app.stock.tab = 2;
    app.kline_span = 10;
    let bundle=getrich_core::import::parse_json(&json!({"quotes":[{"symbol":"600519","market":"CN.SH","ts":"2026-10-28T02:00:00Z","last":61.0,"prev_close":60.0}],"bars":[{"symbol":"600519","market":"CN.SH","timeframe":"1d","ts":"2026-10-28","open":60.0,"high":62.0,"low":59.0,"close":61.0,"volume":1000.0}]}).to_string(),getrich_core::import::ImportKind::Bundle).unwrap();
    let report = getrich_core::import::apply_quotes_hot(
        &app.core.db,
        &bundle.quotes,
        &bundle.bars,
        "fixture.rt_k",
        1000,
    )
    .unwrap();
    getrich_core::ingest::emit_quote_updated(&app.core.events, &report);
    app.observe_data_changes();
    app.tick_live_quote();
    for _ in 0..400 {
        app.apply_io();
        if app
            .detail
            .as_ref()
            .is_some_and(|d| d.quote.last == Some(61.0))
            && app
                .bars
                .as_ref()
                .is_some_and(|b| b.rows.last().is_some_and(|r| r.ts == "2026-10-28"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.detail.as_ref().unwrap().quote.last, Some(61.0));
    let mut stale = app.detail.clone().unwrap();
    stale.quote.last = Some(20.0);
    stale.quote.ts = Some("2024-03-07T07:00:00Z".into());
    app.controller
        .submit("read:stock", move |_| {
            Ok(crate::controller::Payload::Stock(Box::new(stale), old_bars))
        })
        .unwrap();
    for _ in 0..200 {
        app.apply_io();
        if !app.controller.is_pending("read:stock") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        app.detail.as_ref().unwrap().quote.last,
        Some(61.0),
        "late history must not roll back a newer quote"
    );
    assert_eq!(app.bars.as_ref().unwrap().rows.last().unwrap().close, 61.0);
    assert!(app.detail_pinned);
    assert_eq!(app.stock.tab, 2);
    assert_eq!(app.kline_span, 10);
    assert_eq!(app.pending_key.as_ref().unwrap().symbol, "600519");
    app.stats = Some(initial_stats);
    app.screen = Screen::Pulse;
    app.tick_map_reads();
    for _ in 0..400 {
        app.apply_io();
        if !app.market.loading && !app.controller.is_pending("read:stats") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        app.stats.as_ref().unwrap().last_quote_ts,
        app.core.health_stats().unwrap().last_quote_ts,
        "map-only refresh must advance the global quote timestamp too"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

pub(super) fn workspace() -> (GetRichApp, std::path::PathBuf) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "getrich-interactions-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        session::shanghai_now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let core = Arc::new(App::at_path(Default::default(), root.join("test.db")));
    core.migrate().unwrap();
    let bundle = getrich_core::import::parse_json(
        include_str!("../../../../docs/examples/demo.board.json"),
        getrich_core::import::ImportKind::Bundle,
    )
    .unwrap();
    core.import_bundle(&bundle, "fixture").unwrap();
    (GetRichApp::new(core, Default::default()), root)
}

#[test]
fn late_map_results_queue_latest_filters_and_preserve_the_last_canvas() {
    let (mut app, root) = workspace();
    let original = app.core.market_map(app.market.query.clone()).unwrap();
    app.accept_map(original.clone());
    let previous = app.market.data.clone().unwrap();
    app.market.loading = true;
    app.market.request_query = Some(app.market.query.clone());
    app.market.query.industry = "银行".into();
    app.market.query.weight_policy = "same_day".into();
    app.market.query.aspect = 2.7;
    app.reload_map();
    assert!(Arc::ptr_eq(app.market.data.as_ref().unwrap(), &previous));
    app.accept_map(original);
    assert!(
        app.market.loading,
        "latest filter must be submitted immediately"
    );
    assert_eq!(app.market.request_query.as_ref(), Some(&app.market.query));
    assert!(Arc::ptr_eq(app.market.data.as_ref().unwrap(), &previous));
    for _ in 0..200 {
        app.apply_io();
        if !app.market.loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.market.loading);
    assert_eq!(app.market.data.as_ref().unwrap().query, app.market.query);
    app.fail_map("temporary read failure".into());
    assert!(
        app.market.data.is_some(),
        "read errors must retain the valid canvas"
    );
    assert_eq!(app.market.error.as_deref(), Some("temporary read failure"));
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn map_navigation_keeps_the_instrument_outside_watch_and_empty_filtered_pages() {
    let (mut app, root) = workspace();
    app.board = "US".into();
    let page = app
        .core
        .quote_board(Some("US"), None, None, 20, 0, None)
        .unwrap();
    app.market.inspector_open = true;
    app.navigate_stock("600519", "CN.SH");
    app.apply_board(page, true);
    assert_eq!(app.pending_key.as_ref().unwrap().symbol, "600519");
    assert!(app.selected.is_none());
    assert!(!app.market.inspector_open);
    assert!(app.screen == Screen::Detail);
    let empty = app
        .core
        .quote_board(Some("US"), Some("no-such-symbol"), None, 20, 0, None)
        .unwrap();
    app.apply_board(empty, false);
    assert_eq!(app.pending_key.as_ref().unwrap().market.as_str(), "CN.SH");
    for _ in 0..100 {
        app.apply_io();
        if app.detail.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.detail.as_ref().unwrap().quote.symbol, "600519");
    let row = app
        .core
        .quote_board(Some("US"), Some("AAPL"), None, 20, 0, None)
        .unwrap()
        .rows
        .remove(0);
    app.open_row(&row);
    assert!(!app.detail_pinned);
    assert_eq!(app.pending_key.as_ref().unwrap().symbol, "AAPL");
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn adding_watch_search_finds_names_and_codes_then_fills_exact_market() {
    let (mut app, root) = workspace();
    app.screen = Screen::Settings;
    app.settings_tab = 2;
    // Complete the constructor's initial read before searching.
    for _ in 0..20 {
        app.apply_io();
        std::thread::sleep(Duration::from_millis(5));
    }
    for (query, market, symbol, expected_market) in [
        ("茅台", "ALL", "600519", "CN.SH"),
        ("AAPL", "US", "AAPL", "US"),
        ("000001", "CN.SZ", "000001", "CN.SZ"),
    ] {
        app.watch_search.query = query.into();
        app.watch_search.market = market.into();
        app.watch_search.rows.clear();
        app.watch_search.dirty = Some(Instant::now() - Duration::from_secs(1));
        app.tick_watch_search();
        for _ in 0..100 {
            app.apply_io();
            if !app.watch_search.loading {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let index = app
            .watch_search
            .rows
            .iter()
            .position(|r| r.symbol == symbol && r.market == expected_market)
            .expect("matching listing");
        app.choose_watch_result(index);
        assert_eq!(app.watch_symbol, symbol);
        assert_eq!(app.watch_market, expected_market);
        assert!(!app.watch_name.is_empty());
    }
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
