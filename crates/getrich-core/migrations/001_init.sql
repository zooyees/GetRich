-- Stock foundation schema: listings, bars, quotes, metadata, ingest bookkeeping.

CREATE TABLE listings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol TEXT NOT NULL,
    market TEXT NOT NULL,
    exchange TEXT,
    name TEXT,
    name_en TEXT,
    currency TEXT,
    instrument_type TEXT NOT NULL DEFAULT 'equity',
    isin TEXT,
    listing_date TEXT,
    delisting_date TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    sector TEXT,
    industry TEXT,
    lot_size INTEGER,
    source TEXT,
    source_symbol TEXT,
    extra_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(symbol, market)
);

CREATE INDEX idx_listings_market ON listings(market);
CREATE INDEX idx_listings_status ON listings(status);
CREATE INDEX idx_listings_name ON listings(name);
CREATE INDEX idx_listings_source_symbol ON listings(source_symbol);

CREATE TABLE bars (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
    timeframe TEXT NOT NULL,
    ts TEXT NOT NULL,
    open REAL NOT NULL,
    high REAL NOT NULL,
    low REAL NOT NULL,
    close REAL NOT NULL,
    adj_close REAL,
    volume REAL,
    turnover REAL,
    source TEXT NOT NULL,
    ingested_at TEXT NOT NULL,
    UNIQUE(instrument_id, timeframe, ts)
);

CREATE INDEX idx_bars_lookup ON bars(instrument_id, timeframe, ts);
CREATE INDEX idx_bars_source ON bars(source);

CREATE TABLE quotes (
    instrument_id INTEGER PRIMARY KEY REFERENCES listings(id) ON DELETE CASCADE,
    ts TEXT NOT NULL,
    last REAL NOT NULL,
    open REAL,
    high REAL,
    low REAL,
    prev_close REAL,
    volume REAL,
    change_pct REAL,
    currency TEXT,
    source TEXT NOT NULL,
    ingested_at TEXT NOT NULL
);

CREATE TABLE instrument_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT,
    source TEXT,
    as_of TEXT,
    updated_at TEXT NOT NULL,
    UNIQUE(instrument_id, key)
);

CREATE INDEX idx_metadata_key ON instrument_metadata(key);

CREATE TABLE ingest_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    instruments_upserted INTEGER NOT NULL DEFAULT 0,
    bars_upserted INTEGER NOT NULL DEFAULT 0,
    quotes_upserted INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    detail_json TEXT
);

CREATE INDEX idx_ingest_runs_started ON ingest_runs(started_at);

CREATE TABLE data_freshness (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
    dataset TEXT NOT NULL,
    source TEXT NOT NULL,
    last_attempt_at TEXT,
    last_success_at TEXT,
    watermark_ts TEXT,
    status TEXT NOT NULL DEFAULT 'unknown',
    error TEXT,
    UNIQUE(instrument_id, dataset, source)
);

CREATE INDEX idx_freshness_dataset ON data_freshness(dataset, status);
