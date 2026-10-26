#!/usr/bin/env bash
# Wrap a macOS build of MangaPoint in an app bundle and a disk image.
#
#   tools/package-macos.sh <binary> <version> <output .dmg>
#
# Makes MangaPoint.app (with the cat as its icon) and puts it in a .dmg beside a
# shortcut to Applications, so installing is dragging one onto the other. The app
# is not signed or notarized, so macOS asks for confirmation the first time it is
# opened (see the README).
set -euo pipefail

binary="$1"
version="$2"
dmg="$3"
root="$(cd "$(dirname "$0")/.." && pwd)"

stage="$(mktemp -d)"
app="$stage/MangaPoint.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/mangapoint"
chmod +x "$app/Contents/MacOS/mangapoint"
cp "$(cd "$(dirname "$0")/.." && pwd)/LICENSE" "$(cd "$(dirname "$0")/.." && pwd)/THIRD-PARTY-LICENSES.md" "$app/Contents/Resources/"

# The icon: the PNG at every size an .icns holds.
iconset="$(mktemp -d)/MangaPoint.iconset"
mkdir -p "$iconset"
source_png="$root/apps/desktop/assets/icon.png"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$source_png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$source_png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/MangaPoint.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>MangaPoint</string>
  <key>CFBundleDisplayName</key><string>MangaPoint</string>
  <key>CFBundleIdentifier</key><string>com.mangapoint.app</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleExecutable</key><string>mangapoint</string>
  <key>CFBundleIconFile</key><string>MangaPoint</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Seal the whole bundle, Info.plist included. The compiler only signs the bare
# program; with the plist outside that signature macOS calls a downloaded copy
# "damaged" and offers no way to open it. A sealed ad-hoc signature gives the
# usual "unidentified developer" prompt instead (see the README).
codesign --force --sign - "$app"
codesign --verify --strict "$app"

ln -s /Applications "$stage/Applications"
mkdir -p "$(dirname "$dmg")"
hdiutil create -volname "MangaPoint" -srcfolder "$stage" -ov -format UDZO "$dmg"
