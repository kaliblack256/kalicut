//! DaVinci-style NLE timeline with **program-time** layout:
//! after Delete, remaining clips join left (ripple) — no gap on the scale.
//! Clips store source ranges; export still uses source times.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    Playhead,
}

#[derive(Default)]
pub struct TimelineState {
    drag: Option<DragTarget>,
}

pub struct TimelineOutput {
    /// Source-time playhead (for seek in player/mpv).
    pub playhead: f64,
    pub seeked: bool,
    pub selected_clip: Option<usize>,
}

pub struct TimelineVisuals<'a> {
    pub peaks: Option<&'a [f32]>,
    pub has_video: bool,
    pub has_audio: bool,
    /// Source ranges (clips after B / Delete).
    pub keep_ranges: &'a [(f64, f64)],
    pub selected_clip: Option<usize>,
    pub clip_label: &'a str,
}

/// One clip laid out on the program timeline.
#[derive(Clone, Copy)]
struct ProgClip {
    /// Index into keep_ranges / edit_clips.
    index: usize,
    src_start: f64,
    src_end: f64,
    prog_start: f64,
    prog_end: f64,
}

const RULER_H: f32 = 22.0;
const VIDEO_H: f32 = 40.0;
const AUDIO_H: f32 = 52.0;
const LANE_GAP: f32 = 3.0;
const PAD_X: f32 = 4.0;
const PAD_Y: f32 = 4.0;

/// Build contiguous program layout from source clips (ripple join).
fn build_program_layout(source_clips: &[(f64, f64)]) -> (Vec<ProgClip>, f64) {
    let mut layout = Vec::with_capacity(source_clips.len());
    let mut prog = 0.0_f64;
    for (i, &(s, e)) in source_clips.iter().enumerate() {
        let len = (e - s).max(0.0);
        if len < 0.001 {
            continue;
        }
        layout.push(ProgClip {
            index: i,
            src_start: s,
            src_end: e,
            prog_start: prog,
            prog_end: prog + len,
        });
        prog += len;
    }
    (layout, prog.max(0.001))
}

fn source_to_program(source_t: f64, layout: &[ProgClip]) -> f64 {
    if layout.is_empty() {
        return source_t.max(0.0);
    }
    for c in layout {
        if source_t < c.src_start {
            return c.prog_start;
        }
        if source_t <= c.src_end + 1e-9 {
            return c.prog_start + (source_t - c.src_start);
        }
    }
    layout.last().map(|c| c.prog_end).unwrap_or(0.0)
}

fn program_to_source(program_t: f64, layout: &[ProgClip]) -> f64 {
    if layout.is_empty() {
        return program_t.max(0.0);
    }
    let program_t = program_t.max(0.0);
    for c in layout {
        if program_t <= c.prog_end + 1e-9 {
            let u = (program_t - c.prog_start).max(0.0);
            return (c.src_start + u).min(c.src_end);
        }
    }
    layout.last().map(|c| c.src_end).unwrap_or(0.0)
}

