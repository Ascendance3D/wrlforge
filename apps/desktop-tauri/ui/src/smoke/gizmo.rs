// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A in-window translation-gizmo run (`--smoke-move`).
//!
//! Every drag is REAL X pointer input (xdotool inside the harness's own
//! Xvfb, see `src-tauri/src/smoke.rs`): press on the drawn handle, motion
//! with the button held, release. Nothing here dispatches a synthetic
//! pointer event at the gizmo. Mid-drag, the run proves that the RENDERED
//! object moved (pixel probes of the real frame) while the source, revision,
//! dirty flag and undo history did not.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::gizmo::{self as gz, Axis};

use super::create::{all, el, previewed, text, tree_item, wait_ms};
use super::pick::{calibrate, gesture, ready, viewport, REAL, REAL_CLICK_GAP_MS};
use super::{choose, click, computed, snapshot, token, R};
use crate::editor::{self, textarea, CORE};
use crate::gizmo::GIZMO;
use crate::ipc::{self, call};
use crate::ui::ui;

fn pct(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f64 * q).round() as usize]
}

#[derive(Default)]
struct Perf {
    commit_ms: Vec<f64>,
    inspector_ms: Vec<f64>,
    preview_ms: Vec<f64>,
    drags: u32,
}

/// One REAL X input action at client point (`x`, `y`), calibrated.
async fn real(action: &str, x: f64, y: f64) -> Result<String, String> {
    let (dx, dy) = REAL.get().ok_or("real pointer input is not calibrated")?;
    #[derive(serde::Serialize)]
    struct A<'a> {
        action: &'a str,
        x: f64,
        y: f64,
    }
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

/// The source state a drag must not touch until it is released.
#[derive(Debug, PartialEq, Clone)]
struct Source {
    revision: u64,
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
    text: String,
    view_hash: u64,
}

async fn source() -> Option<Source> {
    let s = snapshot().await?;
    Some(Source {
        revision: s.revision,
        dirty: s.dirty,
        can_undo: s.can_undo,
        can_redo: s.can_redo,
        view_hash: p::view_hash(&s.view),
        text: text(),
    })
}

/// The gizmo is drawn, usable and bound to the CURRENT selection, revision
/// and preview generation.
async fn gz_ready(ms: u32) -> Option<crate::gizmo::Target> {
    wait_ms(ms, || {
        let t = GIZMO.with_borrow(|g| {
            (g.drag.is_none() && !g.committing && g.layout.is_some())
                .then(|| g.target.clone())
                .flatten()
        })?;
        let sel = ui().selected.get_untracked()?;
        let gen = crate::pick::active()?;
        let rev = CORE.with_borrow(|c| c.revision);
        let svg = crate::element_by_id::<web_sys::Element>("gizmo")?;
        (t.item == sel.id
            && t.revision == sel.revision
            && t.revision == rev
            && t.seq == gen.seq
            && svg.get_attribute("data-state").as_deref() == Some("ready"))
        .then_some(t)
    })
    .await
}

/// Wait until the camera has not moved for a few frames (a preview reload
/// re-binds the viewpoint with X_ITE's transition animation).
async fn still_camera() -> Option<()> {
    let mut last = ipc::gizmo_camera()?;
    let mut same = 0;
    for _ in 0..80 {
        ipc::sleep(50).await;
        let c = ipc::gizmo_camera()?;
        if c.view == last.view && c.proj == last.proj {
            same += 1;
            if same >= 3 {
                return Some(());
            }
        } else {
            same = 0;
        }
        last = c;
    }
    None
}

/// Orbit the camera with a REAL drag on empty viewport space (Examine
/// navigation), then wait until it is still. Returns (before, after).
async fn orbit() -> Option<(gz::Camera, gz::Camera)> {
    ipc::sleep(REAL_CLICK_GAP_MS).await;
    still_camera().await?;
    let vp = viewport()?.get_bounding_client_rect();
    let cam0 = ipc::gizmo_camera()?;
    let (sx, sy) = (vp.left() + vp.width() * 0.15, vp.top() + vp.height() * 0.2);
    real("down", sx, sy).await.ok()?;
    for k in 1..=8 {
        real("move", sx + 12.0 * k as f64, sy + 7.0 * k as f64)
            .await
            .ok()?;
        ipc::sleep(40).await;
    }
    ipc::sleep(400).await; // no fling
    real("up", sx + 96.0, sy + 56.0).await.ok()?;
    still_camera().await?;
    Some((cam0, ipc::gizmo_camera()?))
}

fn layout() -> Option<gz::Layout> {
    GIZMO.with_borrow(|g| g.layout)
}

fn rendered() -> Option<[f64; 3]> {
    crate::gizmo::rendered()
}

