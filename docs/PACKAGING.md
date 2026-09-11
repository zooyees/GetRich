# Windows packaging (WiParse-style)

Industrial pack for a Windows host with Rust + MSVC (or GNU) toolchain. This Linux VM cannot emit a PE `.exe`; run the script on Windows.

## Layout after pack

```
dist/
  GetRich.exe          # GUI workstation (windows_subsystem = windows)
  GetRich-CLI.exe      # JSON CLI
  config.default.json
  GetRich.ico
```

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
powershell -ExecutionPolicy Bypass -File packaging\pack-windows.ps1
```

Requires: Rust stable, `cargo` on PATH. The script builds `--release` for `getrich-gui` and `getrich-cli`, then copies artifacts into `dist\`.

Optional API sidecar:

```powershell
cargo build --release -p getrich-api
Copy-Item target\release\getrich-api.exe dist\GetRich-API.exe
```

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
