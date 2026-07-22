# KALICUT

Small **Rust + egui** GUI to cut **audio and video** with **ffmpeg**.

Default mode is **stream copy** (`-map 0:v? -map 0:a? -map 0:s? -c copy`): container, codecs, bitrate, resolution, fps, audio, and subtitles stay like the source — no re-encoding.

## Features

- Audio and video (mp4, mkv, webm, mov, avi, mp3, flac, …)
- Metadata: type, codecs, resolution, fps, bitrate, channels
- **Timeline** + SoundCloud-style waveform (from the audio track)
- Embedded **mpv** preview (`libmpv`, `hwdec=auto`) with Auto / Quality / Speed preview sizing
- Playback: ▶, Space play/pause, selection play
- Time: min / sec / ms + drag handles on the timeline (magnetic snap)
- **Stream copy** (lossless) or **re-encode** (accurate cuts) with presets and manual settings
- Output: `name_cut.ext` (or chosen container when re-encoding)

## Install on Linux

Full guide: **[docs/LINUX.md](docs/LINUX.md)**.

**Release packages are self-contained:** `kalicut` + **libmpv** + **ffmpeg/ffprobe** + libraries.  
No separate system packages are required for cut/export (normal Linux desktop is enough).

### Download (GitHub Releases)

Grab from [Releases](https://github.com/kaliblack256/kalicut/releases):

| Asset | Use |
|-------|-----|
| `kalicut_*_amd64.deb` | Debian / Ubuntu / Kali / Mint |
| `KALICUT-*-x86_64.AppImage` | Most distros (portable) |
| `kalicut-*-linux-x86_64.tar.gz` | Unpack and run `./kalicut` |

```bash
# .deb
sudo apt install ./kalicut_*_amd64.deb

# AppImage
chmod +x KALICUT-*-x86_64.AppImage
./KALICUT-*-x86_64.AppImage

# portable
tar -xzf kalicut-*-linux-x86_64.tar.gz
cd kalicut-*-linux-x86_64 && ./kalicut
```

### From source

```bash
./scripts/install-deps.sh   # Debian/Ubuntu/Kali, Fedora, Arch, openSUSE
# need Rust: https://rustup.rs
./scripts/build.sh
./target/release/kalicut
```

Optional selftest (sample videos directory):

```bash
# default: ~/Videos   or:  export KALICUT_TEST_VIDEOS=/path/to/videos
cargo run --release --bin kalicut_selftest
# cargo run --release --bin kalicut_selftest -- /path/to/videos
```

### Build packages yourself

```bash
make all           # portable + .deb + AppImage → dist/
# or Docker builder:
make docker-out
```

| Method | Best for |
|--------|----------|
| Release download | end users (Linux + macOS arm64) |
| Source | development |
| `.deb` / AppImage / tar | Linux packaging |
| `package-macos.sh` / Actions | Apple Silicon (M1–M4) |
| Docker | clean Linux build host |

### macOS Apple Silicon (M1 / M2 / M3 / M4)

CI builds `kalicut-*-macos-arm64.tar.gz` (Actions → **macOS arm64**, or on tag `v*`).

```bash
tar -xzf kalicut-*-macos-arm64.tar.gz
cd kalicut-*-macos-arm64
./kalicut
```

If Gatekeeper blocks an unsigned build: **Privacy & Security → Open Anyway**,  
or `xattr -dr com.apple.quarantine .` in the folder.

## Dependencies

**Run (packaged builds):** none beyond a normal Linux desktop.

**Build from source / package on a builder:** `ffmpeg`, `libmpv-dev`, ALSA, X11/GTK — see `./scripts/install-deps.sh`.

## Quality modes

| Mode | FFmpeg | When to use |
|------|--------|-------------|
| Stream copy | `-c copy` | Default — keep original quality |
| Re-encode | H.264/H.265/VP9 + audio codecs | Accurate cuts, or convert format/resolution/bitrate |

**Preview quality** (Auto / Quality / Speed) only affects on-screen playback, not the exported file when stream copy is selected.

## License

MIT — see [LICENSE](LICENSE).

Bundled third-party tools (**ffmpeg**, **libmpv** and their libraries) keep their own upstream licenses (GPL/LGPL/etc.).