async fn pixel(x: f64, y: f64) -> Option<[u8; 3]> {
    let v = ipc::preview_pixel(x, y).await?;
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

/// The scene background: a viewport point far from every object.
async fn background() -> Option<[u8; 3]> {
    let r = viewport()?.get_bounding_client_rect();
    pixel(r.left() + 6.0, r.top() + 6.0).await
}

fn differs(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter()
        .zip(b)
        .map(|(x, y)| (*x as i32 - y as i32).abs())
        .sum::<i32>()
        > 24
}

fn class_of(id: &str) -> String {
    crate::element_by_id::<web_sys::Element>(id)
        .and_then(|e| e.get_attribute("class"))
        .unwrap_or_default()
}

/// (grab point, unit screen direction of the handle) for `axis`.
fn grab(axis: Axis) -> Option<((f64, f64), (f64, f64))> {
    let l = layout()?;
    let h = l.handles[axis.index()];
    if !h.enabled {
        return None;
    }
    let tip = h.tip?;
    let (dx, dy) = (tip.0 - l.origin.0, tip.1 - l.origin.1);
    let len = dx.hypot(dy);
    Some((
        (l.origin.0 + dx * 0.6, l.origin.1 + dy * 0.6),
        (dx / len, dy / len),
    ))
}

/// Press on the handle, move `dist` px along its screen direction in
/// `steps`, without releasing. Returns the last pointer position.
async fn press_and_move(axis: Axis, dist: f64, steps: u32) -> Option<(f64, f64)> {
    let (g, d) = grab(axis)?;
    real("down", g.0, g.1).await.ok()?;
    ipc::sleep(60).await;
    let mut at = g;
    for k in 1..=steps {
        let f = k as f64 / steps as f64;
        at = (g.0 + d.0 * dist * f, g.1 + d.1 * dist * f);
        real("move", at.0, at.1).await.ok()?;
        ipc::sleep(40).await;
    }
    ipc::sleep(80).await;
    Some(at)
}

/// The Inspector's translation inputs.
fn inspector_translation() -> Vec<String> {
    all("tr[data-field=\"translation\"] input")
        .into_iter()
        .filter_map(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .collect()
}

/// The authored translation of DEF `name` in the editor text (tests only:
/// the first `translation` line after the DEF).
fn source_translation(name: &str) -> Option<[f64; 3]> {
    let t = text();
    let at = t.find(&format!("DEF {name} Transform"))?;
    let rest = &t[at..];
    let line = rest
        .lines()
        .find(|l| l.trim_start().starts_with("translation "))?;
    let v: Vec<f64> = line
        .split_whitespace()
        .skip(1)
        .take(3)
        .filter_map(|s| s.parse().ok())
        .collect();
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

fn close3(a: [f64; 3], b: [f64; 3], eps: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= eps)
}

/// One full drag along `axis` by `dist` px, with the mid-drag proofs, the
/// release and the post-commit proofs. Returns the committed translation.
#[allow(clippy::too_many_arguments)]
async fn drag_commit(
    tag: &str,
    name: &str,
    axis: Axis,
    dist: f64,
    perf: &mut Perf,
    r: &mut R,
) -> Option<[f64; 3]> {
    let t = gz_ready(8000).await;
    let still = still_camera().await;
    let t = if still.is_some() {
        gz_ready(2000).await
    } else {
        t
    };
    if !r.step(
        &format!(
            "{tag}: gizmo ready on {name} before the {} drag",
            axis.label()
        ),
        t.is_some() && still.is_some(),
        "",
    ) {
        return None;
    }
    let t = t?;
    let before = source().await?;
    let commits = GIZMO.with_borrow(|g| g.commits);
    let bg = background().await?;
    let o0 = layout()?.origin;
    let lay0 = layout();
    let grab0 = grab(axis);
    let at = press_and_move(axis, dist, 8).await;
    let status = crate::element_by_id::<HtmlElement>("gizmo-status")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    let mid_src = source().await?;
    let mid_rend = rendered();
    let mid_origin = layout().map(|l| l.origin);
    let dragging = class_of(&format!("gz-{}", axis.label().to_lowercase())).contains("active");
    let px_new = match mid_origin {
        Some(o) => pixel(o.0, o.1).await,
        None => None,
    };
    let px_old = pixel(o0.0, o0.1).await;
    let i = axis.index();
    let moved_only_axis = mid_rend.is_some_and(|m| {
        (0..3).all(|k| {
            if k == i {
                (m[k] - t.translation[k]).abs() > 0.3
            } else {
                (m[k] - t.translation[k]).abs() < 1e-6
            }
        })
    });
    r.step(
        &format!("{tag}: during the {} drag the rendered {name} moves along {} only; source, revision, dirty and history are unchanged", axis.label(), axis.label()),
        at.is_some()
            && dragging
            && moved_only_axis
            && mid_src == before
            && px_new.is_some_and(|p| differs(p, bg))
            // The old origin is uncovered only when the move is larger than
            // the object on screen.
            && (mid_origin.is_none_or(|o| (o.0 - o0.0).hypot(o.1 - o0.1) < 90.0)
                || px_old.is_some_and(|p| !differs(p, bg))),
        format!(
            "rendered {mid_rend:?} from {:?} · old-origin px {px_old:?} new-origin px {px_new:?} bg {bg:?} · source unchanged {} · grab {grab0:?} · origin {:?} tips {:?} · status {status}",
            t.translation,
            mid_src == before,
            lay0.map(|l| l.origin),
            lay0.map(|l| l.handles.map(|h| (h.tip, h.enabled))),
        ),
    );
    let at = at?;
    real("up", at.0, at.1).await.ok()?;
    let committed = wait_ms(5000, || {
        (GIZMO.with_borrow(|g| g.commits) > commits
            && CORE.with_borrow(|c| c.revision) == before.revision + 1)
            .then_some(())
    })
    .await;
    editor::idle().await;
    let after = source().await?;
    let (from, old, new) = super::change(&before.text, &after.text);
    let line_ok = after.text[..]
        .lines()
        .any(|l| l.contains("translation ") && l.contains(&new));
    let want = mid_rend.unwrap_or(t.translation);
    let st = source_translation(name);
    r.step(
        &format!(
            "{tag}: release commits ONE {} token edit as ONE undo step",
            axis.label()
        ),
        committed.is_some()
            && after.revision == before.revision + 1
            && after.dirty
            && after.can_undo
            && line_ok
            && st.is_some_and(|s| {
                (0..3).all(|k| {
                    if k == i {
                        (s[k] - want[k]).abs() < 0.05
                    } else {
                        s[k] == t.translation[k]
                    }
                })
            }),
        format!(
            "rev {} → {} · @{from} {old:?} → {new:?} · source {st:?}",
            before.revision, after.revision
        ),
    );
    // Inspector, Scene Tree, final preview.
    let label = format!("Transform {name}");
    let insp = wait_ms(5000, || {
        ui().inspection
            .get_untracked()
            .filter(|i| i.revision == after.revision && i.title == label)
    })
    .await;
    let tr = inspector_translation();
    let tr_ok = st.is_some_and(|s| {
        tr.len() == 3
            && tr
                .iter()
                .zip(s)
                .all(|(v, w)| v.parse::<f64>().ok() == Some(w))
    });
    // The Scene Tree re-renders after its (debounced) analysis of the new
    // revision.
    let li = wait_ms(3000, || {
        let held = ui()
            .analysis
            .with_untracked(|a| a.as_ref().map(|a| a.revision));
        (held == Some(after.revision))
            .then(|| tree_item(&label).filter(|li| li.class_list().contains("selected")))?
    })
    .await;
    let sel_ok = ui()
        .selected
        .get_untracked()
        .is_some_and(|s| s.revision == after.revision);
    r.step(
        &format!("{tag}: Inspector shows the new translation; Scene Tree still selects {name} at the new revision"),
        insp.is_some() && tr_ok && li.is_some() && sel_ok,
        format!("inspector {tr:?} · source {st:?}"),
    );
    let reb = gz_ready(10_000).await;
    still_camera().await;
    let fin = layout().map(|l| l.origin);
    let px_fin = match fin {
        Some(o) => pixel(o.0, o.1).await,
        None => None,
    };
    r.step(
        &format!("{tag}: the reloaded preview draws {name} at the committed position; the gizmo re-binds there"),
        reb.as_ref().is_some_and(|g| st.is_some_and(|s| close3(g.translation, s, 0.0)))
            && px_fin.is_some_and(|p| differs(p, bg)),
        format!("{} · px {px_fin:?}", ui().preview_status.get_untracked()),
    );
    if let Some(c) = GIZMO.with_borrow(|g| g.last_commit) {
        perf.commit_ms.push(c.commit_ms);
        if let Some(v) = c.inspector_ms {
            perf.inspector_ms.push(v);
        }
        if let Some(v) = c.preview_ms {
            perf.preview_ms.push(v);
        }
    }
    perf.drags += 1;
    st
}

async fn new_world_with_box(tag: &str, r: &mut R) -> Option<()> {
    let u = ui();
    let old = CORE.with_borrow(|c| c.session);
    click("btn-new")?;
    wait_ms(5000, || {
        u.doc
            .get_untracked()
            .filter(|d| d.untitled && d.revision == 0 && Some(d.session) != old)
    })
    .await?;
    click("btn-create")?;
    wait_ms(2000, || el("#create-list")).await?;
    click("create-box")?;
    let ok = wait_ms(5000, || {
        u.inspection
            .get_untracked()
            .filter(|i| i.title == "Transform Box_1" && i.revision == 1)
    })
    .await;
    let shown = previewed(1, 1, 20_000).await;
    r.step(
        &format!("{tag}: New World → Create Box"),
        ok.is_some() && shown.is_some() && ready(5000).await.is_some(),
        u.preview_status.get_untracked(),
    );
    Some(())
}

/// Select the object at world `p` with a REAL click (Select tool), then
/// switch to the Move tool.
async fn select_then_move(tag: &str, name: &str, p: [f64; 3], r: &mut R) -> Option<()> {
    let u = ui();
    if !u.pick_mode.get_untracked() {
        click("btn-select")?;
    }
    ready(10_000).await?;
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
    click("btn-move")?;
    let g = gz_ready(8000).await;
    let pressed = crate::element_by_id::<HtmlElement>("btn-move")
        .and_then(|b| b.get_attribute("aria-pressed"))
        .as_deref()
        == Some("true");
    let sel_off = crate::element_by_id::<HtmlElement>("btn-select")
        .and_then(|b| b.get_attribute("aria-pressed"))
        .as_deref()
        == Some("false");
    r.step(
        &format!("{tag}: Move tool on (Select off); the gizmo shows at {name}'s origin"),
        pressed && sel_off && g.as_ref().is_some_and(|g| close3(g.origin, p, 1e-9)),
        format!("{:?}", g.map(|g| g.origin)),
    );
    Some(())
}

async fn main_theme(c: &p::GizmoSmoke, perf: &mut Perf, r: &mut R) -> Option<()> {
    let u = ui();
    let theme = c.themes.first()?.clone();
    let tag = format!("[{theme}]");
    let sel = crate::element_by_id::<web_sys::HtmlSelectElement>("theme-select")?;
    choose(&sel, &theme)?;
    new_world_with_box(&tag, r).await?;

    // Calibrate REAL input (Select mode), then select by a real click.
    click("btn-select")?;
    ready(5000).await?;
    calibrate(r).await?;
    if REAL.get().is_none() {
        r.step(
            "real pointer input is available for the gizmo run",
            false,
            "",
        );
        return None;
    }
    select_then_move(&tag, "Box_1", [0.0; 3], r).await?;

    // Handles: X and Y usable, Z (towards the default camera) disabled.
    let l = layout()?;
    let cls: Vec<String> = ["gz-x", "gz-y", "gz-z"]
        .iter()
        .map(|i| class_of(i))
        .collect();
    r.step(
        &format!(
            "{tag}: front view: X and Y handles usable, Z (pointing at the viewer) drawn disabled"
        ),
        l.handles[0].enabled
            && l.handles[1].enabled
            && !l.handles[2].enabled
            && !cls[0].contains("disabled")
            && !cls[1].contains("disabled")
            && cls[2].contains("disabled"),
        format!("{cls:?}"),
    );
    let colors_ok = |theme_ok: &mut Vec<String>| {
        ["x", "y", "z"].iter().all(|a| {
            let line = crate::element_by_id::<web_sys::Element>(&format!("gz-{a}-line"));
            let got = line.map(|e| computed(&e, "stroke")).unwrap_or_default();
            let want = super::hex_rgb(&token(&format!("--wf-axis-{a}")));
            theme_ok.push(format!("{a}:{got}"));
            // A disabled handle is drawn in the disabled color instead.
            got == want || (*a == "z" && got == super::hex_rgb(&token("--wf-gizmo-disabled")))
        })
    };
    let mut seen = vec![];
    let ok = colors_ok(&mut seen);
    r.step(
        &format!("{tag}: handle colors are the theme's --wf-axis-* tokens"),
        ok,
        seen.join(" "),
    );

    // X positive, Y negative.
    let mut history = vec![text()];
    let t1 = drag_commit(&tag, "Box_1", Axis::X, 150.0, perf, r).await?;
    history.push(text());
    let t2 = drag_commit(&tag, "Box_1", Axis::Y, -110.0, perf, r).await?;
    history.push(text());
    r.step(
        &format!("{tag}: X moved positive, then Y moved negative"),
        t1[0] > 0.5 && t1[1] == 0.0 && t2[1] < -0.5 && t2[0] == t1[0],
        format!("{t1:?} → {t2:?}"),
    );

    // Zero-distance: press and release on the handle without moving.
    gz_ready(8000).await?;
    let before = source().await?;
    let unchanged = GIZMO.with_borrow(|g| g.unchanged);
    let (g, _) = grab(Axis::Y)?;
    real("down", g.0, g.1).await.ok()?;
    ipc::sleep(120).await;
    real("up", g.0, g.1).await.ok()?;
    ipc::sleep(300).await;
    let after = source().await?;
    r.step(
        &format!("{tag}: a zero-distance drag changes nothing (no revision, no undo step)"),
        after == before && GIZMO.with_borrow(|g| g.unchanged) == unchanged + 1,
        "",
    );

    // Escape cancels mid-drag; the rendered object returns.
    gz_ready(8000).await?;
    let before = source().await?;
    let bg = background().await?;
    let origin = layout()?.origin;
    let cancels = GIZMO.with_borrow(|g| g.cancels);
    let at = press_and_move(Axis::X, 120.0, 6).await?;
    let moved = rendered();
    real("escape", at.0, at.1).await.ok()?;
    let canceled = wait_ms(2000, || {
        (GIZMO.with_borrow(|g| g.cancels) > cancels).then_some(())
    })
    .await;
    ipc::sleep(150).await;
    let back = pixel(origin.0, origin.1).await;
    real("up", at.0, at.1).await.ok()?;
    ipc::sleep(300).await;
    let after = source().await?;
    r.step(
        &format!(
            "{tag}: Escape cancels the drag: the rendered object returns, release commits nothing"
        ),
        canceled.is_some()
            && moved.is_some_and(|m| m[0] > t2[0] + 0.3)
            && back.is_some_and(|p| differs(p, bg))
            && after == before
            && GIZMO.with_borrow(|g| g.last_cancel.as_deref() == Some("Escape")),
        format!(
            "moved {moved:?} · back px {back:?} · {:?}",
            GIZMO.with_borrow(|g| g.last_cancel.clone())
        ),
    );

    // Release OUTSIDE the viewport (over the source editor): pointer
    // capture delivers the release; the axis math still holds there.
    gz_ready(8000).await?;
    let vp = viewport()?.get_bounding_client_rect();
    let (g, d) = grab(Axis::X)?;
    let dist = -(g.0 - (vp.left() - 120.0)).max(10.0) / d.0.abs().max(0.2);
    let before = source().await?;
    let commits = GIZMO.with_borrow(|g| g.commits);
    let at = press_and_move(Axis::X, dist, 10).await?;
    let outside = at.0 < vp.left();
    real("up", at.0, at.1).await.ok()?;
    let done = wait_ms(5000, || {
        (GIZMO.with_borrow(|g| g.commits) > commits).then_some(())
    })
    .await;
    editor::idle().await;
    let after = source().await?;
    let st = source_translation("Box_1");
    r.step(
        &format!("{tag}: a release outside the viewport commits the computed X (one step)"),
        outside
            && done.is_some()
            && after.revision == before.revision + 1
            && st.is_some_and(|s| s[0] < t2[0] && s[1] == t2[1]),
        format!(
            "release at ({:.0}, {:.0}), viewport left {:.0} · {st:?}",
            at.0,
            at.1,
            vp.left()
        ),
    );
    // That move took the Box off screen: Undo it (one step, exact).
    editor::history(true).await;
    editor::idle().await;
    let back = source().await?;
    r.step(
        &format!("{tag}: one Undo removes exactly that move"),
        back.text == before.text && back.revision == after.revision + 1,
        format!("{:?}", source_translation("Box_1")),
    );
    let st = source_translation("Box_1");
    gz_ready(10_000).await?;

    // A document change during the drag cancels it; nothing commits.
    let before = source().await?;
    let cancels = GIZMO.with_borrow(|g| g.cancels);
    let commits = GIZMO.with_borrow(|g| g.commits);
    let at = press_and_move(Axis::Y, 80.0, 4).await?;
    let ta = textarea()?;
    let end = ta.value().encode_utf16().count() as u32;
    ta.set_range_text_with_start_and_end("# typed during a drag\n", end, end)
        .ok()?;
    let ev = web_sys::InputEvent::new("input").ok()?;
    ta.dispatch_event(&ev).ok()?;
    let typed = wait_ms(3000, || {
        (CORE.with_borrow(|c| c.revision) == before.revision + 1).then_some(())
    })
    .await;
    let canceled = wait_ms(2000, || {
        (GIZMO.with_borrow(|g| g.cancels) > cancels).then_some(())
    })
    .await;
    real("up", at.0, at.1).await.ok()?;
    ipc::sleep(400).await;
    editor::idle().await;
    let mid = text();
    let only_typed = mid.ends_with("# typed during a drag\n") && source_translation("Box_1") == st;
    editor::history(true).await;
    let restored = text() == before.text;
    r.step(
        &format!("{tag}: a source edit during a drag cancels it; the release commits nothing"),
        typed.is_some()
            && canceled.is_some()
            && GIZMO.with_borrow(|g| g.commits) == commits
            && only_typed
            && restored,
        format!("{:?}", GIZMO.with_borrow(|g| g.last_cancel.clone())),
    );

    // Second camera orientation: a REAL drag on empty viewport space orbits
    // the camera (navigation still works in Move mode).
    gz_ready(10_000).await?;
    let before = source().await?;
    let sel0 = u.selected.get_untracked();
    let (cam0, cam1) = orbit().await?;
    let after = source().await?;
    let g1 = gz_ready(5000).await;
    let l1 = layout();
    r.step(
        &format!("{tag}: a real drag on empty viewport space orbits the camera; source and selection unchanged; all three handles usable"),
        cam1.view != cam0.view
            && after == before
            && u.selected.get_untracked() == sel0
            && g1.is_some()
            && l1.is_some_and(|l| l.handles.iter().all(|h| h.enabled)),
        format!("{:?}", l1.map(|l| l.handles.map(|h| h.enabled))),
    );
    let t3 = drag_commit(&format!("{tag} oblique"), "Box_1", Axis::Z, 100.0, perf, r).await?;
    history.push(text());
    // The reload after a commit re-binds the default viewpoint (known
    // limit): orbit again for the second oblique drag.
    let o2 = orbit().await;
    r.step(
        &format!("{tag}: orbit again after the reload re-bound the default view"),
        o2.is_some_and(|(a, b)| a.view != b.view) && layout().is_some_and(|l| l.handles[0].enabled),
        "",
    );
    let t4 = drag_commit(&format!("{tag} oblique"), "Box_1", Axis::X, -45.0, perf, r).await?;
    history.push(text());
    r.step(
        &format!("{tag} oblique: Z then X moved only their own components"),
        t3[2].abs() > 0.3
            && t3[0] == st.map_or(f64::NAN, |s| s[0])
            && t4[2] == t3[2]
            && t4[1] == t3[1]
            && t4[0] != t3[0],
        format!("{t3:?} → {t4:?}"),
    );

    // Undo / Redo: one step per drag, exact text each time.
    let n = history.len() - 1;
    let mut ok = true;
    for k in (0..n).rev() {
        editor::history(true).await;
        editor::idle().await;
        ok &= text() == history[k];
    }
    let undone = ok;
    for want in &history[1..] {
        editor::history(false).await;
        editor::idle().await;
        ok &= text() == *want;
    }
    let g = gz_ready(10_000).await;
    r.step(
        &format!("{tag}: Undo restores each drag exactly ({n} steps); Redo re-applies each; the gizmo follows the source"),
        undone && ok && g.is_some_and(|g| Some(g.translation) == source_translation("Box_1")),
        format!("undo {undone} · redo {ok}"),
    );

    // Picking still selects the right object: add a Sphere, move it, then
    // click each object where it is drawn now.
    click("btn-create")?;
    wait_ms(2000, || el("#create-list")).await?;
    click("create-sphere")?;
    wait_ms(5000, || {
        u.inspection
            .get_untracked()
            .filter(|i| i.title == "Transform Sphere_1")
    })
    .await?;
    let rev = CORE.with_borrow(|c| c.revision);
    previewed(rev, 2, 20_000).await?;
    let ts = drag_commit(&tag, "Sphere_1", Axis::Y, 120.0, perf, r).await?;
    let tb = source_translation("Box_1")?;
    click("btn-select")?;
    ipc::sleep(REAL_CLICK_GAP_MS).await;
    let cam = ipc::gizmo_camera()?;
    let mut picks = vec![];
    for (name, at) in [("Box_1", tb), ("Sphere_1", ts)] {
        let (x, y) = gz::project(&cam, at)?;
        let out = gesture(x, y).await;
        let want = u.analysis.with_untracked(|a| {
            a.as_ref()?
                .items
                .iter()
                .find(|i| i.label == format!("Transform {name}"))
                .map(|i| i.id.clone())
        });
        picks.push(out.is_some_and(|o| o.is_proven() && o.item == want));
    }
    r.step(
        &format!("{tag}: after moving, viewport picking still selects the correct object (Box_1, Sphere_1)"),
        picks.iter().all(|b| *b),
        format!("{picks:?}"),
    );

    // Theme switches with the gizmo shown never touch the document.
    click("btn-move")?;
    gz_ready(8000).await?;
    let before = source().await?;
    let loads = u.preview_loads.get_untracked();
    let mut all_ok = true;
    let mut detail = vec![];
    for th in &c.themes {
        choose(&sel, th)?;
        wait_ms(3000, || {
            (crate::theme::current_attribute().as_deref() == Some(th.as_str())).then_some(())
        })
        .await?;
        ipc::sleep(100).await;
        let mut seen = vec![];
        let ok = colors_ok(&mut seen);
        all_ok &= ok;
        detail.push(format!("{th}: {}", seen.join(" ")));
    }
    let after = source().await?;
    r.step(
        "every theme recolors the gizmo from its tokens; the document, history and preview are untouched",
        all_ok && after == before && u.preview_loads.get_untracked() == loads && gz_ready(2000).await.is_some(),
        detail.join(" | "),
    );
    choose(&sel, &theme)?;

    // Save As → Close → Open → the moved objects are where they were saved.
    let saved_text = text();
    click("btn-save-as")?;
    wait_ms(5000, || (!u.dirty.get_untracked()).then_some(())).await?;
    click("btn-close")?;
    wait_ms(5000, || u.doc.get_untracked().is_none().then_some(())).await?;
    click("btn-open")?;
    let reopened = wait_ms(8000, || u.doc.get_untracked().filter(|d| !d.untitled)).await;
    let rev = CORE.with_borrow(|c| c.revision);
    previewed(rev, 2, 20_000).await?;
    r.step(
        &format!("{tag}: Save As → Close → Open shows exactly the saved source"),
        reopened.is_some() && text() == saved_text,
        format!("{:?}", reopened.map(|d| d.name)),
    );
    u.move_mode.set(false);
    select_then_move(&format!("{tag} reopened"), "Box_1", tb, r).await?;
    let g = gz_ready(5000).await;
    r.step(
        &format!("{tag}: after reopening, the gizmo sits at the saved translation"),
        g.is_some_and(|g| g.translation == tb),
        format!("{tb:?}"),
    );
    Some(())
}

async fn other_theme(theme: &str, perf: &mut Perf, r: &mut R) -> Option<()> {
    let tag = format!("[{theme}]");
    let sel = crate::element_by_id::<web_sys::HtmlSelectElement>("theme-select")?;
    choose(&sel, theme)?;
    ui().move_mode.set(false);
    new_world_with_box(&tag, r).await?;
    select_then_move(&tag, "Box_1", [0.0; 3], r).await?;
    let a = drag_commit(&tag, "Box_1", Axis::X, -130.0, perf, r).await?;
    let b = drag_commit(&tag, "Box_1", Axis::Y, 100.0, perf, r).await?;
    r.step(
        &format!("{tag}: X negative then Y positive"),
        a[0] < -0.5 && b[1] > 0.5 && b[0] == a[0],
        format!("{a:?} → {b:?}"),
    );
    Some(())
}

/// The source-format fixtures: open, select by a real click, move X, save.
/// Rust checks the bytes on disk at the end.
async fn fixtures(c: &p::GizmoSmoke, perf: &mut Perf, r: &mut R) -> Option<()> {
    let u = ui();
    for (i, label) in c.fixtures.iter().enumerate() {
        let tag = format!("[fixture {label}]");
        u.move_mode.set(false);
        #[derive(serde::Serialize)]
        struct A {
            index: usize,
        }
        let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: i })
            .await
            .ok()?;
        crate::ui::apply_open(o, "Opened").await;
        let rev = CORE.with_borrow(|c| c.revision);
        let roots = if i == 0 { 3 } else { 2 };
        let shown = previewed(rev, roots, 20_000).await;
        r.step(
            &format!("{tag}: opened and previewed"),
            shown.is_some(),
            u.preview_status.get_untracked(),
        );
        let name = if text().contains("DEF Box_ü") {
            "Box_ü"
        } else {
            "Box_1"
        };
        if i == 0 {
            undefined_transform(&tag, r).await?;
        }
        select_then_move(&tag, name, [0.0; 3], r).await?;
        drag_commit(&tag, name, Axis::X, 120.0, perf, r).await?;
        if i == 0 {
            // A nested Transform is refused with a reason; no handles. The
            // tree item must be from the current analysis (tree_select
            // refuses an outdated one).
            let rev = CORE.with_borrow(|c| c.revision);
            let mut refused = None;
            for _ in 0..3 {
                wait_ms(3000, || {
                    (u.analysis
                        .with_untracked(|a| a.as_ref().map(|a| a.revision))
                        == Some(rev))
                    .then_some(())
                })
                .await;
                if let Some(li) = tree_item("Transform Inner") {
                    li.click();
                }
                refused = wait_ms(2000, || {
                    GIZMO
                        .with_borrow(|g| g.last_refusal.clone())
                        .filter(|r| r == "transform-is-not-top-level")
                })
                .await;
                if refused.is_some() {
                    break;
                }
            }
            ipc::sleep(200).await;
            let state = crate::element_by_id::<web_sys::Element>("gizmo")
                .and_then(|e| e.get_attribute("data-state"));
            let msg = crate::element_by_id::<HtmlElement>("gizmo-status")
                .and_then(|e| e.text_content())
                .unwrap_or_default();
            r.step(
                &format!("{tag}: a nested Transform is refused (not top-level): no usable handles, a reason is shown"),
                refused.is_some() && state.as_deref() != Some("ready") && msg.contains("top-level"),
                format!("{state:?} · {msg}"),
            );
        }
        click("btn-save")?;
        wait_ms(5000, || (!u.dirty.get_untracked()).then_some(())).await?;
        r.step(&format!("{tag}: saved"), !u.dirty.get_untracked(), "");
    }
    Some(())
}

