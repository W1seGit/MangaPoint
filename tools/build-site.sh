#!/usr/bin/env bash
# Builds the home page (mangapoint.org) into apps/site/dist: any static host can serve that folder.
# Nothing is compiled: the page, its pictures, and the app's own fonts, colours and screenshots are copied
# next to each other. The mascot's pictures in apps/site/art are made from the app's drawings by
# `SITE_ART=write cargo test -p mangapoint-ui site_art`.
#
#   tools/build-site.sh
#   python tools/serve-web.py --dir apps/site/dist     to look at it
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/apps/site/dist"

rm -rf "$dist"
mkdir -p "$dist/art" "$dist/icons" "$dist/img"

cp "$root/apps/site/index.html" "$root/apps/site/site.css" "$root/apps/site/site.js" "$dist/"
cp "$root/apps/site/art/"*.svg "$dist/art/"
# The app's own style sheets: its fonts, and its colours and type.
cp "$root/crates/ui/css/fonts.css" "$root/crates/ui/css/base.css" "$dist/"
# The clouds and rocks of the app's Search page.
for name in search-cloud-1 search-cloud-2 search-rock-1 search-rock-2; do
  cp "$root/crates/ui/assets/svg/$name.svg" "$dist/art/"
done
cp "$root/apps/web/assets/icons/favicon-32.png" "$root/apps/web/assets/icons/apple-touch-icon.png" "$dist/icons/"
for name in library search-results series tasks read export reader-settings web-phone; do
  cp "$root/docs/screenshots/$name.jpg" "$dist/img/"
done
cp "$root/docs/screenshots/send-to.png" "$dist/img/"
cp "$root/docs/banner.png" "$dist/img/banner.png"

echo "built $dist"
