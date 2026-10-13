// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1 macOS host (Step 0 design, proven on Apple M1 under the
//! Main Thread Checker).
//!
//! The window content is split into [ WKWebView | NR1View ] (D6: siblings,
//! no native view over a hole in the WebView). NR1View is a layer-backed
//! NSView. The MAIN thread creates a CAMetalLayer, sets its frame and
//! contentsScale, and adds it as a sublayer. The RENDER thread creates the
//! wgpu surface from that layer and configures, acquires and presents, each
//! inside an explicit CATransaction (the render thread has no run loop).
//!
//! wgpu 30 `acquire_texture` walks up from the layer to the first layer with
//! a delegate and sends it `window`, then `occlusionState` to the result:
//! main-thread-only AppKit calls when the delegate is an NSView. Our layer's
//! delegate is `NR1LayerDelegate`, a plain NSObject whose `window` returns
//! nil, so the walk stops there and no AppKit call happens off the main
//! thread. Occlusion is read on the main thread and sent as `visible`.
//! wgpu 30 also turns `allowsNextDrawableTimeout` off; the transaction end
//! turns it back on so a blocked `nextDrawable` returns after 1 s.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, define_class, msg_send, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSEvent, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Manager};
use wrlforge_render::gpu::PresentPolicy;
use wrlforge_render::thread::{self, Config, SurfaceFactory, Transact};

use super::{post_fn, with, Vp};

/// Share of the content width the WebView starts with.
const WEBVIEW_SHARE: f64 = 0.58;

static APP: OnceLock<AppHandle> = OnceLock::new();
static DELEGATE_WINDOW_OFF_MAIN: AtomicU64 = AtomicU64::new(0);

define_class!(
    // SAFETY: plain NSObject subclass, no ivars, no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = AllocAnyThread]
    #[name = "WRLForgeLayerDelegate"]
    struct NR1LayerDelegate;

    impl NR1LayerDelegate {
        /// wgpu's occlusion walk asks the layer delegate for its `window`.
        /// Answer nil from ANY thread, touching no AppKit object.
        #[unsafe(method(window))]
        fn window(&self) -> *mut AnyObject {
            if MainThreadMarker::new().is_none() {
                DELEGATE_WINDOW_OFF_MAIN.fetch_add(1, Ordering::Relaxed);
            }
            std::ptr::null_mut()
        }

        /// No implicit Core Animation actions for this layer.
        #[unsafe(method(actionForLayer:forKey:))]
        fn action_for_layer(&self, _layer: *mut AnyObject, _key: *mut AnyObject) -> *mut AnyObject {
            unsafe { msg_send![class!(NSNull), null] }
        }
    }
);

impl NR1LayerDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

define_class!(
    // SAFETY: NSView subclass; no ivars, no Drop.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WRLForgeViewport"]
    struct NR1View;

    impl NR1View {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { true }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: *mut AnyObject) -> bool { true }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            if let Some(w) = self.window() {
                w.makeFirstResponder(Some(self));
            }
            let (x, y) = self.local(e);
            super::pointer_down(x, y);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, e: &NSEvent) {
            let (x, y) = self.local(e);
            super::pointer_move(x, y);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, e: &NSEvent) {
            let (x, y) = self.local(e);
            super::pointer_up(x, y);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            let dy = e.scrollingDeltaY();
            super::wheel(if e.hasPreciseScrollingDeltas() { dy / 10.0 } else { dy });
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, e: &NSEvent) {
            match e.keyCode() {
                53 => super::escape(),       // Escape
                115 => super::reset_camera(), // Home
                _ => {
                    let _: () = unsafe { msg_send![super(self), keyDown: e] };
                }
            }
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            on_resize(self);
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn did_change_backing(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
            on_resize(self);
        }
    }
);

impl NR1View {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }
    fn scale(&self) -> f64 {
        self.window().map(|w| w.backingScaleFactor()).unwrap_or(1.0)
    }
    fn physical(&self) -> (u32, u32, f64) {
        let b = self.bounds();
        let s = self.scale();
        ((b.size.width * s).round().max(1.0) as u32, (b.size.height * s).round().max(1.0) as u32, s)
    }
    /// Event position in this (flipped) view: logical points, top-left origin.
    fn local(&self, e: &NSEvent) -> (f64, f64) {
        let p = self.convertPoint_fromView(e.locationInWindow(), None);
        (p.x, p.y)
    }
}

pub(crate) struct Plat {
    window: Retained<NSWindow>,
    container: Retained<NSView>,
    view: Option<Retained<NR1View>>,
    layer: Option<Retained<AnyObject>>,
    delegate: Option<Retained<NR1LayerDelegate>>,
    visible_sent: Option<bool>,
    readd: bool,
}

