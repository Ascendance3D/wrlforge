// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-2 in-window picking run (`--smoke-pick`).
//!
//! Part 1, once per built-in theme: New World → Create Box, Sphere, Cylinder,
//! Cone (Inspector moves each to its own place) → Select → click every object
//! in the REAL rendered viewport → Scene Tree + Inspector show it, the source
//! is untouched → empty click → Transform and Material edits through the
//! Inspector reach the viewport → stale clicks (edit pending, preview
//! replaced while a pick is in flight, last valid scene after a syntax error)
//! select nothing → Undo / Redo / Save As.
//!
//! Part 2: the WD2-C0 oracle fixture matrix (P1–P22 plus CRLF / CR / BOM /
//! Unicode forms), each click at the oracle's projected world point.
//!
//! A click is a primary-button press + release dispatched on the viewport
//! element, so it runs the production gesture handler and X_ITE's real hit
//! test on the real rendered scene. `WRONG_SOURCE_SELECTIONS` counts every
//! PROVEN result whose selected source span is not the oracle's.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;
use wrlforge_desktop_protocol as p;

use super::create::{all, apply_field, previewed, text, tree_item, wait_ms};
use super::{choose, click, doc_el, snapshot, R};
use crate::editor::{self, textarea, CORE};
use crate::ipc::{self, call};
use crate::pick::PICK;
use crate::ui::{self, ui};

/// The default VRML97 Viewpoint (no Viewpoint node): ISO 14772-1 6.53.
const DEFAULT_CAMERA: [f64; 3] = [0.0, 0.0, 10.0];
const FOV: f64 = std::f64::consts::FRAC_PI_4;

#[derive(Default)]
struct Stats {
    picks: u32,
    proven: u32,
    refused: u32,
    no_hit: u32,
    wrong: u32,
    mismatched: u32,
    skipped: u32,
    snapshot_ms: Vec<f64>,
    total_ms: Vec<f64>,
}

impl Stats {
    fn record(&mut self, o: &p::PickOutcome) {
        self.picks += 1;
        match o.status.as_str() {
            "PROVEN" => self.proven += 1,
            "NO_HIT" => self.no_hit += 1,
            _ => self.refused += 1,
        }
        PICK.with_borrow(|p| {
            self.snapshot_ms.push(p.last_snapshot_ms);
            self.total_ms.push(p.last_total_ms);
        });
    }
}

fn pct(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f64 * q).round() as usize]
}

pub(super) fn viewport() -> Option<HtmlElement> {
    crate::element_by_id("viewport")
}

/// World point → client px through the bound camera (VRML97 perspective;
/// fieldOfView spans the smaller viewport side). Independent of X_ITE.
fn project(world: [f64; 3], cam: [f64; 3]) -> Option<(f64, f64)> {
    let r = viewport()?.get_bounding_client_rect();
    let (w, h) = (r.width(), r.height());
    let depth = cam[2] - world[2];
    let k = (w.min(h) / 2.0) / (depth * (FOV / 2.0).tan());
    Some((
        r.left() + w / 2.0 + (world[0] - cam[0]) * k,
        r.top() + h / 2.0 - (world[1] - cam[1]) * k,
    ))
}

fn pointer(kind: &str, x: f64, y: f64) {
    let init = web_sys::PointerEventInit::new();
    init.set_pointer_id(1);
    init.set_is_primary(true);
    init.set_pointer_type("mouse");
    init.set_client_x(x as i32);
    init.set_client_y(y as i32);
    init.set_button(0);
    init.set_buttons(if kind == "pointerdown" { 1 } else { 0 });
    init.set_bubbles(true);
    init.set_composed(true);
    init.set_cancelable(true);
    if let (Some(v), Ok(ev)) = (
        viewport(),
        web_sys::PointerEvent::new_with_event_init_dict(kind, &init),
    ) {
        let _ = v.dispatch_event(&ev);
    }
}

thread_local! {
    /// `Some(offset)` once real X pointer input is calibrated: the client
    /// point a real click lands on minus the point requested.
    pub(super) static REAL: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
}

/// GTK's double-click interval is 400 ms; real clicks are spaced further
/// apart so X_ITE never sees a double click (which would move the camera).
pub(super) const REAL_CLICK_GAP_MS: i32 = 550;

async fn real_click(x: f64, y: f64) -> Result<String, String> {
    #[derive(serde::Serialize)]
    struct A {
        x: f64,
        y: f64,
    }
    call::<String>("smoke_real_click", A { x, y }).await
}

/// Click (x, y) through the production gesture handler: a REAL X click when
/// calibrated, else a dispatched press + release. The outcome it applied,
/// or `None` (no pick requested / superseded / timeout).
pub(super) async fn gesture(x: f64, y: f64) -> Option<p::PickOutcome> {
    let done = PICK.with_borrow(|p| p.completed);
    match REAL.get() {
        Some((dx, dy)) => {
            real_click(x - dx, y - dy).await.ok()?;
        }
        None => {
            pointer("pointerdown", x, y);
            pointer("pointerup", x, y);
        }
    }
    let limit = if REAL.get().is_some() { 3000 } else { 10_000 };
    wait_ms(limit, || {
        (PICK.with_borrow(|p| p.completed) > done).then_some(())
    })
    .await?;
    if REAL.get().is_some() {
        ipc::sleep(REAL_CLICK_GAP_MS).await;
    }
    PICK.with_borrow(|p| p.last.clone())
}

/// Calibrate real X input on the viewport: one click at its center, compare
/// where the page received it. A second click must land within 1 px.
pub(super) async fn calibrate(r: &mut R) -> Option<()> {
    let rect = viewport()?.get_bounding_client_rect();
    let (x, y) = (
        rect.left() + rect.width() / 2.0,
        rect.top() + rect.height() / 2.0,
    );
    let done = PICK.with_borrow(|p| p.completed);
    let sent = real_click(x, y).await;
    let got = wait_ms(5000, || {
        (PICK.with_borrow(|p| p.completed) > done).then(|| PICK.with_borrow(|p| p.last_click))?
    })
    .await;
    ipc::sleep(REAL_CLICK_GAP_MS).await;
    let Some((gx, gy)) = got else {
        r.step(
            "real pointer: a real X click reaches the viewport",
            false,
            format!("{sent:?}"),
        );
        return None;
    };
    REAL.set(Some((gx - x, gy - y)));
    let (x2, y2) = (x + 37.0, y - 23.0);
    let _ = gesture(x2, y2).await;
    let land = PICK.with_borrow(|p| p.last_click).unwrap_or_default();
    let ok = (land.0 - x2).abs() <= 1.0 && (land.1 - y2).abs() <= 1.0;
    r.step(
        "real pointer: real X clicks (harness Xvfb, own window, focused) land on the requested viewport point",
        ok,
        format!(
            "{} · offset ({:.0}, {:.0}) · check ({x2:.0}, {y2:.0}) → ({:.0}, {:.0})",
            sent.unwrap_or_default(),
            gx - x,
            gy - y,
            land.0,
            land.1
        ),
    );
    if !ok {
        REAL.set(None);
    }
    Some(())
}

/// Everything a selection must leave untouched.
#[derive(Debug, PartialEq)]
struct Untouched {
    revision: u64,
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
    view_hash: u64,
    textarea_hash: u64,
    caret: (u32, u32),
    preview_loads: u64,
}

