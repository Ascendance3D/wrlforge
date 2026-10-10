// SPDX-License-Identifier: GPL-3.0-or-later
//! `--smoke-native <dir>`: the NATIVE-RENDER-1 self-test.
//!
//! Runs inside the real app (WebView + native viewport) on a DISPOSABLE
//! fixture written into `<dir>`. It sends NO operating-system input: clicks
//! are requests to the viewport host at computed logical positions (the
//! same path a real click takes after GTK/AppKit), and Escape is the host's
//! own handler. The harness runs it on a private display it proves it owns.
//!
//! Checks: selection through the WebView's one selection authority, every
//! refusal class, GPU/CPU agreement over the frame, a forced disagreement,
//! stale frames, hidden frames, create/destroy cycles, a blocked present
//! with teardown, device loss (D9) and the UI-thread watchdog (D14). With
//! `--native-force-fifo-only` it checks the D8 path instead.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use wrlforge_desktop_protocol as p;
use wrlforge_render::thread::{Fault, FrameStamp};
use wrlforge_scene::oracle::{self, Answer, Verdict};
use wrlforge_scene::RenderScene;

use super::{snapshot, Vp};
use crate::commands::Startup;
use crate::native::Native;
use crate::service::Service;

const WATCH_P99_MS: f64 = 50.0;
const WATCH_MAX_MS: f64 = 100.0;
const GATED: [&str; 6] = ["gpu-load", "hidden", "blocked-present", "cleanup", "device-loss", "exit"];
const CYCLES: u32 = 3;

pub const FIXTURE: &str = "#VRML V2.0 utf8
# NATIVE-RENDER-1 smoke fixture (disposable copy).
Transform { translation -3 1.5 0 children Shape { appearance Appearance { material Material { diffuseColor 0.9 0.2 0.2 } } geometry Box { size 1.5 1.5 1.5 } } }
Transform { translation 0 1.5 0 children Shape { appearance Appearance { material Material { diffuseColor 0.2 0.8 0.3 specularColor 1 1 1 shininess 0.5 } } geometry Sphere { radius 0.9 } } }
Transform { translation 3 1.5 0 rotation 0 0 1 0.3 children Shape { appearance Appearance { material Material { diffuseColor 0.2 0.4 0.9 } } geometry Cylinder { radius 0.7 height 1.6 } } }
Transform { translation -3 -1.5 0 scale -1 1 1 children Shape { appearance Appearance { material Material { diffuseColor 0.9 0.8 0.2 } } geometry Cone { bottomRadius 0.8 height 1.6 } } }
Transform { translation 0 -1.5 0 children [ TouchSensor {} Shape { geometry Box { size 1.2 1.2 1.2 } } ] }
DEF SHARED Transform { translation 3 -1.5 0 children Shape { appearance Appearance { material Material { diffuseColor 0.7 0.7 0.7 transparency 0.4 } } geometry Sphere { radius 0.7 } } }
Transform { translation 0 -5 0 children USE SHARED }
Group { children Shape { geometry Box {} } }
Transform { translation 0 0 -4 scale 4 3 0.2 children Shape { appearance Appearance { material Material { diffuseColor 0.3 0.3 0.35 } } geometry Box {} } }
";

/// Expected pick status per pick id (fixture order).
const EXPECT: [(u32, &str, &str); 7] = [
    (1, "PROVEN", "box"),
    (2, "PROVEN", "sphere"),
    (3, "PROVEN", "cylinder"),
    (4, "PROVEN", "mirrored cone"),
    (5, "REFUSED_SENSOR_CONFLICT", "box beside a TouchSensor"),
    (6, "REFUSED_AMBIGUOUS", "DEF/USE sphere (transparent)"),
    (7, "PROVEN", "scaled backdrop"),
];

struct Smoke {
    app: AppHandle,
    report: PathBuf,
    fifo_only: bool,
    step: u32,
    t: Instant,
    t0: Instant,
    checks: Vec<Value>,
    phase: String,
    watch: Arc<std::sync::Mutex<BTreeMap<String, Vec<f64>>>>,
    phase_shared: Arc<std::sync::Mutex<String>>,
    stop: Arc<AtomicBool>,
    /// Raw timing evidence (`<report>.raw.json`). Recorded only; the gate
    /// reads `watch`, exactly as before.
    trace: Arc<Trace>,
    /// Per-step scratch: the request in flight and what it expects.
    pending: Option<Pending>,
    queue: Vec<Pending>,
    cycle: u32,
    marks: BTreeMap<&'static str, u64>,
    notes: BTreeMap<String, Value>,
    finished: bool,
    /// `--smoke-native-control`: NO native viewport; measure the UI thread
    /// of the same app, fixture and display (the ping response delay
    /// without the native viewport, D14).
    control: bool,
}

