CREATE TABLE IF NOT EXISTS pulse_anchors (
    trade_date TEXT NOT NULL,
    phase TEXT NOT NULL,
    amount REAL,
    median_pct REAL,
    payload_json TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (trade_date, phase)
);

CREATE TABLE IF NOT EXISTS pulse_vendor (
    dataset TEXT NOT NULL,
    as_of TEXT NOT NULL,
    vintage TEXT,
    status TEXT NOT NULL DEFAULT 'ok',
    payload_json TEXT,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (dataset, as_of)
);