async fn untouched() -> Option<Untouched> {
    let s = snapshot().await?;
    let ta = textarea()?;
    Some(Untouched {
        revision: s.revision,
        dirty: s.dirty,
        can_undo: s.can_undo,
        can_redo: s.can_redo,
        view_hash: p::view_hash(&s.view),
        textarea_hash: p::view_hash(&ta.value()),
        caret: (
            ta.selection_start().ok().flatten().unwrap_or(0),
            ta.selection_end().ok().flatten().unwrap_or(0),
        ),
        preview_loads: ui().preview_loads.get_untracked(),
    })
}

impl Untouched {
    /// B3: everything but the caret. A PROVEN pick now reveals the picked
    /// node in the Source (a selection, never an edit), so the caret moves
    /// to exactly that node's span; everything else stays untouched.
    fn same_doc(&self, o: &Untouched) -> bool {
        Untouched {
            caret: o.caret,
            ..*self
        } == *o
    }
}

/// The view span of Scene Tree item `id` in the held analysis.
fn item_span(id: &str) -> Option<(u32, u32)> {
    ui().analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| (i.view_from as u32, i.view_to as u32))
    })
}

/// B3: after a PROVEN pick of `id`, the Source shows that node: its span is
/// selected, its first line is inside the scrolled view, the status bar's
/// Ln is that line, and the keyboard focus is NOT in the Source.
fn source_reveals(id: &str) -> (bool, String) {
    let Some(ta) = textarea() else {
        return (false, "no textarea".into());
    };
    let span = item_span(id);
    let caret = (
        ta.selection_start().ok().flatten().unwrap_or(u32::MAX),
        ta.selection_end().ok().flatten().unwrap_or(u32::MAX),
    );
    let from = span.map_or(0, |s| s.0) as usize;
    let line = 1 + ta
        .value()
        .encode_utf16()
        .take(from)
        .filter(|u| *u == b'\n' as u16)
        .count() as u32;
    let y = (line - 1) as i32 * 18;
    let (top, h) = (ta.scroll_top(), ta.client_height());
    let shown = h <= 0 || (y >= top && y < top + h);
    let ln = ui().cursor.get_untracked().0;
    let focused_source = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .is_some_and(|e| e.id() == "source");
    let ok = span == Some(caret) && shown && ln == line && !focused_source;
    (
        ok,
        format!(
            "span {span:?} caret {caret:?} · line {line} (status Ln {ln}) scroll {top}+{h} y {y} · source focused {focused_source}"
        ),
    )
}

fn item_id(label: &str) -> Option<String> {
    ui().analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.label == label)
            .map(|i| i.id.clone())
    })
}

fn selected_id() -> Option<String> {
    ui().selected.get_untracked().map(|s| s.id)
}

fn pick_text() -> String {
    crate::element_by_id::<HtmlElement>("pick-status")
        .and_then(|e| e.text_content())
        .unwrap_or_default()
}

/// The newest preview shows the current revision, and it is pickable.
pub(super) async fn ready(ms: u32) -> Option<()> {
    wait_ms(ms, || {
        let (s, r) = CORE.with_borrow(|c| c.session.map(|s| (s, c.revision)))?;
        let a = crate::pick::active()?;
        (a.session == s && a.revision == r && !CORE.with_borrow(|c| c.busy)).then_some(())
    })
    .await
}

async fn checkpoint(c: &p::PickSmoke, name: &str) {
    if c.hold_ms == 0 {
        return;
    }
    if let Some(b) = doc_el().and_then(|d| d.body()) {
        let _ = b.set_attribute("data-checkpoint", name);
    }
    ui::flash(&format!("checkpoint: {name}"));
    ipc::sleep(c.hold_ms as i32).await;
}

/// The four objects of part 1: (primitive, DEF name, x, y). Default camera
/// at z = 10, so the 2 × 2 grid stays well inside the viewport.
const OBJECTS: [(p::Primitive, &str, f64, f64); 4] = [
    (p::Primitive::Box, "Box_1", -1.6, 1.4),
    (p::Primitive::Sphere, "Sphere_1", 1.6, 1.4),
    (p::Primitive::Cylinder, "Cylinder_1", -1.6, -1.4),
    (p::Primitive::Cone, "Cone_1", 1.6, -1.4),
];

