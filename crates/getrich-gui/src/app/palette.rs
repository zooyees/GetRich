use super::*;
use std::cell::Cell;
thread_local! {static PALETTE:Cell<(bool,bool)>=const {Cell::new((false,false))};}
pub(super) fn background() -> Color32 {
    if PALETTE.with(|v| v.get().0) {
        Color32::from_rgb(12, 17, 25)
    } else {
        Color32::from_rgb(245, 247, 250)
    }
}
pub(super) fn surface() -> Color32 {
    if PALETTE.with(|v| v.get().0) {
        Color32::from_rgb(20, 28, 40)
    } else {
        Color32::WHITE
    }
}
pub(super) fn text_color() -> Color32 {
    if PALETTE.with(|v| v.get().0) {
        Color32::from_rgb(230, 237, 246)
    } else {
        Color32::from_rgb(29, 38, 54)
    }
}
pub(super) fn muted() -> Color32 {
    if PALETTE.with(|v| v.get().0) {
        Color32::from_rgb(145, 160, 180)
    } else {
        Color32::from_rgb(103, 116, 136)
    }
}
pub(super) fn up_color() -> Color32 {
    if PALETTE.with(|v| v.get().1) {
        Color32::from_rgb(19, 166, 108)
    } else {
        Color32::from_rgb(231, 67, 86)
    }
}
pub(super) fn down_color() -> Color32 {
    if PALETTE.with(|v| v.get().1) {
        Color32::from_rgb(231, 67, 86)
    } else {
        Color32::from_rgb(19, 166, 108)
    }
}
pub(super) fn accent() -> Color32 {
    Color32::from_rgb(68, 132, 236)
}
impl GetRichApp {
    pub(super) fn apply_preferences(&mut self, ctx: &egui::Context) {
        let dark = match self.preferences.theme.as_str() {
            "dark" => true,
            "system" => ctx.system_theme() == Some(egui::Theme::Dark),
            _ => false,
        };
        let green = self.preferences.colors == "green_up";
        PALETTE.with(|v| v.set((dark, green)));
        let signature = format!("{dark}:{green}");
        if signature != self.theme_signature {
            self.theme_signature = signature;
            let mut visuals = if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            visuals.window_fill = background();
            visuals.panel_fill = background();
            visuals.extreme_bg_color = background();
            visuals.faint_bg_color = surface();
            visuals.widgets.inactive.weak_bg_fill = surface();
            visuals.widgets.inactive.bg_fill = if dark {
                Color32::from_rgb(43, 57, 77)
            } else {
                Color32::from_rgb(220, 228, 239)
            };
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, super::design::border());
            visuals.widgets.hovered.bg_fill = if dark {
                Color32::from_rgb(33, 56, 88)
            } else {
                Color32::from_rgb(237, 243, 255)
            };
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent());
            visuals.widgets.hovered.weak_bg_fill = selected_surface();
            visuals.widgets.active.weak_bg_fill = selected_surface();
            visuals.widgets.open.weak_bg_fill = selected_surface();
            visuals.selection.bg_fill = if dark {
                Color32::from_rgb(33, 56, 88)
            } else {
                Color32::from_rgb(213, 230, 255)
            };
            visuals.override_text_color = Some(text_color());
            visuals.widgets.noninteractive.bg_stroke =
                Stroke::new(1.0_f32, super::design::border());
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, muted());
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, text_color());
            visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, text_color());
            visuals.widgets.inactive.rounding = Rounding::same(6.0);
            visuals.widgets.hovered.rounding = Rounding::same(6.0);
            visuals.widgets.active.rounding = Rounding::same(6.0);
            visuals.widgets.open.rounding = Rounding::same(6.0);
            visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(170, 202, 255));
            visuals.slider_trailing_fill = true;
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            ctx.set_visuals(visuals);
            Self::install_theme(ctx);
        }
    }
}
