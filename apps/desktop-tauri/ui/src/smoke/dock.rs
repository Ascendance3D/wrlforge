// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-DOCK-1 in-window docking run (`--smoke-dock*`).
//!
//! Every drag, click and key that docks, reorders, resizes or switches a
//! tab is REAL X input inside the harness's own Xvfb (the VISUAL-3A rule).
//! Around every workspace change the run proves that the document, its
//! revision, dirty flag, undo / redo state, the Scene Tree selection, the
//! Source selection range, scroll and syntax layer, the X_ITE canvas object,
//! its preview generation and its load count are all unchanged, and that a
//! real viewport click still selects the exact source node
//! (`WRONG_SOURCE_SELECTIONS = 0`).

use std::cell::Cell;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::gizmo::{self as gz, Axis};
use wrlforge_desktop_protocol::layout::{
    self as lay, DropTarget, Layout, LayoutWire, PanelId, PresetId, SplitAxis, Zone,
};

use super::create::{el, wait_ms};
use super::gizmo::{real, still_camera, tree_click};
use super::pick::{calibrate, gesture, ready, REAL_CLICK_GAP_MS};
use super::{click, doc_state, snapshot, DocState, R};
use crate::dock::{self, STATS};
use crate::editor::{textarea, CORE};
use crate::gizmo::GIZMO;
use crate::ipc::{self, call};
use crate::ui::ui;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Main,
    Restore,
    Damaged,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub phase: Phase,
    pub real_pointer: bool,
    pub expected: Option<LayoutWire>,
}

thread_local! {
    pub(super) static WRONG: Cell<u64> = const { Cell::new(0) };
    pub(super) static PICKS: Cell<u64> = const { Cell::new(0) };
}

const PROBE: &str = "__wfDockProbe";

/// Everything a workspace change must leave untouched.
#[derive(Debug, PartialEq)]
struct Keep {
    doc: DocState,
    scroll: (i32, i32),
    syntax: String,
    generation: Option<u64>,
    preview_status: String,
    probes: bool,
}

async fn keep() -> Option<Keep> {
    let ta = textarea()?;
    Some(Keep {
        doc: doc_state().await?,
        scroll: (ta.scroll_top(), ta.scroll_left()),
        syntax: el("#source-hl")
            .and_then(|e| e.get_attribute("data-state"))
            .unwrap_or_default(),
        generation: crate::pick::active().map(|g| g.seq),
        preview_status: ui().preview_status.get_untracked(),
        probes: probes_ok(),
    })
}

/// Mark the textarea and the canvas OBJECTS: a re-created element has no mark.
pub(super) fn set_probes() {
    for id in ["source", "viewport"] {
        if let Some(e) = crate::element_by_id::<web_sys::Element>(id) {
            let _ = js_sys::Reflect::set(&e, &PROBE.into(), &id.into());
        }
    }
}

pub(super) fn probes_ok() -> bool {
    ["source", "viewport"].iter().all(|id| {
        crate::element_by_id::<web_sys::Element>(id)
            .and_then(|e| js_sys::Reflect::get(&e, &PROBE.into()).ok())
            .and_then(|v| v.as_string())
            .as_deref()
            == Some(*id)
    })
}

fn active_id() -> String {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|e| e.id())
        .unwrap_or_default()
}

fn layout() -> Layout {
    dock::dock().layout.get_untracked()
}

fn rect_of(id: &str) -> Option<web_sys::DomRect> {
    Some(crate::element_by_id::<web_sys::Element>(id)?.get_bounding_client_rect())
}

fn tab_center(p: PanelId) -> Option<(f64, f64)> {
    let r = rect_of(&format!("dock-tab-{}", p.slug()))?;
    Some((r.left() + r.width() * 0.35, r.top() + r.height() / 2.0))
}

/// A point inside the content of the group that holds `p` (fractions).
fn content_point(p: PanelId, fx: f64, fy: f64) -> Option<(f64, f64)> {
    let g = dock::dock().geo.get_untracked();
    let c = g.group_of(p)?.content;
    let o = rect_of("dock")?;
    Some((o.left() + c.x + c.w * fx, o.top() + c.y + c.h * fy))
}

fn host_state(p: PanelId) -> String {
    crate::element_by_id::<web_sys::Element>(&format!("dock-panel-{}", p.slug()))
        .and_then(|e| e.get_attribute("data-state"))
        .unwrap_or_default()
}

fn guide() -> Option<(String, String, String)> {
    let g = el("#dock-guide")?;
    Some((
        g.get_attribute("data-zone").unwrap_or_default(),
        g.get_attribute("data-anchor").unwrap_or_default(),
        g.get_attribute("data-valid").unwrap_or_default(),
    ))
}

/// Press at `from`, move to `to` in steps, without releasing.
async fn press_drag(from: (f64, f64), to: (f64, f64), steps: u32) -> Option<()> {
    real("down", from.0, from.1).await.ok()?;
    ipc::sleep(60).await;
    for k in 1..=steps {
        let f = k as f64 / steps as f64;
        real(
            "move",
            from.0 + (to.0 - from.0) * f,
            from.1 + (to.1 - from.1) * f,
        )
        .await
        .ok()?;
        ipc::sleep(35).await;
    }
    ipc::sleep(120).await;
    Some(())
}

async fn release(at: (f64, f64)) -> Option<()> {
    real("up", at.0, at.1).await.ok()?;
    ipc::sleep(250).await;
    Some(())
}

pub(super) async fn key(name: &str) -> Option<()> {
    real(&format!("key-{name}"), 0.0, 0.0).await.ok()?;
    ipc::sleep(150).await;
    Some(())
}

async fn saved() {
    // The persist loop is idle and at least one save landed.
    let n = STATS.with_borrow(|s| s.saves + s.save_failures);
    let _ = wait_ms(3000, || {
        (STATS.with_borrow(|s| s.saves + s.save_failures) > n).then_some(())
    })
    .await;
    ipc::sleep(200).await;
}

fn diff(a: &Keep, b: &Keep) -> String {
    if a == b {
        "unchanged".into()
    } else {
        format!("{a:?} -> {b:?}")
    }
}

/// A real drag of `panel`'s tab to `to`: the guide shown just before the
/// release, and whether the layout stayed unchanged until the release.
async fn dock_drag(
    panel: PanelId,
    to: (f64, f64),
) -> Option<(Option<(String, String, String)>, bool, bool)> {
    let from = tab_center(panel)?;
    let before = layout();
    press_drag(from, to, 8).await?;
    let shown = guide();
    let overlay = el("#dock-overlay").is_some() && super::create::all(".dock-zone-tile").len() == 5;
    let untouched = layout() == before;
    release(to).await?;
    Some((shown, untouched, overlay))
}

