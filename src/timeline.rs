//! DaVinci-style NLE timeline: time ruler, video lane, audio waveform lane,
//! red playhead. Clips from blade (B); click to select; Delete drops.

use crate::ffmpeg::format_seconds;
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
    pub playhead: f64,
    pub seeked: bool,
    pub selected_clip: Option<usize>,
}

pub struct TimelineVisuals<'a> {
    pub peaks: Option<&'a [f32]>,
    pub has_video: bool,
    pub has_audio: bool,
    /// Source ranges (clips after B).
    pub keep_ranges: &'a [(f64, f64)],
    pub selected_clip: Option<usize>,
    /// Short label drawn on clips (e.g. file name).
    pub clip_label: &'a str,
}

const RULER_H: f32 = 22.0;
const VIDEO_H: f32 = 40.0;
const AUDIO_H: f32 = 52.0;
const LANE_GAP: f32 = 3.0;
const PAD_X: f32 = 4.0;
const PAD_Y: f32 = 4.0;

/// DaVinci-like multi-lane timeline.
pub fn show_timeline(
    ui: &mut Ui,
    state: &mut TimelineState,
    duration: f64,
    mut playhead: f64,
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

    let duration = duration.max(0.001);
    playhead = playhead.clamp(0.0, duration);

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
    let x_of = |t: f64| -> f32 { track_left + (t / duration) as f32 * track_w };
    let t_of = |x: f32| -> f64 {
        let u = ((x - track_left) / track_w).clamp(0.0, 1.0);
        u as f64 * duration
    };

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
                playhead = t_of(x).clamp(0.0, duration);
                seeked = true;
            }
        }

        if response.clicked() {
            if let Some(x) = pointer_x {
                if x >= content.left() && x <= content.right() {
                    let p = t_of(x).clamp(0.0, duration);
                    for (i, &(ks, ke)) in visuals.keep_ranges.iter().enumerate() {
                        if p >= ks && p <= ke {
                            selected_clip = Some(i);
                            break;
                        }
                    }
                    playhead = p;
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
    // Resolve-like dark chrome
    painter.rect_filled(rect, 4.0, Color32::from_rgb(28, 28, 30));

    draw_ruler(&painter, ruler, duration, &x_of);

    // Empty track background
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

    let ranges: &[(f64, f64)] = if visuals.keep_ranges.is_empty() {
        // full-file fallback paint
        &[]
    } else {
        visuals.keep_ranges
    };

    // Dim deleted gaps across lanes
    if !ranges.is_empty() {
        let mut cursor = 0.0_f64;
        let mut sorted = ranges.to_vec();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for &(ks, ke) in &sorted {
            if ks > cursor + 0.01 {
                paint_gap(&painter, content, video_lane, audio_lane, &x_of, cursor, ks, duration);
            }
            cursor = cursor.max(ke);
        }
        if cursor < duration - 0.01 {
            paint_gap(
                &painter,
                content,
                video_lane,
                audio_lane,
                &x_of,
                cursor,
                duration,
                duration,
            );
        }
    }

    let clips: Vec<(f64, f64)> = if ranges.is_empty() {
        vec![(0.0, duration)]
    } else {
        ranges.to_vec()
    };

    for (i, &(ks, ke)) in clips.iter().enumerate() {
        let a = x_of(ks.clamp(0.0, duration));
        let b = x_of(ke.clamp(0.0, duration));
        if b <= a + 1.0 {
            continue;
        }
        let selected = if ranges.is_empty() {
            visuals.selected_clip == Some(0)
        } else {
            visuals.selected_clip == Some(i)
        };

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
                i,
                clips.len(),
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
                duration,
                ks,
                ke,
                visuals.clip_label,
            );
        }

        // Cut edge lines (blade marks)
        if ks > 0.02 {
            let x = a;
            let top = video_lane.map(|r| r.top()).unwrap_or(content.top());
            let bot = audio_lane
                .map(|r| r.bottom())
                .or_else(|| video_lane.map(|r| r.bottom()))
                .unwrap_or(content.bottom());
            painter.line_segment(
                [Pos2::new(x, top), Pos2::new(x, bot)],
                Stroke::new(1.5_f32, Color32::from_rgb(200, 200, 210)),
            );
        }
        if ke < duration - 0.02 {
            let x = b;
            let top = video_lane.map(|r| r.top()).unwrap_or(content.top());
            let bot = audio_lane
                .map(|r| r.bottom())
                .or_else(|| video_lane.map(|r| r.bottom()))
                .unwrap_or(content.bottom());
            painter.line_segment(
                [Pos2::new(x, top), Pos2::new(x, bot)],
                Stroke::new(1.5_f32, Color32::from_rgb(200, 200, 210)),
            );
        }
    }

    // Red playhead (full height through ruler + lanes)
    let px = x_of(playhead);
    let ph_top = content.top();
    let ph_bot = content.bottom();
    painter.line_segment(
        [Pos2::new(px, ph_top + 10.0), Pos2::new(px, ph_bot)],
        Stroke::new(1.5_f32, Color32::from_rgb(220, 40, 40)),
    );
    // Red head on ruler
    let head = [
        Pos2::new(px, ph_top + 2.0),
        Pos2::new(px - 6.0, ph_top + 11.0),
        Pos2::new(px + 6.0, ph_top + 11.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        head.to_vec(),
        Color32::from_rgb(220, 40, 40),
        Stroke::NONE,
    ));

    (
        response,
        TimelineOutput {
            playhead,
            seeked,
            selected_clip,
        },
    )
}

