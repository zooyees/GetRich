CREATE TABLE professional_facts (
 id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, market TEXT NOT NULL,
 symbol TEXT NOT NULL, day TEXT NOT NULL, identity TEXT NOT NULL DEFAULT '',
 source TEXT NOT NULL, observed_at TEXT NOT NULL, payload TEXT NOT NULL
);
CREATE INDEX professional_facts_lookup ON professional_facts(kind,market,symbol,day,id DESC);
CREATE TABLE professional_definitions (
 kind TEXT NOT NULL, name TEXT NOT NULL, revision INTEGER NOT NULL,
 payload TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(kind,name,revision)
);
CREATE TABLE portfolio_entries (
 id INTEGER PRIMARY KEY AUTOINCREMENT, account TEXT NOT NULL, reference TEXT NOT NULL,
 currency TEXT NOT NULL, day TEXT NOT NULL, kind TEXT NOT NULL, market TEXT NOT NULL,
 symbol TEXT NOT NULL, quantity REAL NOT NULL, price REAL NOT NULL, cash REAL NOT NULL,
 fees REAL NOT NULL, memo TEXT NOT NULL, recorded_at TEXT NOT NULL,
 UNIQUE(account,reference)
);
CREATE INDEX portfolio_entries_date ON portfolio_entries(account,day,id);
CREATE TABLE portfolio_corrections(id INTEGER PRIMARY KEY AUTOINCREMENT,account TEXT NOT NULL,reference TEXT NOT NULL,voided INTEGER NOT NULL,reason TEXT NOT NULL,recorded_at TEXT NOT NULL);
CREATE TABLE professional_alert_state (
 rule TEXT PRIMARY KEY, revision INTEGER NOT NULL, active INTEGER NOT NULL DEFAULT 0,
 fired_at TEXT, checked_at TEXT NOT NULL
);
CREATE TABLE professional_alert_events (
 id INTEGER PRIMARY KEY AUTOINCREMENT, rule TEXT NOT NULL, revision INTEGER NOT NULL,
 event_key TEXT NOT NULL, occurred_at TEXT NOT NULL, payload TEXT NOT NULL,
 acknowledged INTEGER NOT NULL DEFAULT 0, UNIQUE(rule,revision,event_key)
);
CREATE TABLE strategy_runs (
 id INTEGER PRIMARY KEY AUTOINCREMENT, created_at TEXT NOT NULL,
 fingerprint TEXT NOT NULL, inputs TEXT NOT NULL, result TEXT NOT NULL
);
CREATE TABLE bar_revisions (
 id INTEGER PRIMARY KEY AUTOINCREMENT, instrument_id INTEGER NOT NULL,
 timeframe TEXT NOT NULL, ts TEXT NOT NULL, observed_at TEXT NOT NULL,
 payload TEXT NOT NULL
);
CREATE INDEX bar_revisions_lookup ON bar_revisions(instrument_id,timeframe,ts,id DESC);
INSERT INTO bar_revisions(instrument_id,timeframe,ts,observed_at,payload)
 SELECT instrument_id,timeframe,ts,strftime('%Y-%m-%dT%H:%M:%fZ','now'),
 json_object('open',open,'high',high,'low',low,'close',close,'volume',volume,'turnover',turnover,'source',source)
 FROM bars;
CREATE TRIGGER bar_revision_insert AFTER INSERT ON bars BEGIN
 INSERT INTO bar_revisions(instrument_id,timeframe,ts,observed_at,payload)
 VALUES(NEW.instrument_id,NEW.timeframe,NEW.ts,strftime('%Y-%m-%dT%H:%M:%fZ','now'),
 json_object('open',NEW.open,'high',NEW.high,'low',NEW.low,'close',NEW.close,'volume',NEW.volume,'turnover',NEW.turnover,'source',NEW.source));
END;
CREATE TRIGGER bar_revision_update AFTER UPDATE ON bars
WHEN (NEW.open IS NOT OLD.open OR NEW.high IS NOT OLD.high OR NEW.low IS NOT OLD.low
 OR NEW.close IS NOT OLD.close OR NEW.volume IS NOT OLD.volume OR NEW.turnover IS NOT OLD.turnover
 OR NEW.source IS NOT OLD.source)
 AND (NEW.source NOT LIKE '%rt_k%' OR NEW.timeframe<>'1d' OR date(NEW.ts)<>date('now','+8 hours')
 OR COALESCE((SELECT substr(observed_at,1,16) FROM bar_revisions WHERE instrument_id=NEW.instrument_id AND timeframe=NEW.timeframe AND ts=NEW.ts ORDER BY id DESC LIMIT 1),'')<>strftime('%Y-%m-%dT%H:%M','now'))
BEGIN
 INSERT INTO bar_revisions(instrument_id,timeframe,ts,observed_at,payload)
 VALUES(NEW.instrument_id,NEW.timeframe,NEW.ts,strftime('%Y-%m-%dT%H:%M:%fZ','now'),
 json_object('open',NEW.open,'high',NEW.high,'low',NEW.low,'close',NEW.close,'volume',NEW.volume,'turnover',NEW.turnover,'source',NEW.source));
END;
CREATE TABLE quote_samples (
 market TEXT NOT NULL,symbol TEXT NOT NULL,source TEXT NOT NULL,bucket TEXT NOT NULL,
 ts TEXT NOT NULL,last REAL NOT NULL,volume REAL,turnover REAL,currency TEXT,
 PRIMARY KEY(market,symbol,source,bucket)
);
CREATE INDEX quote_samples_clock ON quote_samples(julianday(ts));
CREATE TRIGGER quote_sample_insert AFTER INSERT ON quotes BEGIN
 INSERT INTO quote_samples SELECT l.market,l.symbol,NEW.source,substr(NEW.ts,1,16),NEW.ts,NEW.last,NEW.volume,NEW.turnover,COALESCE(NEW.currency,l.currency) FROM listings l WHERE l.id=NEW.instrument_id
 ON CONFLICT(market,symbol,source,bucket) DO NOTHING;
END;
CREATE TRIGGER quote_sample_update AFTER UPDATE ON quotes
WHEN substr(NEW.ts,1,16)<>substr(OLD.ts,1,16) OR NEW.source IS NOT OLD.source
BEGIN
 INSERT INTO quote_samples SELECT l.market,l.symbol,NEW.source,substr(NEW.ts,1,16),NEW.ts,NEW.last,NEW.volume,NEW.turnover,COALESCE(NEW.currency,l.currency) FROM listings l WHERE l.id=NEW.instrument_id
 ON CONFLICT(market,symbol,source,bucket) DO NOTHING;
END;