/// Click the Scene Tree item `label` of the CURRENT analysis (tree_select
/// refuses an outdated one), retrying while the tree catches up.
async fn tree_click(label: &str, until: impl Fn() -> bool) -> bool {
    let rev = CORE.with_borrow(|c| c.revision);
    for _ in 0..3 {
        wait_ms(3000, || {
            (ui()
                .analysis
                .with_untracked(|a| a.as_ref().map(|a| a.revision))
                == Some(rev))
            .then_some(())
        })
        .await;
        if let Some(li) = tree_item(label) {
            li.click();
        }
        if wait_ms(2000, || until().then_some(())).await.is_some() {
            return true;
        }
    }
    false
}

/// A top-level Transform WITHOUT a DEF is bound by its root index: select it
/// in the Scene Tree, drag it, check the one-token commit, then Undo it.
async fn undefined_transform(tag: &str, r: &mut R) -> Option<()> {
    let u = ui();
    if !u.move_mode.get_untracked() {
        click("btn-move")?;
    }
    let picked = tree_click("Transform", || {
        GIZMO.with_borrow(|g| g.target.as_ref().is_some_and(|t| t.def == "the Transform"))
    })
    .await;
    let t = gz_ready(8000).await.filter(|t| t.def == "the Transform");
    r.step(
        &format!("{tag}: a top-level Transform with NO DEF gets handles (bound by its root index)"),
        picked && t.as_ref().is_some_and(|t| t.origin == [0.0, -2.5, 0.0]),
        format!("{:?}", t.as_ref().map(|t| t.origin)),
    );
    t?;
    still_camera().await?;
    let before = source().await?;
    let commits = GIZMO.with_borrow(|g| g.commits);
    let at = press_and_move(Axis::X, 80.0, 6).await?;
    let mid = source().await?;
    let rend = rendered();
    real("up", at.0, at.1).await.ok()?;
    wait_ms(5000, || {
        (GIZMO.with_borrow(|g| g.commits) > commits).then_some(())
    })
    .await;
    editor::idle().await;
    let after = source().await?;
    let (_, old, new) = super::change(&before.text, &after.text);
    let line = after
        .text
        .lines()
        .find(|l| l.starts_with("Transform { translation "))
        .unwrap_or_default()
        .to_string();
    editor::history(true).await;
    editor::idle().await;
    let undone = text() == before.text;
    r.step(
        &format!("{tag}: dragging it moves only its X token (one undo step), and Undo restores it"),
        mid == before
            && rend.is_some_and(|v| v[0] > 0.3 && v[1] == -2.5 && v[2] == 0.0)
            && after.revision == before.revision + 1
            && old == "0"
            && line.starts_with(&format!("Transform {{ translation {new} -2.5 0 "))
            && undone,
        format!("{old:?} → {new:?} · {line}"),
    );
    Some(())
}

