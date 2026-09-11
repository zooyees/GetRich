//! Localhost API + static workstation (WiParse C+E endpoints).

use crate::capabilities::capabilities_json;
use chrono::Utc;
use getrich_core::error::AppError;
use getrich_core::invoke;
use getrich_core::service::App;
use getrich_core::VERSION;
use serde_json::{json, Value};
use std::io::{Read as IoRead, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_CSS: &str = include_str!("../web/app.css");
const APP_JS: &str = include_str!("../web/app.js");
const FAVICON_ICO: &[u8] = include_bytes!("../../../icon/GetRich.ico");

pub fn serve(app: Arc<App>, bind: &str) -> Result<(), String> {
    let server = Server::http(bind).map_err(|e| format!("API bind {bind}: {e}"))?;
    tracing::info!("GetRich API listening on http://{bind}");
    println!("GetRich workstation  http://{bind}/");
    let running = Arc::new(AtomicBool::new(true));
    let seq = Arc::new(AtomicU64::new(0));
    loop {
        let request = match server.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(_) => break,
        };
        let app = app.clone();
        let seq = seq.clone();
        let running = running.clone();
        thread::Builder::new()
            .name("getrich-api-req".into())
            .spawn(move || {
                if let Err(e) = handle_request(request, &app, &seq, &running) {
                    tracing::warn!("API request error: {e}");
                }
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn handle_request(
    mut request: Request,
    app: &App,
    seq: &AtomicU64,
    running: &AtomicBool,
) -> Result<(), String> {
    let url = request.url().to_string();
    let (path, query) = split_query(&url);
    let method = request.method().clone();

    match (method, path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => respond_html(request, INDEX_HTML),
        (Method::Get, "/app.css") => respond_bytes(request, "text/css; charset=utf-8", APP_CSS.as_bytes()),
        (Method::Get, "/app.js") => {
            respond_bytes(request, "application/javascript; charset=utf-8", APP_JS.as_bytes())
        }
        (Method::Get, "/favicon.ico") | (Method::Get, "/GetRich.ico") => {
            respond_bytes(request, "image/x-icon", FAVICON_ICO)
        }
        (Method::Get, "/v1/health") => {
            let stats = app.stats().ok();
            respond_json(
                request,
                200,
                &envelope_ok(
                    "health",
                    json!({
                        "version": VERSION,
                        "listening": true,
                        "db": app.db.path.display().to_string(),
                        "stats": stats,
                    }),
                ),
            )
        }
        (Method::Get, "/v1/capabilities") => {
            respond_json(request, 200, &envelope_ok("capabilities", capabilities_json()))
        }
        (Method::Get, "/v1/stats") => dispatch(request, app, "store.stats", json!({})),
        (Method::Get, "/v1/quotes") => {
            let params = json!({
                "board": qparam(query, "board").unwrap_or_else(|| "ALL".into()),
                "query": qparam(query, "q").or_else(|| qparam(query, "query")).unwrap_or_default(),
                "sort": qparam(query, "sort").unwrap_or_else(|| "symbol".into()),
                "limit": qparam(query, "limit").and_then(|s| s.parse::<u64>().ok()).unwrap_or(200),
                "offset": qparam(query, "offset").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            });
            dispatch(request, app, "stock.list", params)
        }
        (Method::Get, p) if p.starts_with("/v1/stocks/") => stock_get(request, app, p, query),
        (Method::Post, "/v1/invoke") => {
            let body = read_body(&mut request)?;
            let parsed: Value = serde_json::from_str(&body)
                .map_err(|e| format!("invalid JSON: {e}"))
                .unwrap_or_else(|e| json!({"_error": e}));
            if let Some(msg) = parsed.get("_error").and_then(|v| v.as_str()) {
                return respond_json(request, 400, &envelope_err("invoke", msg));
            }
            let method_name = match parsed.get("method").and_then(|v| v.as_str()) {
                Some(m) => m.to_string(),
                None => {
                    return respond_json(request, 400, &envelope_err("invoke", "missing method"));
                }
            };
            let params = parsed.get("params").cloned().unwrap_or_else(|| json!({}));
            dispatch(request, app, &method_name, params)
        }
        (Method::Get, "/v1/events") => serve_events(request, seq, running),
        (Method::Options, _) => {
            let mut response = Response::empty(204);
            if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]) {
                response = response.with_header(h);
            }
            if let Ok(h) =
                Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..])
            {
                response = response.with_header(h);
            }
            if let Ok(h) =
                Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..])
            {
                response = response.with_header(h);
            }
            request.respond(response).map_err(|e| e.to_string())
        }
        _ => respond_json(
            request,
            404,
            &envelope_err("http", &format!("not found: {path}")),
        ),
    }
}