#[derive(Clone)]
struct Pending {
    label: String,
    at: (f64, f64),
    want_status: Vec<&'static str>,
    want_reason: Option<&'static str>,
    want_id: Option<u32>,
    sent_log: usize,
    sent_select: usize,
    expect_select: bool,
}

thread_local! {
    static S: RefCell<Option<Smoke>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Smoke) -> R) -> Option<R> {
    S.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Write the fixture, open it as the startup document, start the run.
pub fn arm(app: &AppHandle, dir: PathBuf, report: Option<PathBuf>, control: bool) {
    let fixture = dir.join("native-fixture.wrl");
    let report = report.unwrap_or_else(|| dir.join("native-report.json"));
    let fifo_only = super::snapshot(|h| h.opts.force_fifo_only).unwrap_or(false);
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&fixture, FIXTURE)) {
        eprintln!("smoke-native: fixture: {e}");
        std::process::exit(2);
    }
    let outcome = app.state::<Service>().open_path(&fixture);
    if let Ok(mut s) = app.state::<Startup>().0.lock() {
        *s = Some(outcome);
    }
    let watch = Arc::new(std::sync::Mutex::new(BTreeMap::new()));
    let phase_shared = Arc::new(std::sync::Mutex::new("startup".to_string()));
    let stop = Arc::new(AtomicBool::new(false));
    let trace = Arc::new(Trace::new());
    start_watchdog(
        app.clone(),
        watch.clone(),
        phase_shared.clone(),
        stop.clone(),
        trace.clone(),
    );
    start_sampler(trace.clone(), stop.clone());
    S.with(|s| {
        *s.borrow_mut() = Some(Smoke {
            app: app.clone(),
            report,
            fifo_only,
            step: 0,
            t: Instant::now(),
            t0: Instant::now(),
            checks: Vec::new(),
            phase: "startup".into(),
            watch,
            phase_shared,
            stop,
            trace,
            pending: None,
            queue: Vec::new(),
            cycle: 0,
            marks: BTreeMap::new(),
            notes: BTreeMap::new(),
            finished: false,
            control,
        })
    });
    #[cfg(target_os = "macos")]
    super::platform::test_present_unfocused();
    super::platform::every(Duration::from_millis(16), tick);
}

/// UI-thread latency, measured from another thread: a ping every 5 ms
/// through the same main-loop queue every UI event uses.
fn start_watchdog(
    app: AppHandle,
    watch: Arc<std::sync::Mutex<BTreeMap<String, Vec<f64>>>>,
    phase: Arc<std::sync::Mutex<String>>,
    stop: Arc<AtomicBool>,
    trace: Arc<Trace>,
) {
    let _ = std::thread::Builder::new()
        .name("native-watchdog".into())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
                let sent = Instant::now();
                let sent_phase = phase.lock().map(|g| g.clone()).unwrap_or_default();
                let (w, ph, tr) = (watch.clone(), phase.clone(), trace.clone());
                tr.sent.fetch_add(1, Ordering::SeqCst);
                let _ = app.run_on_main_thread(move || {
                    let ran = Instant::now();
                    let ms = ran.duration_since(sent).as_secs_f64() * 1e3;
                    let name = ph.lock().map(|g| g.clone()).unwrap_or_default();
                    if let Ok(mut g) = tr.pings.lock() {
                        g.push(json!([tr.ms(sent), tr.ms(ran), sent_phase, name]));
                    }
                    if let Ok(mut g) = w.lock() {
                        g.entry(name).or_default().push(ms);
                    }
                });
            }
        });
}

/// Closeout evidence for the D14 watchdog: raw pings, phase changes, and
/// the UI and render threads' scheduler state every 1 ms (CPU time,
/// run-queue wait, syscall, wchan). Pings queue (tao runs one per GTK
/// pass), so a long sample can be a busy UI thread and a queued ping at
/// once. Linux only; elsewhere the samples stay empty.
struct Trace {
    base: Instant,
    sent: std::sync::atomic::AtomicU64,
    pings: std::sync::Mutex<Vec<Value>>,
    phases: std::sync::Mutex<Vec<Value>>,
    samples: std::sync::Mutex<Vec<Value>>,
}

