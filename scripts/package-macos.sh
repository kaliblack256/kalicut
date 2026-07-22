#!/usr/bin/env bash
# Build a self-contained macOS arm64 (Apple Silicon M1–M4) package.
# Must run on macOS (or GitHub Actions macos-14 arm64).
#
# Output: dist/kalicut-<ver>-macos-arm64.tar.gz
# Contents: kalicut, ffmpeg, ffprobe, lib/*.dylib (libmpv + deps)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: package-macos.sh must run on macOS" >&2
  exit 1
fi

ARCH="$(uname -m)"
case "$ARCH" in
  arm64) ARCH_LABEL=arm64 ;;
  x86_64) ARCH_LABEL=x86_64 ;;
  *)
    echo "error: unsupported arch: $ARCH" >&2
    exit 1
    ;;
esac

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
NAME="kalicut-${VERSION}-macos-${ARCH_LABEL}"
OUT_DIR="$ROOT/dist/$NAME"
OUT_TAR="$ROOT/dist/${NAME}.tar.gz"

echo "==> macOS package for $ARCH_LABEL (version $VERSION)"

# --- deps (Homebrew) ---
if ! command -v brew >/dev/null 2>&1; then
  echo "error: Homebrew required. https://brew.sh" >&2
  exit 1
fi

echo "==> brew packages"
brew list mpv >/dev/null 2>&1 || brew install mpv
brew list ffmpeg >/dev/null 2>&1 || brew install ffmpeg
brew list pkg-config >/dev/null 2>&1 || brew install pkg-config

export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LIBRARY_PATH="$(brew --prefix)/lib:${LIBRARY_PATH:-}"
export CPATH="$(brew --prefix)/include:${CPATH:-}"

# Prefer Homebrew mpv/ffmpeg
BREW_PREFIX="$(brew --prefix)"
export PATH="$BREW_PREFIX/bin:$PATH"

if ! pkg-config --exists mpv; then
  echo "error: pkg-config cannot find mpv (brew install mpv)" >&2
  exit 1
fi

echo "==> cargo build --release"
if [[ -n "${HOME:-}" && -n "${ROOT:-}" ]]; then
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--remap-path-prefix=${HOME}/= --remap-path-prefix=${ROOT}/="
fi
cargo build --release --bin kalicut

BIN="$ROOT/target/release/kalicut"
test -x "$BIN"
if command -v strip >/dev/null 2>&1; then
  strip -x "$BIN" 2>/dev/null || strip "$BIN" || true
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/lib" "$OUT_DIR/bin"

install -m 755 "$BIN" "$OUT_DIR/bin/kalicut"

# --- ffmpeg / ffprobe ---
echo "==> bundling ffmpeg/ffprobe"
for tool in ffmpeg ffprobe; do
  src="$(command -v "$tool" || true)"
  if [[ -z "$src" ]]; then
    echo "error: $tool not found" >&2
    exit 1
  fi
  # Resolve brew cellar real path
  real="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$src")"
  install -m 755 "$real" "$OUT_DIR/bin/$tool"
done