/// A real Select-tool click on the world point `w`: must select exactly
/// `want` (an item label). Counts wrong selections.
pub(super) async fn pick_check(tag: &str, w: [f64; 3], want: &str, r: &mut R) -> Option<()> {
    let u = ui();
    if !u.pick_mode.get_untracked() {
        click("btn-select")?;
    }
    ready(10_000).await?;
    still_camera().await?;
    ipc::sleep(REAL_CLICK_GAP_MS).await;
    let cam = ipc::gizmo_camera()?;
    let (x, y) = gz::project(&cam, w)?;
    let out = gesture(x, y).await;
    let item = u.analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.label == want)
            .map(|i| i.id.clone())
    });
    PICKS.set(PICKS.get() + 1);
    let proven = out.as_ref().is_some_and(|o| o.is_proven());
    let right = out.as_ref().is_some_and(|o| o.item == item);
    if proven && !right {
        WRONG.set(WRONG.get() + 1);
    }
    let canvas = rect_of("viewport")?;
    r.step(
        &format!("{tag}: a real viewport click selects exactly {want}"),
        proven && right && u.selected.get_untracked().map(|s| s.id) == item,
        format!(
            "{:?} · canvas {:.0}x{:.0} at ({:.0},{:.0})",
            out.map(|o| (o.status, o.reason)),
            canvas.width(),
            canvas.height(),
            canvas.left(),
            canvas.top()
        ),
    );
    Some(())
}

/// Every shown panel's content meets its minimum, and nothing overlaps
/// the dock edge.
fn minimums_ok() -> (bool, String) {
    let g = dock::dock().geo.get_untracked();
    let mut bad = vec![];
    for gr in &g.groups {
        for p in &gr.panels {
            let (mw, mh) = p.min_size();
            if gr.content.w + 0.5 < mw || gr.content.h + 0.5 < mh {
                bad.push(format!(
                    "{} {:.0}x{:.0}",
                    p.title(),
                    gr.content.w,
                    gr.content.h
                ));
            }
        }
    }
    // The DOM agrees with the model for the on-screen hosts.
    for p in PanelId::ALL {
        if host_state(p) != "active" {
            continue;
        }
        let Some(r) = rect_of(&format!("dock-panel-{}", p.slug())) else {
            bad.push(format!("{} host missing", p.title()));
            continue;
        };
        let (mw, mh) = p.min_size();
        if r.width() + 1.0 < mw || r.height() + 1.0 < mh {
            bad.push(format!(
                "{} DOM {:.0}x{:.0}",
                p.title(),
                r.width(),
                r.height()
            ));
        }
    }
    (
        bad.is_empty(),
        if bad.is_empty() {
            format!(
                "canvas {:.0}x{:.0}, {} groups",
                g.width,
                g.height,
                g.groups.len()
            )
        } else {
            bad.join("; ")
        },
    )
}

/// The four tool buttons are fully inside the viewport panel and do not
/// overlap (APP-SHELL-0 §2.4 clipping defect).
fn tools_visible() -> (bool, String) {
    let Some(host) = rect_of("dock-panel-viewport") else {
        return (false, "no viewport host".into());
    };
    let mut rects = vec![];
    let mut bad = vec![];
    for id in ["btn-select", "btn-move", "btn-rotate", "btn-scale"] {
        let Some(r) = rect_of(id) else {
            bad.push(format!("{id} missing"));
            continue;
        };
        let inside = r.width() > 10.0
            && r.height() > 10.0
            && r.left() >= host.left() - 0.5
            && r.right() <= host.right() + 0.5
            && r.top() >= host.top() - 0.5
            && r.bottom() <= host.bottom() + 0.5;
        if !inside {
            bad.push(format!(
                "{id} ({:.0},{:.0} {:.0}x{:.0}) outside host ({:.0},{:.0} {:.0}x{:.0})",
                r.left(),
                r.top(),
                r.width(),
                r.height(),
                host.left(),
                host.top(),
                host.width(),
                host.height()
            ));
        }
        rects.push((id, r));
    }
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let (a, b) = (&rects[i].1, &rects[j].1);
            let overlap = a.left() < b.right() - 0.5
                && b.left() < a.right() - 0.5
                && a.top() < b.bottom() - 0.5
                && b.top() < a.bottom() - 0.5;
            if overlap {
                bad.push(format!("{} overlaps {}", rects[i].0, rects[j].0));
            }
        }
    }
    (
        bad.is_empty(),
        if bad.is_empty() {
            format!("4 tools inside a {:.0}px-wide viewport", host.width())
        } else {
            bad.join("; ")
        },
    )
}

async fn resize_window(w: i32, h: i32) -> Option<(f64, f64)> {
    #[derive(serde::Serialize)]
    struct A {
        w: i32,
        h: i32,
    }
    let _ = call::<String>("smoke_real_resize", A { w, h }).await;
    let win = web_sys::window()?;
    let before = win.inner_width().ok()?.as_f64()?;
    let _ = wait_ms(3000, || {
        let iw = win.inner_width().ok()?.as_f64()?;
        ((iw - before).abs() > 1.0 || (iw - w as f64).abs() < 2.0).then_some(())
    })
    .await;
    ipc::sleep(500).await;
    Some((
        win.inner_width().ok()?.as_f64()?,
        win.inner_height().ok()?.as_f64()?,
    ))
}

/// Open the menu and click an item, both with real clicks.
async fn menu(item_id: &str) -> Option<()> {
    let b = rect_of("btn-layout")?;
    real(
        "click",
        b.left() + b.width() / 2.0,
        b.top() + b.height() / 2.0,
    )
    .await
    .ok()?;
    wait_ms(2000, || el("#layout-list")).await?;
    ipc::sleep(100).await;
    let i = rect_of(item_id)?;
    real("click", i.left() + 20.0, i.top() + i.height() / 2.0)
        .await
        .ok()?;
    ipc::sleep(300).await;
    Some(())
}

/// The gizmo is drawn and usable for the current selection.
async fn gizmo_ready() -> Option<gz::Layout> {
    wait_ms(10_000, || {
        GIZMO.with_borrow(|g| {
            (g.drag.is_none()
                && !g.committing
                && g.target.is_some()
                && el("#gizmo")
                    .and_then(|e| e.get_attribute("data-state"))
                    .as_deref()
                    == Some("ready"))
            .then_some(g.layout)
            .flatten()
        })
    })
    .await
}

