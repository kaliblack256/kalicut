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

### Linux

| File | For |
|------|-----|
| `kalicut_*_amd64.deb` | Debian / Ubuntu / Kali / Mint |
| `KALICUT-*-x86_64.AppImage` | Most distros |
| `kalicut-*-linux-x86_64.tar.gz` | Unpack & run |

```bash
# Debian-family
sudo apt install ./kalicut_*_amd64.deb
kalicut

# AppImage
chmod +x KALICUT-*-x86_64.AppImage
./KALICUT-*-x86_64.AppImage

# Portable
tar -xzf kalicut-*-linux-x86_64.tar.gz
cd kalicut-*-linux-x86_64 && ./kalicut
```

### macOS Apple Silicon (M1 / M2 / M3 / M4)

| File | For |
|------|-----|
| `kalicut-*-macos-arm64.tar.gz` | M1–M4 |

```bash
tar -xzf kalicut-*-macos-arm64.tar.gz
cd kalicut-*-macos-arm64

# Remove Gatekeeper quarantine (needed after download from the internet)
xattr -dr com.apple.quarantine .
# or:  ./unquarantine.sh

./kalicut
```

If macOS still blocks: **System Settings → Privacy & Security → Open Anyway**,  
or right-click `kalicut` → **Open** → **Open**.

Signing: **ad-hoc** by default (free). Full Developer ID + notarize needs a paid  
Apple Developer account — see [docs/MACOS_SIGNING.md](docs/MACOS_SIGNING.md).

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
**or** GitHub Actions → workflow **macOS arm64**.

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

---

## License

MIT — [LICENSE](LICENSE).

Bundled **ffmpeg** and **libmpv** keep their own upstream licenses (GPL/LGPL/etc.).
