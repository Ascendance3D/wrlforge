// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-1 in-window workflow (`--smoke-create`): New World → Create Box →
//! Inspector translation → Material color → Undo → Redo → New World
//! (Cancel) → Save As → Close → Reopen → Create Sphere / Cone / Cylinder →
//! Save → themes → New World (Discard). Every action goes through the real
//! toolbar, menu, Scene Tree and Inspector controls. Rust answers the native
//! dialogs from its own plan and verifies the files on disk afterwards.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent, KeyboardEventInit};
use wrlforge_desktop_protocol as p;

use super::{choose, click, doc_el, doc_state, key, snapshot, R};
use crate::editor::{self, textarea, CORE};
use crate::ipc::{self, call};
use crate::ui::{self, ui};

/// Poll `f` every 20 ms for up to `ms`.
async fn wait_ms<T>(ms: u32, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..(ms / 20).max(1) {
        if let Some(v) = f() {
            return Some(v);
        }
        ipc::sleep(20).await;
    }
    None
}

fn el(sel: &str) -> Option<web_sys::Element> {
    doc_el()?.query_selector(sel).ok().flatten()
}

fn all(sel: &str) -> Vec<web_sys::Element> {
    let Some(list) = doc_el().and_then(|d| d.query_selector_all(sel).ok()) else {
        return vec![];
    };
    (0..list.length())
        .filter_map(|i| list.item(i)?.dyn_into().ok())
        .collect()
}

fn disabled(id: &str) -> bool {
    crate::element_by_id::<web_sys::HtmlButtonElement>(id).is_some_and(|b| b.disabled())
}

fn active_id() -> String {
    doc_el()
        .and_then(|d| d.active_element())
        .map(|e| e.id())
        .unwrap_or_default()
}

fn keydown(target: &str, k: &str) {
    let init = KeyboardEventInit::new();
    init.set_key(k);
    init.set_bubbles(true);
    init.set_cancelable(true);
    if let (Some(t), Ok(ev)) = (
        crate::element_by_id::<HtmlElement>(target),
        KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init),
    ) {
        let _ = t.dispatch_event(&ev);
    }
}

fn text() -> String {
    textarea().map(|t| t.value()).unwrap_or_default()
}

/// The Scene Tree `<li>` whose label is exactly `label` (first in order).
fn tree_item(label: &str) -> Option<HtmlElement> {
    all("li.tree-item")
        .into_iter()
        .find(|li| {
            li.query_selector(".label")
                .ok()
                .flatten()
                .and_then(|l| l.text_content())
                .as_deref()
                == Some(label)
        })
        .and_then(|e| e.dyn_into().ok())
}

/// The preview finished loading revision `rev` with `roots` root nodes.
async fn previewed(rev: u64, roots: usize, ms: u32) -> Option<String> {
    let want = format!("rev {rev} · loaded: {roots} root node(s)");
    wait_ms(ms, || {
        let s = ui().preview_status.get_untracked();
        (s == want).then_some(s)
    })
    .await
}

async fn checkpoint(c: &p::CreateSmoke, name: &str) {
    if c.hold_ms == 0 {
        return;
    }
    if let Some(b) = doc_el().and_then(|d| d.body()) {
        let _ = b.set_attribute("data-checkpoint", name);
    }
    ui::flash(&format!("checkpoint: {name}"));
    ipc::sleep(c.hold_ms as i32).await;
}

