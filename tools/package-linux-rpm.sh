#!/usr/bin/env bash
# Make an .rpm of the Linux build of MangaPoint (Fedora, openSUSE, RHEL and kin).
#
#   tools/package-linux-rpm.sh <binary> <version> <output .rpm>
#
# The libraries it needs are named by what they provide (a shared library
# file), not by package, so it installs the same way on every distribution.
set -euo pipefail

binary="$(realpath "$1")"
version="$2"
out="$3"
root="$(cd "$(dirname "$0")/.." && pwd)"
top="$(mktemp -d)"
mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

cat > "$top/SPECS/mangapoint.spec" <<SPEC
%define debug_package %{nil}
%global __os_install_post %{nil}
%define _build_id_links none
AutoReqProv: no

Name:           mangapoint
Version:        ${version}
Release:        1
Summary:        The easiest way to get manga onto your e-reader
License:        MIT
URL:            https://github.com/W1seGit/MangaPoint
BuildArch:      x86_64
Requires:       libwebkit2gtk-4.1.so.0()(64bit)
Requires:       libjavascriptcoregtk-4.1.so.0()(64bit)
Requires:       libgtk-3.so.0()(64bit)
Requires:       libsoup-3.0.so.0()(64bit)
Requires:       libxdo.so.3()(64bit)
Requires:       libdbus-1.so.3()(64bit)
Requires:       libssl.so.3()(64bit)

%description
Search, download, crop and convert manga in one app, then send it to your
Xteink, Kindle, Kobo, Boox or other e-ink reader.

%install
install -D -m 0755 "${binary}" %{buildroot}/usr/bin/mangapoint
install -D -m 0644 "${root}/apps/desktop/assets/icon.png" %{buildroot}/usr/share/icons/hicolor/256x256/apps/mangapoint.png
install -D -m 0644 "${root}/LICENSE" %{buildroot}/usr/share/licenses/mangapoint/LICENSE
install -D -m 0644 "${root}/THIRD-PARTY-LICENSES.md" %{buildroot}/usr/share/licenses/mangapoint/THIRD-PARTY-LICENSES.md
mkdir -p %{buildroot}/usr/share/applications
cat > %{buildroot}/usr/share/applications/mangapoint.desktop <<DESKTOP
[Desktop Entry]
Type=Application
Name=MangaPoint
Comment=The easiest way to get manga onto your e-reader
Exec=mangapoint
Icon=mangapoint
Categories=Graphics;Viewer;
Terminal=false
DESKTOP

%files
/usr/bin/mangapoint
/usr/share/icons/hicolor/256x256/apps/mangapoint.png
/usr/share/applications/mangapoint.desktop
%license /usr/share/licenses/mangapoint/LICENSE
%license /usr/share/licenses/mangapoint/THIRD-PARTY-LICENSES.md
SPEC

rpmbuild -bb --quiet --define "_topdir $top" "$top/SPECS/mangapoint.spec"
mkdir -p "$(dirname "$out")"
cp "$top"/RPMS/x86_64/*.rpm "$out"
