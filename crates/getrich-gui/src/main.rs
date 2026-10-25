//! GetRich desktop workstation — WiParse-style window/taskbar icon wiring.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Direct deps used as rustc 1.83 MSRV pins for eframe/winit (see workspace Cargo.toml).
#![allow(unused_crate_dependencies)]

mod app;
mod chart_model;
mod controller;
mod file_dialog;
mod fonts;
mod windows_dpi;
mod windows_icon;

use app::GetRichApp;
use getrich_core::config::load_config;
use getrich_core::ingest;
use getrich_core::logging;
use getrich_core::paths::{self, project_path};
use getrich_core::realtime;
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
    // Must run before any HWND / user32 work so mixed-DPI monitors do not
    // bitmap-stretch the window. The PE manifest also requests PerMonitorV2.
    windows_dpi::enable_per_monitor_v2();
    windows_icon::set_process_app_id();
    let cfg = match load_config() {
        Ok(cfg) => cfg,
        Err(error) => {
            return startup_failure(format!(
                "配置文件无法读取：{error}\n配置位置：{}\n请修复 JSON 配置后重新启动。",
                paths::config_file().display()
            ))
        }
    };
    let log_path = project_path(&cfg.system.log_file);
    let _ = logging::init(&cfg.system.log_level, Some(&log_path));

    let app = App::from_config(cfg.clone());
    if let Err(e) = app.migrate() {
        return startup_failure(format!(
            "数据库初始化失败：{e}\n数据库：{}\n请检查目录权限、磁盘空间及数据库版本后重新启动。",
            app.db.path.display()
        ));
    }
    let bind = std::env::var("GETRICH_API_BIND").unwrap_or_else(|_| cfg.http.bind.clone());
    let api_app = Arc::new(app);
    realtime::start(&api_app);
    ingest::start(&api_app);
    getrich_core::jobs::start(&api_app);
    let serve_app = api_app.clone();
    let bind_c = bind.clone();
    let startup_status = Arc::new(std::sync::Mutex::new(None));
    let api_status = Arc::clone(&startup_status);
    let spawn_result=std::thread::Builder::new()
        .name("getrich-api".into())
        .spawn(move || {
            if let Err(e) = getrich_api::serve(serve_app, &bind_c) {
                tracing::error!("API server: {e}");
                *api_status.lock().unwrap_or_else(|e|e.into_inner())=Some(format!("浏览器服务 {bind_c} 启动失败：{e}。桌面仍使用当前数据目录；该端口可能属于其他实例，请关闭占用实例或修改 GETRICH_API_BIND 后重启。"));
            }
        });
    if let Err(error) = spawn_result {
        *startup_status.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(format!("浏览器服务线程启动失败：{error}"));
    }

    let icon = load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([760.0, 600.0])
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
            fonts::install(&cc.egui_ctx);
            GetRichApp::install_theme(&cc.egui_ctx);
            let _ = windows_dpi::install();
            if let Some(icon) = load_window_icon() {
                cc.egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
            }
            Ok(Box::new(GetRichApp::new(ui_app, startup_status)))
        }),
    )
}

fn startup_failure(message: String) -> eframe::Result<()> {
    struct Failure(String);
    impl eframe::App for Failure {
        fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("GetRich 启动诊断");
                ui.add_space(16.0);
                ui.label(&self.0);
                if ui.button("复制诊断").clicked() {
                    ui.output_mut(|o| o.copied_text = self.0.clone());
                }
                if ui.button("退出，修复后重试").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
    }
    eframe::run_native(
        "GetRich 启动诊断",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            fonts::install(&cc.egui_ctx);
            Ok(Box::new(Failure(message)))
        }),
    )
}
