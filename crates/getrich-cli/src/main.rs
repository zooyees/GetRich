//! GetRich JSON CLI (WiParse envelope). Local DB by default; `--attach` for API.

mod attach;
mod output;

use clap::{Parser, Subcommand};
use getrich_core::config::load_config;
use getrich_core::error::AppError;
use getrich_core::invoke;
use getrich_core::logging;
use getrich_core::models::Timeframe;
use getrich_core::paths;
use getrich_core::service::App;
use getrich_core::{APP_NAME, VERSION};
use output::{emit_error, emit_ok, OutputOptions};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "getrich",
    version = VERSION,
    about = "GetRich stock foundation CLI. JSON envelope on stdout.",
    after_help = "Docs: docs/CLI_REFERENCE.md  |  Import: docs/IMPORT.md  |  UI: getrich serve",
    arg_required_else_help = true
)]
struct Cli {
    #[arg(long, global = true)]
    pretty: bool,
    #[arg(short, long, global = true)]
    quiet: bool,
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    url: Option<String>,
    /// Forward to a running getrich-api instead of opening SQLite in-process.
    #[arg(long, global = true)]
    attach: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Version,
    /// Run the HTTP workstation (same as getrich-api).
    Serve {
        #[arg(long)]
        bind: Option<String>,
    },
    #[command(subcommand, arg_required_else_help = true)]
    Api(ApiCmd),
    #[command(subcommand, arg_required_else_help = true)]
    Db(DbCmd),
    #[command(subcommand, arg_required_else_help = true)]
    Import(ImportCmd),
    #[command(subcommand, arg_required_else_help = true)]
    Stock(StockCmd),
}

#[derive(Subcommand, Debug)]
enum ApiCmd {
    Health,
    Capabilities,
    Invoke {
        method: String,
        #[arg(short, long, default_value = "{}")]
        params: String,
    },
}

#[derive(Subcommand, Debug)]
enum DbCmd {
    Migrate,
    Status,
}