/// A raw pointer the creator promises is valid on the render thread.
struct SendPtr(*mut std::ffi::c_void);
// SAFETY: only the CAMetalLayer is passed this way. It is retained by the UI
// until `Destroyed` (and by wgpu for the surface's life), and the render
// thread touches it only through wgpu and inside explicit transactions.
unsafe impl Send for SendPtr {}
// SAFETY: as above; the transaction hooks only send thread-safe messages.
unsafe impl Sync for SendPtr {}

pub(crate) fn layout(app: &AppHandle) -> Result<Plat, String> {
    let _ = APP.set(app.clone());
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let w = app.get_webview_window("main").ok_or("no main window")?;
    let nsw = w.ns_window().map_err(|e| e.to_string())? as *mut NSWindow;
    // SAFETY: Tauri's live NSWindow, on the main thread; retained here.
    let window: Retained<NSWindow> = unsafe { Retained::retain(nsw) }.ok_or("null NSWindow")?;
    let content = window.contentView().ok_or("no contentView")?;
    let cname = content.class().name().to_string_lossy().into_owned();
    let (container, webview) = if cname.contains("WebView") {
        let c = NSView::initWithFrame(NSView::alloc(mtm), content.frame());
        window.setContentView(Some(&c));
        c.addSubview(&content);
        (c, content.clone())
    } else {
        let wv = content
            .subviews()
            .iter()
            .find(|v| v.class().name().to_string_lossy().contains("WebView"))
            .ok_or("no WebView subview")?;
        (content.clone(), wv.clone())
    };
    let b = container.bounds();
    webview.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new((b.size.width * WEBVIEW_SHARE).round(), b.size.height)));
    webview.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable | NSAutoresizingMaskOptions::ViewMaxXMargin);
    Ok(Plat { window, container, view: None, layer: None, delegate: None, visible_sent: None, readd: false })
}

/// Run `f` on the main thread every `d` (a timer thread posts to the run loop).
pub(crate) fn every(d: Duration, f: fn()) {
    let Some(app) = APP.get().cloned() else { return };
    let _ = std::thread::Builder::new().name("native-ticker".into()).spawn(move || loop {
        std::thread::sleep(d);
        if app.run_on_main_thread(f).is_err() {
            break;
        }
    });
    // Occlusion follows the window on the same cadence.
    if d == Duration::from_millis(50) {
        if let Some(app) = APP.get().cloned() {
            let _ = std::thread::Builder::new().name("native-occlusion".into()).spawn(move || loop {
                std::thread::sleep(Duration::from_millis(100));
                if app.run_on_main_thread(occlusion_tick).is_err() {
                    break;
                }
            });
        }
    }
}

/// Main thread: read occlusion and tell the render thread (no acquire while
/// the window is not visible).
fn occlusion_tick() {
    with(|h| {
        // SAFETY: NSWindow on the main thread.
        let occ: usize = unsafe { msg_send![&*h.plat.window, occlusionState] };
        let visible = occ & (1 << 1) != 0;
        if h.plat.visible_sent != Some(visible) {
            if let Some(x) = h.handle.as_ref() {
                x.visible(visible);
                h.plat.visible_sent = Some(visible);
            }
        }
    });
}

/// Main thread: keep the sublayer on the view's bounds and backing scale.
fn layer_geometry(view: &NR1View, layer: &AnyObject) {
    let b = view.bounds();
    let s = view.scale();
    // SAFETY: CALayer setters on the main thread inside a transaction.
    unsafe {
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        let _: () = msg_send![layer, setFrame: b];
        let _: () = msg_send![layer, setContentsScale: s];
        let _: () = msg_send![class!(CATransaction), commit];
    }
}

fn on_resize(view: &NR1View) {
    let size = view.physical();
    let layer = with(|h| h.plat.layer.clone()).flatten();
    if let Some(l) = layer {
        layer_geometry(view, &l);
    }
    with(|h| {
        if let Some(x) = h.handle.as_ref() {
            x.configure(size.0, size.1, size.2);
        }
    });
}

