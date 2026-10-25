//! Headless egui layout checks; no desktop input or screenshots are used.
use super::*;
use getrich_core::import::{parse_json, ImportKind};

#[test]
fn all_workspaces_layout_at_minimum_and_large_window_sizes() {
    let root = std::env::temp_dir().join(format!(
        "getrich-ui-layout-{}-{}",
        std::process::id(),
        getrich_core::session::shanghai_now()
            .timestamp_nanos_opt()
            .unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let core = Arc::new(App::at_path(Default::default(), root.join("test.db")));
    core.migrate().unwrap();
    core.import_bundle(
        &parse_json(
            include_str!("../../../../docs/examples/demo.board.json"),
            ImportKind::Bundle,
        )
        .unwrap(),
        "layout.fixture",
    )
    .unwrap();
    let mut app = GetRichApp::new(Arc::clone(&core), Default::default());
    app.settings_cache = core.settings_view().unwrap();
    app.stats = Some(core.stats().unwrap());
    app.market.data = Some(Arc::new(core.market_map(Default::default()).unwrap()));
    app.market.sync_job = Some(json!({"id":1,"status":"succeeded"}));
    let page = core
        .quote_board(Some("CN"), None, None, 200, 0, None)
        .unwrap();
    if let Some(row) = page.rows.first() {
        let key = core.parse_key(&row.symbol, Some(&row.market)).unwrap();
        app.detail = Some(core.show_stock(&key).unwrap());
        app.bars = Some(
            core.list_bars(&key, Timeframe::D1, None, None, 500)
                .unwrap(),
        );
    }
    app.page = Some(page);
    app.stock.data = json!({"datasets":{"valuation":{"rows":[{"total_mv":1668859000000.0,"circ_mv":1668859000000.0,"pe_ttm":20.49,"pb":6.64}]},"holders":{"rows":[{"holder_name":"中国贵州茅台酒厂（集团）有限责任公司","hold_amount":681000000.0,"hold_ratio":54.5},{"holder_name":"贵州省国有资本运营有限责任公司","hold_amount":56996800.0,"hold_ratio":4.56}]}}});
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    GetRichApp::install_theme(&ctx);
    for theme in ["dark", "light"] {
        app.preferences.theme = theme.into();
        for size in [
            Vec2::new(760.0, 600.0),
            Vec2::new(1024.0, 768.0),
            Vec2::new(1440.0, 900.0),
            Vec2::new(1920.0, 1080.0),
        ] {
            for mode in 0..14 {
                app.stock.tab = match mode {
                    9 | 13 => 1,
                    10 => 2,
                    11 => 3,
                    _ => 0,
                };
                app.stock.intraday = mode == 12;
                app.stats.as_mut().unwrap().empty = mode == 8;
                app.settings_tab = match mode {
                    2..=5 => mode - 2,
                    _ => 0,
                };
                app.screen = match mode {
                    0 => Screen::Pulse,
                    1 | 7 => Screen::Data,
                    2..=5 => Screen::Settings,
                    8 => Screen::Quotes,
                    _ => Screen::Detail,
                };
                app.preview = if mode == 7 {
                    Some(
                        json!({"counts":{"requested":300,"inserted":300,"updated":0,"unchanged":0,"stale":0,"invalid":0},"can_apply":true}),
                    )
                } else {
                    None
                };
                for frame in 0..(if mode == 13 { 4 } else { 2 }) {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ctx| {
                            app.apply_preferences(ctx);
                            assert_eq!(
                                ctx.style().visuals.strong_text_color(),
                                text_color(),
                                "headings must remain readable in {theme}"
                            );
                            app.ui_navigation(ctx);
                            match app.screen {
                                Screen::Pulse => app.ui_pulse(ctx),
                                Screen::Data => app.ui_data(ctx),
                                Screen::Settings => app.ui_settings(ctx),
                                _ if mode == 13 => {
                                    egui::CentralPanel::default().show(ctx, |ui| {
                                        let area =
                                            egui::ScrollArea::vertical().id_salt("holders-layout");
                                        let area = if frame == 0 {
                                            area.vertical_scroll_offset(100000.0)
                                        } else {
                                            area
                                        };
                                        area.show(ui, |ui| app.draw_detail(ui));
                                    });
                                }
                                _ => app.ui_quotes(ctx),
                            }
                        },
                    );
                    assert!(!output.shapes.is_empty());
                    if mode == 13 && size.x >= 1000.0 && frame == 3 {
                        let holder = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(t)
                                    if t.galley.job.text
                                        == "中国贵州茅台酒厂（集团）有限责任公司" =>
                                {
                                    Some(t.galley.rect)
                                }
                                _ => None,
                            })
                            .unwrap_or_else(|| {
                                panic!(
                                    "long holder missing at {theme} {size:?}: {:?}",
                                    output
                                        .shapes
                                        .iter()
                                        .filter_map(|s| match &s.shape {
                                            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                                            _ => None,
                                        })
                                        .collect::<Vec<_>>()
                                )
                            });
                        assert!(
                            holder.width() > 200.0 && holder.height() < 60.0,
                            "shareholder names must use the available column width: {holder:?}"
                        );
                    }
                    if mode == 0 {
                        let status = app.market.status_rect.expect("market status");
                        let footer = app.market.footer_rect.expect("market footer");
                        assert!(
                            status.left() >= size.x * 0.49
                                && status.top() >= size.y - 65.0
                                && status.bottom() <= size.y + 1.0,
                            "market messages must stay at bottom right: {status:?} at {size:?}"
                        );
                        let canvas = app.market.canvas_rect.expect("market canvas");
                        assert!(
                            canvas.bottom() <= footer.top() + 1.0,
                            "canvas and footer must not overlap"
                        );
                        assert!(
                            canvas.width() >= size.x * 0.95,
                            "canvas must occupy page width: {canvas:?} at {size:?}"
                        );
                        let share = canvas.area() / (size.x * (size.y - 60.0));
                        assert!(
                            share >= if size.x >= 1000.0 { 0.75 } else { 0.60 },
                            "canvas too small ({share:.2}) at {size:?}: {canvas:?}"
                        );
                        assert!(
                            canvas.bottom() <= size.y - 20.0,
                            "canvas footer must remain visible: {canvas:?}"
                        );
                    }
                    if [1, 7].contains(&mode) && size.x >= 1440.0 {
                        let cards: Vec<_> = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Rect(r)
                                    if r.fill == surface()
                                        && r.rect.width() > 500.0
                                        && r.rect.width() < 800.0
                                        && r.rect.top() > 60.0 =>
                                {
                                    Some(r.rect)
                                }
                                _ => None,
                            })
                            .collect();
                        assert_eq!(cards.len(), 2, "paired cards must both be painted");
                        assert!((cards[0].top() - cards[1].top()).abs() < 0.5);
                        assert!(
                            (cards[0].bottom() - cards[1].bottom()).abs() < 0.5,
                            "paired cards differ: {cards:?}"
                        );
                        assert!(
                            cards[0].height() < 500.0,
                            "content must not double its own height: {cards:?}"
                        );
                    }
                    if mode == 6 && size.x == 1440.0 {
                        let name = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(t)
                                    if t.galley.job.text == "贵州茅台" && t.pos.x < 420.0 =>
                                {
                                    Some(t.pos.x)
                                }
                                _ => None,
                            })
                            .expect("quote name cell");
                        assert!(
                            (100.0..230.0).contains(&name),
                            "quote cells collapsed: name starts at {name}"
                        );
                    }
                    for clipped in output.shapes {
                        if let egui::Shape::Text(text) = clipped.shape {
                            let label = text.galley.job.text.as_str();
                            if [
                                "刷新截面",
                                "保存数据源",
                                "保存外观",
                                "测试连接",
                                "同步行情",
                                "同步市值",
                                "同步指数",
                                "添加自选",
                                "校验预览",
                                "应用有效记录",
                                "选择文件",
                                "配置数据源",
                                "数据源连接",
                                "行情与外观",
                                "自选管理",
                                "存储与诊断",
                                "连接数据源",
                                "导入行情文件",
                                "市场行情",
                                "后台任务",
                                "全部行业",
                                "退出专注",
                                "专注",
                                "大盘星图",
                            ]
                            .contains(&label)
                            {
                                let bounds = text.galley.rect.translate(text.pos.to_vec2());
                                if clipped.clip_rect.intersects(bounds) {
                                    assert!(bounds.left()>=-1.0 && bounds.right()<=size.x+1.0,
                                        "{theme} mode {mode} {label} overflows at {size:?}: {bounds:?}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    drop(app);
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}