impl Trace {
    fn new() -> Self {
        Trace {
            base: Instant::now(),
            sent: Default::default(),
            pings: Default::default(),
            phases: Default::default(),
            samples: Default::default(),
        }
    }
    fn ms(&self, t: Instant) -> f64 {
        (t.duration_since(self.base).as_secs_f64() * 1e6).round() / 1e3
    }
}

/// `[state, run_ns, runqueue_wait_ns, syscall, wchan]` for one thread.
fn thread_sample(tid: u32) -> Option<Value> {
    let dir = format!("/proc/self/task/{tid}");
    let stat = std::fs::read_to_string(format!("{dir}/stat")).ok()?;
    let state = stat.rsplit_once(") ")?.1.split(' ').next()?.to_string();
    let sched = std::fs::read_to_string(format!("{dir}/schedstat")).ok()?;
    let mut it = sched
        .split_whitespace()
        .map(|x| x.parse::<u64>().unwrap_or(0));
    let (run, wait) = (it.next()?, it.next()?);
    let sys = std::fs::read_to_string(format!("{dir}/syscall")).unwrap_or_default();
    let sys = sys.split_whitespace().next().unwrap_or("?").to_string();
    let wchan = std::fs::read_to_string(format!("{dir}/wchan")).unwrap_or_default();
    Some(json!([state, run, wait, sys, wchan]))
}

fn start_sampler(trace: Arc<Trace>, stop: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new()
        .name("native-sampler".into())
        .spawn(move || {
            let main = std::process::id();
            let mut render: Vec<u32> = Vec::new();
            let mut n = 0u64;
            while !stop.load(Ordering::SeqCst) {
                if n.is_multiple_of(20) {
                    render = std::fs::read_dir("/proc/self/task")
                        .map(|d| {
                            d.filter_map(|e| e.ok())
                                .filter(|e| {
                                    std::fs::read_to_string(e.path().join("comm"))
                                        .is_ok_and(|c| c.starts_with("wrlforge-render"))
                                })
                                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                                .collect()
                        })
                        .unwrap_or_default();
                }
                n += 1;
                let t = trace.ms(Instant::now());
                let ui = thread_sample(main);
                let rt: Vec<Value> = render
                    .iter()
                    .filter_map(|&tid| Some(json!([tid, thread_sample(tid)?])))
                    .collect();
                if let Ok(mut g) = trace.samples.lock() {
                    g.push(json!([t, ui, rt]));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
}

fn write_trace(s: &Smoke) {
    let load = |p: &str| {
        std::fs::read_to_string(p)
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    };
    let take = |m: &std::sync::Mutex<Vec<Value>>| m.lock().map(|g| g.clone()).unwrap_or_default();
    let raw = json!({
        "format": {
            "pings": "[sent_ms, ran_ms, phase_at_send, phase_at_run]; latency = ran - sent; the gate uses phase_at_run",
            "phases": "[t_ms, phase]",
            "samples": "[t_ms, ui_thread, [[tid, render_thread], ...]]; thread = [state, run_ns, runqueue_wait_ns, syscall_nr_or_running, wchan]",
        },
        // CLOCK_MONOTONIC of t_ms = 0 (Linux `Instant` debug form), to
        // align with the harness's compositor and GPU samples.
        "base_monotonic": format!("{:?}", s.trace.base),
        "pings_sent": s.trace.sent.load(Ordering::SeqCst),
        "loadavg_end": load("/proc/loadavg"),
        "cpu_end": load("/proc/stat"),
        "phases": take(&s.trace.phases),
        "pings": take(&s.trace.pings),
        "samples": take(&s.trace.samples),
    });
    let _ = std::fs::write(
        s.report.with_extension("raw.json"),
        serde_json::to_string(&raw).unwrap_or_default(),
    );
}

fn check(s: &mut Smoke, name: &str, pass: bool, detail: impl std::fmt::Display) {
    s.checks.push(json!({ "name": name, "pass": pass, "detail": detail.to_string() }));
}

fn set_phase(s: &mut Smoke, p: &str) {
    s.phase = p.into();
    if let Ok(mut g) = s.trace.phases.lock() {
        g.push(json!([s.trace.ms(Instant::now()), p]));
    }
    if let Ok(mut g) = s.phase_shared.lock() {
        *g = p.into();
    }
}

fn next(s: &mut Smoke, step: u32) {
    s.step = step;
    s.t = Instant::now();
}

/// Threads named like a render thread (driver threads it created inherit
/// the name, and end with its device).
fn render_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.starts_with("wrlforge-render")))
                .count()
        })
        .unwrap_or(0)
}

