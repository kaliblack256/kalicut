//! Визуальная шкала: SoundCloud-style waveform, filmstrip, ручки, playhead.

use crate::ffmpeg::format_seconds;
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    Start,
    End,
    Range,
    Playhead,
}

#[derive(Default)]
pub struct TimelineState {
    drag: Option<DragTarget>,
    range_grab_offset: f64,
}

pub struct TimelineOutput {
    pub start: f64,
    pub end: f64,
    pub playhead: f64,
    pub changed_range: bool,
    pub seeked: bool,
    /// Clicked green clip index (Blade pieces).
    pub selected_clip: Option<usize>,
}

pub struct TimelineVisuals<'a> {
    pub peaks: Option<&'a [f32]>,
    pub has_video: bool,
    /// Clips on the scale after B splits (green).
    pub keep_ranges: &'a [(f64, f64)],
    /// Selected clip (bright outline) — Delete removes this.
    pub selected_clip: Option<usize>,
}

/// Рисует шкалу (опционально filmstrip + waveform).
#[allow(clippy::too_many_arguments)]
pub fn show_timeline(
    ui: &mut Ui,
    state: &mut TimelineState,
    duration: f64,
    mut start: f64,
    mut end: f64,
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
    let labels_h = 18.0;
    let height = 8.0 + wave_h + labels_h;
    let width = ui.available_width().max(120.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    let duration = duration.max(0.001);
    start = start.clamp(0.0, duration);
    end = end.clamp(0.0, duration);
    if end < start {
        end = start;
    }
    playhead = playhead.clamp(0.0, duration);

    let pad_x = 8.0_f32;
    let content = Rect::from_min_max(
        Pos2::new(rect.left() + pad_x, rect.top() + 6.0),
        Pos2::new(rect.right() - pad_x, rect.bottom() - labels_h),
    );

    let track = content;

    let handle_half = 7.0_f32;
    let x_of = |t: f64| -> f32 { track.left() + (t / duration) as f32 * track.width() };
    let t_of = |x: f32| -> f64 {
        let u = ((x - track.left()) / track.width()).clamp(0.0, 1.0);
        u as f64 * duration
    };

    let mut changed_range = false;
    let mut seeked = false;
    let mut selected_clip: Option<usize> = None;

    // Hit-test по всей области (strip + wave)
    let interact_rect = content;

    if enabled && (response.hovered() || state.drag.is_some()) {
        ui.ctx().set_cursor_icon(match state.drag {
            Some(DragTarget::Start) | Some(DragTarget::End) => egui::CursorIcon::ResizeHorizontal,
            Some(DragTarget::Range) => egui::CursorIcon::Grabbing,
            Some(DragTarget::Playhead) => egui::CursorIcon::AllScroll,
            None => {
                if let Some(pos) = response.hover_pos() {
                    let sx = x_of(start);
                    let ex = x_of(end);
                    if (pos.x - sx).abs() <= handle_half + 4.0
                        || (pos.x - ex).abs() <= handle_half + 4.0
                    {
                        egui::CursorIcon::ResizeHorizontal
                    } else if pos.x >= sx && pos.x <= ex {
                        egui::CursorIcon::Grab
                    } else {
                        egui::CursorIcon::PointingHand
                    }
                } else {
                    egui::CursorIcon::Default
                }
            }
        });
    }

    if enabled {
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                if interact_rect.contains(pos) {
                    let sx = x_of(start);
                    let ex = x_of(end);
                    let px = x_of(playhead);
                    state.drag = if (pos.x - sx).abs() <= handle_half + 6.0 {
                        Some(DragTarget::Start)
                    } else if (pos.x - ex).abs() <= handle_half + 6.0 {
                        Some(DragTarget::End)
                    } else if (pos.x - px).abs() <= 6.0 {
                        Some(DragTarget::Playhead)
                    } else if pos.x >= sx && pos.x <= ex {
                        state.range_grab_offset = t_of(pos.x) - start;
                        Some(DragTarget::Range)
                    } else {
                        Some(DragTarget::Playhead)
                    };
                }
            }
        }

        // порог примагничивания: ~10–12 px в секундах
        let snap_thr = (12.0 / track.width().max(1.0) as f64) * duration;
        let snap_thr = snap_thr.clamp(0.02, duration * 0.05);

        if response.dragged() {
            if let (Some(target), Some(pos)) = (state.drag, response.interact_pointer_pos()) {
                let t = t_of(pos.x);
                match target {
                    DragTarget::Start => {
                        let max_start = (end - 0.05).max(0.0);
                        let mut s = t.clamp(0.0, max_start);
                        // к playhead и к нулю
                        s = snap_to(s, &[0.0, playhead], snap_thr);
                        s = s.clamp(0.0, max_start);
                        start = s;
                        changed_range = true;
                    }
                    DragTarget::End => {
                        let min_end = (start + 0.05).min(duration);
                        let mut e = t.clamp(min_end, duration);
                        e = snap_to(e, &[duration, playhead], snap_thr);
                        e = e.clamp(min_end, duration);
                        end = e;
                        changed_range = true;
                    }
                    DragTarget::Range => {
                        let sel = end - start;
                        let mut new_start = t - state.range_grab_offset;
                        new_start = new_start.clamp(0.0, (duration - sel).max(0.0));
                        // примагнитить край диапазона к playhead
                        let new_end = new_start + sel;
                        if (new_start - playhead).abs() < snap_thr {
                            new_start = playhead.clamp(0.0, (duration - sel).max(0.0));
                        } else if (new_end - playhead).abs() < snap_thr {
                            new_start = (playhead - sel).clamp(0.0, (duration - sel).max(0.0));
                        } else if new_start < snap_thr {
                            new_start = 0.0;
                        } else if (new_end - duration).abs() < snap_thr {
                            new_start = (duration - sel).max(0.0);
                        }
                        start = new_start;
                        end = (new_start + sel).min(duration);
                        changed_range = true;
                    }
                    DragTarget::Playhead => {
                        // к границам фрагмента и краям файла
                        let mut p = t.clamp(0.0, duration);
                        p = snap_to(p, &[start, end, 0.0, duration], snap_thr);
                        playhead = p.clamp(0.0, duration);
                        seeked = true;
                    }
                }
            }
        }

        if response.drag_stopped() {
            state.drag = None;
        }

        if response.clicked() && state.drag.is_none() {
            if let Some(pos) = response.interact_pointer_pos() {
                if interact_rect.contains(pos) {
                    let mut p = t_of(pos.x).clamp(0.0, duration);
                    // Click a green clip to select it (then Delete)
                    for (i, &(ks, ke)) in visuals.keep_ranges.iter().enumerate() {
                        if p >= ks && p <= ke {
                            selected_clip = Some(i);
                            break;
                        }
                    }
                    p = snap_to(p, &[start, end, 0.0, duration], snap_thr);
                    playhead = p;
                    seeked = true;
                }
            }
        }
    }

    // --- paint ---
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(18, 18, 22));

    // Waveform track (SoundCloud-like)
    painter.rect_filled(track, 4.0, Color32::from_rgb(22, 22, 28));

    if let Some(peaks) = visuals.peaks {
        draw_soundcloud_wave(&painter, track, peaks, duration, start, end, playhead);
    } else if visuals.has_video {
        painter.text(
            track.center(),
            egui::Align2::CENTER_CENTER,
            "B = blade · click clip · Delete",
            egui::FontId::proportional(11.0),
            Color32::from_rgb(120, 120, 140),
        );
    } else {
        painter.text(
            track.center(),
            egui::Align2::CENTER_CENTER,
            "no audio track",
            egui::FontId::proportional(13.0),
            Color32::from_rgb(120, 120, 140),
        );
    }

    // Clips (green). Selected = bright. Gaps = removed (dim red).
    if !visuals.keep_ranges.is_empty() {
        let mut cursor = 0.0_f64;
        let mut sorted: Vec<(f64, f64)> = visuals.keep_ranges.to_vec();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for &(ks, ke) in &sorted {
            if ks > cursor + 0.01 {
                let a = x_of(cursor.clamp(0.0, duration));
                let b = x_of(ks.clamp(0.0, duration));
                if b > a {
                    painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(a, track.top()),
                            Pos2::new(b, track.bottom()),
                        ),
                        0.0,
                        Color32::from_rgba_unmultiplied(180, 50, 50, 38),
                    );
                }
            }
            cursor = cursor.max(ke);
        }
        if cursor < duration - 0.01 {
            let a = x_of(cursor.clamp(0.0, duration));
            let b = x_of(duration);
            if b > a {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(a, track.top()),
                        Pos2::new(b, track.bottom()),
                    ),
                    0.0,
                    Color32::from_rgba_unmultiplied(180, 50, 50, 38),
                );
            }
        }
        for (i, &(ks, ke)) in visuals.keep_ranges.iter().enumerate() {
            let a = x_of(ks.clamp(0.0, duration));
            let b = x_of(ke.clamp(0.0, duration));
            if b > a {
                let r = Rect::from_min_max(
                    Pos2::new(a, track.top()),
                    Pos2::new(b, track.bottom()),
                );
                let sel = visuals.selected_clip == Some(i);
                painter.rect_filled(
                    r,
                    0.0,
                    if sel {
                        Color32::from_rgba_unmultiplied(55, 200, 120, 95)
                    } else {
                        Color32::from_rgba_unmultiplied(40, 160, 90, 50)
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
                // blade lines at inner edges
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
    }

    let sx = x_of(start);
    let ex = x_of(end);

    let sel_rect = Rect::from_min_max(Pos2::new(sx, track.top()), Pos2::new(ex, track.bottom()));
    painter.rect_stroke(
        sel_rect,
        0.0,
        Stroke::new(1.5_f32, Color32::from_rgb(255, 120, 40)),
        egui::StrokeKind::Middle,
    );

    let handle_span = Rect::from_min_max(
        Pos2::new(0.0, track.top()),
        Pos2::new(0.0, track.bottom()),
    );
    draw_handle(&painter, sx, handle_span, Color32::from_rgb(100, 210, 255));
    draw_handle(&painter, ex, handle_span, Color32::from_rgb(255, 200, 80));

    let px = x_of(playhead);
    let ph_top = track.top() - 2.0;
    painter.line_segment(
        [Pos2::new(px, ph_top), Pos2::new(px, track.bottom() + 2.0)],
        Stroke::new(2.0_f32, Color32::from_rgb(255, 255, 255)),
    );
    let tip = [
        Pos2::new(px, ph_top),
        Pos2::new(px - 6.0, ph_top - 9.0),
        Pos2::new(px + 6.0, ph_top - 9.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        tip.to_vec(),
        Color32::from_rgb(255, 85, 0),
        Stroke::NONE,
    ));

    // Подписи: не накладываем друг на друга
    let label_y = track.bottom() + 3.0;
    let font = egui::FontId::monospace(10.0);
    let col_edge = Color32::from_rgb(140, 140, 155);
    let col_start = Color32::from_rgb(100, 210, 255);
    let col_end = Color32::from_rgb(255, 200, 80);
    // примерная ширина метки «00:00.000» ~ 58 px
    let label_w = 58.0_f32;
    let gap = 6.0_f32;

    let start_near_left = (sx - track.left()) < label_w * 0.6;
    let end_near_right = (track.right() - ex) < label_w * 0.6;
    let start_end_close = (ex - sx) < label_w + gap;

    // Левый край файла — только если начало фрагмента не на нём
    if !start_near_left {
        painter.text(
            Pos2::new(track.left(), label_y),
            egui::Align2::LEFT_TOP,
            format_seconds(0.0),
            font.clone(),
            col_edge,
        );
    }

    // Правый край файла — только если конец фрагмента не на нём
    if !end_near_right {
        painter.text(
            Pos2::new(track.right(), label_y),
            egui::Align2::RIGHT_TOP,
            format_seconds(duration),
            font.clone(),
            col_edge,
        );
    }

    // Метка начала (синяя)
    {
        let mut x = sx;
        let mut align = egui::Align2::CENTER_TOP;
        if start_near_left {
            x = track.left();
            align = egui::Align2::LEFT_TOP;
        } else if start_end_close {
            // сдвинуть влево от центра пары
            x = (sx - label_w * 0.35).max(track.left());
            align = egui::Align2::LEFT_TOP;
        } else {
            // не залезать на правую/левую статичные метки
            x = x.clamp(track.left() + label_w * 0.5, track.right() - label_w * 0.5);
        }
        painter.text(
            Pos2::new(x, label_y),
            align,
            format_seconds(start),
            font.clone(),
            col_start,
        );
    }

    // Метка конца (оранжевая)
    {
        let mut x = ex;
        let mut align = egui::Align2::CENTER_TOP;
        if end_near_right {
            x = track.right();
            align = egui::Align2::RIGHT_TOP;
        } else if start_end_close {
            x = (ex + label_w * 0.35).min(track.right());
            align = egui::Align2::RIGHT_TOP;
        } else {
            x = x.clamp(track.left() + label_w * 0.5, track.right() - label_w * 0.5);
        }
        // если всё ещё почти на start — чуть правее
        if (x - sx).abs() < label_w * 0.5 && !end_near_right {
            x = (sx + label_w + gap).min(track.right());
            align = egui::Align2::LEFT_TOP;
        }
        painter.text(
            Pos2::new(x, label_y),
            align,
            format_seconds(end),
            font,
            col_end,
        );
    }

    (
        response,
        TimelineOutput {
            start,
            end,
            playhead,
            changed_range,
            seeked,
            selected_clip,
        },
    )
}

/// SoundCloud-like mirrored bars: played = #ff5500, rest softer.
fn draw_soundcloud_wave(
    painter: &egui::Painter,
    track: Rect,
    peaks: &[f32],
    duration: f64,
    start: f64,
    end: f64,
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
        // лёгкое сглаживание с соседями
        let a0 = peaks[i];
        let a1 = if i > 0 { peaks[i - 1] } else { a0 };
        let a2 = if i + 1 < n { peaks[i + 1] } else { a0 };
        let amp = (a0 * 0.55 + a1 * 0.225 + a2 * 0.225).clamp(0.0, 1.0);
        // кривая «живости» как у SC
        let amp = amp.powf(0.85);
        let h = (amp * half_h).max(2.0);

        let t_mid = (i as f64 + 0.5) / n as f64 * duration;
        let in_sel = t_mid >= start && t_mid <= end;
        let played = t_mid <= playhead;

        let color = match (in_sel, played) {
            // выделение + проиграно — фирменный SoundCloud orange
            (true, true) => Color32::from_rgb(255, 85, 0),
            // выделение, ещё не дошли
            (true, false) => Color32::from_rgb(255, 170, 120),
            // вне выделения, уже прошли
            (false, true) => Color32::from_rgb(90, 70, 60),
            // вне выделения
            (false, false) => Color32::from_rgb(55, 58, 70),
        };

        // верхняя и нижняя «вибрации» (mirror)
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

    // тонкая линия центра
    painter.line_segment(
        [
            Pos2::new(track.left(), mid_y),
            Pos2::new(track.right(), mid_y),
        ],
        Stroke::new(1.0_f32, Color32::from_white_alpha(15)),
    );
}

/// Примагнитить `t` к ближайшей из `targets`, если расстояние < threshold.
fn snap_to(t: f64, targets: &[f64], threshold: f64) -> f64 {
    let mut best = t;
    let mut best_d = threshold;
    for &target in targets {
        if !target.is_finite() {
            continue;
        }
        let d = (t - target).abs();
        if d < best_d {
            best_d = d;
            best = target;
        }
    }
    best
}

fn draw_handle(painter: &egui::Painter, x: f32, span: Rect, color: Color32) {
    let w = 5.0;
    let rect = Rect::from_min_max(
        Pos2::new(x - w * 0.5, span.top()),
        Pos2::new(x + w * 0.5, span.bottom()),
    );
    painter.rect_filled(rect, 2.0, color);
    let cy = (span.top() + span.bottom()) * 0.5;
    for dy in [-10.0_f32, 0.0, 10.0] {
        painter.line_segment(
            [
                Pos2::new(x - 1.5, cy + dy),
                Pos2::new(x + 1.5, cy + dy),
            ],
            Stroke::new(1.0_f32, Color32::from_black_alpha(100)),
        );
    }
}
