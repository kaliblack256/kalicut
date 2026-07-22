#!/usr/bin/env bash
# Build release binary + portable + .deb + AppImage into dist/
# All packages bundle libmpv/libs; only ffmpeg stays a host dependency.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mkdir -p dist
"$ROOT/scripts/build.sh"
"$ROOT/scripts/package-portable.sh"
"$ROOT/scripts/package-deb.sh"
"$ROOT/scripts/package-appimage.sh"

echo
echo "==> dist/ (release artifacts)"
ls -lh "$ROOT/dist"/*.{deb,AppImage,tar.gz} 2>/dev/null || ls -lh "$ROOT/dist"
if [[ -x "$ROOT/dist"/kalicut-*/kalicut ]] || compgen -G "$ROOT/dist/kalicut-*/kalicut" >/dev/null; then
  du -sh "$ROOT/dist"/kalicut-*/ 2>/dev/null || true
fi

# Checksums for release uploads
(
  cd "$ROOT/dist"
  sha256sum ./*.deb ./*.AppImage ./*.tar.gz 2>/dev/null >SHA256SUMS.txt || true
  echo "SHA256SUMS.txt updated"
  cat SHA256SUMS.txt 2>/dev/null || true
)
