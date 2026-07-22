#!/usr/bin/env bash
# Copy host ffmpeg + ffprobe into BIN_DIR and pull their shared libs into LIB_DIR.
# Usage: bundle-ffmpeg.sh <bin_dir> <lib_dir>
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <bin_dir> <lib_dir>" >&2
  exit 1
fi

BIN_DIR="$1"
LIB_DIR="$2"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

mkdir -p "$BIN_DIR" "$LIB_DIR"

FFMPEG="$(command -v ffmpeg || true)"
FFPROBE="$(command -v ffprobe || true)"

if [[ -z "$FFMPEG" || ! -x "$FFMPEG" ]]; then
  echo "error: ffmpeg not found on build host — install ffmpeg to bundle it" >&2
  exit 1
fi
if [[ -z "$FFPROBE" || ! -x "$FFPROBE" ]]; then
  echo "error: ffprobe not found on build host" >&2
  exit 1
fi

echo "==> Bundling ffmpeg from $FFMPEG"
install -m 755 "$FFMPEG" "$BIN_DIR/ffmpeg"
install -m 755 "$FFPROBE" "$BIN_DIR/ffprobe"

# Shared libraries for both tools (same lib dir as kalicut/libmpv)
"$ROOT/scripts/bundle-libs.sh" "$BIN_DIR/ffmpeg" "$LIB_DIR"
"$ROOT/scripts/bundle-libs.sh" "$BIN_DIR/ffprobe" "$LIB_DIR"

# Smoke: can the bundled binary start with only bundled libs (+ glibc)?
if ! LD_LIBRARY_PATH="$LIB_DIR" "$BIN_DIR/ffmpeg" -version >/dev/null 2>&1; then
  echo "warning: bundled ffmpeg -version failed under LD_LIBRARY_PATH=$LIB_DIR" >&2
  LD_LIBRARY_PATH="$LIB_DIR" "$BIN_DIR/ffmpeg" -version 2>&1 | head -5 || true
else
  ver="$(LD_LIBRARY_PATH="$LIB_DIR" "$BIN_DIR/ffmpeg" -version 2>&1 | head -1)"
  echo "bundled ffmpeg OK: $ver"
fi

ls -lh "$BIN_DIR/ffmpeg" "$BIN_DIR/ffprobe"
