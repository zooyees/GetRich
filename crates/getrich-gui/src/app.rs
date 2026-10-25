//! Quote board in egui: dense Tonghuashun table, Apple chrome.

use eframe::egui::{self, Color32, Margin, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
mod async_ui;
mod design;
mod empty_states;
#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod layout_tests;
mod management;
mod market_footer;
mod market_map;
mod stock_research;
mod watch_search;
use crate::controller::Controller;
use getrich_core::models::{InstrumentKey, Timeframe};
use getrich_core::pulse::MarketPulse;
use getrich_core::service::App;
use getrich_core::session;
use getrich_core::view::{BarPage, QuoteBoardPage, QuoteRow, StockDetail, StoreStats};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod palette;
use design::{
    action, card, card_title, draw_quote_summary, field_label, page_canvas, page_title,
    secondary_surface, selected_surface, status_badge, text_field,
};
use palette::*;

const BOARDS: &[(&str, &str)] = &[
    ("WATCH", "自选"),
    ("ALL", "全部"),
    ("CN", "沪深京"),
    ("CN.SH", "沪市"),
    ("CN.SZ", "深市"),
    ("CHINEXT", "创业板"),
    ("STAR", "科创板"),
    ("CN.BJ", "北证"),
    ("HK", "港股"),
    ("US", "美股"),
];

const ALL_COLS: &[(&str, &str, bool)] = &[
    ("symbol", "代码", false),
    ("name", "名称", false),
    ("last", "最新", true),
    ("change_pct", "涨跌幅", true),
    ("change_amt", "涨跌额", true),
    ("turnover", "成交额", true),
    ("amplitude", "振幅", true),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Quotes,
    Detail,
    Pulse,
    Data,
    Settings,
}

pub struct GetRichApp {
    core: Arc<App>,
    market: market_map::MapState,
    stock: stock_research::StockResearch,
    screen: Screen,
    board: String,
    query: String,
    sort: String,
    page: Option<QuoteBoardPage>,
    stats: Option<StoreStats>,
    error: Option<String>,
    detail: Option<StockDetail>,
    bars: Option<BarPage>,
    tf: Timeframe,
    selected: Option<usize>,
    detail_pinned: bool,
    watch_search: watch_search::WatchSearch,
    taskbar_icon_applied: bool,
    taskbar_icon_attempts: u8,
    query_dirty: Option<Instant>,
    focus_search: bool,
    import_path: String,
    import_msg: String,
    kline_hover: Option<usize>,
    kline_span: usize,
    kline_end: usize,
    quote_tick: Option<Instant>,
    pulse: MarketPulse,
    pulse_tick: Option<Instant>,
    industry: String,
    controller: Controller,
    settings_cache: Value,
    jobs_cache: Value,
    settings_tick: Option<Instant>,
    preview: Option<Value>,
    import_data: Option<Value>,
    pending_key: Option<InstrumentKey>,
    loading: bool,
    detail_loading: bool,
    page_offset: usize,
    change_filter: String,
    provider_user: String,
    provider_token: String,
    provider_enabled: bool,
    provider_poll: u64,
    preferences: getrich_core::config::UiPreferences,
    watch_symbol: String,
    watch_market: String,
    watch_name: String,
    job_signature: String,
    watch_transfer: String,
    theme_signature: String,
    settings_tab: usize,
    settings_default_board: String,
    ui_notice: Option<(String, Instant)>,
    startup_status: Arc<std::sync::Mutex<Option<String>>>,
}

impl GetRichApp {
    pub(super) fn ui_navigation(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::none()
                    .fill(surface())
                    .inner_margin(Margin::symmetric(24.0, 12.0))
                    .stroke(Stroke::new(1.0_f32, design::border())),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if nav_label(
                        ui,
                        "行情",
                        matches!(self.screen, Screen::Quotes | Screen::Detail),
                    )
                    .clicked()
                    {
                        self.open_quotes();
                    }
                    if nav_label(ui, "大盘星图", self.screen == Screen::Pulse).clicked() {
                        self.screen = Screen::Pulse;
                        self.read_pulse();
                        self.reload_map();
                        self.pulse_tick = Some(Instant::now());
                    }
                    if nav_label(ui, "数据中心", self.screen == Screen::Data).clicked() {
                        self.screen = Screen::Data;
                        self.reload_stats();
                    }
                    if nav_label(ui, "设置", self.screen == Screen::Settings).clicked() {
                        self.read_settings();
                        self.screen = Screen::Settings;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let searchable = matches!(self.screen, Screen::Quotes | Screen::Detail);
                        if ui.available_width() > if searchable { 480.0 } else { 300.0 } {
                            ui.colored_label(
                                muted(),
                                egui::RichText::new(self.status_text()).size(12.0),
                            );
                            ui.add_space(12.0);
                        }
                        if searchable {
                            let resp = ui.add_sized(
                                [190.0_f32.min(ui.available_width().max(120.0)), 36.0],
                                egui::TextEdit::singleline(&mut self.query)
                                    .hint_text("搜索名称或代码")
                                    .margin(Vec2::new(12.0, 9.0)),
                            );
                            if self.focus_search {
                                resp.request_focus();
                                self.focus_search = false;
                            }
                            if resp.changed() {
                                self.query_dirty = Some(Instant::now());
                            }
                        } else if ui.available_width() > 160.0 {
                            ui.colored_label(
                                muted(),
                                egui::RichText::new("本地行情工作区").size(12.0),
                            );
                        }
                    });
                });
                if let Some(err) = self.error.clone() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.colored_label(up_color(), format!("无法完成操作：{err}"));
                        if ui.button("重试").clicked() {
                            match self.screen {
                                Screen::Quotes | Screen::Detail => self.reload_list(),
                                Screen::Pulse => {
                                    self.market.error = None;
                                    self.reload_map();
                                }
                                Screen::Data => self.reload_stats(),
                                Screen::Settings => {}
                            }
                        }
                    });
                }
            });
    }
    pub fn install_theme(ctx: &egui::Context) {
        ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = Vec2::new(10.0, 10.0);
            style.spacing.button_padding = Vec2::new(14.0, 8.0);
            style.spacing.interact_size = Vec2::new(40.0, 36.0);
            style.spacing.window_margin = Margin::same(20.0);
            style.spacing.text_edit_width = 280.0;
            style.spacing.combo_width = 240.0;
            style.spacing.icon_width = 16.0;
            style.spacing.icon_spacing = 8.0;
            style.spacing.slider_width = 210.0;
            style.spacing.scroll.bar_width = 6.0;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        });
    }

    pub fn new(core: Arc<App>, startup_status: Arc<std::sync::Mutex<Option<String>>>) -> Self {
        let stats = None;
        let board = core.cfg.universe.default_board.clone();
        let pulse = getrich_core::pulse::error_pulse("正在初始化市场数据");
        let controller = Controller::new(Arc::clone(&core));
        let preferences = core.cfg.preferences.clone();
        let provider_user = core.cfg.data.tushare.user_id.clone();
        let provider_enabled = core.cfg.data.tushare.enabled;
        let provider_poll = core.cfg.data.tushare.poll_ms;
        let mut app = Self {
            core,
            market: Default::default(),
            stock: Default::default(),
            screen: Screen::Quotes,
            board: board.clone(),
            query: String::new(),
            sort: "symbol".into(),
            page: None,
            stats,
            error: None,
            detail: None,
            bars: None,
            tf: Timeframe::D1,
            selected: None,
            detail_pinned: false,
            watch_search: Default::default(),
            taskbar_icon_applied: false,
            taskbar_icon_attempts: 0,
            query_dirty: None,
            focus_search: false,
            import_path: "examples/demo.board.json".into(),
            import_msg: String::new(),
            kline_hover: None,
            kline_span: 0,
            kline_end: 0,
            quote_tick: None,
            pulse,
            pulse_tick: None,
            industry: String::new(),
            controller,
            settings_cache: json!({}),
            jobs_cache: json!({"jobs":[]}),
            settings_tick: None,
            preview: None,
            import_data: None,
            pending_key: None,
            loading: false,
            detail_loading: false,
            page_offset: 0,
            change_filter: String::new(),
            provider_user,
            provider_token: String::new(),
            provider_enabled,
            provider_poll,
            preferences,
            watch_symbol: String::new(),
            watch_market: "CN.SH".into(),
            watch_name: String::new(),
            job_signature: String::new(),
            watch_transfer: String::new(),
            theme_signature: String::new(),
            settings_tab: 0,
            settings_default_board: board.clone(),
            ui_notice: None,
            startup_status,
        };
        app.reload_list();
        app
    }

    fn open_quotes(&mut self) {
        self.screen = Screen::Quotes;
        self.reload_list();
    }

    fn open_row(&mut self, row: &QuoteRow) {
        self.detail_pinned = false;
        self.open_stock(&row.symbol, &row.market);
    }

    fn toggle_watch_current(&mut self) {
        let Some(d) = self.detail.clone() else { return };
        self.apply_watch_toggle(&d.quote.symbol, &d.quote.market, d.quote.name.clone());
    }

    fn status_text(&self) -> String {
        match &self.stats {
            None => "无法连接".into(),
            Some(s) if s.empty => "空库".into(),
            Some(s) => {
                let n = s.listings;
                match &s.last_quote_ts {
                    Some(ts) => format!("已导入 {n} 只 · 行情 {}（上海）", shanghai_ts(ts)),
                    None => format!("已导入 {n} 只"),
                }
            }
        }
    }
}

