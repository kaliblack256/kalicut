//! Wrapper around system `ffprobe` / `ffmpeg`.
//! Default trim uses stream copy (`-c copy`) so container, codecs, and quality
//! of the source are preserved without re-encoding (audio, video, subtitles, etc.).

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Media file info (audio and/or video).
#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub duration: f64,
    pub format_name: String,
    pub bit_rate: Option<u64>,
    pub size_bytes: Option<u64>,

    pub has_video: bool,
    pub has_audio: bool,

    pub audio_codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,

    pub video_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

impl MediaInfo {
    pub fn duration_label(&self) -> String {
        format_seconds(self.duration)
    }

    pub fn bit_rate_label(&self) -> String {
        match self.bit_rate {
            Some(b) if b >= 1_000_000 => format!("{:.1} Mbit/s", b as f64 / 1_000_000.0),
            Some(b) if b >= 1_000 => format!("{} kbit/s", b / 1_000),
            Some(b) => format!("{b} bit/s"),
            None => "—".into(),
        }
    }

    pub fn size_label(&self) -> String {
        match self.size_bytes {
            Some(s) if s >= 1_048_576 => format!("{:.2} MiB", s as f64 / 1_048_576.0),
            Some(s) if s >= 1024 => format!("{:.1} KiB", s as f64 / 1024.0),
            Some(s) => format!("{s} B"),
            None => "—".into(),
        }
    }

    pub fn resolution_label(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            _ => "—".into(),
        }
    }

    pub fn fps_label(&self) -> String {
        match self.fps {
            Some(f) if f > 0.0 => {
                if (f - f.round()).abs() < 0.01 {
                    format!("{:.0} fps", f.round())
                } else {
                    format!("{f:.3} fps")
                }
            }
            _ => "—".into(),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match (self.has_video, self.has_audio) {
            (true, true) => "video + audio",
            (true, false) => "video only",
            (false, true) => "audio only",
            (false, false) => "unknown",
        }
    }

    pub fn codecs_label(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = &self.video_codec {
            parts.push(format!("V:{v}"));
        }
        if let Some(a) = &self.audio_codec {
            parts.push(format!("A:{a}"));
        }
        if parts.is_empty() {
            "—".into()
        } else {
            parts.join(" · ")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeMode {
    /// `-c copy` — no re-encode; same codecs/bitrate/resolution.
    StreamCopy,
    /// Re-encode (more accurate cut boundaries).
    Reencode,
}

impl EncodeMode {
    pub fn label(self) -> &'static str {
        match self {
            EncodeMode::StreamCopy => "Stream copy (lossless)",
            EncodeMode::Reencode => "Re-encode (accurate cuts)",
        }
    }
}

#[derive(Debug)]
pub enum FfmpegError {
    NotFound(String),
    ProbeFailed(String),
    TrimFailed(String),
    InvalidRange(String),
    Io(String),
}

impl std::fmt::Display for FfmpegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfmpegError::NotFound(t) => write!(f, "{t} not found. Install ffmpeg."),
            FfmpegError::ProbeFailed(m) => write!(f, "Probe failed: {m}"),
            FfmpegError::TrimFailed(m) => write!(f, "Trim failed: {m}"),
            FfmpegError::InvalidRange(m) => write!(f, "Invalid range: {m}"),
            FfmpegError::Io(m) => write!(f, "I/O error: {m}"),
        }
    }
}

pub fn ensure_tools() -> Result<(), FfmpegError> {
    for tool in ["ffmpeg", "ffprobe"] {
        let ok = Command::new(tool)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err(FfmpegError::NotFound(tool.into()));
        }
    }
    Ok(())
}

