use super::*;
use getrich_core::market_map::MarketMap;

fn summary(data: &MarketMap) -> String {
    let mode = if data.historical {
        "历史截面"
    } else {
        match data.quote_mode.as_str() {
            "realtime" => "实时快照",
            "mixed" => "混合来源",
            _ => "收盘行情",
        }
    };
    let mut weight = String::new();
    if data.coverage.lagged_weight > 0 {
        let from = data.weight_date_from.as_deref().unwrap_or("—");
        let to = data.weight_date_to.as_deref().unwrap_or(from);
        weight = if from == to {
            format!(" · 收盘权重 {from}")
        } else {
            format!(" · 收盘权重 {from}～{to}")
        };
    }
    format!(
        "{mode} · {} | 已导入 {} · 同日有效 {} · 展示 {} | 面积 · {} {}{weight}",
        data.trade_date.as_deref().unwrap_or("暂无报价"),
        data.coverage.universe,
        data.coverage.quoted,
        data.coverage.displayed,
        data.metric_label,
        if data.metric == "equal" {
            ""
        } else {
            &data.currency
        }
    )
}
impl GetRichApp {
    pub(super) fn map_footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("market-footer")
            .exact_height(54.0)
            .frame(
                egui::Frame::none()
                    .fill(surface())
                    .inner_margin(Margin::symmetric(10.0, 5.0))
                    .stroke(Stroke::new(1.0_f32, design::border())),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(5.0, 2.0);
                ui.spacing_mut().interact_size.y = 18.0;
                ui.columns(2, |cols| {
                    cols[0].horizontal(|ui| {
                        for n in [-5, -3, -1, 0, 1, 3, 5] {
                            let rgb = getrich_core::market_map::tile_rgb(
                                Some(f64::from(n)),
                                self.preferences.colors == "green_up",
                            );
                            egui::Frame::none()
                                .fill(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
                                .inner_margin(Vec2::new(6.0, 1.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{n:+}%"))
                                            .size(10.0)
                                            .color(Color32::WHITE),
                                    );
                                });
                        }
                    });
                    if let Some(data) = &self.market.data {
                        cols[0].horizontal(|ui| {
                            ui.colored_label(
                                up_color(),
                                egui::RichText::new(format!("涨 {}", data.coverage.up)).size(11.0),
                            );
                            ui.colored_label(
                                down_color(),
                                egui::RichText::new(format!("跌 {}", data.coverage.down))
                                    .size(11.0),
                            );
                            ui.colored_label(
                                muted(),
                                egui::RichText::new(format!(
                                    "平 {} · 未知 {}",
                                    data.coverage.flat, data.coverage.unknown_change
                                ))
                                .size(11.0),
                            );
                        });
                    }
                    let response = cols[1].vertical(|ui| {
                        if let Some(message) = self
                            .realtime_warning()
                            .filter(|_| !self.market.sync_pending)
                        {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(message)
                                        .size(11.0)
                                        .color(Color32::from_rgb(219, 166, 78)),
                                )
                                .truncate(),
                            )
                            .on_hover_text(self.realtime_hint());
                        } else {
                            self.map_sync_status(ui);
                        }
                        let text = self
                            .market
                            .data
                            .as_deref()
                            .map(summary)
                            .unwrap_or_else(|| "正在读取市场截面…".into());
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), 18.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&text).size(11.0).color(muted()),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&text);
                            },
                        );
                    });
                    self.market.status_rect = Some(response.response.rect);
                });
                self.market.footer_rect = Some(ui.max_rect());
            });
    }
}
