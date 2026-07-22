#!/usr/bin/env bash
# Build KALICUT release binary.
# Usage: ./scripts/build.sh [--debug]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROFILE=release
CARGO_ARGS=(build --release)
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE=debug
  CARGO_ARGS=(build)
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found. Install Rust: https://rustup.rs" >&2
  exit 1
fi

if ! command -v pkg-config >/dev/null 2>&1; then
  echo "warning: pkg-config missing — libmpv link may fail" >&2
fi

if ! pkg-config --exists mpv 2>/dev/null; then
  echo "warning: mpv.pc not found — install libmpv-dev / mpv-libs-devel" >&2
fi

echo "==> cargo ${CARGO_ARGS[*]}"
cargo "${CARGO_ARGS[@]}"

BIN="$ROOT/target/$PROFILE/kalicut"
echo
echo "Binary: $BIN"
ls -lh "$BIN"
file "$BIN" || true

if command -v ffmpeg >/dev/null 2>&1; then
  echo "ffmpeg: $(command -v ffmpeg) ($(ffmpeg -version 2>&1 | head -1))"
else
  echo "warning: ffmpeg not in PATH (needed at runtime for cut/export)"
fi

echo
echo "Run: $BIN"