/// Click `name` at world (x, y, 0) and verify the whole selection contract.
async fn select_object(
    name: &str,
    x: f64,
    y: f64,
    s: &mut Stats,
    r: &mut R,
    tag: &str,
) -> Option<()> {
    let label = format!("Transform {name}");
    let want = item_id(&label)?;
    let before = untouched().await?;
    let (cx, cy) = project([x, y, 0.0], DEFAULT_CAMERA)?;
    let out = gesture(cx, cy).await;
    let Some(out) = out else {
        r.step(&format!("{tag}: click {name}"), false, "no pick result");
        return None;
    };
    s.record(&out);
    if out.is_proven() && out.item.as_deref() != Some(want.as_str()) {
        s.wrong += 1;
    }
    let li = wait_ms(3000, || {
        tree_item(&label).filter(|li| li.class_list().contains("selected"))
    })
    .await;
    let rev = before.revision;
    let insp = wait_ms(3000, || {
        ui().inspection
            .get_untracked()
            .filter(|i| i.title == label && i.revision == rev && i.id == want)
    })
    .await;
    let tr_inputs = all("tr[data-field=\"translation\"] input[type=\"text\"]")
        .into_iter()
        .filter_map(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .collect::<Vec<_>>();
    let after = untouched().await?;
    r.step(
        &format!("{tag}: click {name} in the viewport selects exactly Transform {name}"),
        out.is_proven()
            && out.item.as_deref() == Some(want.as_str())
            && out.role.as_deref() == Some("simple-object")
            && li.is_some()
            && insp.is_some()
            && tr_inputs.len() == 3
            && after.same_doc(&before)
            && Some(after.caret) == item_span(&want),
        format!(
            "{} {} · tree {} · inspector {} · translation {:?} · unchanged {} · {:.1} ms",
            out.status,
            out.reason,
            li.is_some(),
            insp.is_some(),
            tr_inputs,
            after.same_doc(&before),
            PICK.with_borrow(|p| p.last_total_ms)
        ),
    );
    Some(())
}

/// Create `prim`, then move it to (x, y) through the Inspector.
async fn create_at(prim: p::Primitive, name: &str, x: f64, y: f64) -> Option<()> {
    let rev0 = CORE.with_borrow(|c| c.revision);
    click("btn-create")?;
    wait_ms(2000, || super::create::el("#create-list")).await?;
    click(&format!("create-{}", prim.label().to_lowercase()))?;
    let label = format!("Transform {name}");
    wait_ms(5000, || {
        ui().inspection
            .get_untracked()
            .filter(|i| i.title == label && i.revision == rev0 + 1)
    })
    .await?;
    apply_field("translation", &[format!("{x}"), format!("{y}"), "0".into()]).await?;
    Some(())
}

async fn workflow(
    c: &p::PickSmoke,
    theme: &str,
    first: bool,
    s: &mut Stats,
    r: &mut R,
) -> Option<()> {
    let u = ui();
    let tag = format!("[{theme}]");
    let sel = crate::element_by_id::<web_sys::HtmlSelectElement>("theme-select")?;
    choose(&sel, theme)?;
    let applied = wait_ms(3000, || {
        (crate::theme::current_attribute().as_deref() == Some(theme)).then_some(())
    })
    .await;
    r.step(&format!("{tag} theme applied"), applied.is_some(), "");

    // --- New World + four primitives at their own places. ---
    let old = CORE.with_borrow(|c| c.session);
    click("btn-new")?;
    wait_ms(5000, || {
        u.doc
            .get_untracked()
            .filter(|d| d.untitled && d.revision == 0 && Some(d.session) != old)
    })
    .await?;
    for (prim, name, x, y) in OBJECTS {
        let ok = create_at(prim, name, x, y).await;
        r.step(
            &format!(
                "{tag} New World: Create {} and place it with the Inspector",
                prim.label()
            ),
            ok.is_some(),
            "",
        );
        ok?;
    }
    let rev = CORE.with_borrow(|c| c.revision);
    let st = previewed(rev, 4, 20_000).await;
    let cov = ipc::preview_coverage().await;
    r.step(
        &format!("{tag} viewport renders the four objects (rev {rev})"),
        st.is_some() && cov > 0.02 && ready(5000).await.is_some(),
        format!(
            "covered {:.2}% · {}",
            cov * 100.0,
            u.preview_status.get_untracked()
        ),
    );
    let status = ipc::preview_pick_status();
    r.step(
        &format!("{tag} X_ITE picking compatibility proven"),
        status.contains("\"ok\":true"),
        status,
    );

    // --- Select mode. ---
    if u.pick_mode.get_untracked() {
        click("btn-select")?; // a fresh state per round
    }
    // Without Select a click selects nothing.
    let before = selected_id();
    let (bx, by) = project([OBJECTS[0].2, OBJECTS[0].3, 0.0], DEFAULT_CAMERA)?;
    let done = PICK.with_borrow(|p| p.completed);
    pointer("pointerdown", bx, by);
    pointer("pointerup", bx, by);
    ipc::sleep(300).await;
    r.step(
        &format!("{tag} Select off: a viewport click requests no pick"),
        PICK.with_borrow(|p| p.completed) == done && selected_id() == before,
        "",
    );
    click("btn-select")?;
    let pressed = wait_ms(2000, || {
        crate::element_by_id::<HtmlElement>("btn-select")
            .and_then(|b| b.get_attribute("aria-pressed"))
            .filter(|v| v == "true")
    })
    .await;
    r.step(
        &format!("{tag} Select on (aria-pressed)"),
        pressed.is_some() && u.pick_mode.get_untracked(),
        "",
    );
    if first && c.real_pointer && REAL.get().is_none() {
        calibrate(r).await;
    }
    // A drag (pointer travel) never picks.
    let done = PICK.with_borrow(|p| p.completed);
    pointer("pointerdown", bx, by);
    pointer("pointerup", bx + 30.0, by + 12.0);
    ipc::sleep(300).await;
    r.step(
        &format!("{tag} a camera drag requests no pick"),
        PICK.with_borrow(|p| p.completed) == done,
        "",
    );

    // --- Click every object, twice, in two orders. ---
    for i in [0usize, 1, 2, 3, 3, 0, 2, 1] {
        let (_, name, x, y) = OBJECTS[i];
        select_object(name, x, y, s, r, &tag).await?;
    }
    // B3: the last pick is revealed in the Source without taking the focus;
    // "Show in Source" brings the Source tab forward and selects the same
    // span. Neither is an edit: revision, history and text are unchanged.
    {
        let id = selected_id().unwrap_or_default();
        let (shown, detail) = source_reveals(&id);
        let before = untouched().await?;
        let clicked = click("inspector-show-source").is_some();
        let on = wait_ms(2000, || {
            let ta = textarea()?;
            let focused = web_sys::window()?.document()?.active_element()?.id() == "source";
            let caret = (ta.selection_start().ok()??, ta.selection_end().ok()??);
            (crate::dock::dock().layout.with_untracked(|l| l.is_on_screen(p::layout::PanelId::Source))
                && focused
                && Some(caret) == item_span(&id))
            .then_some(())
        })
        .await;
        let after = untouched().await?;
        r.step(
            &format!("{tag} the picked node is revealed in Source (span selected, line in view, Ln follows, focus kept); Show in Source brings the Source tab forward; no edit"),
            shown && clicked && on.is_some() && after == before,
            format!("{detail} · show-in-source {} · unchanged {}", on.is_some(), after == before),
        );
        // Back to the preset's front tab, so later steps see the same layout.
        crate::dock::focus(p::layout::PanelId::Diagnostics);
        ipc::sleep(100).await;
    }
    checkpoint(c, &format!("{theme}-selected")).await;

    // --- Empty space: NO_HIT, the selection stays. ---
    let keep = selected_id();
    let rect = viewport()?.get_bounding_client_rect();
    let before = untouched().await?;
    let out = gesture(rect.left() + 6.0, rect.top() + 6.0).await?;
    s.record(&out);
    r.step(
        &format!("{tag} empty space: NO_HIT; the selection and source are unchanged"),
        out.status == "NO_HIT"
            && selected_id() == keep
            && untouched().await? == before
            && !pick_text().is_empty(),
        format!("{} · \"{}\"", out.reason, pick_text()),
    );

    // --- Sphere: Transform edit through the Inspector after a viewport pick. ---
    let (_, sphere, sx, sy) = OBJECTS[1];
    select_object(sphere, sx, sy, s, r, &tag).await?;
    let rev0 = CORE.with_borrow(|c| c.revision);
    let ny = 0.6;
    let t = apply_field("translation", &["1.6".into(), format!("{ny}"), "0".into()]).await;
    // Clicked before the preview shows the edit: stale, selection kept.
    let keep = selected_id();
    let (px, py) = project([sx, sy, 0.0], DEFAULT_CAMERA)?;
    let stale = gesture(px, py).await?;
    s.record(&stale);
    r.step(
        &format!("{tag} edit pending in the preview: the click is refused as stale"),
        t.is_some() && stale.status == "REFUSED_STALE" && selected_id() == keep && keep.is_some(),
        format!("{} {} · \"{}\"", stale.status, stale.reason, pick_text()),
    );
    let st = previewed(rev0 + 1, 4, 20_000).await;
    ready(5000).await;
    // (1.6, -0.3) is inside the MOVED sphere only: the preview updated.
    let (qx, qy) = project([1.6, -0.3, 0.0], DEFAULT_CAMERA)?;
    let moved = gesture(qx, qy).await?;
    s.record(&moved);
    let want = item_id(&format!("Transform {sphere}"));
    if moved.is_proven() && moved.item != want {
        s.wrong += 1;
    }
    r.step(
        &format!("{tag} Inspector translation reached the viewport: the moved Sphere is picked at its new place"),
        st.is_some() && moved.is_proven() && moved.item == want,
        format!("{} {}", moved.status, moved.reason),
    );

    // --- Sphere Material: diffuseColor through the Inspector. ---
    let px0 = ipc::preview_pixel(qx, qy).await.unwrap_or_default();
    let items = all("li.tree-item");
    let at = items
        .iter()
        .position(|li| li.class_list().contains("selected"))?;
    let mat = items[at..]
        .iter()
        .find(|li| {
            li.query_selector(".label")
                .ok()
                .flatten()
                .and_then(|l| l.text_content())
                .as_deref()
                == Some("Material")
        })?
        .clone()
        .dyn_into::<HtmlElement>()
        .ok()?;
    mat.click();
    wait_ms(3000, || {
        u.inspection
            .get_untracked()
            .filter(|i| i.title == "Material")
    })
    .await?;
    let rev1 = CORE.with_borrow(|c| c.revision);
    let col = apply_field("diffuseColor", &["0.95".into(), "0.1".into(), "0.1".into()]).await;
    let st = previewed(rev1 + 1, 4, 20_000).await;
    ready(5000).await;
    let px1 = ipc::preview_pixel(qx, qy).await.unwrap_or_default();
    r.step(
        &format!("{tag} Inspector diffuseColor reached the viewport (pixel color changed)"),
        col.is_some()
            && st.is_some()
            && text().contains("diffuseColor 0.95 0.1 0.1")
            && px0 != px1
            && px1.len() == 3
            && px1[0] > px1[1],
        format!("{px0:?} → {px1:?}"),
    );
    let again = gesture(qx, qy).await?;
    s.record(&again);
    r.step(
        &format!("{tag} after the Material edit the Sphere is still picked exactly"),
        again.is_proven() && again.item == item_id(&format!("Transform {sphere}")),
        format!("{} {}", again.status, again.reason),
    );

    // --- Preview replaced while a pick is in flight: refused, never late. ---
    let keep = selected_id();
    let late0 = PICK.with_borrow(|p| p.late_refused);
    let (cx, cy) = project([OBJECTS[2].2, OBJECTS[2].3, 0.0], DEFAULT_CAMERA)?;
    let done = PICK.with_borrow(|p| p.completed);
    // The production pick path, then a preview reload queued right behind
    // it: the pick's IPC is in flight when the reload retires its generation.
    leptos::task::spawn_local(async move {
        let _ = crate::pick::click(cx, cy).await;
    });
    leptos::task::spawn_local(ui::preview(true));
    wait_ms(10_000, || {
        (PICK.with_borrow(|p| p.completed) > done).then_some(())
    })
    .await?;
    let late = PICK.with_borrow(|p| p.last.clone())?;
    s.record(&late);
    r.step(
        &format!("{tag} preview replaced during a pending pick: the late reply is refused"),
        late.status == "REFUSED_STALE"
            && PICK.with_borrow(|p| p.late_refused) == late0 + 1
            && selected_id() == keep,
        format!("{} {}", late.status, late.reason),
    );
    ready(20_000).await;

    // --- A syntax error: the preview keeps the LAST VALID scene, which is
    //     no longer the document and must never be picked. ---
    let ta = textarea()?;
    let end = editor::utf16_len(&ta.value()) as u32;
    let rev2 = CORE.with_borrow(|c| c.revision);
    let _ = ta.focus();
    ta.set_range_text_with_start_and_end("Transform { children [ ", end, end)
        .ok()?;
    let ev = web_sys::InputEvent::new("input").ok()?;
    ta.dispatch_event(&ev).ok()?;
    wait_ms(5000, || {
        (CORE.with_borrow(|c| c.revision) == rev2 + 1).then_some(())
    })
    .await?;
    editor::idle().await;
    let kept = wait_ms(20_000, || {
        let s = u.preview_status.get_untracked();
        s.starts_with(&format!("rev {} · error (last valid scene kept)", rev2 + 1))
            .then_some(s)
    })
    .await;
    let keep = selected_id();
    let (bx, by) = project([OBJECTS[0].2, OBJECTS[0].3, 0.0], DEFAULT_CAMERA)?;
    let lv = gesture(bx, by).await?;
    s.record(&lv);
    r.step(
        &format!("{tag} last valid scene after a syntax error: the click is refused"),
        kept.is_some() && lv.status == "REFUSED_STALE" && selected_id() == keep,
        format!("{} {} · {:?}", lv.status, lv.reason, kept),
    );
    // Undo the error: picking works again.
    click("btn-undo")?;
    wait_ms(5000, || {
        (CORE.with_borrow(|c| c.revision) == rev2 + 2).then_some(())
    })
    .await?;
    let st = previewed(rev2 + 2, 4, 20_000).await;
    ready(5000).await;
    r.step(
        &format!("{tag} Undo restores the valid document and preview"),
        st.is_some(),
        "",
    );
    select_object(OBJECTS[0].1, bx_world(0), by_world(0), s, r, &tag).await?;

    // --- Undo / Redo / Save As keep working (first round). ---
    if first {
        let t0 = text();
        let rev = CORE.with_borrow(|c| c.revision);
        click("btn-undo")?;
        wait_ms(5000, || {
            (CORE.with_borrow(|c| c.revision) == rev + 1).then_some(())
        })
        .await?;
        let undone = text();
        click("btn-redo")?;
        wait_ms(5000, || {
            (CORE.with_borrow(|c| c.revision) == rev + 2).then_some(())
        })
        .await?;
        r.step(
            &format!("{tag} Undo / Redo after viewport selection"),
            undone != t0 && text() == t0,
            "",
        );
        click("btn-save-as")?;
        let saved = wait_ms(5000, || {
            u.doc.get_untracked().filter(|d| !d.untitled && !d.dirty)
        })
        .await;
        r.step(
            &format!("{tag} Save As writes the world"),
            saved.is_some(),
            u.last_save.get_untracked().unwrap_or_default(),
        );
        ready(20_000).await;
        select_object(OBJECTS[3].1, bx_world(3), by_world(3), s, r, &tag).await?;
    }
    checkpoint(c, &format!("{theme}-done")).await;
    Some(())
}

fn bx_world(i: usize) -> f64 {
    OBJECTS[i].2
}
fn by_world(i: usize) -> f64 {
    OBJECTS[i].3
}

async fn matrix(c: &p::PickSmoke, s: &mut Stats, r: &mut R) -> Option<()> {
    let u = ui();
    if !u.pick_mode.get_untracked() {
        click("btn-select")?;
    }
    for (i, fx) in c.fixtures.iter().enumerate() {
        #[derive(serde::Serialize)]
        struct A {
            index: usize,
        }
        let old = CORE.with_borrow(|c| c.session);
        let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: i })
            .await
            .ok()?;
        ui::apply_open(o, "Opened").await;
        let opened = wait_ms(5000, || {
            u.doc
                .get_untracked()
                .filter(|d| d.revision == 0 && !d.untitled && Some(d.session) != old)
        })
        .await;
        let loaded = ready(20_000).await;
        let (mut ok, mut wrong, mut skipped, mut detail) = (0u32, 0u32, 0u32, Vec::new());
        let mut run = 0u32;
        let layered = fx.clicks.iter().any(|c| !c.then.is_empty());
        if layered {
            capture(c, &format!("{}-loaded", slug(&fx.id))).await;
        }
        for ck in &fx.clicks {
            if let Some(why) = &ck.skip {
                skipped += 1;
                detail.push(format!("{}:SKIP({why})", ck.id));
                continue;
            }
            let (x, y) = project(ck.world, fx.camera)?;
            // P0-A: the first click, then every repeated click at the same
            // point with ITS expected answer (the next Shape behind).
            let expects = std::iter::once((ck.status.clone(), ck.logical))
                .chain(ck.then.iter().map(|t| (t.status.clone(), t.logical)));
            for (n, (status, logical)) in expects.enumerate() {
                run += 1;
                let id = if n == 0 { ck.id.clone() } else { format!("{}#{}", ck.id, n + 1) };
                let keep = selected_id();
                let before = untouched().await?;
                let Some(out) = gesture(x, y).await else {
                    detail.push(format!("{id}:no-result"));
                    continue;
                };
                s.record(&out);
                let span = out.logical.as_ref().map(|l| [l.from, l.to]);
                let is_wrong = out.is_proven() && (status != "PROVEN" || span != logical);
                let after = untouched().await?;
                // B3: a PROVEN pick reveals its node in the Source (the caret
                // moves to that node's span); anything else leaves all untouched.
                let doc_ok = if out.is_proven() {
                    after.same_doc(&before)
                        && out.item.as_deref().and_then(item_span) == Some(after.caret)
                } else {
                    after == before
                };
                let good = !is_wrong
                    && out.status == status
                    && doc_ok
                    && (out.is_proven() || selected_id() == keep)
                    && (!out.is_proven() || selected_id() == out.item);
                if is_wrong {
                    wrong += 1;
                }
                if good {
                    ok += 1;
                } else {
                    s.mismatched += 1;
                }
                let layer = PICK.with_borrow(|p| {
                    p.cycle.as_ref().map(|c| format!("[{}/{}]", c.layer + 1, c.layers))
                });
                detail.push(format!(
                    "{id}:{}{}{}{}",
                    out.status,
                    if layered { layer.unwrap_or_default() } else { String::new() },
                    if good { "" } else { "≠" },
                    if good {
                        String::new()
                    } else {
                        format!("{status}({}) span {span:?} want {logical:?}", out.reason)
                    }
                ));
                if layered {
                    capture(c, &format!("{}-click{}", slug(&fx.id), n + 1)).await;
                }
            }
        }
        s.wrong += wrong;
        s.skipped += skipped;
        r.step(
            &format!(
                "matrix {}: {ok}/{run} clicks as the oracle expects, WRONG {wrong}",
                fx.id
            ),
            opened.is_some() && loaded.is_some() && ok == run && wrong == 0,
            detail.join(" "),
        );
        if layered {
            stale_status(fx, r).await?;
            escape(fx, r).await?;
        }
        if fx.id == "L1-coat-over-paint" {
            duplicate_offset(c, fx, s, r).await?;
        }
        if fx.id == "S1-edit-input-safety" {
            s.wrong += sensor_gate(c, fx, r).await?;
        }
    }
    Some(())
}

