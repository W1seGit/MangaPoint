#!/usr/bin/env bash
# Build the website engine as WebAssembly, in the two flavours the site uses:
#
#   <out>/core    the small one the page loads (state file, chapter guessing,
#                 fingerprints, device list), for `--target web`
#   <out>/worker  the full one the Web Worker loads (adds the converter and the
#                 file writers), for `--target web`
#   <out>/node    the full one for Node (`--target nodejs`), used by the smoke test
#
# usage: build-wasm.sh [OUT_DIR]        (default: ./wasm-out next to this script)
# env:   CARGO_TARGET_DIR   where cargo builds (default: <workspace>/target)
#
# Needs: the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.129 (the
# version the crate pins). wasm-opt (binaryen) is used only if it is installed.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${1:-$here/wasm-out}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
wasm="$target_dir/wasm32-unknown-unknown/release/web_engine.wasm"

mkdir -p "$out"
out="$(cd "$out" && pwd)"

build() { # <label> <cargo feature args...>
  local label="$1"; shift
  # Small code for the page (it only does book-keeping); fast code for the worker.
  local level="s"; [ "$label" = core ] || level=3
  echo "== building $label (opt-level $level)"
  ( cd "$root" && \
    CARGO_PROFILE_RELEASE_OPT_LEVEL="$level" CARGO_PROFILE_RELEASE_LTO=true \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_STRIP=true \
    cargo build -p web-engine --target wasm32-unknown-unknown --release "$@" )
}

bindgen() { # <target> <dir>
  rm -rf "$out/$2"
  wasm-bindgen --target "$1" --out-dir "$out/$2" --out-name web_engine "$wasm"
  if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals \
      "$out/$2/web_engine_bg.wasm" -o "$out/$2/web_engine_bg.wasm.opt" && mv "$out/$2/web_engine_bg.wasm.opt" "$out/$2/web_engine_bg.wasm"
  fi
}

build core --no-default-features --features core
bindgen web core

build worker
bindgen web worker
bindgen nodejs node

echo
echo "== sizes (bytes)"
for d in core worker node; do
  f="$out/$d/web_engine_bg.wasm"
  printf '%-8s %10d  %s\n' "$d" "$(wc -c < "$f")" "$f"
done
