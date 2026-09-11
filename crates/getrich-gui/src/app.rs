//! Quote board in egui: Apple-minimal chrome, Tonghuashun data columns.

use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use getrich_core::models::Timeframe;
use getrich_core::service::App;
use getrich_core::view::{BarPage, QuoteBoardPage, QuoteRow, StockDetail, StoreStats};
use std::sync::Arc;

const UP: Color32 = Color32::from_rgb(225, 29, 47);
const DOWN: Color32 = Color32::from_rgb(18, 138, 75);
const FLAT: Color32 = Color32::from_rgb(110, 110, 115);
const ACCENT: Color32 = Color32::from_rgb(0, 113, 227);
const TEXT: Color32 = Color32::from_rgb(29, 29, 31);
const BG: Color32 = Color32::from_rgb(245, 245, 247);
const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);

const BOARDS: &[(&str, &str)] = &[
    ("ALL", "全部"),
    ("CN", "沪深京"),
    ("CN.SH", "沪市"),
    ("CN.SZ", "深市"),
    ("CHINEXT", "创业板"),
    ("STAR", "科创板"),
    ("CN.BJ", "北证"),
    ("HK", "港股"),
    ("US", "美股"),
    ("WATCH", "自选"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Quotes,
    Detail,
}

pub struct GetRichApp {
    core: Arc<App>,
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
    taskbar_icon_applied: bool,
    taskbar_icon_attempts: u8,
}

impl GetRichApp {
    pub fn new(core: Arc<App>) -> Self {
        let stats = core.stats().ok();
        Self {
            core,
            screen: Screen::Home,
            board: "ALL".into(),
            query: String::new(),
            sort: "symbol".into(),
            page: None,
            stats,
            error: None,
            detail: None,
            bars: None,
            tf: Timeframe::D1,
            selected: None,
            taskbar_icon_applied: false,
            taskbar_icon_attempts: 0,
        }
    }

    fn reload_stats(&mut self) {
        match self.core.stats() {
            Ok(s) => {
                self.stats = Some(s);
                if self.screen == Screen::Home {
                    self.error = None;
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn reload_list(&mut self) {
        match self.core.quote_board(
            Some(&self.board),
            if self.query.trim().is_empty() {
                None
            } else {
                Some(self.query.as_str())
            },
            Some(&self.sort),
            400,
            0,
        ) {
            Ok(page) => {
                self.stats = Some(page.stats.clone());
                self.page = Some(page);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn open_quotes(&mut self) {
        self.screen = Screen::Quotes;
        self.detail = None;
        self.bars = None;
        self.reload_list();
    }

    fn open_row(&mut self, row: &QuoteRow) {
        match self.core.parse_key(&row.symbol, Some(&row.market)) {
            Ok(key) => match self.core.show_stock(&key) {
                Ok(detail) => {
                    self.bars = self.core.list_bars(&key, self.tf, None, None, 500).ok();
                    self.detail = Some(detail);
                    self.screen = Screen::Detail;
                    self.error = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

impl eframe::App for GetRichApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.taskbar_icon_applied && self.taskbar_icon_attempts < 10 {
            self.taskbar_icon_attempts += 1;
            self.taskbar_icon_applied = crate::windows_icon::apply_embedded_icon();
            if !self.taskbar_icon_applied {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }

        let mut visuals = egui::Visuals::light();
        visuals.window_fill = BG;
        visuals.panel_fill = BG;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.inactive.bg_fill = SURFACE;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(236, 236, 239);
        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0, 113, 227, 40);
        ctx.set_visuals(visuals);

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("GetRich").size(18.0).strong().color(TEXT));
                ui.add_space(16.0);
                if ui
                    .selectable_label(self.screen == Screen::Home, "主页")
                    .clicked()
                {
                    self.screen = Screen::Home;
                    self.detail = None;
                    self.reload_stats();
                }
                if ui
                    .selectable_label(self.screen != Screen::Home, "行情")
                    .clicked()
                {
                    self.open_quotes();
                }
                if self.screen == Screen::Quotes {
                    ui.separator();
                    ui.label(egui::RichText::new("搜索").color(FLAT).small());
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .desired_width(180.0)
                            .hint_text("名称 / 代码"),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.reload_list();
                    }
                    if ui.button("刷新").clicked() {
                        self.reload_list();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    let status = self
                        .stats
                        .as_ref()
                        .map(|s| if s.empty { "空库" } else { "已连接" })
                        .unwrap_or("—");
                    ui.label(egui::RichText::new(status).color(FLAT).small());
                });
            });
            if self.screen == Screen::Quotes {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    for (id, label) in BOARDS {
                        let on = self.board == *id;
                        if ui.selectable_label(on, *label).clicked() {
                            self.board = (*id).into();
                            self.reload_list();
                        }
                    }
                });
            }
            if let Some(err) = self.error.clone() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.colored_label(UP, format!("无法完成操作：{err}"));
                    if ui.button("重试").clicked() {
                        match self.screen {
                            Screen::Home => self.reload_stats(),
                            Screen::Quotes => self.reload_list(),
                            Screen::Detail => {
                                if let Some(d) = self.detail.clone() {
                                    self.open_row(&d.quote);
                                }
                            }
                        }
                    }
                });
            }
            ui.add_space(8.0);
        });

        match self.screen {
            Screen::Home => self.ui_home(ctx),
            Screen::Quotes => self.ui_list(ctx),
            Screen::Detail => self.ui_detail(ctx),
        }
    }
}

