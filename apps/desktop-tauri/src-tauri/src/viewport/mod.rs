// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1: the native viewport host (UI thread side).
//!
//! HIDDEN and OFF by default (owner decision D7): it starts only when
//! `settings.json` has `"viewport": {"renderer": "native-experimental"}` or the app was
//! launched with `--native-viewport`. X_ITE stays the default and is the
//! fallback for every failure (D8 FIFO-only Wayland, D9 device loss, any
//! start error). It is SELECTION ONLY: no manipulation, no edit path.
//!
//! Layout (D6): a native split. The WebView and the native viewport are
//! siblings in a native split container; no native view is placed over a
//! hole in the WebView.
//!
//! Threads: GTK/AppKit objects live on the UI thread (`HOST`, thread-local).
//! The render thread (`wrlforge_render::thread`) owns every GPU object. The
//! UI thread never presents, reads back or polls the GPU.
//!
//! Teardown rule (Step 0): the UI asks the render thread to destroy; the
//! native window/layer stays alive until `Out::Destroyed`. If that does not
//! arrive within `PARK_AFTER`, the viewport is PARKED (hidden, still
//! realized) and freed when `Destroyed` arrives. Window close and app exit
//! wait for `Destroyed` up to `EXIT_DEADLINE`, then exit the process
//! WITHOUT destroying a native window a render thread may still use.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod smoke;
#[cfg(target_os = "linux")]
mod wl;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod none;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use mac as platform;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use none as platform;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use wrlforge_desktop_protocol as p;
use wrlforge_render::thread::{Fault, Handle, Out, Stats, Sweep};
use wrlforge_scene::RenderScene;

use crate::native::Native;
use crate::service::Service;

pub const PARK_AFTER: Duration = Duration::from_millis(500);
pub const EXIT_DEADLINE: Duration = Duration::from_secs(5);
/// A click must not travel further than this (logical px); longer is a drag.
pub const CLICK_SLOP: f64 = 4.0;

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub requested: bool,
    /// Test flag: pretend the surface offers only FIFO (exercises D8).
    pub force_fifo_only: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Vp {
    /// No viewport (never started, or removed after `Destroyed`).
    None,
    Attaching,
    Live,
    Destroying(Instant),
    Parked,
}

#[derive(Debug, Clone, Default)]
pub struct Counters {
    pub created: u32,
    pub destroyed: u32,
    pub joined: u32,
    pub parked: u32,
    pub unrealized_before_destroyed: u32,
    pub frames_presented: u64,
    pub frames_offscreen: u64,
    pub picks_sent: u64,
    pub picks_busy: u64,
    pub picks_answered: u64,
    pub last_stats: Option<String>,
}

