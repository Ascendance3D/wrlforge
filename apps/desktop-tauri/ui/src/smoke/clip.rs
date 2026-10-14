// SPDX-License-Identifier: GPL-3.0-or-later
//! LV2-C in-window overlay clipping run (part of `--smoke-move`).
//!
//! The Move handles and the selection outline are drawn only inside the
//! 3D canvas. For a pivot outside the canvas (four directions) no handles
//! are drawn and the status line says why; for a pivot just inside the top
//! edge the Y handle runs past the canvas and must be CLIPPED -- painting
//! and hit testing -- so the toolbar's Live control above it still receives
//! the pointer. Checked after Fit, orbit, a resize to two window sizes and
//! View → Frame Selection, with the browser's own hit test
//! (`elementFromPoint`) at the real coordinates.

use leptos::prelude::*;
use web_sys::Element;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::gizmo as gz;

use super::create::{apply_field, el, wait_ms};
use super::gizmo::{orbit, resize, still_camera, tree_click};
use super::pick::ready;
use super::{click, R};
use crate::gizmo::GIZMO;
use crate::ipc;
use crate::ui::ui;

fn svg_state() -> String {
    el("#gizmo")
        .and_then(|e| e.get_attribute("data-state"))
        .unwrap_or_default()
}

fn hit(x: f64, y: f64) -> Option<Element> {
    web_sys::window()?
        .document()?
        .element_from_point(x as f32, y as f32)
}

fn inside(e: &Element, sel: &str) -> bool {
    el(sel).is_some_and(|c| c.contains(Some(e)))
}

/// The Live toggle in the viewport toolbar still receives the pointer.
fn live_reachable() -> bool {
    let Some(label) = el("#chk-live").and_then(|c| c.parent_element()) else {
        return false;
    };
    let r = label.get_bounding_client_rect();
    hit(r.left() + r.width() / 2.0, r.top() + r.height() / 2.0)
        .is_some_and(|e| label.contains(Some(&e)) && !inside(&e, "#gizmo"))
}

/// Every drawn handle point OUTSIDE the canvas hits something other than
/// the gizmo (it is clipped); `(points outside, all clipped)`.
fn clipped() -> (usize, bool) {
    let Some(cam) = ipc::gizmo_camera() else {
        return (0, false);
    };
    let pts: Vec<(f64, f64)> = GIZMO.with_borrow(|g| {
        g.layout
            .map(|l| {
                std::iter::once(l.origin)
                    .chain(l.handles.iter().filter_map(|h| h.tip))
                    .collect()
            })
            .unwrap_or_default()
    });
    let out: Vec<_> = pts.into_iter().filter(|p| !cam.contains(*p)).collect();
    let ok = out
        .iter()
        .all(|&(x, y)| hit(x, y).is_none_or(|e| !inside(&e, "#gizmo")));
    (out.len(), ok)
}

fn status() -> String {
    ui().gizmo_message
        .get_untracked()
        .map(|m| m.1)
        .unwrap_or_default()
}

/// Write `v` into the selected Transform's translation (Inspector, one
/// undo step) and wait until the handles settle on it.
async fn place(v: [f64; 3]) -> Option<()> {
    let vals: Vec<String> = v.iter().map(|x| format!("{x}")).collect();
    apply_field("translation", &vals).await?;
    ready(20_000).await?;
    still_camera().await?;
    wait_ms(8000, || {
        let bound = GIZMO.with_borrow(|g| {
            g.target
                .as_ref()
                .is_some_and(|t| (0..3).all(|i| (t.origin_w()[i] - v[i]).abs() < 1e-6))
        });
        (bound && matches!(svg_state().as_str(), "ready" | "hidden")).then_some(())
    })
    .await?;
    ipc::sleep(150).await;
    Some(())
}

/// The world point on the camera's own up axis through the origin whose
/// projection is `px` below the canvas top (bisection; the camera may have
/// been orbited, so world Y is not "up" on screen).
fn near_top(cam: &gz::Camera, px: f64) -> Option<[f64; 3]> {
    let m = cam.view;
    let up = [m[1], m[5], m[9]];
    let at = |t: f64| [up[0] * t, up[1] * t, up[2] * t];
    let want = cam.rect[1] + px;
    let (mut lo, mut hi) = (0.0_f64, 1000.0_f64);
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        let y = gz::project(cam, at(mid))?.1;
        if y > want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(at((lo + hi) / 2.0))
}

/// One check of the invariant, named `what`.
fn check(r: &mut R, what: &str, offscreen: bool) {
    let state = svg_state();
    let (outside, ok) = clipped();
    let live = live_reachable();
    let msg = status();
    let pass = live
        && ok
        && if offscreen {
            state == "hidden" && msg.contains("outside the view")
        } else {
            state == "ready"
        };
    r.step(
        &format!("[clip] {what}"),
        pass,
        format!("state {state} · handle points outside canvas {outside} (clipped {ok}) · Live reachable {live} · {msg:?}"),
    );
}

