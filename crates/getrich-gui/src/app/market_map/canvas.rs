//! Cache stable geometry and glyphs; pointer movement paints only the outline/tooltip.
use super::*;

pub(super) struct CanvasCache {
    data: Arc<MarketMap>,
    rect: Rect,
    zoom: f32,
    pan: Vec2,
    search: String,
    watch: bool,
    green: bool,
    scale: f32,
    mesh: egui::epaint::Mesh,
    text: Vec<(Rect, egui::Shape)>,
    marks: Vec<Rect>,
    pub tiles: Vec<(Rect, usize, usize)>,
    pub headers: Vec<(Rect, usize)>,
}
impl CanvasCache {
    #[allow(clippy::too_many_arguments)]
    pub fn matches(
        &self,
        data: &Arc<MarketMap>,
        rect: Rect,
        zoom: f32,
        pan: Vec2,
        search: &str,
        watch: bool,
        green: bool,
        scale: f32,
    ) -> bool {
        Arc::ptr_eq(&self.data, data)
            && self.rect == rect
            && self.zoom == zoom
            && self.pan == pan
            && self.search == search
            && self.watch == watch
            && self.green == green
            && self.scale == scale
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data: Arc<MarketMap>,
        painter: &egui::Painter,
        rect: Rect,
        zoom: f32,
        pan: Vec2,
        search: &str,
        watch: bool,
        green: bool,
    ) -> Self {
        let mut mesh = egui::epaint::Mesh::default();
        let mut text = Vec::new();
        let mut marks = Vec::new();
        let mut tiles = Vec::new();
        let mut headers = Vec::new();
        for (gi, group) in data.groups.iter().enumerate() {
            let g = project(rect, group.bounds, zoom, pan);
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
                mesh.add_colored_rect(head.shrink(0.5), Color32::from_rgb(29, 40, 56));
                headers.push((head, gi));
                add_text(
                    painter,
                    &mut text,
                    head.intersect(rect).shrink(4.0),
                    head.left_center() + Vec2::new(7.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &group.name,
                    11.0,
                    Color32::from_rgb(220, 231, 247),
                );
                if head.width() > 170.0 {
                    add_text(
                        painter,
                        &mut text,
                        head.intersect(rect).shrink(4.0),
                        head.right_center() - Vec2::new(7.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        &pct(group.mean_pct),
                        10.0,
                        map_pct_color(group.mean_pct, green),
                    );
                }
            }
            let body = Rect::from_min_max(
                g.min + Vec2::new(1.0, header + 1.0),
                g.max - Vec2::splat(1.0),
            );
            for (ti, tile) in group.tiles.iter().enumerate() {
                let r = project(body, tile.bounds, 1.0, Vec2::ZERO);
                if r.width() < 0.3 || r.height() < 0.3 || !r.intersects(rect) {
                    continue;
                }
                tiles.push((r, gi, ti));
                let highlighted = if !search.is_empty() {
                    map_matches(tile, search)
                } else {
                    !watch || tile.watched
                };
                let rgb = getrich_core::market_map::tile_rgb(tile.quote.change_pct, green);
                let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                mesh.add_colored_rect(
                    r.shrink(0.5),
                    if highlighted {
                        color
                    } else {
                        color.gamma_multiply(0.24)
                    },
                );
                if (!search.is_empty() || watch) && highlighted {
                    marks.push(r);
                }
                if r.width() < 36.0 || r.height() < 23.0 {
                    continue;
                }
                let clip = r.intersect(rect).shrink(3.0);
                let size = (r.width() / 6.0).min(r.height() / 3.8).clamp(10.0, 19.0);
                let lines = r.height() > 42.0;
                let color = if highlighted {
                    Color32::WHITE
                } else {
                    Color32::from_gray(125)
                };
                add_text(
                    painter,
                    &mut text,
                    clip,
                    r.center() - Vec2::new(0.0, if lines { size * 0.65 } else { 0.0 }),
                    egui::Align2::CENTER_CENTER,
                    tile.quote.name.as_deref().unwrap_or(&tile.quote.symbol),
                    size,
                    color,
                );
                if lines {
                    add_text(
                        painter,
                        &mut text,
                        clip,
                        r.center() + Vec2::new(0.0, size * 0.7),
                        egui::Align2::CENTER_CENTER,
                        &pct(tile.quote.change_pct),
                        (size - 1.0).max(10.0),
                        color,
                    );
                }
                if tile.watched && r.width() > 70.0 && r.height() > 65.0 {
                    add_text(
                        painter,
                        &mut text,
                        clip,
                        r.min + Vec2::new(6.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        "★",
                        11.0,
                        Color32::from_rgb(255, 226, 168),
                    );
                }
            }
        }
        Self {
            data,
            rect,
            zoom,
            pan,
            search: search.into(),
            watch,
            green,
            scale: painter.ctx().pixels_per_point(),
            mesh,
            text,
            marks,
            tiles,
            headers,
        }
    }
    pub fn draw(&self, painter: &egui::Painter) {
        painter.add(egui::Shape::Mesh(self.mesh.clone()));
        for (clip, shape) in &self.text {
            painter.with_clip_rect(*clip).add(shape.clone());
        }
        for r in &self.marks {
            painter.rect_stroke(
                r.shrink(1.0),
                0.0,
                Stroke::new(1.5_f32, Color32::from_rgb(144, 185, 255)),
            );
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn add_text(
    painter: &egui::Painter,
    out: &mut Vec<(Rect, egui::Shape)>,
    clip: Rect,
    pos: Pos2,
    align: egui::Align2,
    label: &str,
    size: f32,
    color: Color32,
) {
    if !clip.is_positive() {
        return;
    }
    let galley = painter.layout_no_wrap(label.into(), egui::FontId::proportional(size), color);
    let pos = align.anchor_size(pos, galley.size()).min;
    out.push((
        clip,
        egui::Shape::Text(egui::epaint::TextShape::new(pos, galley, color)),
    ));
}
