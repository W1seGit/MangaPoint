#!/usr/bin/env bash
# Make a .deb of the Linux build of MangaPoint.
#
#   tools/package-linux-deb.sh <binary> <version> <output .deb>
#
# Installs the program, a menu entry and the icon. The libraries it needs
# (the system's web view and GTK) are named as dependencies, so installing with
# `sudo apt install ./MangaPoint-….deb` brings them in.
set -euo pipefail

binary="$1"
version="$2"
out="$3"
root="$(cd "$(dirname "$0")/.." && pwd)"

stage="$(mktemp -d)/mangapoint_${version}_amd64"
mkdir -p "$stage/DEBIAN" "$stage/usr/bin" \
  "$stage/usr/share/applications" "$stage/usr/share/icons/hicolor/256x256/apps"

install -m 0755 "$binary" "$stage/usr/bin/mangapoint"
mkdir -p "$stage/usr/share/doc/mangapoint"
install -m 0644 "$root/LICENSE" "$root/THIRD-PARTY-LICENSES.md" "$stage/usr/share/doc/mangapoint/"
install -m 0644 "$root/apps/desktop/assets/icon.png" \
  "$stage/usr/share/icons/hicolor/256x256/apps/mangapoint.png"

cat > "$stage/usr/share/applications/mangapoint.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=MangaPoint
Comment=The easiest way to get manga onto your e-reader
Exec=mangapoint
Icon=mangapoint
Categories=Graphics;Viewer;
Terminal=false
DESKTOP

cat > "$stage/DEBIAN/control" <<CONTROL
Package: mangapoint
Version: ${version}
Section: graphics
Priority: optional
Architecture: amd64
Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libxdo3, libdbus-1-3, libssl3 | libssl3t64
Recommends: libayatana-appindicator3-1
Maintainer: MangaPoint contributors <https://github.com/W1seGit/MangaPoint>
Homepage: https://github.com/W1seGit/MangaPoint
Description: The easiest way to get manga onto your e-reader
 Search, download, crop and convert manga in one app, then send it to your
 Xteink, Kindle, Kobo, Boox or other e-ink reader.
CONTROL

mkdir -p "$(dirname "$out")"
dpkg-deb --build --root-owner-group "$stage" "$out"