fn grab_x(l: &gz::Layout) -> Option<((f64, f64), (f64, f64))> {
    let h = l.handles[Axis::X.index()];
    let tip = h.tip.filter(|_| h.enabled)?;
    let (dx, dy) = (tip.0 - l.origin.0, tip.1 - l.origin.1);
    let len = dx.hypot(dy);
    Some((
        (l.origin.0 + dx * 0.6, l.origin.1 + dy * 0.6),
        (dx / len, dy / len),
    ))
}

/// Start a real Move drag on the X handle (held), or `None`.
async fn start_move_drag() -> Option<(f64, f64)> {
    let l = gizmo_ready().await?;
    still_camera().await?;
    let (g, d) = grab_x(&l)?;
    real("down", g.0, g.1).await.ok()?;
    ipc::sleep(60).await;
    let mut at = g;
    for k in 1..=5 {
        at = (g.0 + d.0 * 12.0 * k as f64, g.1 + d.1 * 12.0 * k as f64);
        real("move", at.0, at.1).await.ok()?;
        ipc::sleep(40).await;
    }
    ipc::sleep(120).await;
    GIZMO.with_borrow(|g| g.drag.is_some()).then_some(at)
}

fn pressed(id: &str) -> bool {
    crate::element_by_id::<HtmlElement>(id)
        .and_then(|b| b.get_attribute("aria-pressed"))
        .as_deref()
        == Some("true")
}

pub(super) async fn open_fixture(r: &mut R) -> Option<()> {
    #[derive(serde::Serialize)]
    struct A {
        index: usize,
    }
    let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: 0 })
        .await
        .ok()?;
    crate::ui::apply_open(o, "Opened").await;
    let rev = CORE.with_borrow(|c| c.revision);
    let want = format!("rev {rev} · loaded:");
    let shown = wait_ms(20_000, || {
        ui().preview_status
            .get_untracked()
            .starts_with(&want)
            .then_some(())
    })
    .await;
    r.step(
        "fixture opened and previewed",
        shown.is_some(),
        ui().preview_status.get_untracked(),
    );
    Some(())
}

