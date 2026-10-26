#!/usr/bin/env bash
# Builds the website into web/dist (any static host can serve that folder).
#
#   tools/build-web.sh              the page + the converter
#   tools/build-web.sh --no-engine  only the page (no export; faster)
#   CARGO_TARGET_DIR=...            where cargo builds (default: ./target)
#
# Needs: Rust with the wasm32-unknown-unknown target, wasm-bindgen-cli 0.2.129
# (cargo install wasm-bindgen-cli --version 0.2.129), and Node 18 or newer.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
engine=1
for arg in "$@"; do
  case "$arg" in
    --no-engine) engine=0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

target="${CARGO_TARGET_DIR:-$root/target}"
dist="$root/web/dist"

want="0.2.129"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
if [ "$have" != "$want" ]; then
  echo "wasm-bindgen $want is needed (found: ${have:-none}). Install it with: cargo install wasm-bindgen-cli --version $want" >&2
  exit 1
fi

rm -rf "$dist"
mkdir -p "$dist/css" "$dist/pkg"

echo "== page (Rust, Dioxus) =="
cargo build -p mangapoint-web --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir "$dist/pkg" --out-name mangapoint_web \
  "$target/wasm32-unknown-unknown/release/mangapoint_web.wasm"

if [ "$engine" = 1 ]; then
  echo "== converter (Web Worker) =="
  cargo build -p web-engine --target wasm32-unknown-unknown --release
  mkdir -p "$dist/engine"
  wasm-bindgen --target web --no-typescript --out-dir "$dist/engine" --out-name web_engine \
    "$target/wasm32-unknown-unknown/release/web_engine.wasm"
fi

# Smaller downloads when binaryen is installed.
if command -v wasm-opt >/dev/null 2>&1; then
  for f in "$dist"/pkg/*_bg.wasm "$dist"/engine/*_bg.wasm; do
    [ -f "$f" ] && wasm-opt -O2 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o "$f.opt" "$f" && mv "$f.opt" "$f" || true
  done
fi

echo "== site files =="
cp "$root/web/index.html" "$root/web/manifest.webmanifest" "$dist/"
# Hosts that show a page for unknown addresses (GitHub Pages) show the app at /export and the rest.
cp "$root/web/index.html" "$dist/404.html"
cp -r "$root/web/js" "$dist/js"
rm -rf "$dist/js/test"
cp -r "$root/web/icons" "$dist/icons"
# The style sheets, each folder's files in the order of its ORDER file: css/ui.css is the look shared
# with the desktop app (crates/ui/css), css/web.css what a browser and a phone add (web/css).
join_css() {
  : > "$2"
  while IFS= read -r name; do
    [ -n "$name" ] && { cat "$1/$name"; printf '\n'; } >> "$2"
  done < "$1/ORDER"
}
join_css "$root/crates/ui/css" "$dist/css/ui.css"
join_css "$root/web/css" "$dist/css/web.css"

# The page scripts shared with the desktop app (crates/ui/js), loaded by name from js/ui/.
mkdir -p "$dist/js/ui"
cp "$root"/crates/ui/js/*.js "$dist/js/ui/"

node "$root/tools/web-precache.mjs" "$dist"
echo "built: $dist"