/// Open fx-lf.wrl (one top-level DEF Box_1 Transform at the origin) and
/// select Box_1 with the Move tool on.
async fn open_box(r: &mut R, tag: &str) -> Option<()> {
    let u = ui();
    #[derive(serde::Serialize)]
    struct A {
        index: usize,
    }
    let o = ipc::call::<p::OpenOutcome>("smoke_open_fixture", A { index: 2 })
        .await
        .ok()?;
    crate::ui::apply_open(o, "Opened").await;
    ready(20_000).await?;
    if !u.move_mode.get_untracked() {
        click("btn-move")?;
    }
    let picked = tree_click("Transform Box_1", || {
        GIZMO.with_borrow(|g| g.target.as_ref().is_some_and(|t| t.def.contains("Box_1")))
    })
    .await;
    let labels: Vec<String> = super::create::all("li.tree-item .label")
        .iter()
        .filter_map(|l| l.text_content())
        .collect();
    let def = GIZMO.with_borrow(|g| g.target.as_ref().map(|t| t.def.clone()));
    r.step(
        &format!("[{tag}] Box_1 selected with the Move tool"),
        picked,
        format!(
            "{} · target {def:?} · tree {labels:?} · move {}",
            status(),
            u.move_mode.get_untracked()
        ),
    );
    picked.then_some(())
}

/// The selected node's world box corners, once its outline is proven.
async fn selected_corners() -> Option<[[f64; 3]; 8]> {
    wait_ms(8000, || {
        crate::highlight::selection_bounds().and_then(|b| p::highlight::world_corners(&b).ok())
    })
    .await
}

