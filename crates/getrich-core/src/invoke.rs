//! JSON invoke dispatcher shared by CLI and HTTP (WiParse C+E parity).

use crate::error::AppError;
use crate::import::{parse_json, ImportKind};
use crate::service::App;
use crate::APP_NAME;
use crate::VERSION;
use serde_json::{json, Value};
use std::path::PathBuf;

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
            Ok(serde_json::to_value(app.quote_board(board, query, sort, limit, offset)?)
                .unwrap_or(json!({})))
        }
        "stock.show" => {
            let symbol = str_param(&params, "symbol")
                .ok_or_else(|| AppError::InvalidArg("missing symbol".into()))?;
            let market = str_param(&params, "market");
            let key = app.parse_key(symbol, market)?;
            Ok(serde_json::to_value(app.show_stock(&key)?).unwrap_or(json!({})))
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
            Ok(serde_json::to_value(app.list_bars(&key, tf, from, to, limit)?)
                .unwrap_or(json!({})))
        }
        "import.file" => {
            let path = str_param(&params, "path")
                .ok_or_else(|| AppError::InvalidArg("missing path".into()))?;
            let kind = parse_kind(str_param(&params, "kind").unwrap_or("bundle"))?;
            Ok(serde_json::to_value(app.import_file(&PathBuf::from(path), kind)?)
                .unwrap_or(json!({})))
        }
        "import.json" => {
            let kind = parse_kind(str_param(&params, "kind").unwrap_or("bundle"))?;
            let payload = params
                .get("data")
                .cloned()
                .ok_or_else(|| AppError::InvalidArg("missing data".into()))?;
            let text = serde_json::to_string(&payload)
                .map_err(|e| AppError::InvalidArg(e.to_string()))?;
            let bundle = parse_json(&text, kind)?;
            Ok(serde_json::to_value(app.import_bundle(&bundle, "invoke")?)
                .unwrap_or(json!({})))
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
        other => Err(AppError::InvalidArg(format!("unknown import kind: {other}"))),
    }
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
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
