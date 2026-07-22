#!/usr/bin/env bash
# Portable folder + .tar.gz: kalicut + libmpv + ffmpeg/ffprobe (fully offline-ready).
# Usage: ./scripts/package-portable.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
esac

NAME="kalicut-${VERSION}-linux-${ARCH}"
OUT_DIR="$ROOT/dist/$NAME"
OUT_TAR="$ROOT/dist/${NAME}.tar.gz"

echo "==> Building release binary"
"$ROOT/scripts/build.sh"
BIN="$ROOT/target/release/kalicut"
test -x "$BIN"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/lib" "$OUT_DIR/share/icons" "$OUT_DIR/share/applications"

install -m 755 "$BIN" "$OUT_DIR/kalicut.bin"
echo "==> Bundling app libraries"
"$ROOT/scripts/bundle-libs.sh" "$OUT_DIR/kalicut.bin" "$OUT_DIR/lib"

echo "==> Bundling ffmpeg + ffprobe"
"$ROOT/scripts/bundle-ffmpeg.sh" "$OUT_DIR" "$OUT_DIR/lib"

# PATH=HERE so ./ffmpeg is found; libs in ./lib
"$ROOT/scripts/make-wrapper.sh" "$OUT_DIR/kalicut" "kalicut.bin" "lib" relative "."

install -m 644 "$ROOT/packaging/kalicut.desktop" "$OUT_DIR/share/applications/kalicut.desktop"
sed -i "s|^Exec=.*|Exec=kalicut|" "$OUT_DIR/share/applications/kalicut.desktop" || true
install -m 644 "$ROOT/packaging/icons/kalicut.png" "$OUT_DIR/share/icons/kalicut.png"
install -m 644 "$ROOT/README.md" "$OUT_DIR/README.md"
install -m 644 "$ROOT/LICENSE" "$OUT_DIR/LICENSE"
if [[ -f "$ROOT/docs/LINUX.md" ]]; then
  install -m 644 "$ROOT/docs/LINUX.md" "$OUT_DIR/LINUX.md"
fi

cat >"$OUT_DIR/RUN.txt" <<'EOF'
KALICUT portable build
======================

Run:
  ./kalicut

Fully self-contained:
  - kalicut.bin
  - ffmpeg / ffprobe (bundled)
  - lib/* (libmpv + codecs + ffmpeg libs)

No system packages required beyond a normal Linux desktop (glibc, display).
EOF

mkdir -p "$ROOT/dist"
rm -f "$OUT_TAR"
tar -C "$ROOT/dist" -czf "$OUT_TAR" "$NAME"

echo
echo "Created: $OUT_DIR"
echo "Archive: $OUT_TAR"
ls -lh "$OUT_TAR"
du -sh "$OUT_DIR"
# verify bundled ffmpeg is preferred
(
  cd "$OUT_DIR"
  # shellcheck disable=SC1091
  export PATH="$(pwd):$PATH"
  export LD_LIBRARY_PATH="$(pwd)/lib"
  command -v ffmpeg
  ffmpeg -version 2>&1 | head -1
)