/// DaVinci-like multi-lane timeline (program time = joined clips).
pub fn show_timeline(
    ui: &mut Ui,
    state: &mut TimelineState,
    source_duration: f64,
    source_playhead: f64,
    visuals: TimelineVisuals<'_>,
    enabled: bool,
) -> (Response, TimelineOutput) {
    let show_video = visuals.has_video;
    let show_audio = visuals.has_audio || visuals.peaks.is_some();

    let mut body_h = 0.0_f32;
    if show_video {
        body_h += VIDEO_H;
    }
    if show_video && show_audio {
        body_h += LANE_GAP;
    }
    if show_audio {
        body_h += AUDIO_H;
    }
    if body_h < 36.0 {
        body_h = 44.0;
    }

    let height = PAD_Y * 2.0 + RULER_H + body_h;
    let width = ui.available_width().max(160.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    let source_duration = source_duration.max(0.001);

    // Source clips (fallback: full file)
    let source_clips: Vec<(f64, f64)> = if visuals.keep_ranges.is_empty() {
        vec![(0.0, source_duration)]
    } else {
        visuals.keep_ranges.to_vec()
    };
    let (layout, program_len) = build_program_layout(&source_clips);

    let content = Rect::from_min_max(
        Pos2::new(rect.left() + PAD_X, rect.top() + PAD_Y),
        Pos2::new(rect.right() - PAD_X, rect.bottom() - PAD_Y),
    );
    let ruler = Rect::from_min_max(
        Pos2::new(content.left(), content.top()),
        Pos2::new(content.right(), content.top() + RULER_H),
    );
    let lanes_top = ruler.bottom() + 2.0;
    let mut y = lanes_top;
    let video_lane = if show_video {
        let r = Rect::from_min_max(
            Pos2::new(content.left(), y),
            Pos2::new(content.right(), y + VIDEO_H),
        );
        y = r.bottom() + LANE_GAP;
        Some(r)
    } else {
        None
    };
    let audio_lane = if show_audio {
        Some(Rect::from_min_max(
            Pos2::new(content.left(), y),
            Pos2::new(content.right(), y + AUDIO_H),
        ))
    } else {
        None
    };

    let track_left = content.left();
    let track_w = content.width().max(1.0);
    // x maps **program** time 0..program_len
    let x_of = |prog_t: f64| -> f32 { track_left + (prog_t / program_len) as f32 * track_w };
    let prog_of = |x: f32| -> f64 {
        let u = ((x - track_left) / track_w).clamp(0.0, 1.0);
        u as f64 * program_len
    };

    let mut out_playhead = source_playhead.clamp(0.0, source_duration);
    let mut seeked = false;
    let mut selected_clip: Option<usize> = None;

    if enabled && (response.hovered() || state.drag.is_some()) {
        ui.ctx().set_cursor_icon(match state.drag {
            Some(DragTarget::Playhead) => egui::CursorIcon::ResizeHorizontal,
            None => egui::CursorIcon::PointingHand,
        });
    }

    if enabled {
        let pointer_x = response
            .interact_pointer_pos()
            .map(|p| p.x)
            .or_else(|| ui.input(|i| i.pointer.interact_pos().map(|p| p.x)))
            .or_else(|| ui.input(|i| i.pointer.hover_pos().map(|p| p.x)));

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                if content.contains(pos) {
                    state.drag = Some(DragTarget::Playhead);
                }
            }
        }

        if response.dragged() && matches!(state.drag, Some(DragTarget::Playhead)) {
            if let Some(x) = pointer_x {
                let prog = prog_of(x).clamp(0.0, program_len);
                out_playhead = program_to_source(prog, &layout);
                seeked = true;
            }
        }

        if response.clicked() {
            if let Some(x) = pointer_x {
                if x >= content.left() && x <= content.right() {
                    let prog = prog_of(x).clamp(0.0, program_len);
                    for c in &layout {
                        if prog >= c.prog_start && prog <= c.prog_end + 1e-9 {
                            selected_clip = Some(c.index);
                            break;
                        }
                    }
                    out_playhead = program_to_source(prog, &layout);
                    seeked = true;
                }
            }
        }

        if response.drag_stopped() {
            state.drag = None;
        }
    }

    // --- paint ---
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_rgb(28, 28, 30));

    draw_ruler(&painter, ruler, program_len, &x_of);

    if let Some(vl) = video_lane {
        painter.rect_filled(vl, 2.0, Color32::from_rgb(22, 22, 24));
        painter.rect_stroke(
            vl,
            2.0,
            Stroke::new(1.0_f32, Color32::from_rgb(40, 40, 44)),
            egui::StrokeKind::Middle,
        );
    }
    if let Some(al) = audio_lane {
        painter.rect_filled(al, 2.0, Color32::from_rgb(22, 22, 24));
        painter.rect_stroke(
            al,
            2.0,
            Stroke::new(1.0_f32, Color32::from_rgb(40, 40, 44)),
            egui::StrokeKind::Middle,
        );
    }

    // Draw clips back-to-back in program time (no gaps between remaining pieces)
    for (draw_i, c) in layout.iter().enumerate() {
        let a = x_of(c.prog_start);
        let b = x_of(c.prog_end);
        if b <= a + 1.0 {
            continue;
        }
        let selected = visuals.selected_clip == Some(c.index);

        if let Some(vl) = video_lane {
            let clip_r = Rect::from_min_max(
                Pos2::new(a, vl.top() + 2.0),
                Pos2::new(b, vl.bottom() - 2.0),
            );
            draw_video_clip(
                &painter,
                clip_r,
                selected,
                visuals.clip_label,
                draw_i,
                layout.len(),
            );
        }
        if let Some(al) = audio_lane {
            let clip_r = Rect::from_min_max(
                Pos2::new(a, al.top() + 2.0),
                Pos2::new(b, al.bottom() - 2.0),
            );
            draw_audio_clip(
                &painter,
                clip_r,
                selected,
                visuals.peaks,
                source_duration,
                c.src_start,
                c.src_end,
                visuals.clip_label,
            );
        }

        // Blade cut mark between joined clips (except first)
        if draw_i > 0 {
            let top = video_lane.map(|r| r.top()).unwrap_or(content.top());
            let bot = audio_lane
                .map(|r| r.bottom())
                .or_else(|| video_lane.map(|r| r.bottom()))
                .unwrap_or(content.bottom());
            painter.line_segment(
                [Pos2::new(a, top), Pos2::new(a, bot)],
                Stroke::new(2.0_f32, Color32::from_rgb(220, 220, 230)),
            );
        }
    }

    // Red playhead at program position of current source time
    let prog_ph = source_to_program(out_playhead, &layout).clamp(0.0, program_len);
    let px = x_of(prog_ph);
    let ph_top = content.top();
    let ph_bot = content.bottom();
    painter.line_segment(
        [Pos2::new(px, ph_top + 10.0), Pos2::new(px, ph_bot)],
        Stroke::new(1.5_f32, Color32::from_rgb(220, 40, 40)),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(px, ph_top + 2.0),
            Pos2::new(px - 6.0, ph_top + 11.0),
            Pos2::new(px + 6.0, ph_top + 11.0),
        ],
        Color32::from_rgb(220, 40, 40),
        Stroke::NONE,
    ));

    (
        response,
        TimelineOutput {
            playhead: out_playhead,
            seeked,
            selected_clip,
        },
    )
}