impl eframe::App for GetRichApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Default eframe clear is near-black; the SidePanel resize gap is not
        // filled by panel_fill, so it showed as a wide black strip.
        background().to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.taskbar_icon_attempts < 10 {
            self.taskbar_icon_attempts += 1;
            if !self.taskbar_icon_applied {
                self.taskbar_icon_applied = crate::windows_icon::apply_embedded_icon();
            }
            if !self.taskbar_icon_applied {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        let _ = crate::windows_dpi::install();
        if let Some(t) = self.query_dirty {
            if t.elapsed() >= std::time::Duration::from_millis(180) {
                self.query_dirty = None;
                self.reload_list();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }

        self.apply_preferences(ctx);
        if let Some(error) = self.startup_status.lock().ok().and_then(|e| e.clone()) {
            self.error = Some(error);
        }
        self.handle_keys(ctx);
        self.apply_io();
        self.tick_background();
        self.tick_live_quote();
        self.tick_pulse();
        if self.loading
            || self.detail_loading
            || matches!(self.screen, Screen::Data | Screen::Settings)
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        } else if self.screen == Screen::Pulse {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        } else if matches!(self.screen, Screen::Quotes | Screen::Detail) {
            let live = self.core.ingest.in_session() && self.core.ingest.worker_running();
            let wait = if live {
                std::time::Duration::from_secs(3)
            } else {
                std::time::Duration::from_secs(15)
            };
            ctx.request_repaint_after(wait);
        }

        self.ui_navigation(ctx);

        match self.screen {
            Screen::Quotes | Screen::Detail => self.ui_quotes(ctx),
            Screen::Pulse => self.ui_pulse(ctx),
            Screen::Data => self.ui_data(ctx),
            Screen::Settings => self.ui_settings(ctx),
        }
        if let Some((message, created)) = &self.ui_notice {
            if created.elapsed() < Duration::from_secs(6) {
                ctx.request_repaint_after(Duration::from_millis(200));
                egui::Area::new(egui::Id::new("workspace-notice"))
                    .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -24.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(selected_surface())
                            .stroke(Stroke::new(1.0_f32, design::border()))
                            .rounding(10.0)
                            .inner_margin(Vec2::new(18.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_max_width(440.0);
                                ui.label(message);
                            });
                    });
            } else {
                self.ui_notice = None;
            }
        }
    }
}

