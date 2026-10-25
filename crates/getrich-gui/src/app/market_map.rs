use super::*;
use getrich_core::market_map::{Bounds, MapQuery, MapTile, MarketMap};

pub(super) struct MapState {
    pub query: MapQuery,
    pub data: Option<Arc<MarketMap>>,
    pub loading: bool,
    pub error: Option<String>,
    search: String,
    highlight_watch: bool,
    selected: Option<String>,
    zoom: f32,
    pan: Vec2,
    resize_at: Option<Instant>,
    full: bool,
    pub(super) controls_open: bool,
    pub(super) inspector_open: bool,
    pub(super) sync_details_open: bool,
    pub(super) sync_job: Option<Value>,
    pub(super) sync_pending: bool,
    pub(super) auto_sync_attempted: bool,
    pub(super) canvas_rect: Option<Rect>,
    pub(super) footer_rect: Option<Rect>,
    pub(super) status_rect: Option<Rect>,
}
impl Default for MapState {
    fn default() -> Self {
        Self {
            query: MapQuery::default(),
            data: None,
            loading: false,
            error: None,
            search: String::new(),
            highlight_watch: false,
            selected: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            resize_at: None,
            full: false,
            controls_open: false,
            inspector_open: false,
            sync_details_open: false,
            sync_job: None,
            sync_pending: false,
            auto_sync_attempted: false,
            canvas_rect: None,
            footer_rect: None,
            status_rect: None,
        }
    }
}
impl GetRichApp {
    pub(super) fn reload_map(&mut self) {
        if self.market.loading {
            return;
        }
        self.market.loading = true;
        let query = self.market.query.clone();
        if let Err(error) = self.controller.submit("read:map", move |core| {
            core.market_map(query)
                .map(|v| crate::controller::Payload::Map(Box::new(v)))
                .map_err(|e| e.to_string())
        }) {
            self.market.loading = false;
            self.market.error = Some(error);
        }
    }
    pub(super) fn ui_pulse(&mut self, ctx: &egui::Context) {
        let mut refresh = false;
        self.map_footer(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(background()).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                refresh |= self.map_toolbar(ui);
                if !self.market.full {
                    self.map_indices_compact(ui);
                }
                let data = self.market.data.clone();
                let search = self.market.search.trim().to_lowercase();
                let canvas_height = ui.available_height().max(100.0);
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), canvas_height),
                    Sense::click_and_drag(),
                );
                self.market.canvas_rect = Some(rect);
                let aspect = f64::from(rect.width() / rect.height());
                if (aspect - self.market.query.aspect).abs() > 0.12 {
                    self.market.query.aspect = aspect;
                    self.market.resize_at = Some(Instant::now());
                }
                if self
                    .market
                    .resize_at
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(300))
                {
                    self.market.resize_at = None;
                    refresh = true;
                }
                if response.clicked() {
                    response.request_focus();
                }
                if response.has_focus() {
                    let delta = ui.input(|i| {
                        if i.key_pressed(egui::Key::ArrowRight)
                            || i.key_pressed(egui::Key::ArrowDown)
                        {
                            1
                        } else if i.key_pressed(egui::Key::ArrowLeft)
                            || i.key_pressed(egui::Key::ArrowUp)
                        {
                            -1
                        } else {
                            0
                        }
                    });
                    if let Some(d) = &data {
                        let tiles: Vec<_> = d.groups.iter().flat_map(|g| &g.tiles).collect();
                        if !tiles.is_empty() && delta != 0 {
                            let current = tiles
                                .iter()
                                .position(|t| self.market.selected.as_deref() == Some(&t.key))
                                .map(|v| v as i32)
                                .unwrap_or(-1);
                            let next = (current + delta).rem_euclid(tiles.len() as i32) as usize;
                            self.market.selected = Some(tiles[next].key.clone());
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Some(tile) = tiles
                                .iter()
                                .find(|t| self.market.selected.as_deref() == Some(&t.key))
                            {
                                self.navigate_stock(&tile.quote.symbol, &tile.quote.market);
                            }
                        }
                    }
                }
                if response.dragged() {
                    self.market.pan += ui.input(|i| i.pointer.delta());
                }
                if response.hovered() {
                    let wheel = ui.input(|i| i.smooth_scroll_delta.y);
                    if wheel.abs() > 0.1 {
                        let old = self.market.zoom;
                        self.market.zoom = (old * (wheel * 0.003).exp()).clamp(1.0, 12.0);
                        if let Some(p) = response.hover_pos() {
                            let pos = p - rect.min;
                            self.market.pan =
                                pos - (pos - self.market.pan) * (self.market.zoom / old);
                        }
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.market.pan = Vec2::ZERO;
                        self.market.zoom = 1.0;
                    }
                }
                self.market.pan.x = self
                    .market
                    .pan
                    .x
                    .clamp(rect.width() * (1.0 - self.market.zoom), 0.0);
                self.market.pan.y = self
                    .market
                    .pan
                    .y
                    .clamp(rect.height() * (1.0 - self.market.zoom), 0.0);
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, Color32::from_rgb(14, 21, 33));
                let mut hovered: Option<&MapTile> = None;
                let mut clicked_industry = None;
                let pointer = response.hover_pos();
                if let Some(d) = &data {
                    for group in &d.groups {
                        let g = project(rect, group.bounds, self.market.zoom, self.market.pan);
                        if !g.intersects(rect) {
                            continue;
                        }
                        let header = if g.width() > 55.0 && g.height() > 45.0 {
                            24.0
                        } else {
                            0.0
                        };
                        if header > 0.0 {
                            let head = Rect::from_min_size(g.min, Vec2::new(g.width(), header));
                            painter.rect_filled(
                                head.shrink(0.5),
                                0.0,
                                Color32::from_rgb(29, 40, 56),
                            );
                            let clipped = painter.with_clip_rect(head.intersect(rect).shrink(4.0));
                            clipped.text(
                                head.left_center() + Vec2::new(7.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                &group.name,
                                egui::FontId::proportional(11.0),
                                Color32::from_rgb(220, 231, 247),
                            );
                            if head.width() > 170.0 {
                                clipped.text(
                                    head.right_center() - Vec2::new(7.0, 0.0),
                                    egui::Align2::RIGHT_CENTER,
                                    pct(group.mean_pct),
                                    egui::FontId::proportional(10.0),
                                    map_pct_color(
                                        group.mean_pct,
                                        self.preferences.colors == "green_up",
                                    ),
                                );
                            }
                            if pointer.is_some_and(|p| head.contains(p)) && response.clicked() {
                                clicked_industry = Some(group.name.clone());
                            }
                        }
                        let body = Rect::from_min_max(
                            g.min + Vec2::new(1.0, header + 1.0),
                            g.max - Vec2::splat(1.0),
                        );
                        for tile in &group.tiles {
                            let r = project(body, tile.bounds, 1.0, Vec2::ZERO);
                            if r.width() < 0.3 || r.height() < 0.3 || !r.intersects(rect) {
                                continue;
                            }
                            let highlighted = if !search.is_empty() {
                                map_matches(tile, &search)
                            } else {
                                !self.market.highlight_watch || tile.watched
                            };
                            let rgb = getrich_core::market_map::tile_rgb(
                                tile.quote.change_pct,
                                self.preferences.colors == "green_up",
                            );
                            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                            painter.rect_filled(
                                r.shrink(0.5),
                                0.0,
                                if highlighted {
                                    color
                                } else {
                                    color.gamma_multiply(0.24)
                                },
                            );
                            let hover = pointer.is_some_and(|p| r.contains(p));
                            if hover {
                                hovered = Some(tile);
                            }
                            let selected =
                                self.market.selected.as_deref() == Some(tile.key.as_str());
                            if selected
                                || hover
                                || ((!search.is_empty() || self.market.highlight_watch)
                                    && highlighted)
                            {
                                painter.rect_stroke(
                                    r.shrink(1.0),
                                    0.0,
                                    Stroke::new(
                                        if selected { 2.5_f32 } else { 1.5_f32 },
                                        if selected {
                                            Color32::from_rgb(255, 215, 130)
                                        } else if hover {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_rgb(144, 185, 255)
                                        },
                                    ),
                                );
                            }
                            if r.width() < 36.0 || r.height() < 23.0 {
                                continue;
                            }
                            let clipped = painter.with_clip_rect(r.intersect(rect).shrink(3.0));
                            let size = (r.width() / 6.0).min(r.height() / 3.8).clamp(10.0, 19.0);
                            let lines = r.height() > 42.0;
                            let color = if highlighted {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(125)
                            };
                            clipped.text(
                                r.center() - Vec2::new(0.0, if lines { size * 0.65 } else { 0.0 }),
                                egui::Align2::CENTER_CENTER,
                                tile.quote.name.as_deref().unwrap_or(&tile.quote.symbol),
                                egui::FontId::proportional(size),
                                color,
                            );
                            if lines {
                                clipped.text(
                                    r.center() + Vec2::new(0.0, size * 0.7),
                                    egui::Align2::CENTER_CENTER,
                                    pct(tile.quote.change_pct),
                                    egui::FontId::proportional((size - 1.0).max(10.0)),
                                    color,
                                );
                            }
                            if tile.watched && r.width() > 70.0 && r.height() > 65.0 {
                                clipped.text(
                                    r.min + Vec2::new(6.0, 4.0),
                                    egui::Align2::LEFT_TOP,
                                    "★",
                                    egui::FontId::proportional(11.0),
                                    Color32::from_rgb(255, 226, 168),
                                );
                            }
                        }
                    }
                }
                if let Some(tile) = hovered {
                    response.clone().on_hover_ui_at_pointer(|ui| {
                        ui.strong(tile.quote.name.as_deref().unwrap_or(&tile.quote.symbol));
                        ui.small(format!("{} · {}", tile.quote.symbol, tile.industry));
                        ui.colored_label(
                            map_pct_color(
                                tile.quote.change_pct,
                                self.preferences.colors == "green_up",
                            ),
                            pct(tile.quote.change_pct),
                        );
                        ui.label(format!(
                            "最新 {} {}",
                            fmt(tile.quote.last),
                            tile.quote.currency.as_deref().unwrap_or("")
                        ));
                        if let Some(d) = &data {
                            ui.small(format!("{} {}", d.metric_label, map_weight(tile, d)));
                        }
                        ui.small(format!(
                            "报价日 {}",
                            tile.quote.trade_date.as_deref().unwrap_or("—")
                        ));
                    });
                    if response.clicked() {
                        self.market.selected = Some(tile.key.clone());
                        self.market.inspector_open = true;
                    }
                    if response.double_clicked() {
                        self.navigate_stock(&tile.quote.symbol, &tile.quote.market);
                    }
                }
                if let Some(industry) = clicked_industry {
                    self.market.query.industry = industry;
                    refresh = true;
                }
                if data.as_ref().is_none_or(|d| d.coverage.displayed == 0) {
                    let title = if self.market.loading || self.map_syncing() {
                        "正在读取市场截面…"
                    } else if data.as_ref().is_some_and(|d| d.coverage.missing_weight > 0) {
                        "当前指标缺少同日权重"
                    } else {
                        "当前范围暂无有效报价"
                    };
                    painter.text(
                        rect.center() - Vec2::new(0.0, 20.0),
                        egui::Align2::CENTER_CENTER,
                        title,
                        egui::FontId::proportional(20.0),
                        Color32::from_rgb(219, 229, 245),
                    );
                    painter.text(
                        rect.center() + Vec2::new(0.0, 15.0),
                        egui::Align2::CENTER_CENTER,
                        if self.map_syncing() {
                            "正在同步行情，完成后自动绘制星图"
                        } else if data.as_ref().is_some_and(|d| d.coverage.missing_weight > 0) {
                            "当前指标缺少同日权重，请切换自动权重或等面积，或刷新行情获取市值"
                        } else if self.settings_cache["provider"]["configured"]
                            .as_bool()
                            .unwrap_or(false)
                        {
                            "点击右上方「刷新行情」获取数据，或打开同步结果查看原因"
                        } else {
                            "先配置数据源，或在数据中心导入行情文件"
                        },
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(140, 157, 180),
                    );
                }
            });
        if self.market.controls_open {
            let mut open = true;
            egui::Window::new("星图筛选与数据说明")
                .open(&mut open)
                .default_width(270.0)
                .default_pos(Pos2::new(16.0, 110.0))
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.screen_rect().height() - 200.0).max(200.0))
                        .show(ui, |ui| {
                            refresh |= self.map_controls(ui);
                        });
                });
            self.market.controls_open = open;
        }
        if self.market.inspector_open {
            let mut open = true;
            let data = self.market.data.clone();
            egui::Window::new("个股速览")
                .open(&mut open)
                .default_width(300.0)
                .default_pos(Pos2::new(
                    (ctx.screen_rect().width() - 332.0).max(16.0),
                    110.0,
                ))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.screen_rect().height() - 200.0).max(200.0))
                        .show(ui, |ui| self.map_inspector(ui, data.as_deref()));
                });
            self.market.inspector_open = open && self.screen == Screen::Pulse;
        }
        if self.market.sync_details_open {
            let mut open = true;
            egui::Window::new("星图数据同步结果")
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    if let Some(job) = &self.market.sync_job {
                        ui.label(format!(
                            "任务 #{} · {}",
                            job["id"],
                            super::management::status_zh(job["status"].as_str().unwrap_or(""))
                        ));
                        if let Some(error) = job["error"].as_str() {
                            ui.colored_label(up_color(), error);
                        }
                        if let Some(warnings) = job["result"]["warnings"].as_array() {
                            for w in warnings {
                                ui.colored_label(
                                    Color32::from_rgb(205, 167, 103),
                                    w.as_str().unwrap_or(""),
                                );
                            }
                        }
                        ui.collapsing("完整结果", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(320.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(
                                                serde_json::to_string_pretty(&job["result"])
                                                    .unwrap_or_default(),
                                            )
                                            .monospace()
                                            .size(12.0),
                                        )
                                        .wrap(),
                                    );
                                });
                        });
                    }
                    if ui.button("打开数据中心").clicked() {
                        self.screen = Screen::Data;
                        self.reload_stats();
                    }
                });
            self.market.sync_details_open = open;
        }
        if refresh && !self.market.loading {
            self.market.zoom = 1.0;
            self.market.pan = Vec2::ZERO;
            self.reload_map();
        }
        if self.market.data.is_none() && !self.market.loading && self.market.error.is_none() {
            self.reload_map();
        }
    }
    fn map_toolbar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let wide = ui.available_width() > 1000.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            if !self.market.full {
                let old = self.market.query.board.clone();
                egui::ComboBox::from_id_salt("map-board-compact")
                    .width(112.0)
                    .selected_text(match old.as_str() {
                        "CN" => "沪深京",
                        "CN.SH" => "沪市",
                        "CN.SZ" => "深市",
                        "CN.BJ" => "北证",
                        "STAR" => "科创板",
                        "CHINEXT" => "创业板",
                        "HK" => "港股",
                        _ => "美股",
                    })
                    .show_ui(ui, |ui| {
                        for (id, label) in [
                            ("CN", "沪深京 A 股"),
                            ("CN.SH", "上证 A 股"),
                            ("CN.SZ", "深证 A 股"),
                            ("CN.BJ", "北证 A 股"),
                            ("STAR", "科创板"),
                            ("CHINEXT", "创业板"),
                            ("HK", "港股"),
                            ("US", "美股"),
                        ] {
                            ui.selectable_value(&mut self.market.query.board, id.into(), label);
                        }
                    });
                if old != self.market.query.board {
                    self.market.query.industry.clear();
                    changed = true;
                }
                let old = self.market.query.metric.clone();
                egui::ComboBox::from_id_salt("map-metric-compact")
                    .width(130.0)
                    .selected_text(match old.as_str() {
                        "auto" => "自动权重",
                        "float_market_cap" => "流通市值",
                        "market_cap" => "总市值",
                        "turnover" => "成交额",
                        _ => "等面积",
                    })
                    .show_ui(ui, |ui| {
                        for id in [
                            "auto",
                            "float_market_cap",
                            "market_cap",
                            "turnover",
                            "equal",
                        ] {
                            ui.selectable_value(
                                &mut self.market.query.metric,
                                id.into(),
                                map_metric(id),
                            );
                        }
                    });
                changed |= old != self.market.query.metric;
                ui.add_sized(
                    [if wide { 180.0 } else { 110.0 }, 34.0],
                    egui::TextEdit::singleline(&mut self.market.search)
                        .hint_text("定位名称或代码")
                        .margin(Vec2::new(10.0, 8.0)),
                );
                if ui.button("筛选").clicked() {
                    self.market.controls_open = !self.market.controls_open;
                }
                if wide && ui.button("速览").clicked() {
                    self.market.inspector_open = !self.market.inspector_open;
                }
            }
            if !self.market.query.industry.is_empty() && ui.button("全部行业").clicked() {
                self.market.query.industry.clear();
                changed = true;
            }
            if ui
                .button("重置")
                .on_hover_text("重置星图缩放与位置")
                .clicked()
            {
                self.market.zoom = 1.0;
                self.market.pan = Vec2::ZERO;
            }
            let cn = !["HK", "US"].contains(&self.market.query.board.as_str());
            let configured = self.settings_cache["provider"]["configured"]
                .as_bool()
                .unwrap_or(false);
            let label = if !cn {
                "刷新本地"
            } else if configured {
                "刷新行情"
            } else {
                "配置数据源"
            };
            if ui
                .add_enabled(!self.map_syncing(), primary_btn(label))
                .clicked()
            {
                if !cn {
                    changed = true;
                } else if !configured {
                    self.settings_tab = 0;
                    self.read_settings();
                    self.screen = Screen::Settings;
                } else {
                    self.request_map_sync();
                }
            }
            if ui
                .button(if self.market.full {
                    "退出专注"
                } else {
                    "专注"
                })
                .clicked()
            {
                self.market.full = !self.market.full;
            }
        });
        if changed {
            self.market.selected = None;
            self.market.data = None;
            self.market.error = None;
        }
        changed
    }
    fn map_indices_compact(&self, ui: &mut egui::Ui) {
        if ["HK", "US"].contains(&self.market.query.board.as_str()) {
            return;
        }
        let show_pct = ui.available_width() > 1000.0;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 2.0);
            ui.spacing_mut().interact_size.y = 20.0;
            ui.columns(5, |cols| {
                for (index, (col, name)) in cols
                    .iter_mut()
                    .zip(["上证指数", "深证成指", "创业板指", "科创50", "沪深300"])
                    .enumerate()
                {
                    let quote = self
                        .market
                        .data
                        .as_ref()
                        .and_then(|d| d.indices.get(index))
                        .and_then(|i| i.quote.as_ref());
                    egui::Frame::none()
                        .fill(surface())
                        .rounding(5.0)
                        .inner_margin(Vec2::new(8.0, 5.0))
                        .show(col, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(name).size(11.0));
                                ui.label(
                                    egui::RichText::new(fmt(quote.and_then(|q| q.last)))
                                        .size(13.0)
                                        .color(
                                            quote
                                                .map(|q| dir_color(&q.direction))
                                                .unwrap_or(muted()),
                                        ),
                                );
                                if show_pct {
                                    ui.colored_label(
                                        quote.map(|q| dir_color(&q.direction)).unwrap_or(muted()),
                                        egui::RichText::new(pct(quote.and_then(|q| q.change_pct)))
                                            .size(11.0),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(
                                quote
                                    .map(|q| {
                                        format!(
                                            "{} · {} · {}",
                                            q.trade_date.as_deref().unwrap_or("日期未知"),
                                            pct(q.change_pct),
                                            q.source.as_deref().unwrap_or("来源未标明")
                                        )
                                    })
                                    .unwrap_or_else(|| {
                                        "尚未同步指数日线，刷新行情时会一并尝试获取".into()
                                    }),
                            );
                        });
                }
            });
        });
    }
    pub(super) fn map_syncing(&self) -> bool {
        self.market.sync_pending
            || self.market.sync_job.as_ref().is_some_and(|j| {
                ["queued", "running", "cancelling"].contains(&j["status"].as_str().unwrap_or(""))
            })
    }
    pub(super) fn request_map_sync(&mut self) {
        if self.map_syncing() {
            return;
        }
        self.market.auto_sync_attempted = true;
        self.market.sync_pending = true;
        self.market.error = None;
        self.submit_action(
            "map-sync",
            "job.submit",
            json!({"kind":"market.sync","params":{}}),
        );
    }
    pub(super) fn maybe_start_map_sync(&mut self) {
        if self.screen == Screen::Pulse
            && !self.market.auto_sync_attempted
            && !["HK", "US"].contains(&self.market.query.board.as_str())
            && self.settings_cache["provider"]["configured"]
                .as_bool()
                .unwrap_or(false)
            && self
                .market
                .data
                .as_ref()
                .is_some_and(|m| m.coverage.universe == 0)
        {
            self.request_map_sync();
        }
    }
    pub(super) fn map_sync_status(&mut self, ui: &mut egui::Ui) {
        let job = self.market.sync_job.clone();
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), 18.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if let Some(error) = &self.market.error {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("读取失败：{error}"))
                                .size(11.0)
                                .color(Color32::from_rgb(219, 166, 78)),
                        )
                        .truncate(),
                    )
                    .on_hover_text(error);
                    return;
                }
                if self.market.sync_pending {
                    ui.small("正在提交行情同步…");
                    ui.spinner();
                    return;
                }
                let Some(job) = job else {
                    ui.colored_label(
                        muted(),
                        egui::RichText::new("点选速览 · 双击详情 · 滚轮缩放 · 拖动平移").size(10.0),
                    );
                    return;
                };
                let status = job["status"].as_str().unwrap_or("");
                if ["queued", "running", "cancelling"].contains(&status) {
                    ui.add(
                        egui::ProgressBar::new(
                            job["progress"].as_f64().unwrap_or(0.0) as f32 / 100.0,
                        )
                        .desired_width(90.0)
                        .desired_height(5.0),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(job["phase"].as_str().unwrap_or("正在同步"))
                                .size(11.0),
                        )
                        .truncate(),
                    );
                    ui.spinner();
                } else {
                    if ui.small_button("查看结果").clicked() {
                        self.market.sync_details_open = true;
                    }
                    let text = match status {
                        "succeeded" => "行情、市值与指数已更新",
                        "partial" => "行情已更新，部分附加数据未获取",
                        "failed" | "interrupted" => "同步未完成，请查看原因并重试",
                        "cancelled" => "同步已取消",
                        _ => "",
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).size(11.0).color(
                            if status == "succeeded" {
                                muted()
                            } else {
                                Color32::from_rgb(219, 166, 78)
                            },
                        ))
                        .truncate(),
                    )
                    .on_hover_text(text);
                }
            },
        );
    }
    fn map_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.colored_label(muted(), egui::RichText::new("市场范围").size(11.0));
        for (id, label) in [
            ("CN", "沪深京 A 股"),
            ("CN.SH", "上证 A 股"),
            ("CN.SZ", "深证 A 股"),
            ("CN.BJ", "北证 A 股"),
            ("STAR", "科创板"),
            ("CHINEXT", "创业板"),
            ("HK", "港股"),
            ("US", "美股"),
        ] {
            if ui
                .add_sized(
                    [165.0, 30.0],
                    egui::Button::new(label).selected(self.market.query.board == id),
                )
                .clicked()
            {
                self.market.query.board = id.into();
                self.market.query.industry.clear();
                changed = true;
            }
        }
        ui.add_space(12.0);
        ui.separator();
        ui.small("面积代表");
        let old = self.market.query.metric.clone();
        egui::ComboBox::from_id_salt("map-metric")
            .width(158.0)
            .selected_text(map_metric(&old))
            .show_ui(ui, |ui| {
                for id in [
                    "auto",
                    "float_market_cap",
                    "market_cap",
                    "turnover",
                    "equal",
                ] {
                    ui.selectable_value(&mut self.market.query.metric, id.into(), map_metric(id));
                }
            });
        changed |= old != self.market.query.metric;
        ui.colored_label(
            muted(),
            egui::RichText::new("颜色 · 涨跌幅 / ±5% 饱和").size(10.0),
        );
        changed |= ui
            .checkbox(&mut self.market.query.watch_only, "仅看当前市场自选")
            .changed();
        ui.checkbox(&mut self.market.highlight_watch, "高亮自选");
        ui.add_space(12.0);
        ui.separator();
        ui.small("定位个股 / 行业");
        ui.add(
            egui::TextEdit::singleline(&mut self.market.search)
                .hint_text("代码 / 名称 / 行业")
                .desired_width(158.0),
        );
        let search = self.market.search.trim().to_lowercase();
        if !search.is_empty() {
            if let Some(data) = self.market.data.clone() {
                let matches: Vec<_> = data
                    .groups
                    .iter()
                    .flat_map(|g| &g.tiles)
                    .filter(|t| map_matches(t, &search))
                    .collect();
                ui.small(format!("{} 个匹配", matches.len()));
                for tile in matches.iter().take(10) {
                    if ui
                        .link(format!(
                            "{} {}",
                            tile.quote.name.as_deref().unwrap_or(&tile.quote.symbol),
                            pct(tile.quote.change_pct)
                        ))
                        .clicked()
                    {
                        self.market.selected = Some(tile.key.clone());
                    }
                }
            }
        }
        if let Some(data) = &self.market.data {
            ui.add_space(12.0);
            ui.separator();
            ui.small("数据覆盖");
            let c = &data.coverage;
            for (name, value) in [
                ("无报价", c.missing_quote),
                ("其他日期", c.other_date),
                ("无效 / 币种不符", c.invalid),
                ("缺少面积权重", c.missing_weight),
                ("盘中延迟", c.delayed),
                ("未分类", c.unclassified),
            ] {
                ui.horizontal(|ui| {
                    ui.colored_label(muted(), egui::RichText::new(name).size(10.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(value.to_string());
                    });
                });
            }
            ui.collapsing("统计口径与数据说明", |ui| {
                for note in &data.notes {
                    ui.colored_label(muted(), egui::RichText::new(note).size(10.0));
                }
            });
        }
        if ui.link("前往数据中心 →").clicked() {
            self.screen = Screen::Data;
            self.reload_stats();
        }
        if changed {
            self.market.selected = None;
            self.market.data = None;
            self.market.error = None;
        }
        changed
    }
    fn map_inspector(&mut self, ui: &mut egui::Ui, data: Option<&MarketMap>) {
        ui.colored_label(muted(), egui::RichText::new("个股速览").size(11.0));
        ui.add_space(16.0);
        let tile = data.and_then(|d| {
            d.groups
                .iter()
                .flat_map(|g| &g.tiles)
                .find(|t| self.market.selected.as_deref() == Some(&t.key))
        });
        if let (Some(tile), Some(data)) = (tile, data) {
            ui.label(
                egui::RichText::new(tile.quote.name.as_deref().unwrap_or(&tile.quote.symbol))
                    .size(22.0)
                    .strong(),
            );
            ui.small(format!(
                "{} · {} {}",
                tile.quote.symbol,
                tile.quote.market,
                if tile.watched { "★ 自选" } else { "" }
            ));
            ui.add_space(12.0);
            ui.label(egui::RichText::new(fmt(tile.quote.last)).size(32.0));
            ui.colored_label(
                map_pct_color(tile.quote.change_pct, self.preferences.colors == "green_up"),
                egui::RichText::new(pct(tile.quote.change_pct)).size(17.0),
            );
            ui.add_space(14.0);
            ui.separator();
            for (key, value) in [
                ("行业", tile.industry.clone()),
                (data.metric_label.as_str(), map_weight(tile, data)),
                (
                    "成交额",
                    amount(tile.quote.turnover, tile.quote.currency.as_deref()),
                ),
                (
                    "报价日",
                    tile.quote.trade_date.clone().unwrap_or_else(|| "—".into()),
                ),
                (
                    "报价来源",
                    tile.quote.source.clone().unwrap_or_else(|| "未标明".into()),
                ),
                (
                    "源时间",
                    if tile.quote.source_kind == "daily" {
                        "日线 · 日期精度".into()
                    } else {
                        tile.quote
                            .ts
                            .as_deref()
                            .map(|s| session::market_timestamp(s, &tile.quote.market))
                            .unwrap_or_else(|| "—".into())
                    },
                ),
                (
                    "权重日期",
                    tile.weight_as_of.clone().unwrap_or_else(|| "不适用".into()),
                ),
                (
                    "权重来源",
                    tile.weight_source
                        .clone()
                        .unwrap_or_else(|| "不适用".into()),
                ),
            ] {
                ui.add_space(6.0);
                ui.colored_label(muted(), egui::RichText::new(key).size(10.0));
                ui.label(egui::RichText::new(value).size(12.0));
            }
            ui.add_space(12.0);
            if ui.add(primary_btn("打开个股详情 →")).clicked() {
                self.navigate_stock(&tile.quote.symbol, &tile.quote.market);
            }
            if ui.button("聚焦所属行业").clicked() {
                self.market.query.industry = tile.industry.clone();
                self.market.selected = None;
                self.market.data = None;
                self.reload_map();
            }
        } else {
            ui.add_space(35.0);
            ui.colored_label(accent(), egui::RichText::new("◎").size(34.0));
            ui.add_space(15.0);
            ui.strong("选择一只股票");
            ui.colored_label(
                muted(),
                "查看价格、面积权重和数据来源。缩放可查看小市值股票。",
            );
        }
        ui.add_space(25.0);
        ui.separator();
        ui.small("报价日期与计算时间分别标注");
        if let Some(data) = data {
            ui.colored_label(
                muted(),
                egui::RichText::new(format!(
                    "计算 {}\n数据版本 {}",
                    shanghai_ts(&data.computed_at),
                    data.data_version
                ))
                .size(10.0),
            );
        }
        ui.add_space(15.0);
        ui.collapsing("A 股市场解读", |ui| {
            ui.label(&self.pulse.headline);
            ui.colored_label(
                muted(),
                format!(
                    "{} · 置信度 {}",
                    self.pulse.regime.label,
                    match self.pulse.regime.confidence.as_str() {
                        "high" => "高",
                        "medium" => "中",
                        _ => "低",
                    }
                ),
            );
            for reason in &self.pulse.regime.reasons {
                ui.small(reason);
            }
            ui.small("解读基于 A 股样本，与当前星图市场筛选独立。");
        });
    }
}
fn map_metric(id: &str) -> &str {
    match id {
        "float_market_cap" => "流通市值",
        "market_cap" => "总市值",
        "turnover" => "成交额",
        "equal" => "等面积",
        _ => "自动 · 优先流通市值",
    }
}
fn map_matches(tile: &MapTile, search: &str) -> bool {
    format!(
        "{} {} {}",
        tile.quote.symbol,
        tile.quote.name.as_deref().unwrap_or(""),
        tile.industry
    )
    .to_lowercase()
    .contains(search)
}
fn project(rect: Rect, b: Bounds, zoom: f32, pan: Vec2) -> Rect {
    Rect::from_min_size(
        rect.min
            + pan
            + Vec2::new(
                b.x as f32 * rect.width() * zoom,
                b.y as f32 * rect.height() * zoom,
            ),
        Vec2::new(
            b.w as f32 * rect.width() * zoom,
            b.h as f32 * rect.height() * zoom,
        ),
    )
}
fn map_pct_color(value: Option<f64>, green: bool) -> Color32 {
    let up = Color32::from_rgb(255, 140, 152);
    let down = Color32::from_rgb(85, 214, 172);
    match value {
        Some(n) if n > 0.0 => {
            if green {
                down
            } else {
                up
            }
        }
        Some(n) if n < 0.0 => {
            if green {
                up
            } else {
                down
            }
        }
        _ => Color32::from_rgb(154, 168, 190),
    }
}
fn map_weight(tile: &MapTile, data: &MarketMap) -> String {
    if data.metric == "equal" {
        "1 份".into()
    } else {
        amount(Some(tile.weight), Some(&data.currency))
    }
}
