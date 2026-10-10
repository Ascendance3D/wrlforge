// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1 Linux host: X11 and Wayland (Step 0 design).
//!
//! * X11: the viewport is a native child `GdkWindow` (a `GtkDrawingArea`
//!   made native). The render thread opens its OWN XCB connection to the
//!   same display and creates the surface on it; GDK's `Display*` never
//!   leaves the UI thread. The connection is dropped after the surface.
//! * Wayland: a desynchronized `wl_subsurface` of the toplevel on GDK's own
//!   `wl_display` (libwayland is thread-safe), with an EMPTY input region so
//!   GTK stays the only input authority. Present must be non-blocking (D8).
//!
//! Layout (D6): the WebView and the viewport are the two panes of a
//! `GtkPaned`.

use std::time::Duration;

use glib::translate::ToGlibPtr;
use gtk::prelude::*;
use tauri::{AppHandle, Manager};
use wrlforge_render::gpu::PresentPolicy;
use wrlforge_render::thread::{self, Config, SurfaceFactory};

use super::{post_fn, with, wl, Vp};

pub(crate) struct Plat {
    paned: gtk::Paned,
    bx: gtk::Box,
    area: Option<gtk::DrawingArea>,
    wl: Option<wl::Sub>,
    attach_timer: Option<glib::SourceId>,
    wayland: bool,
    /// The GTK scale the viewport was created at (Wayland buffer scale).
    scale: i32,
    native_window: &'static str,
    /// Re-create the viewport once the old one is `Destroyed`.
    readd: bool,
}

/// A raw pointer that the creator promises is valid on the render thread.
struct SendPtr(*mut std::ffi::c_void);
// SAFETY: only libwayland objects (thread-safe) are passed this way; their
// lifetime is held by the UI until `Destroyed`.
unsafe impl Send for SendPtr {}

pub(crate) fn layout(app: &AppHandle) -> Result<Plat, String> {
    let w = app.get_webview_window("main").ok_or("no main window")?;
    let vbox = w.default_vbox().map_err(|e| format!("default_vbox: {e}"))?;
    let webview = vbox
        .children()
        .into_iter()
        .find(|c| c.type_().name().contains("WebView"))
        .ok_or("no WebKitWebView in the window")?;
    let width = w.inner_size().map(|s| s.width as f64 / w.scale_factor().unwrap_or(1.0)).unwrap_or(1440.0);
    vbox.remove(&webview);
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_wide_handle(true);
    paned.pack1(&webview, true, false);
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bx.set_size_request(240, -1);
    paned.pack2(&bx, true, false);
    paned.set_position((width * 0.58) as i32);
    vbox.pack_start(&paned, true, true, 0);
    paned.show_all();
    let display = webview.display();
    let wayland = display.type_().name() == "GdkWaylandDisplay";
    Ok(Plat {
        paned,
        bx,
        area: None,
        wl: None,
        attach_timer: None,
        wayland,
        scale: 1,
        readd: false,
        native_window: if wayland { "wayland-subsurface" } else { "x11-child-window" },
    })
}

pub(crate) fn every(d: Duration, f: fn()) {
    glib::timeout_add_local(d, move || {
        f();
        glib::ControlFlow::Continue
    });
}