/// LV2-C: the pick status line ("Shape 2 of 2 under the pointer") belongs to
/// the selection, revision and tool it was written for. Each other way of
/// changing those clears it (and the hit cycle): a Scene Tree click, a
/// breadcrumb, Parent and a tool switch here; Undo in `duplicate_offset`.
async fn stale_status(fx: &p::PickFixture, r: &mut R) -> Option<()> {
    let ck = fx.clicks.iter().find(|k| !k.then.is_empty())?;
    let (x, y) = project(ck.world, fx.camera)?;
    let el = |q: &str| -> Option<HtmlElement> {
        doc_el()?.query_selector(q).ok()??.dyn_into::<HtmlElement>().ok()
    };
    let usable = |q: &str| {
        el(q).filter(|b| !b.dyn_ref::<web_sys::HtmlButtonElement>().is_some_and(|b| b.disabled()))
    };
    let cleared = || {
        pick_text().trim_matches(|c: char| c.is_whitespace() || c == '\u{a0}').is_empty()
            && PICK.with_borrow(|p| p.cycle.is_none())
    };
    let mut done = Vec::new();
    for action in ["tree", "same", "crumb", "parent", "tool"] {
        // Two clicks at one place: the second names the Shape behind.
        let _ = gesture(x, y).await;
        let _ = gesture(x, y).await;
        let shown = pick_text();
        let cycled = shown.contains(" of ") && PICK.with_borrow(|p| p.cycle.is_some());
        let before = selected_id();
        let acted = match action {
            "tree" => {
                let other = ui().analysis.with_untracked(|a| {
                    a.as_ref()?
                        .items
                        .iter()
                        .find(|i| Some(&i.id) != before.as_ref())
                        .map(|i| i.id.clone())
                });
                other
                    .and_then(|id| el(&format!(".tree-item[data-id=\"{id}\"]")))
                    .map(|li| li.click())
                    .is_some()
            }
            // The item that is ALREADY selected (Copilot #147): still a
            // non-pick selection, so the status and the cycle reset.
            "same" => before
                .as_ref()
                .and_then(|id| el(&format!(".tree-item[data-id=\"{id}\"]")))
                .map(|li| li.click())
                .is_some(),
            "crumb" => usable("#inspector-path button.crumb").map(|b| b.click()).is_some(),
            "parent" => usable("#inspector-up").map(|b| b.click()).is_some(),
            _ => click("btn-move").is_some(),
        };
        if !acted {
            done.push(format!("{action}:n/a"));
            continue;
        }
        let changed = wait_ms(3000, || {
            match action {
                "tool" => true,
                "same" => selected_id() == before,
                _ => selected_id() != before,
            }
            .then_some(())
        })
        .await
        .is_some();
        let ok = wait_ms(3000, || cleared().then_some(())).await.is_some();
        r.step(
            &format!("{}: pick status cleared after {action}", fx.id),
            cycled && changed && ok,
            format!(
                "before {:?} · cycled {cycled} · selection changed {changed} · after {:?}",
                shown.chars().take(80).collect::<String>(),
                pick_text()
            ),
        );
        done.push(format!("{action}:{}", cycled && changed && ok));
        if action == "tool" {
            click("btn-select")?;
        }
    }
    // Leave the front Shape selected, as the matrix did (Duplicate next).
    let _ = gesture(x, y).await;
    r.step(
        &format!("{}: stale pick status actions covered", fx.id),
        done.iter().any(|d| d.starts_with("tree:")),
        done.join(" "),
    );
    Some(())
}

