#!/usr/bin/env bash
# Closest runnable artifact on Linux: native binaries in dist/.
# Windows PE output requires packaging/pack-windows.ps1 on a Windows host.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ICON="$ROOT/icon/GetRich.ico"
if [[ ! -f "$ICON" ]]; then
  echo "Missing icon/GetRich.ico" >&2
  exit 1
fi

echo "Building release (getrich-gui, getrich-cli, getrich-api)..."
cargo build --release -p getrich-gui -p getrich-cli -p getrich-api

DIST="$ROOT/dist"
mkdir -p "$DIST"

copy_bin() {
  local src="$1"
  local dest="$2"
  if [[ ! -f "$src" ]]; then
    echo "Build output missing: $src" >&2
    exit 1
  fi
  cp -f "$src" "$dest"
  chmod +x "$dest"
}

copy_bin "$ROOT/target/release/getrich-gui" "$DIST/GetRich"
copy_bin "$ROOT/target/release/getrich" "$DIST/GetRich-CLI"
copy_bin "$ROOT/target/release/getrich-api" "$DIST/getrich-api"
cp -f "$ROOT/config.default.json" "$DIST/config.default.json"
cp -f "$ICON" "$DIST/GetRich.ico"

echo
echo "Packed:"
ls -l "$DIST"
echo
echo "GUI:  $DIST/GetRich"
echo "CLI:  $DIST/GetRich-CLI"
echo "API:  $DIST/getrich-api   →  http://127.0.0.1:7878/"
