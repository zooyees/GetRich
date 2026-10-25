# Build a clean, self-contained Windows release archive.
param(
    [ValidatePattern('^\d{8}$')]
    [string]$Stamp = (Get-Date -Format "yyyyMMdd")
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) { $Root = (Get-Location).Path }
Set-Location $Root

$Metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw "Cannot read workspace version" }
$Version = ($Metadata.packages | Where-Object { $_.name -eq 'getrich-gui' }).version
if (-not $Version) { throw "Missing GUI version" }
$Dist = Join-Path $Root "dist"
$PackageName = "GetRich-Windows-$Version-$Stamp"
$Stage = Join-Path $Dist $PackageName
$Zip = Join-Path $Dist "$PackageName.zip"
$Icon = Join-Path $Root "icon\GetRich.ico"
$Example = Join-Path $Root "docs\examples\demo.board.json"
foreach ($Required in @($Icon, $Example, (Join-Path $Root "config.default.json"))) {
    if (-not (Test-Path -LiteralPath $Required -PathType Leaf)) {
        throw "Missing package input: $Required"
    }
}

Write-Host "Building release (getrich-gui, getrich-cli)..."
cargo build --locked --release -p getrich-gui -p getrich-cli
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Gui = Join-Path $Root "target\release\getrich-gui.exe"
$Cli = Join-Path $Root "target\release\getrich.exe"
foreach ($Binary in @($Gui, $Cli)) {
    if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
        throw "Build output missing: $Binary"
    }
}

New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$DistFull = [IO.Path]::GetFullPath($Dist).TrimEnd('\') + '\'
$StageFull = [IO.Path]::GetFullPath($Stage)
if (-not $StageFull.StartsWith($DistFull, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to clean a staging path outside dist: $StageFull"
}
if (Test-Path -LiteralPath $Stage) {
    $UserFiles = Get-ChildItem -LiteralPath $Stage -Recurse -File | Where-Object { $_.Name -eq 'config.json' -or $_.Extension -in @('.db','.sqlite','.sqlite3') }
    if ($UserFiles) { throw "Staging folder contains user data. Preserve it and use a different Stamp: $StageFull" }
    Remove-Item -LiteralPath $StageFull -Recurse -Force
}
if (Test-Path -LiteralPath $Zip) { Remove-Item -LiteralPath $Zip -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "examples") | Out-Null

Copy-Item -LiteralPath $Gui -Destination (Join-Path $Stage "GetRich.exe")
Copy-Item -LiteralPath $Cli -Destination (Join-Path $Stage "GetRich-CLI.exe")
Copy-Item -LiteralPath (Join-Path $Root "config.default.json") -Destination $Stage
Copy-Item -LiteralPath $Icon -Destination (Join-Path $Stage "GetRich.ico")
Copy-Item -LiteralPath $Example -Destination (Join-Path $Stage "examples\demo.board.json")
Copy-Item -LiteralPath (Join-Path $Root "docs\RELEASE_0.2.3.md") -Destination (Join-Path $Stage "RELEASE_NOTES.md")

Copy-Item -LiteralPath (Join-Path $Root "docs\UI_MARKET_ATLAS_2026-10-25.md") -Destination $Stage
Copy-Item -LiteralPath (Join-Path $Root "docs\UI_CORRECTIONS_0.2.1.md") -Destination $Stage

Compress-Archive -LiteralPath $Stage -DestinationPath $Zip -CompressionLevel Optimal
$Hash = (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash

Write-Host ""
Write-Host "Package: $Zip"
Write-Host "SHA256:  $Hash"
Write-Host "Run GetRich.exe, or GetRich-CLI.exe serve --bind 127.0.0.1:7878"
