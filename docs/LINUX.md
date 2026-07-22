# KALICUT on Linux

Ways to install or run on Linux, from most “packaged” to most DIY.

## Runtime requirements

| Component | Packaged release | Build from source |
|-----------|------------------|-------------------|
| **ffmpeg** / **ffprobe** | **Bundled** in `.deb` / AppImage / portable | install on builder + at run if not packaging |
| **libmpv** + related `.so` | **Bundled** | `libmpv-dev` at compile time |
| **Desktop (glibc, X11/Wayland, audio)** | host | host |

Packaged builds put `ffmpeg`, `ffprobe`, `libmpv` and other `.so` under a private path and put that directory first on `PATH` / `LD_LIBRARY_PATH`. Cut/export works without installing ffmpeg system-wide.

---

## 1. Build from source (any distro)

```bash
git clone <your-repo-url> kalicut
cd kalicut

# Install distro packages (Debian/Ubuntu/Kali, Fedora, Arch, openSUSE)
./scripts/install-deps.sh

# Rust (once)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

./scripts/build.sh
./target/release/kalicut
```

Or: `make deps && make build`.

### Manual package names

**Debian / Ubuntu / Kali / Mint**

```bash
sudo apt install build-essential pkg-config \
  libmpv-dev libasound2-dev \
  libx11-dev libxkbcommon-dev libgtk-3-dev \
  ffmpeg
```

**Fedora**

```bash
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config \
  mpv-libs-devel alsa-lib-devel \
  libX11-devel libxkbcommon-devel gtk3-devel \
  ffmpeg
```

**Arch**

```bash
sudo pacman -S base-devel mpv alsa-lib libx11 libxkbcommon gtk3 ffmpeg
```

---

## 2. Install `.deb` (Debian, Ubuntu, Kali, Mint, …)

Build on a Debian-family system (or via Docker — see below):

```bash
./scripts/package-deb.sh
# → dist/kalicut_<version>_<arch>.deb
```

Install:

```bash
sudo apt install ./dist/kalicut_*.deb
# self-contained: ffmpeg + libmpv inside /usr/lib/kalicut/
kalicut
```

Uninstall:

```bash
sudo apt remove kalicut
```

---

## 3. AppImage (portable, most distributions)

```bash
./scripts/package-appimage.sh
# → dist/KALICUT-<version>-x86_64.AppImage
```

Run:

```bash
chmod +x dist/KALICUT-*.AppImage
./dist/KALICUT-*.AppImage
```

Notes:

- First run downloads **linuxdeploy** and **appimagetool** into `dist/tools/` (needs network).
- AppImage **bundles libmpv, ffmpeg, ffprobe** and libraries.
- Needs FUSE **or** the script sets `APPIMAGE_EXTRACT_AND_RUN=1` (works without FUSE).

---

## Portable `.tar.gz` (any distro)

```bash
./scripts/package-portable.sh
# → dist/kalicut-<ver>-linux-x86_64.tar.gz
tar -xzf dist/kalicut-*-linux-*.tar.gz
cd kalicut-*-linux-*
./kalicut
```

Layout: `kalicut` wrapper + `kalicut.bin` + `ffmpeg` + `ffprobe` + `lib/*`.

---

## 4. Docker (build machine, not a daily GUI)

Docker is aimed at **reproducible packaging**, not as the primary way to use the GUI (X11 passthrough is possible but awkward).

```bash
# Needs Docker installed on the host
make docker-out
# artifacts land in ./dist/
```

Manual:

```bash
docker build -t kalicut-builder .
mkdir -p dist
docker run --rm -v "$PWD/dist:/out" kalicut-builder \
  bash -c 'cp -a dist/*.deb dist/*.AppImage target/release/kalicut /out/; ls -lah /out'
```

Image base: **Ubuntu 22.04** (older glibc → AppImage/binary more portable than bleeding-edge hosts).

### Optional: run GUI via X11 (advanced)

```bash
xhost +local:docker
docker run --rm -e DISPLAY="$DISPLAY" \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  -v "$HOME:/data" \
  kalicut-builder ./target/release/kalicut
```

Prefer native `.deb` / AppImage / source build for daily use.

---

## 5. One-shot: everything

On a Debian-like host with network:

```bash
./scripts/install-deps.sh
./scripts/package-all.sh
ls -lh dist/
```

Produces:

- `target/release/kalicut` — raw binary (stripped; paths remapped for privacy)  
- `dist/kalicut_*_*.deb` — system package  
- `dist/KALICUT-*-*.AppImage` — portable  

### Selftest (developers)

```bash
# Default directory: ~/Videos
# Or: export KALICUT_TEST_VIDEOS=/path/to/samples
cargo run --release --bin kalicut_selftest
cargo run --release --bin kalicut_selftest -- /path/to/videos
```

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `mpv` / pkg-config errors at build | install `libmpv-dev` / `mpv-libs-devel` |
| App starts, cut fails (source build) | install `ffmpeg` or use a packaged build |
| No video in Fragment | libmpv missing/broken → ffmpeg fallback may still work |
| AppImage “permission denied” | `chmod +x …AppImage` |
| AppImage FUSE errors | already uses extract-and-run; update `dist/tools/` |
| Wayland / blank window | try `WINIT_UNIX_BACKEND=x11 ./kalicut` |

---

## What we intentionally skip (for now)

- Windows / macOS installers  
- Flatpak / Snap (possible later)  
- Bundled static ffmpeg inside AppImage (legal + size; opt-in later)  
