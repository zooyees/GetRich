//! GetRich desktop workstation — WiParse-style window/taskbar icon wiring.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Direct deps used as rustc 1.83 MSRV pins for eframe/winit (see workspace Cargo.toml).
#![allow(unused_crate_dependencies)]

mod app;
mod windows_icon;

use app::GetRichApp;
use getrich_core::config::load_config;
use getrich_core::logging;
use getrich_core::paths::{self, project_path};
use getrich_core::service::App;
use std::sync::Arc;

/// Embedded at compile time so window/taskbar icons work from dist without sidecar files.
const EMBEDDED_ICO: &[u8] = include_bytes!("../../../icon/GetRich.ico");

fn icon_from_bytes(bytes: &[u8]) -> Option<egui::IconData> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(to_icon_data(pick_icon_size(img)))
}

fn pick_icon_size(img: image::DynamicImage) -> image::DynamicImage {
    const TARGET: u32 = 32;
    if img.width() == TARGET && img.height() == TARGET {
        return img;
    }
    img.resize_exact(TARGET, TARGET, image::imageops::FilterType::Lanczos3)
}

fn to_icon_data(img: image::DynamicImage) -> egui::IconData {
    let rgba = img.to_rgba8();
    let (width, height) = (img.width(), img.height());
    egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    }
}

fn load_window_icon() -> Option<egui::IconData> {
    if let Some(icon) = icon_from_bytes(EMBEDDED_ICO) {
        return Some(icon);
    }
    let mut candidates = vec![
        project_path("icon/GetRich.ico"),
        project_path("Icon/GetRich.ico"),
        project_path("packaging/GetRich.ico"),
        std::path::PathBuf::from("icon/GetRich.ico"),
        std::path::PathBuf::from("GetRich.ico"),
    ];
    if let Some(p) = paths::app_icon_path() {
        candidates.insert(0, p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("GetRich.ico"));
            candidates.push(dir.join("icon").join("GetRich.ico"));
        }
    }
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if let Some(icon) = icon_from_bytes(&bytes) {
                tracing::info!("Loaded window icon from {}", path.display());
                return Some(icon);
            }
        }
    }
    tracing::warn!("Failed to load GetRich.ico for window/taskbar icon");
    None
}

fn main() -> eframe::Result<()> {
    windows_icon::set_process_app_id();
    let cfg = load_config().unwrap_or_default();
    let log_path = project_path(&cfg.system.log_file);
    let _ = logging::init(&cfg.system.log_level, Some(&log_path));

    let app = App::from_config(cfg.clone());
    if let Err(e) = app.migrate() {
        tracing::error!("migrate: {e}");
    }
    let bind = std::env::var("GETRICH_API_BIND").unwrap_or_else(|_| cfg.http.bind.clone());
    let api_app = Arc::new(app);
    let serve_app = api_app.clone();
    let bind_c = bind.clone();
    std::thread::Builder::new()
        .name("getrich-api".into())
        .spawn(move || {
            if let Err(e) = getrich_api::serve(serve_app, &bind_c) {
                tracing::error!("API server: {e}");
            }
        })
        .ok();

    let icon = load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([1100.0, 680.0])
        .with_title("GetRich 行情");
    if let Some(ref icon) = icon {
        viewport = viewport.with_icon(Arc::new(icon.clone()));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let ui_app = api_app;
    eframe::run_native(
        "GetRich",
        options,
        Box::new(move |cc| {
            if let Some(icon) = load_window_icon() {
                cc.egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
            }
            Ok(Box::new(GetRichApp::new(ui_app)))
        }),
    )
}
