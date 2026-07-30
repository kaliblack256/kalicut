#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod encode_settings;
mod ffmpeg;
mod mpv_player;
mod player;
mod preview_quality;
mod timeline;
mod video_viz;

use encode_settings::{
    AudioCodec, ContainerFmt, EncodePreset, EncodeSpeed, FpsOpt, ReencodeSettings, ResolutionOpt,
    VideoCodec, VideoRateMode,
};
use eframe::egui;
use ffmpeg::{
    ensure_tools, format_seconds, probe, suggest_output, trim_multi, EncodeMode, FfmpegError,
    KeepSegment, MediaInfo, MultiTrimRequest,
};
use mpv_player::{MpvPlayer, EMBED_H, EMBED_W};
use player::{decode_file, DecodedAudio, PlayerState};
use preview_quality::{machine_hint, resolve_preview_size, PreviewMode};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use timeline::{show_timeline, TimelineState, TimelineVisuals};
use video_viz::{StreamProfile, VideoStream};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 720.0])
            .with_min_inner_size([520.0, 480.0])
            .with_title("KALICUT"),
        ..Default::default()
    };

    eframe::run_native(
        "KALICUT",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(6.0, 3.0);
            style.spacing.button_padding = egui::vec2(6.0, 2.0);
            style.spacing.indent = 12.0;
            style.text_styles.insert(
                egui::TextStyle::Body,
                egui::FontId::new(13.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::new(13.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Small,
                egui::FontId::new(11.0, egui::FontFamily::Proportional),
            );
            cc.egui_ctx.set_style(style);
            Ok(Box::new(App::new()))
        }),
    )
}

enum WorkerMsg {
    Probed(Result<MediaInfo, String>),
    Decoded(Result<DecodedAudio, String>),
    Trimmed(Result<(PathBuf, String), String>),
}

