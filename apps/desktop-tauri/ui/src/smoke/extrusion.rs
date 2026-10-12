// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-4A in-window workflow (`--smoke-extrusion`): New World → Create
//! Extrusion → Extrusion Inspector (outline, spine, scale, orientation, a
//! flag) → refused invalid values → outline editor Cancel → outline editor
//! session (set, drag, add, insert, delete) Apply → Undo / Redo every step →
//! Save As → Close → Reopen → one coordinate edit in LF, BOM + CRLF and
//! lone-CR fixtures → render the research files read-only. Every action uses
//! the real controls. After EVERY committed action the whole source must
//! equal the previous source with exactly the intended replacement; Rust
//! re-checks the files on disk at the end.

use leptos::prelude::*;
use web_sys::{PointerEvent, PointerEventInit};
use wrlforge_desktop_protocol as p;

use super::create::{all, el, previewed, text, tree_item, wait_ms};
use super::{click, set_input, snapshot, R};
use crate::editor::{self, CORE};
use crate::extrusion::outline;
use crate::ipc::{self, call};
use crate::ui::{self, ui};

const NEW_WORLD: &str = "#VRML V2.0 utf8\n";

async fn capture(c: &p::ExtrusionSmoke, id: &str, r: &mut R) {
    if c.window_capture {
        ipc::sleep(150).await;
        #[derive(serde::Serialize)]
        struct A<'a> {
            id: &'a str,
        }
        let res = call::<String>("smoke_window_capture", A { id }).await;
        r.step(
            &format!("evidence: window capture {id}"),
            res.is_ok(),
            format!("{res:?}"),
        );
    }
    if c.hold_ms > 0 {
        ipc::sleep(c.hold_ms as i32).await;
    }
}

async fn viewport_capture(id: &str) -> String {
    let Some(png) = ipc::preview_capture().await else {
        return "no capture".into();
    };
    #[derive(serde::Serialize)]
    struct C {
        id: String,
        png: Vec<u8>,
    }
    call::<String>("smoke_texture_capture", C { id: id.into(), png })
        .await
        .unwrap_or_else(|e| e)
}

/// Wait for a NEW preview load (after `loads`) of revision 0 to finish:
/// never a status left over from the previous document.
async fn loaded_new(loads: u64, ms: u32) -> Option<String> {
    let u = ui();
    wait_ms(ms, || {
        let s = u.preview_status.get_untracked();
        (u.preview_loads.get_untracked() > loads && s.starts_with("rev 0 · loaded")).then_some(s)
    })
    .await
}

fn rev() -> u64 {
    CORE.with_borrow(|c| c.revision)
}

/// Wait until the Inspector shows `title` for the current revision.
async fn inspected(title: &str) -> Option<p::Inspection> {
    let want = rev();
    wait_ms(5000, || {
        ui().inspection
            .get_untracked()
            .filter(|i| i.title == title && i.revision == want)
    })
    .await
}

/// Select the Scene Tree item `label` of the CURRENT analysis.
async fn select(label: &str) -> Option<p::Inspection> {
    let u = ui();
    for _ in 0..3 {
        let want = rev();
        wait_ms(4000, || {
            (u.analysis
                .with_untracked(|a| a.as_ref().map(|a| a.revision))
                == Some(want))
            .then_some(())
        })
        .await;
        if let Some(li) = tree_item(label) {
            li.click();
        }
        if let Some(i) = inspected(label).await {
            // The panel is rendered from this inspection.
            wait_ms(2000, || el("#extrusion-panel")).await;
            return Some(i);
        }
    }
    None
}

/// Run `act`, then require exactly one new revision whose source equals
/// `before` with `old` replaced by `new` (first occurrence) and nothing else.
async fn commit(
    r: &mut R,
    name: &str,
    old: &str,
    new: &str,
    act: impl FnOnce() -> Option<()>,
) -> Option<()> {
    editor::idle().await;
    let (before, r0) = (text(), rev());
    let want = before.replacen(old, new, 1);
    let found = before.contains(old);
    act()?;
    let moved = wait_ms(6000, || (rev() == r0 + 1).then_some(())).await;
    editor::idle().await;
    ipc::sleep(30).await;
    let after = text();
    let s = snapshot().await?;
    let ok = moved.is_some() && found && after == want && s.view == after && s.revision == r0 + 1;
    r.step(
        &format!("{name}: one revision; the source differs by exactly `{old}` → `{new}`"),
        ok,
        format!(
            "rev {r0} → {} · {}",
            s.revision,
            ui().field_error.get_untracked().unwrap_or_default()
        ),
    );
    ok.then_some(())
}

