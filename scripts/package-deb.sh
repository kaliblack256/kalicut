#!/usr/bin/env bash
# Build a fully self-contained .deb: kalicut + libmpv + ffmpeg/ffprobe.
# Host needs only a normal desktop (glibc / X11 or Wayland).
# Usage: ./scripts/package-deb.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "error: dpkg-deb not found (install dpkg-dev)" >&2
  exit 1
fi

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
PKG_NAME="kalicut"
DEB_DIR="$ROOT/dist/deb/${PKG_NAME}_${VERSION}_${ARCH}"
OUT_DEB="$ROOT/dist/${PKG_NAME}_${VERSION}_${ARCH}.deb"
LIBDIR_REL="usr/lib/kalicut"

echo "==> Building release binary"
"$ROOT/scripts/build.sh"

BIN="$ROOT/target/release/kalicut"
test -x "$BIN"

rm -rf "$DEB_DIR"
mkdir -p \
  "$DEB_DIR/DEBIAN" \
  "$DEB_DIR/usr/bin" \
  "$DEB_DIR/$LIBDIR_REL" \
  "$DEB_DIR/usr/share/applications" \
  "$DEB_DIR/usr/share/icons/hicolor/256x256/apps" \
  "$DEB_DIR/usr/share/icons/hicolor/128x128/apps" \
  "$DEB_DIR/usr/share/doc/$PKG_NAME"

# Real binary + bundled shared libraries (libmpv tree, etc.)
install -m 755 "$BIN" "$DEB_DIR/$LIBDIR_REL/kalicut"
echo "==> Bundling app libraries (libmpv, …)"
"$ROOT/scripts/bundle-libs.sh" "$DEB_DIR/$LIBDIR_REL/kalicut" "$DEB_DIR/$LIBDIR_REL"

echo "==> Bundling ffmpeg + ffprobe"
"$ROOT/scripts/bundle-ffmpeg.sh" "$DEB_DIR/$LIBDIR_REL" "$DEB_DIR/$LIBDIR_REL"

# /usr/bin/kalicut wrapper → private dir (PATH + LD_LIBRARY_PATH)
"$ROOT/scripts/make-wrapper.sh" \
  "$DEB_DIR/usr/bin/kalicut" \
  "/usr/lib/kalicut/kalicut" \
  "/usr/lib/kalicut" \
  absolute \
  "/usr/lib/kalicut"

install -m 644 "$ROOT/packaging/kalicut.desktop" "$DEB_DIR/usr/share/applications/kalicut.desktop"
install -m 644 "$ROOT/packaging/icons/kalicut.png" \
  "$DEB_DIR/usr/share/icons/hicolor/256x256/apps/kalicut.png"
if [[ -f "$ROOT/packaging/icons/kalicut-128.png" ]]; then
  install -m 644 "$ROOT/packaging/icons/kalicut-128.png" \
    "$DEB_DIR/usr/share/icons/hicolor/128x128/apps/kalicut.png"
fi
install -m 644 "$ROOT/README.md" "$DEB_DIR/usr/share/doc/$PKG_NAME/README.md"
install -m 644 "$ROOT/LICENSE" "$DEB_DIR/usr/share/doc/$PKG_NAME/copyright"
if [[ -f "$ROOT/docs/LINUX.md" ]]; then
  install -m 644 "$ROOT/docs/LINUX.md" "$DEB_DIR/usr/share/doc/$PKG_NAME/LINUX.md"
fi
cat >"$DEB_DIR/usr/share/doc/$PKG_NAME/THIRD_PARTY.txt" <<'EOF'
KALICUT ships bundled native libraries and tools under /usr/lib/kalicut/:
  - libmpv and dependencies
  - ffmpeg and ffprobe (copied from the build host)

Those components keep their upstream licenses (LGPL/GPL/etc. as applicable).
EOF

# Self-contained: no ffmpeg package dependency
cat >"$DEB_DIR/DEBIAN/control" <<EOF
Package: $PKG_NAME
Version: $VERSION
Section: video
Priority: optional
Architecture: $ARCH
Maintainer: KALICUT contributors <kalicut@local>
Depends: libc6
Recommends: libgtk-3-0 | libgtk-3-0t64
Homepage: https://github.com/kalicut/kalicut
Description: KALICUT — lossless audio/video cutting with ffmpeg
 GUI (Rust + egui) for cutting audio and video. Default mode is stream
 copy (-c copy) so quality matches the source. Optional re-encode presets.
 Bundles libmpv, ffmpeg and ffprobe — no extra packages required for cut/export.
EOF

cat >"$DEB_DIR/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true
fi
exit 0
EOF
chmod 755 "$DEB_DIR/DEBIAN/postinst"

SIZE_KB="$(du -sk "$DEB_DIR" | awk '{print $1}')"
if grep -q '^Installed-Size:' "$DEB_DIR/DEBIAN/control"; then
  sed -i "s/^Installed-Size:.*/Installed-Size: $SIZE_KB/" "$DEB_DIR/DEBIAN/control"
else
  echo "Installed-Size: $SIZE_KB" >>"$DEB_DIR/DEBIAN/control"
fi

mkdir -p "$ROOT/dist"
rm -f "$OUT_DEB"
dpkg-deb --root-owner-group --build "$DEB_DIR" "$OUT_DEB"

echo
echo "Created: $OUT_DEB"
ls -lh "$OUT_DEB"
echo "Bundled: libmpv + ffmpeg + ffprobe under /usr/lib/kalicut/"
echo "Depends: libc6 only"
echo
echo "Install:  sudo apt install ./$(basename "$OUT_DEB")"