fn paint_gap(
    painter: &egui::Painter,
    content: Rect,
    video_lane: Option<Rect>,
    audio_lane: Option<Rect>,
    x_of: &dyn Fn(f64) -> f32,
    from: f64,
    to: f64,
    duration: f64,
) {
    let a = x_of(from.clamp(0.0, duration));
    let b = x_of(to.clamp(0.0, duration));
    if b <= a {
        return;
    }
    let dim = Color32::from_rgba_unmultiplied(12, 12, 14, 180);
    if let Some(vl) = video_lane {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(a, vl.top()), Pos2::new(b, vl.bottom())),
            0.0,
            dim,
        );
    }
    if let Some(al) = audio_lane {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(a, al.top()), Pos2::new(b, al.bottom())),
            0.0,
            dim,
        );
    }
    let _ = content;
}

fn draw_ruler(painter: &egui::Painter, ruler: Rect, duration: f64, x_of: &dyn Fn(f64) -> f32) {
    painter.rect_filled(ruler, 0.0, Color32::from_rgb(32, 32, 34));
    painter.line_segment(
        [
            Pos2::new(ruler.left(), ruler.bottom()),
            Pos2::new(ruler.right(), ruler.bottom()),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(50, 50, 54)),
    );

    // Choose tick step ~ 6–10 labels across width
    let target_labels = 8.0_f64;
    let raw = duration / target_labels;
    let step = nice_time_step(raw.max(0.1));

    let font = egui::FontId::monospace(10.0);
    let mut t = 0.0_f64;
    while t <= duration + 0.001 {
        let x = x_of(t.min(duration));
        let major = (t / step).round() % 2.0 == 0.0 || t < 0.001 || (duration - t).abs() < step * 0.25;
        let tick_h = if major { 8.0 } else { 4.0 };
        painter.line_segment(
            [
                Pos2::new(x, ruler.bottom()),
                Pos2::new(x, ruler.bottom() - tick_h),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(110, 110, 118)),
        );
        if major {
            let label = format_tc(t);
            painter.text(
                Pos2::new(x + 3.0, ruler.top() + 3.0),
                egui::Align2::LEFT_TOP,
                label,
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
    // compact Resolve-like: M:SS or H:MM:SS
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
    // Blue video clip (Resolve-like)
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

    // Fake filmstrip cells
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
            // tiny "frame" cross
            let cx = cell.center();
            painter.line_segment(
                [
                    Pos2::new(cx.x - 4.0, cx.y),
                    Pos2::new(cx.x + 4.0, cx.y),
                ],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
            );
        }
        x += cell_w;
        i += 1;
    }

    // Name bar
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

/// Horizontal centered waveform inside a green audio clip (Resolve style).
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
    // how many bars fit
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
        let top = Rect::from_min_max(
            Pos2::new(x, mid_y - h),
            Pos2::new(x + bar_w, mid_y),
        );
        let bot = Rect::from_min_max(
            Pos2::new(x, mid_y),
            Pos2::new(x + bar_w, mid_y + h),
        );
        let col = Color32::from_rgb(210, 240, 220);
        painter.rect_filled(top, 0.5, col);
        painter.rect_filled(bot, 0.5, col);
    }
}

fn short_label(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return "Clip".into();
    }
    // file name only
    let name = s.rsplit(['/', '\\']).next().unwrap_or(s);
    if name.chars().count() > 28 {
        let mut out = String::new();
        for (i, c) in name.chars().enumerate() {
            if i >= 25 {
                break;
            }
            out.push(c);
        }
        out.push('…');
        out
    } else {
        name.to_string()
    }
}

// silence unused import warning if format_seconds unused after redesign
#[allow(dead_code)]
fn _keep_format_seconds() {
    let _ = format_seconds(0.0);
}
