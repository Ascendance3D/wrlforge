// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-4B in-window Rotate / Scale run (`--smoke-rotscale`).
//!
//! Every drag is REAL X pointer input inside the harness's own Xvfb (the
//! VISUAL-3A rule): press on the drawn ring or handle, motion with the
//! button held, release. Mid-drag the run proves that the RENDERED frame
//! changed while the source, revision, dirty flag and history did not; after
//! each release it proves one revision, one changed source line, the
//! intended rotation axis or scale component, the kept camera and the kept
//! selection. Rust then checks every saved file on disk
//! (`src-tauri/src/smoke.rs`, `verify_rotscale`).

use leptos::prelude::*;
use web_sys::HtmlElement;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::gizmo::{self as gz, Axis, Tool};

use super::create::{previewed, text, wait_ms};
use super::gizmo::{class_of, new_world_with_box, orbit, real, source, still_camera, tree_click};
use super::pick::{calibrate, gesture, ready, viewport, REAL};
use super::{click, R};
use crate::editor::CORE;
use crate::gizmo::{Target, GIZMO};
use crate::ipc::{self, call};
use crate::ui::ui;

/// The largest view-matrix element change a commit may cause.
const CAM_TOL: f64 = 1e-5;

fn pressed(id: &str) -> bool {
    crate::element_by_id::<HtmlElement>(id)
        .and_then(|b| b.get_attribute("aria-pressed"))
        .as_deref()
        == Some("true")
}

fn svg_attr(k: &str) -> Option<String> {
    crate::element_by_id::<web_sys::Element>("gizmo").and_then(|e| e.get_attribute(k))
}

fn gizmo_msg() -> String {
    crate::element_by_id::<HtmlElement>("gizmo-status")
        .and_then(|e| e.text_content())
        .unwrap_or_default()
}

/// The handles of `tool` are drawn, usable and bound to the CURRENT
/// selection, revision and preview generation.
async fn tool_ready(tool: Tool, ms: u32) -> Option<Target> {
    wait_ms(ms, || {
        let t = GIZMO.with_borrow(|g| {
            let drawn = match tool {
                Tool::Rotate => g.rings.is_some(),
                _ => g.layout.is_some(),
            };
            (g.drag.is_none() && !g.committing && drawn)
                .then(|| g.target.clone())
                .flatten()
        })?;
        let sel = ui().selected.get_untracked()?;
        let gen = crate::pick::active()?;
        let rev = CORE.with_borrow(|c| c.revision);
        (t.tool == tool
            && t.item == sel.id
            && t.revision == sel.revision
            && t.revision == rev
            && t.seq == gen.seq
            && svg_attr("data-state").as_deref() == Some("ready")
            && svg_attr("data-tool").as_deref()
                == Some(if tool == Tool::Rotate {
                    "rotate"
                } else {
                    "scale"
                }))
        .then_some(t)
    })
    .await
}

/// The source lines that differ (same line count required).
fn changed_lines(a: &str, b: &str) -> Option<Vec<String>> {
    let (la, lb): (Vec<_>, Vec<_>) = (a.split('\n').collect(), b.split('\n').collect());
    (la.len() == lb.len()).then(|| {
        la.iter()
            .zip(&lb)
            .filter(|(x, y)| x != y)
            .map(|(_, y)| y.trim_start().to_string())
            .collect()
    })
}

fn mat_mul(a: &gz::M3, b: &gz::M3) -> gz::M3 {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn transpose(m: &gz::M3) -> gz::M3 {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| m[c][r]))
}

/// The relative rotation `new · old⁻¹` as (unit axis, angle).
fn relative(old: [f64; 4], new: [f64; 4]) -> Option<([f64; 3], f64)> {
    let m = mat_mul(&gz::rot_matrix(new)?, &transpose(&gz::rot_matrix(old)?));
    let tr = m[0][0] + m[1][1] + m[2][2];
    let angle = ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos();
    let v = [m[2][1] - m[1][2], m[0][2] - m[2][0], m[1][0] - m[0][1]];
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (l > 1e-9).then(|| (v.map(|x| x / l), angle))
}

