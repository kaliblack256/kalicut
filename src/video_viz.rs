//! Live-видеопоток без proxy-файла.
//! 4K HEVC: VAAPI decode + маленький RGB preview.
//! Video-only: playhead ведётся по времени кадров (нет гонки wall-clock ↔ decode).

use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Размер preview (меньше = плавнее на 4K HEVC).
pub const STREAM_W: u32 = 480;
pub const STREAM_H: u32 = 270;

const FRAME_PIXELS: usize = (STREAM_W * STREAM_H) as usize;
const RGB_BYTES: usize = FRAME_PIXELS * 3;
const RGBA_BYTES: usize = FRAME_PIXELS * 4;

#[derive(Debug, Clone)]
pub struct StreamProfile {
    pub fps: f64,
    pub prefer_hw: bool,
    pub heavy: bool,
    pub label: String,
}

impl Default for StreamProfile {
    fn default() -> Self {
        Self {
            fps: 24.0,
            prefer_hw: false,
            heavy: false,
            label: "live".into(),
        }
    }
}

impl StreamProfile {
    pub fn from_media(
        width: Option<u32>,
        height: Option<u32>,
        codec: Option<&str>,
        src_fps: Option<f64>,
        bit_rate: Option<u64>,
    ) -> Self {
        let w = width.unwrap_or(0);
        let h = height.unwrap_or(0);
        let pixels = w.saturating_mul(h);
        let codec_l = codec.unwrap_or("").to_ascii_lowercase();
        let hevc = codec_l.contains("hevc")
            || codec_l.contains("h265")
            || codec_l == "hev1"
            || codec_l == "hvc1";
        let av1 = codec_l.contains("av1");
        let heavy_codec = hevc || av1;
        let uhd = pixels >= 3_000_000 || w >= 2560;
        let high_br = bit_rate.is_some_and(|b| b >= 20_000_000);
        let heavy = uhd || (heavy_codec && pixels >= 1_500_000) || high_br;

        if heavy {
            // 4K HEVC: меньше fps/пикселей, обязательно пробуем VAAPI
            let fps = if uhd && heavy_codec { 12.0 } else { 15.0 };
            Self {
                fps,
                prefer_hw: true,
                heavy: true,
                label: format!(
                    "live {}×{} {} → {}p@{:.0} VAAPI/CPU",
                    w,
                    h,
                    if hevc {
                        "HEVC"
                    } else if av1 {
                        "AV1"
                    } else {
                        codec.unwrap_or("?")
                    },
                    STREAM_H,
                    fps
                ),
            }
        } else if pixels >= 1_500_000 {
            Self {
                fps: 20.0,
                prefer_hw: true,
                heavy: false,
                label: format!("live {STREAM_W}×{STREAM_H}@20"),
            }
        } else {
            let fps = src_fps
                .filter(|f| f.is_finite() && *f > 1.0)
                .map(|f| f.min(24.0))
                .unwrap_or(24.0);
            Self {
                fps,
                prefer_hw: false,
                heavy: false,
                label: format!("live @{fps:.0}"),
            }
        }
    }
}

struct SharedFrame {
    time_bits: AtomicU64,
    gen: AtomicU64,
    rgb: Mutex<Vec<u8>>,
}

impl SharedFrame {
    fn new() -> Self {
        Self {
            time_bits: AtomicU64::new(f64::NAN.to_bits()),
            gen: AtomicU64::new(0),
            rgb: Mutex::new(vec![0u8; RGB_BYTES]),
        }
    }

    fn store_rgb(&self, t: f64, rgb: &[u8]) {
        if rgb.len() != RGB_BYTES {
            return;
        }
        if let Ok(mut g) = self.rgb.lock() {
            g.copy_from_slice(rgb);
        }
        self.time_bits.store(t.to_bits(), Ordering::Release);
        self.gen.fetch_add(1, Ordering::Release);
    }

    fn gen(&self) -> u64 {
        self.gen.load(Ordering::Acquire)
    }

