//! Stock-detail typography and spacing. Shared by all research subpages.
use super::*;

pub(super) fn install(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    for (kind, size) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 13.0),
        (egui::TextStyle::Small, 12.0),
        (egui::TextStyle::Heading, 17.0),
    ] {
        style
            .text_styles
            .insert(kind, egui::FontId::proportional(size));
    }
    style.spacing.item_spacing = Vec2::new(10.0, 4.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.interact_size.y = 32.0;
}
pub(super) fn frame() -> egui::Frame {
    egui::Frame::none()
        .fill(surface())
        .stroke(Stroke::new(1.0_f32, design::border()))
        .rounding(8.0)
        .inner_margin(16.0)
}
pub(super) fn heading(ui: &mut egui::Ui, title: &str, note: &str) {
    ui.label(egui::RichText::new(title).size(16.0).strong());
    if !note.is_empty() {
        caption(ui, note);
    }
    ui.add_space(6.0);
}
pub(super) fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).size(12.0).color(muted()));
}
pub(super) fn section(
    ui: &mut egui::Ui,
    title: &str,
    note: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, title, note);
        body(ui);
    });
    ui.add_space(6.0);
}
pub(super) fn metrics(ui: &mut egui::Ui, fields: &[(&str, String)]) {
    let cols = if ui.available_width() >= 760.0 { 4 } else { 2 };
    for row in fields.chunks(cols) {
        ui.columns(cols, |columns| {
            for ((label, value), col) in row.iter().zip(columns) {
                col.spacing_mut().item_spacing.y = 4.0;
                col.add(
                    egui::Label::new(egui::RichText::new(*label).size(12.0).color(muted()))
                        .truncate(),
                )
                .on_hover_text(*label);
                col.add(egui::Label::new(egui::RichText::new(value).size(16.0)).truncate())
                    .on_hover_text(value);
                col.add_space(6.0);
            }
        });
    }
}
pub(super) fn properties(ui: &mut egui::Ui, id: &str, rows: &[(&str, String)]) {
    let width = (ui.available_width() - 106.0).max(80.0);
    // These read-only columns have explicit widths; resized frames need fresh column positions.
    egui::Grid::new((id, ui.available_width().round() as u32))
        .num_columns(2)
        .min_row_height(24.0)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.allocate_ui_with_layout(
                    Vec2::new(90.0, 24.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(90.0);
                        ui.label(egui::RichText::new(*label).size(12.0).color(muted()));
                    },
                );
                ui.allocate_ui_with_layout(
                    Vec2::new(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.add(egui::Label::new(value).wrap());
                    },
                );
                ui.end_row();
            }
        });
}
pub(super) fn cell(
    ui: &mut egui::Ui,
    width: f32,
    text: impl Into<String>,
    numeric: bool,
    color: Color32,
) {
    let text = text.into();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &text));
    let mut job =
        egui::text::LayoutJob::simple(text.clone(), egui::FontId::proportional(13.0), color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let x = if numeric {
        rect.right() - galley.size().x
    } else {
        rect.left()
    };
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter_at(rect).galley(Pos2::new(x, y), galley, color);
    response.on_hover_text(text);
}
pub(super) fn date(raw: &str) -> String {
    if raw.len() == 8 && raw.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &raw[..4], &raw[4..6], &raw[6..])
    } else {
        raw.into()
    }
}