fn view_delta(a: &[f64; 16], b: &[f64; 16]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

/// The ring of `axis` as drawn: client points, and the index farthest from
/// the pivot (where the ring's tangent is clearly visible).
fn ring_path(axis: Axis) -> Option<(Vec<(f64, f64)>, usize)> {
    let rl = GIZMO.with_borrow(|g| g.rings.clone())?;
    let ring = &rl.rings[axis.index()];
    if !ring.enabled {
        return None;
    }
    let pts: Vec<(f64, f64)> = ring.points.iter().map(|p| p.unwrap()).collect();
    let o = rl.origin;
    let k = (0..pts.len() - 1).max_by(|a, b| {
        let da = (pts[*a].0 - o.0).hypot(pts[*a].1 - o.1);
        let db = (pts[*b].0 - o.0).hypot(pts[*b].1 - o.1);
        da.total_cmp(&db)
    })?;
    Some((pts, k))
}

/// Press on the ring of `axis` and follow it for `steps` segments (each
/// 360/64 degrees), without releasing. Returns the last pointer position.
async fn ring_press_and_follow(axis: Axis, steps: usize) -> Option<(f64, f64)> {
    let (pts, k) = ring_path(axis)?;
    let n = pts.len() - 1;
    real("down", pts[k].0, pts[k].1).await.ok()?;
    ipc::sleep(60).await;
    let mut at = pts[k];
    for s in 1..=steps {
        at = pts[(k + s) % n];
        real("move", at.0, at.1).await.ok()?;
        ipc::sleep(40).await;
    }
    ipc::sleep(80).await;
    Some(at)
}

/// (grab point, screen direction) of the scale handle of `axis`.
fn handle_grab(axis: Axis) -> Option<((f64, f64), (f64, f64))> {
    let l = GIZMO.with_borrow(|g| g.layout)?;
    let h = l.handles[axis.index()];
    if !h.enabled {
        return None;
    }
    let tip = h.tip?;
    let (dx, dy) = (tip.0 - l.origin.0, tip.1 - l.origin.1);
    let len = dx.hypot(dy);
    Some((
        (l.origin.0 + dx * 0.8, l.origin.1 + dy * 0.8),
        (dx / len, dy / len),
    ))
}

/// Press at `from`, move by (`dx`, `dy`) in `steps`, without releasing.
async fn press_and_move(from: (f64, f64), dx: f64, dy: f64, steps: u32) -> Option<(f64, f64)> {
    real("down", from.0, from.1).await.ok()?;
    ipc::sleep(60).await;
    let mut at = from;
    for k in 1..=steps {
        let f = k as f64 / steps as f64;
        at = (from.0 + dx * f, from.1 + dy * f);
        real("move", at.0, at.1).await.ok()?;
        ipc::sleep(40).await;
    }
    ipc::sleep(80).await;
    Some(at)
}

/// Release at `at`, wait for the commit and the re-bound handles.
async fn release_commit(tool: Tool, at: (f64, f64), commits: u64) -> Option<Target> {
    let before_rev = CORE.with_borrow(|c| c.revision);
    real("up", at.0, at.1).await.ok()?;
    wait_ms(5000, || {
        (GIZMO.with_borrow(|g| g.commits) > commits).then_some(())
    })
    .await?;
    let rev = wait_ms(5000, || {
        let r = CORE.with_borrow(|c| c.revision);
        (r > before_rev).then_some(r)
    })
    .await?;
    let loaded = format!("rev {rev} · loaded:");
    if wait_ms(20_000, || {
        ui().preview_status
            .get_untracked()
            .starts_with(&loaded)
            .then_some(())
    })
    .await
    .is_none()
    {
        LAST_WAIT.with_borrow_mut(|w| {
            *w = format!(
                "preview of rev {rev} timed out: {}",
                ui().preview_status.get_untracked()
            )
        });
        return None;
    }
    let t = tool_ready(tool, 10_000).await;
    if t.is_none() {
        let why = GIZMO.with_borrow(|g| {
            format!(
                "target {:?} rings {} layout {} drag {} committing {}",
                g.target
                    .as_ref()
                    .map(|t| (t.tool, t.item.clone(), t.revision, t.seq)),
                g.rings.is_some(),
                g.layout.is_some(),
                g.drag.is_some(),
                g.committing
            )
        });
        let sel = ui().selected.get_untracked().map(|s| (s.id, s.revision));
        let gen = crate::pick::active().map(|g| g.seq);
        LAST_WAIT.with_borrow_mut(|w| {
            *w = format!(
                "{why} · sel {sel:?} · gen {gen:?} · rev {} · svg {:?}/{:?}",
                CORE.with_borrow(|c| c.revision),
                svg_attr("data-state"),
                svg_attr("data-tool")
            )
        });
    }
    t
}

thread_local! {
    static LAST_WAIT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// One real rotation drag of `steps` ring segments about `axis`, checked
/// mid-drag and after the commit. Returns the committed text.
async fn rotate_drag(tag: &str, axis: Axis, steps: usize, r: &mut R) -> Option<String> {
    let t0 = tool_ready(Tool::Rotate, 10_000).await?;
    still_camera().await?;
    let before = source().await?;
    let commits = GIZMO.with_borrow(|g| g.commits);
    let at = ring_press_and_follow(axis, steps).await?;
    let mid = crate::gizmo::rendered_frame();
    let during = source().await?;
    r.step(
        &format!("{tag}: mid-drag about {}: the rendered rotation changed; source, revision, dirty flag and history did not", axis.label()),
        mid.is_some_and(|f| !gz::same_rotation(f.rotation, t0.rotation, 1e-4))
            && during == before
            && GIZMO.with_borrow(|g| g.drag.is_some()),
        format!("{:?} → {:?}", t0.rotation, mid.map(|f| f.rotation)),
    );
    let view0 = ipc::gizmo_camera()?.view;
    let t1 = release_commit(Tool::Rotate, at, commits).await;
    let after = source().await?;
    let view1 = ipc::gizmo_camera().map(|c| c.view);
    let lines = changed_lines(&before.text, &after.text);
    let rel = t1.as_ref().and_then(|t| relative(t0.rotation, t.rotation));
    let u = axis.unit();
    let about_axis = rel.is_some_and(|(a, ang)| {
        (a[0] * u[0] + a[1] * u[1] + a[2] * u[2]).abs() > 0.999 && ang > 0.2 && ang < 1.6
    });
    let one_line = lines.as_ref().is_some_and(|l| {
        l.len() == 1 && (l[0].starts_with("rotation ") || l[0].contains(" rotation "))
    });
    r.step(
        &format!("{tag}: release about {}: one revision, only the rotation line changed, the new rotation is Rot({}, δ)·R", axis.label(), axis.label()),
        t1.is_some() && after.revision == before.revision + 1 && after.can_undo && one_line && about_axis,
        format!("{lines:?} · rel {rel:?} · {}", LAST_WAIT.with_borrow(|w| w.clone())),
    );
    let sel_ok = t1.as_ref().is_some_and(|t| t.def == t0.def)
        && ui()
            .inspection
            .get_untracked()
            .is_some_and(|i| i.title.ends_with(&t0.def) && i.revision == after.revision);
    r.step(
        &format!(
            "{tag}: the camera is kept (max Δ ≤ {CAM_TOL}) and {} stays selected",
            t0.def
        ),
        view1.is_some_and(|v| view_delta(&view0, &v) <= CAM_TOL) && sel_ok,
        format!("Δ {:?}", view1.map(|v| view_delta(&view0, &v))),
    );
    Some(after.text)
}

/// One real scale drag (`axis` handle, or the uniform square for `None`).
async fn scale_drag(tag: &str, axis: Option<Axis>, r: &mut R) -> Option<String> {
    let t0 = tool_ready(Tool::Scale, 10_000).await?;
    still_camera().await?;
    let before = source().await?;
    let commits = GIZMO.with_borrow(|g| g.commits);
    let (from, dx, dy) = match axis {
        Some(a) => {
            let (g, d) = handle_grab(a)?;
            (g, d.0 * 45.0, d.1 * 45.0)
        }
        None => {
            let o = GIZMO.with_borrow(|g| g.layout.map(|l| l.origin))?;
            (o, 0.0, -45.0)
        }
    };
    let at = press_and_move(from, dx, dy, 6).await?;
    let mid = crate::gizmo::rendered_frame();
    let during = source().await?;
    let name = axis.map_or("uniform".to_string(), |a| a.label().to_string());
    r.step(
        &format!(
            "{tag}: mid-drag scale {name}: the rendered scale grew; the source did not change"
        ),
        mid.is_some_and(|f| match axis {
            Some(a) => f.scale[a.index()] > t0.scale[a.index()] * 1.05,
            None => (0..3).all(|i| f.scale[i] > t0.scale[i] * 1.05),
        }) && during == before,
        format!("{:?} → {:?}", t0.scale, mid.map(|f| f.scale)),
    );
    let view0 = ipc::gizmo_camera()?.view;
    let t1 = release_commit(Tool::Scale, at, commits).await;
    let after = source().await?;
    let view1 = ipc::gizmo_camera().map(|c| c.view);
    let lines = changed_lines(&before.text, &after.text);
    let one_line = lines
        .as_ref()
        .is_some_and(|l| l.len() == 1 && l[0].starts_with("scale "));
    let comps_ok = t1.as_ref().is_some_and(|t| match axis {
        Some(a) => (0..3).all(|i| {
            if i == a.index() {
                t.scale[i] > t0.scale[i] * 1.05
            } else {
                t.scale[i] == t0.scale[i]
            }
        }),
        None => {
            let f: Vec<f64> = (0..3).map(|i| t.scale[i] / t0.scale[i]).collect();
            f[0] > 1.05 && f.iter().all(|x| (x - f[0]).abs() <= 2e-3 * f[0])
        }
    });
    r.step(
        &format!("{tag}: release scale {name}: one revision, only the scale line changed, only the intended component(s), camera kept"),
        t1.is_some()
            && after.revision == before.revision + 1
            && one_line
            && comps_ok
            && view1.is_some_and(|v| view_delta(&view0, &v) <= CAM_TOL),
        format!("{lines:?} · {:?} → {:?}", t0.scale, t1.map(|t| t.scale)),
    );
    Some(after.text)
}

/// Escape mid-drag: the rendered frame returns, nothing commits.
async fn escape_cancels(tag: &str, tool: Tool, r: &mut R) -> Option<()> {
    let t0 = tool_ready(tool, 10_000).await?;
    let before = source().await?;
    let cancels = GIZMO.with_borrow(|g| g.cancels);
    let at = match tool {
        Tool::Rotate => {
            let axis = Axis::ALL.into_iter().find(|a| ring_path(*a).is_some())?;
            ring_press_and_follow(axis, 6).await?
        }
        _ => {
            let (g, d) = handle_grab(Axis::ALL.into_iter().find(|a| handle_grab(*a).is_some())?)?;
            press_and_move(g, d.0 * 40.0, d.1 * 40.0, 5).await?
        }
    };
    let moved = crate::gizmo::rendered_frame();
    real("escape", at.0, at.1).await.ok()?;
    let canceled = wait_ms(2000, || {
        (GIZMO.with_borrow(|g| g.cancels) > cancels).then_some(())
    })
    .await;
    ipc::sleep(150).await;
    let back = crate::gizmo::rendered_frame();
    real("up", at.0, at.1).await.ok()?;
    ipc::sleep(300).await;
    let after = source().await?;
    let same = |f: &ipc::RenderedFrame| {
        gz::same_rotation(f.rotation, t0.rotation, 1e-5)
            && (0..3).all(|i| (f.scale[i] - t0.scale[i]).abs() <= 1e-5 * t0.scale[i].abs().max(1.0))
    };
    r.step(
        &format!("{tag}: Escape cancels a {} drag: the rendered frame returns, the release commits nothing", tool.label()),
        canceled.is_some()
            && moved.as_ref().is_some_and(|f| !same(f))
            && back.as_ref().is_some_and(same)
            && after == before
            && GIZMO.with_borrow(|g| g.last_cancel.as_deref() == Some("Escape")),
        format!("{:?}", GIZMO.with_borrow(|g| g.last_cancel.clone())),
    );
    Some(())
}

/// A drag is canceled when the revision or the preview generation moves
/// under it; nothing commits.
async fn stale_cancels(tag: &str, r: &mut R) -> Option<()> {
    let u = ui();
    // 1. The revision changes mid-drag (Undo), then Redo restores it.
    tool_ready(Tool::Rotate, 10_000).await?;
    let before = source().await?;
    let commits = GIZMO.with_borrow(|g| g.commits);
    let axis = Axis::ALL.into_iter().find(|a| ring_path(*a).is_some())?;
    let at = ring_press_and_follow(axis, 5).await?;
    let cancels = GIZMO.with_borrow(|g| g.cancels);
    let dragging = GIZMO.with_borrow(|g| g.drag.is_some());
    click("btn-undo")?;
    let canceled = wait_ms(3000, || {
        GIZMO
            .with_borrow(|g| (g.cancels > cancels).then(|| g.last_cancel.clone()))
            .flatten()
    })
    .await
    .filter(|_| dragging);
    real("up", at.0, at.1).await.ok()?;
    ipc::sleep(400).await;
    let undone = source().await?;
    click("btn-redo")?;
    wait_ms(5000, || {
        (CORE.with_borrow(|c| c.revision) > undone.revision).then_some(())
    })
    .await?;
    let redone = text();
    r.step(
        &format!("{tag}: a revision change (Undo) mid-drag cancels it; the release commits nothing; Redo restores the exact text"),
        canceled.is_some() && GIZMO.with_borrow(|g| g.commits) == commits && redone == before.text,
        format!("{canceled:?} · last {:?} · cancels {} → {}", GIZMO.with_borrow(|g| g.last_cancel.clone()), cancels, GIZMO.with_borrow(|g| g.cancels)),
    );
    // 2. The preview generation changes mid-drag (forced reload).
    let rev = CORE.with_borrow(|c| c.revision);
    previewed(rev, 1, 20_000).await;
    tool_ready(Tool::Rotate, 10_000).await?;
    let before = source().await?;
    let at = ring_press_and_follow(axis, 5).await?;
    let loads = u.preview_loads.get_untracked();
    leptos::task::spawn_local(crate::ui::preview(true));
    let canceled = wait_ms(5000, || {
        GIZMO
            .with_borrow(|g| g.last_cancel.clone())
            .filter(|c| c == "the preview was reloaded")
    })
    .await;
    real("up", at.0, at.1).await.ok()?;
    wait_ms(10_000, || {
        (u.preview_loads.get_untracked() > loads).then_some(())
    })
    .await;
    ipc::sleep(400).await;
    let after = source().await?;
    r.step(
        &format!("{tag}: a new preview generation mid-drag cancels it; nothing commits"),
        canceled.is_some() && after == before && GIZMO.with_borrow(|g| g.commits) == commits,
        format!("{canceled:?}"),
    );
    Some(())
}

/// Select the object at world `p` with a REAL click (Select tool).
async fn real_select(tag: &str, name: &str, p: [f64; 3], r: &mut R) -> Option<()> {
    let u = ui();
    if !u.pick_mode.get_untracked() {
        click("btn-select")?;
    }
    ready(10_000).await?;
    still_camera().await?;
    let cam = ipc::gizmo_camera()?;
    let (x, y) = gz::project(&cam, p)?;
    let before = source().await?;
    let out = gesture(x, y).await;
    let want = format!("Transform {name}");
    let item = u.analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.label == want)
            .map(|i| i.id.clone())
    });
    let after = source().await?;
    r.step(
        &format!("{tag}: a real viewport click selects exactly {want} (source untouched)"),
        out.as_ref()
            .is_some_and(|o| o.is_proven() && o.item == item)
            && after == before,
        format!("{:?}", out.map(|o| (o.status, o.reason))),
    );
    Some(())
}

