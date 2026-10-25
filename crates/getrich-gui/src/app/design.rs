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
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            muted(),
            egui::RichText::new(format!(
                "报价 {} · {} · {}",
                q.trade_date.as_deref().unwrap_or("—"),
                q.data_status,
                q.source.as_deref().unwrap_or("来源未标明")
            ))
            .size(11.0),
        );
        if q.source_kind == "daily" {
            ui.colored_label(
                Color32::from_rgb(202, 164, 95),
                egui::RichText::new("日线 / 日期精度").size(10.0),
            );
        }
    });
    ui.add_space(5.0);
    let fields = [
        ("今开", fmt(q.open)),
        ("最高", fmt(q.high)),
        ("最低", fmt(q.low)),
        ("昨收", fmt(q.prev_close)),
        ("成交量", vol(q.volume)),
        ("成交额", amount(q.turnover, q.currency.as_deref())),
        ("振幅", pct(q.amplitude)),
        (
            "行业",
            detail
                .listing
                .industry
                .clone()
                .unwrap_or_else(|| "未分类".into()),
        ),
    ];
    let columns = if ui.available_width() > 1050.0 { 8 } else { 4 };
    for row in fields.chunks(columns) {
        ui.columns(columns, |cols| {
            for ((label, value), col) in row.iter().zip(cols) {
                col.colored_label(muted(), egui::RichText::new(*label).size(12.0));
                col.label(egui::RichText::new(value).size(14.0));
            }
        });
        ui.add_space(5.0);
    }
    ui.separator();
}
