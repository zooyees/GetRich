use super::*;

#[test]
#[ignore = "6000-stock native canvas CPU audit"]
fn six_thousand_stock_pointer_frames_reuse_geometry() {
    let (mut app, root) = super::super::interaction_tests::workspace();
    let listings:Vec<_>=(0..6000).map(|i|json!({"symbol":format!("{:06}",600000+i),"market":"CN.SH","name":format!("测试股票{i}"),"industry":format!("行业{}",i%60),"currency":"CNY","metadata":[{"key":"float_market_cap","value":(1000000.0/(1.0+i as f64)).to_string(),"as_of":"2026-10-23"}]})).collect();
    let quotes:Vec<_>=(0..6000).map(|i|json!({"symbol":format!("{:06}",600000+i),"market":"CN.SH","ts":"2026-10-23T02:00:00Z","last":10.0,"prev_close":9.8,"currency":"CNY"})).collect();
    app.core
        .import_bundle(
            &getrich_core::import::parse_json(
                &json!({"listings":listings,"quotes":quotes}).to_string(),
                getrich_core::import::ImportKind::Bundle,
            )
            .unwrap(),
            "synthetic.audit",
        )
        .unwrap();
    app.market.query.metric = "equal".into();
    app.market.data = Some(Arc::new(
        app.core.market_map(app.market.query.clone()).unwrap(),
    ));
    assert_eq!(app.market.data.as_ref().unwrap().coverage.displayed, 6000);
    app.screen = Screen::Pulse;
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    GetRichApp::install_theme(&ctx);
    let mut run = |rebuild: bool| {
        if rebuild {
            app.market.canvas_cache = None;
        }
        let start = Instant::now();
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0))),
                events: vec![egui::Event::PointerMoved(Pos2::new(1000.0, 500.0))],
                ..Default::default()
            },
            |ctx| app.ui_pulse(ctx),
        );
        let _ = ctx.tessellate(out.shapes, out.pixels_per_point);
        app.market.resize_at = None;
        if let Some(rect) = app.market.canvas_rect {
            app.market.query.aspect = f64::from(rect.width() / rect.height());
        }
        start.elapsed().as_micros() as u64
    };
    for _ in 0..4 {
        run(false);
    }
    let mut rebuilt: Vec<_> = (0..30).map(|_| run(true)).collect();
    let mut cached: Vec<_> = (0..60).map(|_| run(false)).collect();
    rebuilt.sort_unstable();
    cached.sort_unstable();
    let result = json!({"stocks":6000,"rebuild_median_us":rebuilt[15],"cached_median_us":cached[30],"cached_p95_us":cached[57],"scope":"headless egui layout + tessellation CPU; excludes desktop GPU/presentation"});
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/refresh-audit-20261028");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join("native-canvas.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("{result}");
    assert_eq!(app.market.canvas_cache.as_ref().unwrap().tiles.len(), 6000);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