impl GetRichApp {
    fn handle_keys(&mut self, ctx: &egui::Context) {
        let wants_text = ctx.wants_keyboard_input();
        let cmd_k =
            ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::K));
        let slash = ctx.input(|i| i.key_pressed(egui::Key::Slash));
        if cmd_k || (slash && !wants_text) {
            self.focus_search = true;
            if !matches!(self.screen, Screen::Quotes | Screen::Detail) {
                self.open_quotes();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.query.is_empty() {
                self.query.clear();
                self.reload_list();
            } else if self.detail.is_some() {
                self.detail = None;
                self.screen = Screen::Quotes;
            }
        }
        if wants_text {
            return;
        }
        if matches!(self.screen, Screen::Quotes | Screen::Detail) {
            if self.detail.is_some() {
                if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
                    self.tf = Timeframe::D1;
                    self.reload_bars();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Num2)) {
                    self.tf = Timeframe::W1;
                    self.reload_bars();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Num3)) {
                    self.tf = Timeframe::Mo1;
                    self.reload_bars();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::W)) {
                if self.detail.is_some() {
                    self.toggle_watch_current();
                } else if self.board == "WATCH" {
                    if let Some(i) = self.selected {
                        if let Some(row) = self.page.as_ref().and_then(|p| p.rows.get(i)).cloned() {
                            self.apply_watch_toggle(&row.symbol, &row.market, row.name.clone());
                        }
                    }
                }
            }
        }
        if matches!(self.screen, Screen::Quotes | Screen::Detail) {
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::J)) {
                let n = self.page.as_ref().map(|p| p.rows.len()).unwrap_or(0);
                if n > 0 {
                    let i = self.selected.unwrap_or(0).saturating_add(1).min(n - 1);
                    self.selected = Some(i);
                    if let Some(row) = self.page.as_ref().and_then(|p| p.rows.get(i)).cloned() {
                        self.open_row(&row);
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::K)) {
                let i = self.selected.unwrap_or(0).saturating_sub(1);
                self.selected = Some(i);
                if let Some(row) = self.page.as_ref().and_then(|p| p.rows.get(i)).cloned() {
                    self.open_row(&row);
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(i) = self.selected {
                    if let Some(row) = self.page.as_ref().and_then(|p| p.rows.get(i)).cloned() {
                        self.open_row(&row);
                    }
                }
            }
            if self.board == "WATCH" && ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                if let Some(i) = self.selected {
                    if let Some(row) = self.page.as_ref().and_then(|p| p.rows.get(i)).cloned() {
                        self.apply_watch_toggle(&row.symbol, &row.market, row.name.clone());
                    }
                }
            }
        }
    }

    fn tick_pulse(&mut self) {
        if self.screen != Screen::Pulse {
            return;
        }
        let due = self
            .pulse_tick
            .map(|t| t.elapsed() >= Duration::from_secs(3))
            .unwrap_or(true);
        if !due {
            return;
        }
        self.read_pulse();
        self.reload_map();
        self.pulse_tick = Some(Instant::now());
    }

    fn tick_live_quote(&mut self) {
        if !matches!(self.screen, Screen::Quotes | Screen::Detail) {
            return;
        }
        let worker = self.core.ingest.worker_running();
        let in_session = self.core.ingest.in_session();
        let interval = if worker && in_session {
            Duration::from_secs(3)
        } else {
            Duration::from_secs(15)
        };
        let due = self
            .quote_tick
            .map(|t| t.elapsed() >= interval)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.quote_tick = Some(Instant::now());
        self.refresh_from_db();
    }

    fn ui_quotes(&mut self, ctx: &egui::Context) {
        if self.stats.as_ref().is_some_and(|s| s.empty)
            && self.query.is_empty()
            && self.industry.is_empty()
        {
            self.ui_empty_quotes(ctx);
            return;
        }
        let narrow = ctx.screen_rect().width() < 1000.0;
        if narrow {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(surface()).inner_margin(20.0))
                .show(ctx, |ui| {
                    if self.screen == Screen::Detail {
                        if ui.button("← 返回行情列表").clicked() {
                            self.screen = Screen::Quotes;
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| self.draw_detail(ui));
                    } else {
                        self.draw_list(ui);
                    }
                });
            return;
        }
        let panel = egui::SidePanel::left(egui::Id::new((
            "quote-list",
            self.preferences.list_width.round() as u32,
        )))
        .resizable(true)
        .show_separator_line(false)
        .default_width(self.preferences.list_width)
        .min_width(280.0)
        .max_width(ctx.screen_rect().width() * 0.6)
        .frame(
            egui::Frame::none()
                .fill(surface())
                .inner_margin(18.0)
                .stroke(Stroke::new(1.0_f32, design::border())),
        )
        .show(ctx, |ui| self.draw_list(ui));
        let width = panel.response.rect.width().clamp(280.0, 900.0);
        if (width - self.preferences.list_width).abs() > 10.0
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.preferences.list_width = width;
            self.submit_action(
                "settings",
                "settings.save",
                json!({"preferences":{"list_width":width}}),
            );
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(surface()).inner_margin(24.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.draw_detail(ui));
            });
    }

    fn draw_list(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("市场行情").size(19.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link("大盘星图 ↗").clicked() {
                    self.screen = Screen::Pulse;
                    self.reload_map();
                }
            });
        });
        ui.add_space(12.0);
        egui::ScrollArea::horizontal()
            .id_salt("boards")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (id, label) in BOARDS {
                        if chip(ui, label, self.board == *id).clicked() {
                            self.board = (*id).into();
                            self.industry.clear();
                            self.change_filter.clear();
                            self.reload_list();
                        }
                    }
                });
            });
        let old_filter = self.change_filter.clone();
        egui::ComboBox::from_id_salt("change-filter")
            .selected_text(filter_label(&self.change_filter))
            .show_ui(ui, |ui| {
                for (id, label) in [
                    ("", "全部涨跌"),
                    ("up", "上涨"),
                    ("down", "下跌"),
                    ("flat", "平盘"),
                    ("limit_up", "涨停估算"),
                    ("limit_down", "跌停估算"),
                ] {
                    ui.selectable_value(&mut self.change_filter, id.into(), label);
                }
            });
        if old_filter != self.change_filter {
            self.reload_list();
        }
        if !self.industry.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("行业：{}", self.industry));
                if ui.button("清除").clicked() {
                    self.industry.clear();
                    self.reload_list();
                }
            });
        }
        if self.loading {
            ui.label("正在更新…");
        }
        let Some(page) = self.page.clone() else {
            ui.spinner();
            return;
        };
        if page.empty {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(empty_title(&page.empty_kind))
                        .size(19.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.colored_label(muted(), &page.empty_message);
                ui.add_space(12.0);
                if page.empty_kind == "no_data" && action(ui, "导入数据", true).clicked() {
                    self.screen = Screen::Data;
                }
                if page.empty_kind == "no_watch" && action(ui, "管理自选", true).clicked() {
                    self.settings_tab = 2;
                    self.screen = Screen::Settings;
                    self.read_settings();
                }
                if page.empty_kind == "no_match" && action(ui, "清除筛选", false).clicked() {
                    self.query.clear();
                    self.industry.clear();
                    self.change_filter.clear();
                    self.reload_list();
                }
            });
            return;
        }
        let columns: Vec<_> = self
            .preferences
            .columns
            .iter()
            .filter_map(|id| ALL_COLS.iter().find(|(key, _, _)| key == id))
            .collect();
        let base_width: f32 = columns
            .iter()
            .map(|(id, _, _)| if *id == "name" { 130.0 } else { 82.0 })
            .sum();
        let table_h = (ui.available_height() - 68.0).max(100.0);
        let mut selected = None;
        let mut sort = None;
        egui::ScrollArea::horizontal()
            .id_salt("quote-columns")
            .show(ui, |ui| {
                ui.set_min_width(base_width.max(ui.available_width()));
                let widths: Vec<f32> = columns
                    .iter()
                    .map(|(id, _, _)| if *id == "name" { 130.0 } else { 82.0 })
                    .collect();
                ui.horizontal(|ui| {
                    for (i, (id, label, _)) in columns.iter().enumerate() {
                        let on =
                            self.sort.trim_end_matches("_asc").trim_end_matches("_desc") == *id;
                        let title = format!(
                            "{}{}",
                            label,
                            if on {
                                if self.sort.ends_with("_asc") || *id == "symbol" || *id == "name" {
                                    " ↑"
                                } else {
                                    " ↓"
                                }
                            } else {
                                ""
                            }
                        );
                        if ui
                            .add_sized([widths[i], 36.0], egui::Button::new(title).frame(false))
                            .clicked()
                        {
                            sort = Some(*id);
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("quote-rows")
                    .max_height(table_h)
                    .show_rows(ui, 38.0, page.rows.len(), |ui, range| {
                        for index in range {
                            let row = &page.rows[index];
                            let is_selected = self.pending_key.as_ref().is_some_and(|k| {
                                k.symbol == row.symbol && k.market.as_str() == row.market
                            });
                            let bg = ui.painter().add(egui::Shape::Noop);
                            let response = ui.horizontal(|ui| {
                                for (i, (id, _, numeric)) in columns.iter().enumerate() {
                                    let value = match *id {
                                        "symbol" => row.symbol.clone(),
                                        "name" => row.name.clone().unwrap_or_default(),
                                        "last" => fmt(row.last),
                                        "change_pct" => pct(row.change_pct),
                                        "change_amt" => signed(row.change),
                                        "turnover" => amount(row.turnover, row.currency.as_deref()),
                                        "amplitude" => pct(row.amplitude),
                                        _ => String::new(),
                                    };
                                    let color = if *numeric
                                        && ["last", "change_pct", "change_amt"].contains(id)
                                    {
                                        dir_color(&row.direction)
                                    } else {
                                        text_color()
                                    };
                                    let label = egui::RichText::new(value).color(color).size(13.0);
                                    let (cell, _) = ui.allocate_exact_size(
                                        Vec2::new(widths[i], 36.0),
                                        Sense::hover(),
                                    );
                                    let mut cell_ui = ui.new_child(
                                        egui::UiBuilder::new().max_rect(cell).layout(
                                            egui::Layout::left_to_right(egui::Align::Center)
                                                .with_main_align(if *numeric {
                                                    egui::Align::Max
                                                } else {
                                                    egui::Align::Min
                                                }),
                                        ),
                                    );
                                    cell_ui.add(egui::Label::new(label).truncate());
                                }
                            });
                            let row_rect = response.response.rect;
                            let row_response = ui
                                .interact(
                                    row_rect,
                                    ui.id().with(("quote-row", index)),
                                    Sense::click(),
                                )
                                .on_hover_text(format!(
                                    "{} · {} · {}\n点击选择，↑/↓ 切换标的",
                                    row.symbol, row.market, row.data_status
                                ));
                            if is_selected || row_response.hovered() {
                                ui.painter().set(
                                    bg,
                                    egui::Shape::rect_filled(
                                        row_rect,
                                        4.0,
                                        if is_selected {
                                            selected_surface()
                                        } else {
                                            secondary_surface()
                                        },
                                    ),
                                );
                            }
                            row_response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    format!("{} {}", row.symbol, row.name.as_deref().unwrap_or("")),
                                )
                            });
                            if row_response.clicked() {
                                selected = Some(index);
                            }
                        }
                    });
            });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.page_offset > 0 && !self.loading,
                    egui::Button::new("上一页"),
                )
                .clicked()
            {
                self.page_offset = self.page_offset.saturating_sub(200);
                self.reload_board(true);
            }
            ui.small(format!(
                "{}–{} / {} 只",
                self.page_offset + 1,
                self.page_offset + page.count,
                page.matched
            ));
            if ui
                .add_enabled(
                    self.page_offset + page.count < page.matched && !self.loading,
                    egui::Button::new("下一页"),
                )
                .clicked()
            {
                self.page_offset += 200;
                self.reload_board(true);
            }
        });
        if let Some(id) = sort {
            self.sort = if self.sort == id {
                format!(
                    "{id}{}",
                    if ["symbol", "name"].contains(&id) {
                        "_desc"
                    } else {
                        "_asc"
                    }
                )
            } else {
                id.into()
            };
            self.reload_list();
        } else if let Some(index) = selected {
            self.selected = Some(index);
            if let Some(row) = page.rows.get(index) {
                self.open_row(row);
            }
        }
    }

    fn draw_detail(&mut self, ui: &mut egui::Ui) {
        if self.detail.is_none() {
            self.detail_placeholder(ui);
            return;
        }
        ui.spacing_mut().item_spacing.y = 6.0;
        let mut watch_clicked = false;
        let mut pull_bars = false;
        let mut new_tf: Option<Timeframe> = None;
        {
            let detail = self.detail.as_ref().unwrap();
            let q = &detail.quote;
            let dir = dir_color(&q.direction);
            let name = q.name.clone().unwrap_or_else(|| q.symbol.clone());
            let ccy = q.currency.clone().unwrap_or_default();
            let last = if detail.empty.no_quote {
                "暂无报价".into()
            } else {
                fmt(q.last)
            };
            let chg_line = format!("{}  {}", signed(q.change), pct(q.change_pct));
            let star = if detail.watched {
                "★ 自选"
            } else {
                "☆ 自选"
            };
            let board_label = q.board_label.clone();
            let symbol = q.symbol.clone();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(name).size(27.0).strong());
                    ui.label(
                        egui::RichText::new(format!("{symbol}  ·  {board_label}  {ccy}"))
                            .color(muted())
                            .size(13.0),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(star).clicked() {
                        watch_clicked = true;
                    }
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(last).size(29.0).strong().color(dir));
                        ui.label(egui::RichText::new(chg_line).size(14.0).color(dir));
                    });
                });
            });
        }
        ui.add_space(12.0);
        if let Some(detail) = &self.detail {
            draw_quote_summary(ui, detail);
        }
        ui.add_space(6.0);
        self.stock_tabs(ui);
        if self.stock.tab != 0 {
            self.research_panel(ui);
            if watch_clicked {
                self.toggle_watch_current();
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if chip(ui, "分时", self.stock.intraday).clicked() {
                self.stock.intraday = true;
            }
            for (tf, label) in [
                (Timeframe::D1, "日K"),
                (Timeframe::W1, "周K"),
                (Timeframe::Mo1, "月K"),
            ] {
                if chip(ui, label, !self.stock.intraday && self.tf == tf).clicked() {
                    self.stock.intraday = false;
                    new_tf = Some(tf);
                }
            }
            if !self.stock.intraday {
                ui.label(egui::RichText::new("MA5").size(11.0).color(muted()));
                ui.label(egui::RichText::new("MA10").size(11.0).color(accent()));
                ui.label(
                    egui::RichText::new("MA20")
                        .size(11.0)
                        .color(Color32::from_rgb(191, 72, 0)),
                );
            }
        });
        ui.add_space(8.0);
        if self.stock.intraday {
            self.intraday_chart(ui);
            if watch_clicked {
                self.toggle_watch_current();
            }
            return;
        }
        self.research_status(ui, "history");
        self.dataset_note(ui, "history");
        if let Some(b) = &self.bars {
            ui.small(format!(
                "{} — {}  ·  {} 根  ·  原始价格 / 未复权",
                b.from.as_deref().unwrap_or("—"),
                b.to.as_deref().unwrap_or("—"),
                b.count
            ));
        }
        let bars_empty = self.bars.as_ref().map(|b| b.empty).unwrap_or(true);
        if let Some(bars) = self.bars.as_ref().filter(|b| !b.empty) {
            if bars.partial_last_period {
                ui.small("最新周/月周期仍在进行，按已导入日线合成。以行情日期为准。");
            }
            ui.horizontal(|ui| {
                for (action, label) in [
                    ("left", "←"),
                    ("right", "→"),
                    ("in", "＋"),
                    ("out", "－"),
                    ("reset", "回到最新"),
                ] {
                    if ui.button(label).clicked() {
                        let (_, end, span) = crate::chart_model::viewport(
                            bars.rows.len(),
                            self.kline_span,
                            self.kline_end,
                        );
                        match action {
                            "left" => {
                                self.kline_end = end.saturating_sub((span / 2).max(1)).max(span)
                            }
                            "right" => {
                                let next = (end + span / 2).min(bars.rows.len());
                                self.kline_end = if next == bars.rows.len() { 0 } else { next };
                            }
                            "in" => self.kline_span = (span * 4 / 5).max(1),
                            "out" => self.kline_span = (span * 5 / 4 + 1).min(bars.rows.len()),
                            _ => {
                                self.kline_span = 0;
                                self.kline_end = 0;
                            }
                        }
                    }
                }
            });
        }
        let empty_msg = self.bars.as_ref().map(|b| b.empty_message.clone());
        if bars_empty {
            egui::Frame::none()
                .fill(surface())
                .rounding(Rounding::same(16.0))
                .inner_margin(Margin::same(40.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(280.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("暂无该周期 K 线").size(18.0).strong());
                        if let Some(msg) = &empty_msg {
                            ui.label(egui::RichText::new(msg).color(muted()));
                        }
                        ui.add_space(10.0);
                        let supported = self
                            .detail
                            .as_ref()
                            .is_some_and(|d| d.quote.market.starts_with("CN."))
                            && self.settings_cache["provider"]["configured"]
                                .as_bool()
                                .unwrap_or(false);
                        if ui
                            .add_enabled(supported, primary_btn("补全历史日线"))
                            .clicked()
                        {
                            pull_bars = true;
                        }
                    });
                });
        } else if let Some(bars) = self.bars.as_ref() {
            let h = (ui.available_height() - 90.0).clamp(260.0, 480.0);
            let mut span = self.kline_span;
            let mut hover = self.kline_hover;
            egui::Frame::none()
                .fill(surface())
                .rounding(Rounding::same(16.0))
                .show(ui, |ui| {
                    ui.allocate_ui(Vec2::new(ui.available_width(), h), |ui| {
                        hover = draw_kline(ui, &bars.rows, &mut span, &mut self.kline_end, hover);
                    });
                });
            self.kline_span = span;
            self.kline_hover = hover;
        }
        ui.add_space(14.0);
        if let Some(d) = &self.detail {
            ui.collapsing("公司资料与数据来源", |ui| {
                for (key, value) in [
                    ("名称", d.listing.name.clone().unwrap_or_else(|| "—".into())),
                    (
                        "行业",
                        d.listing
                            .industry
                            .clone()
                            .unwrap_or_else(|| "未分类".into()),
                    ),
                    (
                        "上市日期",
                        d.listing.listing_date.clone().unwrap_or_else(|| "—".into()),
                    ),
                    (
                        "报价来源",
                        d.quote.source.clone().unwrap_or_else(|| "未标明".into()),
                    ),
                    (
                        "接收时间",
                        d.quote
                            .received_at
                            .as_deref()
                            .map(shanghai_ts)
                            .unwrap_or_else(|| "—".into()),
                    ),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(muted(), key);
                        ui.label(value);
                    });
                }
                ui.small("图表为原始价格，未复权；周 / 月 K 由已导入日线合成。");
            });
        }
        if watch_clicked {
            self.toggle_watch_current();
        }
        if let Some(tf) = new_tf {
            self.tf = tf;
            self.reload_bars();
        }
        if pull_bars {
            if let Some(d) = self.detail.as_ref() {
                let p = json!({"symbol":d.quote.symbol,"market":d.quote.market});
                let _ = p;
                self.request_research("history", true);
            }
        }
    }
}

