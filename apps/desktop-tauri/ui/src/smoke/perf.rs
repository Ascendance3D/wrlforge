// SPDX-License-Identifier: GPL-3.0-or-later
//! Viewport frame-timing run (`--smoke-perf`). Opens the armed model copy
//! and records animation-frame statistics in five phases: idle, orbit (a
//! REAL X drag), selection (real clicks in Select mode), camera commands
//! and Inspector edits of a Material color. The steps pass when each phase
//! produced measurements; the numbers are the evidence (GPU vs software is
//! compared by running the suite twice). Nothing is saved.

use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::command::CommandId;

use super::create::{all, apply_field, wait_ms};
use super::pick::{calibrate, gesture, viewport, REAL};
use super::R;
use crate::editor::CORE;
use crate::ipc::{self, call};
use crate::ui::ui;

/// The plan Rust sends (`smoke_perf_plan`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub real_pointer: bool,
}

fn adapter(name: &str) -> Option<JsValue> {
    let w = web_sys::window()?;
    let o = js_sys::Reflect::get(&w, &"wrlforgePreview".into()).ok()?;
    let f = js_sys::Reflect::get(&o, &name.into())
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    f.call0(&o).ok()
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn start() {
    let _ = adapter("frameStatsStart");
}

fn stop() -> String {
    adapter("frameStatsStop")
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

fn measured(stats: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stats)
        .ok()
        .and_then(|v| v.get("frames").and_then(|f| f.as_u64()))
        .is_some_and(|f| f > 2)
}

/// The preview finished loading the CURRENT revision.
async fn loaded(ms: u32) -> Option<f64> {
    let t0 = now();
    wait_ms(ms, || {
        let rev = CORE.with_borrow(|c| c.revision);
        ui().preview_status
            .get_untracked()
            .starts_with(&format!("rev {rev} · loaded"))
            .then_some(())
    })
    .await?;
    Some(now() - t0)
}

async fn pointer(action: &str, x: f64, y: f64) -> Result<String, String> {
    #[derive(serde::Serialize)]
    struct A<'a> {
        action: &'a str,
        x: f64,
        y: f64,
    }
    let (dx, dy) = REAL.get().unwrap_or_default();
    call::<String>(
        "smoke_real_pointer",
        A {
            action,
            x: x - dx,
            y: y - dy,
        },
    )
    .await
}

fn pct(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f64 * q).round() as usize]
}

