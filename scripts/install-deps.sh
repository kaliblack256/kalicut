#!/usr/bin/env bash
# Install build + runtime dependencies for KALICUT on common Linux distros.
# Usage:
#   ./scripts/install-deps.sh          # print packages + install if root/sudo available
#   ./scripts/install-deps.sh --print  # only print commands
set -euo pipefail

PRINT_ONLY=0
if [[ "${1:-}" == "--print" ]]; then
  PRINT_ONLY=1
fi

run() {
  if [[ "$PRINT_ONLY" -eq 1 ]]; then
    echo "+ $*"
    return 0
  fi
  echo "+ $*"
  eval "$@"
}

need_sudo() {
  if [[ "$(id -u)" -eq 0 ]]; then
    echo ""
  else
    echo "sudo"
  fi
}

SUDO="$(need_sudo)"

detect() {
  if [[ -f /etc/os-release ]]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    echo "${ID:-unknown}"
  else
    echo "unknown"
  fi
}

ID="$(detect)"
echo "Detected distro id: $ID"
echo

case "$ID" in
  debian|ubuntu|linuxmint|pop|kali|raspbian|neon|zorin|elementary)
    echo "==> Debian-family packages"
    PKGS=(
      build-essential pkg-config curl
      libmpv-dev libasound2-dev
      libx11-dev libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
      libgtk-3-dev
      ffmpeg
    )
    run $SUDO apt-get update
    run $SUDO apt-get install -y "${PKGS[@]}"
    ;;
  fedora|rhel|centos|rocky|almalinux)
    echo "==> Fedora/RHEL-family packages"
    if command -v dnf >/dev/null 2>&1; then
      PM=dnf
    else
      PM=yum
    fi
    PKGS=(
      gcc gcc-c++ make pkgconf-pkg-config curl
      mpv-libs-devel alsa-lib-devel
      libX11-devel libxkbcommon-devel
      gtk3-devel
      ffmpeg
    )
    run $SUDO $PM install -y "${PKGS[@]}"
    ;;
  arch|manjaro|endeavouros|garuda)
    echo "==> Arch-family packages"
    PKGS=(
      base-devel pkgconf curl
      mpv alsa-lib
      libx11 libxkbcommon
      gtk3
      ffmpeg
    )
    run $SUDO pacman -Sy --needed --noconfirm "${PKGS[@]}"
    ;;
  opensuse*|suse)
    echo "==> openSUSE packages"
    PKGS=(
      gcc gcc-c++ make pkg-config curl
      mpv-devel alsa-devel
      libX11-devel libxkbcommon-devel
      gtk3-devel
      ffmpeg
    )
    run $SUDO zypper install -y "${PKGS[@]}"
    ;;
  *)
    echo "Unknown distro. Install roughly:"
    echo "  - Rust toolchain (rustup)"
    echo "  - C toolchain (gcc/clang, pkg-config)"
    echo "  - libmpv development headers"
    echo "  - ALSA development headers"
    echo "  - X11 / libxkbcommon / GTK3 (for rfd dialogs)"
    echo "  - ffmpeg (runtime)"
    exit 1
    ;;
esac

echo
if ! command -v rustc >/dev/null 2>&1; then
  echo "Rust not found. Install with:"
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  echo "  source \"\$HOME/.cargo/env\""
else
  echo "Rust: $(rustc --version)"
fi

echo
echo "Done. Build with: ./scripts/build.sh"
