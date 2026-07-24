# Changelog

## Unreleased

## [0.1.2] — 2026-07-24

### Added

- **Full desktop arch matrix** (CI + packages)
  - Linux **x86_64** + **aarch64** (`.deb`, portable `.tar.gz`, AppImage) — Actions `linux.yml`
  - Windows **x86_64** + **aarch64** portable zip — matrix in `windows.yml`
  - macOS **arm64** + **Intel x86_64**
- **Windows** portable with bundled **`libmpv-2.dll`** + ffmpeg/ffprobe
  - Optional **Authenticode** via `WINDOWS_PFX_BASE64` / `WINDOWS_PFX_PASSWORD`
  - Docs: [docs/WINDOWS_SIGNING.md](docs/WINDOWS_SIGNING.md)
- macOS multi-mode signing + `unquarantine.sh` (Developer ID via secrets)

### Notes

- Signing still optional: macOS ad-hoc / Windows unsigned unless cert secrets are set
- AppImage on Linux aarch64 is best-effort; portable + `.deb` are required

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