struct App {
    tools_ok: Result<(), String>,
    input_path: Option<PathBuf>,
    output_path: String,
    info: Option<MediaInfo>,
    start_sec: f64,
    end_sec: f64,
    mode: EncodeMode,
    reencode: ReencodeSettings,
    status: String,
    status_ok: Option<bool>,
    busy: bool,
    decoding: bool,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    player: PlayerState,
    timeline: TimelineState,
    video: VideoStream,
    /// playhead при последнем still-кадре (чтобы не дёргать ffmpeg)
    last_video_still: f64,
    /// mpv: аппаратный play видео
    mpv: MpvPlayer,
    /// Ширина окна видео (pts); тянется за угол.
    video_view_w: f32,
    /// Quality превью: авто / качество / скорость (не влияет на обрезку).
    preview_mode: PreviewMode,
    /// Clips after B (blade). Click one, then Delete.
    edit_clips: Vec<KeepSegment>,
    /// Selected green clip index.
    edit_selected: Option<usize>,
    /// Undo stack of edit_clips snapshots.
    edit_undo: Vec<Vec<KeepSegment>>,
    /// Space play through joined program (skip deleted gaps, chain clips).
    program_play: bool,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let tools_ok = ensure_tools().map_err(|e| e.to_string());
        let player = PlayerState::new();
        let mpv = MpvPlayer::new();
        let mut status =
            "B · click piece · Delete · Cut"
                .to_string();
        if mpv.available {
            status.push_str(" · video: embedded mpv (libmpv/hwdec).");
        } else {
            let err = mpv.init_error().unwrap_or("n/a");
            status.push_str(&format!(" · libmpv: {err} · ffmpeg fallback."));
        }
        if let Some(e) = player.init_error() {
            status = format!("{status} Audio: {e}");
        }
        Self {
            tools_ok,
            input_path: None,
            output_path: String::new(),
            info: None,
            start_sec: 0.0,
            end_sec: 0.0,
            mode: EncodeMode::StreamCopy,
            reencode: ReencodeSettings::default(),
            status,
            status_ok: None,
            busy: false,
            decoding: false,
            tx,
            rx,
            player,
            timeline: TimelineState::default(),
            video: VideoStream::new(),
            last_video_still: -1.0,
            mpv,
            video_view_w: 480.0,
            preview_mode: PreviewMode::Auto,
            edit_clips: Vec::new(),
            edit_selected: None,
            edit_undo: Vec::new(),
            program_play: false,
        }
    }

    fn apply_preview_quality(&mut self) {
        let (w, h, codec, br) = match self.info.as_ref() {
            Some(i) => (i.width, i.height, i.video_codec.as_deref(), i.bit_rate),
            None => (None, None, None, None),
        };
        let size = resolve_preview_size(self.preview_mode, w, h, codec, br);
        if self.mpv.available {
            self.mpv.set_render_size(size);
        }
        // ffmpeg-fallback: обновить target в профиле стрима при следующем play
        if let Some(info) = &self.info {
            if info.has_video {
                if let Some(path) = self.input_path.clone() {
                    let mut profile = StreamProfile::from_media(
                        info.width,
                        info.height,
                        info.video_codec.as_deref(),
                        info.fps,
                        info.bit_rate,
                    );
                    // подмешать max long edge из preview size
                    profile.label = format!(
                        "{} · preview {}",
                        profile.label.split(" · ").next().unwrap_or("live"),
                        size.label()
                    );
                    let has_audio = info.has_audio;
                    self.video.set_source(path, profile, has_audio);
                }
            }
        }
    }

    /// Video + mpv установлен → play через mpv.
    fn use_mpv(&self) -> bool {
        self.mpv.available && self.info.as_ref().is_some_and(|i| i.has_video)
    }

    fn duration(&self) -> f64 {
        let d = self.player.duration();
        if d > 0.0 {
            d
        } else {
            self.info.as_ref().map(|i| i.duration).unwrap_or(0.0)
        }
    }

    fn clamp_range(&mut self) {
        let d = self.duration();
        if d > 0.0 {
            self.start_sec = self.start_sec.clamp(0.0, d);
            self.end_sec = self.end_sec.clamp(0.0, d);
        } else {
            self.start_sec = self.start_sec.max(0.0);
            self.end_sec = self.end_sec.max(0.0);
        }
        if self.end_sec <= self.start_sec {
            let min_len = 0.05;
            if d > 0.0 {
                if self.start_sec + min_len <= d {
                    self.end_sec = self.start_sec + min_len;
                } else {
                    self.start_sec = (d - min_len).max(0.0);
                    self.end_sec = d;
                }
            } else {
                self.end_sec = self.start_sec + min_len;
            }
        }
    }

    fn open_file(&mut self) {
        if self.busy {
            return;
        }
        let path = rfd::FileDialog::new()
            .add_filter(
                "Media (video + audio)",
                &[
                    "mp4", "mkv", "webm", "mov", "m4v", "avi", "wmv", "flv", "ts", "mts", "m2ts",
                    "mpg", "mpeg", "3gp", "ogv", "mp3", "flac", "wav", "m4a", "aac", "ogg", "oga",
                    "opus", "wma", "aiff", "aif", "alac",
                ],
            )
            .add_filter(
                "Video",
                &[
                    "mp4", "mkv", "webm", "mov", "m4v", "avi", "wmv", "flv", "ts", "mts", "m2ts",
                    "mpg", "mpeg", "3gp", "ogv",
                ],
            )
            .add_filter(
                "Audio",
                &[
                    "mp3", "flac", "wav", "m4a", "aac", "ogg", "oga", "opus", "wma", "aiff", "aif",
                    "alac",
                ],
            )
            .add_filter("All files", &["*"])
            .set_title("Select audio or video")
            .pick_file();

        if let Some(path) = path {
            self.load_path(path);
        }
    }

    fn load_path(&mut self, path: PathBuf) {
        self.player.stop();
        self.player.set_decoded(None);
        self.video.clear();
        self.mpv.clear_media();
        self.last_video_still = -1.0;
        self.input_path = Some(path.clone());
        self.output_path = suggest_output(&path).display().to_string();
        self.info = None;
        self.start_sec = 0.0;
        self.end_sec = 0.0;
        self.edit_clips.clear();
        self.edit_selected = None;
        self.edit_undo.clear();
        self.status = format!("Probing: {}…", path.display());
        self.status_ok = None;
        self.busy = true;
        self.decoding = false;

        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = probe(&path).map_err(|e: FfmpegError| e.to_string());
            let _ = tx.send(WorkerMsg::Probed(result));
        });
    }

    fn start_decode(&mut self, path: PathBuf) {
        self.decoding = true;
        self.status = "Decoding waveform / audio…".into();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = decode_file(&path);
            let _ = tx.send(WorkerMsg::Decoded(result));
        });
    }

    fn sync_video_with_player(&mut self, was_playing: bool) {
        let has_video = self.info.as_ref().is_some_and(|i| i.has_video);
        if !has_video {
            return;
        }
        let playing = self.player.is_playing();
        let ph = self.player.playhead();

        // --- mpv path ---
        if self.use_mpv() {
            let path = match self.input_path.clone() {
                Some(p) => p,
                None => return,
            };
            if playing && !was_playing {
                let (from, stop) = self
                    .program_play_range(ph)
                    .map(|(a, b)| (a, Some(b)))
                    .unwrap_or((ph, Some(self.end_sec)));
                self.program_play = true;
                match self.mpv.play_from(&path, from, stop) {
                    Ok(()) => {
                        self.player.mark_external(true);
                        self.status = "▶ program play · gaps skipped".into();
                        self.status_ok = Some(true);
                    }
                    Err(e) => {
                        self.program_play = false;
                        self.player.mark_external(false);
                        self.status = format!("mpv: {e}");
                        self.status_ok = Some(false);
                    }
                }
            } else if !playing && was_playing {
                self.program_play = false;
                let _ = self.mpv.pause();
                self.player.mark_external(false);
            } else if !playing {
                if (ph - self.last_video_still).abs() > 0.05 {
                    let _ = self.mpv.seek(ph);
                    self.last_video_still = ph;
                }
            }
            return;
        }

        // --- ffmpeg live fallback ---
        if playing && !was_playing {
            self.video.play_from(ph);
        } else if !playing && was_playing {
            self.video.stop();
            self.video.show_still(ph, true);
            self.last_video_still = ph;
        } else if !playing {
            if (ph - self.last_video_still).abs() > 0.04 {
                self.video.show_still(ph, false);
                self.last_video_still = ph;
            }
        }
    }

    fn pick_output(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Save as");
        if let Some(ref input) = self.input_path {
            if let Some(parent) = input.parent() {
                dialog = dialog.set_directory(parent);
            }
            if let Some(name) = suggest_output(input).file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        }
        if let Some(path) = dialog.save_file() {
            self.output_path = path.display().to_string();
        }
    }




    fn init_edit_timeline(&mut self) {
        let d = self.duration();
        if d <= 0.05 {
            self.edit_clips.clear();
            self.edit_selected = None;
            return;
        }
        self.edit_clips = vec![KeepSegment {
            start: 0.0,
            end: d,
        }];
        self.edit_selected = Some(0);
        self.edit_undo.clear();
    }

    fn push_undo(&mut self) {
        self.edit_undo.push(self.edit_clips.clone());
        if self.edit_undo.len() > 32 {
            self.edit_undo.remove(0);
        }
    }

    fn undo_edit(&mut self) {
        if let Some(prev) = self.edit_undo.pop() {
            self.edit_clips = prev;
            if self.edit_clips.is_empty() {
                self.edit_selected = None;
            } else {
                self.edit_selected =
                    Some(self.edit_selected.unwrap_or(0).min(self.edit_clips.len() - 1));
            }
            let left: f64 = self.edit_clips.iter().map(|s| s.duration()).sum();
            self.status = format!("Undo · {} piece(s) · {}", self.edit_clips.len(), format_seconds(left));
            self.status_ok = Some(true);
        }
    }


    /// Click green clip on the scale. Does **not** move the playhead
    /// (caller keeps the click position so the line does not jump to 0 / clip start).
    fn select_clip(&mut self, i: usize) {
        if i >= self.edit_clips.len() {
            return;
        }
        self.edit_selected = Some(i);
        let s = self.edit_clips[i];
        self.start_sec = s.start;
        self.end_sec = s.end;
        self.clamp_range();
        self.status = format!(
            "Selected #{}  {}–{}  · Delete to drop",
            i + 1,
            format_seconds(s.start),
            format_seconds(s.end)
        );
        self.status_ok = Some(true);
    }

    /// B — blade at playhead (repeat to slice more).
    fn blade_at_playhead(&mut self) {
        if self.busy || self.input_path.is_none() {
            return;
        }
        if self.edit_clips.is_empty() {
            self.init_edit_timeline();
        }
        let t = self.player.playhead();
        let min_edge = 0.05_f64;
        let Some(i) = self
            .edit_clips
            .iter()
            .position(|s| t > s.start + min_edge && t < s.end - min_edge)
        else {
            self.status = "Put playhead on a green piece, then B.".into();
            self.status_ok = Some(false);
            return;
        };
        self.push_undo();
        let old = self.edit_clips[i];
        self.edit_clips[i] = KeepSegment {
            start: old.start,
            end: t,
        };
        self.edit_clips.insert(
            i + 1,
            KeepSegment {
                start: t,
                end: old.end,
            },
        );
        // Don't force selection — user clicks the junk piece
        self.edit_selected = None;
        self.status = format!(
            "Blade @ {} · {} pieces · click one → Delete",
            format_seconds(t),
            self.edit_clips.len()
        );
        self.status_ok = Some(true);
    }

    /// Delete — drop the clicked/selected green piece.
    fn delete_selected_clip(&mut self) {
        if self.busy || self.input_path.is_none() {
            return;
        }
        if self.edit_clips.is_empty() {
            self.init_edit_timeline();
        }
        let Some(i) = self.edit_selected else {
            self.status = "Click a green piece first, then Delete.".into();
            self.status_ok = Some(false);
            return;
        };
        if i >= self.edit_clips.len() {
            self.edit_selected = None;
            return;
        }
        if self.edit_clips.len() == 1 {
            self.status = "Can't delete the only piece — blade first to split.".into();
            self.status_ok = Some(false);
            return;
        }
        self.push_undo();
        // Before remove: remember where to park playhead (inside LEFT remaining piece)
        let ph_after = if i > 0 {
            // deleted a piece after the first → stay near end of previous (still "first side")
            let left = self.edit_clips[i - 1];
            (left.end - 0.15).max(left.start)
        } else if i + 1 < self.edit_clips.len() {
            // deleted the first piece → start of what becomes the new first
            self.edit_clips[i + 1].start
        } else {
            0.0
        };
        let _removed = self.edit_clips.remove(i);
        // Ripple on the scale: pieces join; playhead stays on the left fragment
        if self.edit_clips.is_empty() {
            self.edit_selected = None;
            self.player.set_playhead(0.0);
        } else {
            // Select whichever clip contains ph_after (prefer left)
            let sel = self
                .edit_clips
                .iter()
                .position(|c| ph_after >= c.start - 1e-3 && ph_after <= c.end + 1e-3)
                .unwrap_or(0);
            self.edit_selected = Some(sel);
            let s = self.edit_clips[sel];
            self.start_sec = s.start;
            self.end_sec = s.end;
            let ph = ph_after.clamp(s.start, (s.end - 0.02).max(s.start));
            self.player.set_playhead(ph);
            if self.use_mpv() {
                let _ = self.mpv.seek(ph);
            } else if self.info.as_ref().is_some_and(|inf| inf.has_video) {
                self.video.show_still(ph, true);
                self.last_video_still = ph;
            }
        }
        let left: f64 = self.edit_clips.iter().map(|s| s.duration()).sum();
        self.status = format!(
            "Deleted · pieces joined · {} left · Cut when done",
            format_seconds(left)
        );
        self.status_ok = Some(true);
    }

    fn export_segments(&self) -> Vec<KeepSegment> {
        self.keep_segments_list()
    }

    /// Kept pieces in order (full file if never bladed).
    fn keep_segments_list(&self) -> Vec<KeepSegment> {
        if !self.edit_clips.is_empty() {
            return self.edit_clips.clone();
        }
        let d = self.duration();
        if d > 0.05 {
            vec![KeepSegment {
                start: 0.0,
                end: d,
            }]
        } else {
            vec![]
        }
    }

    /// Contiguous play range from `from` in source time until a deleted gap (or end).
    /// At a cut boundary, prefer the **left** clip so the first fragment still plays.
    fn program_play_range(&self, from: f64) -> Option<(f64, f64)> {
        let clips = self.keep_segments_list();
        if clips.is_empty() {
            return None;
        }
        let from = from.max(0.0);

        // Prefer clip containing `from` (end-inclusive → left clip wins on a cut line)
        let mut idx: Option<usize> = None;
        for (i, c) in clips.iter().enumerate() {
            if from >= c.start - 1e-3 && from <= c.end + 1e-3 {
                idx = Some(i);
                break;
            }
        }
        // Deleted gap → next keep after the gap
        if idx.is_none() {
            for (i, c) in clips.iter().enumerate() {
                if from < c.start {
                    idx = Some(i);
                    break;
                }
            }
        }
        // Past all keeps → first clip (start of program), never the last by default
        let i = idx.unwrap_or(0);

        let start = if from >= clips[i].start && from < clips[i].end {
            from
        } else if from >= clips[i].end - 0.05 && from <= clips[i].end + 0.05 {
            // On the right edge of this clip: start slightly earlier so left piece plays
            (clips[i].end - 0.25).max(clips[i].start)
        } else {
            clips[i].start
        };

        // Extend only through source-adjacent clips (no deleted gap)
        let mut end = clips[i].end;
        let mut j = i;
        while j + 1 < clips.len() {
            let next = &clips[j + 1];
            if next.start <= end + 0.08 {
                end = next.end;
                j += 1;
            } else {
                break;
            }
        }
        Some((start, end))
    }

    /// If playback hit end of a keep-run, next segment after a gap.
    fn next_program_after(&self, source_t: f64) -> Option<(f64, f64)> {
        let clips = self.keep_segments_list();
        for (i, c) in clips.iter().enumerate() {
            // At or just past end of this clip/run
            if source_t >= c.end - 0.12 && source_t <= c.end + 0.25 {
                // skip adjacent chain already covered by stop_at
                let mut j = i;
                let mut end = c.end;
                while j + 1 < clips.len() && clips[j + 1].start <= end + 0.08 {
                    j += 1;
                    end = clips[j].end;
                }
                if source_t < end - 0.12 {
                    return None; // still inside contiguous run
                }
                if j + 1 < clips.len() {
                    let n = &clips[j + 1];
                    if let Some((_, stop)) = self.program_play_range(n.start) {
                        return Some((n.start, stop));
                    }
                }
                return None;
            }
        }
        // In a deleted gap: jump to next keep
        for c in &clips {
            if c.start > source_t + 0.01 {
                if let Some((_, stop)) = self.program_play_range(c.start) {
                    return Some((c.start, stop));
                }
            }
        }
        None
    }

    /// Chain play across joined pieces (skip deleted source gaps).
    fn continue_program_play(&mut self, source_t: f64) -> bool {
        if !self.program_play {
            return false;
        }
        let Some(path) = self.input_path.clone() else {
            return false;
        };
        let Some((from, stop)) = self.next_program_after(source_t) else {
            return false;
        };
        match self.mpv.play_from(&path, from, Some(stop)) {
            Ok(()) => {
                self.player.set_playhead_live(from);
                self.player.mark_external(true);
                true
            }
            Err(e) => {
                self.status = format!("play: {e}");
                self.status_ok = Some(false);
                self.program_play = false;
                false
            }
        }
    }

    fn do_trim(&mut self) {
        if self.busy {
            return;
        }
        let Some(ref input) = self.input_path else {
            self.status = "Select a file first.".into();
            self.status_ok = Some(false);
            return;
        };
        if self.output_path.trim().is_empty() {
            self.status = "Set an output path.".into();
            self.status_ok = Some(false);
            return;
        }

        let segments = self.export_segments();
        if segments.is_empty() || segments.iter().any(|s| !s.is_valid()) {
            self.status = "Nothing to export.".into();
            self.status_ok = Some(false);
            return;
        }

        let total = self.duration();
        let mut output = PathBuf::from(self.output_path.trim());
        let mode = self.mode;
        let has_video = self.info.as_ref().is_some_and(|i| i.has_video);
        let has_audio = self.info.as_ref().is_some_and(|i| i.has_audio);
        let reencode = self.reencode.clone();

        if mode == EncodeMode::Reencode {
            output = reencode.with_output_ext(&output);
            self.output_path = output.display().to_string();
        }

        self.player.pause();
        let _ = self.mpv.pause();
        self.player.mark_external(false);
        self.busy = true;
        let n = segments.len();
        let keep: f64 = segments.iter().map(|s| s.duration()).sum();
        self.status = if n == 1 {
            format!(
                "Cutting {} → {} …",
                format_seconds(segments[0].start),
                format_seconds(segments[0].end)
            )
        } else {
            format!("Cutting {} pieces ({}) …", n, format_seconds(keep))
        };
        self.status_ok = None;

        let input = input.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let re_ref = match mode {
                EncodeMode::Reencode => Some(&reencode),
                EncodeMode::StreamCopy => None,
            };
            let result = trim_multi(MultiTrimRequest {
                input: &input,
                output: &output,
                segments: &segments,
                mode,
                total_duration: total,
                has_video,
                has_audio,
                reencode: re_ref,
            })
            .map(|elapsed| {
                let msg = format!(
                    "Done in {:.2}s → {}",
                    elapsed.as_secs_f64(),
                    output.display()
                );
                (output, msg)
            })
            .map_err(|e: FfmpegError| e.to_string());
            let _ = tx.send(WorkerMsg::Trimmed(result));
        });
    }

    fn sync_output_ext_from_reencode(&mut self) {
        if self.mode != EncodeMode::Reencode || self.output_path.trim().is_empty() {
            return;
        }
        let p = PathBuf::from(self.output_path.trim());
        self.output_path = self.reencode.with_output_ext(&p).display().to_string();
    }

    fn ui_reencode_settings(&mut self, ui: &mut egui::Ui) {
        // Шаблон — один ComboBox (компактно)
        ui.horizontal(|ui| {
            ui.label("Preset:");
            egui::ComboBox::from_id_salt("preset")
                .width(160.0)
                .selected_text(self.reencode.preset.label())
                .show_ui(ui, |ui| {
                    for p in EncodePreset::all() {
                        if ui
                            .selectable_value(&mut self.reencode.preset, *p, p.label())
                            .on_hover_text(p.hint())
                            .changed()
                        {
                            self.reencode.apply_preset(*p);
                            self.sync_output_ext_from_reencode();
                        }
                    }
                });
            ui.label(
                egui::RichText::new(self.reencode.preset.hint())
                    .weak()
                    .size(11.0),
            );
        });

        // 4 колонки: плотная сетка
        egui::Grid::new("reencode_grid")
            .num_columns(4)
            .spacing([8.0, 3.0])
            .min_col_width(70.0)
            .show(ui, |ui| {
                ui.label("Container");
                let mut cont_changed = false;
                egui::ComboBox::from_id_salt("container")
                    .width(100.0)
                    .selected_text(self.reencode.container.label())
                    .show_ui(ui, |ui| {
                        for c in ContainerFmt::all() {
                            if ui
                                .selectable_value(&mut self.reencode.container, *c, c.label())
                                .changed()
                            {
                                cont_changed = true;
                            }
                        }
                    });
                if cont_changed {
                    self.reencode.touch_custom();
                    if self.reencode.container.is_audio_only() {
                        self.reencode.video_codec = VideoCodec::None;
                    }
                    if self.reencode.container == ContainerFmt::Webm {
                        if matches!(
                            self.reencode.video_codec,
                            VideoCodec::H264 | VideoCodec::H265
                        ) {
                            self.reencode.video_codec = VideoCodec::Vp9;
                        }
                        if matches!(
                            self.reencode.audio_codec,
                            AudioCodec::Aac | AudioCodec::Mp3
                        ) {
                            self.reencode.audio_codec = AudioCodec::Opus;
                        }
                    }
                    self.sync_output_ext_from_reencode();
                }

                ui.label("Video");
                egui::ComboBox::from_id_salt("vcodec")
                    .width(110.0)
                    .selected_text(self.reencode.video_codec.label())
                    .show_ui(ui, |ui| {
                        for c in VideoCodec::all() {
                            if ui
                                .selectable_value(&mut self.reencode.video_codec, *c, c.label())
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                        }
                    });
                ui.end_row();

                if self.reencode.video_codec != VideoCodec::None {
                    ui.label("Res");
                    egui::ComboBox::from_id_salt("res")
                        .width(110.0)
                        .selected_text(self.reencode.resolution.label())
                        .show_ui(ui, |ui| {
                            for r in ResolutionOpt::all() {
                                if ui
                                    .selectable_value(
                                        &mut self.reencode.resolution,
                                        *r,
                                        r.label(),
                                    )
                                    .changed()
                                {
                                    self.reencode.touch_custom();
                                }
                            }
                        });

                    ui.label("FPS");
                    egui::ComboBox::from_id_salt("fps")
                        .width(100.0)
                        .selected_text(self.reencode.fps.label())
                        .show_ui(ui, |ui| {
                            for f in FpsOpt::all() {
                                if ui
                                    .selectable_value(&mut self.reencode.fps, *f, f.label())
                                    .changed()
                                {
                                    self.reencode.touch_custom();
                                }
                            }
                        });
                    ui.end_row();

                    if self.reencode.resolution == ResolutionOpt::Custom {
                        ui.label("W×H");
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.reencode.custom_w)
                                        .range(16..=7680)
                                        .speed(10.0),
                                )
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                            ui.label("×");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.reencode.custom_h)
                                        .range(16..=4320)
                                        .speed(10.0),
                                )
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                        });
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }

                    ui.label("Quality");
                    ui.horizontal(|ui| {
                        for m in [VideoRateMode::Crf, VideoRateMode::Bitrate] {
                            if ui
                                .selectable_value(&mut self.reencode.rate_mode, m, m.label())
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                        }
                    });
                    match self.reencode.rate_mode {
                        VideoRateMode::Crf => {
                            ui.label("CRF");
                            if ui
                                .add(egui::Slider::new(&mut self.reencode.crf, 0..=40).fixed_decimals(0))
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                        }
                        VideoRateMode::Bitrate => {
                            ui.label("V-kbit");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.reencode.video_bitrate_k)
                                        .range(100..=50000)
                                        .suffix("k")
                                        .speed(50.0),
                                )
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                        }
                    }
                    ui.end_row();

                    ui.label("Speed");
                    egui::ComboBox::from_id_salt("speed")
                        .width(100.0)
                        .selected_text(self.reencode.speed.label())
                        .show_ui(ui, |ui| {
                            for s in EncodeSpeed::all() {
                                if ui
                                    .selectable_value(&mut self.reencode.speed, *s, s.label())
                                    .changed()
                                {
                                    self.reencode.touch_custom();
                                }
                            }
                        });
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                }

                ui.label("Audio");
                egui::ComboBox::from_id_salt("acodec")
                    .width(100.0)
                    .selected_text(self.reencode.audio_codec.label())
                    .show_ui(ui, |ui| {
                        for c in AudioCodec::all() {
                            if ui
                                .selectable_value(&mut self.reencode.audio_codec, *c, c.label())
                                .changed()
                            {
                                self.reencode.touch_custom();
                            }
                        }
                    });

                if self.reencode.audio_codec != AudioCodec::None
                    && self.reencode.audio_codec != AudioCodec::Flac
                    && self.reencode.audio_codec != AudioCodec::Pcm
                {
                    ui.label("A-kbit");
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.reencode.audio_bitrate_k)
                                .range(32..=512)
                                .suffix("k")
                                .speed(8.0),
                        )
                        .changed()
                    {
                        self.reencode.touch_custom();
                    }
                } else {
                    ui.label("");
                    ui.label("");
                }
                ui.end_row();
            });

        ui.label(
            egui::RichText::new(format!("→ {}", self.reencode.summary()))
                .monospace()
                .weak()
                .size(11.0),
        );
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::Probed(Ok(info)) => {
                    self.end_sec = info.duration;
                    self.start_sec = 0.0;
                    self.clamp_range();
                    self.status = format!(
                        "Loaded: {} · {} · {} · {}",
                        info.duration_label(),
                        info.kind_label(),
                        info.codecs_label(),
                        info.format_name
                    );
                    self.status_ok = Some(true);
                    let path = info.path.clone();
                    let path = if path.as_os_str().is_empty() {
                        self.input_path.clone()
                    } else {
                        Some(path)
                    };
                    let has_audio = info.has_audio;
                    let has_video = info.has_video;
                    let duration = info.duration;
                    let profile = StreamProfile::from_media(
                        info.width,
                        info.height,
                        info.video_codec.as_deref(),
                        info.fps,
                        info.bit_rate,
                    );
                    self.player.set_media_duration(duration);
                    self.info = Some(info);
                    self.init_edit_timeline();
                    self.busy = false;
                    if let Some(ref p) = path {
                        if has_video {
                            let label = profile.label.clone();
                            let hw = if profile.prefer_hw && self.video.vaapi_device.is_some() {
                                "VAAPI"
                            } else {
                                "CPU"
                            };
                            self.video.set_source(p.clone(), profile, has_audio);
                            self.video.show_still(0.0, true);
                            self.last_video_still = 0.0;
                            if self.mpv.available {
                                self.apply_preview_quality();
                                self.status = format!(
                                    "Video (libmpv) · preview {} · {}",
                                    self.mpv.render_size_label(),
                                    label
                                );
                                if let Err(e) = self.mpv.load(p) {
                                    self.status = format!("mpv load: {e} · fallback {hw}");
                                } else {
                                    let _ = self.mpv.seek(0.0);
                                }
                                self.status_ok = Some(true);
                            } else if !has_audio {
                                self.apply_preview_quality();
                                self.status = format!(
                                    "libmpv unavailable · ffmpeg preview · {label} · {hw}"
                                );
                                self.status_ok = Some(true);
                            }
                        }
                        if has_audio {
                            self.start_decode(p.clone());
                        } else {
                            self.decoding = false;
                            self.player.set_decoded(None);
                        }
                    }
                }
                WorkerMsg::Probed(Err(e)) => {
                    self.info = None;
                    self.status = e;
                    self.status_ok = Some(false);
                    self.busy = false;
                }
                WorkerMsg::Decoded(Ok(decoded)) => {
                    if self.end_sec <= 0.0 || (self.end_sec - decoded.duration).abs() < 0.5 {
                        self.end_sec = decoded.duration;
                        self.clamp_range();
                    }
                    self.player.set_decoded(Some(decoded));
                    self.decoding = false;
                    self.status = "B · click · Delete · Cut"
                        .into();
                    self.status_ok = Some(true);
                }
                WorkerMsg::Decoded(Err(e)) => {
                    self.decoding = false;
                    self.status = format!(
                        "Audio/waveform: {e}. Video and cut still work."
                    );
                    self.status_ok = Some(false);
                }
                WorkerMsg::Trimmed(Ok((_path, msg))) => {
                    self.busy = false;
                    self.status = msg;
                    self.status_ok = Some(true);
                }
                WorkerMsg::Trimmed(Err(e)) => {
                    self.busy = false;
                    self.status = e;
                    self.status_ok = Some(false);
                }
            }
        }
        if self.busy
            || self.decoding
            || self.player.is_playing()
            || self.video.streaming
            || (self.use_mpv() && self.mpv.is_running())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    fn selection_duration(&self) -> Option<f64> {
        if self.end_sec > self.start_sec {
            Some(self.end_sec - self.start_sec)
        } else {
            None
        }
    }

    fn can_play(&self) -> bool {
        if self.busy {
            return false;
        }
        if self.use_mpv() {
            return self.input_path.is_some();
        }
        if self.decoding {
            return false;
        }
        self.player.has_audio() || self.info.as_ref().is_some_and(|i| i.has_video)
    }

    /// B blade · click clip · Delete · Ctrl+Z undo · Space play.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (space, blade, del, undo) = ctx.input(|i| {
            let b = i.key_pressed(egui::Key::B);
            // Plain B or Ctrl/Cmd+B (Resolve-style)
            let blade = b;
            (
                i.key_pressed(egui::Key::Space),
                blade,
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::Z),
            )
        });
        if space {
            self.toggle_playback();
        }
        if blade {
            self.blade_at_playhead();
        }
        if del {
            self.delete_selected_clip();
        }
        if undo {
            self.undo_edit();
        }
    }

    fn toggle_playback(&mut self) {
        if !self.can_play() {
            return;
        }
        let was = self.player.is_playing();

        if self.use_mpv() {
            // mpv: continuous program play (joined clips, skip gaps)
            if was {
                self.program_play = false;
                let _ = self.mpv.pause();
                self.player.mark_external(false);
                let ph = self.player.playhead();
                self.video.show_still(ph, true);
                self.last_video_still = ph;
            } else {
                let path = match self.input_path.clone() {
                    Some(p) => p,
                    None => return,
                };
                let ph = self.player.playhead();
                let Some((from, stop)) = self.program_play_range(ph) else {
                    self.status = "Nothing to play.".into();
                    self.status_ok = Some(false);
                    return;
                };
                self.program_play = true;
                match self.mpv.play_from(&path, from, Some(stop)) {
                    Ok(()) => {
                        self.player.set_playhead_live(from);
                        self.player.mark_external(true);
                        self.status = "▶ program play · gaps skipped".into();
                        self.status_ok = Some(true);
                    }
                    Err(e) => {
                        self.program_play = false;
                        self.status = format!("mpv: {e}");
                        self.status_ok = Some(false);
                    }
                }
            }
            return;
        }

        // audio / ffmpeg fallback — play through keep segments one by one
        if was {
            self.program_play = false;
            self.player.pause();
        } else {
            let ph = self.player.playhead();
            if let Some((from, stop)) = self.program_play_range(ph) {
                self.program_play = true;
                self.player.play_selection(from, stop);
            } else {
                self.player.play(None);
            }
        }
        self.sync_video_with_player(was);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let was_playing = self.player.is_playing();

        // --- mpv poll + render; chain joined clips into one stream ---
        if self.use_mpv() && self.mpv.is_running() {
            if let Some(st) = self.mpv.poll() {
                self.player.set_playhead_live(st.time);
                if st.paused || st.eof {
                    // End of one keep-run → jump to next piece (continuous program)
                    if self.program_play && self.continue_program_play(st.time) {
                        // still playing next segment
                    } else if self.program_play {
                        self.program_play = false;
                        self.player.mark_external(false);
                        self.status = "▶ end of program".into();
                        self.status_ok = Some(true);
                    } else if self.player.is_playing() {
                        self.player.mark_external(false);
                    }
                } else {
                    self.player.mark_external(true);
                    // Proactive jump slightly before stop_at for smoother join
                    if self.program_play {
                        let _ = self.continue_program_play(st.time);
                    }
                }
            }
            self.mpv.pump_texture(ctx);
        } else {
            self.player.tick();
            if was_playing && !self.player.is_playing() {
                // audio path: chain next segment
                if self.program_play {
                    let t = self.player.playhead();
                    if let Some((from, stop)) = self.next_program_after(t) {
                        self.player.play_selection(from, stop);
                        self.sync_video_with_player(false);
                    } else {
                        self.program_play = false;
                        self.sync_video_with_player(true);
                    }
                } else {
                    self.sync_video_with_player(true);
                }
            }
            if self.player.is_playing() && self.player.video_clock {
                if let Some(t) = self.video.latest_frame_time() {
                    self.player.set_playhead_live(t);
                    if !self.video.streaming {
                        self.player.pause();
                    }
                }
            }
            let ph = self.player.playhead();
            self.video.set_playhead(ph);
            self.video.pump_texture(ctx, ph);
        }

        self.poll_worker(ctx);
        self.handle_shortcuts(ctx);

        let ph = self.player.playhead();
        let has_video = self.info.as_ref().is_some_and(|i| i.has_video);
        if has_video && !self.player.is_playing() {
            let min_dt = if self.use_mpv() {
                0.08
            } else if self.video.profile.heavy {
                0.12
            } else {
                0.06
            };
            if (ph - self.last_video_still).abs() > min_dt {
                if self.use_mpv() {
                    let _ = self.mpv.seek(ph);
                    self.mpv.pump_texture(ctx);
                } else {
                    self.video.show_still(ph, false);
                }
                self.last_video_still = ph;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Прокрутка колёсиком всей формы
            egui::ScrollArea::vertical()
                .id_salt("main_scroll")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.ui_main_body(ui);
                });
        });

        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    if !self.busy {
                        self.load_path(path.clone());
                    }
                    break;
                }
            }
        });
    }
}