pub fn probe(path: &Path) -> Result<MediaInfo, FfmpegError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| FfmpegError::ProbeFailed(e.to_string()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(FfmpegError::ProbeFailed(err.trim().to_string()));
    }

    let json: ProbeJson = serde_json::from_slice(&output.stdout)
        .map_err(|e| FfmpegError::ProbeFailed(format!("JSON: {e}")))?;

    let audio = json
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));
    let video = json
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video") && !is_attached_pic(s));

    if audio.is_none() && video.is_none() {
        return Err(FfmpegError::ProbeFailed(
            "No video or audio stream in file".into(),
        ));
    }

    let duration = json
        .format
        .as_ref()
        .and_then(|f| f.duration.as_ref())
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            video
                .and_then(|s| s.duration.as_ref())
                .and_then(|d| d.parse().ok())
        })
        .or_else(|| {
            audio
                .and_then(|s| s.duration.as_ref())
                .and_then(|d| d.parse().ok())
        })
        .unwrap_or(0.0);

    let bit_rate = json
        .format
        .as_ref()
        .and_then(|f| f.bit_rate.as_ref())
        .and_then(|b| b.parse().ok());

    let size_bytes = json
        .format
        .as_ref()
        .and_then(|f| f.size.as_ref())
        .and_then(|s| s.parse().ok());

    let fps = video.and_then(parse_fps);

    Ok(MediaInfo {
        path: path.to_path_buf(),
        duration,
        format_name: json
            .format
            .as_ref()
            .and_then(|f| f.format_name.clone())
            .unwrap_or_else(|| "unknown".into()),
        bit_rate,
        size_bytes,
        has_video: video.is_some(),
        has_audio: audio.is_some(),
        audio_codec: audio.and_then(|s| s.codec_name.clone()),
        sample_rate: audio
            .and_then(|s| s.sample_rate.as_ref())
            .and_then(|s| s.parse().ok()),
        channels: audio.and_then(|s| s.channels),
        video_codec: video.and_then(|s| s.codec_name.clone()),
        width: video.and_then(|s| s.width),
        height: video.and_then(|s| s.height),
        fps,
    })
}

fn is_attached_pic(s: &ProbeStream) -> bool {
    s.disposition
        .as_ref()
        .and_then(|d| d.attached_pic)
        .unwrap_or(0)
        != 0
}

fn parse_fps(s: &ProbeStream) -> Option<f64> {
    let raw = s
        .avg_frame_rate
        .as_deref()
        .filter(|r| *r != "0/0")
        .or(s.r_frame_rate.as_deref().filter(|r| *r != "0/0"))?;
    if let Some((n, d)) = raw.split_once('/') {
        let n: f64 = n.parse().ok()?;
        let d: f64 = d.parse().ok()?;
        if d != 0.0 {
            return Some(n / d);
        }
    }
    raw.parse().ok()
}

pub struct TrimRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub start: f64,
    pub end: f64,
    pub mode: EncodeMode,
    pub total_duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    /// Re-encode params (ignored for StreamCopy).
    pub reencode: Option<&'a crate::encode_settings::ReencodeSettings>,
}

pub fn trim(req: TrimRequest<'_>) -> Result<Duration, FfmpegError> {
    if req.start < 0.0 {
        return Err(FfmpegError::InvalidRange(
            "start cannot be < 0".into(),
        ));
    }
    if req.end <= req.start {
        return Err(FfmpegError::InvalidRange(
            "end must be greater than start".into(),
        ));
    }
    if req.total_duration > 0.0 && req.start >= req.total_duration {
        return Err(FfmpegError::InvalidRange(
            "start is past end of file".into(),
        ));
    }

    if let Some(parent) = req.output.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| FfmpegError::Io(e.to_string()))?;
        }
    }

    let start_s = format_ffmpeg_time(req.start);
    let dur_s = format_ffmpeg_time(req.end - req.start);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-hide_banner").arg("-loglevel").arg("error");

    match req.mode {
        EncodeMode::StreamCopy => {
            // -ss before -i: seek to nearest keyframe
            cmd.arg("-ss")
                .arg(&start_s)
                .arg("-i")
                .arg(req.input)
                .arg("-t")
                .arg(&dur_s)
                // Only v/a/s — data/tmcd/bin_data (GoPro etc.) break mp4 mux
                .args([
                    "-map",
                    "0:v?",
                    "-map",
                    "0:a?",
                    "-map",
                    "0:s?",
                    "-map_metadata",
                    "0",
                    "-map_chapters",
                    "0",
                    "-c",
                    "copy",
                    "-ignore_unknown",
                    "-avoid_negative_ts",
                    "make_zero",
                ]);
            let ext = req
                .output
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(ext.as_str(), "mp4" | "m4v" | "mov" | "m4a") {
                cmd.args(["-movflags", "+faststart"]);
            }
        }
        EncodeMode::Reencode => {
            // -ss after -i: accurate cut after decode
            cmd.arg("-i")
                .arg(req.input)
                .arg("-ss")
                .arg(&start_s)
                .arg("-t")
                .arg(&dur_s)
                .arg("-map_metadata")
                .arg("0");
            if let Some(settings) = req.reencode {
                apply_reencode_settings(&mut cmd, settings, req.has_video, req.has_audio);
            } else {
                let ext = req
                    .output
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                apply_reencode_args(&mut cmd, &ext, req.has_video, req.has_audio);
            }
        }
    }

    cmd.arg(req.output);

    let started = std::time::Instant::now();
    let output = cmd
        .output()
        .map_err(|e| FfmpegError::TrimFailed(e.to_string()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = if !err.trim().is_empty() {
            err.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("exit code {:?}", output.status.code())
        };
        return Err(FfmpegError::TrimFailed(msg));
    }

    Ok(started.elapsed())
}

