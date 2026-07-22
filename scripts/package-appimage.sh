#!/usr/bin/env bash
# Build AppImage with libmpv + ffmpeg/ffprobe bundled (fully self-contained).
# Usage: ./scripts/package-appimage.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *)
    echo "error: unsupported arch: $ARCH" >&2
    exit 1
    ;;
esac

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
TOOLS="$ROOT/dist/tools"
APPDIR="$ROOT/dist/AppDir"
OUT_NAME="KALICUT-${VERSION}-${ARCH}.AppImage"
OUT_PATH="$ROOT/dist/$OUT_NAME"

mkdir -p "$TOOLS" "$ROOT/dist"

download() {
  local url="$1" dest="$2"
  if [[ -x "$dest" ]]; then
    return 0
  fi
  echo "==> Downloading $(basename "$dest")"
  curl -fsSL -o "$dest" "$url"
  chmod +x "$dest"
}

LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage"

download "$LINUXDEPLOY_URL" "$TOOLS/linuxdeploy-${ARCH}.AppImage"
download "$APPIMAGETOOL_URL" "$TOOLS/appimagetool-${ARCH}.AppImage"

echo "==> Building release binary"
"$ROOT/scripts/build.sh"

BIN="$ROOT/target/release/kalicut"
test -x "$BIN"

rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/lib" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
  "$APPDIR/usr/share/metainfo"

install -m 755 "$BIN" "$APPDIR/usr/bin/kalicut"
install -m 644 "$ROOT/packaging/kalicut.desktop" "$APPDIR/usr/share/applications/kalicut.desktop"
install -m 644 "$ROOT/packaging/icons/kalicut.png" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps/kalicut.png"
cp "$ROOT/packaging/icons/kalicut.png" "$APPDIR/kalicut.png"

cat >"$APPDIR/usr/share/metainfo/kalicut.appdata.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>kalicut</id>
  <name>KALICUT</name>
  <summary>Lossless audio/video cutting with ffmpeg</summary>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <description>
    <p>
      KALICUT is a small GUI to cut audio and video with ffmpeg.
      libmpv, ffmpeg and ffprobe are bundled — fully self-contained.
    </p>
  </description>
  <launchable type="desktop-id">kalicut.desktop</launchable>
  <provides>
    <binary>kalicut</binary>
  </provides>
</component>
EOF

# AppRun: bundled libs + bundled ffmpeg first
cat >"$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="$HERE/usr/bin${PATH:+:$PATH}"
export LD_LIBRARY_PATH="$HERE/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$HERE/usr/bin/kalicut" "$@"
EOF
chmod 755 "$APPDIR/AppRun"

export ARCH
export APPIMAGE_EXTRACT_AND_RUN=1

echo "==> linuxdeploy (bundle libraries for kalicut)"
"$TOOLS/linuxdeploy-${ARCH}.AppImage" \
  --appimage-extract-and-run \
  --appdir "$APPDIR" \
  --executable "$APPDIR/usr/bin/kalicut" \
  --desktop-file "$APPDIR/usr/share/applications/kalicut.desktop" \
  --icon-file "$APPDIR/kalicut.png" \
  || echo "warning: linuxdeploy reported issues — filling gaps with bundle-libs" >&2

echo "==> Extra library pass"
"$ROOT/scripts/bundle-libs.sh" "$APPDIR/usr/bin/kalicut" "$APPDIR/usr/lib"

echo "==> Bundling ffmpeg + ffprobe into AppDir"
"$ROOT/scripts/bundle-ffmpeg.sh" "$APPDIR/usr/bin" "$APPDIR/usr/lib"

sed -i 's|^Exec=.*|Exec=kalicut|' "$APPDIR/usr/share/applications/kalicut.desktop" || true

echo "==> appimagetool"
rm -f "$OUT_PATH"
"$TOOLS/appimagetool-${ARCH}.AppImage" \
  --appimage-extract-and-run \
  "$APPDIR" \
  "$OUT_PATH"

echo
echo "Created: $OUT_PATH"
ls -lh "$OUT_PATH"
echo "Bundled: libmpv + ffmpeg + ffprobe"
