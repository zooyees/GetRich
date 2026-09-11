# Changelog

## 0.1.0

- WiParse-style workspace: `getrich-core` / `getrich-cli` / `getrich-api`
- SQLite foundation: listings, quotes, bars, metadata, ingest_runs, data_freshness, migrations
- File import contract (JSON/CSV), idempotent upserts; no live scraping
- Tonghuashun-style workstation: quote board, detail, K-line, empty/loading/error
- Apple-minimal home / shell chrome (web + egui); quote columns stay Tonghuashun
- JSON CLI + `/v1/invoke` + REST read path with DB busy retries
- Windows pack (`packaging/pack-windows.ps1` → `dist/GetRich.exe`) and WiParse-style `GetRich.ico` wiring