/// Run `act`, which must be REFUSED: no revision, same text, an error shown.
async fn refused(r: &mut R, name: &str, act: impl FnOnce() -> Option<()>) -> Option<()> {
    editor::idle().await;
    let u = ui();
    u.field_error.set(None);
    let (before, r0) = (text(), rev());
    act()?;
    let err = wait_ms(4000, || u.field_error.get_untracked()).await;
    ipc::sleep(100).await;
    let ok = err.is_some() && rev() == r0 && text() == before;
    r.step(
        &format!("{name}: refused with a message; nothing changed"),
        ok,
        err.unwrap_or_default(),
    );
    Some(())
}

fn set_row(field: &str, row: usize, values: &[&str]) -> Option<()> {
    for (c, v) in values.iter().enumerate() {
        set_input(&format!("le-{field}-{row}-{c}"), v)?;
    }
    click(&format!("le-{field}-{row}-set"))
}

fn count_attr(field: &str) -> Option<usize> {
    el(&format!("table.xlist[data-field=\"{field}\"]"))?
        .get_attribute("data-count")?
        .parse()
        .ok()
}

fn pointer(target: &web_sys::Element, kind: &str, x: f64, y: f64) {
    let init = PointerEventInit::new();
    init.set_client_x(x as i32);
    init.set_client_y(y as i32);
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_pointer_id(1);
    init.set_is_primary(true);
    init.set_buttons(if kind == "pointerup" { 0 } else { 1 });
    if let Ok(ev) = PointerEvent::new_with_event_init_dict(kind, &init) {
        let _ = target.dispatch_event(&ev);
    }
}

fn circle(i: usize) -> Option<web_sys::Element> {
    el(&format!("#outline-svg circle.pt[data-index=\"{i}\"]"))
}

/// Press (and release) on point `i` of the outline view: selects it.
async fn pick_point(i: usize) -> Option<()> {
    let c = circle(i)?;
    let b = c.get_bounding_client_rect();
    let (x, y) = (b.left() + b.width() / 2.0, b.top() + b.height() / 2.0);
    pointer(&c, "pointerdown", x, y);
    let svg = el("#outline-svg")?;
    pointer(&svg, "pointerup", x, y);
    ipc::sleep(30).await;
    (outline().get_untracked()?.selected == Some(i)).then_some(())
}

/// Client coordinates of model point (x, z) in the outline view.
fn client_of(x: f64, z: f64) -> Option<(f64, f64)> {
    let o = outline().get_untracked()?;
    let svg = el("#outline-svg")?;
    let b = svg.get_bounding_client_rect();
    let (x0, z0, size) = o.view;
    Some((
        b.left() + (x - x0) / size * b.width(),
        b.top() + (z - z0) / size * b.height(),
    ))
}

fn ops() -> usize {
    outline()
        .get_untracked()
        .map(|o| o.ops.len())
        .unwrap_or(usize::MAX)
}

fn shown() -> bool {
    el("#outline-editor").is_some_and(|e| !e.has_attribute("hidden"))
}