pub(super) async fn run(plan: &Plan, r: &mut R) -> Option<()> {
    #[derive(serde::Serialize)]
    struct A {
        index: usize,
    }
    let t0 = now();
    let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: 0 })
        .await
        .ok()?;
    crate::ui::apply_open(o, "Opened").await;
    let load = loaded(120_000).await;
    r.step(
        "perf: the model copy opens and the preview loads",
        load.is_some(),
        format!("{:.0} ms open → loaded · {}", now() - t0, ui().preview_status.get_untracked()),
    );
    load?;
    ipc::sleep(1500).await;

    let gl = adapter("glInfo").and_then(|v| v.as_string()).unwrap_or_default();
    let probe = adapter("probe").and_then(|v| v.as_string()).unwrap_or_default();
    r.step(
        "perf: the viewport's X_ITE WebGL context reports its renderer",
        !gl.is_empty(),
        format!("x_ite context {gl} · new canvas {probe}"),
    );

    start();
    ipc::sleep(3000).await;
    let s = stop();
    r.step("perf: idle 3 s", measured(&s), s);

    // Real X input: calibrate in Select mode (a click selects; an empty
    // drag orbits in either mode).
    ui().pick_mode.set(true);
    if plan.real_pointer {
        calibrate(r).await?;
    }
    let rect = viewport()?.get_bounding_client_rect();
    let (cx, cy) = (
        rect.left() + rect.width() / 2.0,
        rect.top() + rect.height() * 0.85,
    );
    let real = REAL.get().is_some();
    if real {
        // Orbit: press below the model (background), drag 540 px in 90
        // steps, release.
        start();
        let t = now();
        let mut ok = pointer("down", cx - 270.0, cy).await.is_ok();
        for i in 1..=90 {
            ok &= pointer("move", cx - 270.0 + 6.0 * i as f64, cy).await.is_ok();
            ipc::sleep(16).await;
        }
        ok &= pointer("up", cx + 270.0, cy).await.is_ok();
        let dt = now() - t;
        ipc::sleep(300).await;
        let s = stop();
        r.step(
            "perf: orbit (real X drag, 90 steps)",
            ok && measured(&s),
            format!("{dt:.0} ms · {s}"),
        );
    } else {
        r.step("perf: orbit (real X drag)", false, "no real X pointer in this run");
    }

    // Selection: real clicks over the model in Select mode.
    let pts = [
        (0.0, 0.0),
        (-0.15, 0.05),
        (0.15, 0.05),
        (0.0, -0.1),
        (-0.25, 0.1),
        (0.25, 0.1),
        (0.05, 0.15),
        (-0.05, -0.05),
    ];
    let mut lat = vec![];
    let mut hits = 0;
    start();
    for (fx, fy) in pts {
        let (x, y) = (
            rect.left() + rect.width() * (0.5 + fx),
            rect.top() + rect.height() * (0.5 + fy),
        );
        let t = now();
        let out = gesture(x, y).await;
        lat.push(now() - t - if real { super::pick::REAL_CLICK_GAP_MS as f64 } else { 0.0 });
        if out.is_some_and(|o| o.status == "PROVEN" && o.item.is_some()) {
            hits += 1;
        }
    }
    let s = stop();
    r.step(
        "perf: selection (8 clicks in Select mode)",
        measured(&s) && lat.len() == pts.len(),
        format!(
            "{hits}/{} selected · pick latency p50 {:.0} ms p95 {:.0} ms max {:.0} ms · {s}",
            pts.len(),
            pct(&lat, 0.5),
            pct(&lat, 0.95),
            pct(&lat, 1.0)
        ),
    );
    ui().pick_mode.set(false);

    // Camera commands.
    start();
    let cmds = [
        CommandId::ViewFitAll,
        CommandId::ViewFront,
        CommandId::ViewTop,
        CommandId::ViewLeft,
        CommandId::ViewRight,
        CommandId::ViewBack,
        CommandId::ViewBottom,
        CommandId::ViewResetCamera,
    ];
    for c in cmds {
        crate::command::run(c);
        ipc::sleep(500).await;
    }
    let s = stop();
    r.step("perf: camera (Fit + 6 views + Reset)", measured(&s), s);

    // Inspector edits of the first Material's diffuseColor (a shared PROTO
    // Material on the Chevy; the run's consent answer is Edit).
    let row = all(".tree-item")
        .into_iter()
        .find(|e| e.text_content().unwrap_or_default().trim_start_matches(['•', ' ']).starts_with("Material"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    let Some(row) = row else {
        r.step("perf: a Material row exists in the Scene Tree", false, "");
        return Some(());
    };
    row.click();
    wait_ms(5000, || (!all("tr[data-field=\"diffuseColor\"] input[type=\"text\"]").is_empty()).then_some(())).await;
    let mut edit = vec![];
    let mut reload = vec![];
    let mut ok = true;
    start();
    for v in ["0.9 0.1 0.1", "0.1 0.9 0.1", "0.1 0.1 0.9", "0.9 0.8 0.2"] {
        let vals: Vec<String> = v.split(' ').map(str::to_string).collect();
        let t = now();
        ok &= apply_field("diffuseColor", &vals).await.is_some();
        edit.push(now() - t);
        match loaded(30_000).await {
            Some(ms) => reload.push(ms + edit.last().copied().unwrap_or(0.0)),
            None => ok = false,
        }
    }
    let s = stop();
    r.step(
        "perf: 4 Material color edits (edit → preview reloaded)",
        ok && measured(&s),
        format!(
            "edit p50 {:.0} ms max {:.0} ms · edit→reloaded p50 {:.0} ms max {:.0} ms · {s}",
            pct(&edit, 0.5),
            pct(&edit, 1.0),
            pct(&reload, 0.5),
            pct(&reload, 1.0)
        ),
    );
    Some(())
}