fn tool_on(tool: Tool) -> Option<()> {
    let (id, on) = match tool {
        Tool::Rotate => ("btn-rotate", ui().rotate_mode.get_untracked()),
        Tool::Scale => ("btn-scale", ui().scale_mode.get_untracked()),
        Tool::Move => ("btn-move", ui().move_mode.get_untracked()),
    };
    if !on {
        click(id)?;
    }
    Some(())
}

async fn new_world(r: &mut R) -> Option<()> {
    let u = ui();
    let tag = "[new world]";
    new_world_with_box(tag, r).await?;
    // The experimental native renderer stays off: X_ITE draws the viewport.
    let native_off = !u.native.with_untracked(|s| s.requested || s.native_shown());
    r.step(
        "the native (wgpu) renderer is disabled by default; X_ITE renders the viewport",
        native_off && viewport().is_some() && u.preview_status.get_untracked().contains("loaded"),
        u.preview_status.get_untracked(),
    );
    click("btn-select")?;
    ready(5000).await?;
    calibrate(r).await?;
    if REAL.get().is_none() {
        r.step(
            "real pointer input is available for the Rotate / Scale run",
            false,
            "",
        );
        return None;
    }
    real_select(tag, "Box_1", [0.0; 3], r).await?;

    // Rotate: the tool button is exclusive; rings show at the pivot.
    click("btn-rotate")?;
    let t = tool_ready(Tool::Rotate, 8000).await;
    let rings = GIZMO.with_borrow(|g| g.rings.clone());
    r.step(
        &format!("{tag}: Rotate on (Select, Move, Scale off); rings at Box_1's pivot; front view: X and Y rings edge-on (disabled), Z usable"),
        pressed("btn-rotate")
            && !pressed("btn-select")
            && !pressed("btn-move")
            && !pressed("btn-scale")
            && t.as_ref().is_some_and(|t| t.pivot == [0.0; 3])
            && rings.as_ref().is_some_and(|r| {
                !r.rings[0].enabled && !r.rings[1].enabled && r.rings[2].enabled
            }),
        format!("{:?}", rings.map(|r| r.rings.map(|x| x.enabled))),
    );
    // An oblique camera (real orbit) makes every ring usable.
    orbit().await?;
    let t = tool_ready(Tool::Rotate, 8000).await;
    let all_on = GIZMO.with_borrow(|g| g.rings.as_ref().map(|r| r.rings.iter().all(|x| x.enabled)));
    r.step(
        &format!("{tag}: after a real orbit all three rings are usable"),
        t.is_some() && all_on == Some(true),
        format!("{all_on:?}"),
    );
    let mut texts = vec![text()];
    for axis in [Axis::Y, Axis::X, Axis::Z] {
        texts.push(rotate_drag(tag, axis, 8, r).await?);
    }
    escape_cancels(tag, Tool::Rotate, r).await?;
    stale_cancels(tag, r).await?;

    // Scale: axis handles follow the rotated object's frame.
    click("btn-scale")?;
    let t = tool_ready(Tool::Scale, 8000).await;
    r.step(
        &format!("{tag}: Scale on (Rotate off); handles along the rotated scale axes, a uniform square at the pivot"),
        pressed("btn-scale")
            && !pressed("btn-rotate")
            && t.is_some()
            && crate::element_by_id::<web_sys::Element>("gz-u")
                .and_then(|e| e.get_attribute("aria-disabled"))
                .as_deref()
                == Some("false"),
        gizmo_msg(),
    );
    for axis in Axis::ALL {
        if handle_grab(axis).is_none() {
            // This view looks (nearly) along that scale axis: the handle is
            // drawn disabled and takes no drag (the fixtures drag it).
            r.step(
                &format!("{tag}: the scale {} handle points at the viewer: drawn disabled, no drag possible", axis.label()),
                class_of(&format!("gz-{}", axis.label().to_lowercase())).contains("disabled"),
                "",
            );
            continue;
        }
        texts.push(scale_drag(tag, Some(axis), r).await?);
    }
    texts.push(scale_drag(tag, None, r).await?);
    escape_cancels(tag, Tool::Scale, r).await?;

    // One undo step per completed drag; exact text both ways.
    let n = texts.len() - 1;
    let mut undo_ok = true;
    for want in texts.iter().rev().skip(1) {
        let rev = CORE.with_borrow(|c| c.revision);
        click("btn-undo")?;
        wait_ms(5000, || {
            (CORE.with_borrow(|c| c.revision) != rev).then_some(())
        })
        .await?;
        undo_ok &= text() == *want;
    }
    let mut redo_ok = true;
    for want in texts.iter().skip(1) {
        let rev = CORE.with_borrow(|c| c.revision);
        click("btn-redo")?;
        wait_ms(5000, || {
            (CORE.with_borrow(|c| c.revision) != rev).then_some(())
        })
        .await?;
        redo_ok &= text() == *want;
    }
    r.step(
        &format!("{tag}: {n} drags = {n} undo steps; each Undo restores the exact previous source, each Redo the exact committed source"),
        undo_ok && redo_ok,
        format!("undo {undo_ok} redo {redo_ok}"),
    );

    // Save As → Close → Open: the same source and the same frame.
    let saved = text();
    let frame = tool_ready(Tool::Scale, 10_000)
        .await
        .map(|t| (t.rotation, t.scale));
    click("btn-save-as")?;
    wait_ms(5000, || (!u.dirty.get_untracked()).then_some(())).await?;
    click("btn-close")?;
    wait_ms(5000, || u.doc.get_untracked().is_none().then_some(())).await?;
    click("btn-open")?;
    let reopened = wait_ms(8000, || u.doc.get_untracked().filter(|d| !d.untitled)).await;
    let rev = CORE.with_borrow(|c| c.revision);
    previewed(rev, 1, 20_000).await?;
    r.step(
        &format!("{tag}: Save As → Close → Open shows exactly the saved source"),
        reopened.is_some() && text() == saved,
        format!("{:?}", reopened.map(|d| d.name)),
    );
    // Selection after reopen by a real click, then the same frame.
    real_select(&format!("{tag} reopened"), "Box_1", [0.0; 3], r).await?;
    tool_on(Tool::Scale)?;
    let again = tool_ready(Tool::Scale, 8000)
        .await
        .map(|t| (t.rotation, t.scale));
    r.step(
        &format!("{tag}: after reopening, the Scale tool binds the saved rotation and scale"),
        again.is_some() && again == frame,
        format!("{again:?}"),
    );
    Some(())
}