fn filter_label(kind: &str) -> &str {
    match kind {
        "up" => "上涨",
        "down" => "下跌",
        "flat" => "平盘",
        "limit_up" => "涨停估算",
        "limit_down" => "跌停估算",
        _ => "全部涨跌",
    }
}

fn empty_title(kind: &str) -> &'static str {
    match kind {
        "no_match" => "无匹配结果",
        "no_watch" => "还没有自选",
        _ => "库是空的",
    }
}

fn nav_label(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(14.0).color(if on {
            accent()
        } else {
            muted()
        }))
        .fill(if on {
            selected_surface()
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE)
        .rounding(7.0)
        .min_size(Vec2::new(58.0, 36.0)),
    )
}

fn chip(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let color = if on { accent() } else { muted() };
    let fill = if on {
        selected_surface()
    } else {
        Color32::TRANSPARENT
    };
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(13.0).color(color))
            .fill(fill)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(5.0)),
    )
}

fn primary_btn(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(accent())
        .rounding(Rounding::same(6.0))
}

fn dir_color(dir: &str) -> Color32 {
    match dir {
        "up" => up_color(),
        "down" => down_color(),
        _ => muted(),
    }
}

fn fmt(n: Option<f64>) -> String {
    match n {
        Some(v) => format!("{v:.2}"),
        None => "—".into(),
    }
}

