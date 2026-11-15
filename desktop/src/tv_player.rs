//! Native mpv rendering in GTK4's OpenGL framebuffer.
//!
//! GTK4/Wayland has no X11 window ID suitable for mpv --wid, so use libmpv's
//! render API instead. This shares mpv's HLS/HTTP behavior with the checker.

use std::cell::RefCell;
use std::ffi::{CString, c_char, c_void};
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use libloading::Library;
use mpv_engine::{Engine, GlRenderOptions, PlaybackEvent};

use crate::catalog::Channel;

type EglGetProcAddress = unsafe extern "C" fn(*const c_char) -> *const c_void;

#[derive(Clone)]
pub struct TvPlayer {
    pub area: gtk::GLArea,
    engine: Arc<Engine>,
    renderer_error: Rc<RefCell<Option<String>>>,
}

impl TvPlayer {
    pub fn new() -> Result<Self, String> {
        let engine = Arc::new(
            Engine::video()
                .property("hwdec", "auto-safe")
                .build()
                .map_err(|err| format!("Cannot initialize libmpv: {err}"))?,
        );
        let area = gtk::GLArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_auto_render(false);
        let renderer_error = Rc::new(RefCell::new(None));

        let (redraw_tx, redraw_rx) = mpsc::sync_channel::<()>(1);

        {
            let engine = Arc::clone(&engine);
            let error = renderer_error.clone();
            area.connect_realize(move |area| {
                area.make_current();
                if let Some(err) = area.error() {
                    *error.borrow_mut() = Some(format!("OpenGL context: {err}"));
                    return;
                }

                if let Err(err) = attach_renderer(&engine, redraw_tx.clone()) {
                    *error.borrow_mut() = Some(err);
                } else {
                    *error.borrow_mut() = None;
                }
            });
        }

        {
            let engine = Arc::clone(&engine);
            area.connect_unrealize(move |area| {
                area.make_current();
                if area.error().is_none() && engine.has_render() {
                    // libmpv requires the *same* GL context to be current
                    // when releasing the renderer.
                    engine.detach_render();
                }
            });
        }

        {
            let engine = Arc::clone(&engine);
            let error = renderer_error.clone();
            area.connect_render(move |area, _| {
                if !engine.has_render() {
                    return glib::Propagation::Stop;
                }

                let mut fbo = 0i32;
                // GTK has bound its own framebuffer at this point.
                unsafe {
                    gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fbo);
                }
                let width = area.width() * area.scale_factor();
                let height = area.height() * area.scale_factor();
                if width > 0
                    && height > 0
                    && let Err(err) = engine.render_gl(fbo, width, height, true)
                {
                    *error.borrow_mut() = Some(format!("mpv rendering: {err}"));
                }
                unsafe {
                    gl::BindFramebuffer(gl::FRAMEBUFFER, fbo as u32);
                }
                glib::Propagation::Stop
            });
        }

        {
            let area = area.clone();
            // The mpv callback runs on a decoding thread. Never call GTK
            // there: just forward a bounded notification to the main loop.
            glib::timeout_add_local(Duration::from_millis(16), move || {
                if redraw_rx.try_recv().is_ok() {
                    area.queue_render();
                }
                glib::ControlFlow::Continue
            });
        }

        Ok(Self {
            area,
            engine,
            renderer_error,
        })
    }

    pub fn load(&self, channel: &Channel) -> Result<(), String> {
        // All headers are replaced on *every* channel switch, so special
        // headers from one channel cannot leak into another channel.
        let referrer = channel
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("referer"))
            .map(|(_, value)| value.as_str())
            .unwrap_or("");
        let user_agent = channel
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("user-agent"))
            .map(|(_, value)| value.as_str())
            .unwrap_or("mpv");

        let mut other_headers: Vec<_> = channel
            .headers
            .iter()
            .filter(|(name, _)| {
                !name.eq_ignore_ascii_case("referer") && !name.eq_ignore_ascii_case("user-agent")
            })
            .map(|(name, value)| format!("{name}: {value}"))
            .collect();
        other_headers.sort();

        self.engine
            .set_property("referrer", referrer)
            .map_err(|err| format!("mpv Referer: {err}"))?;
        self.engine
            .set_property("user-agent", user_agent)
            .map_err(|err| format!("mpv User-Agent: {err}"))?;
        self.engine
            .set_property("http-header-fields", other_headers.join(","))
            .map_err(|err| format!("mpv HTTP headers: {err}"))?;
        self.engine
            .load_when_ready(&channel.url)
            .map_err(|err| format!("Cannot play {}: {err}", channel.name))
    }

    pub fn toggle_pause(&self) -> Result<bool, String> {
        let next = !self.engine.is_paused();
        self.engine
            .set_paused(next)
            .map_err(|err| format!("mpv pause: {err}"))?;
        Ok(next)
    }

    pub fn is_paused(&self) -> bool {
        self.engine.is_paused()
    }

    pub fn events(&self) -> Vec<PlaybackEvent> {
        self.engine.pump_events()
    }

    pub fn error(&self) -> Option<String> {
        self.renderer_error.borrow().clone()
    }
}

fn attach_renderer(engine: &Engine, redraw_tx: mpsc::SyncSender<()>) -> Result<(), String> {
    // libEGL.so.1 exists on Mesa and proprietary-driver EGL installations.
    // Loading it dynamically avoids requiring a separate EGL development
    // package just to build the Rust frontend.
    let egl = unsafe { Library::new("libEGL.so.1") }
        .map_err(|err| format!("Cannot load EGL for embedded mpv: {err}"))?;
    let get_proc: EglGetProcAddress = unsafe {
        *egl.get::<EglGetProcAddress>(b"eglGetProcAddress\0")
            .map_err(|err| format!("Cannot find eglGetProcAddress: {err}"))?
    };

    let resolve = move |name: &str| -> *mut c_void {
        let _keep_egl_loaded = &egl;
        let Ok(name) = CString::new(name) else {
            return std::ptr::null_mut();
        };
        unsafe { get_proc(name.as_ptr()) as *mut c_void }
    };

    gl::load_with(|name| resolve(name).cast_const());

    // SAFETY: called by GtkGLArea::realize after make_current(). Every
    // render/detach operation is likewise confined to its GTK GL callback.
    unsafe {
        engine.attach_gl_render(Box::new(resolve), GlRenderOptions::default(), move || {
            let _ = redraw_tx.try_send(());
        })
    }
    .map_err(|err| format!("Cannot attach libmpv OpenGL renderer: {err}"))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpv_accepts_network_header_options_and_clearing_them() {
        let engine = Engine::headless().build().expect("libmpv initializes");
        engine
            .set_property("referrer", "https://example.org/")
            .expect("referrer option");
        engine
            .set_property("user-agent", "Kanálik")
            .expect("user agent option");
        engine
            .set_property("http-header-fields", "X-Test: stream")
            .expect("custom HTTP field");
        engine
            .set_property("http-header-fields", "")
            .expect("clear custom HTTP fields");
        engine
            .set_property("referrer", "")
            .expect("clear referrer");
        engine
            .set_property("user-agent", "mpv")
            .expect("reset user agent");
    }
}
