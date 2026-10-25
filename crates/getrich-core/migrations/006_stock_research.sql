CREATE TABLE stock_research (
    market TEXT NOT NULL,
    symbol TEXT NOT NULL,
    dataset TEXT NOT NULL,
    rows_json TEXT NOT NULL DEFAULT '[]',
    source TEXT NOT NULL,
    as_of TEXT,
    updated_at TEXT,
    attempted_at TEXT NOT NULL,
    error TEXT,
    PRIMARY KEY (market, symbol, dataset)
);