fn outline_points() -> Vec<String> {
    outline()
        .get_untracked()
        .map(|o| {
            o.points
                .iter()
                .map(|p| format!("{} {}", p[0], p[1]))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) async fn run(c: &p::ExtrusionSmoke, r: &mut R) -> Option<()> {
    let u = ui();
    ipc::sleep(300).await;

    // --- New World → Create Extrusion through the menu. ---
    click("btn-new")?;
    wait_ms(5000, || u.doc.get_untracked().filter(|d| d.untitled)).await?;
    previewed(0, 0, 20_000).await;
    let bg = ipc::preview_coverage().await;
    click("btn-create")?;
    wait_ms(2000, || el("#create-extrusion")).await?;
    let menu = all("#create-list button[role=menuitem]").len();
    r.step(
        "Create menu lists Box, Sphere, Cylinder, Cone and Extrusion",
        menu == 5,
        format!("{menu} items"),
    );
    let t0 = editor::now();
    click("create-extrusion")?;
    let st = previewed(1, 1, 20_000).await;
    let t1 = editor::now();
    let cov = ipc::preview_coverage().await;
    r.step(
        "Create Extrusion: the viewport renders the new geometry",
        st.is_some() && cov > bg + 0.002,
        format!(
            "click→drawn {:.0} ms; covered {:.2}% → {:.2}%; {}",
            t1 - t0,
            bg * 100.0,
            cov * 100.0,
            u.preview_status.get_untracked()
        ),
    );
    let created = text();
    r.step(
        "Create Extrusion: one undo step appends DEF Extrusion_1 Transform { … Shape … geometry Extrusion { crossSection … spine … } }",
        created.starts_with(NEW_WORLD)
            && created.contains("DEF Extrusion_1 Transform {")
            && created.contains("geometry Extrusion {\n        crossSection [ 1 0, 0.5 -0.866, -0.5 -0.866, -1 0, -0.5 0.866, 0.5 0.866, 1 0 ]\n        spine [ 0 -1 0, 0 1 0 ]\n      }")
            && !created.contains("scale [")
            && rev() == 1,
        format!("rev {}", rev()),
    );
    let li = wait_ms(4000, || {
        tree_item("Transform Extrusion_1").filter(|l| l.class_list().contains("selected"))
    })
    .await;
    r.step(
        "Create Extrusion: Scene Tree shows and selects Transform Extrusion_1",
        li.is_some(),
        "",
    );
    let vp = viewport_capture("viewport-created").await;
    r.step(
        "evidence: viewport capture after Create",
        vp.starts_with("captures/"),
        vp,
    );
    capture(c, "01-created", r).await;

    // --- The Extrusion Inspector. ---
    let insp = select("Extrusion").await;
    let sections = all("#extrusion-panel section.xsec").len();
    let notes = el("#extrusion-notes")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    r.step(
        "Inspector: Extrusion sections (cross-section, spine, scale, orientation, caps/flags) with authored values",
        insp.is_some()
            && sections == 5
            && count_attr("crossSection") == Some(7)
            && count_attr("spine") == Some(2)
            && all("table.xlist[data-field=\"crossSection\"] tbody input").len() == 14
            && all("table.xlist[data-field=\"spine\"] tbody input").len() == 6,
        format!("{sections} sections · notes: {notes}"),
    );
    let defaults = [
        "scale",
        "orientation",
        "beginCap",
        "endCap",
        "solid",
        "ccw",
        "convex",
        "creaseAngle",
    ]
    .iter()
    .all(|f| {
        el(&format!(".xfield-head[data-field=\"{f}\"]"))
            .is_some_and(|e| e.get_attribute("data-authored").as_deref() == Some("false"))
            && el(&format!("#xadd-{f}")).is_some()
    });
    r.step(
        "Inspector: unwritten fields show their VRML97 defaults and are NOT written automatically",
        defaults && !text().contains("solid") && notes.contains("Closed outline, 7 point(s)"),
        notes.clone(),
    );
    capture(c, "02-inspector", r).await;

    // --- Committed edits, each one exact replacement. ---
    commit(
        r,
        "crossSection point 2",
        "0.5 -0.866, -0.5",
        "0.5 -0.9, -0.5",
        || set_row("crossSection", 1, &["0.5", "-0.9"]),
    )
    .await?;
    let st = previewed(rev(), 1, 20_000).await;
    r.step(
        "crossSection edit reached the viewport (current revision rendered)",
        st.is_some(),
        u.preview_status.get_untracked(),
    );
    inspected("Extrusion").await?;
    commit(r, "spine point 2", "0 1 0 ]", "0 1.5 0 ]", || {
        set_row("spine", 1, &["0", "1.5", "0"])
    })
    .await?;
    inspected("Extrusion").await?;
    commit(
        r,
        "Write scale (explicit)",
        "0 1.5 0 ]",
        "0 1.5 0 ]\n        scale [ 1 1 ]",
        || click("xadd-scale"),
    )
    .await?;
    inspected("Extrusion").await?;
    commit(
        r,
        "scale value",
        "scale [ 1 1 ]",
        "scale [ 1.25 1.25 ]",
        || set_row("scale", 0, &["1.25", "1.25"]),
    )
    .await?;
    inspected("Extrusion").await?;
    refused(r, "scale -1 (VRML97 scale must be > 0)", || {
        set_row("scale", 0, &["-1", "1"])
    })
    .await?;
    refused(r, "malformed number 1..5", || {
        set_row("spine", 0, &["1..5", "0", "0"])
    })
    .await?;
    refused(r, "delete the only scale value", || click("le-scale-0-del")).await?;
    commit(
        r,
        "Write orientation (explicit)",
        "scale [ 1.25 1.25 ]",
        "scale [ 1.25 1.25 ]\n        orientation [ 0 0 1 0 ]",
        || click("xadd-orientation"),
    )
    .await?;
    inspected("Extrusion").await?;
    refused(r, "orientation axis 0 0 0", || {
        set_row("orientation", 0, &["0", "0", "0", "1"])
    })
    .await?;
    commit(
        r,
        "orientation value",
        "orientation [ 0 0 1 0 ]",
        "orientation [ 0 1 0 0.3 ]",
        || set_row("orientation", 0, &["0", "1", "0", "0.3"]),
    )
    .await?;
    inspected("Extrusion").await?;
    commit(
        r,
        "Write beginCap FALSE (explicit)",
        "orientation [ 0 1 0 0.3 ]",
        "orientation [ 0 1 0 0.3 ]\n        beginCap FALSE",
        || {
            let s: web_sys::HtmlSelectElement = crate::element_by_id("xf-beginCap")?;
            s.set_value("FALSE");
            click("xadd-beginCap")
        },
    )
    .await?;
    inspected("Extrusion").await?;
    let st = previewed(rev(), 1, 20_000).await;
    r.step(
        "flag edit reached the viewport",
        st.is_some(),
        u.preview_status.get_untracked(),
    );

    // --- Outline editor: Cancel leaves source and history unchanged. ---
    let s0 = snapshot().await?;
    click("btn-outline")?;
    wait_ms(2000, || shown().then_some(())).await;
    let drawn = all("#outline-svg circle.pt").len();
    let axes = el("#outline-svg")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    r.step(
        "Outline editor opens: 2D grid, axis directions, 6 points drawn (the closing point once)",
        shown()
            && drawn == 6
            && axes.contains("+x →")
            && axes.contains("+z ↓")
            && all("#outline-svg line.grid").len() > 4,
        format!("{drawn} points"),
    );
    pick_point(2).await?;
    set_input("outline-x", "9")?;
    set_input("outline-z", "9")?;
    click("outline-set")?;
    click("outline-add")?;
    let pending = ops();
    click("outline-cancel")?;
    ipc::sleep(150).await;
    let s1 = snapshot().await?;
    r.step(
        "Outline Cancel: dialog closes; source, revision and undo history unchanged",
        pending == 2
            && !shown()
            && s1.revision == s0.revision
            && s1.view == s0.view
            && s1.can_undo == s0.can_undo
            && s1.can_redo == s0.can_redo,
        format!("{pending} pending change(s) discarded"),
    );

    // --- Outline editor: one session, one Apply, one undo step. ---
    click("btn-outline")?;
    wait_ms(2000, || shown().then_some(())).await?;
    pick_point(0).await?;
    click("outline-delete")?;
    ipc::sleep(50).await;
    let msg = el("#outline-message")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    r.step(
        "Outline: deleting the closing point is refused locally (the outline is never silently opened)",
        ops() == 0 && msg.contains("closing point"),
        msg,
    );
    pick_point(2).await?;
    set_input("outline-x", "-0.6")?;
    set_input("outline-z", "-1")?;
    click("outline-set")?;
    // Drag point 4 with Snap on to the model point (-1.5, 0).
    let snap: web_sys::HtmlInputElement = crate::element_by_id("outline-snap")?;
    snap.set_checked(true);
    let _ = snap.dispatch_event(&web_sys::Event::new("change").ok()?);
    pick_point(3).await?;
    let (sx, sy) = {
        let c3 = circle(3)?.get_bounding_client_rect();
        (c3.left() + c3.width() / 2.0, c3.top() + c3.height() / 2.0)
    };
    let (tx, ty) = client_of(-1.5, 0.02)?;
    pointer(&circle(3)?, "pointerdown", sx, sy);
    let svg = el("#outline-svg")?;
    for k in 1..=6 {
        let f = k as f64 / 6.0;
        pointer(&svg, "pointermove", sx + (tx - sx) * f, sy + (ty - sy) * f);
        ipc::sleep(16).await;
    }
    pointer(&svg, "pointerup", tx, ty);
    ipc::sleep(50).await;
    let dragged = outline_points().get(3).cloned().unwrap_or_default();
    r.step(
        "Outline: dragging point 4 (snap on) moves it to -1.5 0",
        dragged == "-1.5 0",
        dragged,
    );
    click("outline-add")?;
    pick_point(1).await?;
    click("outline-insert")?;
    pick_point(5).await?;
    click("outline-delete")?;
    ipc::sleep(50).await;
    let pts = outline_points();
    let status = el("#outline-status")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    r.step(
        "Outline: set, drag, add, insert-between and delete give the intended working outline (order kept, still closed)",
        pts.join(", ") == "1 0, 0.5 -0.9, -0.05 -0.95, -0.6 -1, -1.5 0, 0.5 0.866, 0.75 0.433, 1 0" && status.contains("closed"),
        format!("{} · {status}", pts.join(", ")),
    );
    capture(c, "03-outline-editor", r).await;
    let old = "crossSection [ 1 0, 0.5 -0.9, -0.5 -0.866, -1 0, -0.5 0.866, 0.5 0.866, 1 0 ]";
    let new =
        "crossSection [ 1 0, 0.5 -0.9, -0.05 -0.95, -0.6 -1, -1.5 0, 0.5 0.866, 0.75 0.433, 1 0 ]";
    commit(r, "Outline Apply (one session)", old, new, || {
        click("outline-apply")
    })
    .await?;
    r.step(
        "Outline Apply closes the editor",
        wait_ms(2000, || (!shown()).then_some(())).await.is_some(),
        "",
    );
    let st = previewed(rev(), 1, 20_000).await;
    let vp = viewport_capture("viewport-after-outline").await;
    r.step(
        "Outline edit reached the viewport (current revision rendered)",
        st.is_some(),
        format!("{} · {vp}", u.preview_status.get_untracked()),
    );
    inspected("Extrusion").await?;
    r.step(
        "Inspector shows the new outline (8 points)",
        count_attr("crossSection") == Some(8),
        "",
    );
    capture(c, "04-after-outline", r).await;

    // --- Undo / Redo: one step per committed action. ---
    let final_text = text();
    let final_rev = rev();
    let mut chain = vec![final_text.clone()];
    for k in 0..9 {
        let r0 = rev();
        click("btn-undo")?;
        if wait_ms(5000, || (rev() == r0 + 1).then_some(()))
            .await
            .is_none()
        {
            r.step(&format!("Undo {}", k + 1), false, "no new revision");
            return None;
        }
        editor::idle().await;
        chain.push(text());
    }
    r.step(
        "Undo ×9: each committed action is one step; the 8th undo leaves exactly the created object; the 9th removes it completely",
        chain[8] == created && chain[9] == NEW_WORLD && chain.windows(2).all(|w| w[0] != w[1]),
        format!("revisions {final_rev} → {}", rev()),
    );
    let st = previewed(rev(), 0, 20_000).await;
    r.step(
        "after removing the object the viewport shows the empty world",
        st.is_some(),
        u.preview_status.get_untracked(),
    );
    for _ in 0..9 {
        let r0 = rev();
        click("btn-redo")?;
        wait_ms(5000, || (rev() == r0 + 1).then_some(())).await?;
        editor::idle().await;
    }
    r.step(
        "Redo ×9 restores the exact source text",
        text() == final_text,
        "",
    );
    let mid = {
        // Redo after Undo of the create restores the exact created text.
        let r0 = rev();
        for _ in 0..8 {
            click("btn-undo")?;
        }
        wait_ms(8000, || (rev() == r0 + 8).then_some(())).await?;
        editor::idle().await;
        let t = text();
        for _ in 0..8 {
            let r1 = rev();
            click("btn-redo")?;
            wait_ms(5000, || (rev() == r1 + 1).then_some(())).await?;
        }
        editor::idle().await;
        t
    };
    r.step(
        "Undo back to the creation leaves exactly the generated source",
        mid == created && text() == final_text,
        "",
    );

    // --- Save As → Close → Reopen. ---
    click("btn-save")?;
    let saved = wait_ms(6000, || {
        u.doc.get_untracked().filter(|d| !d.untitled && !d.dirty)
    })
    .await;
    r.step(
        "Save on the new world runs Save As (Rust-chosen temporary path)",
        saved.is_some(),
        u.message.get_untracked(),
    );
    click("btn-close")?;
    wait_ms(4000, || {
        CORE.with_borrow(|c| c.session.is_none()).then_some(())
    })
    .await;
    click("btn-open")?;
    let reopened = wait_ms(6000, || u.doc.get_untracked().filter(|d| !d.untitled)).await;
    let same = reopened.as_ref().is_some_and(|d| d.view == final_text);
    r.step(
        "Reopen: the saved file shows exactly the same source",
        same,
        "",
    );
    previewed(0, 1, 20_000).await;
    let insp = select("Extrusion").await;
    r.step(
        "Reopen: the Extrusion Inspector shows the same outline, spine, scale, orientation and beginCap",
        insp.is_some()
            && count_attr("crossSection") == Some(8)
            && count_attr("spine") == Some(2)
            && count_attr("scale") == Some(1)
            && count_attr("orientation") == Some(1)
            && el(".xfield-head[data-field=\"beginCap\"]").is_some_and(|e| e.get_attribute("data-authored").as_deref() == Some("true")),
        "",
    );
    // Picking the Extrusion object selects its Transform (VISUAL-2 promotion).
    capture(c, "05-reopened", r).await;

    // --- Source-format fixtures: one coordinate each, then Save. ---
    #[derive(serde::Serialize)]
    struct A {
        index: usize,
    }
    for (i, label) in c.fixtures.iter().enumerate() {
        let tag = format!("[fixture {label}]");
        let loads = u.preview_loads.get_untracked();
        let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: i })
            .await
            .ok()?;
        ui::apply_open(o, "Opened").await;
        let loaded = loaded_new(loads, 20_000).await;
        r.step(
            &format!("{tag}: opened and previewed (3 root nodes)"),
            loaded
                .as_deref()
                .is_some_and(|s| s.contains("loaded: 3 root node(s)")),
            u.preview_status.get_untracked(),
        );
        select("Extrusion").await?;
        commit(
            r,
            &format!("{tag} crossSection point 2"),
            "[ 1 0, 0 -1, -1 0,",
            "[ 1 0, 0 -1.5, -1 0,",
            || {
                set_input("le-crossSection-1-1", "-1.5")?;
                click("le-crossSection-1-set")
            },
        )
        .await?;
        let use_kept = text().contains("children [ USE Body ]")
            && text().contains("PROTO Keep")
            && text().contains("# diamond ✓");
        r.step(
            &format!("{tag}: DEF/USE, PROTO and comments are untouched"),
            use_kept,
            "",
        );
        click("btn-save")?;
        wait_ms(6000, || (!u.dirty.get_untracked()).then_some(())).await?;
        r.step(&format!("{tag}: saved"), !u.dirty.get_untracked(), "");
    }

    // --- Research files: render by the VRML97 rule, read-only. ---
    for (j, name) in c.research.iter().enumerate() {
        let tag = format!("[research {name}]");
        let loads = u.preview_loads.get_untracked();
        let o = call::<p::OpenOutcome>(
            "smoke_open_fixture",
            A {
                index: c.fixtures.len() + j,
            },
        )
        .await
        .ok()?;
        ui::apply_open(o, "Opened").await;
        let loaded = loaded_new(loads, 30_000).await;
        ipc::sleep(300).await;
        let cov = ipc::preview_coverage().await;
        let id = format!("research-{j}");
        let vp = viewport_capture(&id).await;
        let n_ext = u
            .analysis
            .with_untracked(|a| {
                a.as_ref().map(|a| {
                    a.items
                        .iter()
                        .filter(|i| i.label.starts_with("Extrusion"))
                        .count()
                })
            })
            .unwrap_or(0);
        let first = u.analysis.with_untracked(|a| {
            a.as_ref()
                .and_then(|a| a.items.iter().find(|i| i.label.starts_with("Extrusion")))
                .map(|i| i.label.clone())
        });
        let panel = match &first {
            Some(l) => select(l).await.is_some() && el("#extrusion-panel").is_some(),
            None => false,
        };
        let notes = el("#extrusion-notes")
            .and_then(|e| e.text_content())
            .unwrap_or_default();
        let clean = snapshot()
            .await
            .is_some_and(|d| d.revision == 0 && !d.dirty);
        r.step(
            &format!("{tag}: X_ITE renders it (VRML97 rule); the Extrusion Inspector opens; nothing is written"),
            loaded.is_some() && cov > 0.001 && panel && clean,
            format!("{n_ext} Extrusion node(s) · {} · covered {:.2}% · {vp} · first: {first:?} · notes: {notes}", loaded.unwrap_or_default(), cov * 100.0),
        );
        capture(c, &format!("06-research-{j}"), r).await;
    }
    Some(())
}
