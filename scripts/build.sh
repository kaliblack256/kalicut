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

# Anonymize host paths baked into panic/debug strings (release only)
if [[ "$PROFILE" == "release" ]]; then
  # e.g. /home/you/.cargo/... → /.cargo/...  and project root → /
  REMAP_FLAGS=(
    "--remap-path-prefix=${HOME}/="
    "--remap-path-prefix=${ROOT}/="
  )
  export RUSTFLAGS="${REMAP_FLAGS[*]}${RUSTFLAGS:+ ${RUSTFLAGS}}"
  echo "==> RUSTFLAGS path remap (privacy)"
fi

echo "==> cargo ${CARGO_ARGS[*]}"
cargo "${CARGO_ARGS[@]}"

BIN="$ROOT/target/$PROFILE/kalicut"
SELFTEST="$ROOT/target/$PROFILE/kalicut_selftest"

# Extra strip pass (Cargo profile.release also strips; this covers toolchains without it)
if [[ "$PROFILE" == "release" ]] && command -v strip >/dev/null 2>&1; then
  echo "==> strip symbols"
  strip -s "$BIN" 2>/dev/null || strip --strip-all "$BIN" 2>/dev/null || strip "$BIN" || true
  if [[ -x "$SELFTEST" ]]; then
    strip -s "$SELFTEST" 2>/dev/null || strip --strip-all "$SELFTEST" 2>/dev/null || strip "$SELFTEST" || true
  fi
fi

echo
echo "Binary: $BIN"
ls -lh "$BIN"
file "$BIN" || true
# Privacy check: no real home directory strings
if strings "$BIN" 2>/dev/null | grep -F "${HOME}" | head -3 | grep -q .; then
  echo "warning: host HOME path still appears in binary strings"
  strings "$BIN" 2>/dev/null | grep -F "${HOME}" | head -5
else
  echo "privacy: host HOME path not found in binary strings"
fi

if command -v ffmpeg >/dev/null 2>&1; then
  echo "ffmpeg: $(command -v ffmpeg) ($(ffmpeg -version 2>&1 | head -1))"
else
  echo "warning: ffmpeg not in PATH (needed at runtime for cut/export when not using a bundled package)"
fi

echo
echo "Run: $BIN"