fn current(s: &Smoke) -> Option<Arc<RenderScene>> {
    s.app.state::<Native>().current()
}

/// The last frame, if it shows the current projection.
fn frame_of_current(s: &Smoke) -> Option<FrameStamp> {
    let scene = current(s)?;
    snapshot(|h| h.last_frame).flatten().filter(|f| f.generation == scene.generation)
}

/// Logical position of an object's world origin in frame `f`.
fn center(scene: &RenderScene, f: &FrameStamp, id: u32) -> Option<(f64, f64)> {
    let o = scene.object(id)?;
    let c = f.view.view_proj() * o.world.transform_point3(glam::DVec3::ZERO).extend(1.0);
    let (nx, ny) = (c.x / c.w, c.y / c.w);
    let (px, py) = ((nx + 1.0) / 2.0 * f.width as f64, (1.0 - ny) / 2.0 * f.height as f64);
    Some((px / f.scale, py / f.scale))
}

/// First pixel (logical) on a scan where the oracle answers `want`.
fn scan(scene: &RenderScene, f: &FrameStamp, want: impl Fn(Answer) -> bool, through: Option<(f64, f64)>) -> Option<(f64, f64)> {
    let rows: Vec<u32> = match through {
        Some((_, y)) => vec![(y * f.scale) as u32],
        None => (0..f.height).step_by(7).collect(),
    };
    for py in rows {
        for px in 0..f.width {
            if want(oracle::pick(&scene.objects, &f.view, px, py)) {
                return Some(((px as f64 + 0.5) / f.scale, (py as f64 + 0.5) / f.scale));
            }
        }
    }
    None
}

fn send(s: &mut Smoke, mut pd: Pending) {
    pd.sent_log = snapshot(|h| h.picked_log.len()).unwrap_or(0);
    pd.sent_select = snapshot(|h| h.selects.len()).unwrap_or(0);
    super::test_pick(pd.at.0, pd.at.1);
    s.pending = Some(pd);
    s.t = Instant::now();
}

/// Has the in-flight pick been answered (and adopted, when expected)?
fn settle(s: &mut Smoke) -> Option<bool> {
    let pd = s.pending.clone()?;
    let (log, selects) = snapshot(|h| (h.picked_log.get(pd.sent_log).cloned(), h.selects[pd.sent_select.min(h.selects.len())..].to_vec()))?;
    let timed_out = s.t.elapsed() > Duration::from_secs(5);
    let Some((picked, outcome)) = log else {
        if timed_out {
            check(s, &format!("pick:{}", pd.label), false, "no answer in 5 s");
            s.pending = None;
            return Some(true);
        }
        return None;
    };
    let o = outcome.unwrap_or_else(|| p::PickOutcome {
        status: "NONE".into(),
        reason: String::new(),
        message: String::new(),
        generation: 0,
        revision: 0,
        item: None,
        role: None,
        logical: None,
        shape: None,
    });
    let id_ok = pd.want_id.is_none_or(|w| matches!(picked.verdict, Verdict::Object(g) if g == w));
    let status_ok = pd.want_status.contains(&o.status.as_str());
    let reason_ok = pd.want_reason.is_none_or(|r| o.reason == r);
    if pd.expect_select {
        // Adoption: the WebView applied it through its one selection
        // authority and asked Rust to highlight exactly this object.
        let adopted = selects.iter().any(|(_, ids)| pd.want_id.is_some_and(|w| ids == &vec![w]));
        if !adopted && !timed_out {
            return None;
        }
        check(s, &format!("ui-adopted:{}", pd.label), adopted, format!("highlight requests since pick: {selects:?}"));
    }
    check(
        s,
        &format!("pick:{}", pd.label),
        id_ok && status_ok && reason_ok,
        format!(
            "verdict={:?} oracle={:?} gpu={:?} status={} reason={} item={:?} frame={:?} ms={:.2}",
            picked.verdict,
            picked.oracle,
            picked.gpu,
            o.status,
            o.reason,
            o.item,
            picked.frame.map(|f| (f.seq, f.generation, f.revision)),
            picked.ms
        ),
    );
    s.pending = None;
    Some(true)
}

