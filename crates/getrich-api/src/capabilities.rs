use serde_json::{json, Value};

pub fn capabilities_json() -> Value {
    json!({
        "transport": {
            "contract_version":2,
            "event_limit":16,
            "request_limit":64,
            "control_limit":8,
            "bind": "127.0.0.1:7878",
            "env": "GETRICH_API_BIND",
            "endpoints": [
                "GET /",
                "GET /v1/health",
                "GET /v1/capabilities",
                "GET /v1/stats",
                "GET /v1/settings",
                "GET /v1/jobs",
                "GET /v1/providers",
                "GET /v1/quotes",
                "GET /v1/stocks/{market}/{symbol}",
                "GET /v1/stocks/{market}/{symbol}/bars",
                "GET /v1/pulse",
                "GET /v2/pulse",
                "GET /v2/market-map?board=CN&metric=auto&aspect=1.6",
                "GET /v2/pulse/summary",
                "GET /v2/system/realtime",
                "GET /v1/tushare",
                "POST /v1/tushare/sync",
                "POST /v1/invoke",
                "GET /v1/events?since_seq=0"
            ]
        },
        "ui": {
            "workstation": "/",
            "style": "market-atlas-workstation"
        },
        "methods": [
            {"method":"settings.get","params":{}},
            {"method":"settings.save","params":{"default_board":"ALL","preferences":{}}},
            {"method":"provider.capabilities","params":{}},
            {"method":"import.preview","params":{"path":"examples/demo.board.json"}},
            {"method":"job.submit","params":{"kind":"pulse.recompute","params":{}}},
            {"method":"job.list","params":{}},
            {"method":"job.get","params":{"id":1}},
            {"method":"job.cancel","params":{"id":1}},
            {"method":"job.retry","params":{"id":1}},
            {"method":"watch.set","params":{"symbol":"600519","market":"CN.SH","watched":true}},
            {"method":"watch.import","params":{"watchlist":[]}},
            {"method":"watch.move","params":{"symbol":"600519","market":"CN.SH","direction":1}},
            {"method":"history.view","params":{"symbol":"600519","market":"CN.SH"}},
            {"method": "system.version", "params": {}},
            {"method": "db.migrate", "params": {}},
            {"method": "db.status", "params": {}},
            {"method": "store.stats", "params": {}},
            {"method": "stock.list", "params": {"board": "ALL", "query": "", "sort": "symbol", "limit": 200}},
            {"method": "quotes.list", "params": {"board": "沪市"}, "alias_of": "stock.list"},
            {"method": "stock.show", "params": {"symbol": "600519", "market": "CN.SH"}},
            {"method":"stock.research","params":{"symbol":"600519","market":"CN.SH"}},
            {"method": "stock.bars", "params": {"symbol": "600519", "market": "CN.SH", "timeframe": "1d", "limit": 500}},
            {"method": "import.file", "params": {"path": "data/import.json", "kind": "bundle"}},
            {"method": "import.json", "params": {"kind": "bundle", "data": {}}},
            {"method": "watch.list", "params": {}},
            {"method": "watch.toggle", "params": {"symbol": "600519", "market": "CN.SH"}},
            {"method": "ingest.runs", "params": {"limit": 10}},
            {"method": "market.pulse", "params": {}},
            {"method": "market.map", "params": {"board":"CN","metric":"auto","industry":"","watch_only":false,"aspect":1.6}},
            {"method": "freshness.list", "params": {"limit": 50}},
            {"method": "tushare.status", "params": {}},
            {"method": "tushare.sync", "params": {}},
            {"method": "tushare.bars", "params": {"symbol": "600519", "market": "CN.SH", "days": 400}},
            {"method": "tushare.quote", "params": {"symbol": "600519", "market": "CN.SH"}}
        ],
        "events": ["quote.updated", "ingest.done", "snapshot.available", "settings.changed", "job.updated", "stock.updated", "resync_required", "ping"]
    })
}