/// Create `prim` through the toolbar menu with the mouse path; checks every
/// view and measures click → visible geometry. Returns the DEF name.
async fn create_one(
    c: &p::CreateSmoke,
    prim: p::Primitive,
    nth: usize,
    roots: usize,
    r: &mut R,
) -> Option<String> {
    let label = prim.label();
    let name = format!("{label}_{nth}");
    let rev0 = CORE.with_borrow(|c| c.revision);
    let before = ipc::preview_coverage().await;
    click("btn-create")?;
    wait_ms(2000, || el("#create-list")).await?;
    let id = format!("create-{}", label.to_lowercase());
    let t0 = editor::now();
    click(&id)?;
    let rev = rev0 + 1;
    let st = previewed(rev, roots, 20_000).await;
    let t_loaded = editor::now();
    let cov = ipc::preview_coverage().await;
    let t_drawn = editor::now();
    r.step(
        &format!("Create {label}: viewport renders the new geometry"),
        st.is_some() && cov > before + 0.002,
        format!(
            "click→scene loaded {:.0} ms, click→frame drawn {:.0} ms; covered pixels {:.2}% → {:.2}%; {}",
            t_loaded - t0,
            t_drawn - t0,
            before * 100.0,
            cov * 100.0,
            ui().preview_status.get_untracked()
        ),
    );
    let s = snapshot().await?;
    let src = text();
    r.step(
        &format!("Create {label}: source editor shows the generated VRML (rev {rev})"),
        s.revision == rev
            && s.dirty
            && src.contains(&format!("DEF {name} Transform {{"))
            && src.contains(&format!("geometry {label} {{"))
            && p::view_hash(&src) == p::view_hash(&s.view),
        format!("rev {}", s.revision),
    );
    let hl = wait_ms(3000, || {
        el("#source-hl")
            .filter(|e| e.get_attribute("data-state").as_deref() == Some("exact"))
            .filter(|e| e.inner_html().contains(&name))
    })
    .await;
    r.step(
        &format!("Create {label}: syntax colors cover the new source"),
        hl.is_some(),
        "",
    );
    let li = wait_ms(3000, || {
        tree_item(&format!("Transform {name}")).filter(|li| li.class_list().contains("selected"))
    })
    .await;
    let sel = ui().selected.get_untracked();
    r.step(
        &format!("Create {label}: Scene Tree shows and selects Transform {name}"),
        li.is_some() && sel.as_ref().is_some_and(|s| s.revision == rev),
        format!("{sel:?}"),
    );
    let insp = wait_ms(3000, || {
        ui().inspection
            .get_untracked()
            .filter(|i| i.title == format!("Transform {name}") && i.revision == rev)
    })
    .await;
    let counts = (
        all("tr[data-field=\"translation\"] input").len(),
        all("tr[data-field=\"rotation\"] input").len(),
        all("tr[data-field=\"scale\"] input").len(),
    );
    r.step(
        &format!("Create {label}: Inspector shows editable translation / rotation / scale"),
        insp.is_some() && counts == (3, 4, 3),
        format!("{counts:?}"),
    );
    checkpoint(c, &format!("created-{}", label.to_lowercase())).await;
    Some(name)
}

/// Type `values` into the field's component boxes and press its Apply
/// button; wait for the new revision.
async fn apply_field(field: &str, values: &[String]) -> Option<p::DocState> {
    let inputs = all(&format!("tr[data-field=\"{field}\"] input"));
    if inputs.len() != values.len() {
        return None;
    }
    for (i, v) in inputs.iter().zip(values) {
        i.clone()
            .dyn_into::<web_sys::HtmlInputElement>()
            .ok()?
            .set_value(v);
    }
    let rev0 = CORE.with_borrow(|c| c.revision);
    el(&format!("tr[data-field=\"{field}\"] button"))?
        .dyn_into::<HtmlElement>()
        .ok()?
        .click();
    wait_ms(5000, || {
        (CORE.with_borrow(|c| c.revision) == rev0 + 1).then_some(())
    })
    .await?;
    editor::idle().await;
    let s = snapshot().await?;
    Some(p::DocState {
        revision: s.revision,
        dirty: s.dirty,
        view_len: 0,
        view_hash: p::view_hash(&s.view),
        caret: 0,
        can_undo: s.can_undo,
        can_redo: s.can_redo,
    })
}