impl GetRichApp {
    fn ui_home(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(72.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("行情工作站").color(FLAT).small());
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("看盘，从这里开始。")
                        .size(36.0)
                        .strong()
                        .color(TEXT),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("报价、明细与 K 线。数据由你导入，软件只负责呈现。")
                        .size(16.0)
                        .color(FLAT),
                );
                ui.add_space(28.0);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("打开行情").color(Color32::WHITE))
                            .fill(ACCENT)
                            .min_size(Vec2::new(140.0, 36.0)),
                    )
                    .clicked()
                {
                    self.open_quotes();
                }
            });
            ui.add_space(48.0);
            ui.horizontal_centered(|ui| {
                let (listings, quotes, bars, note) = match &self.stats {
                    Some(s) => (
                        s.listings.to_string(),
                        s.quotes.to_string(),
                        s.bars.to_string(),
                        if s.empty {
                            s.message.clone()
                        } else {
                            String::new()
                        },
                    ),
                    None => ("—".into(), "—".into(), "—".into(), String::new()),
                };
                stat_card(ui, "标的", &listings);
                ui.add_space(12.0);
                stat_card(ui, "报价", &quotes);
                ui.add_space(12.0);
                stat_card(ui, "日线", &bars);
                if !note.is_empty() {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(note).color(FLAT).small());
                }
            });
        });
    }

    fn ui_list(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(page) = self.page.clone() else {
                ui.add_space(48.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("正在载入").size(18.0).strong());
                    ui.label(egui::RichText::new("读取报价列表。").color(FLAT));
                });
                return;
            };
            if page.empty {
                ui.add_space(64.0);
                ui.vertical_centered(|ui| {
                    let title = if page.empty_kind == "no_match" {
                        "无匹配结果"
                    } else {
                        "暂无行情数据"
                    };
                    ui.label(egui::RichText::new(title).size(22.0).strong());
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&page.empty_message).color(FLAT));
                });
                return;
            }
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("quotes")
                    .striped(true)
                    .min_col_width(72.0)
                    .show(ui, |ui| {
                        header_cell(ui, "代码");
                        header_cell(ui, "名称");
                        header_cell(ui, "最新价");
                        header_cell(ui, "涨跌幅");
                        header_cell(ui, "涨跌额");
                        header_cell(ui, "今开");
                        header_cell(ui, "最高");
                        header_cell(ui, "最低");
                        header_cell(ui, "昨收");
                        header_cell(ui, "振幅");
                        header_cell(ui, "成交量");
                        header_cell(ui, "成交额");
                        header_cell(ui, "市场");
                        ui.end_row();
                        for (i, row) in page.rows.iter().enumerate() {
                            let dir = dir_color(&row.direction);
                            let resp = ui.selectable_label(self.selected == Some(i), &row.symbol);
                            if resp.clicked() {
                                self.selected = Some(i);
                                self.open_row(row);
                            }
                            ui.label(row.name.clone().unwrap_or_else(|| "—".into()));
                            colored(ui, fmt(row.last), dir);
                            colored(ui, pct(row.change_pct), dir);
                            colored(ui, signed(row.change), dir);
                            ui.label(fmt(row.open));
                            colored(ui, fmt(row.high), dir);
                            colored(ui, fmt(row.low), dir);
                            ui.label(fmt(row.prev_close));
                            ui.label(pct(row.amplitude));
                            ui.label(vol(row.volume));
                            ui.label(vol(row.turnover));
                            ui.label(row.board_label.clone());
                            ui.end_row();
                        }
                    });
            });
        });
    }

    fn ui_detail(&mut self, ctx: &egui::Context) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let q = &detail.quote;
        let dir = dir_color(&q.direction);
        egui::TopBottomPanel::top("detail-head").frame(egui::Frame::none().fill(BG)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui.link("行情").clicked() {
                    self.open_quotes();
                }
                ui.heading(q.name.clone().unwrap_or_else(|| q.symbol.clone()));
                ui.label(egui::RichText::new(&q.symbol).color(FLAT));
                ui.label(egui::RichText::new(&q.board_label).color(FLAT).small());
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(fmt(q.last)).size(36.0).color(dir));
                ui.label(egui::RichText::new(signed(q.change)).size(18.0).color(dir));
                ui.label(egui::RichText::new(pct(q.change_pct)).size(18.0).color(dir));
            });
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                kv(ui, "今开", fmt(q.open));
                kv(ui, "最高", fmt(q.high));
                kv(ui, "最低", fmt(q.low));
                kv(ui, "昨收", fmt(q.prev_close));
                kv(ui, "成交量", vol(q.volume));
                kv(ui, "成交额", vol(q.turnover));
                kv(ui, "振幅", pct(q.amplitude));
            });
            if detail.empty.no_quote {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("暂无报价。导入 quotes 后显示最新价。").color(FLAT));
                });
            }
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                for (tf, label) in [
                    (Timeframe::D1, "日K"),
                    (Timeframe::W1, "周K"),
                    (Timeframe::Mo1, "月K"),
                ] {
                    if ui.selectable_label(self.tf == tf, label).clicked() {
                        self.tf = tf;
                        if let Ok(key) = self.core.parse_key(&q.symbol, Some(&q.market)) {
                            self.bars = self.core.list_bars(&key, self.tf, None, None, 500).ok();
                        }
                    }
                }
            });
            ui.add_space(8.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let bars = self.bars.as_ref();
            if bars.map(|b| b.empty).unwrap_or(true) {
                ui.vertical_centered(|ui| {
                    ui.add_space(48.0);
                    ui.label(egui::RichText::new("暂无 K 线").size(22.0).strong());
                    if let Some(b) = bars {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(&b.empty_message).color(FLAT));
                    }
                });
                return;
            }
            let rows = &bars.unwrap().rows;
            draw_kline(ui, rows);
        });
    }
}

