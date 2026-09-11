# Pack Windows release (GetRich.exe + GetRich-CLI.exe)
# WiParse-style: cargo --release, rename GUI binary, copy config + icon.
# Run from repo root or from packaging\:  powershell -ExecutionPolicy Bypass -File packaging\pack-windows.ps1

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) { $Root = (Get-Location).Path }
Set-Location $Root

$Icon = Join-Path $Root "icon\GetRich.ico"
if (-not (Test-Path $Icon)) {
    Write-Error "Missing icon\GetRich.ico. Place the official ICO at icon\GetRich.ico (e.g. copy from D:\windlink\windlink\GetRich\icon\GetRich.ico) and re-run."
}

Write-Host "Building release (getrich-gui, getrich-cli)..."
cargo build --release -p getrich-gui -p getrich-cli
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Dist = Join-Path $Root "dist"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

$Gui = Join-Path $Root "target\release\getrich-gui.exe"
$Cli = Join-Path $Root "target\release\getrich.exe"
if (-not (Test-Path $Gui)) { Write-Error "Build output missing: $Gui" }
if (-not (Test-Path $Cli)) { Write-Error "Build output missing: $Cli" }

Copy-Item -Force $Gui (Join-Path $Dist "GetRich.exe")
Copy-Item -Force $Cli (Join-Path $Dist "GetRich-CLI.exe")
Copy-Item -Force (Join-Path $Root "config.default.json") (Join-Path $Dist "config.default.json")
Copy-Item -Force $Icon (Join-Path $Dist "GetRich.ico")

Write-Host ""
Write-Host "Packed:"
Get-ChildItem $Dist | ForEach-Object { Write-Host ("  {0}" -f $_.FullName) }
Write-Host ""
Write-Host "Run GetRich.exe (GUI) or GetRich-CLI.exe serve --bind 127.0.0.1:7878"