/// Stage 2: a REAL Escape key with the 3D Viewport focused clears the
/// scene selection through the selection authority: no outline, no
/// Inspector fields, no Scene Tree highlight, no hit cycle, no pick status,
/// and the document is untouched. The next click starts at the FRONT layer
/// again and proves exactly the oracle's node (WRONG_SOURCE_SELECTIONS).
/// Escape in the Source editor or in a dialog keeps the selection.
async fn escape(fx: &p::PickFixture, r: &mut R) -> Option<()> {
    let ck = fx.clicks.iter().find(|k| !k.then.is_empty())?;
    let (x, y) = project(ck.world, fx.camera)?;
    let el = |q: &str| -> Option<HtmlElement> {
        doc_el()?.query_selector(q).ok()??.dyn_into::<HtmlElement>().ok()
    };
    let active = || {
        doc_el()
            .and_then(|d| d.active_element())
            .map(|e| format!("{}#{}", e.tag_name().to_lowercase(), e.id()))
            .unwrap_or_default()
    };
    let span = |o: &Option<p::PickOutcome>| {
        o.as_ref().and_then(|o| o.logical.as_ref().map(|l| [l.from, l.to]))
    };
    // (1) Cycle to the Shape behind, then Escape in the viewport.
    let _ = gesture(x, y).await;
    let _ = gesture(x, y).await;
    let cycled = PICK.with_borrow(|p| p.cycle.is_some()) && selected_id().is_some();
    let focus = active();
    let before = untouched().await?;
    // A real X key press (xdotool) into our focused window.
    super::gizmo::real("escape", x, y).await.ok()?;
    let gone = wait_ms(3000, || ui().selected.get_untracked().is_none().then_some(())).await;
    ipc::sleep(300).await;
    let after = untouched().await?;
    let outline = crate::highlight::HIGHLIGHT.with_borrow(|h| h.path.clone());
    let tree_sel = all(".tree-item.selected").len();
    let inspector_empty = el("section.inspector p.empty").is_some();
    let status = pick_text();
    let quiet = status.trim_matches(|c: char| c.is_whitespace() || c == '\u{a0}').is_empty();
    let state = PICK.with_borrow(|p| (p.cycle.is_none(), p.last.is_none()));
    r.step(
        &format!("{}: real Escape in the viewport clears the selection: no outline, Inspector and Scene Tree empty, no hit cycle or pick status, the source untouched", fx.id),
        cycled
            && gone.is_some()
            && outline.is_empty()
            && tree_sel == 0
            && inspector_empty
            && quiet
            && state == (true, true)
            && before == after,
        format!(
            "cycled {cycled} · focus {focus} · cleared {} · outline {} · tree {tree_sel} · inspector-empty {inspector_empty} · status {status:?} · cycle/last none {state:?} · doc same {}",
            gone.is_some(),
            outline.len(),
            before == after
        ),
    );
    // (2) The next click is the FRONT layer again, proven exactly.
    let out = gesture(x, y).await;
    let front = ck.status == "PROVEN" && span(&out) == ck.logical && out.as_ref().is_some_and(|o| o.is_proven());
    let wrong = usize::from(out.as_ref().is_some_and(|o| o.is_proven()) && !front);
    r.step(
        &format!("{}: WRONG_SOURCE_SELECTIONS after Escape: the next click selects the front Shape", fx.id),
        front && wrong == 0,
        format!("wrong {wrong} · span {:?} want {:?}", span(&out), ck.logical),
    );
    // (3) Escape in the Source editor keeps the scene selection.
    let sel = ui().selected.get_untracked();
    crate::command::run(p::command::CommandId::ViewFocusSource);
    let in_source = wait_ms(2000, || (active() == "textarea#source").then_some(())).await.is_some();
    super::gizmo::real("escape", x, y).await.ok()?;
    ipc::sleep(300).await;
    let kept_source = in_source && sel.is_some() && ui().selected.get_untracked() == sel;
    // (4) Escape in a dialog (About) closes the dialog only.
    click("btn-about")?;
    let open = wait_ms(2000, || el("#about-dialog").filter(|d| !d.hidden()).map(|_| ())).await;
    super::gizmo::real("escape", x, y).await.ok()?;
    let closed = wait_ms(2000, || el("#about-dialog").is_none_or(|d| d.hidden()).then_some(())).await;
    ipc::sleep(200).await;
    let kept_dialog = ui().selected.get_untracked() == sel;
    r.step(
        &format!("{}: Escape in the Source editor or in a dialog keeps the scene selection (the dialog closes)", fx.id),
        kept_source && open.is_some() && closed.is_some() && kept_dialog,
        format!("focus {} · source {kept_source} · dialog open {} closed {} kept {kept_dialog}", active(), open.is_some(), closed.is_some()),
    );
    // Back to the viewport; the front Shape (step 2) is still selected
    // (Duplicate next). Another click here would cycle to the one behind.
    crate::command::run(p::command::CommandId::ViewFocusViewport);
    ipc::sleep(300).await;
    Some(())
}