fn signed(n: Option<f64>) -> String {
    match n {
        Some(v) if v > 0.0 => format!("+{v:.2}"),
        Some(v) => format!("{v:.2}"),
        None => "—".into(),
    }
}

fn pct(n: Option<f64>) -> String {
    match n {
        Some(v) if v > 0.0 => format!("+{v:.2}%"),
        Some(v) => format!("{v:.2}%"),
        None => "—".into(),
    }
}

fn vol(n: Option<f64>) -> String {
    match n {
        Some(v) if v.abs() >= 1e8 => format!("{:.2}亿", v / 1e8),
        Some(v) if v.abs() >= 1e4 => format!("{:.2}万", v / 1e4),
        Some(v) => format!("{v:.0}"),
        None => "—".into(),
    }
}

fn amount(n: Option<f64>, currency: Option<&str>) -> String {
    let value = vol(n);
    match (value.as_str(), currency.filter(|value| !value.is_empty())) {
        ("—", _) | (_, None) => value,
        (_, Some(currency)) => format!("{value} {currency}"),
    }
}

fn shanghai_ts(value: &str) -> String {
    getrich_core::session::parse_cn_ts(value)
        .map(getrich_core::session::shanghai_from_utc)
        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|_| value.replace('T', " ").chars().take(19).collect())
}

