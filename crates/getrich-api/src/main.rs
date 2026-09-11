//! GetRich long-running HTTP API (WiParse C+E: localhost invoke + events).

use getrich_core::config::load_config;
use getrich_core::logging;
use getrich_core::paths;
use getrich_core::service::App;
use std::sync::Arc;

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<(), String> {
    if let Some(path) = std::env::args().nth(1) {
        if path == "--help" || path == "-h" {
            println!(
                "getrich-api — quote workstation + JSON API\n\n  GETRICH_API_BIND  bind address (default config http.bind)\n  GETRICH_CONFIG    config.json path\n  GETRICH_DB        sqlite path\n"
            );
            return Ok(());
        }
    }
    let cfg = load_config().map_err(|e| e.to_string())?;
    let log_path = paths::project_path(&cfg.system.log_file);
    logging::init(&cfg.system.log_level, Some(&log_path)).map_err(|e| e.to_string())?;
    let app = Arc::new(App::from_config(cfg.clone()));
    app.migrate().map_err(|e| e.to_string())?;
    let bind = std::env::var("GETRICH_API_BIND").unwrap_or_else(|_| cfg.http.bind.clone());
    getrich_api::serve(app, &bind)
}