/// Open a Rust-written fixture by index and wait for its preview.
async fn open_fixture(i: usize, tag: &str, r: &mut R) -> Option<()> {
    let u = ui();
    #[derive(serde::Serialize)]
    struct A {
        index: usize,
    }
    let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: i })
        .await
        .ok()?;
    crate::ui::apply_open(o, "Opened").await;
    let rev = CORE.with_borrow(|c| c.revision);
    let shown = previewed(rev, 5, 20_000).await;
    r.step(
        &format!("{tag}: opened and previewed"),
        shown.is_some(),
        u.preview_status.get_untracked(),
    );
    Some(())
}

/// Select a Scene Tree item until the gizmo answers for it.
async fn tree_select_for(label: &str, until: impl Fn() -> bool) -> bool {
    tree_click(label, until).await
}

async fn fixtures(c: &p::RotScaleSmoke, r: &mut R) -> Option<()> {
    let u = ui();
    for (i, label) in c.fixtures.iter().enumerate() {
        let tag = format!("[fixture {label}]");
        u.move_mode.set(false);
        u.scale_mode.set(false);
        u.rotate_mode.set(false);
        open_fixture(i, &tag, r).await?;
        // An existing translated, rotated and centered object.
        tool_on(Tool::Rotate)?;
        let ok = tree_select_for("Transform Turn", || {
            GIZMO.with_borrow(|g| g.target.as_ref().is_some_and(|t| t.def == "Turn"))
        })
        .await;
        let t = tool_ready(Tool::Rotate, 8000).await;
        r.step(
            &format!("{tag}: Turn (translation 1 0.5 0, rotation 0 0 1 0.5, center 0.25 0 0) binds; the pivot is T + C"),
            ok && t.as_ref().is_some_and(|t| t.pivot == [1.25, 0.5, 0.0] && t.rotation == [0.0, 0.0, 1.0, 0.5]),
            format!("{:?}", t.map(|t| (t.pivot, t.rotation))),
        );
        let before = text();
        let after = rotate_drag(&tag, Axis::Z, 6, r).await?;
        let line = changed_lines(&before, &after).and_then(|l| l.first().cloned());
        r.step(
            &format!(
                "{tag}: rotating about the authored (parallel) axis rewrites only the angle token"
            ),
            line.as_deref()
                .is_some_and(|l| l.starts_with("rotation 0 0 1 ") && !l.ends_with(" 0.5")),
            format!("{line:?}"),
        );
        tool_on(Tool::Scale)?;
        scale_drag(&tag, Some(Axis::Y), r).await?;

        // An instanced (USEd) Transform is refused with a reason.
        let refused = tree_select_for("Transform Shared", || {
            GIZMO.with_borrow(|g| g.last_refusal.as_deref() == Some("transform-is-used-elsewhere"))
        })
        .await;
        ipc::sleep(200).await;
        r.step(
            &format!("{tag}: an instanced (USEd) Transform is refused: no usable handles, a reason is shown"),
            refused && svg_attr("data-state").as_deref() != Some("ready") && gizmo_msg().contains("USEd"),
            gizmo_msg(),
        );
        if i == 0 {
            // A Transform without rotation: refused, then written ONLY on
            // the explicit "Write rotation" action (one undo step).
            tool_on(Tool::Rotate)?;
            let before = source().await?;
            let refused = tree_select_for("Transform Bare", || {
                GIZMO.with_borrow(|g| g.missing_field == Some("rotation"))
            })
            .await;
            ipc::sleep(300).await;
            let after_select = source().await?;
            r.step(
                &format!("{tag}: a Transform without rotation is refused; nothing is written; a Write rotation action is offered"),
                refused
                    && after_select == before
                    && crate::element_by_id::<HtmlElement>("gz-write-field").is_some()
                    && gizmo_msg().contains("rotation"),
                gizmo_msg(),
            );
            click("gz-write-field")?;
            let t = tool_ready(Tool::Rotate, 10_000).await;
            let after = source().await?;
            let lines = changed_lines(&before.text, &after.text);
            r.step(
                &format!("{tag}: Write rotation adds exactly `rotation 0 0 1 0` to Bare as one undo step; the rings bind"),
                t.is_some_and(|t| t.def == "Bare")
                    && after.revision == before.revision + 1
                    && lines.as_ref().is_some_and(|l| l.len() == 1 && l[0].contains("DEF Bare") && l[0].contains("rotation 0 0 1 0 }")),
                format!("{lines:?}"),
            );
            rotate_drag(&tag, Axis::Z, 5, r).await?;
        }
        click("btn-save")?;
        wait_ms(5000, || (!u.dirty.get_untracked()).then_some(())).await?;
        r.step(&format!("{tag}: saved"), !u.dirty.get_untracked(), "");
    }
    Some(())
}

pub(super) async fn run(c: &p::RotScaleSmoke, r: &mut R) -> Option<()> {
    if !c.real_pointer {
        r.step(
            "the Rotate / Scale run needs the harness Xvfb (real pointer input)",
            false,
            "",
        );
        return None;
    }
    new_world(r).await?;
    fixtures(c, r).await?;
    // The Move tool still works on the same document after Rotate / Scale.
    tool_on(Tool::Move)?;
    tree_select_for("Transform Turn", || {
        GIZMO.with_borrow(|g| g.target.as_ref().is_some_and(|t| t.def == "Turn"))
    })
    .await;
    let moved = wait_ms(8000, || {
        GIZMO.with_borrow(|g| {
            g.target
                .as_ref()
                .is_some_and(|t| t.tool == Tool::Move)
                .then_some(())
        })
    })
    .await;
    r.step(
        "the Move tool binds again after Rotate / Scale (tools exclude each other)",
        moved.is_some() && pressed("btn-move") && !pressed("btn-rotate") && !pressed("btn-scale"),
        gizmo_msg(),
    );
    Some(())
}
