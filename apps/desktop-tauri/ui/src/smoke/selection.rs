// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-EDITOR-2 smoke steps: the Scene Tree, Diagnostics and Inspector never
//! select or show positions of an older revision than the document's.
//! Every step uses the real widget and real DOM events; the text is
//! returned to its starting state at the end.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent, KeyboardEventInit};
use wrlforge_desktop_protocol as p;

use super::syntax::{history_key, type_at, wait_exact};
use super::{doc_el, settle, snapshot, R};
use crate::editor::{textarea, CORE};
use crate::ipc::{self, call};
use crate::ui::{self, ui};

fn tree_attr(name: &str) -> Option<String> {
    doc_el()?
        .query_selector("ul.tree")
        .ok()??
        .get_attribute(name)
}

/// The rendered `<li>` of `id` (whatever revision the DOM shows).
fn tree_li(id: &str) -> Option<HtmlElement> {
    doc_el()?
        .query_selector(&format!(".tree-item[data-id=\"{id}\"]"))
        .ok()??
        .dyn_into()
        .ok()
}

fn sel_range() -> (u32, u32) {
    textarea()
        .map(|t| {
            (
                t.selection_start().ok().flatten().unwrap_or(u32::MAX),
                t.selection_end().ok().flatten().unwrap_or(u32::MAX),
            )
        })
        .unwrap_or((u32::MAX, u32::MAX))
}

fn enter_on(el: &HtmlElement) {
    let init = KeyboardEventInit::new();
    init.set_key("Enter");
    init.set_bubbles(true);
    init.set_cancelable(true);
    if let Ok(ev) = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init) {
        let _ = el.dispatch_event(&ev);
    }
}

/// Wait until the analysis AND its rendered tree are of the document's
/// current revision, with no edit in flight.
async fn tree_current() -> bool {
    for _ in 0..300 {
        let (rev, busy) = CORE.with_borrow(|c| (c.revision, c.busy));
        let a = ui()
            .analysis
            .with_untracked(|a| a.as_ref().map(|a| a.revision));
        if !busy && a == Some(rev) && tree_attr("data-revision") == Some(rev.to_string()) {
            return true;
        }
        ipc::sleep(20).await;
    }
    false
}

fn view_slice(view: &str, from: u64, to: u64) -> String {
    let u: Vec<u16> = view.encode_utf16().collect();
    String::from_utf16_lossy(&u[from as usize..to as usize])
}

/// A node item of the current analysis that starts after `after` (so an
/// insert at `after` shifts it) -- its id, span and source text.
fn node_after(after: u64) -> Option<p::SceneItem> {
    ui().analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.kind == "Node" && i.view_from > after)
            .cloned()
    })
}