pub(crate) fn add_viewport() {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_can_focus(true);
    area.set_app_paintable(true);
    // SAFETY: GTK FFI on the UI thread; the widget is alive.
    unsafe { gtk::ffi::gtk_widget_set_double_buffered(area.upcast_ref::<gtk::Widget>().to_glib_none().0, 0) };
    area.add_events(
        gdk::EventMask::BUTTON_PRESS_MASK
            | gdk::EventMask::BUTTON_RELEASE_MASK
            | gdk::EventMask::POINTER_MOTION_MASK
            | gdk::EventMask::SCROLL_MASK
            | gdk::EventMask::SMOOTH_SCROLL_MASK
            | gdk::EventMask::KEY_PRESS_MASK,
    );
    area.connect_realize(on_realize);
    area.connect_unrealize(|_| {
        with(|h| {
            if !matches!(h.vp, Vp::None) {
                h.c.unrealized_before_destroyed += 1;
            }
        });
    });
    area.connect_size_allocate(|a, _| on_alloc(a));
    area.connect_button_press_event(|a, e| {
        if e.button() == 1 && e.event_type() == gdk::EventType::ButtonPress {
            a.grab_focus();
            let (x, y) = e.position();
            super::pointer_down(x, y);
        }
        glib::Propagation::Stop
    });
    area.connect_motion_notify_event(|_, e| {
        let (x, y) = e.position();
        super::pointer_move(x, y);
        glib::Propagation::Stop
    });
    area.connect_button_release_event(|_, e| {
        if e.button() == 1 {
            let (x, y) = e.position();
            super::pointer_up(x, y);
        }
        glib::Propagation::Stop
    });
    area.connect_scroll_event(|_, e| {
        let s = match e.direction() {
            gdk::ScrollDirection::Up => 1.0,
            gdk::ScrollDirection::Down => -1.0,
            gdk::ScrollDirection::Smooth => -e.delta().1,
            _ => 0.0,
        };
        super::wheel(s);
        glib::Propagation::Stop
    });
    area.connect_key_press_event(|_, e| {
        if e.keyval() == gdk::keys::constants::Escape {
            super::escape();
            return glib::Propagation::Stop;
        }
        if e.keyval() == gdk::keys::constants::Home {
            super::reset_camera();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    let bx = with(|h| {
        h.vp = Vp::Attaching;
        h.plat.area = Some(area.clone());
        h.plat.bx.clone()
    });
    if let Some(bx) = bx {
        bx.show();
        bx.pack_start(&area, true, true, 0);
        area.show(); // realizes (outside any borrow) when the window is realized
    }
}

fn physical_size(area: &gtk::DrawingArea) -> (u32, u32, f64) {
    let scale = area.scale_factor().max(1);
    ((area.allocated_width().max(1) * scale) as u32, (area.allocated_height().max(1) * scale) as u32, scale as f64)
}

fn spawn(factory: SurfaceFactory, size: (u32, u32, f64), policy: PresentPolicy) {
    with(|h| {
        let id = h.c.created as u64 + 1;
        let cfg = Config { id, size, policy, force_fifo_only: h.opts.force_fifo_only };
        match thread::spawn(cfg, factory, None, post_fn(&h.app)) {
            Ok(x) => {
                h.c.created += 1;
                if let Some(s) = super::current_scene(&h.app) {
                    x.set_scene(s, true);
                }
                h.handle = Some(x);
                h.vp = Vp::Live;
            }
            Err(e) => {
                h.vp = Vp::None;
                h.state.state = "failed".into();
                h.state.reason = Some(format!("Native viewport failed: render thread not started ({e}). Using X_ITE."));
            }
        }
    });
}

fn on_realize(area: &gtk::DrawingArea) {
    let display = area.display();
    if display.type_().name() == "GdkWaylandDisplay" {
        // The toplevel wl_surface may not exist yet: retry on a timer that
        // the viewport owns and removes on teardown.
        let a = area.clone();
        let mut tries = 0;
        let src = glib::timeout_add_local(Duration::from_millis(30), move || {
            tries += 1;
            match wayland_attach(&a) {
                Ok(true) => {
                    with(|h| h.plat.attach_timer = None);
                    glib::ControlFlow::Break
                }
                Ok(false) if tries < 200 => glib::ControlFlow::Continue,
                r => {
                    let why = match r {
                        Err(e) => e,
                        _ => "the toplevel wl_surface never appeared".into(),
                    };
                    with(|h| {
                        h.plat.attach_timer = None;
                        h.state.state = "failed".into();
                        h.state.reason = Some(format!("Native viewport failed: {why}. Using X_ITE."));
                    });
                    super::teardown();
                    glib::ControlFlow::Break
                }
            }
        });
        with(|h| h.plat.attach_timer = Some(src));
        return;
    }
    let Some(gw) = area.window() else { return };
    if display.downcast_ref::<gdkx11::X11Display>().is_none() {
        return;
    }
    gw.ensure_native();
    let Some(xw) = gw.downcast_ref::<gdkx11::X11Window>() else { return };
    let xid = xw.xid() as u32;
    let screen = display.default_screen().downcast::<gdkx11::X11Screen>().map(|s| s.screen_number()).unwrap_or(0);
    let name = display.name().to_string();
    let factory: SurfaceFactory = Box::new(move |instance: &wgpu::Instance| {
        use raw_window_handle::{RawDisplayHandle, RawWindowHandle, XcbDisplayHandle, XcbWindowHandle};
        let cname = std::ffi::CString::new(name.clone()).map_err(|e| e.to_string())?;
        // This thread's OWN connection to the X server.
        let (conn, _) = x11rb::xcb_ffi::XCBConnection::connect(Some(&cname)).map_err(|e| format!("own xcb connect: {e}"))?;
        let raw = conn.get_raw_xcb_connection();
        let win = std::num::NonZeroU32::new(xid).ok_or("xid 0")?;
        let rd = RawDisplayHandle::Xcb(XcbDisplayHandle::new(std::ptr::NonNull::new(raw), screen));
        let rw = RawWindowHandle::Xcb(XcbWindowHandle::new(win));
        // SAFETY: `conn` is returned as `keep` and dropped only after the
        // surface; the X window outlives both (the UI destroys it only after
        // `Destroyed`).
        let s = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(rd), raw_window_handle: rw }) }
            .map_err(|e| format!("create_surface(xcb): {e}"))?;
        Ok((s, Box::new(conn) as thread::Keep, "x11 own-connection".to_string()))
    });
    let size = physical_size(area);
    with(|h| h.plat.scale = size.2 as i32);
    spawn(factory, size, PresentPolicy::AllowFifo);
}

fn wl_origin(area: &gtk::DrawingArea) -> Option<(i32, i32)> {
    let top = area.toplevel()?;
    let (x, y) = area.translate_coordinates(&top, 0, 0)?;
    let a = top.allocation();
    Some((x + a.x(), y + a.y()))
}

