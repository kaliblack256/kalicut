//! Embedded mpv player (optional feature `embedded-mpv`).
//! When disabled, a stub is used and the app falls back to ffmpeg preview.

#[cfg(feature = "embedded-mpv")]
#[path = "mpv_player_impl.rs"]
mod imp;

#[cfg(feature = "embedded-mpv")]
pub use imp::*;

#[cfg(not(feature = "embedded-mpv"))]
mod stub {
    use crate::preview_quality::PreviewSize;
    use eframe::egui::{Context, TextureHandle};
    use std::path::Path;

    pub const EMBED_W: u32 = 960;
    pub const EMBED_H: u32 = 540;

    pub struct MpvStatus {
        pub time: f64,
        pub paused: bool,
        pub eof: bool,
    }

    pub struct MpvPlayer {
        pub render_w: u32,
        pub render_h: u32,
        pub texture: Option<TextureHandle>,
        pub available: bool,
        pub stop_at: Option<f64>,
        pub last_time: f64,
        pub last_paused: bool,
        init_error: Option<String>,
    }

    impl Default for MpvPlayer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MpvPlayer {
        pub fn new() -> Self {
            Self {
                render_w: EMBED_W,
                render_h: EMBED_H,
                texture: None,
                available: false,
                stop_at: None,
                last_time: 0.0,
                last_paused: true,
                init_error: Some(
                    "built without embedded-mpv (ffmpeg preview)".into(),
                ),
            }
        }

        pub fn init_error(&self) -> Option<&str> {
            self.init_error.as_deref()
        }

        pub fn is_running(&self) -> bool {
            false
        }

        pub fn set_render_size(&mut self, size: PreviewSize) {
            self.render_w = size.w.max(2) & !1;
            self.render_h = size.h.max(2) & !1;
        }

        pub fn render_size_label(&self) -> String {
            format!("{}×{}", self.render_w, self.render_h)
        }

        pub fn load(&mut self, _path: &Path) -> Result<(), String> {
            Err(self.init_error.clone().unwrap_or_else(|| "mpv off".into()))
        }

        pub fn seek(&mut self, time: f64) -> Result<(), String> {
            self.last_time = time.max(0.0);
            Ok(())
        }

        pub fn set_pause(&mut self, pause: bool) -> Result<(), String> {
            self.last_paused = pause;
            Ok(())
        }

        pub fn play_from(
            &mut self,
            _path: &Path,
            from: f64,
            stop_at: Option<f64>,
        ) -> Result<(), String> {
            self.stop_at = stop_at;
            self.last_time = from;
            self.last_paused = false;
            Err(self.init_error.clone().unwrap_or_else(|| "mpv off".into()))
        }

        pub fn pause(&mut self) -> Result<(), String> {
            self.set_pause(true)
        }

        pub fn stop_playback(&mut self) -> Result<(), String> {
            self.last_paused = true;
            self.stop_at = None;
            Ok(())
        }

        pub fn clear_media(&mut self) {
            let _ = self.stop_playback();
            self.last_time = 0.0;
        }

        pub fn poll(&mut self) -> Option<MpvStatus> {
            None
        }

        pub fn pump_texture(&mut self, _ctx: &Context) -> bool {
            false
        }
    }
}

#[cfg(not(feature = "embedded-mpv"))]
pub use stub::*;
