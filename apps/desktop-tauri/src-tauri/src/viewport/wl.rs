// SPDX-License-Identifier: GPL-3.0-or-later
//! Wayland viewport: a desynchronized wl_subsurface of GTK's toplevel
//! wl_surface, created on GDK's own wl_display. Its input region is EMPTY, so
//! pointer/keyboard input passes through to the GtkDrawingArea underneath and
//! GTK remains the single input authority. (NATIVE-RENDER-1 Step 0 design.)

use std::ffi::c_void;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry, wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle};

pub struct St;
impl Dispatch<WlRegistry, GlobalListContents> for St {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(St: ignore WlCompositor);
delegate_noop!(St: ignore WlSubcompositor);
delegate_noop!(St: ignore WlSurface);
delegate_noop!(St: ignore WlSubsurface);
delegate_noop!(St: ignore WlRegion);

pub struct Sub {
    conn: Connection,
    _queue: EventQueue<St>,
    pub surface: WlSurface,
    sub: WlSubsurface,
}

impl Sub {
    /// # Safety
    /// `display` and `parent` must be GDK's live wl_display / toplevel wl_surface.
    pub unsafe fn new(display: *mut c_void, parent: *mut c_void, x: i32, y: i32, scale: i32) -> Result<Sub, String> {
        let backend = unsafe { wayland_backend::client::Backend::from_foreign_display(display as *mut _) };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<St>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = queue.handle();
        let comp: WlCompositor = globals.bind(&qh, 1..=4, ()).map_err(|e| format!("wl_compositor: {e}"))?;
        let subc: WlSubcompositor = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("wl_subcompositor: {e}"))?;
        let pid = unsafe { wayland_backend::client::ObjectId::from_ptr(WlSurface::interface(), parent as *mut _) }.map_err(|e| format!("parent id: {e}"))?;
        let parent = WlSurface::from_id(&conn, pid).map_err(|e| format!("parent proxy: {e}"))?;
        let surface = comp.create_surface(&qh, ());
        let sub = subc.get_subsurface(&surface, &parent, &qh, ());
        sub.set_desync();
        sub.set_position(x, y);
        let region = comp.create_region(&qh, ());
        surface.set_input_region(Some(&region)); // empty: input falls through to GTK
        region.destroy();
        surface.set_buffer_scale(scale);
        surface.commit();
        queue.roundtrip(&mut St).map_err(|e| format!("roundtrip: {e}"))?;
        Ok(Sub { conn, _queue: queue, surface, sub })
    }

    pub fn surface_ptr(&self) -> *mut c_void {
        self.surface.id().as_ptr() as *mut c_void
    }

    /// UI thread only. Applied by the compositor on the parent's next commit.
    /// (The buffer scale is NOT set here: it is double-buffered state of the
    /// surface that the render thread commits. A GTK scale change re-creates
    /// the viewport instead.)
    pub fn place(&self, x: i32, y: i32) {
        self.sub.set_position(x, y);
        let _ = self.conn.flush();
    }
}

impl Drop for Sub {
    fn drop(&mut self) {
        self.sub.destroy();
        self.surface.destroy();
        let _ = self.conn.flush();
    }
}
