// SPDX-License-Identifier: GPL-3.0-or-later
//! The render thread (Step 0 design, proven on X11, Wayland and macOS).
//!
//! One thread per viewport owns the wgpu instance, device, queue and surface.
//! The UI thread never presents, reads back or polls the device: it writes
//! requests into a small shared `Pending` record (the lock is held for
//! microseconds and never across a GPU call) and receives `Out` messages that
//! the render thread hands to `post` without blocking.
//!
//! The host supplies two platform pieces:
//! * `SurfaceFactory`, run ON this thread, creates the surface from
//!   thread-neutral inputs and returns a `keep` value (e.g. a private XCB
//!   connection) that is dropped only AFTER the surface;
//! * `Transact` wraps every surface configure and every draw (macOS: an
//!   explicit `CATransaction`, since this thread has no run loop).
//!
//! `Destroyed` is always the last message and is posted only after the
//! renderer, the surface and `keep` are gone. The host destroys the native
//! window only after it receives `Destroyed`.
//!
//! Picks are answered against the LAST DRAWN frame, before any queued camera,
//! size or scene change in the same batch is applied: a click names what was
//! on screen when it happened.

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wrlforge_scene::camera::{Orbit, View};
use wrlforge_scene::oracle::{self, Answer, Verdict};
use wrlforge_scene::RenderScene;

use crate::gpu::{Frame, GpuInfo, PresentPolicy, Renderer};

pub type Keep = Box<dyn Any + Send>;
pub type SurfaceFactory = Box<dyn FnOnce(&wgpu::Instance) -> Result<(wgpu::Surface<'static>, Keep, String), String> + Send>;
/// Called with `true` before and `false` after every surface configure and
/// every draw on the render thread.
pub type Transact = Arc<dyn Fn(bool) + Send + Sync>;

/// Picks queued at once; more are refused by the caller.
pub const MAX_PICKS_IN_FLIGHT: usize = 4;
/// Idle poll period: wgpu-core reports a lost device only from a poll that
/// finds the queue empty, so the loop polls even without UI traffic.
pub const HEARTBEAT: Duration = Duration::from_millis(250);

/// Test-only fault injection (D14 / Step 0 cases).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fault {
    /// Hold the next acquired frame this long before present.
    BlockPresent(Duration),
    /// Destroy the device (a device loss without driver help).
    DeviceLoss,
    /// Make every following GPU pick answer `id + 1`.
    CorruptPick,
}

#[derive(Default)]
struct Pending {
    size: Option<(u32, u32, f64)>,
    scene: Option<(Arc<RenderScene>, bool)>,
    orbit: (f64, f64),
    zoom: f64,
    reset: bool,
    selected: Option<(u64, Vec<u32>)>,
    picks: VecDeque<(u64, f64, f64)>,
    visible: Option<bool>,
    faults: Vec<Fault>,
    sweep: Option<(u64, u32)>,
    destroy: bool,
    dirty: bool,
}

/// The binding of a frame (and of a pick): what was on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStamp {
    pub seq: u64,
    pub session: u64,
    pub revision: u64,
    pub generation: u64,
    pub text_hash: u64,
    pub width: u32,
    pub height: u32,
    /// The camera of the frame and its physical-per-logical scale.
    pub view: View,
    pub scale: f64,
}

