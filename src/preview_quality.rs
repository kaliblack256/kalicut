//! Preview quality: Auto (file + PC) or manual override.

/// Preview mode (does not affect trim / stream copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewMode {
    #[default]
    Auto,
    /// Higher render resolution, more load.
    Quality,
    /// Lower resolution, smoother on weak PCs.
    Speed,
}

impl PreviewMode {
    pub fn all() -> &'static [PreviewMode] {
        &[PreviewMode::Auto, PreviewMode::Quality, PreviewMode::Speed]
    }

    pub fn label(self) -> &'static str {
        match self {
            PreviewMode::Auto => "Auto",
            PreviewMode::Quality => "Quality",
            PreviewMode::Speed => "Speed",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            PreviewMode::Auto => "adapts to the file and machine power",
            PreviewMode::Quality => "sharper preview (heavier load)",
            PreviewMode::Speed => "lighter for CPU/GPU, softer preview",
        }
    }
}

/// Target preview frame size (even, ≤ source when possible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewSize {
    pub w: u32,
    pub h: u32,
}

impl PreviewSize {
    pub fn label(self) -> String {
        format!("{}×{}", self.w, self.h)
    }
}

/// Resolve preview render size.
pub fn resolve_preview_size(
    mode: PreviewMode,
    src_w: Option<u32>,
    src_h: Option<u32>,
    codec: Option<&str>,
    bit_rate: Option<u64>,
) -> PreviewSize {
    let sw = src_w.unwrap_or(1920).max(1);
    let sh = src_h.unwrap_or(1080).max(1);
    let pixels = sw.saturating_mul(sh);
    let codec = codec.unwrap_or("").to_ascii_lowercase();
    let heavy_codec = codec.contains("hevc")
        || codec.contains("h265")
        || codec.contains("av1")
        || codec == "hev1"
        || codec == "hvc1";
    let uhd = pixels >= 3_000_000 || sw >= 3000;
    let high_br = bit_rate.is_some_and(|b| b >= 25_000_000);
    let heavy = uhd || (heavy_codec && pixels >= 1_500_000) || high_br;

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // Max long edge of the preview frame
    let max_long = match mode {
        PreviewMode::Speed => {
            if heavy {
                640
            } else {
                854
            }
        }
        PreviewMode::Quality => {
            if uhd {
                1920
            } else {
                1920.min(sw.max(sh))
            }
        }
        PreviewMode::Auto => {
            if heavy && cores <= 4 {
                854 // ~480p
            } else if heavy && cores <= 8 {
                1280 // 720p
            } else if heavy {
                1600
            } else if sh >= 1080 && cores >= 6 {
                1920
            } else if sh >= 720 {
                1280
            } else {
                960
            }
        }
    };

    fit_even(sw, sh, max_long)
}

fn fit_even(src_w: u32, src_h: u32, max_long: u32) -> PreviewSize {
    let long = src_w.max(src_h);
    if long <= max_long {
        return PreviewSize {
            w: even(src_w),
            h: even(src_h),
        };
    }
    let scale = max_long as f64 / long as f64;
    let w = even(((src_w as f64) * scale).round() as u32);
    let h = even(((src_h as f64) * scale).round() as u32);
    PreviewSize {
        w: w.max(2),
        h: h.max(2),
    }
}

fn even(v: u32) -> u32 {
    v & !1
}

/// Machine power hint for UI.
pub fn machine_hint() -> String {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    format!("{cores} CPU thr.")
}