/// LINUX-EDIT-INPUT-SAFETY: the world's own pointing sensors never get an
/// editing click. Real X input on `S1-edit-input-safety`, whose runtime
/// fields (public SAI, by DEF name) show whether a sensor fired or a ROUTE
/// ran: Select (the matrix just clicked every object), Move / Rotate /
/// Scale, a drag that starts on a PlaneSensor (orbits, never drags it), the
/// looping TimeSensor keeps running, Escape with nothing selected is quiet,
/// then the explicit Interact mode (every sensor responds, nothing is
/// picked, the source is untouched) and leaving it (the preview reloads
/// from the source, editing input is back). Returns the WRONG count.
async fn sensor_gate(c: &p::PickSmoke, fx: &p::PickFixture, r: &mut R) -> Option<u32> {
    use p::command::CommandId as C;
    const WATCH: [(&str, &str, &str); 6] = [
        ("DoorTouch", "touchTime", "-1"), // X_ITE: never touched
        ("Door", "rotation", "0 0 1 0"),
        ("Far", "isBound", "false"),
        ("Slider", "translation", "0 0 0"),
        ("L1", "color", "0.2 0.2 0.2"),
        ("DoorTouch", "isOver", "false"),
    ];
    let read = |d: &str, f: &str| ipc::preview_scene_field(d, f);
    let fields = || WATCH.iter().map(|(d, f, _)| format!("{d}.{f}={}", read(d, f))).collect::<Vec<_>>();
    let authored: Vec<String> = WATCH.iter().map(|(d, f, v)| format!("{d}.{f}={v}")).collect();
    let changed = |now: &[String]| {
        now.iter()
            .zip(&authored)
            .filter(|(n, a)| n != a)
            .map(|(n, _)| n.split('=').next().unwrap_or("").to_string())
            .collect::<Vec<_>>()
    };
    let at = |id: &str| {
        fx.clicks
            .iter()
            .find(|k| k.id == id)
            .and_then(|k| project(k.world, fx.camera))
    };
    let (door, anchor, plane, lamp) = (at("door")?, at("anchor")?, at("plane")?, at("lamp-in-proto")?);
    let u = ui();
    let real = REAL.get().is_some();
    let start = untouched().await?;
    let mut wrong = 0u32;
    let gap = || ipc::sleep(REAL_CLICK_GAP_MS);

    // (1) The matrix clicked every object once in Select.
    let gate = ipc::preview_scene_input(None);
    let now = fields();
    r.step(
        "S1: Select clicks on a TouchSensor door, an Anchor, a PlaneSensor and a TouchSensor in a nested PROTO fire no sensor, run no ROUTE, follow no Anchor",
        real && gate == "edit" && now == authored,
        format!("real {real} · gate {gate} · changed {:?} · {}", changed(&now), now.join(" ")),
    );

    // (2) Move / Rotate / Scale: the same clicks (they pick, too).
    let mut detail = Vec::new();
    let mut quiet = true;
    for (btn, tool) in [("btn-move", "Move"), ("btn-rotate", "Rotate"), ("btn-scale", "Scale")] {
        click(btn)?;
        ipc::sleep(200).await;
        for (x, y) in [anchor, lamp, door] {
            let out = gesture(x, y).await;
            if out.as_ref().is_some_and(|o| o.is_proven()) {
                wrong += 1;
            }
        }
        ipc::sleep(400).await;
        let now = fields();
        let ch = changed(&now);
        quiet &= ch.is_empty() && ipc::preview_scene_input(None) == "edit";
        detail.push(format!("{tool}: changed {ch:?} status {:?}", pick_text().chars().take(60).collect::<String>()));
    }
    click("btn-select")?;
    ipc::sleep(200).await;
    r.step(
        "S1: Move, Rotate and Scale clicks on the same sensor objects fire nothing (no hover either) and select nothing wrong",
        real && quiet && wrong == 0 && u.pick_mode.get_untracked(),
        detail.join(" · "),
    );

    // (3) A drag that STARTS on the PlaneSensor orbits the camera; the
    // sensor never drags its Slider.
    let cam0 = ipc::camera_state().map(|c| c.orientation_offset);
    super::gizmo::real("down", plane.0, plane.1).await.ok()?;
    for k in 1..=8 {
        super::gizmo::real("move", plane.0 - 12.0 * k as f64, plane.1 + 4.0 * k as f64).await.ok()?;
        ipc::sleep(30).await;
    }
    super::gizmo::real("up", plane.0 - 96.0, plane.1 + 32.0).await.ok()?;
    ipc::sleep(500).await;
    let cam1 = ipc::camera_state().map(|c| c.orientation_offset);
    let slider = read("Slider", "translation");
    let active = read("Drag", "isActive");
    let orbited = cam0.is_some() && cam1.is_some() && cam0 != cam1;
    r.step(
        "S1: a Select drag that starts on a PlaneSensor orbits the camera and never drags the sensor",
        real && orbited && slider == "0 0 0" && active == "false",
        format!("camera {cam0:?} -> {cam1:?} · Slider {slider} · Drag.isActive {active}"),
    );
    crate::command::run(C::ViewResetCamera);
    ipc::sleep(900).await;

    // (4) Authored animation is not interaction: the looping TimeSensor runs.
    let s0 = read("Spin", "rotation");
    ipc::sleep(500).await;
    let s1 = read("Spin", "rotation");
    r.step(
        "S1: a looping TimeSensor (ROUTE-driven rotation) keeps running while editing",
        !s0.is_empty() && s0 != s1,
        format!("Spin.rotation {s0} -> {s1}"),
    );

    // (5) Escape with nothing selected: no notice, no change.
    crate::ui::clear_selection();
    crate::command::run(C::ViewFocusViewport);
    ipc::sleep(300).await;
    u.message.set(String::new());
    let runs = crate::command::run_count();
    let before = untouched().await?;
    super::gizmo::real("escape", door.0, door.1).await.ok()?;
    ipc::sleep(400).await;
    let msg = u.message.get_untracked();
    let after = untouched().await?;
    r.step(
        "S1: Escape in the viewport with nothing selected shows no notice and changes nothing",
        msg.is_empty() && crate::command::run_count() == runs && before == after && selected_id().is_none(),
        format!("message {msg:?} · commands run {} · doc same {}", crate::command::run_count() - runs, before == after),
    );

    // (6) Interact: the world responds; the editor does not pick.
    click("btn-interact")?;
    ipc::sleep(300).await;
    let gate = ipc::preview_scene_input(None);
    let modes = (u.interact_mode.get_untracked(), u.pick_mode.get_untracked(), u.move_mode.get_untracked());
    let picks = PICK.with_borrow(|p| p.completed);
    let before = untouched().await?;
    super::gizmo::real("click", door.0, door.1).await.ok()?;
    ipc::sleep(900).await;
    super::gizmo::real("click", lamp.0, lamp.1).await.ok()?;
    ipc::sleep(700).await;
    super::gizmo::real("down", plane.0, plane.1).await.ok()?;
    for k in 1..=6 {
        super::gizmo::real("move", plane.0 + 10.0 * k as f64, plane.1).await.ok()?;
        ipc::sleep(30).await;
    }
    super::gizmo::real("up", plane.0 + 60.0, plane.1).await.ok()?;
    gap().await;
    let mid = fields();
    super::gizmo::real("click", anchor.0, anchor.1).await.ok()?;
    ipc::sleep(900).await;
    let bound = read("Far", "isBound");
    let after = untouched().await?;
    let ch = changed(&mid);
    let fired = ["DoorTouch.touchTime", "Door.rotation", "Slider.translation", "L1.color"]
        .iter()
        .all(|f| ch.iter().any(|c| c == f));
    let no_pick = PICK.with_borrow(|p| p.completed) == picks && selected_id().is_none();
    capture(c, "s1-interact").await;
    r.step(
        "S1: Interact: the door's TouchSensor runs its ROUTEs, the PROTO TouchSensor recolors, the PlaneSensor drags, the Anchor binds its Viewpoint; nothing is picked and the source is untouched",
        gate == "interact" && modes == (true, false, false) && fired && bound == "true" && no_pick && before == after,
        format!("gate {gate} · modes {modes:?} · changed {ch:?} · Far.isBound {bound} · no pick {no_pick} · doc same {} · {}", before == after, mid.join(" ")),
    );

    // (7) Leaving Interact (Select): sensors lose the pointer, the preview
    // reloads from the source, the authored state is back.
    let loads = u.preview_loads.get_untracked();
    click("btn-select")?;
    let reloaded = wait_ms(8000, || (u.preview_loads.get_untracked() > loads).then_some(())).await;
    let ready_ok = ready(20_000).await;
    ipc::sleep(400).await;
    // The Anchor bound "far": put the camera back on "front" for the clicks.
    crate::camera::refresh_viewpoints();
    ipc::sleep(400).await;
    if let Some(i) = u.viewpoints.with_untracked(|v| v.items.iter().position(|i| i.description == "front")) {
        crate::camera::choose_viewpoint(i);
    }
    ipc::sleep(1500).await;
    crate::command::run(C::ViewResetCamera);
    let home = wait_ms(4000, || ipc::camera_state().filter(|c| c.is_default).map(|_| ())).await;
    ipc::sleep(300).await;
    capture(c, "s1-after-interact").await;
    let gate = ipc::preview_scene_input(None);
    let now = fields();
    let ch: Vec<String> = changed(&now).into_iter().filter(|f| f != "Far.isBound").collect();
    let end = untouched().await?;
    let same_source = (end.revision, end.dirty, end.textarea_hash) == (start.revision, start.dirty, start.textarea_hash);
    r.step(
        "S1: leaving Interact closes the gate and reloads the preview from the source (door, slider and lamp as authored; the source unchanged)",
        gate == "edit" && !u.interact_mode.get_untracked() && u.pick_mode.get_untracked() && reloaded.is_some() && ready_ok.is_some() && ch.is_empty() && same_source,
        format!("gate {gate} · reloaded {} · camera home {} · changed {ch:?} · source same {same_source} · {}", reloaded.is_some(), home.is_some(), now.join(" ")),
    );

    // (8) Editing input is back: the door refuses again and stays shut.
    let out = gesture(door.0, door.1).await;
    ipc::sleep(500).await;
    let status = out.as_ref().map(|o| o.status.clone()).unwrap_or_default();
    if out.as_ref().is_some_and(|o| o.is_proven()) {
        wrong += 1;
    }
    let now = fields();
    let ch: Vec<String> = changed(&now).into_iter().filter(|f| f != "Far.isBound").collect();
    r.step(
        "S1: after Interact, a Select click on the door refuses (REFUSED_SENSOR_CONFLICT) and fires nothing",
        status == "REFUSED_SENSOR_CONFLICT" && ch.is_empty(),
        format!("status {status} · changed {ch:?}"),
    );
    Some(wrong)
}