fn apply_reencode_args(cmd: &mut Command, ext: &str, has_video: bool, has_audio: bool) {
    let mut s = crate::encode_settings::ReencodeSettings::default();
    if !has_video {
        s.video_codec = crate::encode_settings::VideoCodec::None;
    }
    if !has_audio {
        s.audio_codec = crate::encode_settings::AudioCodec::None;
    }
    let _ = ext;
    apply_reencode_settings(cmd, &s, has_video, has_audio);
}

fn apply_reencode_settings(
    cmd: &mut Command,
    s: &crate::encode_settings::ReencodeSettings,
    src_has_video: bool,
    src_has_audio: bool,
) {
    use crate::encode_settings::*;

    let want_video =
        src_has_video && s.video_codec != VideoCodec::None && !s.container.is_audio_only();
    let want_audio = src_has_audio && s.audio_codec != AudioCodec::None;

    if want_video {
        cmd.args(["-map", "0:v:0?"]);
    }
    if want_audio {
        cmd.args(["-map", "0:a:0?"]);
    }
    if want_video {
        cmd.args(["-map", "0:s?", "-c:s", "copy"]);
    }

    let mut vf: Vec<String> = Vec::new();
    if want_video {
        match s.resolution {
            ResolutionOpt::Original => {}
            ResolutionOpt::Custom => {
                let w = s.custom_w.max(16);
                let h = s.custom_h.max(16);
                vf.push(format!("scale={w}:{h}:flags=bicubic"));
            }
            other => {
                if let Some(h) = other.max_height() {
                    vf.push(format!("scale=-2:'min({h},ih)':flags=bicubic"));
                }
            }
        }
        if let Some(fps) = s.fps.value() {
            vf.push(format!("fps={fps}"));
        }
        if !vf.is_empty() {
            cmd.args(["-vf", &vf.join(",")]);
        }

        match s.video_codec {
            VideoCodec::H264 => {
                cmd.args([
                    "-c:v",
                    "libx264",
                    "-preset",
                    s.speed.ffmpeg(),
                    "-pix_fmt",
                    "yuv420p",
                ]);
                match s.rate_mode {
                    VideoRateMode::Crf => {
                        cmd.args(["-crf", &s.crf.clamp(0, 51).to_string()]);
                    }
                    VideoRateMode::Bitrate => {
                        let b = format!("{}k", s.video_bitrate_k.max(100));
                        cmd.args([
                            "-b:v",
                            &b,
                            "-maxrate",
                            &b,
                            "-bufsize",
                            &format!("{}k", s.video_bitrate_k * 2),
                        ]);
                    }
                }
            }
            VideoCodec::H265 => {
                cmd.args([
                    "-c:v",
                    "libx265",
                    "-preset",
                    s.speed.ffmpeg(),
                    "-pix_fmt",
                    "yuv420p",
                    "-tag:v",
                    "hvc1",
                ]);
                match s.rate_mode {
                    VideoRateMode::Crf => {
                        cmd.args(["-crf", &s.crf.clamp(0, 51).to_string()]);
                    }
                    VideoRateMode::Bitrate => {
                        let b = format!("{}k", s.video_bitrate_k.max(100));
                        cmd.args(["-b:v", &b]);
                    }
                }
            }
            VideoCodec::Vp9 => {
                cmd.args(["-c:v", "libvpx-vp9", "-row-mt", "1", "-b:v", "0"]);
                match s.rate_mode {
                    VideoRateMode::Crf => {
                        cmd.args(["-crf", &s.crf.clamp(15, 45).to_string()]);
                    }
                    VideoRateMode::Bitrate => {
                        let b = format!("{}k", s.video_bitrate_k.max(100));
                        cmd.args(["-b:v", &b]);
                    }
                }
            }
            VideoCodec::None => {
                cmd.arg("-vn");
            }
        }
    } else {
        cmd.arg("-vn");
    }

    if want_audio {
        let ab = format!("{}k", s.audio_bitrate_k.max(32));
        match s.audio_codec {
            AudioCodec::Aac => {
                cmd.args(["-c:a", "aac", "-b:a", &ab]);
            }
            AudioCodec::Mp3 => {
                cmd.args(["-c:a", "libmp3lame", "-b:a", &ab]);
            }
            AudioCodec::Opus => {
                cmd.args(["-c:a", "libopus", "-b:a", &ab]);
            }
            AudioCodec::Vorbis => {
                cmd.args(["-c:a", "libvorbis", "-b:a", &ab]);
            }
            AudioCodec::Flac => {
                cmd.args(["-c:a", "flac", "-compression_level", "8"]);
            }
            AudioCodec::Pcm => {
                cmd.args(["-c:a", "pcm_s16le"]);
            }
            AudioCodec::None => {
                cmd.arg("-an");
            }
        }
    } else {
        cmd.arg("-an");
    }

    if matches!(
        s.container,
        ContainerFmt::Mp4 | ContainerFmt::Mov | ContainerFmt::M4a
    ) {
        cmd.args(["-movflags", "+faststart"]);
    }
}