/// LV2-C Duplicate visibility in the real window: an object at the right
/// edge of the view (the old +X copy left the canvas) and an object that
/// fills the view (no copy can fit: the camera frames both, camera only).
async fn duplicate_cases(r: &mut R) -> Option<()> {
    let u = ui();
    open_box(r, "dup").await?;
    for case in ["edge", "fills"] {
        // "edge": a small object (a quarter size) near the right edge; the
        // old rule's +X copy left the canvas. "fills": full size, framed.
        // Fit frames the full-size box first; then the size is set.
        // (Already 1 1 1 the first time: no edit, nothing to wait for.)
        let _ = apply_field("scale", &["1".into(), "1".into(), "1".into()]).await;
        ready(20_000).await?;
        crate::command::run(p::command::CommandId::ViewFitAll);
        still_camera().await?;
        if case == "edge" {
            apply_field("scale", &["0.25".into(), "0.25".into(), "0.25".into()]).await?;
            ready(20_000).await?;
            still_camera().await?;
        }
        let cam = ipc::gizmo_camera()?;
        let x = if case == "edge" {
            // The pivot 70 px inside the right edge (bisection along X).
            let want = cam.rect[0] + cam.rect[2] - 70.0;
            let (mut lo, mut hi) = (0.0_f64, 1000.0_f64);
            for _ in 0..80 {
                let mid = (lo + hi) / 2.0;
                if gz::project(&cam, [mid, 0.0, 0.0])?.0 < want {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lo + hi) / 2.0
        } else {
            0.0
        };
        stage(format!("dup {case} place"));
        place([x, 0.0, 0.0]).await?;
        if case == "fills" {
            crate::command::run(p::command::CommandId::ViewFrameSelection);
            still_camera().await?;
        }
        selected_corners().await?;
        let before = super::create::text();
        let rev = crate::editor::CORE.with_borrow(|c| c.revision);
        crate::command::run(p::command::CommandId::EditDuplicateNode);
        wait_ms(8000, || {
            crate::editor::CORE.with_borrow(|c| (c.revision == rev + 1 && !c.busy).then_some(()))
        })
        .await?;
        ready(20_000).await?;
        still_camera().await?;
        let msg = u.message.get_untracked();
        let copy = selected_corners().await;
        let cam = ipc::gizmo_camera()?;
        let in_view = copy.is_some_and(|c| {
            c.iter()
                .all(|p| gz::project(&cam, *p).is_some_and(|q| cam.contains(q)))
        });
        let is_copy = crate::highlight::selection_bounds().is_some()
            && super::create::text().contains("DEF Box_2 Transform")
            && ui()
                .inspection
                .get_untracked()
                .is_some_and(|i| i.title == "Transform Box_2");
        let framed = msg.contains("camera moved");
        r.step(
            &format!(
                "[dup] {}: the selected copy is wholly inside the canvas after Duplicate",
                if case == "edge" {
                    "object at the right edge (+X would leave the view)"
                } else {
                    "object filling the view (the camera frames both)"
                }
            ),
            in_view && is_copy && (if case == "edge" { !framed } else { framed }),
            format!(
                "{msg} · copy in view {in_view} · copy selected {is_copy} · corners {:?}",
                copy.map(|c| c
                    .iter()
                    .filter_map(|p| gz::project(&cam, *p))
                    .map(|(x, y)| (x.round(), y.round()))
                    .collect::<Vec<_>>())
            ),
        );
        crate::editor::history(true).await;
        crate::editor::idle().await;
        r.step(
            &format!("[dup] {case}: one Undo removes the copy (exact bytes)"),
            super::create::text() == before,
            "",
        );
        ready(20_000).await?;
        let _ = tree_click("Transform Box_1", || {
            GIZMO.with_borrow(|g| g.target.as_ref().is_some_and(|t| t.def.contains("Box_1")))
        })
        .await;
    }
    Some(())
}

thread_local! {
    /// The last stage reached (an early stop names it in the report).
    static STAGE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn stage(s: impl Into<String>) {
    STAGE.with_borrow_mut(|x| *x = s.into());
}

/// Run both parts; an early stop is a FAILED step naming where it stopped.
pub(super) async fn run(r: &mut R) {
    stage("duplicate");
    let dup = duplicate_cases(r).await.is_some();
    let at_dup = STAGE.with_borrow(|s| s.clone());
    stage("clip");
    let clip = clipping(r).await.is_some();
    let at_clip = STAGE.with_borrow(|s| s.clone());
    r.step(
        "[clip] the Duplicate and clipping runs completed",
        dup && clip,
        format!("duplicate {dup} (last stage {at_dup:?}) · clipping {clip} (last stage {at_clip:?}) · status {:?}", status()),
    );
}

async fn clipping(r: &mut R) -> Option<()> {
    let u = ui();
    open_box(r, "clip").await?;
    // LV2-C hidden edges: seen from a corner, the Box's precise outline
    // draws its front edges solid and the edges its own faces hide apart
    // (faint, dashed), never as if they were visible.
    stage("hidden edges");
    crate::command::run(p::command::CommandId::ViewFitAll);
    still_camera().await?;
    orbit().await?;
    let seen = wait_ms(5000, || {
        crate::highlight::HIGHLIGHT.with_borrow(|h| {
            (h.mode == "precise" && !h.path.is_empty() && !h.hidden_path.is_empty()).then(|| {
                (
                    h.path.matches('M').count(),
                    h.hidden_path.matches('M').count(),
                    h.depth_reason.clone(),
                )
            })
        })
    })
    .await;
    r.step(
        "[clip] the Box's precise outline separates its hidden edges (dashed) from the visible ones",
        seen.as_ref().is_some_and(|(v, h, d)| *v >= 6 && *h >= 1 && d.is_none()),
        format!("{seen:?} (visible segments, hidden segments, why not separated)"),
    );
    // The harness screen is 1600 x 1000 (the host refuses any other
    // display), so 1920 x 1080 is checked in the packaged-app acceptance.
    for (w, h) in [(1366, 768), (1600, 900)] {
        stage(format!("{w}x{h} resize"));
        resize(w, h).await?;
        crate::command::run(p::command::CommandId::ViewFitAll);
        still_camera().await?;
        let size = format!("{w}×{h}");
        // Four directions, each well outside the canvas.
        let far = 50.0;
        for (dir, v) in [
            ("right", [far, 0.0, 0.0]),
            ("left", [-far, 0.0, 0.0]),
            ("above", [0.0, far, 0.0]),
            ("below", [0.0, -far, 0.0]),
        ] {
            stage(format!("{w}x{h} place {dir}"));
            place(v).await?;
            check(
                r,
                &format!("{size}: pivot {dir} of the canvas — no handles, reason shown"),
                true,
            );
        }
        // Just inside the top edge: the Y handle runs into the toolbar and
        // is clipped there.
        let cam = ipc::gizmo_camera()?;
        stage(format!("{w}x{h} near top"));
        let v = near_top(&cam, 14.0)?;
        stage(format!("{w}x{h} place near top {v:?}"));
        place(v).await?;
        let (outside, _) = clipped();
        check(r, &format!("{size}: pivot 14 px below the canvas top — handles clipped ({outside} points beyond the edge)"), false);
        // After an orbit the same holds (handles hidden or clipped).
        if orbit().await.is_some() {
            ipc::sleep(150).await;
            let off = GIZMO.with_borrow(|g| g.layout.is_none());
            check(r, &format!("{size}: after an orbit"), off);
        }
    }
    // Frame Selection brings an offscreen pivot back with handles.
    stage("frame selection");
    place([60.0, 0.0, 0.0]).await?;
    check(r, "pivot far right before Frame Selection", true);
    crate::command::run(p::command::CommandId::ViewFrameSelection);
    still_camera().await?;
    let back = wait_ms(5000, || (svg_state() == "ready").then_some(())).await;
    let origin_in = GIZMO
        .with_borrow(|g| g.layout.map(|l| l.origin))
        .zip(ipc::gizmo_camera())
        .is_some_and(|(o, c)| c.contains(o));
    r.step(
        "[clip] View → Frame Selection shows the object again with its handles inside the canvas",
        back.is_some() && origin_in && live_reachable(),
        format!("state {} · {}", svg_state(), u.message.get_untracked()),
    );
    // The selection outline is clipped the same way (CSS clip-path).
    let outline_clip = el("#selection-outline")
        .and_then(|e| {
            web_sys::window()?
                .get_computed_style(&e)
                .ok()??
                .get_property_value("clip-path")
                .ok()
        })
        .unwrap_or_default();
    r.step(
        "[clip] the selection outline and the gizmo are clipped to the canvas (clip-path)",
        outline_clip.contains("inset"),
        outline_clip,
    );
    Some(())
}
