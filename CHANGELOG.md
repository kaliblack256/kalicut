# Changelog

## Unreleased

### Added

- **Windows 10/11 x64** portable zip (`kalicut-*-windows-x86_64.zip`)
  - `scripts/package-windows.ps1` + Actions `windows.yml`
  - Bundled `ffmpeg.exe` / `ffprobe.exe`, `KALICUT.bat` launcher
  - Preview via ffmpeg path (optional `embedded-mpv` feature for Linux/macOS)
- macOS packaging for **Apple Silicon (arm64)** and **Intel (x86_64)**
- macOS multi-mode signing + `unquarantine.sh`

## [0.1.1] — 2026-07-22

### Security / privacy

- Selftest no longer hardcodes a developer home path; uses CLI path, `KALICUT_TEST_VIDEOS`, or `~/Videos`
- Release builds strip symbols and remap host absolute paths out of the binary
- Smaller stripped release binary (~13 MB before packaging)

### Packaging

- Rebuild of self-contained Linux packages (`.deb`, AppImage, portable `.tar.gz`) with the privacy fixes

## [0.1.0] — 2026-07-22

### Added

- GUI (Rust + egui) for lossless audio/video cutting via ffmpeg stream copy
- Optional re-encode with presets and manual codec/bitrate/resolution settings
- Timeline with SoundCloud-style waveform and magnetic handles
- Embedded libmpv preview (Fragment panel) with Auto / Quality / Speed modes
- Linux packaging: `.deb`, AppImage, portable `.tar.gz` (self-contained)
- Bundled **libmpv**, **ffmpeg**, and **ffprobe** in release packages
- Docker build image for reproducible packaging
- `kalicut_selftest` binary for smoke tests

### Notes

- Host runtime needs only a normal desktop stack (glibc, display, audio)
- Source builds still require `libmpv-dev` / `ffmpeg` on the builder
