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
    let tr_inputs = all("tr[data-field=\"translation\"] input")
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
            && after == before,
        format!(
            "{} {} · tree {} · inspector {} · translation {:?} · unchanged {} · {:.1} ms",
            out.status,
            out.reason,
            li.is_some(),
            insp.is_some(),
            tr_inputs,
            after == before,
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
        for ck in &fx.clicks {
            if let Some(why) = &ck.skip {
                skipped += 1;
                detail.push(format!("{}:SKIP({why})", ck.id));
                continue;
            }
            let keep = selected_id();
            let before = untouched().await?;
            let (x, y) = project(ck.world, fx.camera)?;
            let Some(out) = gesture(x, y).await else {
                detail.push(format!("{}:no-result", ck.id));
                continue;
            };
            s.record(&out);
            let span = out.logical.as_ref().map(|l| [l.from, l.to]);
            let is_wrong = out.is_proven() && (ck.status != "PROVEN" || span != ck.logical);
            let after = untouched().await?;
            let good = !is_wrong
                && out.status == ck.status
                && after == before
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
            detail.push(format!(
                "{}:{}{}{}",
                ck.id,
                out.status,
                if good { "" } else { "≠" },
                if good {
                    String::new()
                } else {
                    format!("{}({})", ck.status, out.reason)
                }
            ));
        }
        s.wrong += wrong;
        s.skipped += skipped;
        let run = fx.clicks.len() as u32 - skipped;
        r.step(
            &format!(
                "matrix {}: {ok}/{run} clicks as the oracle expects, WRONG {wrong}",
                fx.id
            ),
            opened.is_some() && loaded.is_some() && ok == run && wrong == 0,
            detail.join(" "),
        );
    }
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