impl App {
    fn ui_main_body(&mut self, ui: &mut egui::Ui) {
            if let Err(ref e) = self.tools_ok {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 90, 90),
                    format!("⚠ {e}"),
                );
                ui.label("On Debian/Kali: sudo apt install ffmpeg");
                ui.separator();
            }

            // --- File ---
            ui.group(|ui| {
                ui.label(egui::RichText::new("File").strong());
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new("📂 Open…").min_size(egui::vec2(120.0, 28.0)),
                        )
                        .clicked()
                    {
                        self.open_file();
                    }
                    let path_label = self
                        .input_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "no file selected".into());
                    ui.add(
                        egui::Label::new(egui::RichText::new(path_label).monospace().weak())
                            .truncate(),
                    );
                });

                if let Some(ref info) = self.info {
                    ui.add_space(6.0);
                    egui::Grid::new("info_grid")
                        .num_columns(4)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Type:");
                            ui.monospace(info.kind_label());
                            ui.label("Duration:");
                            ui.monospace(info.duration_label());
                            ui.end_row();

                            ui.label("Size:");
                            ui.monospace(info.size_label());
                            ui.label("Container:");
                            ui.monospace(&info.format_name);
                            ui.end_row();

                            ui.label("Codecs:");
                            ui.monospace(info.codecs_label());
                            ui.label("Bitrate:");
                            ui.monospace(info.bit_rate_label());
                            ui.end_row();

                            if info.has_video {
                                ui.label("Resolution:");
                                ui.monospace(info.resolution_label());
                                ui.label("Frame rate:");
                                ui.monospace(info.fps_label());
                                ui.end_row();
                            }

                            if info.has_audio {
                                ui.label("Audio:");
                                let sr = info
                                    .sample_rate
                                    .map(|s| format!("{s} Hz"))
                                    .unwrap_or_else(|| "—".into());
                                let ch = info
                                    .channels
                                    .map(|c| format!("{c} ch"))
                                    .unwrap_or_else(|| "—".into());
                                let ac = info
                                    .audio_codec
                                    .clone()
                                    .unwrap_or_else(|| "—".into());
                                ui.monospace(format!("{ac} · {sr} · {ch}"));
                                ui.label("");
                                ui.label("");
                                ui.end_row();
                            }
                        });
                }
            });

            ui.add_space(4.0);

            // --- Fragment: превью + шкала + плеер ---
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Fragment").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Preview: Авто / Quality / Speed
                        let mut mode = self.preview_mode;
                        egui::ComboBox::from_id_salt("preview_mode")
                            .width(100.0)
                            .selected_text(mode.label())
                            .show_ui(ui, |ui| {
                                for m in PreviewMode::all() {
                                    ui.selectable_value(&mut mode, *m, m.label())
                                        .on_hover_text(m.hint());
                                }
                            });
                        if mode != self.preview_mode {
                            self.preview_mode = mode;
                            self.apply_preview_quality();
                            if self.use_mpv() {
                                let ph = self.player.playhead();
                                let _ = self.mpv.seek(ph);
                            }
                        }
                        ui.label(
                            egui::RichText::new("Preview:")
                                .weak()
                                .size(11.0),
                        );
                        if self.info.as_ref().is_some_and(|i| i.has_video) && self.mpv.available {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · {}",
                                    self.mpv.render_size_label(),
                                    machine_hint()
                                ))
                                .weak()
                                .size(10.0),
                            );
                        }
                    });
                });
                ui.add_space(2.0);

                let duration = self.duration();
                let has_timeline = duration > 0.0 && self.info.is_some();
                let has_video = self.info.as_ref().is_some_and(|i| i.has_video);
                let playhead = self.player.playhead();
                let peaks_arc = self.player.decoded().map(|d| d.peaks.clone());

                // Video: размер по умолчанию компактный, тянется за угол ↘
                let mut video_clicked = false;
                if has_video {
                    let aspect = if self.use_mpv() {
                        EMBED_H as f32 / EMBED_W as f32
                    } else {
                        video_viz::STREAM_H as f32 / video_viz::STREAM_W as f32
                    };
                    let avail_w = ui.available_width().max(160.0);
                    let min_w = 200.0_f32;
                    let max_w = avail_w;
                    // clamp сохранённой ширины
                    self.video_view_w = self.video_view_w.clamp(min_w, max_w);
                    let disp_w = self.video_view_w;
                    let disp_h = (disp_w * aspect).max(100.0);
                    let disp = egui::vec2(disp_w, disp_h);

                    // по центру панели
                    ui.horizontal(|ui| {
                        let pad = (ui.available_width() - disp_w).max(0.0) * 0.5;
                        if pad > 0.0 {
                            ui.add_space(pad);
                        }

                        let (resp, painter) =
                            ui.allocate_painter(disp, egui::Sense::click_and_drag());
                        painter.rect_filled(
                            resp.rect,
                            6.0,
                            egui::Color32::from_rgb(8, 8, 12),
                        );

                        let tex = if self.use_mpv() {
                            self.mpv.texture.as_ref()
                        } else {
                            self.video.texture.as_ref()
                        };
                        if let Some(tex) = tex {
                            painter.image(
                                tex.id(),
                                resp.rect.shrink(1.0),
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        } else {
                            painter.text(
                                resp.rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "loading video…",
                                egui::FontId::proportional(14.0),
                                egui::Color32::from_rgb(140, 140, 160),
                            );
                        }
                        if !self.player.is_playing() {
                            painter.circle_filled(
                                resp.rect.center(),
                                28.0,
                                egui::Color32::from_black_alpha(120),
                            );
                            painter.text(
                                resp.rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "▶",
                                egui::FontId::proportional(28.0),
                                egui::Color32::WHITE,
                            );
                        } else if self.use_mpv() {
                            painter.text(
                                resp.rect.left_top() + egui::vec2(10.0, 8.0),
                                egui::Align2::LEFT_TOP,
                                "● MPV",
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_rgb(80, 200, 120),
                            );
                        } else if self.video.has_realtime_stream() {
                            painter.text(
                                resp.rect.left_top() + egui::vec2(10.0, 8.0),
                                egui::Align2::LEFT_TOP,
                                format!("● LIVE · {}", self.video.hw_label()),
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_rgb(80, 200, 120),
                            );
                        }

                        // Ручка ↘ — размер
                        let grip = 16.0_f32;
                        let grip_rect = egui::Rect::from_min_size(
                            resp.rect.right_bottom() - egui::vec2(grip, grip),
                            egui::vec2(grip, grip),
                        );
                        let grip_id = ui.id().with("video_resize_grip");
                        let grip_resp =
                            ui.interact(grip_rect, grip_id, egui::Sense::click_and_drag());
                        let g = grip_rect;
                        painter.line_segment(
                            [
                                egui::pos2(g.left() + 3.0, g.bottom() - 3.0),
                                egui::pos2(g.right() - 3.0, g.bottom() - 3.0),
                            ],
                            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(180, 180, 200)),
                        );
                        painter.line_segment(
                            [
                                egui::pos2(g.right() - 3.0, g.top() + 3.0),
                                egui::pos2(g.right() - 3.0, g.bottom() - 3.0),
                            ],
                            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(180, 180, 200)),
                        );
                        painter.line_segment(
                            [
                                egui::pos2(g.left() + 6.0, g.bottom() - 3.0),
                                egui::pos2(g.right() - 3.0, g.bottom() - 6.0),
                            ],
                            egui::Stroke::new(1.2_f32, egui::Color32::from_rgb(120, 120, 140)),
                        );
                        if grip_resp.hovered() || grip_resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                        if grip_resp.dragged() {
                            let dx = grip_resp.drag_delta().x;
                            let dy = grip_resp.drag_delta().y / aspect;
                            self.video_view_w =
                                (self.video_view_w + dx.max(dy)).clamp(min_w, max_w);
                        }
                        if grip_resp.double_clicked() {
                            if (self.video_view_w - max_w).abs() < 4.0 {
                                self.video_view_w = 480.0_f32.min(max_w);
                            } else {
                                self.video_view_w = max_w;
                            }
                        }
                        let grip_clicked = grip_resp.clicked();
                        let grip_dragged = grip_resp.dragged();
                        grip_resp.on_hover_text(
                            "Drag ↘ to resize · double-click for full width / reset",
                        );

                        if resp.clicked() && !grip_clicked && !grip_dragged {
                            video_clicked = true;
                        }
                    });
                    ui.add_space(4.0);
                }
                if video_clicked {
                    self.toggle_playback();
                }

                let mut seeked_ph = None;
                if has_timeline {
                    let peaks_ref = peaks_arc.as_ref().map(|a| a.as_slice());
                    let keep_vis: Vec<(f64, f64)> = self
                        .edit_clips
                        .iter()
                        .map(|s| (s.start, s.end))
                        .collect();
                    let has_audio = self.info.as_ref().is_some_and(|i| i.has_audio)
                        || peaks_ref.is_some();
                    let clip_name = self
                        .input_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .unwrap_or("Clip");
                    let visuals = TimelineVisuals {
                        peaks: peaks_ref,
                        has_video,
                        has_audio,
                        keep_ranges: &keep_vis,
                        selected_clip: self.edit_selected,
                        clip_label: clip_name,
                    };
                    let (_, out) = show_timeline(
                        ui,
                        &mut self.timeline,
                        duration,
                        playhead,
                        visuals,
                        !self.busy,
                    );
                    // Select clip first (does not move playhead), then seek to click.
                    if let Some(i) = out.selected_clip {
                        self.select_clip(i);
                    }
                    if out.seeked {
                        seeked_ph = Some(out.playhead);
                    }
                } else {
                    ui.add_sized(
                        egui::vec2(ui.available_width(), 100.0),
                        egui::Label::new(
                            egui::RichText::new("Open a file to show video and timeline")
                                .weak()
                                .italics(),
                        ),
                    );
                }
                if let Some(t) = seeked_ph {
                    let was = self.player.is_playing();
                    if self.use_mpv() {
                        self.player.set_playhead_live(t);
                        let _ = self.mpv.seek(t);
                        if was {
                            let _ = self.mpv.set_pause(false);
                            self.mpv.stop_at = Some(self.end_sec);
                            self.player.mark_external(true);
                        } else {
                            self.video.show_still(t, true);
                            self.last_video_still = t;
                        }
                    } else {
                        self.player.set_playhead(t);
                        if was {
                            self.video.play_from(t);
                        } else {
                            self.video.show_still(t, true);
                            self.last_video_still = t;
                        }
                    }
                }

                ui.add_space(4.0);

                // transport
                ui.horizontal(|ui| {
                    let can_play = self.can_play();

                    if self.player.is_playing() {
                        if ui
                            .add_enabled(can_play, egui::Button::new("⏸ Pause (Space)"))
                            .on_hover_text("Space")
                            .clicked()
                        {
                            self.toggle_playback();
                        }
                    } else if ui
                        .add_enabled(can_play, egui::Button::new("▶ Play (Space)"))
                        .on_hover_text("Space — play/pause")
                        .clicked()
                    {
                        self.toggle_playback();
                    }

                    if ui
                        .add_enabled(
                            can_play && self.selection_duration().is_some(),
                            egui::Button::new("▶ Fragment"),
                        )
                        .on_hover_text("From selection start to end")
                        .clicked()
                    {
                        self.program_play = false;
                        if self.use_mpv() {
                            if let Some(path) = self.input_path.clone() {
                                match self.mpv.play_from(
                                    &path,
                                    self.start_sec,
                                    Some(self.end_sec),
                                ) {
                                    Ok(()) => {
                                        self.player.set_playhead_live(self.start_sec);
                                        self.player.mark_external(true);
                                    }
                                    Err(e) => {
                                        self.status = format!("mpv: {e}");
                                        self.status_ok = Some(false);
                                    }
                                }
                            }
                        } else {
                            let was = self.player.is_playing();
                            self.player
                                .play_selection(self.start_sec, self.end_sec);
                            self.sync_video_with_player(was);
                        }
                    }

                    if ui
                        .add_enabled(can_play || self.player.is_playing(), egui::Button::new("⏹ Stop"))
                        .clicked()
                    {
                        self.program_play = false;
                        if self.use_mpv() {
                            let _ = self.mpv.stop_playback();
                            let _ = self.mpv.seek(self.start_sec);
                            self.player.mark_external(false);
                            self.player.set_playhead_live(self.start_sec);
                        } else {
                            let was = self.player.is_playing();
                            self.player.stop();
                            self.player.set_playhead(self.start_sec);
                            self.sync_video_with_player(was);
                        }
                        if has_video {
                            if self.use_mpv() {
                                let _ = self.mpv.seek(self.start_sec);
                            } else {
                                self.video.show_still(self.start_sec, true);
                            }
                            self.last_video_still = self.start_sec;
                        }
                    }

                    if self.decoding {
                        ui.spinner();
                        ui.label("decode…");
                    }
                    if self.use_mpv() {
                        ui.colored_label(
                            egui::Color32::from_rgb(100, 180, 255),
                            if self.player.is_playing() {
                                "● mpv (embedded)"
                            } else {
                                "mpv ready"
                            },
                        );
                    } else if self.video.has_realtime_stream() {
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 200, 120),
                            format!("● live · {}", self.video.hw_label()),
                        );
                    }
                    if has_video {
                        ui.label(
                            egui::RichText::new(if self.use_mpv() {
                                "player: libmpv hwdec · in Fragment".into()
                            } else {
                                self.video.profile_label().to_string()
                            })
                            .weak()
                            .size(11.0),
                        );
                    }

                    ui.separator();
                    ui.monospace(format!(
                        "{} / {}",
                        format_seconds(self.player.playhead()),
                        format_seconds(duration)
                    ));
                });

                ui.add_space(4.0);
            });

            // --- Режим и выход ---
            ui.group(|ui| {
                ui.label(egui::RichText::new("Quality & export").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.mode,
                        EncodeMode::StreamCopy,
                        EncodeMode::StreamCopy.label(),
                    );
                    ui.radio_value(
                        &mut self.mode,
                        EncodeMode::Reencode,
                        EncodeMode::Reencode.label(),
                    );
                });

                if self.mode == EncodeMode::Reencode {
                    self.ui_reencode_settings(ui);
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Save as:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.output_path)
                            .desired_width(ui.available_width() - 40.0)
                            .hint_text("path"),
                    );
                    if ui.button("…").clicked() {
                        self.pick_output();
                    }
                });
            });

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let segs = self.export_segments();
                let can_trim = !self.busy
                    && self.tools_ok.is_ok()
                    && self.input_path.is_some()
                    && !segs.is_empty()
                    && segs.iter().all(|s| s.is_valid());

                let trim_btn = egui::Button::new(egui::RichText::new("✂  Cut").size(14.0).strong())
                    .min_size(egui::vec2(140.0, 28.0))
                    .fill(if can_trim {
                        egui::Color32::from_rgb(40, 120, 80)
                    } else {
                        egui::Color32::from_rgb(60, 60, 60)
                    });

                if ui.add_enabled(can_trim, trim_btn).clicked() {
                    self.do_trim();
                }

                if self.busy {
                    ui.spinner();
                    ui.label("Working…");
                }
            });

            ui.add_space(8.0); // запас снизу для скролла
    }
}
