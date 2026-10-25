# Windows packaging (WiParse-style)

Build the Windows archive on a Windows host with Rust + MSVC (or GNU) toolchain.

## Layout after pack

```
dist/
  GetRich-Windows-0.2.3-YYYYMMDD/
    GetRich.exe          # GUI workstation (windows_subsystem = windows)
    GetRich-CLI.exe      # JSON CLI and localhost workstation
    config.default.json
    GetRich.ico
    examples/demo.board.json
    RELEASE_NOTES.md
    UI_MARKET_ATLAS_2026-10-25.md
    UI_CORRECTIONS_0.2.1.md
  GetRich-Windows-0.2.3-YYYYMMDD.zip
```

Do **not** copy a filled-in `config.json` into `dist/` (it may contain a Tushare token). Use `GETRICH_TUSHARE_TOKEN` or copy `config.default.json` locally. If an older pack already shipped a token, rotate it.

## Icon

Canonical source: `icon/GetRich.ico`.

WiParse wiring (already in `getrich-gui`):

1. `include_bytes!("../../../icon/GetRich.ico")` for the egui window icon
2. `ViewportBuilder::with_icon` + `ViewportCommand::Icon`
3. `build.rs` + `winresource` embeds PE resource id 1
4. `windows_icon.rs`: AppUserModelID `YuZhao.GetRich`, `WM_SETICON` / `GCLP_HICON`

If you have the official asset at `D:\windlink\windlink\GetRich\icon\GetRich.ico`, copy it over the repo file before packing so Explorer / taskbar / window all use the same ICO.

## Windows

From the repo root (PowerShell):

```powershell
.\packaging\pack-windows.ps1
```

Requires: Rust stable, `cargo` on PATH. The script uses the lockfile, creates a clean dated staging directory, includes an importable demo, writes a ZIP, and prints its SHA-256 hash. It never copies `config.json`, databases, or logs.

`GetRich-CLI.exe serve` contains the browser workstation, so a separate API executable is unnecessary.

For 0.2.3, `RELEASE_NOTES.md` is copied from `docs/RELEASE_0.2.3.md`. This version retains schema 6 for stock research caches. Close the old application, back up its configuration and database, then copy them to the new version directory before launching. Use the original backup to roll back; older applications cannot read a schema 6 database.

## Linux / this VM (closest runnable artifact)

```bash
bash packaging/pack.sh
```

Produces `dist/GetRich`, `dist/GetRich-CLI`, `dist/getrich-api`, plus `config.default.json` and `GetRich.ico`.

Run GUI: `./dist/GetRich`  
Run workstation in a browser: `./dist/getrich-api` then open `http://127.0.0.1:7878/`

## Dev run (no pack)

```bash
cargo run -p getrich-cli -- db migrate
cargo run -p getrich-cli -- serve --bind 127.0.0.1:7878
# or
cargo run -p getrich-gui
```
