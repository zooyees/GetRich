//! JSON invoke dispatcher shared by CLI and HTTP (WiParse C+E parity).

use crate::error::AppError;
use crate::import::{parse_json, ImportKind};
use crate::service::App;
use crate::APP_NAME;
use crate::VERSION;
use serde_json::{json, Value};
use std::path::Path;

pub fn invoke(app: &App, method: &str, params: Value) -> Result<Value, AppError> {
    match method {
        "system.version" => Ok(json!({
            "name": APP_NAME,
            "version": VERSION,
            "edition": "rust",
        })),
        "db.migrate" => app.migrate(),
        "db.status" => app.db_status(),
        "store.stats" => Ok(serde_json::to_value(app.stats()?).unwrap_or(json!({}))),
        "stock.list" | "quotes.list" => {
            let board = str_param(&params, "board");
            let query = str_param(&params, "query").or_else(|| str_param(&params, "q"));
            let sort = str_param(&params, "sort");
            let limit = u_param(&params, "limit").unwrap_or(200) as usize;
            let offset = u_param(&params, "offset").unwrap_or(0) as usize;
            Ok(serde_json::to_value(app.quote_board_filtered(
                board,
                query,
                sort,
                limit,
                offset,
                str_param(&params, "industry"),
                str_param(&params, "change_filter"),
            )?)
            .unwrap_or(json!({})))
        }
        "stock.show" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let key = app.parse_key(symbol, market)?;
            Ok(serde_json::to_value(app.show_stock(&key)?).unwrap_or(json!({})))
        }
        "stock.research" => {
            let key = app.parse_key(
                str_param(&params, "symbol").unwrap_or(""),
                str_param(&params, "market"),
            )?;
            app.stock_research(&key)
        }
        "stock.bars" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let key = app.parse_key(symbol, market)?;
            let tf = str_param(&params, "timeframe")
                .unwrap_or("1d")
                .parse()
                .map_err(AppError::InvalidArg)?;
            let from = str_param(&params, "from");
            let to = str_param(&params, "to");
            let limit = u_param(&params, "limit").unwrap_or(500) as usize;
            Ok(
                serde_json::to_value(app.list_bars(&key, tf, from, to, limit)?)
                    .unwrap_or(json!({})),
            )
        }
        "import.file" => {
            let path = str_param(&params, "path")
                .ok_or_else(|| AppError::InvalidArg("missing path".into()))?;
            let kind = parse_kind(str_param(&params, "kind").unwrap_or("bundle"))?;
            let path = app.resolve_import_path(Path::new(path), true)?;
            Ok(serde_json::to_value(app.import_file(&path, kind)?).unwrap_or(json!({})))
        }
        "import.json" => {
            let kind = parse_kind(str_param(&params, "kind").unwrap_or("bundle"))?;
            let payload = params
                .get("data")
                .cloned()
                .ok_or_else(|| AppError::InvalidArg("missing data".into()))?;
            let text =
                serde_json::to_string(&payload).map_err(|e| AppError::InvalidArg(e.to_string()))?;
            let bundle = parse_json(&text, kind)?;
            Ok(serde_json::to_value(app.import_bundle(&bundle, "invoke")?).unwrap_or(json!({})))
        }
        "watch.list" => Ok(json!({
            "watchlist": app.watchlist(),
            "default_board": app.default_board(),
        })),
        "watch.toggle" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let name = str_param(&params, "name").map(|s| s.to_string());
            app.toggle_watch(symbol, market, name)
        }
        "watch.set" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let name = str_param(&params, "name").map(str::to_string);
            let watched = params
                .get("watched")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| AppError::InvalidArg("missing watched boolean".into()))?;
            app.set_watch(symbol, market, name, watched)
        }
        "watch.import" => app.import_watch(&params),
        "watch.move" => app.move_watch(&params),
        "settings.board" => {
            let board = str_param(&params, "board").unwrap_or("ALL");
            app.set_default_board(board)
        }
        "settings.get" => app.settings_view(),
        "history.view" => app.record_view(
            str_param(&params, "symbol").unwrap_or(""),
            str_param(&params, "market"),
        ),
        "settings.save" => app.save_settings(&params),
        "provider.capabilities" => Ok(app.provider_capabilities()),
        "provider.test" => app.provider_test(),
        "import.preview" => {
            let kind = if params.get("data").is_some() {
                "import.json"
            } else {
                "import.file"
            };
            crate::import::preview(
                &app.db,
                &app.import_bundle_from_params(kind, &params)?,
                crate::jobs::import_source(kind, &params),
            )
        }
        "job.submit" => app.submit_job(
            str_param(&params, "kind").unwrap_or(""),
            params.get("params").cloned().unwrap_or(json!({})),
        ),
        "job.list" => app.jobs_list(),
        "job.get" => app.job(u_param(&params, "id").unwrap_or(0) as i64),
        "job.cancel" => app.cancel_job(u_param(&params, "id").unwrap_or(0) as i64),
        "job.retry" => app.retry_job(u_param(&params, "id").unwrap_or(0) as i64),
        "ingest.runs" => {
            let limit = u_param(&params, "limit").unwrap_or(10) as usize;
            Ok(json!({ "runs": app.ingest_runs(limit)? }))
        }
        "market.pulse" | "pulse.snapshot" => {
            Ok(serde_json::to_value(app.market_pulse()).unwrap_or(json!({})))
        }
        "market.map" => {
            let query = serde_json::from_value(params)
                .map_err(|e| AppError::InvalidArg(format!("星图参数：{e}")))?;
            serde_json::to_value(app.market_map(query)?)
                .map_err(|e| AppError::Message(e.to_string()))
        }
        "freshness.list" => {
            let limit = u_param(&params, "limit").unwrap_or(50) as usize;
            Ok(json!({ "rows": app.list_freshness(limit)? }))
        }
        "tushare.status" => Ok(app.tushare_status()),
        "tushare.sync" => Ok(serde_json::to_value(app.ingest_tushare()?).unwrap_or(json!({}))),
        "tushare.quote" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let key = app.parse_key(symbol, market)?;
            Ok(serde_json::to_value(app.sync_tushare_quote(&key)?).unwrap_or(json!({})))
        }
        "tushare.bars" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let key = app.parse_key(symbol, market)?;
            let days = u_param(&params, "days").unwrap_or(400) as i64;
            Ok(serde_json::to_value(app.sync_tushare_bars(&key, days)?).unwrap_or(json!({})))
        }
        other => Err(AppError::UnknownMethod(other.into())),
    }
}

fn parse_kind(s: &str) -> Result<ImportKind, AppError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "bundle" => Ok(ImportKind::Bundle),
        "listings" | "listing" => Ok(ImportKind::Listings),
        "quotes" | "quote" => Ok(ImportKind::Quotes),
        "bars" | "bar" => Ok(ImportKind::Bars),
        other => Err(AppError::InvalidArg(format!(
            "unknown import kind: {other}"
        ))),
    }
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

fn u_param(params: &Value, key: &str) -> Option<u64> {
    params.get(key).and_then(|v| v.as_u64()).or_else(|| {
        params
            .get(key)
            .and_then(|v| v.as_i64())
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
    })
}
