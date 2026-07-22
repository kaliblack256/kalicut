//! Декодирование через ffmpeg (PCM) и воспроизведение через rodio.

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const TARGET_RATE: u32 = 44_100;
const TARGET_CHANNELS: u16 = 2;
/// Плотность баров как у SoundCloud (много узких столбиков).
const PEAK_BUCKETS: usize = 2048;

#[derive(Clone)]
pub struct DecodedAudio {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved s16le PCM.
    pub samples: Arc<Vec<i16>>,
    /// Нормированные пики 0..1 для waveform.
    pub peaks: Arc<Vec<f32>>,
    pub duration: f64,
}

impl DecodedAudio {
    pub fn sample_count_frames(&self) -> usize {
        let ch = self.channels as usize;
        if ch == 0 {
            0
        } else {
            self.samples.len() / ch
        }
    }

    pub fn frame_at(&self, secs: f64) -> usize {
        let frames = self.sample_count_frames();
        if frames == 0 {
            return 0;
        }
        let f = (secs * self.sample_rate as f64).round() as isize;
        f.clamp(0, frames as isize - 1) as usize
    }
}

pub fn decode_file(path: &Path) -> Result<DecodedAudio, String> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ac",
            &TARGET_CHANNELS.to_string(),
            "-ar",
            &TARGET_RATE.to_string(),
            "-vn",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;

    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr);
        let msg = msg.trim();
        if msg.is_empty() {
            return Err("ffmpeg could not decode file for playback".into());
        }
        return Err(format!("ffmpeg: {msg}"));
    }

    let mut bytes = output.stdout;

    if bytes.len() < 4 {
        return Err("empty audio stream".into());
    }
    if bytes.len() % 2 != 0 {
        bytes.pop();
    }

    let samples: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();

    let peaks = compute_peaks(&samples, TARGET_CHANNELS, PEAK_BUCKETS);
    let frames = samples.len() / TARGET_CHANNELS as usize;
    let duration = frames as f64 / TARGET_RATE as f64;

    Ok(DecodedAudio {
        path: path.to_path_buf(),
        sample_rate: TARGET_RATE,
        channels: TARGET_CHANNELS,
        samples: Arc::new(samples),
        peaks: Arc::new(peaks),
        duration,
    })
}

fn compute_peaks(samples: &[i16], channels: u16, n: usize) -> Vec<f32> {
    let ch = channels as usize;
    if samples.is_empty() || ch == 0 || n == 0 {
        return vec![0.0; n.max(1)];
    }
    let frames = samples.len() / ch;
    let mut peaks = vec![0.0f32; n];
    let mut rms_acc = vec![0.0f32; n];
    let mut counts = vec![0u32; n];

    for i in 0..frames {
        let mut max_abs = 0.0f32;
        let mut sq = 0.0f32;
        let base = i * ch;
        for c in 0..ch {
            let v = samples[base + c] as f32 / i16::MAX as f32;
            max_abs = max_abs.max(v.abs());
            sq += v * v;
        }
        sq /= ch as f32;
        let bucket = (i as f64 / frames as f64 * n as f64).floor() as usize;
        let bucket = bucket.min(n - 1);
        peaks[bucket] = peaks[bucket].max(max_abs);
        rms_acc[bucket] += sq;
        counts[bucket] += 1;
    }

    // peak + RMS → «вибрации» живее, тишина не совсем плоская
    let mut out = vec![0.0f32; n];
    let mut global_max = 1e-6f32;
    for i in 0..n {
        let rms = if counts[i] > 0 {
            (rms_acc[i] / counts[i] as f32).sqrt()
        } else {
            0.0
        };
        let v = (peaks[i] * 0.72 + rms * 0.55).clamp(0.0, 1.0);
        out[i] = v;
        global_max = global_max.max(v);
    }
    // нормализация к 0.92, чтобы пики не упирались в край
    let scale = 0.92 / global_max;
    for v in &mut out {
        *v = (*v * scale).clamp(0.0, 1.0);
    }
    out
}

/// Состояние плеера для GUI (аудио + wall-clock для video-only).
pub struct PlayerState {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
    decoded: Option<DecodedAudio>,
    /// Длительность медиа (из probe, если нет PCM).
    media_duration: f64,
    playhead: f64,
    /// Позиция (сек) в момент старта текущего sink / wall clock.
    play_base: f64,
    stop_at: Option<f64>,
    playing: bool,
    /// Если нет аудио — wall-clock (пока нет video-clock).
    wall_start: Option<std::time::Instant>,
    /// Video-only: playhead обновляет UI из кадров потока.
    pub video_clock: bool,
    init_error: Option<String>,
}

