#!/usr/bin/env bash
# Recursively copy shared libraries needed by a binary into DEST_DIR.
# Skips core glibc/loader (must come from the host).
# Does NOT bundle the ffmpeg/ffprobe CLI — only shared libs of the app (incl. libmpv).
#
# Usage: bundle-libs.sh <binary> <dest_lib_dir>
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <binary> <dest_lib_dir>" >&2
  exit 1
fi

BIN="$(readlink -f "$1")"
DEST="$(readlink -f "$2" 2>/dev/null || true)"
if [[ -z "${DEST:-}" ]]; then
  mkdir -p "$2"
  DEST="$(cd "$2" && pwd)"
else
  mkdir -p "$DEST"
fi

test -f "$BIN"

# Host essentials — never ship these (ABI / security).
is_blacklisted() {
  local base="$1"
  case "$base" in
    linux-vdso.so.*|ld-linux*.so.*|ld-linux-*) return 0 ;;
    libc.so.*|libm.so.*|libdl.so.*|librt.so.*|libpthread.so.*) return 0 ;;
    libresolv.so.*|libnss_*.so.*|libnsl.so.*) return 0 ;;
    # Keep system C++/gcc runtime (present on virtually all desktops)
    libgcc_s.so.*|libstdc++.so.*) return 0 ;;
    *) return 1 ;;
  esac
}

# Resolve one NEEDED entry via ldd line
# ldd lines look like:  libfoo.so.1 => /path/libfoo.so.1 (0x...)
# or:  /lib64/ld-linux-x86-64.so.2 (0x...)
collect_deps() {
  local file="$1"
  ldd "$file" 2>/dev/null | while IFS= read -r line; do
    if [[ "$line" =~ "not found" ]]; then
      continue
    fi
    local path=""
    if [[ "$line" =~ \=\>\ (/[^ ]+) ]]; then
      path="${BASH_REMATCH[1]}"
    elif [[ "$line" =~ ^[[:space:]]*(/[^ ]+) ]]; then
      path="${BASH_REMATCH[1]}"
    else
      continue
    fi
    if [[ ! -e "$path" ]]; then
      continue
    fi
    echo "$path"
  done
}

declare -A SEEN=()
QUEUE=("$BIN")
COPIED=0

while [[ ${#QUEUE[@]} -gt 0 ]]; do
  cur="${QUEUE[0]}"
  QUEUE=("${QUEUE[@]:1}")

  while IFS= read -r libpath; do
    [[ -z "$libpath" ]] && continue
    real="$(readlink -f "$libpath" 2>/dev/null || true)"
    [[ -z "$real" || ! -f "$real" ]] && continue

    base="$(basename "$real")"
    soname="$(basename "$libpath")"

    if is_blacklisted "$base" || is_blacklisted "$soname"; then
      continue
    fi

    key="$real"
    if [[ -n "${SEEN[$key]:-}" ]]; then
      continue
    fi
    SEEN[$key]=1

    # Already inside DEST (e.g. second pass after linuxdeploy) — just walk deps
    if [[ "$real" == "$DEST"/* ]] || [[ "$(dirname "$real")" == "$DEST" ]]; then
      QUEUE+=("$real")
      continue
    fi

    dest_file="$DEST/$base"
    if [[ -e "$dest_file" ]]; then
      # Prefer existing copy; still recurse its deps
      QUEUE+=("$dest_file")
      continue
    fi

    cp -a "$real" "$dest_file"
    if [[ "$soname" != "$base" && ! -e "$DEST/$soname" ]]; then
      ln -sfn "$base" "$DEST/$soname"
    fi
    # also link common short names from the original path basename if different
    orig_base="$(basename "$libpath")"
    if [[ "$orig_base" != "$base" && ! -e "$DEST/$orig_base" ]]; then
      ln -sfn "$base" "$DEST/$orig_base"
    fi

    COPIED=$((COPIED + 1))
    QUEUE+=("$dest_file")
  done < <(collect_deps "$cur")
done

echo "bundled $COPIED libraries → $DEST"
ls "$DEST" | wc -l | awk '{print "entries:", $1}'