fn slug(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Evidence only (the harness Xvfb): a whole-screen capture into the run's
/// `captures/` folder. Never a step; a failure is noted in the log.
async fn capture(c: &p::PickSmoke, id: &str) {
    if !c.real_pointer {
        return;
    }
    ipc::sleep(250).await;
    #[derive(serde::Serialize)]
    struct A<'a> {
        id: &'a str,
    }
    if let Err(e) = call::<String>("smoke_window_capture", A { id }).await {
        web_sys::console::warn_1(&format!("capture {id}: {e}").into());
    }
}

/// P0-B in the real window: the coat (selected by the layer clicks) is
/// duplicated with the production command (Edit > Duplicate / Ctrl+D). The
/// copy must be inserted after the original as ONE undo step, be SELECTED,
/// and render beside it: a click at the copy's offset position must pick
/// exactly the copy. Undo restores the exact bytes.
async fn duplicate_offset(
    c: &p::PickSmoke,
    fx: &p::PickFixture,
    s: &mut Stats,
    r: &mut R,
) -> Option<()> {
    let coat = fx.clicks.first()?.logical?;
    let before_text = text();
    let rev = CORE.with_borrow(|c| c.revision);
    // The outline must be proven for the selection, so Duplicate measures
    // the coat's rendered size (4 x 4 x 0.2: step 1.2 x 4 -> 5).
    let proven = wait_ms(5000, || {
        crate::highlight::selection_extent().filter(|e| (e - 4.0).abs() < 1e-3)
    })
    .await;
    capture(c, "dup-before").await;
    crate::command::run(p::command::CommandId::EditDuplicateNode);
    let applied = wait_ms(5000, || {
        CORE.with_borrow(|c| (c.revision == rev + 1 && !c.busy).then_some(()))
    })
    .await;
    let after_text = text();
    let to = coat[1] as usize;
    let b16: Vec<u16> = before_text.encode_utf16().collect();
    let a16: Vec<u16> = after_text.encode_utf16().collect();
    let inserted = a16.len().saturating_sub(b16.len());
    let pure_insert = a16.len() > b16.len()
        && a16[..to] == b16[..to]
        && a16[to + inserted..] == b16[to..];
    let copy = String::from_utf16_lossy(&a16[to..to + inserted]);
    let want = "\nTransform { translation 5 0 2 children [ ";
    // The selection is set from the edit result; the scene-tree analysis of
    // the new revision can arrive a little later (a loaded machine), so the
    // copy's span is read once that analysis lists it.
    let sel = selected_id();
    let sel_span = match sel.as_deref() {
        Some(id) => wait_ms(5000, || item_span(id)).await,
        None => None,
    };
    let expect_span = (to as u32 + 1, (to + inserted) as u32);
    let ok = proven.is_some()
        && applied.is_some()
        && pure_insert
        && copy.starts_with(want)
        && sel_span == Some(expect_span);
    r.step(
        "Duplicate (Ctrl+D): the copy is inserted after the original, moved by about its size (+X 5), and selected",
        ok,
        format!(
            "extent {proven:?} · revision {rev} -> {} · pure insert {pure_insert} · copy {:?} · selected {sel:?} span {sel_span:?} want {expect_span:?}",
            CORE.with_borrow(|c| c.revision),
            copy.chars().take(70).collect::<String>()
        ),
    );
    let _ = ready(20_000).await;
    capture(c, "dup-after").await;
    // The copy renders beside the original: a click at its offset place
    // picks exactly the copy (a fresh point: no layer cycling).
    let (x, y) = project([5.0, 0.0, 2.1], fx.camera)?;
    let out = gesture(x, y).await;
    if let Some(o) = &out {
        s.record(o);
    }
    let span = out.as_ref().and_then(|o| o.logical.as_ref().map(|l| [l.from, l.to]));
    let want_span = [expect_span.0 as u64, expect_span.1 as u64];
    let picked = out.as_ref().is_some_and(|o| o.is_proven()) && span == Some(want_span);
    if out.as_ref().is_some_and(|o| o.is_proven()) && !picked {
        s.wrong += 1;
    }
    r.step(
        "the copy is visibly offset: clicking its place in the viewport selects the copy",
        picked,
        format!("{:?} span {span:?} want {want_span:?}", out.as_ref().map(|o| &o.status)),
    );
    capture(c, "dup-copy-picked").await;
    // One undo step restores the exact bytes.
    crate::command::run(p::command::CommandId::EditUndo);
    let undone = wait_ms(5000, || {
        (text() == before_text && !CORE.with_borrow(|c| c.busy)).then_some(())
    })
    .await;
    r.step(
        "Undo after Duplicate restores the exact source bytes (one step)",
        undone.is_some(),
        format!("revision {}", CORE.with_borrow(|c| c.revision)),
    );
    // LV2-C: the copy's pick message does not outlive the Undo.
    let stale = pick_text();
    let cleared = wait_ms(3000, || {
        pick_text()
            .trim_matches(|c: char| c.is_whitespace() || c == '\u{a0}')
            .is_empty()
            .then_some(())
    })
    .await;
    r.step(
        "Undo clears the pick status of the undone selection",
        cleared.is_some(),
        format!("before {stale:?} · after {:?}", pick_text()),
    );
    let _ = ready(20_000).await;
    Some(())
}

