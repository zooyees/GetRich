-- A single quote projection for every read path. Daily bars only promote a
-- quote when they represent a later session, or the quote is missing.
CREATE VIEW canonical_quotes AS
WITH candidates AS (
    SELECT l.id AS instrument_id, q.*, b.ts AS bar_ts,
           b.open AS bar_open, b.high AS bar_high, b.low AS bar_low,
           b.close AS bar_close, b.volume AS bar_volume,
           b.turnover AS bar_turnover, b.source AS bar_source,
           b.ingested_at AS bar_ingested_at, l.currency AS listing_currency,
           (SELECT close FROM bars previous WHERE previous.instrument_id=l.id AND previous.timeframe='1d' AND previous.ts<b.ts ORDER BY previous.ts DESC LIMIT 1) AS prior_bar_close,
           CASE WHEN b.id IS NOT NULL AND
             (q.instrument_id IS NULL OR date(b.ts) > date(q.ts, CASE WHEN l.market='US' THEN '-4 hours' ELSE '+8 hours' END))
             THEN 1 ELSE 0 END AS use_bar
    FROM listings l
    LEFT JOIN quotes q ON q.instrument_id = l.id
    LEFT JOIN bars b ON b.id = (
        SELECT id FROM bars WHERE instrument_id = l.id AND timeframe = '1d'
        ORDER BY ts DESC LIMIT 1
    )
)
SELECT instrument_id,
    CASE WHEN use_bar THEN bar_ts || 'T00:00:00Z' ELSE ts END AS ts,
    CASE WHEN use_bar THEN bar_close ELSE last END AS last,
    CASE WHEN use_bar THEN bar_open ELSE open END AS open,
    CASE WHEN use_bar THEN bar_high ELSE high END AS high,
    CASE WHEN use_bar THEN bar_low ELSE low END AS low,
    CASE WHEN use_bar THEN COALESCE(prior_bar_close,last) ELSE prev_close END AS prev_close,
    CASE WHEN use_bar THEN bar_volume ELSE volume END AS volume,
    CASE WHEN use_bar THEN bar_turnover ELSE turnover END AS turnover,
    CASE WHEN use_bar THEN CASE WHEN COALESCE(prior_bar_close,last) > 0 THEN (bar_close-COALESCE(prior_bar_close,last))*100.0/COALESCE(prior_bar_close,last) ELSE NULL END
         WHEN prev_close > 0 THEN (last-prev_close)*100.0/prev_close ELSE change_pct END AS change_pct,
    COALESCE(currency, listing_currency) AS currency,
    CASE WHEN use_bar THEN bar_source ELSE source END AS source,
    CASE WHEN use_bar THEN bar_ingested_at ELSE ingested_at END AS ingested_at
FROM candidates WHERE last IS NOT NULL OR use_bar = 1;

CREATE TABLE ingest_rejections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL,
    dataset TEXT NOT NULL,
    row_number INTEGER NOT NULL,
    symbol TEXT,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    job_key TEXT NOT NULL,
    params_json TEXT NOT NULL,
    status TEXT NOT NULL,
    phase TEXT NOT NULL,
    progress INTEGER NOT NULL DEFAULT 0,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    result_json TEXT,
    error TEXT,
    owner TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    heartbeat_at TEXT
);
CREATE UNIQUE INDEX jobs_active_key ON jobs(job_key)
    WHERE status IN ('queued','running','cancelling');
CREATE INDEX jobs_recent ON jobs(id DESC);

CREATE TABLE provider_requests(account TEXT NOT NULL,requested_at_ms INTEGER NOT NULL);
CREATE INDEX provider_requests_window ON provider_requests(account,requested_at_ms);
CREATE TABLE provider_backoff(account TEXT PRIMARY KEY,until_ms INTEGER NOT NULL);

CREATE TABLE app_state (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
INSERT INTO app_state(key,value) VALUES ('snapshot_revision',0),('settings_version',0);
ALTER TABLE pulse_snapshots ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