fn tick() {
    let done = with(|s| !s.finished && step(s)).unwrap_or(false);
    if done {
        finish();
    }
}

/// One step of the run. Returns true when the report should be written.
fn step(s: &mut Smoke) -> bool {
    let el = s.t.elapsed();
    if s.control {
        match s.step {
            0 if el > Duration::from_secs(4) => {
                set_phase(s, "control-idle");
                next(s, 1);
            }
            1 if el > Duration::from_secs(8) => {
                check(s, "control:native-viewport-off", snapshot(|_| ()).is_none() && render_threads() == 0, "no host, no render thread");
                return true;
            }
            _ => {}
        }
        return false;
    }
    let (vp, state) = match snapshot(|h| (h.vp, h.state.clone())) {
        Some(x) => x,
        None => {
            check(s, "native-host-installed", false, "no host");
            return true;
        }
    };
    if s.t0.elapsed() > Duration::from_secs(150) {
        check(s, "run-finished-in-time", false, format!("stuck at step {}", s.step));
        return true;
    }
    match s.step {
        // ---- D8: FIFO-only Wayland disables native, X_ITE stays ----------
        0 if s.fifo_only => {
            if state.state == "failed" && vp == Vp::None {
                let r = state.reason.clone().unwrap_or_default();
                check(s, "d8:fifo-only-disables-native", r.contains("only FIFO"), &r);
                check(s, "d8:no-viewport-left", vp == Vp::None && render_threads() == 0, format!("render threads {}", render_threads()));
                check(s, "d8:ui-shows-x_ite", !state.native_shown(), format!("state {}", state.state));
                return true;
            }
            if el > Duration::from_secs(20) {
                check(s, "d8:fifo-only-disables-native", false, format!("state {} after 20 s", state.state));
                return true;
            }
        }
        // ---- startup -----------------------------------------------------
        0 => {
            if state.state == "failed" {
                check(s, "native-ready", false, state.reason.unwrap_or_default());
                return true;
            }
            if state.state == "ready" && frame_of_current(s).is_some() {
                check(s, "native-ready", true, state.info.clone().unwrap_or_default());
                let plat = super::platform_report();
                let wayland = plat["wayland"].as_bool().unwrap_or(false);
                let info = state.info.unwrap_or_default();
                if plat["native_window"] == "cametallayer-sublayer" {
                    // wgpu's occlusion walk reached OUR delegate (and stopped).
                    let off = plat["delegate_window_calls_off_main"].as_u64().unwrap_or(0);
                    check(s, "mac:occlusion-walk-stopped-at-our-delegate", off > 0, format!("off-main delegate `window` calls {off}"));
                } else if wayland {
                    check(s, "wayland:non-blocking-present", info.contains("Mailbox") || info.contains("Immediate"), &info);
                } else {
                    check(s, "x11:render-thread-own-connection", info.contains("own-connection"), &info);
                }
                let sc = current(s);
                let objs = sc.as_ref().map_or(0, |c| c.objects.len());
                let ns: Vec<String> = sc.as_ref().map(|c| c.not_shown.iter().map(|n| format!("{}:{}", n.node_type, n.reason)).collect()).unwrap_or_default();
                check(s, "projection:objects", objs == EXPECT.len(), objs);
                check(s, "projection:not-shown-listed", ns.iter().any(|n| n.starts_with("USE:")) && ns.iter().any(|n| n.starts_with("Group:")), ns.join(", "));
                // (Driver threads inherit the render thread's name, so the
                // host's own record is the count here.)
                let c = snapshot(|h| (h.c.created, h.c.destroyed, h.handle.is_some())).unwrap_or_default();
                check(s, "render-thread:one", c.0 == 1 && c.1 == 0 && c.2, format!("created {} destroyed {} live {}", c.0, c.1, c.2));
                // The environment's own main-loop delay with the native
                // viewport idle (recorded, not gated).
                set_phase(s, "baseline-idle");
                next(s, 9);
            } else if el > Duration::from_secs(30) {
                check(s, "native-ready", false, format!("state {} / no frame of the current projection after 30 s", state.state));
                return true;
            }
        }
        9 => {
            if el > Duration::from_secs(3) {
                set_phase(s, "picks");
                next(s, 10);
            }
        }
        // ---- picks through the WebView -----------------------------------
        10 => {
            let (Some(sc), Some(f)) = (current(s), frame_of_current(s)) else { return false };
            for (id, status, label) in EXPECT {
                if let Some(at) = center(&sc, &f, id) {
                    s.queue.push(Pending {
                        label: label.into(),
                        at,
                        want_status: vec![status],
                        want_reason: None,
                        want_id: Some(id),
                        sent_log: 0,
                        sent_select: 0,
                        expect_select: status == "PROVEN",
                    });
                }
            }
            match scan(&sc, &f, |a| a == Answer::Background, None) {
                Some(at) => s.queue.push(Pending { label: "background".into(), at, want_status: vec!["NO_HIT"], want_reason: None, want_id: None, sent_log: 0, sent_select: 0, expect_select: false }),
                None => check(s, "pick:background", false, "no background pixel"),
            }
            let a = center(&sc, &f, 1);
            match scan(&sc, &f, |x| x == Answer::Refused(oracle::reason::NEAR_EDGE), a) {
                Some(at) => s.queue.push(Pending {
                    label: "edge-band".into(),
                    at,
                    want_status: vec!["REFUSED_AMBIGUOUS"],
                    want_reason: Some(oracle::reason::NEAR_EDGE),
                    want_id: None,
                    sent_log: 0,
                    sent_select: 0,
                    expect_select: false,
                }),
                None => check(s, "pick:edge-band", false, "no edge pixel on the box row"),
            }
            s.queue.reverse();
            next(s, 11);
        }
        11 => {
            if s.pending.is_some() {
                settle(s);
            } else if let Some(pd) = s.queue.pop() {
                send(s, pd);
            } else {
                // Escape clears the WebView selection; the UI then asks for
                // an empty highlight.
                s.marks.insert("selects", snapshot(|h| h.selects.len()).unwrap_or(0) as u64);
                super::escape();
                next(s, 12);
            }
        }
        12 => {
            let from = s.marks.get("selects").copied().unwrap_or(0) as usize;
            let cleared = snapshot(|h| h.selects[from.min(h.selects.len())..].iter().any(|(_, ids)| ids.is_empty())).unwrap_or(false);
            if cleared || el > Duration::from_secs(3) {
                check(s, "escape-clears-ui-selection", cleared, "empty highlight requested after Escape");
                set_phase(s, "gpu-load");
                super::test_sweep(1, 6);
                s.marks.insert("sweeps", snapshot(|h| h.sweeps.len()).unwrap_or(0) as u64);
                next(s, 20);
            }
        }
        // ---- GPU/CPU agreement over the frame (same mesh) ------------------
        20 => {
            let from = s.marks.get("sweeps").copied().unwrap_or(0) as usize;
            let sw = snapshot(|h| h.sweeps.get(from).cloned()).flatten();
            if let Some(sw) = sw {
                let ok = sw.error.is_none() && sw.disagree == 0 && sw.agree_object > 100 && sw.agree_background > 100;
                check(s, "sweep:gpu-cpu-agree", ok, format!("{sw:?}"));
                let band = sw.refused_band as f64 / sw.samples.max(1) as f64;
                s.notes.insert("refusal_band_share".into(), json!(band));
                check(s, "sweep:band-is-thin", band < 0.08, format!("{:.2}% of samples refused by the band", band * 100.0));
                set_phase(s, "hidden");
                super::test_visible(false);
                next(s, 30);
            } else if el > Duration::from_secs(20) {
                check(s, "sweep:gpu-cpu-agree", false, "no sweep in 20 s");
                next(s, 30);
            }
        }
        // ---- hidden: no present, picks refused ---------------------------
        30 => {
            if el > Duration::from_millis(400) {
                let off = snapshot(|h| h.c.frames_offscreen).unwrap_or(0);
                check(s, "hidden:no-present", off > 0, format!("offscreen frames {off}"));
                let (Some(sc), Some(f)) = (current(s), snapshot(|h| h.last_frame).flatten()) else { return false };
                if let Some(at) = center(&sc, &f, 1) {
                    send(s, Pending { label: "while-hidden".into(), at, want_status: vec!["REFUSED_STALE"], want_reason: Some("frame-not-shown"), want_id: None, sent_log: 0, sent_select: 0, expect_select: false });
                }
                next(s, 31);
            }
        }
        31 => {
            if s.pending.is_some() {
                settle(s);
            } else {
                super::test_visible(true);
                set_phase(s, "fault");
                next(s, 40);
            }
        }
        // ---- forced GPU/CPU disagreement is always refused ----------------
        40 => {
            if el < Duration::from_millis(300) {
                return false;
            }
            super::test_fault(Fault::CorruptPick);
            let (Some(sc), Some(f)) = (current(s), frame_of_current(s)) else { return false };
            if let Some(at) = center(&sc, &f, 1) {
                send(s, Pending {
                    label: "forced-disagreement".into(),
                    at,
                    want_status: vec!["UNSUPPORTED"],
                    want_reason: Some(oracle::reason::DISAGREE),
                    want_id: None,
                    sent_log: 0,
                    sent_select: 0,
                    expect_select: false,
                });
            }
            next(s, 41);
        }
        41 => {
            if s.pending.is_some() {
                settle(s);
            } else {
                set_phase(s, "cleanup");
                next(s, 50);
            }
        }
        // ---- create/destroy cycles ----------------------------------------
        50 => {
            if s.cycle >= CYCLES {
                let c = snapshot(|h| h.c.clone()).unwrap_or_default();
                check(s, "cleanup:created==destroyed+1", c.created == c.destroyed + 1, format!("{} / {}", c.created, c.destroyed));
                check(s, "cleanup:threads-joined", c.joined == c.destroyed, format!("{} / {}", c.joined, c.destroyed));
                check(s, "cleanup:native-window-never-destroyed-before-Destroyed", c.unrealized_before_destroyed == 0, c.unrealized_before_destroyed);
                check(s, "cleanup:one-live-viewport", snapshot(|h| h.handle.is_some() && h.vp == Vp::Live).unwrap_or(false), format!("{vp:?}"));
                set_phase(s, "stale");
                next(s, 60);
                return false;
            }
            super::test_recreate();
            next(s, 51);
        }
        51 => {
            if vp == Vp::None {
                super::test_add();
                next(s, 52);
            } else if el > Duration::from_secs(5) {
                check(s, "cleanup:destroyed-in-time", false, format!("cycle {} vp {vp:?}", s.cycle));
                return true;
            }
        }
        52 => {
            if state.state == "ready" && frame_of_current(s).is_some_and(|_| snapshot(|h| h.vp == Vp::Live).unwrap_or(false)) && el > Duration::from_millis(200) {
                s.cycle += 1;
                next(s, 50);
            } else if el > Duration::from_secs(10) {
                check(s, "cleanup:recreated", false, format!("cycle {}", s.cycle));
                return true;
            }
        }
        // ---- stale: the text changed after the frame ---------------------
        60 => {
            let svc = s.app.state::<Service>();
            let Some(sc) = current(s) else { return false };
            let Some(f) = frame_of_current(s) else { return false };
            // An edit the frame does not show: an exact no-op pair (insert,
            // then undo) leaves the text but moves the revision.
            let len = svc.text(sc.session).map(|t| t.encode_utf16().count() as u64).unwrap_or(0);
            let r = svc.edit(&p::EditRequest { session: sc.session, base_revision: sc.revision, from: len, to: len, insert: " ".into(), item: None });
            let _ = svc.undo(sc.session);
            check(s, "stale:edit-applied", matches!(r, Ok(p::EditOutcome::Applied { .. })), format!("{r:?}"));
            if let Some(at) = center(&sc, &f, 1) {
                send(s, Pending { label: "after-edit".into(), at, want_status: vec!["REFUSED_STALE"], want_reason: Some(wrlforge_vrml::pick::reason::SOURCE_CHANGED), want_id: Some(1), sent_log: 0, sent_select: 0, expect_select: false });
            }
            next(s, 61);
        }
        61 => {
            if s.pending.is_some() {
                settle(s);
            } else {
                set_phase(s, "blocked-present");
                super::test_fault(Fault::BlockPresent(Duration::from_millis(1500)));
                super::reset_camera(); // forces a draw that holds its present
                next(s, 70);
            }
        }
        // ---- a present that does not return, then teardown ----------------
        70 => {
            if el > Duration::from_millis(300) {
                s.marks.insert("parked", snapshot(|h| h.c.parked).unwrap_or(0) as u64);
                super::test_recreate();
                next(s, 71);
            }
        }
        71 => {
            let parked = snapshot(|h| h.c.parked).unwrap_or(0) as u64 > s.marks.get("parked").copied().unwrap_or(0);
            if vp == Vp::None {
                check(s, "blocked:parked-then-destroyed", parked, format!("destroyed after {} ms", el.as_millis()));
                check(s, "blocked:destroyed-within-deadline", el < super::EXIT_DEADLINE, format!("{} ms", el.as_millis()));
                super::test_add();
                next(s, 72);
            } else if el > Duration::from_secs(8) {
                check(s, "blocked:destroyed-within-deadline", false, "not destroyed in 8 s");
                return true;
            }
        }
        72 => {
            if state.state == "ready" && frame_of_current(s).is_some() && snapshot(|h| h.vp == Vp::Live).unwrap_or(false) {
                set_phase(s, "device-loss");
                super::test_fault(Fault::DeviceLoss);
                next(s, 80);
            } else if el > Duration::from_secs(10) {
                check(s, "blocked:recreated", false, format!("state {}", state.state));
                return true;
            }
        }
        // ---- D9: device loss is reported, never restarted ------------------
        80 => {
            if state.state == "failed" && vp == Vp::None {
                let r = state.reason.clone().unwrap_or_default();
                check(s, "d9:device-loss-reported", r.contains("device was lost"), &r);
                let c = snapshot(|h| (h.c.created, h.c.destroyed, h.handle.is_some())).unwrap_or_default();
                check(s, "d9:no-restart", c.0 == c.1 && !c.2 && !state.native_shown(), format!("created {} destroyed {} live {}", c.0, c.1, c.2));
                s.marks.insert("left", 0);
                next(s, 81);
            } else if el > Duration::from_secs(5) {
                check(s, "d9:device-loss-reported", false, format!("state {} vp {vp:?}", state.state));
                return true;
            }
        }
        81 => {
            // Driver threads end with the device; allow them a moment.
            let left = render_threads();
            if left == 0 || el > Duration::from_secs(3) {
                check(s, "no-render-thread-left", left == 0, format!("{left} after {} ms", el.as_millis()));
                set_phase(s, "exit");
                return true;
            }
        }
        _ => return true,
    }
    false
}