/// Suggest output path: `track.mp4` → `track_cut.mp4`
pub fn suggest_output(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");
    parent.join(format!("{stem}_cut.{ext}"))
}

/// One keep-range for multi-cut export (several pieces joined in order).
#[derive(Debug, Clone, Copy)]
pub struct KeepSegment {
    pub start: f64,
    pub end: f64,
}

impl KeepSegment {
    pub fn duration(self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn is_valid(self) -> bool {
        self.end > self.start + 0.01
    }
}

pub struct MultiTrimRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub segments: &'a [KeepSegment],
    pub mode: EncodeMode,
    pub total_duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub reencode: Option<&'a crate::encode_settings::ReencodeSettings>,
}

/// Cut one or more keep-ranges and concatenate into a single file.
/// Order of `segments` is the order in the output.
pub fn trim_multi(req: MultiTrimRequest<'_>) -> Result<Duration, FfmpegError> {
    if req.segments.is_empty() {
        return Err(FfmpegError::InvalidRange(
            "no keep ranges — add at least one segment".into(),
        ));
    }
    for (i, s) in req.segments.iter().enumerate() {
        if !s.is_valid() {
            return Err(FfmpegError::InvalidRange(format!(
                "segment #{} invalid (end must be > start)",
                i + 1
            )));
        }
    }

    if req.segments.len() == 1 {
        let s = req.segments[0];
        return trim(TrimRequest {
            input: req.input,
            output: req.output,
            start: s.start,
            end: s.end,
            mode: req.mode,
            total_duration: req.total_duration,
            has_video: req.has_video,
            has_audio: req.has_audio,
            reencode: req.reencode,
        });
    }

    let started = std::time::Instant::now();
    let ext = req
        .output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");

    let tmp = std::env::temp_dir().join(format!(
        "kalicut-multi-{}-{}",
        std::process::id(),
        started.elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&tmp).map_err(|e| FfmpegError::Io(e.to_string()))?;

    let cleanup = |dir: &Path| {
        let _ = std::fs::remove_dir_all(dir);
    };

    let mut part_paths: Vec<PathBuf> = Vec::with_capacity(req.segments.len());
    for (i, seg) in req.segments.iter().enumerate() {
        let part = tmp.join(format!("part_{i:03}.{ext}"));
        if let Err(e) = trim(TrimRequest {
            input: req.input,
            output: &part,
            start: seg.start,
            end: seg.end,
            mode: req.mode,
            total_duration: req.total_duration,
            has_video: req.has_video,
            has_audio: req.has_audio,
            reencode: req.reencode,
        }) {
            cleanup(&tmp);
            return Err(e);
        }
        part_paths.push(part);
    }

    // ffmpeg concat demuxer list
    let list_path = tmp.join("concat.txt");
    let mut list_body = String::new();
    for p in &part_paths {
        // Escape single quotes for concat demuxer: ' → '\''
        let abs = p
            .canonicalize()
            .unwrap_or_else(|_| p.clone())
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        list_body.push_str(&format!("file '{abs}'\n"));
    }
    if let Err(e) = std::fs::write(&list_path, list_body) {
        cleanup(&tmp);
        return Err(FfmpegError::Io(e.to_string()));
    }

    if let Some(parent) = req.output.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                cleanup(&tmp);
                return Err(FfmpegError::Io(e.to_string()));
            }
        }
    }

    // Same codecs from one source → stream-copy concat is fine even after re-encode parts
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(&list_path)
        .arg("-c")
        .arg("copy")
        .arg("-ignore_unknown");
    let out_ext = ext.to_ascii_lowercase();
    if matches!(out_ext.as_str(), "mp4" | "m4v" | "mov" | "m4a") {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.arg(req.output);

    let output = cmd.output().map_err(|e| {
        cleanup(&tmp);
        FfmpegError::TrimFailed(e.to_string())
    })?;

    if !output.status.success() {
        cleanup(&tmp);
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(FfmpegError::TrimFailed(if err.trim().is_empty() {
            format!("concat exit {:?}", output.status.code())
        } else {
            err.trim().to_string()
        }));
    }

    cleanup(&tmp);
    Ok(started.elapsed())
}