async fn main_phase(c: &Plan, r: &mut R) -> Option<()> {
    let d = dock::dock();
    // 1. Fresh config: the default (Modeling: the 3D viewport first) preset
    // with the Viewport, Scene Tree and Inspector on screen.
    let l0 = layout();
    r.step(
        "startup: the default layout is the Modeling preset (Viewport, Scene Tree, Inspector shown)",
        l0 == Layout::preset(PresetId::Modeling)
            && el("#dock")
                .and_then(|e| e.get_attribute("data-preset"))
                .as_deref()
                == Some("preset.modeling")
            && [PanelId::Viewport, PanelId::SceneTree, PanelId::Inspector]
                .iter()
                .all(|p| host_state(*p) == "active")
            && d.notice.get_untracked().is_none(),
        format!("{:?}", d.notice.get_untracked()),
    );
    // The rest of this phase is written for the Source preset.
    dock::set_preset(PresetId::Source);
    let shown = wait_ms(5000, || {
        (layout() == Layout::preset(PresetId::Source)
            && PanelId::ALL.iter().all(|p| host_state(*p) == "active"))
        .then_some(())
    })
    .await;
    r.step(
        "View → Source preset: all five panels on screen",
        shown.is_some()
            && el("#dock")
                .and_then(|e| e.get_attribute("data-preset"))
                .as_deref()
                == Some("preset.source"),
        format!("{:?}", layout().preset),
    );
    let (ok, detail) = minimums_ok();
    r.step("startup: every panel meets its minimum size", ok, detail);
    let (ok, detail) = tools_visible();
    r.step(
        "header: Select, Move, Rotate and Scale are all visible at 1440 px (no clipping)",
        ok,
        detail,
    );
    if !r.step(
        "real X pointer input is available (harness Xvfb)",
        c.real_pointer,
        "the dock run needs real input; synthetic events are not accepted as proof",
    ) {
        return None;
    }
    open_fixture(r).await?;
    click("btn-select")?;
    ready(10_000).await?;
    calibrate(r).await?;

    // 2. A dirty document with history, a tree selection, a Source
    //    selection and a scroll position.
    let ta = textarea()?;
    let text = ta.value();
    let at = text.encode_utf16().position(|u| u == b'\n' as u16)? as u32;
    let _ = ta.focus();
    ta.set_range_text_with_start_and_end("\n# dock edit", at, at)
        .ok()?;
    let ev = web_sys::InputEvent::new("input").ok()?;
    ta.dispatch_event(&ev).ok()?;
    super::settle().await;
    let s1 = snapshot().await?;
    r.step(
        "a typed edit makes the document dirty with undo history",
        s1.dirty && s1.can_undo && s1.revision >= 1,
        format!("rev {}", s1.revision),
    );
    ready(20_000).await?;
    pick_check("baseline", [0.0, 0.0, 0.0], "Transform Crate", r).await?;
    let ta = textarea()?;
    let v = ta.value();
    let units: Vec<u16> = v.encode_utf16().collect();
    let needle: Vec<u16> = "Ball".encode_utf16().collect();
    let sel = units.windows(4).position(|w| w == needle.as_slice())? as u32;
    let _ = ta.focus();
    let _ = ta.set_selection_range(sel, sel + 4);
    ta.set_scroll_top(140);
    crate::syntax::on_scroll();
    ipc::sleep(200).await;
    set_probes();
    let k0 = keep().await?;
    r.step(
        "baseline: Source selection, scroll, syntax layer and canvas marked",
        k0.doc.selection == (sel, sel + 4) && k0.scroll.0 > 0 && k0.probes,
        format!(
            "{:?} scroll {:?} syntax {}",
            k0.doc.selection, k0.scroll, k0.syntax
        ),
    );

    // 3. Center drop: Inspector into the Viewport's group (a tab group).
    let to = content_point(PanelId::Viewport, 0.5, 0.5)?;
    let (g, untouched, overlay) = dock_drag(PanelId::Inspector, to).await?;
    r.step(
        "drag Inspector over the Viewport: guides show, Center targeted, nothing docked before release",
        g.as_ref().is_some_and(|g| g.0 == "center" && g.1 == "panel.viewport" && g.2 == "true")
            && untouched
            && overlay,
        format!("{g:?} overlay {overlay}"),
    );
    let l = layout();
    r.step(
        "release: Inspector and Viewport form one tab group (Inspector in front)",
        l.group_of(PanelId::Viewport)
            == Some((
                vec![PanelId::Viewport, PanelId::Inspector],
                PanelId::Inspector,
            ))
            && el("#dock-overlay").is_none()
            && host_state(PanelId::Viewport) == "inactive",
        format!("{:?}", l.group_of(PanelId::Viewport)),
    );
    // UI-MENU-1 (PR #142 review): the moved tab keeps the keyboard focus.
    let moved = wait_ms(1000, || (active_id() == "dock-tab-inspector").then_some(())).await;
    r.step(
        "after a drop the moved tab (Inspector) has the keyboard focus, not the document body",
        moved.is_some(),
        active_id(),
    );
    let k = keep().await?;
    r.step(
        "center drop: text, revision, dirty, history, selections, scroll, canvas, preview unchanged",
        k == k0,
        diff(&k0, &k),
    );

    // 4. Tab switch with an active X_ITE scene.
    let (x, y) = tab_center(PanelId::Viewport)?;
    real("click", x, y).await.ok()?;
    ipc::sleep(300).await;
    let k = keep().await?;
    r.step(
        "tab click: the Viewport comes to the front; the Inspector keeps its state",
        host_state(PanelId::Viewport) == "active"
            && host_state(PanelId::Inspector) == "inactive"
            && k == k0,
        diff(&k0, &k),
    );
    pick_check("after a tab switch", [3.0, 0.0, 0.0], "Transform Ball", r).await?;
    pick_check("after a tab switch", [0.0, 0.0, 0.0], "Transform Crate", r).await?;
    let ta = textarea()?;
    let _ = ta.set_selection_range(sel, sel + 4);
    let k0 = keep().await?;

    // 5. Tab reorder: real drag in the strip, then keyboard.
    let strip = rect_of("dock-tab-inspector")?;
    let (g, _, _) = dock_drag(
        PanelId::Viewport,
        (strip.right() + 30.0, strip.top() + strip.height() / 2.0),
    )
    .await?;
    let l = layout();
    r.step(
        "tab drag along the strip reorders the group",
        l.group_of(PanelId::Viewport).map(|g| g.0)
            == Some(vec![PanelId::Inspector, PanelId::Viewport])
            && g.as_ref().is_some_and(|g| g.0 == "center" && g.2 == "true"),
        format!("{:?} guide {g:?}", l.group_of(PanelId::Viewport)),
    );
    let tab: HtmlElement = crate::element_by_id("dock-tab-viewport")?;
    let _ = tab.focus();
    key("shift+Left").await?;
    let l = layout();
    let order_ok = l.group_of(PanelId::Viewport).map(|g| g.0)
        == Some(vec![PanelId::Viewport, PanelId::Inspector]);
    key("Right").await?;
    let right_ok = host_state(PanelId::Inspector) == "active"
        && super::create::all(":focus")
            .first()
            .map(|e| e.id())
            .as_deref()
            == Some("dock-tab-inspector");
    key("Left").await?;
    let left_ok = host_state(PanelId::Viewport) == "active";
    let k = keep().await?;
    r.step(
        "keyboard: Shift+Left reorders, Right / Left switch tabs (focus follows)",
        order_ok && right_ok && left_ok && k == k0,
        format!(
            "order {order_ok} right {right_ok} left {left_ok} · {}",
            diff(&k0, &k)
        ),
    );

    // 6. Edge drop: Scene Tree to the right of the Source.
    let to = content_point(PanelId::Source, 0.95, 0.5)?;
    let (g, untouched, _) = dock_drag(PanelId::SceneTree, to).await?;
    let geo = d.geo.get_untracked();
    let (st, src) = (
        geo.group_of(PanelId::SceneTree)?.rect,
        geo.group_of(PanelId::Source)?.rect,
    );
    let k = keep().await?;
    r.step(
        "edge drop (right): a split puts the Scene Tree right of the Source",
        g.as_ref()
            .is_some_and(|g| g.0 == "right" && g.1 == "panel.source" && g.2 == "true")
            && untouched
            && st.x > src.x
            && (st.y - src.y).abs() < 1.0
            && k == k0,
        format!("{g:?} · {}", diff(&k0, &k)),
    );

    // 7. Invalid drop: the Source onto itself.
    let before = layout();
    let refused = STATS.with_borrow(|s| s.refused);
    let to = content_point(PanelId::Source, 0.5, 0.5)?;
    let (g, _, _) = dock_drag(PanelId::Source, to).await?;
    let k = keep().await?;
    r.step(
        "invalid drop: marked invalid while dragging; the layout is unchanged",
        g.as_ref().is_some_and(|g| g.2 == "false")
            && layout() == before
            && STATS.with_borrow(|s| s.refused) == refused + 1
            && k == k0,
        format!("{g:?} · {}", diff(&k0, &k)),
    );

    // 8. Escape cancels a drag; the release afterwards does nothing.
    let before = layout();
    let cancels = STATS.with_borrow(|s| s.cancels);
    let from = tab_center(PanelId::Diagnostics)?;
    let to = content_point(PanelId::Viewport, 0.1, 0.5)?;
    press_drag(from, to, 8).await?;
    let mid = guide();
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(150).await;
    let gone = el("#dock-overlay").is_none();
    real("move", to.0 + 10.0, to.1).await.ok()?;
    release((to.0 + 10.0, to.1)).await?;
    let k = keep().await?;
    r.step(
        "Escape cancels a docking drag; the later release docks nothing",
        mid.as_ref().is_some_and(|g| g.0 == "left")
            && gone
            && layout() == before
            && STATS.with_borrow(|s| (s.cancels, s.last_cancel.clone()))
                == (cancels + 1, Some("Escape".into()))
            && k == k0,
        format!("{mid:?} · {}", diff(&k0, &k)),
    );

    // 9. Reset Layout from the toolbar menu (real clicks).
    menu("layout-view-resetLayout").await?;
    let k = keep().await?;
    r.step(
        "Reset Layout restores the Source preset; the document is unchanged",
        layout() == Layout::preset(PresetId::Source) && k == k0,
        diff(&k0, &k),
    );
    saved().await;

    // 10. Divider drags (real), with the viewport resized and still exact.
    let geo = d.geo.get_untracked();
    let div = geo.divider(&[], 1)?.clone();
    let canvas0 = rect_of("viewport")?;
    let (dx, dy) = (div.rect.x + div.rect.w / 2.0, div.rect.y + div.rect.h / 2.0);
    let o = rect_of("dock")?;
    let from = (o.left() + dx, o.top() + dy);
    press_drag(from, (from.0 - 150.0, from.1), 6).await?;
    release((from.0 - 150.0, from.1)).await?;
    ipc::sleep(400).await;
    let canvas1 = rect_of("viewport")?;
    let host = rect_of("dock-panel-viewport")?;
    let k = keep().await?;
    r.step(
        "divider drag: the Viewport widens by 150 px; X_ITE follows without a reload",
        ((canvas1.width() - canvas0.width()) - 150.0).abs() <= 2.0
            && (canvas1.width() - host.width()).abs() <= 1.0
            && k == k0,
        format!(
            "canvas {:.0} -> {:.0} (host {:.0}) · {}",
            canvas0.width(),
            canvas1.width(),
            host.width(),
            diff(&k0, &k)
        ),
    );
    pick_check("after a resize", [3.0, 0.0, 0.0], "Transform Ball", r).await?;
    pick_check("after a resize", [0.0, 0.0, 0.0], "Transform Crate", r).await?;
    let ta = textarea()?;
    let _ = ta.set_selection_range(sel, sel + 4);
    let k0 = keep().await?;
    // Clamp: far left, then a vertical divider far down.
    let geo = d.geo.get_untracked();
    let div = geo.divider(&[], 0)?.clone();
    let from = (
        o.left() + div.rect.x + 3.0,
        o.top() + div.rect.y + div.rect.h / 2.0,
    );
    press_drag(from, (o.left() + 5.0, from.1), 6).await?;
    release((o.left() + 5.0, from.1)).await?;
    let st = d.geo.get_untracked().group_of(PanelId::SceneTree)?.content;
    let geo = d.geo.get_untracked();
    let vdiv = geo
        .dividers
        .iter()
        .find(|v| v.axis == SplitAxis::Column && v.path == vec![1])?
        .clone();
    let from = (
        o.left() + vdiv.rect.x + vdiv.rect.w / 2.0,
        o.top() + vdiv.rect.y + 3.0,
    );
    let far = (from.0, (o.top() + geo.height - 2.0).min(990.0));
    press_drag(from, far, 6).await?;
    release(far).await?;
    let dg = d
        .geo
        .get_untracked()
        .group_of(PanelId::Diagnostics)?
        .content;
    let k = keep().await?;
    r.step(
        "dividers clamp at the minimums (Scene Tree 200 px wide, Diagnostics 100 px high)",
        (st.w - 200.0).abs() <= 1.0 && (dg.h - 100.0).abs() <= 1.0 && k == k0,
        format!(
            "scene tree {:.1} · diagnostics {:.1} · {}",
            st.w,
            dg.h,
            diff(&k0, &k)
        ),
    );

    // 11. Keyboard divider operation.
    let div_el: HtmlElement = crate::element_by_id("dock-div--0")?;
    let _ = div_el.focus();
    let w0 = d
        .geo
        .get_untracked()
        .group_of(PanelId::SceneTree)?
        .content
        .w;
    key("Right").await?;
    let w1 = d
        .geo
        .get_untracked()
        .group_of(PanelId::SceneTree)?
        .content
        .w;
    key("End").await?;
    let w2 = d
        .geo
        .get_untracked()
        .group_of(PanelId::SceneTree)?
        .content
        .w;
    key("Home").await?;
    let w3 = d
        .geo
        .get_untracked()
        .group_of(PanelId::SceneTree)?
        .content
        .w;
    let now = div_el.get_attribute("aria-valuenow").unwrap_or_default();
    let k = keep().await?;
    r.step(
        "keyboard divider: Right +16 px, End to the limit, Home back to the minimum",
        (w1 - w0 - lay::KEY_STEP_PX).abs() <= 1.0
            && w2 > w1 + 50.0
            && (w3 - 200.0).abs() <= 1.0
            && !now.is_empty()
            && k == k0,
        format!(
            "{w0:.0} -> {w1:.0} -> {w2:.0} -> {w3:.0} (aria {now}) · {}",
            diff(&k0, &k)
        ),
    );

    // 12. Workspace changes during an active Move drag cancel it safely.
    click("btn-move")?;
    let before = super::gizmo::source().await?;
    let (cancels, commits) = GIZMO.with_borrow(|g| (g.cancels, g.commits));
    let at = start_move_drag().await;
    let dragging = at.is_some();
    crate::command::run(p::command::CommandId::ViewPresetModeling);
    ipc::sleep(150).await;
    let canceled = GIZMO.with_borrow(|g| {
        g.drag.is_none()
            && g.cancels == cancels + 1
            && g.last_cancel.as_deref() == Some("the layout changed")
    });
    if let Some(at) = at {
        release(at).await?;
    }
    ipc::sleep(600).await;
    let after = super::gizmo::source().await?;
    r.step(
        "a layout change during a Move drag cancels it: no source edit, no undo step, Move stays on",
        dragging
            && canceled
            && after == before
            && GIZMO.with_borrow(|g| g.commits) == commits
            && pressed("btn-move")
            && layout().preset == PresetId::Modeling,
        format!(
            "dragging {dragging} canceled {canceled} last {:?} · rev {} -> {}",
            GIZMO.with_borrow(|g| g.last_cancel.clone()),
            before.revision,
            after.revision
        ),
    );
    // The handles are redrawn at the object in the NEW viewport bounds.
    let l = gizmo_ready().await;
    let cam = ipc::gizmo_camera()?;
    let want = gz::project(&cam, [0.0, 0.0, 0.0])?;
    let vp = rect_of("viewport")?;
    r.step(
        "after the change the gizmo is redrawn at the object inside the new viewport bounds",
        l.is_some_and(|l| (l.origin.0 - want.0).abs() < 1.0 && (l.origin.1 - want.1).abs() < 1.0)
            && vp.left() <= want.0
            && want.0 <= vp.right()
            && vp.top() <= want.1
            && want.1 <= vp.bottom(),
        format!(
            "{:?} vs {want:?} · viewport {:.0}x{:.0}",
            l.map(|l| l.origin),
            vp.width(),
            vp.height()
        ),
    );
    // A window resize during a drag.
    let before = super::gizmo::source().await?;
    let cancels = GIZMO.with_borrow(|g| g.cancels);
    let at = start_move_drag().await;
    let dragging = at.is_some();
    let size = resize_window(1400, 880).await;
    let canceled = GIZMO.with_borrow(|g| g.drag.is_none() && g.cancels > cancels);
    let reason = GIZMO.with_borrow(|g| g.last_cancel.clone());
    if let Some(at) = at {
        release(at).await?;
    }
    ipc::sleep(600).await;
    let after = super::gizmo::source().await?;
    r.step(
        "a window resize during a Move drag cancels it with no source edit",
        dragging && canceled && after == before && pressed("btn-move"),
        format!("{size:?} {reason:?}"),
    );
    resize_window(1440, 900).await;
    click("btn-select")?;
    pick_check("Modeling preset", [3.0, 0.0, 0.0], "Transform Ball", r).await?;
    pick_check("Modeling preset", [0.0, 0.0, 0.0], "Transform Crate", r).await?;

    // 13. Presets.
    let ta = textarea()?;
    let _ = ta.set_selection_range(sel, sel + 4);
    let k0 = keep().await?;
    let geo = d.geo.get_untracked();
    let vp_area = geo
        .group_of(PanelId::Viewport)
        .map(|g| g.rect.w * g.rect.h)?;
    r.step(
        "Modeling preset: the Viewport is the largest; Source is a tab beside Diagnostics",
        layout() == Layout::preset(PresetId::Modeling)
            && geo
                .groups
                .iter()
                .all(|g| g.rect.w * g.rect.h <= vp_area + 1.0)
            && layout().group_of(PanelId::Source).map(|g| g.0)
                == Some(vec![PanelId::Diagnostics, PanelId::Source]),
        format!("{:?}", layout().group_of(PanelId::Source)),
    );
    let (ok, detail) = tools_visible();
    r.step("Modeling preset: all four tools visible", ok, detail);
    menu("layout-view-preset-source").await?;
    let k = keep().await?;
    r.step(
        "Source preset from the menu; the document, selection and preview are unchanged",
        layout() == Layout::preset(PresetId::Source) && k == k0,
        diff(&k0, &k),
    );
    menu("layout-view-preset-modeling").await?;
    let k = keep().await?;
    r.step(
        "Modeling preset from the menu; the document, selection and preview are unchanged",
        layout() == Layout::preset(PresetId::Modeling) && k == k0,
        diff(&k0, &k),
    );
    let (x, y) = tab_center(PanelId::Source)?;
    real("click", x, y).await.ok()?;
    ipc::sleep(300).await;
    let k = keep().await?;
    let ta = textarea()?;
    r.step(
        "Modeling: the Source tab shows the SAME textarea, selection and scroll",
        host_state(PanelId::Source) == "active"
            && k == k0
            && ta.selection_start().ok().flatten() == Some(sel),
        diff(&k0, &k),
    );

    // 14. Close and restore, with an unapplied Inspector value kept.
    menu("layout-view-preset-source").await?;
    tree_click("Transform Crate", || {
        ui().inspection
            .get_untracked()
            .is_some_and(|i| i.title.contains("Crate"))
    })
    .await;
    // Let the selection's own Inspector refresh finish first.
    super::settle().await;
    ipc::sleep(800).await;
    let insp0 = ui().inspection.get_untracked();
    let input: web_sys::HtmlInputElement = el("tr[data-field=\"translation\"] input")?
        .dyn_into()
        .ok()?;
    input.set_value("7.25");
    let rev = CORE.with_borrow(|c| c.revision);
    let tab: HtmlElement = crate::element_by_id("dock-tab-inspector")?;
    let _ = tab.focus();
    let group: Vec<PanelId> = layout()
        .group_of(PanelId::Inspector)
        .map(|(g, _)| g)
        .unwrap_or_default();
    key("Delete").await?;
    let hidden =
        !layout().is_visible(PanelId::Inspector) && host_state(PanelId::Inspector) == "hidden";
    // UI-MENU-1 (PR #142 review): the focus goes to the group's new active
    // tab (or the Source tab when the group is gone), never to the body.
    let now = active_id();
    let next_ok = now.strip_prefix("dock-tab-").is_some_and(|slug| {
        group
            .iter()
            .filter(|p| **p != PanelId::Inspector)
            .any(|p| p.slug() == slug && layout().is_on_screen(*p))
            || (slug == PanelId::Source.slug() && group.len() == 1)
    });
    r.step(
        "Delete on a focused tab moves the focus to the tab now active in its group (not the document body)",
        hidden && next_ok,
        format!("group {group:?} focus {now:?}"),
    );
    menu("layout-panel-inspector").await?;
    let input2: web_sys::HtmlInputElement = el("tr[data-field=\"translation\"] input")?
        .dyn_into()
        .ok()?;
    r.step(
        "Inspector: Delete hides it, Show restores it; the unapplied value is kept, never applied",
        hidden
            && layout().is_visible(PanelId::Inspector)
            && input2.value() == "7.25"
            && CORE.with_borrow(|c| c.revision) == rev
            && input == input2
            && ui().inspection.get_untracked() == insp0,
        format!(
            "hidden {hidden} value {:?} rev {rev} same input {} same inspection {}",
            input2.value(),
            input == input2,
            ui().inspection.get_untracked() == insp0
        ),
    );
    let close = rect_of("dock-tab-diagnostics")?;
    let xb = el("#dock-tab-diagnostics .dock-tab-close")?.get_bounding_client_rect();
    real(
        "click",
        xb.left() + xb.width() / 2.0,
        xb.top() + xb.height() / 2.0,
    )
    .await
    .ok()?;
    ipc::sleep(300).await;
    let diag_hidden = !layout().is_visible(PanelId::Diagnostics);
    menu("layout-panel-sceneTree").await?;
    let tree_hidden = !layout().is_visible(PanelId::SceneTree);
    let none_required = el("#dock-tab-viewport .dock-tab-close").is_none()
        && el("#dock-tab-source .dock-tab-close").is_none();
    menu("layout-panel-diagnostics").await?;
    menu("layout-panel-sceneTree").await?;
    let l = layout();
    let snap = snapshot().await?;
    r.step(
        "close (× button, menu) and restore: every panel returns exactly once; Viewport and Source have no close",
        diag_hidden
            && tree_hidden
            && none_required
            && l.visible().len() == 5
            && l.validate().is_ok()
            && snap.revision == rev
            && close.width() > 0.0,
        format!("{:?}", l.visible()),
    );
    // Restored at the preset positions: equal to the preset.
    r.step(
        "restored panels return to their Source-preset places",
        l.approx_eq(&Layout::preset(PresetId::Source), 1e-9),
        format!("{:?}", l.to_wire()),
    );

    // 15. Narrow windows: overflow, never collapse; tools reachable.
    let mut narrow = vec![];
    for (w, h) in [(900, 560), (960, 600), (1100, 700)] {
        let got = resize_window(w, h).await?;
        let (ok, detail) = minimums_ok();
        let (tok, tdetail) = tools_visible();
        let ws: HtmlElement = crate::element_by_id("workspace")?;
        let scroll = ws.scroll_width() >= d.geo.get_untracked().width as i32 - 1;
        // The toolbar wraps: the page itself never scrolls sideways.
        let page = web_sys::window()?.document()?.document_element()?;
        let no_page_scroll = page.scroll_width() as f64 <= got.0 + 1.0;
        // The preview status line is not pushed out of the viewport panel.
        let host = rect_of("dock-panel-viewport")?;
        let status = rect_of("preview-status")?;
        let status_ok = status.height() > 0.0 && status.bottom() <= host.bottom() + 0.5;
        narrow.push((
            ok && tok && scroll && no_page_scroll && status_ok,
            format!(
                "{got:?}: {detail} · {tdetail} · page {} · status bottom {:.0} / host {:.0}",
                page.scroll_width(),
                status.bottom(),
                host.bottom()
            ),
        ));
    }
    r.step(
        "narrow windows: every panel keeps its minimum (the workspace scrolls); tools stay visible",
        narrow.iter().all(|n| n.0),
        narrow
            .iter()
            .map(|n| n.1.clone())
            .collect::<Vec<_>>()
            .join(" | "),
    );
    resize_window(1440, 900).await;

    // 15b. STACK-REVIEW-1: every snap guide is the box the panel gets.
    snap_guides(r).await?;
    menu("layout-view-preset-source").await?;

    // 16. A distinctive layout for the restart: Modeling + Inspector left
    //     of the Viewport.
    menu("layout-view-preset-modeling").await?;
    let to = content_point(PanelId::Viewport, 0.05, 0.5)?;
    let (g, _, _) = dock_drag(PanelId::Inspector, to).await?;
    saved().await;
    let geo = d.geo.get_untracked();
    let ok = geo.group_of(PanelId::Inspector)?.rect.x < geo.group_of(PanelId::Viewport)?.rect.x;
    r.step(
        "final layout: Modeling with the Inspector docked left of the Viewport (saved)",
        ok && g.as_ref().is_some_and(|g| g.0 == "left")
            && STATS.with_borrow(|s| s.save_failures) == 0,
        format!(
            "{:?} saves {}",
            layout().to_wire(),
            STATS.with_borrow(|s| s.saves)
        ),
    );
    // Undo the edit: the buffer is the file again, byte for byte.
    let ta = textarea()?;
    let _ = ta.focus();
    super::key("z", false);
    super::settle().await;
    let snap = snapshot().await?;
    r.step(
        "Undo after all workspace changes restores the exact original text",
        !snap.dirty && snap.view == text && textarea()?.value() == text,
        format!("rev {} dirty {}", snap.revision, snap.dirty),
    );
    Some(())
}

