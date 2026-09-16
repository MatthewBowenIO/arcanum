#!/usr/bin/env bash
# Build a arcanum AppImage.
#
# Usage:  scripts/build-appimage.sh [output.AppImage]
#
# Requires: cargo, curl, strip. Downloads linuxdeploy and appimagetool
# on first use (cached in /tmp or $XDG_CACHE_HOME).
set -euo pipefail

ROOT="$(readlink -f "$(dirname "$0")/..")"
VERSION="$(cd "$ROOT" && cargo read-manifest --format json 2>/dev/null | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' || echo dev)"
# linuxdeploy's embedded appimagetool mishandles relative output
# paths when APPIMAGE_EXTRACT_AND_RUN is in play — always absolute.
OUT="${1:-$ROOT/dist/arcanum_${VERSION}_x86_64.AppImage}"
APPDIR="$ROOT/target/appimage/AppDir"

# linuxdeploy neither creates the destination directory nor handles
# relative output paths — normalize before running it.
mkdir -p "$(dirname "$OUT")"
OUT="$(readlink -f "$OUT")"

cd "$ROOT"
cargo build --release
BIN="$ROOT/target/release/arcanum"

# ---------------------------------------------------------------- AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"
install -m755 "$BIN" "$APPDIR/usr/bin/arcanum"
install -m755 "$ROOT/packaging/AppRun" "$APPDIR/AppRun"
install -m644 "$ROOT/packaging/arcanum.desktop" \
              "$APPDIR/usr/share/applications/arcanum.desktop"
install -m644 "$ROOT/assets/icon.png" \
              "$APPDIR/usr/share/icons/hicolor/256x256/apps/arcanum.png"
install -m644 "$ROOT/packaging/arcanum.desktop" "$APPDIR/arcanum.desktop"
install -m644 "$ROOT/assets/icon.png" "$APPDIR/arcanum.png"

# ---------------------------------------------------------------- tools
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/arcanum-appimage"
mkdir -p "$CACHE"
fetch() { # fetch <url> <dest>
    [[ -f "$2" ]] || curl -LsSf "$1" -o "$2"
    chmod +x "$2"
}
fetch "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" \
      "$CACHE/appimagetool"
fetch "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" \
      "$CACHE/linuxdeploy"

export APPIMAGE_EXTRACT_AND_RUN=1   # needed inside some CI containers
NO_STRIP=1 "$CACHE/linuxdeploy" \
    --appdir "$APPDIR" \
    --desktop-file "$APPDIR/arcanum.desktop" \
    --icon-file "$ROOT/assets/icon.png" \
    --icon-filename arcanum \
    --output appimage \
    --appimagetool "$CACHE/appimagetool" || true

# linuxdeploy writes arcanum-x86_64.AppImage next to the AppDir
if [[ -f "$ROOT/target/appimage/arcanum-x86_64.AppImage" ]]; then
    mv "$ROOT/target/appimage/arcanum-x86_64.AppImage" "$OUT"
    echo "Built: $OUT"
else
    echo "linuxdeploy failed — falling back to plain appimagetool"
    ARCH=x86_64 "$CACHE/appimagetool" "$APPDIR" "$OUT"
    echo "Built: $OUT"
fi
