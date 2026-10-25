//! Localhost API + static workstation (WiParse C+E endpoints).

use crate::capabilities::capabilities_json;
use chrono::Utc;
use getrich_core::error::AppError;
use getrich_core::ingest;
use getrich_core::invoke;
use getrich_core::realtime;
use getrich_core::service::App;
use getrich_core::VERSION;
use serde_json::{json, Value};
use std::io::{Read as IoRead, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_CSS: &str = include_str!("../web/app.css");
const APP_JS: &str = include_str!("../web/app.js");
const RUNTIME_JS: &str = include_str!("../web/runtime.js");
const CHART_JS: &str = include_str!("../web/chart.js");
const WATCH_SEARCH_JS: &str = include_str!("../web/watch-search.js");
const STOCK_RESEARCH_JS: &str = include_str!("../web/stock-research.js");
const MANAGEMENT_JS: &str = include_str!("../web/management.js");
const PULSE_JS: &str = include_str!("../web/pulse.js");
const MARKET_MAP_JS: &str = include_str!("../web/market-map.js");
const FAVICON_ICO: &[u8] = include_bytes!("../../../icon/GetRich.ico");
const MAX_CONCURRENT_REQUESTS: usize = 64;

struct ActiveRequestGuard(Arc<AtomicUsize>);

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn serve(app: Arc<App>, bind: &str) -> Result<(), String> {
    if !is_loopback_bind(bind) {
        return Err(format!(
            "refusing non-loopback bind {bind}; use 127.0.0.1, localhost, or [::1]"
        ));
    }
    realtime::start(&app);
    ingest::start(&app);
    getrich_core::jobs::start(&app);
    let server = Server::http(bind).map_err(|e| format!("API bind {bind}: {e}"))?;
    tracing::info!("GetRich API listening on http://{bind}");
    println!("GetRich workstation  http://{bind}/");
    let running = Arc::new(AtomicBool::new(true));
    let active_requests = Arc::new(AtomicUsize::new(0));
    let event_requests = Arc::new(AtomicUsize::new(0));
    let control_requests = Arc::new(AtomicUsize::new(0));
    loop {
        let request = match server.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(_) => break,
        };
        let (budget, limit) = if request.url().starts_with("/v1/events") {
            (&event_requests, 16)
        } else if request.url().starts_with("/v1/health") || request.url().starts_with("/v2/system")
        {
            (&control_requests, 8)
        } else {
            (&active_requests, MAX_CONCURRENT_REQUESTS)
        };
        if budget.fetch_add(1, Ordering::SeqCst) >= limit {
            budget.fetch_sub(1, Ordering::SeqCst);
            let _ = respond_json(
                request,
                503,
                &envelope_err("http", "server is busy; retry shortly"),
            );
            continue;
        }
        let app = app.clone();
        let running = running.clone();
        let active = budget.clone();
        if let Err(error) = thread::Builder::new()
            .name("getrich-api-req".into())
            .spawn(move || {
                let _guard = ActiveRequestGuard(active);
                if let Err(e) = handle_request(request, &app, &running) {
                    tracing::warn!("API request error: {e}");
                }
            })
        {
            budget.fetch_sub(1, Ordering::SeqCst);
            return Err(error.to_string());
        }
    }
    Ok(())
}