fn draw_ruler(painter: &egui::Painter, ruler: Rect, program_len: f64, x_of: &dyn Fn(f64) -> f32) {
    painter.rect_filled(ruler, 0.0, Color32::from_rgb(32, 32, 34));
    painter.line_segment(
        [
            Pos2::new(ruler.left(), ruler.bottom()),
            Pos2::new(ruler.right(), ruler.bottom()),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(50, 50, 54)),
    );

    let raw = program_len / 8.0;
    let step = nice_time_step(raw.max(0.1));
    let font = egui::FontId::monospace(10.0);
    let mut t = 0.0_f64;
    while t <= program_len + 0.001 {
        let x = x_of(t.min(program_len));
        let major = (t / step).round() % 2.0 == 0.0
            || t < 0.001
            || (program_len - t).abs() < step * 0.25;
        let tick_h = if major { 8.0 } else { 4.0 };
        painter.line_segment(
            [
                Pos2::new(x, ruler.bottom()),
                Pos2::new(x, ruler.bottom() - tick_h),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(110, 110, 118)),
        );
        if major {
            painter.text(
                Pos2::new(x + 3.0, ruler.top() + 3.0),
                egui::Align2::LEFT_TOP,
                format_tc(t),
                font.clone(),
                Color32::from_rgb(160, 160, 168),
            );
        }
        t += step;
        if step <= 0.0 {
            break;
        }
    }
}

fn nice_time_step(raw: f64) -> f64 {
    let candidates = [
        0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    ];
    for &c in &candidates {
        if c >= raw {
            return c;
        }
    }
    600.0
}

