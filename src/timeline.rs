//! Waveform scale: clips (blade pieces), click to select, playhead.
//! No orange In/Out handles — only B / click / Delete workflow.

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
    /// Clicked green clip index.
    pub selected_clip: Option<usize>,
}

pub struct TimelineVisuals<'a> {
    pub peaks: Option<&'a [f32]>,
    pub has_video: bool,
    /// Green clips after B.
    pub keep_ranges: &'a [(f64, f64)],
    /// Selected clip (Delete removes this).
    pub selected_clip: Option<usize>,
}

/// Waveform + clips + playhead only.
pub fn show_timeline(
    ui: &mut Ui,
    state: &mut TimelineState,
    duration: f64,
    mut playhead: f64,
    visuals: TimelineVisuals<'_>,
    enabled: bool,
) -> (Response, TimelineOutput) {
    let wave_h = if visuals.peaks.is_some() {
        72.0
    } else if visuals.has_video {
        48.0
    } else {
        44.0
    };
    let labels_h = 16.0;
    let height = 8.0 + wave_h + labels_h;
    let width = ui.available_width().max(120.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    let duration = duration.max(0.001);
    playhead = playhead.clamp(0.0, duration);

    let pad_x = 8.0_f32;
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + pad_x, rect.top() + 6.0),
        Pos2::new(rect.right() - pad_x, rect.bottom() - labels_h),
    );

    let x_of = |t: f64| -> f32 { track.left() + (t / duration) as f32 * track.width() };
    let t_of = |x: f32| -> f64 {
        let u = ((x - track.left()) / track.width()).clamp(0.0, 1.0);
        u as f64 * duration
    };

    let mut seeked = false;
    let mut selected_clip: Option<usize> = None;

    if enabled && (response.hovered() || state.drag.is_some()) {
        ui.ctx().set_cursor_icon(match state.drag {
            Some(DragTarget::Playhead) => egui::CursorIcon::AllScroll,
            None => egui::CursorIcon::PointingHand,
        });
    }

    if enabled {
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                if track.contains(pos) {
                    state.drag = Some(DragTarget::Playhead);
                    playhead = t_of(pos.x).clamp(0.0, duration);
                    seeked = true;
                }
            }
        }

        if response.dragged() {
            if let (Some(DragTarget::Playhead), Some(pos)) =
                (state.drag, response.interact_pointer_pos())
            {
                playhead = t_of(pos.x).clamp(0.0, duration);
                seeked = true;
            }
        }

        if response.drag_stopped() {
            state.drag = None;
        }

        if response.clicked() && state.drag.is_none() {
            if let Some(pos) = response.interact_pointer_pos() {
                if track.contains(pos) {
                    let p = t_of(pos.x).clamp(0.0, duration);
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
    }

    // --- paint ---
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(18, 18, 22));
    painter.rect_filled(track, 4.0, Color32::from_rgb(22, 22, 28));

    if let Some(peaks) = visuals.peaks {
        draw_waveform(&painter, track, peaks, duration, playhead);
    } else if visuals.has_video {
        painter.text(
            track.center(),
            egui::Align2::CENTER_CENTER,
            "video",
            egui::FontId::proportional(12.0),
            Color32::from_rgb(120, 120, 140),
        );
    } else {
        painter.text(
            track.center(),
            egui::Align2::CENTER_CENTER,
            "no audio",
            egui::FontId::proportional(12.0),
            Color32::from_rgb(120, 120, 140),
        );
    }

    // Green clips; dim gaps where deleted
    if !visuals.keep_ranges.is_empty() {
        let mut cursor = 0.0_f64;
        let mut sorted: Vec<(f64, f64)> = visuals.keep_ranges.to_vec();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for &(ks, ke) in &sorted {
            if ks > cursor + 0.01 {
                fill_gap(&painter, &track, x_of, cursor, ks, duration);
            }
            cursor = cursor.max(ke);
        }
        if cursor < duration - 0.01 {
            fill_gap(&painter, &track, x_of, cursor, duration, duration);
        }

        for (i, &(ks, ke)) in visuals.keep_ranges.iter().enumerate() {
            let a = x_of(ks.clamp(0.0, duration));
            let b = x_of(ke.clamp(0.0, duration));
            if b <= a {
                continue;
            }
            let r = Rect::from_min_max(
                Pos2::new(a, track.top()),
                Pos2::new(b, track.bottom()),
            );
            let sel = visuals.selected_clip == Some(i);
            painter.rect_filled(
                r,
                0.0,
                if sel {
                    Color32::from_rgba_unmultiplied(55, 200, 120, 90)
                } else {
                    Color32::from_rgba_unmultiplied(40, 160, 90, 45)
                },
            );
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(
                    if sel { 2.5_f32 } else { 1.0_f32 },
                    if sel {
                        Color32::from_rgb(140, 255, 190)
                    } else {
                        Color32::from_rgb(60, 200, 110)
                    },
                ),
                egui::StrokeKind::Middle,
            );
            // blade marks
            if ks > 0.02 {
                painter.line_segment(
                    [Pos2::new(a, track.top()), Pos2::new(a, track.bottom())],
                    Stroke::new(1.5_f32, Color32::from_rgb(240, 240, 250)),
                );
            }
            if ke < duration - 0.02 {
                painter.line_segment(
                    [Pos2::new(b, track.top()), Pos2::new(b, track.bottom())],
                    Stroke::new(1.5_f32, Color32::from_rgb(240, 240, 250)),
                );
            }
        }
    }

    // Playhead only
    let px = x_of(playhead);
    let ph_top = track.top() - 2.0;
    painter.line_segment(
        [Pos2::new(px, ph_top), Pos2::new(px, track.bottom() + 2.0)],
        Stroke::new(2.0_f32, Color32::from_rgb(255, 255, 255)),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(px, ph_top),
            Pos2::new(px - 6.0, ph_top - 9.0),
            Pos2::new(px + 6.0, ph_top - 9.0),
        ],
        Color32::from_rgb(255, 85, 0),
        Stroke::NONE,
    ));

    // Time: 0 · playhead · duration
    let label_y = track.bottom() + 2.0;
    let font = egui::FontId::monospace(10.0);
    painter.text(
        Pos2::new(track.left(), label_y),
        egui::Align2::LEFT_TOP,
        format_seconds(0.0),
        font.clone(),
        Color32::from_rgb(120, 120, 135),
    );
    painter.text(
        Pos2::new(px, label_y),
        egui::Align2::CENTER_TOP,
        format_seconds(playhead),
        font.clone(),
        Color32::from_rgb(255, 160, 80),
    );
    painter.text(
        Pos2::new(track.right(), label_y),
        egui::Align2::RIGHT_TOP,
        format_seconds(duration),
        font,
        Color32::from_rgb(120, 120, 135),
    );

    (
        response,
        TimelineOutput {
            playhead,
            seeked,
            selected_clip,
        },
    )
}

