use super::*;
impl GetRichApp {
    pub(super) fn ui_empty_quotes(&mut self, ctx: &egui::Context) {
        page_canvas(ctx, "quotes-onboarding", 1160.0, |ui| {
            page_title(
                ui,
                "MARKET / 行情工作区",
                "市场行情",
                "从行情列表到个股研究，在一个工作区内完成。 ",
            );
            card(ui, 290.0, |ui| {
                let wide = ui.available_width() > 850.0;
                if wide {
                    ui.columns(2, |cols| {
                        self.quote_welcome(&mut cols[0]);
                        quote_steps(&mut cols[1]);
                    });
                } else {
                    self.quote_welcome(ui);
                    ui.add_space(24.0);
                    quote_steps(ui);
                }
            });
            ui.add_space(22.0);
            ui.columns(3, |cols| {
                for (col, (number, title, note)) in cols.iter_mut().zip([
                    (
                        "01",
                        "市场行情",
                        "按市场、行业与涨跌条件筛选，快速定位关注标的。",
                    ),
                    (
                        "02",
                        "个股研究",
                        "查看价格、量能与日周月 K 线，整理你的自选。",
                    ),
                    (
                        "03",
                        "大盘星图",
                        "以行业与个股面积观察市场分布，核对数据来源。",
                    ),
                ]) {
                    card(col, 150.0, |ui| {
                        ui.colored_label(accent(), egui::RichText::new(number).size(12.0));
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(title).size(18.0).strong());
                        ui.colored_label(muted(), egui::RichText::new(note).size(13.0));
                    });
                }
            });
            ui.add_space(20.0);
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    muted(),
                    egui::RichText::new("行情保存在本地 · 不填充模拟价格 · 来源与日期可追溯")
                        .size(12.0),
                );
            });
        });
    }
    fn quote_welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            egui::Frame::none()
                .fill(selected_surface())
                .rounding(6.0)
                .inner_margin(Vec2::new(10.0, 5.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("开始使用 GETRICH")
                            .color(accent())
                            .size(11.0),
                    );
                });
        });
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("建立你的行情工作区")
                .size(29.0)
                .strong(),
        );
        ui.add_space(6.0);
        ui.colored_label(
            muted(),
            "当前尚未导入行情。连接数据源或导入本地文件后，即可查看股票、K 线和行业星图。",
        );
        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            if action(ui, "连接数据源", true).clicked() {
                self.settings_tab = 0;
                self.read_settings();
                self.screen = Screen::Settings;
            }
            if action(ui, "导入行情文件", false).clicked() {
                self.screen = Screen::Data;
                self.reload_stats();
            }
        });
        ui.add_space(6.0);
        ui.colored_label(
            muted(),
            egui::RichText::new("想先了解功能？在数据中心选择演示文件并预览。").size(12.0),
        );
    }
    pub(super) fn detail_placeholder(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height().max(280.0);
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), height),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space((height * 0.28).max(30.0));
                empty_chart(ui);
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new(if self.detail_loading {
                        "正在载入个股资料"
                    } else {
                        "选择一只股票，开始研究"
                    })
                    .size(22.0)
                    .strong(),
                );
                ui.colored_label(muted(), "点击左侧行情，或使用名称、代码搜索。");
                ui.colored_label(
                    muted(),
                    egui::RichText::new("价格摘要 / 量价图表 / 日周月 K 线 / 数据来源").size(12.0),
                );
            },
        );
    }
}
fn quote_steps(ui: &mut egui::Ui) {
    ui.add_space(12.0);
    for (number, title, note) in [
        (
            "1",
            "配置行情来源",
            "在设置中保存数据源，或准备本地行情文件。",
        ),
        (
            "2",
            "同步与校验",
            "在数据中心同步报价，检查任务结果与来源日期。",
        ),
        (
            "3",
            "开始市场研究",
            "浏览行情、打开个股详情，或进入行业大盘星图。",
        ),
    ] {
        egui::Frame::none()
            .fill(secondary_surface())
            .rounding(8.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 15.0, selected_surface());
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        number,
                        egui::FontId::proportional(13.0),
                        accent(),
                    );
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(title).size(14.0).strong());
                        ui.colored_label(muted(), egui::RichText::new(note).size(12.0));
                    });
                });
            });
    }
}
fn empty_chart(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(170.0, 100.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10.0, secondary_surface());
    for y in [25.0, 50.0, 75.0] {
        painter.line_segment(
            [
                rect.min + Vec2::new(16.0, y),
                rect.min + Vec2::new(154.0, y),
            ],
            Stroke::new(1.0_f32, super::design::border()),
        );
    }
    for (index, (top, bottom)) in [
        (58.0, 80.0),
        (46.0, 64.0),
        (49.0, 70.0),
        (31.0, 53.0),
        (20.0, 41.0),
    ]
    .iter()
    .enumerate()
    {
        let x = rect.left() + 30.0 + index as f32 * 27.0;
        painter.line_segment(
            [
                Pos2::new(x, rect.top() + top - 7.0),
                Pos2::new(x, rect.top() + bottom + 7.0),
            ],
            Stroke::new(1.0_f32, accent().gamma_multiply(0.5)),
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x - 5.0, rect.top() + top),
                Pos2::new(x + 5.0, rect.top() + bottom),
            ),
            2.0,
            accent().gamma_multiply(0.35),
        );
    }
}