    fn time(&self) -> Option<f64> {
        if self.gen() == 0 {
            return None;
        }
        let t = f64::from_bits(self.time_bits.load(Ordering::Acquire));
        if t.is_finite() {
            Some(t)
        } else {
            None
        }
    }

    fn copy_rgba_into(&self, rgba_out: &mut [u8]) -> bool {
        if self.gen() == 0 || rgba_out.len() < RGBA_BYTES {
            return false;
        }
        let Ok(g) = self.rgb.lock() else {
            return false;
        };
        for (i, px) in g.chunks_exact(3).enumerate() {
            let o = i * 4;
            rgba_out[o] = px[0];
            rgba_out[o + 1] = px[1];
            rgba_out[o + 2] = px[2];
            rgba_out[o + 3] = 255;
        }
        true
    }
}

#[derive(Clone)]
struct DecodeOpts {
    fps: f64,
    use_vaapi: bool,
    vaapi_device: Option<String>,
    /// true = audio master (skip frames). false = free-run, sleep to fps.
    audio_master: bool,
    heavy: bool,
}

enum StreamCmd {
    Start {
        path: PathBuf,
        from: f64,
        opts: DecodeOpts,
    },
    Still {
        path: PathBuf,
        at: f64,
        opts: DecodeOpts,
    },
    Stop,
    Shutdown,
}

