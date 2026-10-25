//! Load a CJK face so Chinese labels are not tofu (□).
//! egui's default fonts are Latin/emoji only.

use egui::{FontData, FontDefinitions, FontFamily};
use std::path::{Path, PathBuf};

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    match load_cjk_font() {
        Some((name, data, path)) => {
            tracing::info!("CJK font: {name} from {}", path.display());
            fonts.font_data.insert(name.clone(), data);
            if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                family.insert(0, name.clone());
            }
            if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                family.push(name);
            }
        }
        None => {
            tracing::error!("No CJK font found. Chinese UI text will render as boxes.");
        }
    }
    ctx.set_fonts(fonts);
}

fn load_cjk_font() -> Option<(String, FontData, PathBuf)> {
    for path in candidates() {
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes.len() < 8 {
            continue;
        }
        // CFF OpenType (OTTO) reports zero advances in ab_glyph → stacked glyphs.
        if &bytes[0..4] == b"OTTO" {
            continue;
        }
        let mut data = FontData::from_owned(bytes);
        data.index = 0;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("cjk")
            .to_string();
        return Some((stem, data, path));
    }
    None
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("fonts").join("Deng.ttf"));
            out.push(dir.join("Deng.ttf"));
        }
    }
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let fonts = Path::new(&win).join("Fonts");
    // TrueType only. CFF .otf and some TTC faces report zero advance in ab_glyph,
    // which stacks every glyph of 「涨跌幅」 on one pixel.
    for name in [
        "Deng.ttf",
        "msyh.ttf",
        "simhei.ttf",
        "msyh.ttc",
        "msyhl.ttc",
    ] {
        out.push(fonts.join(name));
    }
    out.push(PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"));
    out.push(PathBuf::from(
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ));
    out
}
