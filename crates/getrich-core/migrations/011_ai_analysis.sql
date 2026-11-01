CREATE TABLE ai_analyses (
    id INTEGER PRIMARY KEY,
    market TEXT NOT NULL,
    symbol TEXT NOT NULL,
    mode TEXT NOT NULL,
    basis TEXT NOT NULL,
    model TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    report_json TEXT,
    status TEXT NOT NULL CHECK(status IN ('running','succeeded','failed')),
    error TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);
CREATE INDEX ai_analysis_lookup ON ai_analyses(market,symbol,mode,basis,id DESC);
