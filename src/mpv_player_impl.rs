//! Встроенный mpv (libmpv) → RGB-кадры в панель «Фрагмент».
//! Размер рендера задаётся извне (превью Авто/Качество/Скорость).

use crate::preview_quality::PreviewSize;
use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use libmpv2::Mpv;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

/// Дефолт, пока не вызван set_render_size.
pub const EMBED_W: u32 = 960;
pub const EMBED_H: u32 = 540;

pub struct MpvStatus {
    pub time: f64,
    pub paused: bool,
    pub eof: bool,
}

pub struct MpvPlayer {
    mpv: Option<Mpv>,
    render: *mut libmpv2_sys::mpv_render_context,
    pixels: Vec<u8>,
    pub render_w: u32,
    pub render_h: u32,
    pub texture: Option<TextureHandle>,
    pub available: bool,
    loaded: Option<PathBuf>,
    pub stop_at: Option<f64>,
    pub last_time: f64,
    pub last_paused: bool,
    last_gen: u64,
    frame_gen: u64,
    init_error: Option<String>,
}

unsafe impl Send for MpvPlayer {}

impl Default for MpvPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl MpvPlayer {
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(p) => p,
            Err(e) => Self {
                mpv: None,
                render: ptr::null_mut(),
                pixels: vec![0u8; (EMBED_W * EMBED_H * 4) as usize],
                render_w: EMBED_W,
                render_h: EMBED_H,
                texture: None,
                available: false,
                loaded: None,
                stop_at: None,
                last_time: 0.0,
                last_paused: true,
                last_gen: 0,
                frame_gen: 0,
                init_error: Some(e),
            },
        }
    }

    fn try_new() -> Result<Self, String> {
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("vo", "libmpv")?;
            init.set_option("hwdec", "auto")?;
            init.set_option("terminal", "no")?;
            init.set_option("msg-level", "all=error")?;
            init.set_option("keep-open", "yes")?;
            init.set_option("osc", "no")?;
            init.set_option("osd-level", "0")?;
            init.set_option("input-default-bindings", "no")?;
            init.set_option("input-vo-keyboard", "no")?;
            init.set_option("hr-seek", "yes")?;
            init.set_option("pause", true)?;
            Ok(())
        })
        .map_err(|e| format!("libmpv init: {e}"))?;

        let mut render: *mut libmpv2_sys::mpv_render_context = ptr::null_mut();
        unsafe {
            let api = libmpv2_sys::MPV_RENDER_API_TYPE_SW.as_ptr() as *mut c_void;
            let mut params = [
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                    data: api,
                },
                libmpv2_sys::mpv_render_param {
                    type_: 0,
                    data: ptr::null_mut(),
                },
            ];
            let err = libmpv2_sys::mpv_render_context_create(
                &mut render,
                mpv.ctx.as_ptr(),
                params.as_mut_ptr(),
            );
            if err < 0 {
                return Err(format!("mpv_render_context_create: {err}"));
            }
        }

        Ok(Self {
            mpv: Some(mpv),
            render,
            pixels: vec![0u8; (EMBED_W * EMBED_H * 4) as usize],
            render_w: EMBED_W,
            render_h: EMBED_H,
            texture: None,
            available: true,
            loaded: None,
            stop_at: None,
            last_time: 0.0,
            last_paused: true,
            last_gen: 0,
            frame_gen: 0,
            init_error: None,
        })
    }

    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    pub fn is_running(&self) -> bool {
        self.mpv.is_some()
    }

    /// Сменить разрешение SW-рендера (превью quality).
    pub fn set_render_size(&mut self, size: PreviewSize) {
        let w = size.w.max(2) & !1;
        let h = size.h.max(2) & !1;
        if w == self.render_w && h == self.render_h {
            return;
        }
        self.render_w = w;
        self.render_h = h;
        self.pixels.resize((w * h * 4) as usize, 0);
        self.texture = None; // пересоздать под новый размер
        self.frame_gen = 0;
        self.last_gen = 0;
        // перерисовать текущий кадр
        self.render_frame(true);
    }

    pub fn render_size_label(&self) -> String {
        format!("{}×{}", self.render_w, self.render_h)
    }

    fn with_mpv<R>(&self, f: impl FnOnce(&Mpv) -> Result<R, String>) -> Result<R, String> {
        let mpv = self.mpv.as_ref().ok_or_else(|| {
            self.init_error
                .clone()
                .unwrap_or_else(|| "mpv unavailable".into())
        })?;
        f(mpv)
    }

    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        let p = path.to_string_lossy().to_string();
        self.with_mpv(|mpv| {
            mpv.command("loadfile", &[&p, "replace"])
                .map_err(|e| format!("loadfile: {e}"))?;
            mpv.set_property("pause", true)
                .map_err(|e| format!("pause: {e}"))?;
            Ok(())
        })?;
        self.loaded = Some(path.to_path_buf());
        self.last_paused = true;
        self.last_time = 0.0;
        self.stop_at = None;
        Ok(())
    }

    pub fn seek(&mut self, time: f64) -> Result<(), String> {
        let t = time.max(0.0);
        let ts = format!("{t:.3}");
        self.with_mpv(|mpv| {
            mpv.command("seek", &[&ts, "absolute"])
                .map_err(|e| format!("seek: {e}"))
        })?;
        self.last_time = t;
        self.render_frame(true);
        Ok(())
    }

    pub fn set_pause(&mut self, pause: bool) -> Result<(), String> {
        self.with_mpv(|mpv| {
            mpv.set_property("pause", pause)
                .map_err(|e| format!("set pause: {e}"))
        })?;
        self.last_paused = pause;
        Ok(())
    }

    pub fn play_from(&mut self, path: &Path, from: f64, stop_at: Option<f64>) -> Result<(), String> {
        if self.loaded.as_ref() != Some(&path.to_path_buf()) {
            self.load(path)?;
        }
        self.stop_at = stop_at;
        self.seek(from)?;
        self.set_pause(false)?;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.set_pause(true)
    }

    pub fn stop_playback(&mut self) -> Result<(), String> {
        self.set_pause(true)?;
        self.stop_at = None;
        Ok(())
    }

    pub fn clear_media(&mut self) {
        let _ = self.stop_playback();
        if let Some(mpv) = &self.mpv {
            let _ = mpv.command("stop", &[]);
        }
        self.loaded = None;
        self.stop_at = None;
        self.last_time = 0.0;
        self.pixels.fill(0);
        self.frame_gen = self.frame_gen.wrapping_add(1);
    }

    pub fn poll(&mut self) -> Option<MpvStatus> {
        let mpv = self.mpv.as_ref()?;

        loop {
            let ev = unsafe { libmpv2_sys::mpv_wait_event(mpv.ctx.as_ptr(), 0.0) };
            if ev.is_null() {
                break;
            }
            let id = unsafe { (*ev).event_id };
            if id == libmpv2_sys::mpv_event_id_MPV_EVENT_NONE {
                break;
            }
        }

        let time = mpv
            .get_property::<f64>("time-pos")
            .unwrap_or(self.last_time);
        let paused = mpv.get_property::<bool>("pause").unwrap_or(self.last_paused);
        let eof = mpv
            .get_property::<bool>("eof-reached")
            .unwrap_or(false);

        if !paused {
            if let Some(end) = self.stop_at {
                if time >= end - 0.04 {
                    let _ = self.set_pause(true);
                    let _ = self.seek(end);
                    self.stop_at = None;
                    self.last_time = end;
                    self.last_paused = true;
                    self.render_frame(true);
                    return Some(MpvStatus {
                        time: end,
                        paused: true,
                        eof: false,
                    });
                }
            }
        }

        self.last_time = time;
        self.last_paused = paused;
        self.render_frame(false);

        Some(MpvStatus {
            time,
            paused,
            eof,
        })
    }

    fn render_frame(&mut self, force: bool) {
        if self.render.is_null() {
            return;
        }
        let flags = unsafe { libmpv2_sys::mpv_render_context_update(self.render) };
        let need = force
            || (flags & libmpv2_sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64) != 0
            || self.frame_gen == 0;

        if !need && self.frame_gen > 0 {
            if !self.last_paused {
                return;
            }
            if !force {
                return;
            }
        }

        let w = self.render_w;
        let h = self.render_h;
        let need_bytes = (w * h * 4) as usize;
        if self.pixels.len() != need_bytes {
            self.pixels.resize(need_bytes, 0);
        }

        let mut size = [w as c_int, h as c_int];
        let format = CString::new("rgba").unwrap();
        let mut stride: usize = (w * 4) as usize;
        let ptr = self.pixels.as_mut_ptr() as *mut c_void;

        unsafe {
            let mut params = [
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_SIZE,
                    data: size.as_mut_ptr() as *mut c_void,
                },
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_FORMAT,
                    data: format.as_ptr() as *mut c_void,
                },
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_STRIDE,
                    data: &mut stride as *mut usize as *mut c_void,
                },
                libmpv2_sys::mpv_render_param {
                    type_: libmpv2_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_POINTER,
                    data: ptr,
                },
                libmpv2_sys::mpv_render_param {
                    type_: 0,
                    data: ptr::null_mut(),
                },
            ];
            let err = libmpv2_sys::mpv_render_context_render(self.render, params.as_mut_ptr());
            if err >= 0 {
                self.frame_gen = self.frame_gen.wrapping_add(1);
            }
        }
        let _ = format;
    }

    pub fn pump_texture(&mut self, ctx: &Context) -> bool {
        if self.frame_gen == 0 || self.frame_gen == self.last_gen {
            return false;
        }
        self.last_gen = self.frame_gen;
        let img = ColorImage::from_rgba_unmultiplied(
            [self.render_w as usize, self.render_h as usize],
            &self.pixels,
        );
        match &mut self.texture {
            Some(tex) => tex.set(img, TextureOptions::LINEAR),
            None => {
                self.texture =
                    Some(ctx.load_texture("mpv_embed", img, TextureOptions::LINEAR));
            }
        }
        true
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        if !self.render.is_null() {
            unsafe {
                libmpv2_sys::mpv_render_context_free(self.render);
            }
            self.render = ptr::null_mut();
        }
        self.mpv.take();
    }
}

#[allow(dead_code)]
fn _keep(c: *const c_char) {
    let _ = c;
}
