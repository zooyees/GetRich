CREATE TABLE research_journal (
 market TEXT NOT NULL, symbol TEXT NOT NULL, revision INTEGER NOT NULL,
 thesis TEXT NOT NULL, risks TEXT NOT NULL, plan TEXT NOT NULL, review TEXT NOT NULL,
 updated_at TEXT NOT NULL, analysis_as_of TEXT, analysis_snapshot TEXT NOT NULL,
 PRIMARY KEY(market,symbol,revision)
);