fn handle_request(mut request: Request, app: &App, running: &AtomicBool) -> Result<(), String> {
    let url = request.url().to_string();
    let (path, query) = split_query(&url);
    let method = request.method().clone();
    if method == Method::Post && forbidden_origin(&request) {
        return respond_json(request, 403, &envelope_err("http", "origin not allowed"));
    }

    match (method, path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => respond_html(request, INDEX_HTML),
        (Method::Get, "/app.css") => {
            respond_bytes(request, "text/css; charset=utf-8", APP_CSS.as_bytes())
        }
        (Method::Get, "/app.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            APP_JS.as_bytes(),
        ),
        (Method::Get, "/favicon.ico") | (Method::Get, "/GetRich.ico") => {
            respond_bytes(request, "image/x-icon", FAVICON_ICO)
        }
        (Method::Get, "/runtime.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            RUNTIME_JS.as_bytes(),
        ),
        (Method::Get, "/watch-search.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            WATCH_SEARCH_JS.as_bytes(),
        ),
        (Method::Get, "/stock-research.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            STOCK_RESEARCH_JS.as_bytes(),
        ),
        (Method::Get, "/chart.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            CHART_JS.as_bytes(),
        ),
        (Method::Get, "/management.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            MANAGEMENT_JS.as_bytes(),
        ),
        (Method::Get, "/market-map.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            MARKET_MAP_JS.as_bytes(),
        ),
        (Method::Get, "/v2/market-map") => {
            let mut params = serde_json::Map::new();
            for key in ["board", "metric", "industry"] {
                if let Some(value) = qparam(query, key) {
                    params.insert(key.into(), json!(value));
                }
            }
            if let Some(value) = qparam(query, "aspect").and_then(|s| s.parse::<f64>().ok()) {
                params.insert("aspect".into(), json!(value));
            }
            if let Some(value) = qparam(query, "watch_only") {
                params.insert("watch_only".into(), json!(value == "true"));
            }
            match invoke::invoke(app, "market.map", Value::Object(params)) {
                Ok(value) => respond_json(request, 200, &value),
                Err(e) => respond_json(request, 400, &envelope_err("market.map", &e.to_string())),
            }
        }
        (Method::Get, "/pulse.js") => respond_bytes(
            request,
            "application/javascript; charset=utf-8",
            PULSE_JS.as_bytes(),
        ),
        (Method::Get, "/v1/health") => match app.stats() {
            Ok(stats) => respond_json(
                request,
                200,
                &envelope_ok(
                    "health",
                    json!({
                        "version": VERSION,
                        "listening": true,
                        "instance_id": app.events.epoch(),
                        "data_root": app.db.path.parent(),
                        "runtime": app.realtime.health(),
                        "db": app.db.path.display().to_string(),
                        "stats": stats,
                    }),
                ),
            ),
            Err(error) => respond_json(
                request,
                503,
                &envelope_err_code("health", error.code(), &error.to_string()),
            ),
        },
        (Method::Get, "/v1/capabilities") => respond_json(
            request,
            200,
            &envelope_ok("capabilities", capabilities_json()),
        ),
        (Method::Get, "/v1/stats") => dispatch(request, app, "store.stats", json!({})),
        (Method::Get, "/v1/watch") => dispatch(request, app, "watch.list", json!({})),
        (Method::Get, "/v1/settings") => dispatch(request, app, "settings.get", json!({})),
        (Method::Get, "/v1/jobs") => dispatch(request, app, "job.list", json!({})),
        (Method::Get, "/v1/providers") => {
            dispatch(request, app, "provider.capabilities", json!({}))
        }
        (Method::Get, "/v1/runs") => dispatch(request, app, "ingest.runs", json!({"limit": 10})),
        (Method::Get, "/v1/pulse") => dispatch(request, app, "market.pulse", json!({})),
        (Method::Get, "/v2/pulse") => match app.realtime_snapshot() {
            Some(snapshot) => respond_json(
                request,
                200,
                &serde_json::to_value(&*snapshot).unwrap_or_else(|_| json!({})),
            ),
            None => respond_json(request, 503, &envelope_err("pulse", "snapshot warming up")),
        },
        (Method::Get, "/v2/pulse/summary") => match app.realtime_snapshot() {
            Some(snapshot) => respond_json(
                request,
                200,
                &json!({
                    "schema_version": snapshot.schema_version,
                    "snapshot_version": snapshot.snapshot_version,
                    "trade_date": snapshot.trade_date,
                    "phase": snapshot.phase,
                    "source_at": snapshot.source_at,
                    "computed_at": snapshot.computed_at,
                    "quality": snapshot.quality,
                    "regime": snapshot.pulse.regime,
                    "headline": snapshot.pulse.headline,
                }),
            ),
            None => respond_json(request, 503, &envelope_err("pulse", "snapshot warming up")),
        },
        (Method::Get, "/v2/system/realtime") => respond_json(
            request,
            200,
            &json!({
                "schema_version": 2,
                "runtime": app.realtime.health(),
                "events": {
                    "subscribers": app.events.subscriber_count(),
                    "coalesced_total": app.events.coalesced(),
                    "seq": app.events.seq(),
                },
                "ingest": app.tushare_status(),
            }),
        ),
        (Method::Get, "/v1/tushare") => dispatch(request, app, "tushare.status", json!({})),
        (Method::Post, "/v1/tushare/sync") => dispatch(request, app, "tushare.sync", json!({})),
        (Method::Post, "/v1/tushare/bars") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err(error) => {
                    return respond_json(request, 400, &envelope_err("tushare.bars", &error))
                }
            };
            let params = match serde_json::from_str(&body) {
                Ok(params) => params,
                Err(error) => {
                    return respond_json(
                        request,
                        400,
                        &envelope_err("tushare.bars", &format!("invalid JSON: {error}")),
                    )
                }
            };
            dispatch(request, app, "tushare.bars", params)
        }
        (Method::Get, "/v1/quotes") => {
            let params = json!({
                "board": qparam(query, "board").unwrap_or_else(|| "ALL".into()),
                "query": qparam(query, "q").or_else(|| qparam(query, "query")).unwrap_or_default(),
                "sort": qparam(query, "sort").unwrap_or_else(|| "symbol".into()),
                "industry": qparam(query, "industry").unwrap_or_default(),
                "change_filter": qparam(query,"change_filter").unwrap_or_default(),
                "limit": qparam(query, "limit").and_then(|s| s.parse::<u64>().ok()).unwrap_or(200),
                "offset": qparam(query, "offset").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            });
            dispatch(request, app, "stock.list", params)
        }
        (Method::Get, p) if p.starts_with("/v1/stocks/") => stock_get(request, app, p, query),
        (Method::Post, "/v1/invoke") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err(error) => return respond_json(request, 400, &envelope_err("invoke", &error)),
            };
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
        (Method::Get, "/v1/events") => {
            let since = qparam(query, "since_seq").and_then(|value| value.parse::<u64>().ok());
            let epoch = qparam(query, "stream_epoch");
            serve_events(request, app, running, since, epoch.as_deref())
        }
        (Method::Options, _) => {
            let origin = loopback_origin(&request);
            let mut response = Response::empty(204);
            if let Some(ref origin) = origin {
                if let Ok(h) =
                    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes())
                {
                    response = response.with_header(h);
                }
            }
            if let Ok(h) = Header::from_bytes(
                &b"Access-Control-Allow-Methods"[..],
                &b"GET, POST, OPTIONS"[..],
            ) {
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
        return respond_json(
            request,
            400,
            &envelope_err("stock", "expected /v1/stocks/{market}/{symbol}"),
        );
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
                AppError::Config(_) => 503,
                AppError::Db(_) | AppError::Io(_) | AppError::Message(_) => 500,
            };
            respond_json(
                request,
                code,
                &envelope_err_code(method, e.code(), &e.to_string()),
            )
        }
    }
}