/// The tolerance between a snap guide and the panel box after the drop,
/// in CSS pixels (both are placed from the same solved f64 rectangle; the
/// slack only covers sub-pixel rounding of the bounding boxes).
const GUIDE_TOL_PX: f64 = 1.0;

fn dom_rect(id: &str) -> Option<lay::Rect> {
    let r = rect_of(id)?;
    Some(lay::Rect {
        x: r.left(),
        y: r.top(),
        w: r.width(),
        h: r.height(),
    })
}

fn max_diff(a: lay::Rect, b: lay::Rect) -> f64 {
    [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
        .iter()
        .fold(0.0_f64, |m, d| m.max(d.abs()))
}

/// One real drag: the guide box just before the release, the moved panel's
/// box after it, and their largest edge difference (CSS px).
async fn guide_vs_result(
    preset: PresetId,
    panel: PanelId,
    anchor: PanelId,
    fx: f64,
    fy: f64,
) -> Option<(bool, String)> {
    dock::set_preset(preset);
    ipc::sleep(300).await;
    let before = layout();
    let to = content_point(anchor, fx, fy)?;
    let from = tab_center(panel)?;
    press_drag(from, to, 8).await?;
    let (zone, _, valid) = guide()?;
    let g = dom_rect("dock-guide")?;
    let untouched = layout() == before;
    release(to).await?;
    ipc::sleep(150).await;
    let after = layout();
    let host = dom_rect(&format!("dock-panel-{}", panel.slug()))?;
    let d = max_diff(g, host);
    let focus = active_id();
    let ok = valid == "true"
        && untouched
        && after != before
        && d <= GUIDE_TOL_PX
        && focus == format!("dock-tab-{}", panel.slug());
    Some((
        ok,
        format!(
            "{:?} {:?}->{:?} {zone}: guide {:.1},{:.1} {:.1}x{:.1} panel {:.1},{:.1} {:.1}x{:.1} diff {d:.2}px focus {focus}",
            preset, panel, anchor, g.x, g.y, g.w, g.h, host.x, host.y, host.w, host.h
        ),
    ))
}

/// STACK-REVIEW-1 (PR #142 r4242637970): a valid drop's snap guide is the
/// content box the moved panel has after the release, within GUIDE_TOL_PX;
/// an invalid target keeps the zone-half fallback and changes nothing.
async fn snap_guides(r: &mut R) -> Option<()> {
    use PanelId::*;
    let k0 = keep().await?;
    let mut worst = 0.0_f64;
    let cases: [(&str, PresetId, PanelId, PanelId, f64, f64); 9] = [
        (
            "edge left (last tab leaves a split)",
            PresetId::Source,
            Inspector,
            Viewport,
            0.08,
            0.5,
        ),
        (
            "edge right",
            PresetId::Source,
            Inspector,
            Viewport,
            0.92,
            0.5,
        ),
        ("edge top", PresetId::Source, Inspector, Viewport, 0.5, 0.08),
        (
            "edge bottom",
            PresetId::Source,
            Inspector,
            Viewport,
            0.5,
            0.92,
        ),
        (
            "edge bottom, neighbor group stays",
            PresetId::Source,
            Diagnostics,
            Viewport,
            0.5,
            0.92,
        ),
        (
            "merge into a group (center)",
            PresetId::Source,
            Diagnostics,
            Source,
            0.5,
            0.5,
        ),
        (
            "split another group",
            PresetId::Source,
            SceneTree,
            Source,
            0.92,
            0.5,
        ),
        (
            "out of a two-tab group (one tab left)",
            PresetId::Modeling,
            Source,
            Viewport,
            0.92,
            0.5,
        ),
        (
            "into a two-tab group's edge",
            PresetId::Modeling,
            Inspector,
            Diagnostics,
            0.5,
            0.92,
        ),
    ];
    for (name, preset, panel, anchor, fx, fy) in cases {
        let (ok, detail) = guide_vs_result(preset, panel, anchor, fx, fy).await?;
        if let Some(d) = detail
            .split("diff ")
            .nth(1)
            .and_then(|t| t.split("px").next())
            .and_then(|t| t.parse::<f64>().ok())
        {
            worst = worst.max(d);
        }
        r.step(
            &format!("snap guide == final panel box ({name}, tol {GUIDE_TOL_PX} px)"),
            ok,
            detail,
        );
    }
    // Minimum sizes: a narrow window, where half of the Viewport is
    // narrower than the Scene Tree's minimum.
    resize_window(1000, 760).await?;
    let (ok, detail) = guide_vs_result(PresetId::Source, SceneTree, Viewport, 0.08, 0.5).await?;
    if let Some(d) = detail
        .split("diff ")
        .nth(1)
        .and_then(|t| t.split("px").next())
        .and_then(|t| t.parse::<f64>().ok())
    {
        worst = worst.max(d);
    }
    r.step(
        &format!("snap guide == final panel box (minimum width binds, tol {GUIDE_TOL_PX} px)"),
        ok,
        detail,
    );
    resize_window(1440, 900).await?;
    // An invalid target: the zone-half fallback, nothing docks.
    dock::set_preset(PresetId::Source);
    ipc::sleep(300).await;
    let before = layout();
    let refused0 = STATS.with_borrow(|s| s.refused);
    let content = dock::dock().geo.get_untracked().group_of(Source)?.content;
    let to = content_point(Source, 0.08, 0.5)?;
    press_drag(tab_center(Source)?, to, 8).await?;
    let (zone, _, valid) = guide()?;
    let g = dom_rect("dock-guide")?;
    let o = rect_of("dock")?;
    let half = lay::zone_rect(content, Zone::Left);
    let fallback = lay::Rect {
        x: o.left() + half.x,
        y: o.top() + half.y,
        ..half
    };
    release(to).await?;
    r.step(
        "an invalid target shows the zone-half fallback guide and docks nothing",
        valid == "false"
            && zone == "left"
            && max_diff(g, fallback) <= GUIDE_TOL_PX
            && layout() == before
            && STATS.with_borrow(|s| s.refused) == refused0 + 1,
        format!("valid {valid} diff {:.2}px", max_diff(g, fallback)),
    );
    let k1 = keep().await?;
    r.step(
        "the snap-guide drags changed no document state, selection, preview or panel object",
        k1 == k0,
        diff(&k0, &k1),
    );
    r.step(
        &format!("SNAP_GUIDE_MAX_DIFF_PX = {worst:.2} (tolerance {GUIDE_TOL_PX})"),
        worst <= GUIDE_TOL_PX,
        format!("{worst}"),
    );
    pick_check(
        "after the snap-guide drags",
        [0.0, 0.0, 0.0],
        "Transform Crate",
        r,
    )
    .await?;
    Some(())
}

async fn restore_phase(c: &Plan, r: &mut R) -> Option<()> {
    let d = dock::dock();
    let want = c.expected.clone();
    let got = wait_ms(5000, || {
        let w = layout().to_wire();
        (Some(&w) == want.as_ref()).then_some(w)
    })
    .await;
    r.step(
        "restart: the saved layout is restored exactly, with no notice",
        got.is_some() && d.notice.get_untracked().is_none(),
        format!(
            "{:?} notice {:?}",
            layout().to_wire(),
            d.notice.get_untracked()
        ),
    );
    let (ok, detail) = minimums_ok();
    r.step("restart: every panel meets its minimum size", ok, detail);
    let preset = want
        .as_ref()
        .and_then(|w| PresetId::from_key(&w.preset))
        .unwrap_or(PresetId::Source);
    crate::command::run(p::command::CommandId::ViewResetLayout);
    saved().await;
    r.step(
        "restart: Reset Layout restores the saved layout's preset",
        layout() == Layout::preset(preset),
        format!("{:?}", layout().preset),
    );
    Some(())
}

async fn damaged_phase(r: &mut R) -> Option<()> {
    let d = dock::dock();
    let notice = wait_ms(5000, || el("#layout-notice").and_then(|e| e.text_content())).await;
    let theme = web_sys::window()?
        .document()?
        .document_element()?
        .get_attribute("data-theme");
    r.step(
        "damaged layout: startup succeeds with the default preset and a clear notice",
        layout() == Layout::default_layout()
            && notice
                .as_deref()
                .is_some_and(|n| n.contains("invalid") && n.contains(&format!("{} layout", p::layout::PresetId::DEFAULT.label()))),
        format!("{notice:?}"),
    );
    r.step(
        "damaged layout: the theme in the same file still loads",
        theme.as_deref() == Some("tokyo-night-storm"),
        format!("{theme:?}"),
    );
    // A real change replaces it and clears the notice.
    let t = DropTarget {
        anchor: PanelId::Viewport,
        zone: Zone::Center,
        index: None,
    };
    let n = layout().dock(PanelId::Inspector, t).ok()?;
    dock::apply(n, true);
    saved().await;
    r.step(
        "damaged layout: the next change is saved and the notice clears",
        d.notice.get_untracked().is_none() && STATS.with_borrow(|s| s.saves) >= 1,
        format!("{:?}", d.notice.get_untracked()),
    );
    Some(())
}

pub async fn run(c: &Plan, r: &mut R) -> Option<()> {
    // The UI is mounted and the saved layout loaded.
    wait_ms(5000, || crate::element_by_id::<HtmlElement>("btn-layout")).await?;
    ipc::sleep(800).await;
    match c.phase {
        Phase::Main => main_phase(c, r).await?,
        Phase::Restore => restore_phase(c, r).await?,
        Phase::Damaged => damaged_phase(r).await?,
    }
    if matches!(c.phase, Phase::Main) {
        r.step(
            "WRONG_SOURCE_SELECTIONS = 0 over the whole run",
            WRONG.get() == 0 && PICKS.get() > 0,
            format!("{} picks, {} wrong", PICKS.get(), WRONG.get()),
        );
    }
    r.step(
        "dock counters",
        true,
        format!("{:?}", STATS.with_borrow(|s| s.clone())),
    );
    Some(())
}
