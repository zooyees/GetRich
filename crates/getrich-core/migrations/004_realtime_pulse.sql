-- Realtime pulse batches, durable snapshots, and alert audit state.

CREATE TABLE ingest_batches (
    version INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    requested INTEGER NOT NULL,
    accepted INTEGER NOT NULL,
    changed INTEGER NOT NULL,
    unchanged INTEGER NOT NULL,
    stale INTEGER NOT NULL,
    invalid INTEGER NOT NULL,
    missing_listing INTEGER NOT NULL,
    min_source_at TEXT,
    max_source_at TEXT,
    committed_at TEXT NOT NULL,
    detail_json TEXT
);

CREATE TABLE pulse_snapshots (
    trade_date TEXT NOT NULL,
    snapshot_version INTEGER NOT NULL,
    phase TEXT NOT NULL,
    source_at TEXT,
    computed_at TEXT NOT NULL,
    quality TEXT NOT NULL,
    regime_code TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (trade_date, snapshot_version)
);
CREATE INDEX idx_pulse_snapshots_latest
    ON pulse_snapshots(trade_date, snapshot_version DESC);

CREATE TABLE alert_rules (
    id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL,
    severity TEXT NOT NULL,
    scope TEXT NOT NULL,
    expression_json TEXT NOT NULL,
    hold_ms INTEGER NOT NULL,
    clear_hold_ms INTEGER NOT NULL,
    cooldown_ms INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE alert_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dedupe_key TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    state TEXT NOT NULL,
    severity TEXT NOT NULL,
    snapshot_version INTEGER NOT NULL,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    fired_at TEXT,
    cleared_at TEXT,
    acknowledged_at TEXT,
    evidence_json TEXT NOT NULL
);
CREATE INDEX idx_alert_events_active
    ON alert_events(state, last_seen_at DESC);
