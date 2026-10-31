//! Aligned, compact controls for the fixed-width quote sidebar.
use super::*;

impl GetRichApp {
    pub(super) fn quote_filters(&mut self, ui: &mut egui::Ui) -> [Rect; 2] {
        let old_board = self.board.clone();
        let old_filter = self.change_filter.clone();
        let mut bounds = [Rect::NOTHING; 2];
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.columns(2, |cols| {
            for col in cols.iter_mut() {
                col.spacing_mut().interact_size.y = 32.0;
                col.spacing_mut().button_padding = Vec2::new(6.0, 6.0);
                col.spacing_mut().item_spacing.y = 5.0;
            }
            cols[0].label(egui::RichText::new("市场范围").size(11.0).color(muted()));
            let label = BOARDS
                .iter()
                .find(|(id, _)| *id == self.board)
                .map(|(_, label)| *label)
                .unwrap_or("全部");
            bounds[0] = egui::ComboBox::from_id_salt("quote-board")
                .width(cols[0].available_width())
                .selected_text(egui::RichText::new(label).size(12.0))
                .show_ui(&mut cols[0], |ui| {
                    for (id, label) in BOARDS {
                        ui.selectable_value(&mut self.board, (*id).into(), *label);
                    }
                })
                .response
                .rect;
            cols[1].label(egui::RichText::new("涨跌筛选").size(11.0).color(muted()));
            bounds[1] = egui::ComboBox::from_id_salt("change-filter")
                .width(cols[1].available_width())
                .selected_text(egui::RichText::new(filter_label(&self.change_filter)).size(12.0))
                .show_ui(&mut cols[1], |ui| {
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
                })
                .response
                .rect;
        });
        if old_board != self.board {
            self.industry.clear();
            self.change_filter.clear();
        }
        if old_board != self.board || old_filter != self.change_filter {
            self.reload_list();
        }
        bounds
    }
}

pub(super) fn table_heading(
    ui: &mut egui::Ui,
    title: &str,
    width: f32,
    numeric: bool,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, selected_surface());
    }
    let (anchor, align) = if numeric {
        (rect.right_center(), egui::Align2::RIGHT_CENTER)
    } else {
        (rect.left_center(), egui::Align2::LEFT_CENTER)
    };
    ui.painter().text(
        anchor,
        align,
        title,
        egui::FontId::proportional(11.0),
        if active { accent() } else { muted() },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_fields_share_edges_and_do_not_overflow_a_narrow_sidebar() {
        let (mut app, root) = super::super::interaction_tests::workspace();
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        for width in [180.0, 240.0, 320.0, 480.0] {
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(width + 16.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_width(width);
                            let left = ui.cursor().left();
                            let rects = app.quote_filters(ui);
                            assert!((rects[0].top() - rects[1].top()).abs() < 0.1);
                            assert!((rects[0].height() - rects[1].height()).abs() < 0.1);
                            assert!((rects[0].width() - rects[1].width()).abs() < 0.1);
                            assert!((rects[0].left() - left).abs() < 0.1);
                            assert!(
                                rects[1].right() <= left + width + 0.1,
                                "filters overflow {width}: {rects:?}"
                            );
                            assert!((rects[1].left() - rects[0].right() - 8.0).abs() < 0.1);
                        });
                    },
                );
            }
        }
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
