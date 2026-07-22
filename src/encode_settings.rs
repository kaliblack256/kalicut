//! Re-encode settings: presets + manual parameters.

use std::path::{Path, PathBuf};

/// Template (preset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodePreset {
    /// CRF 18, source resolution, medium
    HighQuality,
    /// CRF 23, up to 1080p
    Balanced,
    /// 720p, CRF 23, faster
    Web720,
    /// 480p, CRF 28, veryfast
    Mobile480,
    /// Audio only MP3 320k
    AudioMp3,
    /// Audio only AAC 256k
    AudioAac,
    /// Manual settings
    Custom,
}

impl EncodePreset {
    pub fn all() -> &'static [EncodePreset] {
        &[
            EncodePreset::HighQuality,
            EncodePreset::Balanced,
            EncodePreset::Web720,
            EncodePreset::Mobile480,
            EncodePreset::AudioMp3,
            EncodePreset::AudioAac,
            EncodePreset::Custom,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            EncodePreset::HighQuality => "High quality",
            EncodePreset::Balanced => "Balanced",
            EncodePreset::Web720 => "Web 720p",
            EncodePreset::Mobile480 => "Mobile 480p",
            EncodePreset::AudioMp3 => "Audio only · MP3",
            EncodePreset::AudioAac => "Audio only · AAC",
            EncodePreset::Custom => "Custom",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            EncodePreset::HighQuality => "near-lossless, source frame size",
            EncodePreset::Balanced => "good quality, up to Full HD",
            EncodePreset::Web720 => "good for web and messengers",
            EncodePreset::Mobile480 => "small file size",
            EncodePreset::AudioMp3 => "drop video, MP3 320k",
            EncodePreset::AudioAac => "drop video, AAC 256k",
            EncodePreset::Custom => "format, codec, bitrate, resolution by hand",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerFmt {
    Mp4,
    Mkv,
    Webm,
    Mov,
    Mp3,
    M4a,
    Flac,
    Wav,
    Ogg,
}

impl ContainerFmt {
    pub fn all() -> &'static [ContainerFmt] {
        &[
            ContainerFmt::Mp4,
            ContainerFmt::Mkv,
            ContainerFmt::Webm,
            ContainerFmt::Mov,
            ContainerFmt::Mp3,
            ContainerFmt::M4a,
            ContainerFmt::Flac,
            ContainerFmt::Wav,
            ContainerFmt::Ogg,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            ContainerFmt::Mp4 => "MP4",
            ContainerFmt::Mkv => "MKV",
            ContainerFmt::Webm => "WebM",
            ContainerFmt::Mov => "MOV",
            ContainerFmt::Mp3 => "MP3",
            ContainerFmt::M4a => "M4A",
            ContainerFmt::Flac => "FLAC",
            ContainerFmt::Wav => "WAV",
            ContainerFmt::Ogg => "OGG",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            ContainerFmt::Mp4 => "mp4",
            ContainerFmt::Mkv => "mkv",
            ContainerFmt::Webm => "webm",
            ContainerFmt::Mov => "mov",
            ContainerFmt::Mp3 => "mp3",
            ContainerFmt::M4a => "m4a",
            ContainerFmt::Flac => "flac",
            ContainerFmt::Wav => "wav",
            ContainerFmt::Ogg => "ogg",
        }
    }

    pub fn is_audio_only(self) -> bool {
        matches!(
            self,
            ContainerFmt::Mp3
                | ContainerFmt::M4a
                | ContainerFmt::Flac
                | ContainerFmt::Wav
                | ContainerFmt::Ogg
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
    /// Drop video track
    None,
}

impl VideoCodec {
    pub fn all() -> &'static [VideoCodec] {
        &[
            VideoCodec::H264,
            VideoCodec::H265,
            VideoCodec::Vp9,
            VideoCodec::None,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265 / HEVC",
            VideoCodec::Vp9 => "VP9",
            VideoCodec::None => "No video",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Pcm,
    /// Drop audio
    None,
}

impl AudioCodec {
    pub fn all() -> &'static [AudioCodec] {
        &[
            AudioCodec::Aac,
            AudioCodec::Mp3,
            AudioCodec::Opus,
            AudioCodec::Vorbis,
            AudioCodec::Flac,
            AudioCodec::Pcm,
            AudioCodec::None,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            AudioCodec::Aac => "AAC",
            AudioCodec::Mp3 => "MP3",
            AudioCodec::Opus => "Opus",
            AudioCodec::Vorbis => "Vorbis",
            AudioCodec::Flac => "FLAC",
            AudioCodec::Pcm => "PCM (WAV)",
            AudioCodec::None => "No audio",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionOpt {
    Original,
    P2160,
    P1440,
    P1080,
    P720,
    P480,
    P360,
    Custom,
}

impl ResolutionOpt {
    pub fn all() -> &'static [ResolutionOpt] {
        &[
            ResolutionOpt::Original,
            ResolutionOpt::P2160,
            ResolutionOpt::P1440,
            ResolutionOpt::P1080,
            ResolutionOpt::P720,
            ResolutionOpt::P480,
            ResolutionOpt::P360,
            ResolutionOpt::Custom,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            ResolutionOpt::Original => "Source",
            ResolutionOpt::P2160 => "2160p (4K)",
            ResolutionOpt::P1440 => "1440p",
            ResolutionOpt::P1080 => "1080p",
            ResolutionOpt::P720 => "720p",
            ResolutionOpt::P480 => "480p",
            ResolutionOpt::P360 => "360p",
            ResolutionOpt::Custom => "Custom…",
        }
    }

    /// Max height (scale=-2:H).
    pub fn max_height(self) -> Option<u32> {
        match self {
            ResolutionOpt::Original => None,
            ResolutionOpt::P2160 => Some(2160),
            ResolutionOpt::P1440 => Some(1440),
            ResolutionOpt::P1080 => Some(1080),
            ResolutionOpt::P720 => Some(720),
            ResolutionOpt::P480 => Some(480),
            ResolutionOpt::P360 => Some(360),
            ResolutionOpt::Custom => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoRateMode {
    /// CRF (quality, VBR)
    Crf,
    /// Target video bitrate
    Bitrate,
}

impl VideoRateMode {
    pub fn label(self) -> &'static str {
        match self {
            VideoRateMode::Crf => "CRF (quality)",
            VideoRateMode::Bitrate => "Bitrate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsOpt {
    Original,
    F60,
    F30,
    F25,
    F24,
    F15,
}

impl FpsOpt {
    pub fn all() -> &'static [FpsOpt] {
        &[
            FpsOpt::Original,
            FpsOpt::F60,
            FpsOpt::F30,
            FpsOpt::F25,
            FpsOpt::F24,
            FpsOpt::F15,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            FpsOpt::Original => "Source",
            FpsOpt::F60 => "60",
            FpsOpt::F30 => "30",
            FpsOpt::F25 => "25",
            FpsOpt::F24 => "24",
            FpsOpt::F15 => "15",
        }
    }

    pub fn value(self) -> Option<f64> {
        match self {
            FpsOpt::Original => None,
            FpsOpt::F60 => Some(60.0),
            FpsOpt::F30 => Some(30.0),
            FpsOpt::F25 => Some(25.0),
            FpsOpt::F24 => Some(24.0),
            FpsOpt::F15 => Some(15.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeSpeed {
    Ultrafast,
    Veryfast,
    Faster,
    Fast,
    Medium,
    Slow,
}

impl EncodeSpeed {
    pub fn all() -> &'static [EncodeSpeed] {
        &[
            EncodeSpeed::Ultrafast,
            EncodeSpeed::Veryfast,
            EncodeSpeed::Faster,
            EncodeSpeed::Fast,
            EncodeSpeed::Medium,
            EncodeSpeed::Slow,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            EncodeSpeed::Ultrafast => "ultrafast",
            EncodeSpeed::Veryfast => "veryfast",
            EncodeSpeed::Faster => "faster",
            EncodeSpeed::Fast => "fast",
            EncodeSpeed::Medium => "medium",
            EncodeSpeed::Slow => "slow",
        }
    }

    pub fn ffmpeg(self) -> &'static str {
        self.label()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReencodeSettings {
    pub preset: EncodePreset,
    pub container: ContainerFmt,
    pub video_codec: VideoCodec,
    pub audio_codec: AudioCodec,
    pub resolution: ResolutionOpt,
    pub custom_w: u32,
    pub custom_h: u32,
    pub rate_mode: VideoRateMode,
    pub crf: u8,
    pub video_bitrate_k: u32,
    pub audio_bitrate_k: u32,
    pub fps: FpsOpt,
    pub speed: EncodeSpeed,
}

impl Default for ReencodeSettings {
    fn default() -> Self {
        Self::from_preset(EncodePreset::HighQuality)
    }
}

impl ReencodeSettings {
    pub fn from_preset(preset: EncodePreset) -> Self {
        let mut s = Self {
            preset,
            container: ContainerFmt::Mp4,
            video_codec: VideoCodec::H264,
            audio_codec: AudioCodec::Aac,
            resolution: ResolutionOpt::Original,
            custom_w: 1920,
            custom_h: 1080,
            rate_mode: VideoRateMode::Crf,
            crf: 18,
            video_bitrate_k: 4000,
            audio_bitrate_k: 192,
            fps: FpsOpt::Original,
            speed: EncodeSpeed::Medium,
        };
        match preset {
            EncodePreset::HighQuality => {
                s.crf = 18;
                s.speed = EncodeSpeed::Medium;
                s.audio_bitrate_k = 256;
            }
            EncodePreset::Balanced => {
                s.crf = 23;
                s.resolution = ResolutionOpt::P1080;
                s.speed = EncodeSpeed::Faster;
                s.audio_bitrate_k = 160;
            }
            EncodePreset::Web720 => {
                s.crf = 23;
                s.resolution = ResolutionOpt::P720;
                s.speed = EncodeSpeed::Veryfast;
                s.audio_bitrate_k = 128;
            }
            EncodePreset::Mobile480 => {
                s.crf = 28;
                s.resolution = ResolutionOpt::P480;
                s.speed = EncodeSpeed::Ultrafast;
                s.audio_bitrate_k = 96;
                s.fps = FpsOpt::F30;
            }
            EncodePreset::AudioMp3 => {
                s.container = ContainerFmt::Mp3;
                s.video_codec = VideoCodec::None;
                s.audio_codec = AudioCodec::Mp3;
                s.audio_bitrate_k = 320;
            }
            EncodePreset::AudioAac => {
                s.container = ContainerFmt::M4a;
                s.video_codec = VideoCodec::None;
                s.audio_codec = AudioCodec::Aac;
                s.audio_bitrate_k = 256;
            }
            EncodePreset::Custom => {
                s.preset = EncodePreset::Custom;
            }
        }
        s
    }

    pub fn apply_preset(&mut self, preset: EncodePreset) {
        *self = Self::from_preset(preset);
    }

    pub fn touch_custom(&mut self) {
        self.preset = EncodePreset::Custom;
    }

    pub fn extension(&self) -> &'static str {
        self.container.ext()
    }

    pub fn with_output_ext(&self, path: &Path) -> PathBuf {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        parent.join(format!("{stem}.{}", self.extension()))
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![self.container.label().to_string()];
        if self.video_codec != VideoCodec::None {
            parts.push(self.video_codec.label().to_string());
            parts.push(self.resolution.label().to_string());
            match self.rate_mode {
                VideoRateMode::Crf => parts.push(format!("CRF {}", self.crf)),
                VideoRateMode::Bitrate => {
                    parts.push(format!("{}k video", self.video_bitrate_k))
                }
            }
        } else {
            parts.push("no video".into());
        }
        if self.audio_codec != AudioCodec::None {
            parts.push(format!(
                "{} {}k",
                self.audio_codec.label(),
                self.audio_bitrate_k
            ));
        }
        parts.join(" · ")
    }
}