fn stock_get(request: Request, app: &App, path: &str, query: &str) -> Result<(), String> {
    let rest = path.trim_start_matches("/v1/stocks/");
    let bars = rest.ends_with("/bars");
    let rest = rest.strip_suffix("/bars").unwrap_or(rest);
    let Some((market, symbol)) = rest.rsplit_once('/') else {
        return respond_json(request, 400, &envelope_err("stock", "expected /v1/stocks/{market}/{symbol}"));
    };
    if bars {
        let params = json!({
            "market": market,
            "symbol": symbol,
            "timeframe": qparam(query, "timeframe").unwrap_or_else(|| "1d".into()),
            "from": qparam(query, "from").unwrap_or_default(),
            "to": qparam(query, "to").unwrap_or_default(),
            "limit": qparam(query, "limit").and_then(|s| s.parse::<u64>().ok()).unwrap_or(500),
        });
        dispatch(request, app, "stock.bars", params)
    } else {
        dispatch(
            request,
            app,
            "stock.show",
            json!({ "market": market, "symbol": symbol }),
        )
    }
}

fn dispatch(request: Request, app: &App, method: &str, params: Value) -> Result<(), String> {
    match invoke::invoke(app, method, params) {
        Ok(data) => respond_json(request, 200, &envelope_ok(method, data)),
        Err(e) => {
            let code = match &e {
                AppError::NotFound(_) => 404,
                AppError::InvalidArg(_) | AppError::UnknownMethod(_) => 400,
                _ => 400,
            };
            respond_json(request, code, &envelope_err_code(method, e.code(), &e.to_string()))
        }
    }
}

fn serve_events(
    request: Request,
    seq: &AtomicU64,
    running: &AtomicBool,
) -> Result<(), String> {
    let mut writer = request.into_writer();
    write!(
        writer,
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-cache\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    let hello = json!({
        "type": "subscribed",
        "seq": seq.load(Ordering::SeqCst),
        "ts": now_iso(),
        "data": {}
    });
    writeln!(writer, "{hello}").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(15));
        let n = seq.fetch_add(1, Ordering::SeqCst) + 1;
        let ping = json!({"type":"ping","seq": n, "ts": now_iso()});
        if writeln!(writer, "{ping}").is_err() {
            break;
        }
        if writer.flush().is_err() {
            break;
        }
    }
    Ok(())
}

fn split_query(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    }
}

fn qparam(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            Some(percent_decode(v))
        } else {
            None
        }
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                    16,
                ) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_body(request: &mut Request) -> Result<String, String> {
    let len = request.body_length().unwrap_or(0);
    let mut buf = vec![0u8; len.min(8 * 1024 * 1024)];
    let mut read = 0;
    while read < buf.len() {
        match IoRead::read(request.as_reader(), &mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => {
                read += n;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    buf.truncate(read);
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn respond_html(request: Request, body: &str) -> Result<(), String> {
    respond_bytes(request, "text/html; charset=utf-8", body.as_bytes())
}

fn respond_bytes(request: Request, ctype: &str, body: &[u8]) -> Result<(), String> {
    let mut response = Response::from_data(body.to_vec()).with_status_code(StatusCode(200));
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()) {
        response = response.with_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]) {
        response = response.with_header(h);
    }
    request.respond(response).map_err(|e| e.to_string())
}

fn respond_json(request: Request, code: u16, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let mut response = Response::from_data(body).with_status_code(StatusCode(code));
    if let Ok(h) =
        Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
    {
        response = response.with_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]) {
        response = response.with_header(h);
    }
    if let Ok(h) =
        Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..])
    {
        response = response.with_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..]) {
        response = response.with_header(h);
    }
    request.respond(response).map_err(|e| e.to_string())
}

fn envelope_ok(cmd: &str, data: Value) -> Value {
    json!({
        "ok": true,
        "cmd": cmd,
        "ts": now_iso(),
        "data": data,
    })
}

fn envelope_err(cmd: &str, message: &str) -> Value {
    envelope_err_code(cmd, "ERROR", message)
}

fn envelope_err_code(cmd: &str, code: &str, message: &str) -> Value {
    json!({
        "ok": false,
        "cmd": cmd,
        "ts": now_iso(),
        "error": { "code": code, "message": message },
    })
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
