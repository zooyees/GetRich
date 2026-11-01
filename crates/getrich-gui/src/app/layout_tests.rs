//! Headless egui layout checks; no desktop input or screenshots are used.
use super::*;
use getrich_core::import::{parse_json, ImportKind};

#[test]
fn professional_forms_layout_and_drafts_survive_all_groups_and_window_sizes() {
    let (mut app, root) = super::interaction_tests::workspace();
    app.workbench.catalog =
        getrich_core::professional::execute(&app.core, "catalog", &json!({})).unwrap();
    let tools = app.workbench.catalog["tools"].as_array().unwrap().clone();
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    GetRichApp::install_theme(&ctx);
    app.screen = Screen::Workbench;
    for size in [
        Vec2::new(760.0, 600.0),
        Vec2::new(1024.0, 768.0),
        Vec2::new(1920.0, 1080.0),
    ] {
        for tool in &tools {
            app.workbench.tool = tool["id"].as_str().unwrap().into();
            app.workbench.group = tool["group"].as_str().unwrap().into();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    app.ui_navigation(ctx);
                    app.ui_workbench(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
        }
    }
    app.workbench.forms.insert(
        "pool.save".into(),
        json!({"name":"待保存研究池","members":"CN.SH:600519","revision":0}),
    );
    let draft = app.workbench.forms["pool.save"].clone();
    app.run_workbench("pool.save", draft);
    for _ in 0..300 {
        app.apply_io();
        if app.workbench.results.contains_key("pool.save") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.workbench.forms["pool.save"]["revision"], 1);
    assert_eq!(app.workbench.forms["pool.save"]["name"], "待保存研究池");
    let result = app.workbench.results["pool.save"].clone();
    app.workbench.results.insert("overview".into(), result);
    app.workbench.tool = "overview".into();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui_workbench(ctx));
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn full_market_pagination_stays_visible_in_the_one_fifth_sidebar() {
    let (mut app, root) = super::interaction_tests::workspace();
    let mut page = app
        .core
        .quote_board(Some("CN"), None, None, 200, 0, None)
        .unwrap();
    let row = page.rows[0].clone();
    page.rows = (0..200)
        .map(|i| {
            let mut row = row.clone();
            row.symbol = format!("{i:06}");
            row
        })
        .collect();
    page.count = 200;
    page.matched = 6000;
    app.stats = Some(app.core.stats().unwrap());
    app.page = Some(page);
    app.loading = false;
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    GetRichApp::install_theme(&ctx);
    for size in [
        Vec2::new(1024.0, 768.0),
        Vec2::new(1440.0, 900.0),
        Vec2::new(1920.0, 1080.0),
    ] {
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    app.ui_navigation(ctx);
                    app.ui_quotes(ctx);
                },
            );
            for label in ["上一页", "下一页"] {
                let (bounds, clip) = output
                    .shapes
                    .iter()
                    .find_map(|s| {
                        if let egui::Shape::Text(t) = &s.shape {
                            if t.galley.job.text == label {
                                return Some((
                                    t.galley.rect.translate(t.pos.to_vec2()),
                                    s.clip_rect,
                                ));
                            }
                        }
                        None
                    })
                    .expect("pagination must be painted");
                assert!(
                    bounds.bottom() <= size.y
                        && bounds.right() <= size.x * 0.2 + 1.0
                        && clip.contains_rect(bounds),
                    "pager {label} is clipped at {size:?}: {bounds:?} clip {clip:?}"
                );
            }
        }
    }
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

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
                            (45.0..180.0).contains(&name),
                            "quote cells collapsed: name starts at {name}"
                        );
                    }
                    if mode == 6 && size.x >= 1440.0 {
                        let label_rect = |label: &str| {
                            output
                                .shapes
                                .iter()
                                .find_map(|shape| {
                                    if let egui::Shape::Text(t) = &shape.shape {
                                        if t.galley.job.text == label
                                            && t.pos.x > size.x * 0.2 + 10.0
                                        {
                                            return Some(t.galley.rect.translate(t.pos.to_vec2()));
                                        }
                                    }
                                    None
                                })
                                .unwrap_or_else(|| panic!("detail label missing: {label}"))
                        };
                        let open = label_rect("今开");
                        assert!((open.left()-size.x*0.2-24.0).abs()<2.0,"metrics and stock heading must share the detail pane edge: metric left {} at {:?}",open.left(),size);
                        for label in ["最高", "最低", "昨收"] {
                            assert!(
                                (label_rect(label).top() - open.top()).abs() < 1.0,
                                "metric labels must align"
                            );
                        }
                        let high = label_rect("最高");
                        let low = label_rect("最低");
                        let close = label_rect("昨收");
                        let step = high.left() - open.left();
                        assert!(
                            step > 80.0
                                && (low.left() - high.left() - step).abs() < 1.0
                                && (close.left() - low.left() - step).abs() < 1.0,
                            "metric columns must have equal widths, including long values"
                        );
                        let volume = label_rect("成交量");
                        assert!(
                            volume.top() == open.top() || (volume.left() - open.left()).abs() < 1.0,
                            "wrapped metrics must align to the first column"
                        );
                        let period = label_rect("分时");
                        for label in [
                            "MA5",
                            "MA10",
                            "MA20",
                            "MA30",
                            "MA120 · 半年",
                            "MA250 · 年线",
                        ] {
                            let bounds = label_rect(label);
                            assert!(bounds.left()>=size.x*0.2 && bounds.right()<size.x && bounds.top()>period.top(),"MA control {label} must remain aligned inside the chart toolbar at {size:?}: {bounds:?}");
                        }
                        let heading = label_rect("贵州茅台");
                        let last_average = label_rect("MA250 · 年线");
                        let budget = if size.x >= 1920.0 { 205.0 } else { 245.0 };
                        assert!(last_average.bottom() - heading.top() <= budget, "stock header wastes chart space at {size:?}: heading {heading:?}, averages {last_average:?}");
                        let range = output
                            .shapes
                            .iter()
                            .find_map(|shape| {
                                if let egui::Shape::Text(t) = &shape.shape {
                                    if t.galley.job.text.contains("根 · 原始价格") {
                                        Some(t.pos.y)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .expect("history range in toolbar");
                        assert!(
                            (range - period.top()).abs() < 55.0,
                            "history range must sit inside the toolbar"
                        );
                        let refresh = label_rect(
                            if app.settings_cache["provider"]["configured"]
                                .as_bool()
                                .unwrap_or(false)
                            {
                                "更新完整历史"
                            } else {
                                "配置数据源"
                            },
                        );
                        assert!(
                            (period.center().y - refresh.center().y).abs() < 3.0,
                            "period and refresh controls must share a row"
                        );
                        assert!(
                            refresh.right() <= size.x && refresh.top() - open.top() < 220.0,
                            "chart controls must remain compact and inside the pane"
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
