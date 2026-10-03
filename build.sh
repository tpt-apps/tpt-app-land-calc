#!/usr/bin/env bash
# TPT Land Calc - build pipeline (POSIX mirror of build.ps1).
#
# Free edition (default):  ./build.sh
#   cargo build --release --target wasm32-unknown-unknown (no pro feature),
#   wasm-bindgen --target web -> $TPT_WEB_REPO/public/apps/land-calc/ when
#   TPT_WEB_REPO is set, otherwise dist/web.
# Pro desktop edition:     ./build.sh --pro
#   trunk build --release --features pro (into dist/) + cargo build the
#   native webview exe. Never touches the free bundle destination.
set -euo pipefail
cd "$(dirname "$0")"

CRATE=tpt-app-land-calc
TARGET=wasm32-unknown-unknown
OUT_NAME=land-calc
DEFAULT_WEB_REPO="$HOME/Programming/2 WIP/TPT Electrician/tpt-electrician-nz-2"

if [[ "${1:-}" == "--pro" ]]; then
  echo "edition: pro (multi-parcel + buffer/overlay + exports)"
  trunk build --release --features pro
  cargo build --release -p tpt-land-desktop
  echo "exe: target/release/tpt-land-calc-pro (serves dist/)"
  exit 0
fi

WEB_REPO="${TPT_WEB_REPO:-}"
if [[ -z "$WEB_REPO" && -d "$DEFAULT_WEB_REPO" ]]; then
  WEB_REPO="$DEFAULT_WEB_REPO"
fi
if [[ -n "$WEB_REPO" ]]; then
  DEST="$WEB_REPO/public/apps/land-calc"
else
  DEST="dist/web"
fi
echo "edition: free (single-parcel area)"
echo "dest:    $DEST"

cargo build --release --target "$TARGET" -p "$CRATE"
WASM="target/$TARGET/release/tpt_app_land_calc.wasm"
mkdir -p "$DEST"
wasm-bindgen --target web --out-name "$OUT_NAME" --out-dir "$DEST" "$WASM"

RAW=$(wc -c < "$DEST/${OUT_NAME}_bg.wasm" | tr -d ' ')
GZ=$(gzip -c "$DEST/${OUT_NAME}_bg.wasm" | wc -c | tr -d ' ')
echo "${OUT_NAME}_bg.wasm: $RAW bytes raw, $GZ bytes gzipped"
echo "-- done"