impl PlayerState {
    pub fn new() -> Self {
        match OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                sink: None,
                decoded: None,
                media_duration: 0.0,
                playhead: 0.0,
                play_base: 0.0,
                stop_at: None,
                playing: false,
                wall_start: None,
                video_clock: false,
                init_error: None,
            },
            Err(e) => Self {
                _stream: None,
                handle: None,
                sink: None,
                decoded: None,
                media_duration: 0.0,
                playhead: 0.0,
                play_base: 0.0,
                stop_at: None,
                playing: false,
                wall_start: None,
                video_clock: false,
                init_error: Some(format!("audio output: {e}")),
            },
        }
    }

    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    pub fn decoded(&self) -> Option<&DecodedAudio> {
        self.decoded.as_ref()
    }

    pub fn set_media_duration(&mut self, d: f64) {
        self.media_duration = d.max(0.0);
    }

    pub fn duration(&self) -> f64 {
        self.decoded
            .as_ref()
            .map(|d| d.duration)
            .filter(|d| *d > 0.0)
            .unwrap_or(self.media_duration)
    }

    pub fn playhead(&self) -> f64 {
        self.playhead
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn has_audio(&self) -> bool {
        self.decoded
            .as_ref()
            .is_some_and(|d| !d.samples.is_empty())
    }

    pub fn set_decoded(&mut self, decoded: Option<DecodedAudio>) {
        self.stop_sink();
        if let Some(ref d) = decoded {
            self.media_duration = d.duration;
        }
        self.decoded = decoded;
        self.playhead = 0.0;
        self.play_base = 0.0;
        self.stop_at = None;
        self.playing = false;
        self.wall_start = None;
        self.video_clock = false;
    }

    /// Принудительно выставить playhead (video-clock / mpv), не останавливая play.
    pub fn set_playhead_live(&mut self, secs: f64) {
        let d = self.duration();
        self.playhead = if d > 0.0 {
            secs.clamp(0.0, d)
        } else {
            secs.max(0.0)
        };
    }

    /// Внешний плеер (mpv): playing + video_clock без rodio.
    pub fn mark_external(&mut self, playing: bool) {
        if playing {
            self.stop_sink();
            self.wall_start = None;
            self.video_clock = true;
            self.playing = true;
        } else {
            self.playing = false;
            self.video_clock = false;
            self.wall_start = None;
            self.stop_at = None;
        }
    }

    pub fn set_playhead(&mut self, secs: f64) {
        let was = self.playing;
        let stop_at = self.stop_at;
        self.stop_sink();
        self.wall_start = None;
        self.video_clock = false;
        self.playing = false;
        let d = self.duration();
        self.playhead = if d > 0.0 {
            secs.clamp(0.0, d)
        } else {
            secs.max(0.0)
        };
        if was {
            self.stop_at = stop_at;
            self.start_playback();
        }
    }

    pub fn play(&mut self, stop_at: Option<f64>) {
        self.stop_at = stop_at;
        self.start_playback();
    }

    pub fn play_selection(&mut self, start: f64, end: f64) {
        let d = self.duration();
        self.playhead = if d > 0.0 {
            start.clamp(0.0, d)
        } else {
            start.max(0.0)
        };
        self.stop_at = Some(end);
        self.start_playback();
    }

    pub fn pause(&mut self) {
        if !self.video_clock {
            self.sync_clock();
        }
        self.stop_sink();
        self.wall_start = None;
        self.video_clock = false;
        self.playing = false;
        self.stop_at = None;
    }

    pub fn stop(&mut self) {
        self.stop_sink();
        self.wall_start = None;
        self.video_clock = false;
        self.playing = false;
        self.stop_at = None;
    }

    pub fn tick(&mut self) {
        if !self.playing {
            return;
        }
        // video_clock: playhead обновляет main из кадров
        if !self.video_clock {
            self.sync_clock();
        }

        if let Some(end) = self.stop_at {
            if self.playhead >= end - 0.005 {
                self.playhead = end;
                self.stop_sink();
                self.wall_start = None;
                self.video_clock = false;
                self.playing = false;
                self.stop_at = None;
                return;
            }
        }

        let d = self.duration();
        if d > 0.0 && self.playhead >= d - 0.01 {
            self.playhead = d;
            self.stop_sink();
            self.wall_start = None;
            self.video_clock = false;
            self.playing = false;
            self.stop_at = None;
            return;
        }

        // Аудио закончилось
        if self.has_audio() && self.sink.as_ref().is_none_or(|s| s.empty()) {
            if self.wall_start.is_none() && !self.video_clock {
                self.playing = false;
                self.sink = None;
                self.stop_at = None;
            }
        }
    }

    fn sync_clock(&mut self) {
        if self.video_clock {
            return;
        }
        if let Some(start) = self.wall_start {
            let pos = self.play_base + start.elapsed().as_secs_f64();
            let d = self.duration();
            self.playhead = if d > 0.0 {
                pos.clamp(0.0, d)
            } else {
                pos.max(0.0)
            };
        } else if let Some(sink) = &self.sink {
            let pos = self.play_base + sink.get_pos().as_secs_f64();
            let d = self.duration();
            self.playhead = if d > 0.0 {
                pos.clamp(0.0, d)
            } else {
                pos.max(0.0)
            };
        }
    }

    fn stop_sink(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
    }

    fn start_playback(&mut self) {
        self.stop_sink();
        self.wall_start = None;
        self.video_clock = false;
        self.play_base = self.playhead;

        // Есть PCM — rodio master clock
        if let Some(decoded) = self.decoded.clone() {
            if !decoded.samples.is_empty() {
                if self.handle.is_none() {
                    self.wall_start = Some(std::time::Instant::now());
                    self.playing = true;
                    return;
                }
                let frame = decoded.frame_at(self.playhead);
                let ch = decoded.channels as usize;
                let start_idx = frame * ch;
                if start_idx >= decoded.samples.len() {
                    self.playing = false;
                    return;
                }
                let slice = decoded.samples[start_idx..].to_vec();
                let source = SamplesBuffer::new(decoded.channels, decoded.sample_rate, slice);
                let source: Box<dyn Source<Item = i16> + Send> = if let Some(end) = self.stop_at {
                    let max_secs = (end - self.playhead).max(0.0);
                    Box::new(source.take_duration(Duration::from_secs_f64(max_secs)))
                } else {
                    Box::new(source)
                };
                let handle = self.handle.as_ref().unwrap();
                match Sink::try_new(handle) {
                    Ok(sink) => {
                        sink.append(source);
                        sink.play();
                        self.sink = Some(sink);
                        self.playing = true;
                        return;
                    }
                    Err(_) => {}
                }
            }
        }

        // Video-only: playhead из кадров потока (main выставит video_clock)
        self.video_clock = true;
        self.playing = true;
    }
}