fn format_tc(t: f64) -> String {
    let t = t.max(0.0);
    let total = t.floor() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn draw_video_clip(
    painter: &egui::Painter,
    r: Rect,
    selected: bool,
    label: &str,
    index: usize,
    total: usize,
) {
    let fill = if selected {
        Color32::from_rgb(55, 110, 180)
    } else {
        Color32::from_rgb(40, 90, 150)
    };
    let border = if selected {
        Color32::from_rgb(140, 200, 255)
    } else {
        Color32::from_rgb(30, 70, 120)
    };
    painter.rect_filled(r, 2.0, fill);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, border), egui::StrokeKind::Middle);

    let cell_w = 28.0_f32;
    let mut x = r.left();
    let mut i = 0_u32;
    while x < r.right() - 2.0 {
        let cell = Rect::from_min_max(
            Pos2::new(x, r.top() + 2.0),
            Pos2::new((x + cell_w).min(r.right() - 1.0), r.bottom() - 12.0),
        );
        if cell.width() > 4.0 {
            let shade = if i % 2 == 0 {
                Color32::from_rgba_unmultiplied(255, 255, 255, 18)
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, 25)
            };
            painter.rect_filled(cell, 1.0, shade);
        }
        x += cell_w;
        i += 1;
    }

    let name = if total > 1 {
        format!("{}  ·{}", short_label(label), index + 1)
    } else {
        short_label(label)
    };
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(r.left(), r.bottom() - 12.0),
            Pos2::new(r.right(), r.bottom()),
        ),
        0.0,
        Color32::from_rgba_unmultiplied(0, 0, 0, 80),
    );
    painter.text(
        Pos2::new(r.left() + 4.0, r.bottom() - 11.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(10.0),
        Color32::from_rgb(220, 230, 245),
    );
}

fn draw_audio_clip(
    painter: &egui::Painter,
    r: Rect,
    selected: bool,
    peaks: Option<&[f32]>,
    full_duration: f64,
    clip_start: f64,
    clip_end: f64,
    label: &str,
) {
    let fill = if selected {
        Color32::from_rgb(45, 140, 95)
    } else {
        Color32::from_rgb(35, 110, 75)
    };
    let border = if selected {
        Color32::from_rgb(140, 240, 180)
    } else {
        Color32::from_rgb(25, 90, 60)
    };
    painter.rect_filled(r, 2.0, fill);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, border), egui::StrokeKind::Middle);

    if let Some(peaks) = peaks {
        if !peaks.is_empty() && full_duration > 0.0 {
            draw_waveform_in_clip(painter, r, peaks, full_duration, clip_start, clip_end);
        }
    }

    painter.text(
        Pos2::new(r.left() + 4.0, r.top() + 2.0),
        egui::Align2::LEFT_TOP,
        short_label(label),
        egui::FontId::proportional(10.0),
        Color32::from_rgba_unmultiplied(255, 255, 255, 180),
    );
}

fn draw_waveform_in_clip(
    painter: &egui::Painter,
    r: Rect,
    peaks: &[f32],
    full_duration: f64,
    clip_start: f64,
    clip_end: f64,
) {
    let n = peaks.len();
    let mid_y = r.center().y + 4.0;
    let half_h = (r.height() * 0.32).max(4.0);
    let clip_dur = (clip_end - clip_start).max(0.001);
    let bars = ((r.width() / 2.5).floor() as usize).clamp(8, 400);
    let bar_w = (r.width() / bars as f32 - 0.5).max(1.0);

    for i in 0..bars {
        let u0 = i as f64 / bars as f64;
        let t = clip_start + u0 * clip_dur;
        let peak_i = ((t / full_duration) * n as f64).floor() as usize;
        let peak_i = peak_i.min(n.saturating_sub(1));
        let amp = peaks[peak_i].clamp(0.0, 1.0).powf(0.85);
        let h = (amp * half_h).max(1.0);
        let x = r.left() + i as f32 / bars as f32 * r.width();
        let col = Color32::from_rgb(210, 240, 220);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x, mid_y - h), Pos2::new(x + bar_w, mid_y)),
            0.5,
            col,
        );
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x, mid_y), Pos2::new(x + bar_w, mid_y + h)),
            0.5,
            col,
        );
    }
}

fn short_label(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return "Clip".into();
    }
    let name = s.rsplit(['/', '\\']).next().unwrap_or(s);
    if name.chars().count() > 28 {
        let mut out: String = name.chars().take(25).collect();
        out.push('…');
        out
    } else {
        name.to_string()
    }
}