# --- collect dylibs for kalicut + ffmpeg ---
echo "==> collecting dylibs"
collect_dylibs() {
  local binary="$1"
  local dest="$2"
  # BFS over LC_LOAD_DYLIB
  local queue=("$binary")
  local -a seen=()
  is_seen() {
    local x="$1"
    local s
    for s in "${seen[@]+"${seen[@]}"}"; do
      [[ "$s" == "$x" ]] && return 0
    done
    return 1
  }
  while [[ ${#queue[@]} -gt 0 ]]; do
    local cur="${queue[0]}"
    queue=("${queue[@]:1}")
    while IFS= read -r line; do
      # otool -L lines:  \t/path/libfoo.dylib (compatibility ...)
      local lib
      lib="$(echo "$line" | awk '{print $1}')"
      case "$lib" in
        /usr/lib/*|/System/*|@*) continue ;;
        *.dylib|*.so|*.so.*) ;;
        *) continue ;;
      esac
      if [[ ! -f "$lib" ]]; then
        # try brew prefix
        local base
        base="$(basename "$lib")"
        if [[ -f "$BREW_PREFIX/lib/$base" ]]; then
          lib="$BREW_PREFIX/lib/$base"
        else
          continue
        fi
      fi
      local real
      real="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$lib")"
      if is_seen "$real"; then
        continue
      fi
      seen+=("$real")
      local base
      base="$(basename "$real")"
      if [[ ! -f "$dest/$base" ]]; then
        cp -p "$real" "$dest/$base"
        chmod 755 "$dest/$base" 2>/dev/null || true
      fi
      queue+=("$real")
    done < <(otool -L "$cur" 2>/dev/null | tail -n +2)
  done
}

collect_dylibs "$OUT_DIR/bin/kalicut" "$OUT_DIR/lib"
collect_dylibs "$OUT_DIR/bin/ffmpeg" "$OUT_DIR/lib"
collect_dylibs "$OUT_DIR/bin/ffprobe" "$OUT_DIR/lib"

# Fix install names to @loader_path / @executable_path relative
echo "==> rewriting install names (install_name_tool)"
fix_binary() {
  local bin="$1"
  local rel_lib="$2" # e.g. @executable_path/../lib
  # change each non-system dylib dependency
  while IFS= read -r line; do
    local lib
    lib="$(echo "$line" | awk '{print $1}')"
    case "$lib" in
      /usr/lib/*|/System/*|@*) continue ;;
      *.dylib)
        local base
        base="$(basename "$lib")"
        if [[ -f "$OUT_DIR/lib/$base" ]]; then
          install_name_tool -change "$lib" "${rel_lib}/${base}" "$bin" 2>/dev/null || true
        fi
        ;;
    esac
  done < <(otool -L "$bin" 2>/dev/null | tail -n +2)
  # also try resolved cellar paths that might differ
  for dylib in "$OUT_DIR/lib"/*.dylib; do
    [[ -f "$dylib" ]] || continue
    local base
    base="$(basename "$dylib")"
    # change any absolute path ending with this basename
    while IFS= read -r line; do
      local lib
      lib="$(echo "$line" | awk '{print $1}')"
      case "$lib" in
        */"$base")
          install_name_tool -change "$lib" "${rel_lib}/${base}" "$bin" 2>/dev/null || true
          ;;
      esac
    done < <(otool -L "$bin" 2>/dev/null | tail -n +2)
  done
}

fix_dylib_id_and_deps() {
  local dylib="$1"
  local base
  base="$(basename "$dylib")"
  install_name_tool -id "@loader_path/${base}" "$dylib" 2>/dev/null || true
  while IFS= read -r line; do
    local lib
    lib="$(echo "$line" | awk '{print $1}')"
    case "$lib" in
      /usr/lib/*|/System/*|@*) continue ;;
      *.dylib)
        local b
        b="$(basename "$lib")"
        if [[ -f "$OUT_DIR/lib/$b" ]]; then
          install_name_tool -change "$lib" "@loader_path/${b}" "$dylib" 2>/dev/null || true
        fi
        ;;
    esac
  done < <(otool -L "$dylib" 2>/dev/null | tail -n +2)
}

for f in "$OUT_DIR/bin"/*; do
  fix_binary "$f" "@executable_path/../lib"
done
for d in "$OUT_DIR/lib"/*.dylib; do
  [[ -f "$d" ]] || continue
  fix_dylib_id_and_deps "$d"
done

# Launcher in package root
cat >"$OUT_DIR/kalicut" <<'EOF'
#!/bin/bash
HERE="$(cd "$(dirname "$0")" && pwd)"
export PATH="$HERE/bin:${PATH:-}"
# Prefer bundled dylibs
export DYLD_LIBRARY_PATH="$HERE/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
export DYLD_FALLBACK_LIBRARY_PATH="$HERE/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
exec "$HERE/bin/kalicut" "$@"
EOF
chmod 755 "$OUT_DIR/kalicut"

# ---------------------------------------------------------------------------
# Code signing — several modes (docs/MACOS_SIGNING.md)
#
#   SIGN_MODE=adhoc|identity|none   (default: adhoc)
#   CODESIGN_IDENTITY="Developer ID Application: Name (TEAMID)"
#   ENTITLEMENTS=path/to.plist
#   NOTARIZE=1 + APPLE_ID + APPLE_TEAM_ID + APPLE_APP_PASSWORD
# ---------------------------------------------------------------------------
SIGN_MODE="${SIGN_MODE:-adhoc}"
SIGN_INFO="unsigned"
ENTITLEMENTS="${ENTITLEMENTS:-}"

codesign_one() {
  local path="$1"
  local identity="$2"
  local extra=()
  if [[ -n "$ENTITLEMENTS" && -f "$ENTITLEMENTS" ]]; then
    extra+=(--entitlements "$ENTITLEMENTS")
  fi
  if [[ "$identity" == "-" ]]; then
    codesign --force --sign - --timestamp=none "${extra[@]}" "$path" 2>/dev/null \
      || codesign --force --sign - "${extra[@]}" "$path"
  else
    codesign --force --options runtime --timestamp --sign "$identity" "${extra[@]}" "$path"
  fi
}

sign_tree() {
  local identity="$1"
  echo "==> codesign identity: ${identity}"
  local f
  for f in "$OUT_DIR/lib"/*.dylib; do
    [[ -f "$f" ]] || continue
    codesign_one "$f" "$identity"
  done
  for f in "$OUT_DIR/bin"/*; do
    [[ -f "$f" ]] || continue
    codesign_one "$f" "$identity"
  done
  codesign_one "$OUT_DIR/kalicut" "$identity" || true
  codesign -dv --verbose=2 "$OUT_DIR/bin/kalicut" 2>&1 | head -20 || true
  codesign --verify --verbose "$OUT_DIR/bin/kalicut" 2>&1 || true
}

case "$SIGN_MODE" in
  none)
    echo "==> signing disabled (SIGN_MODE=none)"
    SIGN_INFO="unsigned"
    ;;
  identity)
    if [[ -z "${CODESIGN_IDENTITY:-}" ]]; then
      echo "error: SIGN_MODE=identity requires CODESIGN_IDENTITY" >&2
      exit 1
    fi
    sign_tree "$CODESIGN_IDENTITY"
    SIGN_INFO="identity: ${CODESIGN_IDENTITY}"
    if [[ "${NOTARIZE:-0}" == "1" ]]; then
      echo "==> notarize (notarytool)"
      if [[ -z "${APPLE_ID:-}" || -z "${APPLE_TEAM_ID:-}" || -z "${APPLE_APP_PASSWORD:-}" ]]; then
        echo "warning: NOTARIZE=1 but APPLE_* credentials missing — skip" >&2
      else
        ZIP_NOTARY="$ROOT/dist/${NAME}-for-notary.zip"
        ditto -c -k --keepParent "$OUT_DIR" "$ZIP_NOTARY"
        xcrun notarytool submit "$ZIP_NOTARY" \
          --apple-id "$APPLE_ID" \
          --team-id "$APPLE_TEAM_ID" \
          --password "$APPLE_APP_PASSWORD" \
          --wait
        SIGN_INFO="${SIGN_INFO}; notarized"
      fi
    fi
    ;;
  adhoc|*)
    # Free. Fixes post-install_name_tool consistency; not a public Gatekeeper pass.
    sign_tree "-"
    SIGN_INFO="ad-hoc (codesign -s -)"
    ;;
esac

echo "$SIGN_INFO" >"$OUT_DIR/SIGNING.txt"

cat >"$OUT_DIR/RUN.txt" <<EOF
KALICUT ${VERSION} for macOS (${ARCH_LABEL})
================================

Apple Silicon (M1 / M2 / M3 / M4) and Intel when built as x86_64.

Signing: ${SIGN_INFO}

Run:
  ./kalicut

If macOS blocks the download:
  System Settings → Privacy & Security → Open Anyway
  or:  xattr -dr com.apple.quarantine .

Ad-hoc / self-signed builds are NOT fully trusted by Gatekeeper for strangers.
Developer ID + notarization: see docs/MACOS_SIGNING.md

Bundled: kalicut, libmpv (+ dylibs), ffmpeg, ffprobe.
EOF

install -m 644 "$ROOT/LICENSE" "$OUT_DIR/LICENSE" 2>/dev/null || true
install -m 644 "$ROOT/README.md" "$OUT_DIR/README.md" 2>/dev/null || true
if [[ -f "$ROOT/docs/MACOS_SIGNING.md" ]]; then
  install -m 644 "$ROOT/docs/MACOS_SIGNING.md" "$OUT_DIR/MACOS_SIGNING.md"
fi

mkdir -p "$ROOT/dist"
rm -f "$OUT_TAR"
tar -C "$ROOT/dist" -czf "$OUT_TAR" "$NAME"

echo
echo "Created: $OUT_TAR"
echo "Signing: $SIGN_INFO"
ls -lh "$OUT_TAR"
du -sh "$OUT_DIR"

echo "==> otool sample"
otool -L "$OUT_DIR/bin/kalicut" | head -15
echo "ffmpeg: $("$OUT_DIR/bin/ffmpeg" -version 2>&1 | head -1)"
