ALTER TABLE research_journal RENAME TO research_journal_v8;
CREATE TABLE research_journal (
 market TEXT NOT NULL, symbol TEXT NOT NULL,
 mode TEXT NOT NULL CHECK(mode IN ('intraday','swing','value')),
 revision INTEGER NOT NULL,
 thesis TEXT NOT NULL, risks TEXT NOT NULL, plan TEXT NOT NULL, review TEXT NOT NULL,
 updated_at TEXT NOT NULL, analysis_as_of TEXT, analysis_snapshot TEXT NOT NULL,
 PRIMARY KEY(market,symbol,mode,revision)
);
INSERT INTO research_journal SELECT market,symbol,'swing',revision,thesis,risks,plan,review,updated_at,analysis_as_of,analysis_snapshot FROM research_journal_v8;
DROP TABLE research_journal_v8;