fn sma(values: &[f64], n: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= n {
            sum -= values[i - n];
        }
        if i + 1 >= n {
            out[i] = Some(sum / n as f64);
        }
    }
    out
}

fn draw_kline(
    ui: &mut egui::Ui,
    rows: &[getrich_core::models::Bar],
    span: &mut usize,
    end: &mut usize,
    hover: Option<usize>,
) -> Option<usize> {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let rect = Rect::from_min_max(
        resp.rect.min + Vec2::new(66.0, 34.0),
        resp.rect.max - Vec2::new(18.0, 30.0),
    );
    if rows.is_empty() || rect.width() < 10.0 {
        return hover;
    }
    let n = rows.len();
    if *span == 0 {
        *span = n.clamp(1, 120);
    }
    *span = (*span).clamp(1, n);

    if resp.hovered() {
        let (ctrl, zoom, scroll) = ui.input(|i| {
            (
                i.modifiers.ctrl || i.modifiers.command,
                i.zoom_delta(),
                i.raw_scroll_delta.y,
            )
        });
        if ctrl {
            ui.ctx().input_mut(|i| {
                i.raw_scroll_delta.y = 0.0;
                i.smooth_scroll_delta.y = 0.0;
            });
            if (zoom - 1.0).abs() > 0.001 {
                let next = (*span as f32 / zoom).round() as usize;
                *span = next.clamp(8.min(n), n);
                ui.ctx().request_repaint();
            } else if scroll.abs() > 0.1 {
                if scroll > 0.0 {
                    *span = ((*span as f32) * 0.8).round() as usize;
                } else {
                    *span = ((*span as f32) * 1.25).round() as usize;
                }
                *span = (*span).clamp(8.min(n), n);
                ui.ctx().request_repaint();
            }
        }
    }

    let (start, finish, vis) = crate::chart_model::viewport(n, *span, *end);
    *span = vis;
    let all_rows = rows;
    let rows = &rows[start..finish];
    let closes: Vec<f64> = all_rows.iter().map(|b| b.close).collect();
    let ma5 = sma(&closes, 5)[start..finish].to_vec();
    let ma10 = sma(&closes, 10)[start..finish].to_vec();
    let ma20 = sma(&closes, 20)[start..finish].to_vec();
    let mut min = rows.iter().map(|b| b.low).fold(f64::MAX, f64::min);
    let mut max = rows.iter().map(|b| b.high).fold(f64::MIN, f64::max);
    for arr in [&ma5, &ma10, &ma20] {
        for v in arr.iter().flatten() {
            min = min.min(*v);
            max = max.max(*v);
        }
    }
    if (max - min).abs() < 1e-9 {
        min -= 1.0;
        max += 1.0;
    }
    let vol_h = rect.height() * 0.18;
    let plot = Rect::from_min_max(
        rect.left_top(),
        Pos2::new(rect.right(), rect.bottom() - vol_h - 8.0),
    );
    let vols: Vec<f64> = rows.iter().map(|b| b.volume.unwrap_or(0.0)).collect();
    let vmax = vols.iter().copied().fold(1.0_f64, f64::max);
    let vis_n = rows.len();
    let slots = vis_n.max(80) as f32;
    let slot = plot.width() / slots;
    let pad = slots - vis_n as f32;
    let y = |p: f64| {
        let t = (max - p) / (max - min);
        plot.top() + t as f32 * plot.height()
    };
    let x_at = |i: usize| plot.left() + slot * (pad + i as f32) + slot / 2.0;
    let hover = resp
        .hover_pos()
        .and_then(|pos| {
            if !plot.x_range().contains(pos.x) {
                return None;
            }
            crate::chart_model::hit(pos.x, plot.left(), slot, pad, rows.len())
        })
        .or(hover.filter(|_| resp.hovered()));
    let hi = hover.unwrap_or(rows.len() - 1);
    for tick in 0..=4 {
        let value = min + (max - min) * tick as f64 / 4.0;
        let yy = y(value);
        painter.line_segment(
            [Pos2::new(plot.left(), yy), Pos2::new(plot.right(), yy)],
            Stroke::new(0.5_f32, muted().gamma_multiply(0.25)),
        );
        painter.text(
            Pos2::new(plot.left() - 7.0, yy),
            egui::Align2::RIGHT_CENTER,
            format!("{value:.2}"),
            egui::FontId::proportional(11.0),
            muted(),
        );
    }
    for index in [0, rows.len() / 2, rows.len() - 1] {
        painter.text(
            Pos2::new(
                x_at(index).clamp(
                    plot.left() + 42.0_f32.min(plot.width() / 2.0),
                    plot.right() - 42.0_f32.min(plot.width() / 2.0),
                ),
                rect.bottom() + 10.0,
            ),
            egui::Align2::CENTER_TOP,
            rows[index].ts.chars().take(10).collect::<String>(),
            egui::FontId::proportional(11.0),
            muted(),
        );
    }
    painter.text(
        Pos2::new(plot.left() - 7.0, plot.bottom() + 12.0),
        egui::Align2::RIGHT_TOP,
        format!("量 {}", vol(Some(vmax))),
        egui::FontId::proportional(10.0),
        muted(),
    );
    for (i, b) in rows.iter().enumerate() {
        let x = x_at(i);
        let up = b.close >= b.open;
        let color = if up { up_color() } else { down_color() };
        painter.line_segment(
            [Pos2::new(x, y(b.high)), Pos2::new(x, y(b.low))],
            Stroke::new(1.0_f32, color),
        );
        let top = y(b.open.max(b.close));
        let bot = y(b.open.min(b.close));
        let w = (slot * 0.62).clamp(1.0, 8.0);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x - w / 2.0, top),
                Pos2::new(x + w / 2.0, bot.max(top + 1.0)),
            ),
            Rounding::ZERO,
            color,
        );
        let vh = ((vols[i] / vmax) as f32) * vol_h;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x - w / 2.0, rect.bottom() - vh),
                Pos2::new(x + w / 2.0, rect.bottom()),
            ),
            Rounding::ZERO,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90),
        );
    }
    let stroke_ma = |arr: &[Option<f64>], color: Color32| {
        let mut prev: Option<Pos2> = None;
        for (i, v) in arr.iter().enumerate() {
            let Some(v) = v else {
                prev = None;
                continue;
            };
            let p = Pos2::new(x_at(i), y(*v));
            if let Some(a) = prev {
                painter.line_segment([a, p], Stroke::new(1.2_f32, color));
            }
            prev = Some(p);
        }
    };
    stroke_ma(&ma5, muted());
    stroke_ma(&ma10, accent());
    stroke_ma(&ma20, Color32::from_rgb(191, 72, 0));
    if let Some(cur) = rows.get(hi) {
        let x = x_at(hi);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0_f32, muted().gamma_multiply(0.65)),
        );
        painter.text(
            Pos2::new(plot.left(), resp.rect.top() + 5.0),
            egui::Align2::LEFT_TOP,
            format!(
                "{}  开 {:.2}  高 {:.2}  低 {:.2}  收 {:.2}",
                cur.ts, cur.open, cur.high, cur.low, cur.close
            ),
            egui::FontId::proportional(12.0),
            muted(),
        );
    }
    hover.or(Some(hi))
}