fn pct(v: &[f64]) -> (f64, f64, usize) {
    if v.is_empty() {
        return (0.0, 0.0, 0);
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let i = ((s.len() as f64 * 0.99).ceil() as usize).saturating_sub(1).min(s.len() - 1);
    (s[i], s[s.len() - 1], s.len())
}

fn finish() {
    let Some(code) = with(|s| {
        s.finished = true;
        s.stop.store(true, Ordering::SeqCst);
        let watch = s.watch.lock().map(|g| g.clone()).unwrap_or_default();
        for (phase, v) in &watch {
            let (p99, max, n) = pct(v);
            let gated = GATED.contains(&phase.as_str());
            let pass = !gated || (p99 < WATCH_P99_MS && max < WATCH_MAX_MS);
            check(s, &format!("watchdog:{phase}{}", if gated { "" } else { " (recorded, not gated)" }), pass, format!("n={n} p99={p99:.2}ms max={max:.2}ms"));
        }
        let c = snapshot(|h| h.c.clone()).unwrap_or_default();
        let fails: Vec<Value> = s.checks.iter().filter(|c| c["pass"] != true).cloned().collect();
        let rep = json!({
            "pass": fails.is_empty(),
            "checks_total": s.checks.len(),
            "failures": fails,
            "fifo_only_run": s.fifo_only,
            "control_run": s.control,
            "platform": super::platform_report(),
            "state": snapshot(|h| json!({"state": h.state.state, "reason": h.state.reason, "info": h.state.info})),
            "counters": {
                "created": c.created, "destroyed": c.destroyed, "joined": c.joined, "parked": c.parked,
                "unrealized_before_destroyed": c.unrealized_before_destroyed,
                "frames_presented": c.frames_presented, "frames_offscreen": c.frames_offscreen,
                "picks_sent": c.picks_sent, "picks_busy": c.picks_busy, "picks_answered": c.picks_answered,
                "last_render_thread_stats": c.last_stats,
            },
            "notes": s.notes,
            "elapsed_ms": s.t0.elapsed().as_millis() as u64,
            "checks": s.checks,
        });
        let text = serde_json::to_string_pretty(&rep).unwrap_or_default();
        let _ = std::fs::write(&s.report, &text);
        write_trace(s);
        println!("{text}");
        if rep["pass"] == true { 0 } else { 1 }
    }) else {
        return;
    };
    // Steps end the run only from a tick: stop ticking by exiting. A live
    // viewport defers the exit until its render thread is gone.
    S.with(|s| {
        if let Some(app) = s.borrow().as_ref().map(|x| x.app.clone()) {
            app.exit(code);
        }
    });
}