pub(super) async fn run(c: &p::CreateSmoke, r: &mut R) -> Option<()> {
    let u = ui();
    // --- Startup: no document. ---
    ipc::sleep(300).await;
    r.step(
        "startup: no document; Create, Save and Close are disabled",
        CORE.with_borrow(|c| c.session.is_none())
            && disabled("btn-create")
            && disabled("btn-save")
            && disabled("btn-close")
            && !disabled("btn-new"),
        "",
    );

    // --- New World. ---
    click("btn-new")?;
    let doc = wait_ms(5000, || u.doc.get_untracked().filter(|d| d.untitled)).await;
    let ok = doc.as_ref().is_some_and(|d| {
        d.view == "#VRML V2.0 utf8\n" && !d.dirty && d.revision == 0 && text() == d.view
    });
    r.step(
        "New World: an untitled, clean VRML97 world with no file",
        ok,
        format!("{:?}", doc.as_ref().map(|d| (&d.name, &d.display_path))),
    );
    doc?;
    let empty = previewed(0, 0, 20_000).await;
    let bg = ipc::preview_coverage().await;
    r.step(
        "New World: the viewport shows an empty world",
        empty.is_some() && (0.0..0.001).contains(&bg),
        format!(
            "{} · covered {:.3}% · {}",
            u.preview_status.get_untracked(),
            bg * 100.0,
            ipc::preview_probe()
        ),
    );
    checkpoint(c, "new-world").await;

    // --- The Create menu by keyboard (then closed without creating). ---
    if let Some(b) = crate::element_by_id::<HtmlElement>("btn-create") {
        let _ = b.focus();
    }
    keydown("btn-create", "ArrowDown");
    let first = wait_ms(2000, || (active_id() == "create-box").then_some(())).await;
    keydown("create-box", "ArrowUp");
    let wrap = active_id() == "create-cone";
    keydown("create-cone", "Home");
    let home = active_id() == "create-box";
    keydown("create-box", "Escape");
    ipc::sleep(50).await;
    r.step(
        "Create menu: keyboard opens, moves (wraps, Home) and Escape closes to the button",
        first.is_some()
            && wrap
            && home
            && el("#create-list").is_none()
            && active_id() == "btn-create"
            && CORE.with_borrow(|c| c.revision) == 0,
        format!("active {}", active_id()),
    );

    // --- Create Box. ---
    let name = create_one(c, p::Primitive::Box, 1, 1, r).await?;

    // --- Inspector: translation. ---
    let t = apply_field("translation", &c.translation).await;
    let tr = format!("translation {}", c.translation.join(" "));
    r.step(
        "Inspector: translation edited through the real controls",
        t.is_some() && text().contains(&tr),
        tr.clone(),
    );
    let st = previewed(CORE.with_borrow(|c| c.revision), 1, 20_000).await;
    r.step(
        "Inspector: translation edit reached the viewport",
        st.is_some(),
        u.preview_status.get_untracked(),
    );

    // --- Scene Tree → Material → diffuseColor. ---
    tree_item("Material")?.click();
    let insp = wait_ms(3000, || {
        u.inspection
            .get_untracked()
            .filter(|i| i.title == "Material")
    })
    .await;
    r.step(
        "Scene Tree: the new Material is selectable; the Inspector shows diffuseColor",
        insp.is_some() && all("tr[data-field=\"diffuseColor\"] input").len() == 3,
        "",
    );
    let col = apply_field("diffuseColor", &c.color).await;
    let dc = format!("diffuseColor {}", c.color.join(" "));
    r.step(
        "Inspector: diffuseColor edited through the real controls",
        col.is_some() && text().contains(&dc),
        dc.clone(),
    );
    let edited = text();

    // --- Undo / Redo. ---
    let rev = CORE.with_borrow(|c| c.revision);
    click("btn-undo")?;
    wait_ms(5000, || {
        (CORE.with_borrow(|c| c.revision) == rev + 1).then_some(())
    })
    .await?;
    let undone = text();
    r.step(
        "Undo restores the previous color, keeps the translation",
        undone.contains("diffuseColor 0.8 0.3 0.2") && undone.contains(&tr),
        "",
    );
    click("btn-redo")?;
    wait_ms(5000, || {
        (CORE.with_borrow(|c| c.revision) == rev + 2).then_some(())
    })
    .await?;
    let sel = u.selected.get_untracked();
    r.step(
        "Redo re-applies the color; the Material selection is carried",
        text() == edited
            && sel.as_ref().is_some_and(|s| s.revision == rev + 2)
            && wait_ms(3000, || {
                u.inspection
                    .get_untracked()
                    .filter(|i| i.title == "Material" && i.revision == rev + 2)
            })
            .await
            .is_some(),
        format!("{sel:?}"),
    );
    let st = previewed(rev + 2, 1, 20_000).await;
    r.step(
        "Undo/redo: the viewport follows the document",
        st.is_some(),
        u.preview_status.get_untracked(),
    );

    // --- New World on a dirty document: Cancel leaves it unchanged. ---
    let before = doc_state().await?;
    let session = CORE.with_borrow(|c| c.session);
    click("btn-new")?;
    wait_ms(5000, || {
        u.message.get_untracked().contains("canceled").then_some(())
    })
    .await;
    let after = doc_state().await?;
    r.step(
        "New World on unsaved changes: Cancel leaves the document unchanged",
        CORE.with_borrow(|c| c.session) == session
            && after.revision == before.revision
            && after.view_hash == before.view_hash
            && after.dirty
            && text() == edited,
        u.message.get_untracked(),
    );

    // --- Stale actions fail safely. ---
    let a = u.analysis.get_untracked()?;
    let cur = CORE.with_borrow(|c| c.revision);
    let item = a
        .items
        .iter()
        .find(|i| i.label == format!("Transform {name}"))?;
    let sel_before = u.selected.get_untracked();
    let tree_ok = !ui::tree_select(
        a.session,
        cur.saturating_sub(1),
        item.id.clone(),
        item.view_from,
        item.view_to,
    );
    #[derive(serde::Serialize)]
    struct A {
        request: p::CreateRequest,
    }
    let stale = call::<p::CreateOutcome>(
        "doc_create",
        A {
            request: p::CreateRequest {
                session: a.session,
                base_revision: cur.saturating_sub(1),
                primitive: p::Primitive::Cone,
            },
        },
    )
    .await;
    let s = snapshot().await?;
    r.step(
        "Stale Scene Tree action and stale Create are refused; nothing changes",
        tree_ok
            && u.selected.get_untracked() == sel_before
            && matches!(stale, Ok(p::CreateOutcome::Refused { .. }))
            && s.revision == cur
            && s.view == edited,
        format!("{stale:?}"),
    );

    // --- Save (untitled → Save As). ---
    click("btn-save")?;
    let saved = wait_ms(5000, || {
        u.doc.get_untracked().filter(|d| !d.untitled && !d.dirty)
    })
    .await;
    r.step(
        "Save on a new world runs Save As and assigns the file",
        saved
            .as_ref()
            .is_some_and(|d| d.name == "visual1-world.wrl")
            && !u.dirty.get_untracked(),
        u.message.get_untracked(),
    );
    checkpoint(c, "saved").await;

    // --- Close → Reopen. ---
    click("btn-close")?;
    let closed = wait_ms(5000, || u.doc.get_untracked().is_none().then_some(())).await;
    r.step(
        "Close: no document, empty editor, empty Scene Tree",
        closed.is_some() && text().is_empty() && all("li.tree-item").is_empty(),
        u.message.get_untracked(),
    );
    click("btn-open")?;
    let re = wait_ms(5000, || {
        u.doc
            .get_untracked()
            .filter(|d| d.name == "visual1-world.wrl")
    })
    .await;
    let st = previewed(re.as_ref().map_or(0, |d| d.revision), 1, 20_000).await;
    let cov = ipc::preview_coverage().await;
    let tree = wait_ms(5000, || {
        tree_item(&format!("Transform {name}")).zip(tree_item("Material"))
    })
    .await;
    r.step(
        "Reopen: same source, Scene Tree and rendered object",
        re.as_ref().is_some_and(|d| d.view == edited)
            && st.is_some()
            && cov > 0.002
            && tree.is_some(),
        format!("covered {:.2}%", cov * 100.0),
    );
    checkpoint(c, "reopened").await;

    // --- The other primitives, into the reopened (existing) file. ---
    // Each new object is at the origin. In this order each one widens the
    // silhouette (a Cone made after the same-size Cylinder is hidden in it).
    create_one(c, p::Primitive::Sphere, 1, 2, r).await?;
    create_one(c, p::Primitive::Cone, 1, 3, r).await?;
    create_one(c, p::Primitive::Cylinder, 1, 4, r).await?;

    // --- Save (Ctrl+S): the ordinary backup-first save. ---
    if let Some(ta) = textarea() {
        let _ = ta.focus();
    }
    key("s", false);
    let ok = wait_ms(5000, || (!u.dirty.get_untracked()).then_some(())).await;
    r.step(
        "Ctrl+S saves the existing file (backup first)",
        ok.is_some(),
        u.last_save.get_untracked().unwrap_or_default(),
    );

    // --- Themes never alter the document. ---
    let before = doc_state().await?;
    let sel = crate::element_by_id::<web_sys::HtmlSelectElement>("theme-select")?;
    for id in &c.themes {
        choose(&sel, id)?;
        let applied = wait_ms(3000, || {
            (crate::theme::current_attribute().as_deref() == Some(id.as_str())).then_some(())
        })
        .await;
        ipc::sleep(100).await;
        let after = doc_state().await?;
        let cov = ipc::preview_coverage().await;
        r.step(
            &format!("Theme {id}: applied; document, revision and preview unchanged"),
            applied.is_some()
                && after.revision == before.revision
                && after.view_hash == before.view_hash
                && after.textarea_hash == before.textarea_hash
                && after.dirty == before.dirty
                && after.preview_loads == before.preview_loads
                && cov > 0.002,
            format!("covered {:.2}%", cov * 100.0),
        );
        checkpoint(c, &format!("theme-{id}")).await;
    }

    // --- New World on unsaved changes: Discard. ---
    create_one(c, p::Primitive::Box, 2, 5, r).await?;
    click("btn-new")?;
    let fresh = wait_ms(5000, || {
        u.doc.get_untracked().filter(|d| {
            d.untitled && d.view == "#VRML V2.0 utf8\n" && all("li.tree-item").is_empty()
        })
    })
    .await;
    r.step(
        "New World on unsaved changes: Discard opens a new world (file untouched)",
        fresh.is_some() && text() == "#VRML V2.0 utf8\n" && all("li.tree-item").is_empty(),
        u.message.get_untracked(),
    );
    Some(())
}