fn fill_gap(
    painter: &egui::Painter,
    track: &Rect,
    x_of: impl Fn(f64) -> f32,
    from: f64,
    to: f64,
    duration: f64,
) {
    let a = x_of(from.clamp(0.0, duration));
    let b = x_of(to.clamp(0.0, duration));
    if b > a {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(a, track.top()),
                Pos2::new(b, track.bottom()),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(40, 40, 48, 120),
        );
    }
}

/// SoundCloud-style waveform; played region brighter by playhead.
fn draw_waveform(
    painter: &egui::Painter,
    track: Rect,
    peaks: &[f32],
    duration: f64,
    playhead: f64,
) {
    if peaks.is_empty() {
        return;
    }
    let n = peaks.len();
    let mid_y = track.center().y;
    let half_h = track.height() * 0.46;
    let gap = 1.0_f32;
    let bar_w = (track.width() / n as f32 - gap).max(1.0);

    for i in 0..n {
        let x = track.left() + i as f32 / n as f32 * track.width() + gap * 0.5;
        let a0 = peaks[i];
        let a1 = if i > 0 { peaks[i - 1] } else { a0 };
        let a2 = if i + 1 < n { peaks[i + 1] } else { a0 };
        let amp = (a0 * 0.55 + a1 * 0.225 + a2 * 0.225).clamp(0.0, 1.0);
        let amp = amp.powf(0.85);
        let h = (amp * half_h).max(2.0);

        let t_mid = (i as f64 + 0.5) / n as f64 * duration;
        let played = t_mid <= playhead;
        let color = if played {
            Color32::from_rgb(255, 85, 0)
        } else {
            Color32::from_rgb(55, 58, 70)
        };

        let top = Rect::from_min_max(
            Pos2::new(x, mid_y - h),
            Pos2::new(x + bar_w, mid_y - 0.5),
        );
        let bot = Rect::from_min_max(
            Pos2::new(x, mid_y + 0.5),
            Pos2::new(x + bar_w, mid_y + h),
        );
        painter.rect_filled(top, 1.0, color);
        painter.rect_filled(bot, 1.0, color);
    }

    painter.line_segment(
        [
            Pos2::new(track.left(), mid_y),
            Pos2::new(track.right(), mid_y),
        ],
        Stroke::new(1.0_f32, Color32::from_white_alpha(15)),
    );
}