fn kill_child(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn detect_vaapi_device() -> Option<String> {
    for cand in ["/dev/dri/renderD128", "/dev/dri/renderD129"] {
        if Path::new(cand).exists() {
            let ok = Command::new("ffmpeg")
                .args(["-hide_banner", "-hwaccels"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).contains("vaapi"))
                .unwrap_or(false);
            if ok {
                return Some(cand.to_string());
            }
        }
    }
    None
}

pub struct VideoStream {
    cmd_tx: Option<std::sync::mpsc::Sender<StreamCmd>>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<SharedFrame>,
    playhead_bits: Arc<AtomicU64>,
    /// 1 = audio master (drop), 0 = free-run paced
    audio_master: Arc<AtomicBool>,
    streaming_flag: Arc<AtomicBool>,
    pub texture: Option<TextureHandle>,
    rgba_scratch: Vec<u8>,
    last_gen: u64,
    pub source_path: Option<PathBuf>,
    pub profile: StreamProfile,
    pub vaapi_device: Option<String>,
    pub streaming: bool,
    /// Есть ли звук у ролика (для режима master).
    pub has_audio: bool,
    last_still_at: f64,
    last_tex_ms: u64,
}

impl Default for VideoStream {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoStream {
    pub fn new() -> Self {
        let shared = Arc::new(SharedFrame::new());
        let playhead_bits = Arc::new(AtomicU64::new(0f64.to_bits()));
        let audio_master = Arc::new(AtomicBool::new(false));
        let streaming_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<StreamCmd>();
        let vaapi_device = detect_vaapi_device();

        let shared_w = shared.clone();
        let playhead_w = playhead_bits.clone();
        let audio_master_w = audio_master.clone();
        let streaming_w = streaming_flag.clone();

        let worker = thread::spawn(move || {
            worker_loop(rx, shared_w, playhead_w, audio_master_w, streaming_w);
        });

        Self {
            cmd_tx: Some(tx),
            worker: Some(worker),
            shared,
            playhead_bits,
            audio_master,
            streaming_flag,
            texture: None,
            rgba_scratch: vec![0u8; RGBA_BYTES],
            last_gen: 0,
            source_path: None,
            profile: StreamProfile::default(),
            vaapi_device,
            streaming: false,
            has_audio: false,
            last_still_at: -1.0,
            last_tex_ms: 0,
        }
    }

    pub fn clear(&mut self) {
        self.stop();
        self.source_path = None;
        self.texture = None;
        self.last_gen = 0;
        self.last_still_at = -1.0;
        self.profile = StreamProfile::default();
        self.has_audio = false;
    }

    pub fn set_source(&mut self, path: PathBuf, profile: StreamProfile, has_audio: bool) {
        self.source_path = Some(path);
        self.profile = profile;
        self.has_audio = has_audio;
    }

    pub fn profile_label(&self) -> &str {
        &self.profile.label
    }

    pub fn hw_label(&self) -> &'static str {
        if self.profile.prefer_hw && self.vaapi_device.is_some() {
            "VAAPI"
        } else {
            "CPU"
        }
    }

    /// Время последнего кадра (для video-only clock).
    pub fn latest_frame_time(&self) -> Option<f64> {
        self.shared.time()
    }

    fn decode_opts(&self) -> DecodeOpts {
        DecodeOpts {
            fps: self.profile.fps,
            use_vaapi: self.profile.prefer_hw && self.vaapi_device.is_some(),
            vaapi_device: self.vaapi_device.clone(),
            audio_master: self.has_audio,
            heavy: self.profile.heavy,
        }
    }

    pub fn set_playhead(&self, secs: f64) {
        self.playhead_bits
            .store(secs.max(0.0).to_bits(), Ordering::Release);
    }

    pub fn play_from(&mut self, from: f64) {
        let Some(path) = self.source_path.clone() else {
            return;
        };
        self.streaming = true;
        self.streaming_flag.store(true, Ordering::Release);
        self.audio_master
            .store(self.has_audio, Ordering::Release);
        self.set_playhead(from);
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(StreamCmd::Start {
                path,
                from,
                opts: self.decode_opts(),
            });
        }
    }

    pub fn has_realtime_stream(&self) -> bool {
        self.streaming
    }

    pub fn stop(&mut self) {
        self.streaming = false;
        self.streaming_flag.store(false, Ordering::Release);
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(StreamCmd::Stop);
        }
    }

    pub fn show_still(&mut self, at: f64, force: bool) {
        let Some(path) = self.source_path.clone() else {
            return;
        };
        let min_dt = if self.profile.heavy { 0.12 } else { 0.05 };
        if !force && (at - self.last_still_at).abs() < min_dt {
            return;
        }
        self.last_still_at = at;
        if self.streaming {
            self.stop();
        }
        self.set_playhead(at);
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(StreamCmd::Still {
                path,
                at,
                opts: self.decode_opts(),
            });
        }
    }

    pub fn pump_texture(&mut self, ctx: &Context, playhead: f64) -> bool {
        self.set_playhead(playhead);
        let gen = self.shared.gen();
        if gen == 0 || gen == self.last_gen {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        // ≤ profile fps upload
        let min_ms = (1000.0 / self.profile.fps.max(8.0)) as u64;
        if now.saturating_sub(self.last_tex_ms) < min_ms && self.texture.is_some() {
            return false;
        }
        if !self.shared.copy_rgba_into(&mut self.rgba_scratch) {
            return false;
        }
        self.last_gen = gen;
        self.last_tex_ms = now;

        let img = ColorImage::from_rgba_unmultiplied(
            [STREAM_W as usize, STREAM_H as usize],
            &self.rgba_scratch,
        );
        let opts = TextureOptions::NEAREST;
        match &mut self.texture {
            Some(tex) => tex.set(img, opts),
            None => {
                self.texture = Some(ctx.load_texture("video_stream", img, opts));
            }
        }
        true
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        self.streaming_flag.store(false, Ordering::Release);
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(StreamCmd::Shutdown);
        }
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(
    rx: std::sync::mpsc::Receiver<StreamCmd>,
    shared: Arc<SharedFrame>,
    playhead_bits: Arc<AtomicU64>,
    audio_master: Arc<AtomicBool>,
    streaming_flag: Arc<AtomicBool>,
) {
    let mut child: Option<Child> = None;
    let mut frame_idx: u64 = 0;
    let mut start_t = 0.0_f64;
    let mut fps = 15.0_f64;
    let mut buf = vec![0u8; RGB_BYTES];
    let mut skip_buf = vec![0u8; RGB_BYTES];
    let mut present_origin: Option<Instant> = None;

    loop {
        let maybe_cmd = if child.is_some() {
            rx.try_recv().ok()
        } else {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(c) => Some(c),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };

        if let Some(cmd) = maybe_cmd {
            kill_child(&mut child);
            present_origin = None;
            match cmd {
                StreamCmd::Shutdown => break,
                StreamCmd::Stop => {
                    frame_idx = 0;
                    streaming_flag.store(false, Ordering::Release);
                }
                StreamCmd::Still { path, at, opts } => {
                    streaming_flag.store(false, Ordering::Release);
                    if let Ok(rgb) = extract_frame_rgb(&path, at, &opts) {
                        shared.store_rgb(at, &rgb);
                    }
                }
                StreamCmd::Start { path, from, opts } => {
                    start_t = from.max(0.0);
                    fps = opts.fps.clamp(8.0, 30.0);
                    frame_idx = 0;
                    present_origin = Some(Instant::now());
                    streaming_flag.store(true, Ordering::Release);
                    audio_master.store(opts.audio_master, Ordering::Release);
                    match spawn_stream(&path, start_t, &opts) {
                        Ok(c) => child = Some(c),
                        Err(_) => {
                            let mut soft = opts.clone();
                            soft.use_vaapi = false;
                            match spawn_stream(&path, start_t, &soft) {
                                Ok(c) => child = Some(c),
                                Err(_) => {
                                    child = None;
                                    streaming_flag.store(false, Ordering::Release);
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(ref mut c) = child {
            let stdout = match c.stdout.as_mut() {
                Some(s) => s,
                None => {
                    kill_child(&mut child);
                    streaming_flag.store(false, Ordering::Release);
                    continue;
                }
            };

            match read_exact(stdout, &mut buf) {
                Ok(true) => {
                    let t = start_t + frame_idx as f64 / fps;
                    frame_idx += 1;
                    let master = audio_master.load(Ordering::Acquire);
                    let frame_dt = 1.0 / fps;

                    if master {
                        // Аудио-часы: догоняем playhead, лишние кадры выкидываем
                        let ph = f64::from_bits(playhead_bits.load(Ordering::Acquire));
                        if t < ph - frame_dt * 1.5 {
                            let mut used = false;
                            for _ in 0..24 {
                                let cur = start_t + frame_idx as f64 / fps;
                                if cur >= ph - frame_dt * 0.4 {
                                    break;
                                }
                                match read_exact(stdout, &mut skip_buf) {
                                    Ok(true) => {
                                        frame_idx += 1;
                                        used = true;
                                    }
                                    _ => break,
                                }
                            }
                            let show_t = start_t + frame_idx.saturating_sub(1) as f64 / fps;
                            shared.store_rgb(show_t, if used { &skip_buf } else { &buf });
                            continue;
                        }
                        if t > ph + frame_dt * 2.0 {
                            thread::sleep(Duration::from_millis(
                                ((t - ph) * 1000.0).clamp(1.0, 35.0) as u64,
                            ));
                        }
                        shared.store_rgb(t, &buf);
                    } else {
                        // Free-run: показываем каждый кадр, темп ≈ fps от старта
                        // (если decode медленнее realtime — просто чуть «режем» скорость, без дёрганья)
                        if let Some(origin) = present_origin {
                            let target = Duration::from_secs_f64((frame_idx as f64 - 1.0) / fps);
                            let elapsed = origin.elapsed();
                            if elapsed < target {
                                thread::sleep(target - elapsed);
                            }
                        }
                        shared.store_rgb(t, &buf);
                    }
                }
                Ok(false) | Err(_) => {
                    kill_child(&mut child);
                    streaming_flag.store(false, Ordering::Release);
                }
            }
        }
    }
    kill_child(&mut child);
}

fn spawn_stream(path: &Path, from: f64, opts: &DecodeOpts) -> Result<Child, String> {
    let t = format!("{from:.3}");
    let fps = opts.fps.clamp(8.0, 30.0);
    let flags = if opts.heavy { "neighbor" } else { "fast_bilinear" };

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "error"]);
    cmd.args([
        "-fflags",
        "nobuffer+discardcorrupt+fastseek",
        "-flags",
        "low_delay",
        "-probesize",
        "32k",
        "-analyzeduration",
        "0",
    ]);

    let vf = if opts.use_vaapi {
        if let Some(dev) = &opts.vaapi_device {
            cmd.args(["-hwaccel", "vaapi", "-hwaccel_device", dev]);
            // Сначала пробуем scale на GPU (быстрее для 4K)
            // Если драйвер не умеет scale_vaapi — fallback в soft scale ниже при Err spawn? 
            // scale_vaapi может не стартовать — используем безопасный путь: hw decode + soft scale
            // (всё равно ×10–20 быстрее pure CPU HEVC)
        }
        format!(
            "fps={fps:.3}:round=down,scale={STREAM_W}:{STREAM_H}:flags={flags}:force_original_aspect_ratio=decrease,pad={STREAM_W}:{STREAM_H}:(ow-iw)/2:(oh-ih)/2,format=rgb24"
        )
    } else {
        format!(
            "fps={fps:.3}:round=down,scale={STREAM_W}:{STREAM_H}:flags={flags}:force_original_aspect_ratio=decrease,pad={STREAM_W}:{STREAM_H}:(ow-iw)/2:(oh-ih)/2,format=rgb24"
        )
    };

    cmd.args(["-ss", &t, "-i"]).arg(path);
    cmd.args([
        "-an", "-sn", "-dn", "-threads", "0", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgb24",
        "-",
    ]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    cmd.spawn().map_err(|e| e.to_string())
}

fn extract_frame_rgb(path: &Path, time_sec: f64, opts: &DecodeOpts) -> Result<Vec<u8>, String> {
    let t = format!("{:.3}", time_sec.max(0.0));
    let flags = if opts.heavy { "neighbor" } else { "fast_bilinear" };
    let vf = format!(
        "scale={STREAM_W}:{STREAM_H}:flags={flags}:force_original_aspect_ratio=decrease,pad={STREAM_W}:{STREAM_H}:(ow-iw)/2:(oh-ih)/2,format=rgb24"
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "error"]);
    if opts.use_vaapi {
        if let Some(dev) = &opts.vaapi_device {
            cmd.args(["-hwaccel", "vaapi", "-hwaccel_device", dev]);
        }
    }
    // keyframe still — быстрее на HEVC
    if opts.heavy {
        cmd.args(["-skip_frame", "nokey"]);
    }
    cmd.args(["-ss", &t, "-i"]).arg(path);
    cmd.args([
        "-frames:v", "1", "-an", "-sn", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
    ]);

    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.len() < RGB_BYTES {
        if opts.use_vaapi || opts.heavy {
            // soft full frame
            let mut soft = opts.clone();
            soft.use_vaapi = false;
            soft.heavy = false;
            return extract_frame_rgb_basic(path, time_sec, &soft);
        }
        return Err("frame".into());
    }
    Ok(output.stdout[..RGB_BYTES].to_vec())
}

fn extract_frame_rgb_basic(path: &Path, time_sec: f64, opts: &DecodeOpts) -> Result<Vec<u8>, String> {
    let t = format!("{:.3}", time_sec.max(0.0));
    let vf = format!(
        "scale={STREAM_W}:{STREAM_H}:flags=neighbor:force_original_aspect_ratio=decrease,pad={STREAM_W}:{STREAM_H}:(ow-iw)/2:(oh-ih)/2,format=rgb24"
    );
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "error"]);
    if opts.use_vaapi {
        if let Some(dev) = &opts.vaapi_device {
            cmd.args(["-hwaccel", "vaapi", "-hwaccel_device", dev]);
        }
    }
    cmd.args(["-ss", &t, "-i"]).arg(path);
    cmd.args([
        "-frames:v", "1", "-an", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
    ]);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.len() < RGB_BYTES {
        return Err("frame".into());
    }
    Ok(output.stdout[..RGB_BYTES].to_vec())
}

fn read_exact(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut off = 0;
    while off < buf.len() {
        match r.read(&mut buf[off..]) {
            Ok(0) => return Ok(false),
            Ok(n) => off += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}