fn serve_events(
    request: Request,
    app: &App,
    running: &AtomicBool,
    since_seq: Option<u64>,
    stream_epoch: Option<&str>,
) -> Result<(), String> {
    let origin = loopback_origin(&request);
    let cors = origin
        .as_deref()
        .map(|o| format!("Access-Control-Allow-Origin: {o}\r\n"))
        .unwrap_or_default();
    let restarted = stream_epoch.is_some_and(|value| value != app.events.epoch());
    let subscription = app
        .events
        .subscribe_since(if restarted { None } else { since_seq });
    let rx = subscription.receiver;
    let mut writer = request.into_writer();
    write!(
        writer,
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-cache\r\nConnection: close\r\n{cors}\r\n"
    )
    .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    let hello = json!({
        "type": "subscribed",
        "seq": subscription.cursor,
        "stream_epoch": app.events.epoch(),
        "ts": now_iso(),
        "data": {}
    });
    writeln!(writer, "{hello}").map_err(|e| e.to_string())?;
    if restarted || subscription.resync_required {
        writeln!(writer,"{}",json!({"type":"resync_required","stream_epoch":app.events.epoch(),"seq":subscription.cursor})).map_err(|e|e.to_string())?;
    }
    let mut last_seq = if restarted || subscription.resync_required {
        subscription.cursor
    } else {
        since_seq.unwrap_or(subscription.cursor)
    };
    for message in subscription
        .replay
        .into_iter()
        .filter(|_| !restarted && !subscription.resync_required)
    {
        last_seq = message
            .get("seq")
            .and_then(Value::as_u64)
            .unwrap_or(last_seq);
        writeln!(writer, "{message}").map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(msg) => {
                let seq = msg.get("seq").and_then(Value::as_u64).unwrap_or(last_seq);
                if seq <= last_seq {
                    continue;
                }
                if seq > last_seq.saturating_add(1) {
                    let replay = app.events.replay_since(last_seq);
                    if replay
                        .first()
                        .and_then(|m| m.get("seq"))
                        .and_then(Value::as_u64)
                        .is_some_and(|first| first > last_seq + 1)
                    {
                        let _ = writeln!(
                            writer,
                            "{}",
                            json!({"type":"resync_required","stream_epoch":app.events.epoch(),"seq":seq})
                        );
                    }
                    for missed in replay {
                        let missed_seq = missed.get("seq").and_then(Value::as_u64).unwrap_or(0);
                        if missed_seq >= seq {
                            break;
                        }
                        if writeln!(writer, "{missed}").is_err() {
                            return Ok(());
                        }
                    }
                }
                if writeln!(writer, "{msg}").is_err() {
                    break;
                }
                last_seq = seq;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                let ping = json!({"type":"ping","watermark": app.events.seq(),"stream_epoch":app.events.epoch(), "ts": now_iso()});
                if writeln!(writer, "{ping}").is_err() {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
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
                if let Ok(b) =
                    u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
                {
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
    const MAX_BODY: usize = 8 * 1024 * 1024;
    if request.body_length().is_some_and(|len| len > MAX_BODY) {
        return Err("request body exceeds 8 MiB".into());
    }
    let mut buf = Vec::new();
    request
        .as_reader()
        .take((MAX_BODY + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() > MAX_BODY {
        return Err("request body exceeds 8 MiB".into());
    }
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn respond_html(request: Request, body: &str) -> Result<(), String> {
    respond_bytes(request, "text/html; charset=utf-8", body.as_bytes())
}

fn origin_header(request: &Request) -> Option<String> {
    request.headers().iter().find_map(|h| {
        let name = h.field.as_str().to_string();
        if name.eq_ignore_ascii_case("Origin") {
            Some(h.value.as_str().to_string())
        } else {
            None
        }
    })
}

fn forbidden_origin(request: &Request) -> bool {
    origin_header(request).is_some() && loopback_origin(request).is_none()
}

fn is_loopback_bind(bind: &str) -> bool {
    let s = bind.trim();
    let host = if let Some(rest) = s.strip_prefix('[') {
        rest.split(']').next().unwrap_or(s)
    } else {
        match s.rfind(':') {
            Some(i) => &s[..i],
            None => s,
        }
    };
    let host = host.trim();
    host.eq_ignore_ascii_case("localhost")
        || host == "::1"
        || host == "127.0.0.1"
        || (host.starts_with("127.") && host.split('.').count() == 4)
}

fn loopback_origin(request: &Request) -> Option<String> {
    request.headers().iter().find_map(|h| {
        let name = h.field.as_str().to_string();
        if !name.eq_ignore_ascii_case("Origin") {
            return None;
        }
        let v = h.value.as_str().to_string();
        if is_loopback_origin_value(&v) {
            Some(v)
        } else {
            None
        }
    })
}

fn is_loopback_origin_value(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn with_cors(
    mut response: tiny_http::Response<std::io::Cursor<Vec<u8>>>,
    origin: Option<&str>,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let Some(origin) = origin else {
        return response;
    };
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes()) {
        response = response.with_header(h);
    }
    response
}

fn respond_bytes(request: Request, ctype: &str, body: &[u8]) -> Result<(), String> {
    let origin = loopback_origin(&request);
    let mut response = Response::from_data(body.to_vec()).with_status_code(StatusCode(200));
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()) {
        response = response.with_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
        response = response.with_header(h);
    }
    request
        .respond(with_cors(response, origin.as_deref()))
        .map_err(|e| e.to_string())
}

fn respond_json(request: Request, code: u16, value: &Value) -> Result<(), String> {
    let origin = loopback_origin(&request);
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let mut response = Response::from_data(body).with_status_code(StatusCode(code));
    if let Ok(h) = Header::from_bytes(
        &b"Content-Type"[..],
        &b"application/json; charset=utf-8"[..],
    ) {
        response = response.with_header(h);
    }
    if origin.is_some() {
        if let Ok(h) = Header::from_bytes(
            &b"Access-Control-Allow-Methods"[..],
            &b"GET, POST, OPTIONS"[..],
        ) {
            response = response.with_header(h);
        }
        if let Ok(h) =
            Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..])
        {
            response = response.with_header(h);
        }
    }
    request
        .respond(with_cors(response, origin.as_deref()))
        .map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::{is_loopback_bind, is_loopback_origin_value};

    #[test]
    fn loopback_bind_ok() {
        assert!(is_loopback_bind("127.0.0.1:7878"));
        assert!(is_loopback_bind("localhost:7878"));
        assert!(is_loopback_bind("[::1]:7878"));
        assert!(!is_loopback_bind("0.0.0.0:7878"));
        assert!(!is_loopback_bind("192.168.1.2:7878"));
    }

    #[test]
    fn loopback_origin_is_parsed_exactly() {
        assert!(is_loopback_origin_value("http://localhost:7878"));
        assert!(is_loopback_origin_value("http://127.0.0.1:7878"));
        assert!(is_loopback_origin_value("http://[::1]:7878"));
        assert!(!is_loopback_origin_value("http://localhost.evil.example"));
        assert!(!is_loopback_origin_value("null"));
        assert!(!is_loopback_origin_value("file://localhost"));
    }
}
