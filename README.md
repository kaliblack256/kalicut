# KALICUT

**Lossless audio/video cutting** — Rust + egui GUI, **ffmpeg** export, **libmpv** preview.

Default mode is **stream copy** (`-c copy`): quality stays like the source (no re-encode).

[![Release](https://img.shields.io/github/v/release/kaliblack256/kalicut)](https://github.com/kaliblack256/kalicut/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Latest release:** [v0.1.1](https://github.com/kaliblack256/kalicut/releases/tag/v0.1.1)

---

## Features

- Audio and video (mp4, mkv, webm, mov, avi, mp3, flac, …)
- Metadata: codecs, resolution, fps, bitrate, channels
- Timeline + SoundCloud-style waveform
- Embedded **mpv** preview (`hwdec=auto`) · Auto / Quality / Speed
- ▶ · Space play/pause · magnetic handles · min / sec / ms
- **Stream copy** or **re-encode** (presets + manual)
- Output: `name_cut.ext`

---

## Download & install

Packages are **self-contained**: `kalicut` + **libmpv** + **ffmpeg/ffprobe** inside.  
→ [All releases](https://github.com/kaliblack256/kalicut/releases)

### Platform matrix

| OS | Arch | Package |
|----|------|---------|
| **Linux** | x86_64 | `.deb` · AppImage · `.tar.gz` |
| **Linux** | aarch64 (ARM64) | `.deb` · AppImage · `.tar.gz` |
| **Windows** | x86_64 | portable `.zip` (+ libmpv) |
| **Windows** | aarch64 (ARM64) | portable `.zip` (+ libmpv) |
| **macOS** | arm64 (Apple Silicon) | `.tar.gz` |
| **macOS** | x86_64 (Intel) | `.tar.gz` |

### Linux

| File | For |
|------|-----|
| `kalicut_*_amd64.deb` | Debian-family **x86_64** |
| `kalicut_*_arm64.deb` | Debian-family **ARM64** |
| `KALICUT-*-x86_64.AppImage` / `*-aarch64.AppImage` | Most distros |
| `kalicut-*-linux-x86_64.tar.gz` / `*-linux-aarch64.tar.gz` | Unpack & run |

```bash
# Debian-family (pick amd64 or arm64 .deb for your CPU)
sudo apt install ./kalicut_*.deb
kalicut

# AppImage
chmod +x KALICUT-*-*.AppImage
./KALICUT-*-*.AppImage

# Portable
tar -xzf kalicut-*-linux-*.tar.gz
cd kalicut-*-linux-* && ./kalicut
```

### Windows 10 / 11

| File | For |
|------|-----|
| `kalicut-*-windows-x86_64.zip` | Win10/11 **x64** |
| `kalicut-*-windows-aarch64.zip` | **Windows on ARM** (Snapdragon, etc.) |

```bat
:: unzip, then:
KALICUT.bat
:: or double-click kalicut.exe
```

Portable: `kalicut.exe` + **`libmpv-2.dll`** + `ffmpeg.exe` + `ffprobe.exe` in the same folder.  
If SmartScreen blocks: **More info → Run anyway** (unsigned unless Authenticode secrets are set — [docs/WINDOWS_SIGNING.md](docs/WINDOWS_SIGNING.md)).

### macOS

| File | For |
|------|-----|
| `kalicut-*-macos-arm64.tar.gz` | **Apple Silicon** M1 / M2 / M3 / M4 |
| `kalicut-*-macos-x86_64.tar.gz` | **Intel** Mac |

```bash
# Apple Silicon
tar -xzf kalicut-*-macos-arm64.tar.gz && cd kalicut-*-macos-arm64

# Intel
# tar -xzf kalicut-*-macos-x86_64.tar.gz && cd kalicut-*-macos-x86_64

xattr -dr com.apple.quarantine .   # or: ./unquarantine.sh
./kalicut
```

Which Mac? → Apple menu → **About This Mac** → chip (M1/M2/…) or “Intel”.

If still blocked: **Privacy & Security → Open Anyway**, or right-click → **Open**.  
Signing: ad-hoc by default — [docs/MACOS_SIGNING.md](docs/MACOS_SIGNING.md).

---

## Build from source

```bash
# Linux deps
./scripts/install-deps.sh
# Rust: https://rustup.rs

./scripts/build.sh
./target/release/kalicut
```

**macOS (on a Mac):** Homebrew + `./scripts/package-macos.sh`  
**or** GitHub Actions → workflow **macOS** (arm64 + Intel jobs).

### Package locally (Linux)

```bash
make all          # → dist/*.deb AppImage tar.gz
make docker-out   # build inside Docker
```

### Selftest

```bash
# samples in ~/Videos, or: export KALICUT_TEST_VIDEOS=/path
cargo run --release --bin kalicut_selftest -- /path/to/videos
```

---

## Quality modes

| Mode | FFmpeg | When |
|------|--------|------|
| Stream copy | `-c copy` | Default — keep original quality |
| Re-encode | H.264/H.265/… | Accurate cuts, convert format/bitrate |

Preview quality (Auto / Quality / Speed) only affects on-screen playback, not export in copy mode.

---

## Docs

| Doc | Topic |
|-----|--------|
| [docs/LINUX.md](docs/LINUX.md) | Linux install, packages, Docker |
| [docs/MACOS_SIGNING.md](docs/MACOS_SIGNING.md) | Ad-hoc / Developer ID / notarize / xattr |
| [docs/WINDOWS_SIGNING.md](docs/WINDOWS_SIGNING.md) | libmpv zip, Authenticode / SmartScreen |

---

## License

MIT — [LICENSE](LICENSE).

Bundled **ffmpeg** and **libmpv** keep their own upstream licenses (GPL/LGPL/etc.).