#[derive(Subcommand, Debug)]
enum ImportCmd {
    Bundle { #[arg(long)] file: PathBuf },
    Listings { #[arg(long)] file: PathBuf },
    Quotes { #[arg(long)] file: PathBuf },
    Bars { #[arg(long)] file: PathBuf },
}

#[derive(Subcommand, Debug)]
enum StockCmd {
    List {
        #[arg(long)]
        board: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value = "symbol")]
        sort: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Show {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        market: Option<String>,
    },
    Bars {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        market: Option<String>,
        #[arg(long, default_value = "1d")]
        timeframe: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Some(path) = &cli.config {
        std::env::set_var("GETRICH_CONFIG", path);
    }
    let opts = OutputOptions {
        pretty: cli.pretty,
        quiet: cli.quiet,
    };
    let code = match run(cli, &opts) {
        Ok(c) => c,
        Err((cmd, err)) => emit_error(&cmd, err.code(), &err.to_string(), &opts),
    };
    std::process::exit(code);
}

fn run(cli: Cli, opts: &OutputOptions) -> Result<i32, (String, AppError)> {
    match cli.command {
        Commands::Serve { bind } => {
            let cfg = load_config().map_err(|e| ("serve".into(), AppError::from(e)))?;
            let log_path = paths::project_path(&cfg.system.log_file);
            let _ = logging::init(&cfg.system.log_level, Some(&log_path));
            let app = Arc::new(App::from_config(cfg.clone()));
            app.migrate().map_err(|e| ("serve".into(), e))?;
            let bind = bind
                .or_else(|| std::env::var("GETRICH_API_BIND").ok())
                .unwrap_or(cfg.http.bind);
            getrich_api::serve(app, &bind).map_err(|e| ("serve".into(), AppError::Message(e)))?;
            Ok(0)
        }
        Commands::Version => Ok(emit_ok(
            "version",
            json!({ "name": APP_NAME, "version": VERSION, "edition": "rust" }),
            opts,
        )),
        Commands::Api(cmd) => api_cmd(cmd, cli.url, opts),
        other => {
            if cli.attach {
                attach_cmd(other, cli.url, opts)
            } else {
                local_cmd(other, opts)
            }
        }
    }
}

fn load_app() -> Result<App, AppError> {
    let cfg = load_config()?;
    let log_path = paths::project_path(&cfg.system.log_file);
    let _ = logging::init(&cfg.system.log_level, Some(&log_path));
    Ok(App::from_config(cfg))
}

fn local_cmd(cmd: Commands, opts: &OutputOptions) -> Result<i32, (String, AppError)> {
    let app = load_app().map_err(|e| (cmd_name(&cmd).into(), e))?;
    let (name, params) = cmd_to_invoke(&cmd).map_err(|e| (cmd_name(&cmd).into(), e))?;
    match invoke::invoke(&app, &name, params) {
        Ok(data) => Ok(emit_ok(&name, data, opts)),
        Err(e) => Err((name, e)),
    }
}

fn attach_cmd(
    cmd: Commands,
    url: Option<String>,
    opts: &OutputOptions,
) -> Result<i32, (String, AppError)> {
    let url = url.unwrap_or_else(attach::default_url);
    let (name, params) = cmd_to_invoke(&cmd).map_err(|e| (cmd_name(&cmd).into(), e))?;
    match attach::invoke_data(&url, &name, params) {
        Ok(data) => Ok(emit_ok(&name, data, opts)),
        Err(e) => Err((name, AppError::Message(e))),
    }
}

fn api_cmd(cmd: ApiCmd, url: Option<String>, opts: &OutputOptions) -> Result<i32, (String, AppError)> {
    let url = url.unwrap_or_else(attach::default_url);
    match cmd {
        ApiCmd::Health => match attach::health(&url).and_then(attach::data_or_error) {
            Ok(data) => Ok(emit_ok("health", data, opts)),
            Err(e) => Err(("health".into(), AppError::Message(e))),
        },
        ApiCmd::Capabilities => match attach::capabilities(&url).and_then(attach::data_or_error) {
            Ok(data) => Ok(emit_ok("capabilities", data, opts)),
            Err(e) => Err(("capabilities".into(), AppError::Message(e))),
        },
        ApiCmd::Invoke { method, params } => {
            let params: Value = serde_json::from_str(&params)
                .map_err(|e| (method.clone(), AppError::InvalidArg(e.to_string())))?;
            match attach::invoke_data(&url, &method, params) {
                Ok(data) => Ok(emit_ok(&method, data, opts)),
                Err(e) => Err((method, AppError::Message(e))),
            }
        }
    }
}

fn cmd_name(cmd: &Commands) -> &'static str {
    match cmd {
        Commands::Db(_) => "db",
        Commands::Import(_) => "import",
        Commands::Stock(_) => "stock",
        _ => "getrich",
    }
}

fn cmd_to_invoke(cmd: &Commands) -> Result<(String, Value), AppError> {
    match cmd {
        Commands::Db(DbCmd::Migrate) => Ok(("db.migrate".into(), json!({}))),
        Commands::Db(DbCmd::Status) => Ok(("db.status".into(), json!({}))),
        Commands::Import(ImportCmd::Bundle { file }) => Ok((
            "import.file".into(),
            json!({"path": file.display().to_string(), "kind": "bundle"}),
        )),
        Commands::Import(ImportCmd::Listings { file }) => Ok((
            "import.file".into(),
            json!({"path": file.display().to_string(), "kind": "listings"}),
        )),
        Commands::Import(ImportCmd::Quotes { file }) => Ok((
            "import.file".into(),
            json!({"path": file.display().to_string(), "kind": "quotes"}),
        )),
        Commands::Import(ImportCmd::Bars { file }) => Ok((
            "import.file".into(),
            json!({"path": file.display().to_string(), "kind": "bars"}),
        )),
        Commands::Stock(StockCmd::List {
            board,
            query,
            sort,
            limit,
        }) => Ok((
            "stock.list".into(),
            json!({"board": board, "query": query, "sort": sort, "limit": limit}),
        )),
        Commands::Stock(StockCmd::Show { symbol, market }) => Ok((
            "stock.show".into(),
            json!({"symbol": symbol, "market": market}),
        )),
        Commands::Stock(StockCmd::Bars {
            symbol,
            market,
            timeframe,
            from,
            to,
            limit,
        }) => {
            let _tf: Timeframe = timeframe.parse().map_err(AppError::InvalidArg)?;
            Ok((
                "stock.bars".into(),
                json!({
                    "symbol": symbol,
                    "market": market,
                    "timeframe": timeframe,
                    "from": from,
                    "to": to,
                    "limit": limit
                }),
            ))
        }
        _ => Err(AppError::InvalidArg("command not available here".into())),
    }
}