pub(crate) struct Host {
    pub app: AppHandle,
    pub opts: Options,
    pub state: p::NativeState,
    pub handle: Option<Handle>,
    pub vp: Vp,
    pub plat: platform::Plat,
    /// The session whose projection was last handed to the viewport.
    pub session: Option<p::SessionId>,
    pub next_req: u64,
    pub picks: HashMap<u64, p::SessionId>,
    pub press: Option<(f64, f64)>,
    pub dragging: bool,
    pub last: (f64, f64),
    pub c: Counters,
    /// A pending window close / app exit (with its exit code), waiting for
    /// `Destroyed`.
    pub closing: Option<(Instant, i32)>,
    /// The deferred exit was issued: let it through.
    pub exited: bool,
    pub last_frame: Option<wrlforge_render::thread::FrameStamp>,
    pub sweeps: Vec<Sweep>,
    /// Highlight requests from the UI: (generation, pick ids).
    pub selects: Vec<(u64, Vec<u32>)>,
    pub picked_log: Vec<(wrlforge_render::thread::Picked, Option<p::PickOutcome>)>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Run `f` on the host. `None` when there is no host or it is borrowed (a
/// re-entrant GTK signal): the caller treats that as "not now".
pub(crate) fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub(crate) fn post_fn(app: &AppHandle) -> impl Fn(Out) + Send + 'static {
    let app = app.clone();
    move |o| {
        let _ = app.run_on_main_thread(move || on_out(o));
    }
}

fn emit(app: &AppHandle, ev: p::NativeEvent) {
    let _ = app.emit(p::NATIVE_EVENT, ev);
}

fn set_state(h: &mut Host, state: &str, reason: Option<String>, info: Option<String>) {
    h.state.state = state.into();
    h.state.reason = reason;
    if info.is_some() {
        h.state.info = info;
    }
    emit(&h.app, p::NativeEvent::State { state: h.state.clone() });
}

/// The current state (any thread may ask through the command).
pub fn state() -> p::NativeState {
    with(|h| h.state.clone()).unwrap_or_default()
}

/// Called once from `setup` on the UI thread. A start failure is a state,
/// never an app failure: the UI keeps X_ITE. Returns whether a host exists.
pub fn install(app: &AppHandle, opts: Options) -> bool {
    if !opts.requested {
        return false;
    }
    let plat = match platform::layout(app) {
        Ok(p) => p,
        Err(e) => {
            // No host: `state()` reports the default (off); log the reason.
            eprintln!("native viewport not started: {e}");
            return false;
        }
    };
    HOST.with(|h| {
        *h.borrow_mut() = Some(Host {
            app: app.clone(),
            opts,
            state: p::NativeState { requested: true, state: "starting".into(), reason: None, info: None },
            handle: None,
            vp: Vp::None,
            plat,
            session: None,
            next_req: 0,
            picks: HashMap::new(),
            press: None,
            dragging: false,
            last: (0.0, 0.0),
            c: Counters::default(),
            closing: None,
            exited: false,
            last_frame: None,
            sweeps: Vec::new(),
            selects: Vec::new(),
            picked_log: Vec::new(),
        })
    });
    platform::add_viewport();
    glib_like_timer();
    true
}

/// The UI-thread housekeeping tick: park viewports that did not answer
/// Destroy in time; finish a pending close.
fn glib_like_timer() {
    platform::every(Duration::from_millis(50), tick);
}

fn tick() {
    let park = with(|h| match h.vp {
        Vp::Destroying(t) if t.elapsed() >= PARK_AFTER => {
            h.vp = Vp::Parked;
            h.c.parked += 1;
            true
        }
        _ => false,
    })
    .unwrap_or(false);
    if park {
        platform::park();
    }
    let deadline = with(|h| h.closing.filter(|(t, _)| t.elapsed() >= EXIT_DEADLINE).map(|c| c.1)).flatten();
    if let Some(code) = deadline {
        // Never destroy a native window a render thread may still use.
        std::process::exit(code);
    }
}

/// Ask the render thread to stop. The native window stays until `Destroyed`.
pub(crate) fn teardown() {
    let remove_now = with(|h| {
        platform::cancel_attach(&mut h.plat);
        match (&h.handle, h.vp) {
            (Some(x), Vp::Live | Vp::Attaching) => {
                x.destroy();
                h.vp = Vp::Destroying(Instant::now());
                false
            }
            (None, Vp::Attaching) => {
                h.vp = Vp::None;
                true
            }
            _ => false,
        }
    })
    .unwrap_or(false);
    if remove_now {
        platform::remove_viewport();
        finish_close();
    }
}

fn finish_close() {
    let close = with(|h| match h.closing {
        Some((_, code)) if h.vp == Vp::None => {
            h.closing = None;
            h.exited = true;
            Some((h.app.clone(), code))
        }
        _ => None,
    })
    .flatten();
    if let Some((app, code)) = close {
        app.exit(code);
    }
}

/// Window close / app exit with `code`. `true`: deferred until the render
/// thread is gone (the host then exits with `code`).
pub fn defer_close(code: i32) -> bool {
    let live = with(|h| {
        if matches!(h.vp, Vp::None) || h.exited {
            return false;
        }
        if h.closing.is_none() {
            h.closing = Some((Instant::now(), code));
        }
        true
    })
    .unwrap_or(false);
    if live {
        teardown();
    }
    live
}

pub(crate) fn on_out(o: Out) {
    match o {
        Out::Ready { info, platform, init_ms, .. } => {
            with(|h| {
                let text = format!(
                    "{} · {} · {} · MSAA {}× · {platform} · init {init_ms:.0} ms",
                    info.adapter, info.backend, info.present_mode, info.msaa
                );
                set_state(h, "ready", None, Some(text));
            });
        }
        Out::Failed { reason, lost, fifo_only, .. } => {
            let why = if fifo_only {
                format!("Native viewport disabled: this Wayland compositor offers only FIFO presentation ({reason}). Using X_ITE.")
            } else if lost {
                format!("Native viewport stopped: the GPU device was lost ({reason}). Using X_ITE; restart the app to try again.")
            } else {
                format!("Native viewport failed: {reason}. Using X_ITE.")
            };
            with(|h| set_state(h, "failed", Some(why), None));
            // D8/D9: no restart. Release the viewport; the UI shows X_ITE.
            teardown();
        }
        Out::Presented { frame, presented, .. } => {
            with(|h| {
                if presented {
                    h.c.frames_presented += 1
                } else {
                    h.c.frames_offscreen += 1
                }
                h.last_frame = Some(frame);
            });
        }
        Out::Picked { pick, .. } => {
            let Some((app, session)) = with(|h| {
                h.c.picks_answered += 1;
                h.picks.remove(&pick.req).map(|s| (h.app.clone(), s))
            })
            .flatten() else {
                return;
            };
            // Resolution parses the document: off the UI thread.
            tauri::async_runtime::spawn_blocking(move || {
                let out = app.state::<Native>().resolve(&app.state::<Service>(), session, &pick);
                let a2 = app.clone();
                let logged = (pick, Some(out.clone()));
                let _ = app.run_on_main_thread(move || {
                    with(|h| h.picked_log.push(logged));
                    emit(&a2, p::NativeEvent::Pick { session, pick: out });
                });
            });
        }
        Out::Swept { sweep, .. } => {
            with(|h| h.sweeps.push(sweep));
        }
        Out::Destroyed { stats, .. } => {
            let removed = with(|h| {
                let joined = h.handle.as_mut().is_some_and(|x| x.join());
                h.handle = None;
                h.c.destroyed += 1;
                if joined {
                    h.c.joined += 1;
                }
                h.c.last_stats = Some(stats_text(&stats));
                h.vp = Vp::None;
                h.last_frame = None;
                true
            })
            .unwrap_or(false);
            if removed {
                // Only now: the window/layer the render thread used may go.
                platform::remove_viewport();
                finish_close();
            }
        }
    }
}

fn stats_text(s: &Stats) -> String {
    format!(
        "created={} panicked={} frames={} offscreen={} picks={} uncaptured_errors={} device_lost={}",
        s.created, s.panicked, s.frames, s.offscreen_frames, s.picks, s.uncaptured_errors, s.device_lost
    )
}

// ---- requests from commands (UI thread) ------------------------------------

/// Hand a new projection to the viewport. `refit` when the session changed.
/// A viewport created later takes `Native::current()` (see `spawn`).
pub fn set_scene(scene: Arc<RenderScene>) {
    with(|h| {
        let refit = h.session != Some(scene.session);
        h.session = Some(scene.session);
        if let Some(x) = &h.handle {
            x.set_scene(scene, refit);
        }
    });
}

/// The projection a newly created viewport starts with.
pub(crate) fn current_scene(app: &AppHandle) -> Option<Arc<RenderScene>> {
    app.state::<Native>().current()
}

pub fn select(generation: u64, ids: Vec<u32>) {
    with(|h| {
        h.selects.push((generation, ids.clone()));
        if let Some(x) = &h.handle {
            x.select(generation, ids);
        }
    });
}

pub fn reset_camera() {
    with(|h| {
        if let Some(x) = &h.handle {
            x.reset_camera();
        }
    });
}

/// A click at logical (`x`, `y`) of the viewport.
pub(crate) fn send_pick(h: &mut Host, x: f64, y: f64) -> Option<u64> {
    let session = h.session?;
    let handle = h.handle.as_ref()?;
    h.next_req += 1;
    let req = h.next_req;
    if handle.pick(req, x, y) {
        h.picks.insert(req, session);
        h.c.picks_sent += 1;
        Some(req)
    } else {
        h.c.picks_busy += 1;
        None
    }
}

// ---- pointer gesture (shared by the platform hosts) ------------------------

pub(crate) fn pointer_down(x: f64, y: f64) {
    with(|h| {
        h.press = Some((x, y));
        h.last = (x, y);
        h.dragging = false;
    });
}

pub(crate) fn pointer_move(x: f64, y: f64) {
    with(|h| {
        let Some((x0, y0)) = h.press else { return };
        if !h.dragging && ((x - x0).powi(2) + (y - y0).powi(2)).sqrt() > CLICK_SLOP {
            h.dragging = true;
        }
        if h.dragging {
            if let Some(v) = &h.handle {
                v.orbit(x - h.last.0, y - h.last.1);
            }
            h.last = (x, y);
        }
    });
}

pub(crate) fn pointer_up(x: f64, y: f64) {
    with(|h| {
        let was_click = h.press.is_some() && !h.dragging;
        h.press = None;
        h.dragging = false;
        if was_click {
            send_pick(h, x, y);
        }
    });
}

pub(crate) fn wheel(steps: f64) {
    with(|h| {
        if let Some(v) = &h.handle {
            v.zoom(steps);
        }
    });
}

/// Escape in the viewport: clear the selection in the WebView.
pub(crate) fn escape() {
    with(|h| emit(&h.app, p::NativeEvent::Cleared { session: h.session }));
}

// ---- test support (`--smoke-native`) --------------------------------------

pub(crate) fn test_pick(x: f64, y: f64) -> Option<u64> {
    with(|h| send_pick(h, x, y)).flatten()
}

pub(crate) fn test_fault(f: Fault) {
    with(|h| {
        if let Some(v) = &h.handle {
            v.inject(f);
        }
    });
}

pub(crate) fn test_sweep(req: u64, step: u32) {
    with(|h| {
        if let Some(v) = &h.handle {
            v.sweep(req, step);
        }
    });
}

pub(crate) fn test_visible(v: bool) {
    with(|h| {
        if let Some(x) = &h.handle {
            x.visible(v);
        }
    });
}

pub(crate) fn test_recreate() {
    teardown();
}

pub(crate) fn test_add() {
    platform::add_viewport();
}

pub(crate) fn snapshot<R>(f: impl FnOnce(&Host) -> R) -> Option<R> {
    with(|h| f(h))
}

pub(crate) fn platform_report() -> serde_json::Value {
    platform::report()
}
