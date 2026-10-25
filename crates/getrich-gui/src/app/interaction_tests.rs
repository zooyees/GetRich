use super::*;
fn workspace() -> (GetRichApp, std::path::PathBuf) {
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
