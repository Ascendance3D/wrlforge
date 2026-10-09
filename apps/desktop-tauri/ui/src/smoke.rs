// SPDX-License-Identifier: GPL-3.0-or-later
//! In-window end-to-end smoke test (`--smoke`). Drives the real widget with
//! real DOM events, so the same handlers a user triggers are exercised.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent, KeyboardEventInit};
use wrlforge_desktop_protocol as p;

use crate::editor::{self, textarea, CORE};
use crate::ipc::{self, call, Session};
use crate::ui::{self, ui};

struct R(Vec<p::SmokeStep>);
impl R {
    fn step(&mut self, name: &str, ok: bool, detail: impl Into<String>) -> bool {
        self.0.push(p::SmokeStep {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
        ok
    }
}

async fn snapshot() -> Option<p::DocumentInfo> {
    let session = CORE.with_borrow(|c| c.session)?;
    call::<p::DocumentInfo>("doc_snapshot", Session { session })
        .await
        .ok()
}

fn key(k: &str, shift: bool) {
    let init = KeyboardEventInit::new();
    init.set_key(k);
    init.set_ctrl_key(true);
    init.set_shift_key(shift);
    init.set_bubbles(true);
    init.set_cancelable(true);
    if let (Some(ta), Ok(ev)) = (
        textarea(),
        KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init),
    ) {
        let _ = ta.dispatch_event(&ev);
    }
}

async fn settle() {
    ipc::sleep(50).await;
    editor::idle().await;
    ipc::sleep(50).await;
}

pub async fn run(plan: p::SmokePlan) {
    let mut r = R(vec![]);
    if run_steps(&plan, &mut r).await.is_none() {
        r.step(
            "smoke script ran to completion",
            false,
            "a required DOM element or reply was missing",
        );
    }
    let _ = call::<()>(
        "smoke_finish",
        serde_json::json!({ "report": p::SmokeReport { steps: r.0 } }),
    )
    .await;
}

async fn run_steps(plan: &p::SmokePlan, r: &mut R) -> Option<()> {
    let ta = textarea()?;
    // Wait for the startup document.
    for _ in 0..200 {
        if CORE.with_borrow(|c| c.session.is_some()) {
            break;
        }
        ipc::sleep(20).await;
    }
    let doc = snapshot().await;
    if !r.step("startup document opened through Rust", doc.is_some(), "") {
        return None;
    }
    let doc = doc?;
    let original = ta.value();
    r.step(
        "editor shows exactly the Rust view projection",
        original == doc.view,
        format!(
            "{} UTF-16 units, format {}, eol {}, bom {}",
            editor::utf16_len(&original),
            doc.format,
            doc.eol,
            doc.bom
        ),
    );

    // Type a line through a genuine input event at the end of line 1.
    let at = original
        .encode_utf16()
        .position(|u| u == b'\n' as u16)
        .unwrap_or(original.encode_utf16().count()) as u32;
    let _ = ta.focus();
    let _ = ta.set_selection_range(at, at);
    let insert = format!("\n{}", plan.insert_text);
    ta.set_range_text_with_start_and_end(&insert, at, at).ok()?;
    let ev = web_sys::InputEvent::new("input").ok()?;
    ta.dispatch_event(&ev).ok()?;
    settle().await;
    let edited = ta.value();
    let snap = snapshot().await?;
    r.step(
        "typed edit reached the canonical Rust document",
        snap.view == edited && snap.dirty && snap.revision > doc.revision,
        format!(
            "rev {} -> {}, dirty {}",
            doc.revision, snap.revision, snap.dirty
        ),
    );
    r.step("status bar shows Modified", ui().dirty.get_untracked(), "");

    // Undo / redo through the keyboard handler.
    key("z", false);
    settle().await;
    let snap = snapshot().await?;
    r.step(
        "Ctrl+Z restores the exact original",
        ta.value() == original && snap.view == original && !snap.dirty,
        format!("dirty {}", snap.dirty),
    );
    key("z", true);
    settle().await;
    let snap = snapshot().await?;
    r.step(
        "Ctrl+Shift+Z redoes the edit",
        ta.value() == edited && snap.view == edited && snap.dirty,
        "",
    );

    // Save through the toolbar button's handler.
    let btn: HtmlElement = crate::element_by_id("btn-save")?;
    btn.click();
    for _ in 0..200 {
        ipc::sleep(20).await;
        if !ui().dirty.get_untracked() {
            break;
        }
    }
    let snap = snapshot().await?;
    r.step(
        "Save button wrote through Rust and cleared dirty",
        !snap.dirty,
        ui().last_save.get_untracked().unwrap_or_default(),
    );

    // Scene Tree -> selection -> Inspector.
    ui::analyze().await;
    let count = ui()
        .analysis
        .get_untracked()
        .map(|a| a.items.len())
        .unwrap_or(0);
    r.step(
        "Scene Tree lists items from the native parser",
        count > 0,
        format!("{count} items"),
    );
    let diag = ui()
        .analysis
        .get_untracked()
        .map(|a| a.diagnostics.len())
        .unwrap_or(0);
    r.step("diagnostics computed", true, format!("{diag} diagnostics"));
    let mut first = None;
    for _ in 0..100 {
        first = web_sys::window()?
            .document()?
            .query_selector(".tree-item")
            .ok()
            .flatten();
        if first.is_some() {
            break;
        }
        ipc::sleep(20).await;
    }
    if !r.step("Scene Tree items rendered in the DOM", first.is_some(), "") {
        return None;
    }
    let first: HtmlElement = first?.dyn_into().ok()?;
    let item = ui().analysis.get_untracked()?.items.first()?.clone();
    first.click();
    for _ in 0..100 {
        ipc::sleep(20).await;
        if ui().inspection.get_untracked().is_some() {
            break;
        }
    }
    let insp = ui().inspection.get_untracked();
    let sel = (
        ta.selection_start().ok().flatten().unwrap_or(u32::MAX),
        ta.selection_end().ok().flatten().unwrap_or(u32::MAX),
    );
    r.step(
        "clicking a Scene Tree item selects its exact source span",
        sel == (item.view_from as u32, item.view_to as u32),
        format!("{:?} for {}", sel, item.label),
    );
    r.step(
        "Inspector shows the item's fields",
        insp.as_ref().is_some_and(|i| !i.title.is_empty()),
        insp.map(|i| format!("{} ({} rows)", i.title, i.rows.len()))
            .unwrap_or_default(),
    );

    // Preview.
    let probe = ipc::preview_probe();
    r.step("WebView WebGL probe", true, probe.clone());
    if plan.expect_preview {
        ui::preview(true).await;
        let status = ui().preview_status.get_untracked();
        r.step(
            "X_ITE preview loaded the unsaved buffer",
            status.contains("loaded:"),
            status,
        );
    }
    Some(())
}
