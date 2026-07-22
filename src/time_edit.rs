//! Compact horizontal time input: min : sec . ms

use eframe::egui::{self, Color32, DragValue, RichText, Ui};

#[derive(Debug, Clone, Copy, Default)]
pub struct TimeParts {
    pub mins: u32,
    pub secs: u32,
    pub ms: u32,
}

impl TimeParts {
    pub fn from_secs(t: f64) -> Self {
        if !t.is_finite() || t <= 0.0 {
            return Self::default();
        }
        let total_ms = (t * 1000.0).round() as u64;
        let ms = (total_ms % 1000) as u32;
        let total_s = total_ms / 1000;
        let secs = (total_s % 60) as u32;
        let mins = (total_s / 60) as u32;
        Self { mins, secs, ms }
    }

    pub fn to_secs(self) -> f64 {
        self.mins as f64 * 60.0 + self.secs as f64 + self.ms as f64 / 1000.0
    }
}

/// Compact row: `label [mm]:[ss].[ms]  = x.xxx s`
pub fn time_row(ui: &mut Ui, label: &str, seconds: &mut f64, max_secs: f64, enabled: bool) -> bool {
    let mut parts = TimeParts::from_secs(*seconds);
    let max_mins = if max_secs > 0.0 {
        (max_secs / 60.0).ceil() as u32 + 1
    } else {
        9999
    };

    let mut changed = false;

    ui.horizontal(|ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(label).strong().size(12.0));

        ui.add_enabled_ui(enabled, |ui| {
            egui::Frame::NONE
                .fill(ui.visuals().extreme_bg_color)
                .stroke(ui.visuals().widgets.inactive.bg_stroke)
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            DragValue::new(&mut parts.mins)
                                .range(0..=max_mins.max(1))
                                .speed(0.3)
                                .suffix("m")
                                .custom_formatter(|n, _| format!("{:02}", n as u32))
                                .custom_parser(|s| s.parse::<f64>().ok()),
                        );
                        changed |= r.changed();
                        ui.label(RichText::new(":").strong());
                        let r = ui.add(
                            DragValue::new(&mut parts.secs)
                                .range(0..=59)
                                .speed(0.3)
                                .suffix("s")
                                .custom_formatter(|n, _| format!("{:02}", n as u32))
                                .custom_parser(|s| s.parse::<f64>().ok()),
                        );
                        changed |= r.changed();
                        ui.label(RichText::new(".").strong());
                        let r = ui.add(
                            DragValue::new(&mut parts.ms)
                                .range(0..=999)
                                .speed(1.0)
                                .suffix("ms")
                                .custom_formatter(|n, _| format!("{:03}", n as u32))
                                .custom_parser(|s| s.parse::<f64>().ok()),
                        );
                        changed |= r.changed();
                    });
                })
                .response
                .on_hover_text("Click to type · scroll wheel to adjust");
        });

        ui.label(
            RichText::new(format!("={:.2}s", parts.to_secs()))
                .monospace()
                .weak()
                .size(11.0)
                .color(Color32::from_rgb(140, 150, 165)),
        );
    });

    if changed {
        let mut t = parts.to_secs();
        if max_secs > 0.0 {
            t = t.clamp(0.0, max_secs);
        } else {
            t = t.max(0.0);
        }
        *seconds = t;
    }
    changed
}