pub async fn selection_steps(r: &mut R) -> Option<()> {
    let ta = textarea()?;
    if !tree_current().await {
        r.step(
            "selection: Scene Tree reached the current revision",
            false,
            "",
        );
        return None;
    }
    let base = ta.value();
    let base_snap = snapshot().await?;
    // Insert point: end of line 1, so every later item shifts.
    let at = base.encode_utf16().position(|u| u == b'\n' as u16)? as u32;
    let item = node_after(at as u64)?;
    let item_text = view_slice(&base, item.view_from, item.view_to);
    ui().selected.set(None);
    ui().inspection.set(None);
    let _ = ta.set_selection_range(0, 0);

    // ---- 1. click on the tree DURING the analysis delay -----------------
    let li_old = tree_li(&item.id)?;
    type_at(at, "\n# ed2", false)?;
    let typed_caret = sel_range();
    li_old.click();
    let refused_now = ui().selected.get_untracked().is_none() && sel_range() == typed_caret;
    let msg = ui().message.get_untracked();
    settle().await;
    let snap = snapshot().await?;
    let only_typing = snap.view == ta.value()
        && snap.revision == base_snap.revision + 1
        && ta.value().len() == base.len() + "\n# ed2".len();
    r.step(
        "selection: a Scene Tree click during the analysis delay is refused (nothing selected, nothing changed)",
        refused_now && only_typing && msg.contains("updating"),
        format!("caret {typed_caret:?} -> {:?}; message {msg:?}; rev {} -> {}", sel_range(), base_snap.revision, snap.revision),
    );

    // ---- 2. the tree says it is updating while it is behind ------------
    type_at(at, "x", false)?;
    let mut saw_stale = false;
    for _ in 0..20 {
        ipc::sleep(10).await;
        let updating = doc_el()
            .and_then(|d| d.get_element_by_id("tree-updating"))
            .and_then(|e| e.text_content())
            .unwrap_or_default();
        if tree_attr("data-stale").as_deref() == Some("true")
            && tree_attr("aria-busy").as_deref() == Some("true")
            && updating.contains("updating")
        {
            saw_stale = true;
            break;
        }
    }
    let caught_up = tree_current().await;
    r.step(
        "selection: the Scene Tree shows an updating state while behind, then catches up",
        saw_stale && caught_up && tree_attr("data-stale").as_deref() == Some("false"),
        format!("stale seen {saw_stale}, caught up {caught_up}"),
    );

    // ---- 3. once current, the same item selects its CURRENT span --------
    let shift = "\n# ed2x".encode_utf16().count() as u64;
    let now_item = ui().analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.view_from == item.view_from + shift && i.label == item.label)
            .cloned()
    })?;
    tree_li(&now_item.id)?.click();
    let sel = sel_range();
    let selected_text = view_slice(&ta.value(), sel.0 as u64, sel.1 as u64);
    r.step(
        "selection: on a current tree the click selects the item's span in the new revision",
        sel == (now_item.view_from as u32, now_item.view_to as u32)
            && selected_text == item_text
            && ui().selected.get_untracked().is_some_and(|s| {
                s.id == now_item.id && s.revision == CORE.with_borrow(|c| c.revision)
            }),
        format!(
            "{sel:?} == {}..{} ({})",
            now_item.view_from, now_item.view_to, item.label
        ),
    );
    // Wait for the Inspector of that selection.
    for _ in 0..150 {
        if ui().inspection.get_untracked().is_some() {
            break;
        }
        ipc::sleep(20).await;
    }

    // ---- 4. keyboard: Enter on a stale item is refused too --------------
    let _ = ta.set_selection_range(0, 0);
    let li_now = tree_li(&now_item.id)?;
    let sel_before = ui().selected.get_untracked();
    type_at(at, "k", false)?;
    let caret = sel_range();
    let _ = li_now.focus();
    enter_on(&li_now);
    let refused = sel_range() == caret;
    // The selection itself was carried by Rust through the edit (inside or
    // after it), never re-pointed by the stale Enter.
    settle().await;
    let carried = ui().selected.get_untracked();
    let ok_carry = match (&sel_before, &carried) {
        (Some(b), Some(c)) => c.revision == b.revision + 1,
        (_, None) => true,
        _ => false,
    };
    tree_current().await;
    let k_item = ui().analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.label == item.label && i.view_from == now_item.view_from + 1)
            .cloned()
    })?;
    let li_k = tree_li(&k_item.id)?;
    let _ = li_k.focus();
    enter_on(&li_k);
    let sel = sel_range();
    r.step(
        "selection: keyboard Enter on a stale item is refused; on a current item it selects the current span",
        refused
            && ok_carry
            && sel == (k_item.view_from as u32, k_item.view_to as u32)
            && view_slice(&ta.value(), sel.0 as u64, sel.1 as u64) == item_text,
        format!("stale caret {caret:?} kept: {refused}; carried {:?}; current {sel:?}", carried.map(|c| (c.revision, c.id))),
    );

    // ---- 5. rapid edits, then an immediate click ------------------------
    tree_current().await;
    let li = tree_li(&k_item.id)?;
    let _ = ta.set_selection_range(0, 0);
    ui().selected.set(None);
    let before_rev = CORE.with_borrow(|c| c.revision);
    for i in 0..10u32 {
        type_at(at + i, "r", false)?;
    }
    let caret = sel_range();
    li.click();
    let refused = sel_range() == caret && ui().selected.get_untracked().is_none();
    settle().await;
    let snap = snapshot().await?;
    let typed_all = snap.view == ta.value() && snap.revision > before_rev;
    r.step(
        "selection: rapid edits followed by an immediate click: refused, every keystroke applied",
        refused && typed_all,
        format!("rev {before_rev} -> {}", snap.revision),
    );

    // ---- 6. a document change while an Inspector request is active -----
    tree_current().await;
    // Inserted before the item so far: "\n# ed2" + "x" + "k" + 10 x "r".
    let cur_from = item.view_from + 6 + 1 + 1 + 10;
    let cur = ui().analysis.with_untracked(|a| {
        a.as_ref()?
            .items
            .iter()
            .find(|i| i.label == item.label && i.view_from == cur_from)
            .cloned()
    })?;
    tree_li(&cur.id)?.click(); // starts doc_inspect
    type_at(at, "q", false)?; // and the document moves at once
    settle().await;
    tree_current().await;
    for _ in 0..100 {
        let ok = ui()
            .inspection
            .with_untracked(|i| i.as_ref().map(|i| i.revision))
            == Some(CORE.with_borrow(|c| c.revision));
        if ok {
            break;
        }
        ipc::sleep(20).await;
    }
    let s = ui().selected.get_untracked();
    let insp = ui().inspection.get_untracked();
    let rev = CORE.with_borrow(|c| c.revision);
    let consistent = match (&s, &insp) {
        (Some(s), Some(i)) => {
            let span = ui().analysis.with_untracked(|a| {
                a.as_ref()?
                    .items
                    .iter()
                    .find(|x| x.id == s.id)
                    .map(|x| (x.view_from, x.view_to))
            });
            s.revision == rev
                && i.revision == rev
                && i.id == s.id
                && span.is_some_and(|(f, t)| view_slice(&ta.value(), f, t) == item_text)
        }
        (None, None) => true,
        _ => false,
    };
    r.step(
        "selection: an edit during an Inspector request ends with selection, Inspector and document at one revision",
        consistent,
        format!("rev {rev}, selected {:?}, inspection rev {:?}", s.map(|s| (s.revision, s.id)), insp.as_ref().map(|i| i.revision)),
    );

    // ---- 7. late / stale Inspector replies are discarded ---------------
    let (Some(s), Some(i)) = (
        ui().selected.get_untracked(),
        ui().inspection.get_untracked(),
    ) else {
        r.step(
            "selection: a current selection exists for reply tests",
            false,
            "",
        );
        return None;
    };
    let seq = CORE.with_borrow(|c| c.inspect_seq);
    let fake = |rev: u64| p::InspectOutcome::Found {
        inspection: p::Inspection {
            revision: rev,
            title: "STALE".into(),
            ..i.clone()
        },
    };
    let before = CORE.with_borrow(|c| c.stale_inspections);
    let old_rev = !ui::apply_inspection(seq, &s, fake(s.revision.saturating_sub(1)));
    let old_seq = !ui::apply_inspection(seq.saturating_sub(1), &s, fake(s.revision));
    let other = ui::Selected {
        id: "node-0-1".into(),
        ..s.clone()
    };
    let moved = !ui::apply_inspection(seq, &other, fake(s.revision));
    let stale_kind = !ui::apply_inspection(
        seq,
        &s,
        p::InspectOutcome::Stale {
            current: s.revision + 1,
        },
    );
    #[derive(serde::Serialize)]
    struct A {
        session: u64,
        item: String,
        revision: u64,
    }
    let backend = call::<p::InspectOutcome>(
        "doc_inspect",
        A {
            session: s.session,
            item: s.id.clone(),
            revision: s.revision.saturating_sub(1),
        },
    )
    .await
    .ok();
    let unchanged = ui().inspection.get_untracked().as_ref() == Some(&i)
        && ui().selected.get_untracked().as_ref() == Some(&s);
    r.step(
        "selection: a late Inspector reply (older revision, older request, other item, stale) never replaces the Inspector",
        old_rev && old_seq && moved && stale_kind && unchanged
            && CORE.with_borrow(|c| c.stale_inspections) == before + 4
            && backend == Some(p::InspectOutcome::Stale { current: s.revision }),
        format!("backend for rev {}: {backend:?}", s.revision.saturating_sub(1)),
    );

    // ---- 8. an old analysis reply never replaces newer results ----------
    let cur_a = ui().analysis.get_untracked()?;
    let before = CORE.with_borrow(|c| c.stale_analyses);
    let mut old = cur_a.clone();
    old.revision = cur_a.revision.saturating_sub(1);
    old.items.clear();
    old.diagnostics.clear();
    let mut foreign = cur_a.clone();
    foreign.session = cur_a.session + 1000;
    foreign.items.clear();
    let r1 = ui::apply_analysis(old).is_none();
    let r2 = ui::apply_analysis(foreign).is_none();
    let diag_rev = doc_el()
        .and_then(|d| d.get_element_by_id("diag-head"))
        .and_then(|e| e.get_attribute("data-revision"));
    r.step(
        "selection: an older or foreign analysis reply never replaces the Scene Tree or Diagnostics",
        r1 && r2
            && ui().analysis.get_untracked().as_ref() == Some(&cur_a)
            && CORE.with_borrow(|c| c.stale_analyses) == before + 2
            && diag_rev == Some(cur_a.revision.to_string())
            && ui().selected.get_untracked().as_ref() == Some(&s),
        format!("tree rev {:?}, diagnostics rev {diag_rev:?}", tree_attr("data-revision")),
    );

    // ---- 9. an edit while an analysis is in flight ---------------------
    // Sample the invariant: whenever the tree says it is current, it IS
    // the document's revision.
    leptos::task::spawn_local(ui::analyze());
    type_at(at, "z", false)?;
    let mut lies = 0;
    for _ in 0..60 {
        ipc::sleep(10).await;
        let (rev, busy) = CORE.with_borrow(|c| (c.revision, c.busy));
        if !busy
            && tree_attr("data-stale").as_deref() == Some("false")
            && tree_attr("data-revision") != Some(rev.to_string())
        {
            lies += 1;
        }
    }
    r.step(
        "selection: during an edit with an analysis in flight, the tree never claims to be current when it is not",
        lies == 0 && tree_current().await,
        format!("{lies} inconsistent samples of 60"),
    );

    // ---- 10. Inspector Apply on a stale Inspector is refused -----------
    type_at(at, "w", false)?;
    let rev_before = CORE.with_borrow(|c| c.revision);
    // Rust has acknowledged; the Inspector is still of the older revision.
    for _ in 0..50 {
        if !CORE.with_borrow(|c| c.busy) {
            break;
        }
        ipc::sleep(5).await;
    }
    let stale_insp = ui()
        .inspection
        .with_untracked(|i| i.as_ref().map(|i| i.revision))
        != Some(CORE.with_borrow(|c| c.revision));
    let field = ui().inspection.with_untracked(|i| {
        i.as_ref()?
            .node
            .as_ref()?
            .fields
            .iter()
            .find(|f| f.editable)
            .map(|f| (f.index, f.name.clone(), f.components.len()))
    });
    let mut refused_apply = true;
    let mut err = String::new();
    if let (true, Some((idx, name, n))) = (stale_insp, field) {
        let rev0 = CORE.with_borrow(|c| c.revision);
        ui::edit_field(
            idx,
            name,
            vec![p::FieldInput::Text { value: "0".into() }; n],
        )
        .await;
        err = ui().field_error.get_untracked().unwrap_or_default();
        refused_apply = err.contains("updating") && CORE.with_borrow(|c| c.revision) == rev0;
    }
    r.step(
        "selection: Apply on an Inspector of an older revision is refused before it reaches Rust",
        refused_apply,
        format!("inspector stale: {stale_insp}; rev {rev_before}; {err}"),
    );
    ui().field_error.set(None);
    settle().await;

    // ---- restore the starting text ------------------------------------
    for _ in 0..60 {
        if ta.value() == base || !ui().can_undo.get_untracked() {
            break;
        }
        history_key(false).await;
    }
    let back = wait_exact().await.is_some() && ta.value() == base;
    let snap = snapshot().await?;
    r.step(
        "selection: undo returns the document to its text before these steps",
        back && snap.view == base,
        format!("rev {}", snap.revision),
    );
    tree_current().await;
    ui().selected.set(None);
    ui().inspection.set(None);
    Some(())
}
