# Changelog

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