pub(super) async fn run(c: &p::PickSmoke, r: &mut R) -> Option<()> {
    ipc::sleep(300).await;
    let mut s = Stats::default();
    r.step(
        "input path",
        true,
        if c.real_pointer {
            "real X pointer clicks inside the harness Xvfb (calibrated in round 1)"
        } else {
            "dispatched pointer events on the viewport (no harness Xvfb)"
        },
    );
    for (i, theme) in c.themes.iter().enumerate() {
        workflow(c, theme, i == 0, &mut s, r).await?;
    }
    matrix(c, &mut s, r).await?;
    let (late, sup) = PICK.with_borrow(|p| (p.late_refused, p.superseded));
    r.step(
        "object clicks used real X pointer input",
        !c.real_pointer || REAL.get().is_some(),
        format!("{:?}", REAL.get()),
    );
    r.step(
        "WRONG_SOURCE_SELECTIONS = 0",
        s.wrong == 0,
        format!(
            "picks {} · proven {} · refused {} · no-hit {} · wrong {} · oracle mismatches {} · skipped {} · late refused {late} · superseded {sup}",
            s.picks, s.proven, s.refused, s.no_hit, s.wrong, s.mismatched, s.skipped
        ),
    );
    r.step(
        "pick timing (adapter snapshot / click → applied result)",
        true,
        format!(
            "snapshot median {:.2} ms p95 {:.2} ms max {:.2} ms · total median {:.1} ms p95 {:.1} ms max {:.1} ms (n={})",
            pct(&s.snapshot_ms, 0.5),
            pct(&s.snapshot_ms, 0.95),
            pct(&s.snapshot_ms, 1.0),
            pct(&s.total_ms, 0.5),
            pct(&s.total_ms, 0.95),
            pct(&s.total_ms, 1.0),
            s.total_ms.len()
        ),
    );
    Some(())
}