fn stat_card(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::none()
        .fill(SURFACE)
        .rounding(Rounding::same(14.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_width(120.0);
            ui.label(egui::RichText::new(label).color(FLAT).small());
            ui.label(egui::RichText::new(value).size(26.0).strong());
        });
}

fn header_cell(ui: &mut egui::Ui, text: &str) {
    ui.strong(text);
}

fn colored(ui: &mut egui::Ui, text: String, color: Color32) {
    ui.label(egui::RichText::new(text).color(color));
}

fn kv(ui: &mut egui::Ui, k: &str, v: String) {
    ui.label(egui::RichText::new(k).color(FLAT).small());
    ui.monospace(v);
    ui.add_space(10.0);
}

fn dir_color(dir: &str) -> Color32 {
    match dir {
        "up" => UP,
        "down" => DOWN,
        _ => FLAT,
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

fn draw_kline(ui: &mut egui::Ui, rows: &[getrich_core::models::Bar]) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let rect = resp.rect.shrink2(Vec2::new(48.0, 16.0));
    if rows.is_empty() || rect.width() < 10.0 {
        return;
    }
    painter.rect_filled(rect, Rounding::same(12.0), SURFACE);
    let mut min = rows.iter().map(|b| b.low).fold(f64::MAX, f64::min);
    let mut max = rows.iter().map(|b| b.high).fold(f64::MIN, f64::max);
    if (max - min).abs() < 1e-9 {
        min -= 1.0;
        max += 1.0;
    }
    let n = rows.len() as f32;
    let slot = rect.width() / n;
    let y = |p: f64| {
        let t = (max - p) / (max - min);
        rect.top() + t as f32 * rect.height() * 0.78
    };
    painter.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.0, Color32::from_rgb(230, 230, 232)));
    for (i, b) in rows.iter().enumerate() {
        let x = rect.left() + slot * i as f32 + slot / 2.0;
        let up = b.close >= b.open;
        let color = if up { UP } else { DOWN };
        painter.line_segment(
            [Pos2::new(x, y(b.high)), Pos2::new(x, y(b.low))],
            Stroke::new(1.0, color),
        );
        let top = y(b.open.max(b.close));
        let bot = y(b.open.min(b.close));
        let w = (slot * 0.7).max(1.0);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x - w / 2.0, top), Pos2::new(x + w / 2.0, bot.max(top + 1.0))),
            Rounding::ZERO,
            color,
        );
    }
    if let Some(last) = rows.last() {
        painter.text(
            rect.left_top() + Vec2::new(12.0, 10.0),
            egui::Align2::LEFT_TOP,
            format!(
                "{}  开 {:.2}  高 {:.2}  低 {:.2}  收 {:.2}",
                last.ts, last.open, last.high, last.low, last.close
            ),
            egui::FontId::proportional(12.0),
            FLAT,
        );
    }
}
