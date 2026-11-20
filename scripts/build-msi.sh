#!/usr/bin/env bash
# Smart Mix - build the Windows MSI installer (per-user, no admin needed).
#
# Prerequisites (one time, no admin rights required):
#   - cargo-wix:            cargo install cargo-wix
#   - WiX v3 portable:      download wix314-binaries.zip from
#     https://github.com/wixtoolset/wix3/releases (tag wix3141rtm) and unzip
#     it to %LOCALAPPDATA%\wix314 (candle.exe + light.exe there), or set
#     WIX_BIN to wherever you unzipped it.
#
# See docs/RELEASE.md for the full story (signing decision, upgrade codes).
set -euo pipefail

cd "$(dirname "$0")/.."

WIX_BIN="${WIX_BIN:-$LOCALAPPDATA/wix314}"
VERSION="$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)"
MSI="target/wix/SmartMix-${VERSION}-win64.msi"

if [ ! -f "$WIX_BIN/candle.exe" ]; then
    echo "ERROR: candle.exe not found at $WIX_BIN" >&2
    echo "  Download wix314-binaries.zip from" >&2
    echo "  https://github.com/wixtoolset/wix3/releases (wix3141rtm) and unzip to $WIX_BIN" >&2
    echo "  (portable, no admin) or set WIX_BIN." >&2
    exit 1
fi

echo "==> [1/3] cargo build --release -p sm-app"
cargo build --release -p sm-app

echo "==> [2/3] staging models + icon next to the exe (target/release)"
mkdir -p target/release/models
cp models/energy_global.onnx models/energy_segment.onnx models/key_linear.json \
   models/key_model_v2.onnx models/key_model_v4.onnx models/key_model_v4_nomap.onnx \
   models/cue_ranker.json target/release/models/
cp assets/brand/app-icons/smart-mix.ico target/release/smart-mix.ico

echo "==> [3/3] cargo wix (candle + light) -> $MSI"
cargo wix -b "$WIX_BIN" --no-build -p sm-app -n SmartMix -o "$MSI"

echo
echo "OK: $MSI"
sha256sum "$MSI"
echo "Per-user install (no UAC):  msiexec /i '$MSI'"
echo "Uninstall:                  msiexec /x '$MSI'"