pub(crate) fn add_viewport() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some((container, webview_w)) = with(|h| {
        let b = h.plat.container.bounds();
        (h.plat.container.clone(), (b.size.width * WEBVIEW_SHARE).round())
    }) else {
        return;
    };
    let b = container.bounds();
    // FullSizeContentView: keep the viewport inside contentLayoutRect.
    let clr: NSRect = with(|h| unsafe { msg_send![&*h.plat.window, contentLayoutRect] }).unwrap_or(b);
    let frame = NSRect::new(NSPoint::new(webview_w, 0.0), NSSize::new((b.size.width - webview_w).max(1.0), clr.size.height.min(b.size.height)));
    let view = NR1View::new(mtm, frame);
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable | NSAutoresizingMaskOptions::ViewMinXMargin);
    view.setWantsLayer(true);
    container.addSubview(&view);
    // SAFETY: main thread; the layer is retained in `Plat` until `Destroyed`.
    let layer: Retained<AnyObject> = unsafe { msg_send![class!(CAMetalLayer), new] };
    let delegate = NR1LayerDelegate::new();
    unsafe {
        let _: () = msg_send![&*layer, setDelegate: &*delegate];
        let backing: Option<Retained<AnyObject>> = msg_send![&*view, layer];
        if let Some(bl) = backing {
            let _: () = msg_send![&*bl, addSublayer: &*layer];
        }
    }
    layer_geometry(&view, &layer);
    let size = view.physical();
    let ptr = Retained::as_ptr(&layer) as *mut std::ffi::c_void;
    let tx_layer = Arc::new(SendPtr(ptr));
    let transact: Transact = Arc::new(move |begin: bool| {
        // SAFETY: CATransaction class methods are thread-safe; the layer is
        // retained by the UI for the whole render-thread lifetime.
        unsafe {
            if begin {
                let _: () = msg_send![class!(CATransaction), begin];
                let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            } else {
                let layer = &*(tx_layer.0 as *const AnyObject);
                let _: () = msg_send![layer, setAllowsNextDrawableTimeout: true];
                let _: () = msg_send![class!(CATransaction), commit];
            }
        }
    });
    let lp = SendPtr(ptr);
    let factory: SurfaceFactory = Box::new(move |instance: &wgpu::Instance| {
        let lp = lp;
        // SAFETY: the layer is retained by the UI until `Destroyed`; wgpu
        // retains it again for the surface's lifetime.
        let s = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(lp.0)) }
            .map_err(|e| format!("create_surface(layer): {e}"))?;
        Ok((s, Box::new(()) as thread::Keep, "macOS CAMetalLayer (non-view delegate)".to_string()))
    });
    with(|h| {
        h.plat.view = Some(view);
        h.plat.layer = Some(layer);
        h.plat.delegate = Some(delegate);
        h.plat.visible_sent = None;
        h.vp = Vp::Attaching;
        let id = h.c.created as u64 + 1;
        let cfg = Config { id, size, policy: PresentPolicy::AllowFifo, force_fifo_only: h.opts.force_fifo_only };
        match thread::spawn(cfg, factory, Some(transact), post_fn(&h.app)) {
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

pub(crate) fn cancel_attach(_: &mut Plat) {}

/// TEST-ONLY (`--smoke-native`): float the window so it presents without
/// activating the app or taking focus (Step 0 practice).
pub(crate) fn test_present_unfocused() {
    with(|h| {
        // SAFETY: NSWindow on the main thread.
        let _: () = unsafe { msg_send![&*h.plat.window, setLevel: 3isize] };
        h.plat.window.orderFrontRegardless();
    });
}

/// Hide a viewport whose render thread did not answer Destroy in time. The
/// view and layer stay alive until `Destroyed`.
pub(crate) fn park() {
    if let Some(v) = with(|h| h.plat.view.clone()).flatten() {
        v.setHidden(true);
    }
}

/// After `Destroyed`: detach the layer, clear and release its delegate, then
/// remove the view.
pub(crate) fn remove_viewport() {
    let Some((view, layer, delegate, readd, failed, webview)) = with(|h| {
        let readd = std::mem::take(&mut h.plat.readd) && h.closing.is_none() && h.state.state != "failed";
        let webview = h.plat.container.subviews().iter().find(|v| v.class().name().to_string_lossy().contains("WebView")).map(|v| v.clone());
        (h.plat.view.take(), h.plat.layer.take(), h.plat.delegate.take(), readd, h.state.state == "failed", webview)
    }) else {
        return;
    };
    if let Some(layer) = layer {
        // SAFETY: main thread; the render thread and its surface are gone.
        unsafe {
            let _: () = msg_send![&*layer, removeFromSuperlayer];
            let _: () = msg_send![&*layer, setDelegate: std::ptr::null::<AnyObject>()];
        }
    }
    drop(delegate); // CALayer.delegate is weak: release only after clearing it
    if let Some(v) = view {
        v.removeFromSuperview();
    }
    if readd {
        add_viewport();
    } else if failed {
        // X_ITE takes over: give the WebView the whole width.
        if let (Some(wv), Some(b)) = (webview, with(|h| h.plat.container.bounds())) {
            wv.setFrame(b);
        }
    }
}

pub(crate) fn report() -> serde_json::Value {
    with(|h| {
        serde_json::json!({
            "wayland": false,
            "native_window": "cametallayer-sublayer",
            "scale": h.plat.window.backingScaleFactor(),
            "delegate_window_calls_off_main": DELEGATE_WINDOW_OFF_MAIN.load(Ordering::Relaxed),
            "macos": std::process::Command::new("sw_vers").arg("-productVersion").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
        })
    })
    .unwrap_or(serde_json::Value::Null)
}