pub(super) async fn run(c: &p::GizmoSmoke, r: &mut R) -> Option<()> {
    for _ in 0..200 {
        if crate::element_by_id::<HtmlElement>("btn-move").is_some() {
            break;
        }
        ipc::sleep(20).await;
    }
    if !r.step(
        "real X pointer input is available (harness Xvfb)",
        c.real_pointer,
        "the gizmo run needs real input; synthetic events are not accepted as proof",
    ) {
        return None;
    }
    let mut perf = Perf::default();
    main_theme(c, &mut perf, r).await;
    for th in c.themes.iter().skip(1) {
        other_theme(th, &mut perf, r).await;
    }
    fixtures(c, &mut perf, r).await;
    let (mv, fr, lat) =
        GIZMO.with_borrow(|g| (g.move_ms.clone(), g.frame_ms.clone(), g.latency_ms.clone()));
    r.step(
        "performance (ms, median / p95 / n)",
        true,
        format!(
            "pointer event → translation applied {:.1}/{:.1}/{} · move handler {:.2}/{:.2}/{} · gizmo update {:.2}/{:.2}/{} · commit {:.1}/{:.1}/{} · release → Inspector {:.1}/{:.1}/{} · release → final preview bound {:.1}/{:.1}/{} · drags {}",
            pct(&lat, 0.5),
            pct(&lat, 0.95),
            lat.len(),
            pct(&mv, 0.5), pct(&mv, 0.95), mv.len(),
            pct(&fr, 0.5), pct(&fr, 0.95), fr.len(),
            pct(&perf.commit_ms, 0.5), pct(&perf.commit_ms, 0.95), perf.commit_ms.len(),
            pct(&perf.inspector_ms, 0.5), pct(&perf.inspector_ms, 0.95), perf.inspector_ms.len(),
            pct(&perf.preview_ms, 0.5), pct(&perf.preview_ms, 0.95), perf.preview_ms.len(),
            perf.drags,
        ),
    );
    Some(())
}
