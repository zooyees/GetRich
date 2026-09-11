use serde_json::{json, Value};

pub fn capabilities_json() -> Value {
    json!({
        "transport": {
            "bind": "127.0.0.1:7878",
            "env": "GETRICH_API_BIND",
            "endpoints": [
                "GET /",
                "GET /v1/health",
                "GET /v1/capabilities",
                "GET /v1/stats",
                "GET /v1/quotes",
                "GET /v1/stocks/{market}/{symbol}",
                "GET /v1/stocks/{market}/{symbol}/bars",
                "POST /v1/invoke",
                "GET /v1/events?since_seq=0"
            ]
        },
        "ui": {
            "workstation": "/",
            "style": "tonghuashun-quote-board"
        },
        "methods": [
            {"method": "system.version", "params": {}},
            {"method": "db.migrate", "params": {}},
            {"method": "db.status", "params": {}},
            {"method": "store.stats", "params": {}},
            {"method": "stock.list", "params": {"board": "ALL", "query": "", "sort": "symbol", "limit": 200}},
            {"method": "quotes.list", "params": {"board": "沪市"}, "alias_of": "stock.list"},
            {"method": "stock.show", "params": {"symbol": "600519", "market": "CN.SH"}},
            {"method": "stock.bars", "params": {"symbol": "600519", "market": "CN.SH", "timeframe": "1d", "limit": 500}},
            {"method": "import.file", "params": {"path": "data/import.json", "kind": "bundle"}},
            {"method": "import.json", "params": {"kind": "bundle", "data": {}}}
        ],
        "events": ["ingest.done", "ping"]
    })
}
