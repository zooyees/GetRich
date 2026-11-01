use super::*;

pub(super) fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(surface())
        .stroke(Stroke::new(1.0_f32, border()))
        .rounding(12.0)
        .inner_margin(24.0)
}
pub(super) fn secondary_surface() -> Color32 {
    if background().r() < 100 {
        Color32::from_rgb(25, 35, 49)
    } else {
        Color32::from_rgb(242, 246, 251)
    }
}
pub(super) fn selected_surface() -> Color32 {
    if background().r() < 100 {
        Color32::from_rgb(32, 52, 83)
    } else {
        Color32::from_rgb(228, 238, 255)
    }
}
pub(super) fn page_canvas(
    ctx: &egui::Context,
    id: &str,
    max_width: f32,
    content: impl FnOnce(&mut egui::Ui),
) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(background())
                .inner_margin(Margin::symmetric(28.0, 28.0)),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let available = ui.available_width();
                    let width = available.min(max_width);
                    ui.horizontal_top(|ui| {
                        ui.add_space(((available - width) / 2.0).max(0.0));
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(width);
                                content(ui);
                            },
                        );
                    });
                });
        });
}
pub(super) fn card(ui: &mut egui::Ui, min_height: f32, content: impl FnOnce(&mut egui::Ui)) {
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(min_height);
        content(ui);
    });
}
pub(super) fn paired_cards(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui, &mut egui::Ui)) {
    ui.columns(2, |cols| {
        let (left, right) = cols.split_at_mut(1);
        let mut a = card_frame().begin(&mut left[0]);
        let mut b = card_frame().begin(&mut right[0]);
        a.content_ui.set_width(a.content_ui.available_width());
        b.content_ui.set_width(b.content_ui.available_width());
        content(&mut a.content_ui, &mut b.content_ui);
        let height = a
            .content_ui
            .min_rect()
            .height()
            .max(b.content_ui.min_rect().height());
        for panel in [&mut a.content_ui, &mut b.content_ui] {
            let rect = Rect::from_min_size(
                panel.min_rect().min,
                Vec2::new(panel.min_rect().width(), height),
            );
            panel.expand_to_include_rect(rect);
        }
        a.end(&mut left[0]);
        b.end(&mut right[0]);
    });
}
pub(super) fn card_title(ui: &mut egui::Ui, title: &str, note: &str) {
    ui.label(egui::RichText::new(title).size(19.0).strong());
    if !note.is_empty() {
        ui.colored_label(muted(), egui::RichText::new(note).size(13.0));
    }
    ui.add_space(16.0);
}
pub(super) fn field_label(ui: &mut egui::Ui, label: &str) {
    ui.colored_label(muted(), egui::RichText::new(label).size(13.0));
}
pub(super) fn text_field(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    password: bool,
) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 38.0],
        egui::TextEdit::singleline(value)
            .password(password)
            .hint_text(hint)
            .margin(Vec2::new(12.0, 10.0)),
    )
}
pub(super) fn action(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let button = if primary {
        primary_btn(label)
    } else {
        egui::Button::new(label)
    };
    ui.add_sized([0.0, 36.0], button)
}
pub(super) fn status_badge(ui: &mut egui::Ui, label: &str, good: bool) {
    let color = if good {
        Color32::from_rgb(83, 190, 152)
    } else {
        Color32::from_rgb(205, 167, 103)
    };
    egui::Frame::none()
        .fill(color.gamma_multiply(0.12))
        .rounding(5.0)
        .inner_margin(Vec2::new(9.0, 4.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(11.0).color(color));
        });
}
pub(super) fn border() -> Color32 {
    if background().r() < 100 {
        Color32::from_rgb(38, 52, 74)
    } else {
        Color32::from_rgb(224, 230, 239)
    }
}
pub(super) fn page_title(ui: &mut egui::Ui, eyebrow: &str, title: &str, note: &str) {
    ui.colored_label(muted(), egui::RichText::new(eyebrow).size(11.0));
    ui.add_space(1.0);
    ui.label(egui::RichText::new(title).size(28.0).strong());
    ui.colored_label(muted(), egui::RichText::new(note).size(13.0));
    ui.add_space(22.0);
}
pub(super) fn draw_quote_summary(ui: &mut egui::Ui, detail: &StockDetail) {
    let q = &detail.quote;
    let fields = [
        ("今开", fmt(q.open), q.open),
        ("最高", fmt(q.high), q.high),
        ("最低", fmt(q.low), q.low),
        ("昨收", fmt(q.prev_close), None),
        ("成交量", vol(q.volume), None),
        ("成交额", amount(q.turnover, q.currency.as_deref()), None),
        ("振幅", pct(q.amplitude), None),
        (
            "行业",
            detail
                .listing
                .industry
                .clone()
                .unwrap_or_else(|| "未分类".into()),
            None,
        ),
    ];
    egui::Frame::none()
        .inner_margin(Vec2::new(0.0, 3.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 2.0);
            ui.spacing_mut().interact_size.y = 22.0;
            let columns = if ui.available_width() >= 1300.0 {
                8
            } else if ui.available_width() >= 640.0 {
                4
            } else {
                2
            };
            let width = (ui.available_width() - 12.0 * (columns - 1) as f32) / columns as f32;
            for row in fields.chunks(columns) {
                ui.horizontal(|ui| {
                    for (label, value, price) in row {
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 22.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_width(width);
                                ui.set_min_height(22.0);
                                ui.colored_label(muted(), egui::RichText::new(*label).size(12.0));
                                let color = match price.zip(q.prev_close) {
                                    Some((p, c)) if p > c => up_color(),
                                    Some((p, c)) if p < c => down_color(),
                                    _ => text_color(),
                                };
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(value).size(13.0).color(color),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(value);
                            },
                        );
                    }
                });
            }
        });
}