/// `Ok(false)`: not ready yet, retry. `Ok(true)` only once the handle exists.
fn wayland_attach(area: &gtk::DrawingArea) -> Result<bool, String> {
    let top = area.toplevel().ok_or("no toplevel")?;
    let Some(tw) = top.window() else { return Ok(false) };
    // SAFETY: GDK FFI on the UI thread; the GdkWindow is realized.
    let parent = unsafe { gdk_wayland_sys::gdk_wayland_window_get_wl_surface(ToGlibPtr::<*mut gdk::ffi::GdkWindow>::to_glib_none(&tw).0 as *mut _) };
    if parent.is_null() || area.allocated_width() <= 1 {
        return Ok(false);
    }
    let display = area.display();
    // SAFETY: as above; GDK's live wl_display.
    let wl_display = unsafe { gdk_wayland_sys::gdk_wayland_display_get_wl_display(ToGlibPtr::<*mut gdk::ffi::GdkDisplay>::to_glib_none(&display).0 as *mut _) };
    let size = physical_size(area);
    let (ox, oy) = wl_origin(area).ok_or("no origin")?;
    // SAFETY: GDK's live wl_display and toplevel wl_surface (UI thread).
    let sub = unsafe { wl::Sub::new(wl_display as *mut _, parent as *mut _, ox, oy, size.2 as i32) }?;
    let (d, s) = (SendPtr(wl_display as *mut _), SendPtr(sub.surface_ptr()));
    let factory: SurfaceFactory = Box::new(move |instance: &wgpu::Instance| {
        use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
        let (d, s) = (d, s);
        let rd = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(std::ptr::NonNull::new(d.0).ok_or("null wl_display")?));
        let rw = RawWindowHandle::Wayland(WaylandWindowHandle::new(std::ptr::NonNull::new(s.0).ok_or("null wl_surface")?));
        // SAFETY: the UI keeps the wl_surface (and GDK the display) alive
        // until it receives `Destroyed`.
        let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(rd), raw_window_handle: rw }) }
            .map_err(|e| format!("create_surface(wayland): {e}"))?;
        Ok((surface, Box::new(()) as thread::Keep, "wayland subsurface".to_string()))
    });
    let stored = with(|h| {
        h.plat.wl = Some(sub);
        h.plat.scale = size.2 as i32;
    });
    if stored.is_none() {
        return Ok(false); // busy borrow: `sub` dropped, nothing was spawned
    }
    spawn(factory, size, PresentPolicy::RequireNonBlocking);
    top.queue_draw(); // the parent commit applies the subsurface position
    Ok(true)
}

fn on_alloc(area: &gtk::DrawingArea) {
    let size = physical_size(area);
    let origin = wl_origin(area);
    let recreate = with(|h| {
        if h.plat.wayland && h.plat.wl.is_some() && size.2 as i32 != h.plat.scale {
            return true; // buffer scale changed: re-create the viewport
        }
        if let Some(x) = h.handle.as_ref() {
            x.configure(size.0, size.1, size.2);
        }
        if let (Some(s), Some((ox, oy))) = (h.plat.wl.as_ref(), origin) {
            s.place(ox, oy);
        }
        false
    })
    .unwrap_or(false);
    if recreate {
        super::teardown();
        with(|h| h.plat.readd = true);
    } else if let Some(top) = area.toplevel() {
        top.queue_draw();
    }
}

pub(crate) fn cancel_attach(p: &mut Plat) {
    if let Some(t) = p.attach_timer.take() {
        t.remove();
    }
}

/// Hide a viewport whose render thread did not answer Destroy in time. It
/// stays realized: the native window lives until `Destroyed`.
pub(crate) fn park() {
    if let Some(a) = with(|h| h.plat.area.clone()).flatten() {
        a.hide();
    }
}

/// After `Destroyed` (or when nothing was ever attached): drop the
/// subsurface, then remove and destroy the widget (GDK destroys the X window).
pub(crate) fn remove_viewport() {
    let Some((bx, area, readd, failed)) = with(|h| {
        h.plat.wl = None;
        let readd = std::mem::take(&mut h.plat.readd) && h.closing.is_none() && h.state.state != "failed";
        (h.plat.bx.clone(), h.plat.area.take(), readd, h.state.state == "failed")
    }) else {
        return;
    };
    if let Some(area) = area {
        bx.remove(&area);
        // SAFETY: the widget has no other owner left; the render thread is gone.
        unsafe { area.destroy() };
    }
    if readd {
        add_viewport();
    } else if failed {
        // X_ITE takes over: give the WebView the whole width.
        bx.hide();
    }
}

pub(crate) fn report() -> serde_json::Value {
    with(|h| {
        serde_json::json!({
            "wayland": h.plat.wayland,
            "native_window": h.plat.native_window,
            "paned_position": h.plat.paned.position(),
            "scale": h.plat.scale,
        })
    })
    .unwrap_or(serde_json::Value::Null)
}
