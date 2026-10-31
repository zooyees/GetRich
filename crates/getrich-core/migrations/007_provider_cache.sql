-- Raw successful responses survive restarts; credentials are never stored here.
CREATE TABLE provider_cache (
    cache_key TEXT PRIMARY KEY,
    response_json TEXT NOT NULL,
    fetched_ms INTEGER NOT NULL,
    expires_ms INTEGER NOT NULL
);
CREATE TABLE provider_history (
    scope TEXT NOT NULL, trade_date TEXT NOT NULL, row_json TEXT NOT NULL,
    PRIMARY KEY(scope, trade_date)
);
CREATE TABLE provider_coverage (
    scope TEXT NOT NULL, from_date TEXT NOT NULL, to_date TEXT NOT NULL,
    PRIMARY KEY(scope, from_date, to_date)
);
