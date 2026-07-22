#!/usr/bin/env bash
# Write a launcher: bundled libs + bundled ffmpeg/ffprobe first on PATH.
# Usage: make-wrapper.sh <output_path> <bin_ref> <lib_ref> <mode> [path_ref]
#   mode: relative | absolute
#   path_ref: directory prepended to PATH (contains ffmpeg). Defaults:
#     absolute → same as lib_ref
#     relative → dirname of bin_ref or "." 
set -euo pipefail

OUT="$1"
BIN_REF="$2"
LIB_REF="$3"
MODE="${4:-relative}"
PATH_REF="${5:-}"

if [[ "$MODE" == "absolute" ]]; then
  if [[ -z "$PATH_REF" ]]; then
    PATH_REF="$LIB_REF"
  fi
  cat >"$OUT" <<EOF
#!/bin/sh
# KALICUT launcher — bundled libs + bundled ffmpeg/ffprobe
export LD_LIBRARY_PATH="${LIB_REF}\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
export PATH="${PATH_REF}:\${PATH:-}"
exec "${BIN_REF}" "\$@"
EOF
else
  if [[ -z "$PATH_REF" ]]; then
    PATH_REF="."
  fi
  cat >"$OUT" <<'EOF'
#!/bin/sh
# KALICUT launcher — bundled libs + bundled ffmpeg next to this script
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/LIB_PLACEHOLDER${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PATH="$HERE/PATH_PLACEHOLDER${PATH:+:$PATH}"
exec "$HERE/BIN_PLACEHOLDER" "$@"
EOF
  sed -i "s|LIB_PLACEHOLDER|${LIB_REF}|g; s|BIN_PLACEHOLDER|${BIN_REF}|g; s|PATH_PLACEHOLDER|${PATH_REF}|g" "$OUT"
fi
chmod 755 "$OUT"