impl FrameStamp {
    fn of(f: &Frame) -> FrameStamp {
        FrameStamp {
            seq: f.seq,
            session: f.scene.session,
            revision: f.scene.revision,
            generation: f.scene.generation,
            text_hash: f.scene.text_hash,
            width: f.view.width,
            height: f.view.height,
            view: f.view,
            scale: f.scale,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Picked {
    pub req: u64,
    pub verdict: Verdict,
    pub oracle: Option<Answer>,
    pub gpu: Option<Result<u32, String>>,
    /// The frame the pick was answered against; `None` when there was none.
    pub frame: Option<FrameStamp>,
    pub scene: Option<Arc<RenderScene>>,
    pub physical: Option<(u32, u32)>,
    pub ms: f64,
}

/// Full-frame GPU-vs-oracle comparison (test evidence).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sweep {
    pub req: u64,
    pub frame: Option<FrameStamp>,
    pub samples: u64,
    pub agree_object: u64,
    pub agree_background: u64,
    pub refused_band: u64,
    pub disagree: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub created: bool,
    pub panicked: bool,
    pub frames: u64,
    pub offscreen_frames: u64,
    pub picks: u64,
    pub uncaptured_errors: u64,
    pub device_lost: bool,
}

#[derive(Debug)]
pub enum Out {
    Ready { id: u64, info: GpuInfo, platform: String, init_ms: f64 },
    /// `lost`: the device was lost (D9: no restart). `fifo_only`: D8.
    Failed { id: u64, reason: String, lost: bool, fifo_only: bool },
    Presented { id: u64, frame: FrameStamp, presented: bool, ms: f64 },
    Picked { id: u64, pick: Picked },
    Swept { id: u64, sweep: Sweep },
    Destroyed { id: u64, stats: Stats },
}

pub struct Handle {
    pub id: u64,
    shared: Arc<(Mutex<Pending>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

pub struct Config {
    pub id: u64,
    pub size: (u32, u32, f64),
    pub policy: PresentPolicy,
    /// Test: drop every non-FIFO present mode (exercises D8).
    pub force_fifo_only: bool,
}

pub fn spawn(cfg: Config, factory: SurfaceFactory, transact: Option<Transact>, post: impl Fn(Out) + Send + 'static) -> std::io::Result<Handle> {
    let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
    let s2 = shared.clone();
    let id = cfg.id;
    let thread = std::thread::Builder::new().name(format!("wrlforge-render-{id}")).spawn(move || {
        let post = &post;
        let r = catch_unwind(AssertUnwindSafe(|| body(cfg, factory, transact, &s2, post)));
        let stats = r.unwrap_or_else(|_| {
            post(Out::Failed { id, reason: "render thread panic".into(), lost: false, fifo_only: false });
            Stats { panicked: true, ..Stats::default() }
        });
        // Last message: renderer, surface and keep are already dropped.
        post(Out::Destroyed { id, stats });
    })?;
    Ok(Handle { id, shared, thread: Some(thread) })
}

impl Handle {
    fn with(&self, f: impl FnOnce(&mut Pending)) {
        // A poisoned lock means the render thread panicked; it posts Failed.
        if let Ok(mut g) = self.shared.0.lock() {
            f(&mut g);
            g.dirty = true;
        }
        self.shared.1.notify_one();
    }
    /// Physical size and scale.
    pub fn configure(&self, w: u32, h: u32, scale: f64) {
        self.with(|p| p.size = Some((w, h, scale)));
    }
    /// A new projection. `refit`: frame the camera on it (first scene of a
    /// document); otherwise the camera stays where the user put it.
    pub fn set_scene(&self, scene: Arc<RenderScene>, refit: bool) {
        self.with(|p| {
            let refit = refit || p.scene.as_ref().is_some_and(|s| s.1);
            p.scene = Some((scene, refit));
        });
    }
    pub fn orbit(&self, dx: f64, dy: f64) {
        self.with(|p| {
            p.orbit.0 += dx;
            p.orbit.1 += dy;
        });
    }
    pub fn zoom(&self, steps: f64) {
        self.with(|p| p.zoom += steps);
    }
    pub fn reset_camera(&self) {
        self.with(|p| p.reset = true);
    }
    /// Highlight `ids` of `generation` (ignored for any other generation).
    pub fn select(&self, generation: u64, ids: Vec<u32>) {
        self.with(|p| p.selected = Some((generation, ids)));
    }
    /// Queue a pick at logical (`x`, `y`). False when full (caller refuses).
    pub fn pick(&self, req: u64, x: f64, y: f64) -> bool {
        let mut ok = false;
        self.with(|p| {
            if p.picks.len() < MAX_PICKS_IN_FLIGHT {
                p.picks.push_back((req, x, y));
                ok = true;
            }
        });
        ok
    }
    pub fn visible(&self, v: bool) {
        self.with(|p| p.visible = Some(v));
    }
    pub fn inject(&self, f: Fault) {
        self.with(|p| p.faults.push(f));
    }
    /// Compare the GPU id image with the oracle every `step` pixels.
    pub fn sweep(&self, req: u64, step: u32) {
        self.with(|p| p.sweep = Some((req, step.max(1))));
    }
    pub fn destroy(&self) {
        self.with(|p| p.destroy = true);
    }
    /// Call after `Destroyed`: the thread has already returned.
    pub fn join(&mut self) -> bool {
        self.thread.take().map(|t| t.join().is_ok()).unwrap_or(false)
    }
}

fn wait(shared: &(Mutex<Pending>, Condvar)) -> Option<Pending> {
    let Ok(g) = shared.0.lock() else { return Some(Pending { destroy: true, ..Default::default() }) };
    let mut g = match shared.1.wait_timeout_while(g, HEARTBEAT, |p| !p.dirty) {
        Ok((g, _)) => g,
        Err(_) => return Some(Pending { destroy: true, ..Default::default() }),
    };
    if !g.dirty {
        return None;
    }
    Some(std::mem::take(&mut *g))
}

fn in_tx<R>(t: &Option<Transact>, f: impl FnOnce() -> R) -> R {
    if let Some(t) = t {
        t(true);
    }
    let r = f();
    if let Some(t) = t {
        t(false);
    }
    r
}

struct State {
    orbit: Orbit,
    fitted: bool,
    scale: f64,
    size: (u32, u32),
    selected: (u64, Vec<u32>),
    corrupt_pick: bool,
    last: Option<Frame>,
    picks: u64,
}

fn bounds(scene: &RenderScene) -> Option<(glam::DVec3, f64)> {
    let mut lo = glam::DVec3::splat(f64::INFINITY);
    let mut hi = glam::DVec3::splat(f64::NEG_INFINITY);
    for o in &scene.objects {
        let r = o.mesh.radius();
        for c in [-1.0, 1.0] {
            for d in [-1.0, 1.0] {
                for e in [-1.0, 1.0] {
                    let p = o.world.transform_point3(glam::DVec3::new(c * r, d * r, e * r));
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
    }
    (lo.is_finite() && hi.is_finite()).then(|| ((lo + hi) / 2.0, ((hi - lo) / 2.0).length().max(1e-6)))
}

fn body(cfg: Config, factory: SurfaceFactory, transact: Option<Transact>, shared: &(Mutex<Pending>, Condvar), post: &dyn Fn(Out)) -> Stats {
    let id = cfg.id;
    let t0 = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (surface, keep, platform) = match factory(&instance) {
        Ok(x) => x,
        Err(e) => {
            post(Out::Failed { id, reason: e, lost: false, fifo_only: false });
            return idle_until_destroy(shared);
        }
    };
    let (w, h, scale) = cfg.size;
    let mut r = match in_tx(&transact, || Renderer::new(&instance, surface, w, h, cfg.policy, cfg.force_fifo_only)) {
        Ok(r) => r,
        Err(e) => {
            drop(keep);
            let fifo_only = e.starts_with(crate::gpu::FIFO_ONLY);
            post(Out::Failed { id, reason: e, lost: false, fifo_only });
            return idle_until_destroy(shared);
        }
    };
    post(Out::Ready { id, info: r.info.clone(), platform, init_ms: t0.elapsed().as_secs_f64() * 1e3 });
    let mut st = State { orbit: Orbit::default(), fitted: false, scale, size: (w.max(1), h.max(1)), selected: (0, Vec::new()), corrupt_pick: false, last: None, picks: 0 };
    let mut lost_reported = false;
    loop {
        let Some(p) = wait(shared) else {
            let _ = r.device.poll(wgpu::PollType::Poll);
            report_loss(&r, id, &mut lost_reported, post);
            continue;
        };
        if p.destroy {
            break;
        }
        for f in &p.faults {
            match f {
                Fault::BlockPresent(d) => r.hold = Some(*d),
                Fault::DeviceLoss => r.device.destroy(),
                Fault::CorruptPick => st.corrupt_pick = true,
            }
        }
        let _ = r.device.poll(wgpu::PollType::Poll);
        if r.is_lost() {
            report_loss(&r, id, &mut lost_reported, post);
            // D9: no automatic restart, no further GPU call until Destroy.
            for (req, ..) in p.picks {
                post(Out::Picked { id, pick: refused(req, "device-lost", None) });
            }
            continue;
        }
        // 1. Picks, against the frame that was on screen. A size change in
        // the same batch means the click may have been on a resized view.
        let resizing = p.size.is_some_and(|(w, h, s)| st.last.as_ref().is_none_or(|f| (f.view.width, f.view.height, f.scale) != (w.max(1), h.max(1), s)));
        for (req, x, y) in p.picks {
            let t = Instant::now();
            let mut pick = if resizing { refused(req, "viewport-resizing", st.last.as_ref()) } else { answer(&r, &st, req, x, y) };
            pick.ms = t.elapsed().as_secs_f64() * 1e3;
            st.picks += 1;
            post(Out::Picked { id, pick });
        }
        if let Some((req, step)) = p.sweep {
            post(Out::Swept { id, sweep: sweep(&r, &st, req, step) });
        }
        // 2. Then the queued changes, then one draw.
        let mut draw = false;
        if let Some(v) = p.visible {
            r.present_enabled = v;
            draw = true;
        }
        if let Some((w, h, s)) = p.size {
            st.size = (w.max(1), h.max(1));
            st.scale = s;
            in_tx(&transact, || r.resize(w, h));
            draw = true;
        }
        if let Some((scene, refit)) = p.scene {
            if refit || !st.fitted {
                st.orbit = Orbit::fit(bounds(&scene));
                st.fitted = true;
            }
            if let Err(e) = r.set_scene(scene) {
                post(Out::Failed { id, reason: e, lost: false, fifo_only: false });
            }
            draw = true;
        }
        if p.reset {
            st.orbit = r.scene().map_or_else(Orbit::default, |s| Orbit::fit(bounds(s)));
            draw = true;
        }
        if p.orbit != (0.0, 0.0) {
            st.orbit.orbit(p.orbit.0, p.orbit.1);
            draw = true;
        }
        if p.zoom != 0.0 {
            st.orbit.zoom(p.zoom);
            draw = true;
        }
        if let Some(sel) = p.selected {
            st.selected = sel;
            draw = true;
        }
        if draw && r.scene().is_some() {
            let t = Instant::now();
            let view = View::new(&st.orbit, st.size.0, st.size.1);
            let generation = r.scene().map_or(0, |s| s.generation);
            let sel: &[u32] = if st.selected.0 == generation { &st.selected.1 } else { &[] };
            let sel = sel.to_vec();
            match in_tx(&transact, || r.draw(&view, st.scale, &sel)) {
                Ok(f) => {
                    post(Out::Presented { id, frame: FrameStamp::of(&f), presented: f.presented, ms: t.elapsed().as_secs_f64() * 1e3 });
                    st.last = Some(f);
                }
                Err(_) => report_loss(&r, id, &mut lost_reported, post),
            }
        }
    }
    let stats = Stats {
        created: true,
        panicked: false,
        frames: r.frames,
        offscreen_frames: r.offscreen_frames,
        picks: st.picks,
        uncaptured_errors: r.uncaptured_errors.load(std::sync::atomic::Ordering::SeqCst),
        device_lost: r.is_lost(),
    };
    // Order: the frame's scene reference, the renderer (GPU idle with a
    // limit, resources, device, surface), then the host's keep.
    drop(st);
    drop(r);
    drop(keep);
    drop(instance);
    stats
}

fn report_loss(r: &Renderer, id: u64, reported: &mut bool, post: &dyn Fn(Out)) {
    if r.is_lost() && !*reported {
        *reported = true;
        let why = r.lost_reason.lock().map(|g| g.clone()).unwrap_or_default();
        post(Out::Failed { id, reason: format!("device lost: {why}"), lost: true, fifo_only: false });
    }
}

fn refused(req: u64, why: &'static str, frame: Option<&Frame>) -> Picked {
    Picked {
        req,
        verdict: Verdict::Refused(why),
        oracle: None,
        gpu: None,
        frame: frame.map(FrameStamp::of),
        scene: frame.map(|f| f.scene.clone()),
        physical: None,
        ms: 0.0,
    }
}

/// Logical (`x`, `y`) of the last frame -> its physical pixel, or `None`.
fn physical(f: &Frame, x: f64, y: f64) -> Option<(u32, u32)> {
    let (px, py) = ((x * f.scale).floor(), (y * f.scale).floor());
    (px.is_finite() && py.is_finite() && px >= 0.0 && py >= 0.0 && px < f.view.width as f64 && py < f.view.height as f64).then_some((px as u32, py as u32))
}

fn answer(r: &Renderer, st: &State, req: u64, x: f64, y: f64) -> Picked {
    let Some(f) = &st.last else { return refused(req, "no-frame-shown", None) };
    if !f.presented {
        return refused(req, "frame-not-shown", Some(f));
    }
    let Some((px, py)) = physical(f, x, y) else { return refused(req, oracle::reason::OUTSIDE, Some(f)) };
    let cpu = oracle::pick(&f.scene.objects, &f.view, px, py);
    let gpu = r.pick_id(f, px, py).map(|g| if st.corrupt_pick { g.wrapping_add(1) } else { g });
    let verdict = oracle::agree(cpu, gpu.clone().map_err(|_| ()));
    Picked {
        req,
        verdict,
        oracle: Some(cpu),
        gpu: Some(gpu),
        frame: Some(FrameStamp::of(f)),
        scene: Some(f.scene.clone()),
        physical: Some((px, py)),
        ms: 0.0,
    }
}

fn sweep(r: &Renderer, st: &State, req: u64, step: u32) -> Sweep {
    let mut s = Sweep { req, ..Sweep::default() };
    let Some(f) = &st.last else {
        s.error = Some("no frame".into());
        return s;
    };
    s.frame = Some(FrameStamp::of(f));
    let ids = match r.id_image(f) {
        Ok(i) => i,
        Err(e) => {
            s.error = Some(e);
            return s;
        }
    };
    let (w, h) = (f.view.width, f.view.height);
    for py in (step / 2..h).step_by(step as usize) {
        for px in (step / 2..w).step_by(step as usize) {
            let gpu = ids.get((py * w + px) as usize).copied().ok_or(());
            s.samples += 1;
            match oracle::agree(oracle::pick(&f.scene.objects, &f.view, px, py), gpu) {
                Verdict::Object(_) => s.agree_object += 1,
                Verdict::Background => s.agree_background += 1,
                Verdict::Refused(oracle::reason::DISAGREE) | Verdict::Refused(oracle::reason::GPU_FAILED) => s.disagree += 1,
                Verdict::Refused(_) => s.refused_band += 1,
            }
        }
    }
    s
}

fn idle_until_destroy(shared: &(Mutex<Pending>, Condvar)) -> Stats {
    loop {
        if let Some(p) = wait(shared) {
            if p.destroy {
                return Stats::default();
            }
        }
    }
}