/// Parse time: `90`, `1:30`, `01:30.5`, `1:02:03.120`
#[allow(dead_code)]
pub fn parse_time(input: &str) -> Result<f64, String> {
    let s = input.trim().replace(',', ".");
    if s.is_empty() {
        return Err("empty value".into());
    }

    if !s.contains(':') {
        return s
            .parse::<f64>()
            .map_err(|_| format!("not a number: {s}"))
            .and_then(|v| {
                if v < 0.0 {
                    Err("time < 0".into())
                } else {
                    Ok(v)
                }
            });
    }

    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.len() {
        2 => (0.0, parts[0], parts[1]),
        3 => (
            parts[0]
                .parse::<f64>()
                .map_err(|_| format!("hours: {}", parts[0]))?,
            parts[1],
            parts[2],
        ),
        _ => return Err("format: SS, MM:SS, or HH:MM:SS".into()),
    };

    let m: f64 = m.parse().map_err(|_| format!("minutes: {m}"))?;
    let sec: f64 = sec.parse().map_err(|_| format!("seconds: {sec}"))?;

    if m >= 60.0 || sec >= 60.0 {
        return Err("minutes/seconds must be < 60".into());
    }

    let total = h * 3600.0 + m * 60.0 + sec;
    if total < 0.0 {
        return Err("time < 0".into());
    }
    Ok(total)
}

pub fn format_seconds(secs: f64) -> String {
    if secs < 0.0 || !secs.is_finite() {
        return "00:00.000".into();
    }
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m:02}:{s:02}.{ms:03}")
    }
}

fn format_ffmpeg_time(secs: f64) -> String {
    format!("{secs:.3}")
}

#[derive(Debug, Deserialize)]
struct ProbeJson {
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    duration: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    disposition: Option<Disposition>,
}

#[derive(Debug, Deserialize)]
struct Disposition {
    attached_pic: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_seconds() {
        assert!((parse_time("12.5").unwrap() - 12.5).abs() < 1e-9);
        assert!((parse_time("90").unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn parse_mm_ss() {
        assert!((parse_time("1:30").unwrap() - 90.0).abs() < 1e-9);
        assert!((parse_time("01:02.5").unwrap() - 62.5).abs() < 1e-9);
    }

    #[test]
    fn parse_hh_mm_ss() {
        assert!((parse_time("1:02:03").unwrap() - 3723.0).abs() < 1e-9);
    }

    #[test]
    fn format_roundtrip_ish() {
        assert_eq!(format_seconds(90.0), "01:30.000");
        assert_eq!(format_seconds(3723.12), "01:02:03.120");
    }
}
