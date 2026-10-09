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

mod create;
mod pick;
mod selection;
mod syntax;

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

/// Select + inspect an item of the CURRENT analysis directly (probing many
/// items quickly); a click goes through `ui::tree_select` instead.
async fn inspect_item(id: &str) {
    let (Some(session), revision) = CORE.with_borrow(|c| (c.session, c.revision)) else {
        return;
    };
    let sel = ui::Selected {
        session,
        revision,
        id: id.to_string(),
    };
    ui().selected.set(Some(sel.clone()));
    ui::inspect(sel).await;
}

async fn settle() {
    ipc::sleep(50).await;
    editor::idle().await;
    ipc::sleep(50).await;
}

pub async fn run(plan: p::SmokePlan) {
    let mut r = R(vec![]);
    let done = match (&plan.create, &plan.pick) {
        (Some(c), _) => create::run(c, &mut r).await,
        (None, Some(pk)) => pick::run(pk, &mut r).await,
        (None, None) => run_steps(&plan, &mut r).await,
    };
    if done.is_none() {
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

    if let Some(t) = &plan.theme {
        theme_steps(t, plan.expect_preview, r).await?;
    }

    if let Some(value) = &plan.inspector_value {
        inspector_steps(value, plan.expect_preview, r).await?;
    }

    // UI-SYNTAX-1: runs on the edited (and, with --inspector, field-edited)
    // document and returns it to exactly that text before the Save below.
    syntax::syntax_steps(plan, r).await?;

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
    // UI-SYNTAX-1 regression for the UI-THEME-1 cyclone failure: the
    // explicit analysis sets the signal, but Leptos re-renders the tree a
    // tick later. Clicking at once hit items (and offsets) of an OLDER
    // analysis. Wait until the tree DOM carries the current revision.
    let want_rev = ui()
        .analysis
        .get_untracked()
        .map(|a| a.revision.to_string());
    let tree_rev = || {
        web_sys::window()?
            .document()?
            .query_selector("ul.tree")
            .ok()??
            .get_attribute("data-revision")
    };
    let stale_at_once = tree_rev() != want_rev;
    let mut first = None;
    for _ in 0..100 {
        if tree_rev() == want_rev {
            first = web_sys::window()?
                .document()?
                .query_selector(".tree-item")
                .ok()
                .flatten();
        }
        if first.is_some() {
            break;
        }
        ipc::sleep(20).await;
    }
    r.step(
        "Scene Tree DOM shows the current analysis revision before it is clicked",
        tree_rev() == want_rev,
        format!(
            "analysis rev {want_rev:?}, tree DOM rev {:?}; stale immediately after analyze: {stale_at_once}",
            tree_rev()
        ),
    );
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

    // UI-EDITOR-2: no selection from a projection of an older revision.
    selection::selection_steps(r).await?;

    // Preview.
    let probe = ipc::preview_probe();
    r.step("WebView WebGL probe", true, probe.clone());
    if plan.expect_preview {
        ui::preview(true).await;
        // A newer (debounced) load may still be running; wait for it.
        wait_for(|| (!ui().preview_status.get_untracked().starts_with("updating")).then_some(()))
            .await;
        let status = ui().preview_status.get_untracked();
        r.step(
            "X_ITE preview loaded the unsaved buffer",
            status.contains("loaded:"),
            status,
        );
    }
    syntax::reload_steps(r).await?;
    Some(())
}

fn doc_el() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

async fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..150 {
        if let Some(v) = f() {
            return Some(v);
        }
        ipc::sleep(20).await;
    }
    None
}

fn set_input(id: &str, v: &str) -> Option<()> {
    let inp: web_sys::HtmlInputElement = crate::element_by_id(id)?;
    inp.set_value(v);
    Some(())
}

fn click(id: &str) -> Option<()> {
    let b: HtmlElement = crate::element_by_id(id)?;
    b.click();
    Some(())
}

/// The single change between two views, widened to whole tokens:
/// (from, old token, new token) in UTF-16.
fn change(a: &str, b: &str) -> (u64, String, String) {
    let delim =
        |u: u16| char::from_u32(u as u32).is_some_and(|c| c.is_whitespace() || ",[]{}".contains(c));
    let (from, to, ins) = editor::diff(a, b);
    let ua: Vec<u16> = a.encode_utf16().collect();
    let ub: Vec<u16> = b.encode_utf16().collect();
    let (mut f, mut ta) = (from as usize, to as usize);
    let mut tb = f + ins.encode_utf16().count();
    while f > 0 && !delim(ua[f - 1]) {
        f -= 1;
    }
    while ta < ua.len() && tb < ub.len() && !delim(ua[ta]) && ua[ta] == ub[tb] {
        ta += 1;
        tb += 1;
    }
    (
        f as u64,
        String::from_utf16_lossy(&ua[f..ta]),
        String::from_utf16_lossy(&ub[f..tb]),
    )
}

/// Drive the Inspector exactly as a user does: click the Scene Tree item,
/// type into the field's component box, press Apply. Then refusals, the
/// preview, undo and redo.
async fn inspector_steps(value: &str, expect_preview: bool, r: &mut R) -> Option<()> {
    let ta = textarea()?;
    ui::analyze().await;
    let items = ui().analysis.get_untracked()?.items;
    // Find a node whose Inspector offers an editable diffuseColor, else translation.
    let mut target = None;
    'pick: for want in ["diffuseColor", "translation"] {
        for it in items.iter().filter(|i| i.kind == "Node") {
            inspect_item(&it.id).await;
            let Some(n) = ui().inspection.get_untracked().and_then(|i| i.node) else {
                continue;
            };
            if let Some(f) = n
                .fields
                .iter()
                .find(|f| f.name == want && f.editable && f.components.len() >= 3)
            {
                target = Some((it.clone(), f.clone()));
                break 'pick;
            }
        }
    }
    if !r.step(
        "inspector: found an editable SF field on a real node",
        target.is_some(),
        target
            .as_ref()
            .map(|(it, f)| {
                format!(
                    "{} . {} ({})",
                    it.label,
                    f.name,
                    f.field_type.clone().unwrap_or_default()
                )
            })
            .unwrap_or_default(),
    ) {
        return None;
    }
    let (item, field) = target?;
    // Select it through the DOM, like a user.
    ui().inspection.set(None);
    let li: HtmlElement = wait_for(|| {
        doc_el()?
            .query_selector(&format!("li[data-id=\"{}\"]", item.id))
            .ok()
            .flatten()?
            .dyn_into()
            .ok()
    })
    .await?;
    li.click();
    let comp_id = format!("fe-{}-1", field.index);
    let ok = wait_for(|| crate::element_by_id::<web_sys::HtmlInputElement>(&comp_id)).await;
    r.step(
        "inspector: clicking the tree item shows typed, editable controls",
        ok.is_some(),
        format!(
            "{} = {}",
            field.name,
            field
                .components
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );
    let before = snapshot().await?;
    let apply = format!("fe-apply-{}", field.index);

    // Unsupported edits fail visibly and change nothing.
    let mut refusals = Vec::new();
    let mut bad = vec![("abc", "input-not-a-number")];
    if field.bounds.is_some() {
        bad.push(("2", "input-out-of-range"));
    }
    for (v, reason) in bad {
        ui().field_error.set(None);
        wait_for(|| {
            doc_el()?
                .get_element_by_id("inspector-error")
                .is_none()
                .then_some(())
        })
        .await?;
        set_input(&comp_id, v)?;
        click(&apply)?;
        let tag = format!("[{reason}]");
        let shown = wait_for(|| {
            let el = doc_el()?.get_element_by_id("inspector-error")?;
            el.text_content().filter(|t| t.contains(&tag))
        })
        .await;
        let snap = snapshot().await?;
        let same = snap.revision == before.revision
            && snap.view == before.view
            && ta.value() == before.view;
        refusals.push((v, shown.is_some() && same, shown.unwrap_or_default()));
    }
    r.step(
        "inspector: invalid values are refused by Rust, visibly, with no change",
        refusals.iter().all(|x| x.1),
        refusals
            .iter()
            .map(|x| format!("{:?} -> {}", x.0, x.2))
            .collect::<Vec<_>>()
            .join(" | "),
    );

    // The real edit.
    set_input(&comp_id, value)?;
    click(&apply)?;
    let after =
        wait_for(|| (CORE.with_borrow(|c| c.revision) > before.revision).then_some(())).await;
    settle().await;
    let snap = snapshot().await?;
    let (at, old, new) = change(&before.view, &snap.view);
    let old_text = field.components[1].text.clone();
    let in_span = at >= field.view_from && at < field.view_to;
    r.step(
        "inspector: Apply changed exactly the intended source token in Rust",
        after.is_some() && old == old_text && new == value && in_span && snap.dirty,
        format!(
            "rev {} -> {}, view @{at}: {old:?} -> {new:?}",
            before.revision, snap.revision
        ),
    );
    r.step(
        "inspector: source editor shows the Rust result",
        ta.value() == snap.view,
        "",
    );
    let shown = wait_for(|| {
        crate::element_by_id::<web_sys::HtmlInputElement>(&comp_id)
            .map(|i| i.value())
            .filter(|v| v == value)
    })
    .await;
    r.step(
        "inspector: re-inspected node shows the new value",
        shown.is_some() && ui().field_error.get_untracked().is_none(),
        ui().selected
            .get_untracked()
            .map(|s| s.id)
            .unwrap_or_default(),
    );
    let tree_ok = wait_for(|| {
        ui().analysis
            .get_untracked()
            .filter(|a| a.revision == snap.revision)
            .map(|a| {
                a.items
                    .iter()
                    .any(|i| Some(&i.id) == ui().selected.get_untracked().map(|s| s.id).as_ref())
            })
            .filter(|ok| *ok)
    })
    .await;
    r.step(
        "inspector: Scene Tree re-analyzed at the new revision and keeps the node",
        tree_ok.is_some(),
        "",
    );
    if expect_preview && ui().preview_enabled.get_untracked() {
        let want = format!("rev {} ·", snap.revision);
        let mut status = String::new();
        for _ in 0..150 {
            status = ui().preview_status.get_untracked();
            if status.starts_with(&want) {
                break;
            }
            ipc::sleep(20).await;
        }
        r.step(
            "inspector: live preview reloaded the edited revision",
            status.starts_with(&want) && status.contains("loaded:"),
            status,
        );
    }
    let edited = snap.view.clone();
    key("z", false);
    settle().await;
    let s1 = snapshot().await?;
    let shows = |want: String| {
        let id = comp_id.clone();
        async move {
            wait_for(|| {
                crate::element_by_id::<web_sys::HtmlInputElement>(&id)
                    .map(|i| i.value())
                    .filter(|v| *v == want)
            })
            .await
            .is_some()
        }
    };
    let kept = shows(old_text.clone()).await;
    r.step(
        "inspector: Ctrl+Z restores the previous value in one step; Inspector keeps the node",
        s1.view == before.view && ta.value() == before.view && kept,
        format!(
            "rev {}, selected {:?}",
            s1.revision,
            ui().selected.get_untracked()
        ),
    );
    key("z", true);
    settle().await;
    let s2 = snapshot().await?;
    let kept = shows(value.to_string()).await;
    r.step(
        "inspector: Ctrl+Shift+Z reapplies the field edit; Inspector keeps the node",
        s2.view == edited && ta.value() == edited && kept,
        format!(
            "rev {}, selected {:?}",
            s2.revision,
            ui().selected.get_untracked()
        ),
    );
    Some(())
}

// ---- UI-THEME-1 ----------------------------------------------------------

fn computed(el: &web_sys::Element, prop: &str) -> String {
    web_sys::window()
        .and_then(|w| w.get_computed_style(el).ok().flatten())
        .and_then(|cs| cs.get_property_value(prop).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn token(name: &str) -> String {
    doc_el()
        .and_then(|d| d.document_element())
        .map(|root| computed(&root, name))
        .unwrap_or_default()
}

/// `#rrggbb` -> the `rgb(r, g, b)` form getComputedStyle reports.
fn hex_rgb(hex: &str) -> String {
    let h = hex.trim().trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("zz"), 16).ok();
    match (c(0), c(2), c(4)) {
        (Some(r), Some(g), Some(b)) if h.len() == 6 => format!("rgb({r}, {g}, {b})"),
        _ => format!("<not #rrggbb: {hex}>"),
    }
}

/// Every application-owned surface this run checks, with the token that must
/// paint it: (CSS selector, CSS property, token).
const SURFACES: &[(&str, &str, &str)] = &[
    ("body", "background-color", "--wf-bg-app"),
    (".toolbar", "background-color", "--wf-surface-chrome"),
    (".brand", "color", "--wf-text-brand"),
    ("#btn-open", "background-color", "--wf-btn-primary-bg"),
    ("#btn-open", "color", "--wf-btn-primary-fg"),
    ("#btn-undo", "background-color", "--wf-btn-secondary-bg"),
    ("#theme-select", "background-color", "--wf-surface-input"),
    ("#theme-select", "color", "--wf-text-editor"),
    (".tree-col", "background-color", "--wf-surface-panel"),
    (
        ".tree-col .pane-title",
        "background-color",
        "--wf-surface-header",
    ),
    (".tree-item:not(.selected)", "color", "--wf-text-primary"),
    (
        ".tree-item.selected",
        "background-color",
        "--wf-tree-selected-bg",
    ),
    (".source-wrap", "background-color", "--wf-surface-editor"),
    ("#source-hl", "color", "--wf-text-editor"),
    ("#source", "caret-color", "--wf-editor-caret"),
    (".diagnostics", "background-color", "--wf-surface-panel"),
    (".inspector", "background-color", "--wf-surface-panel"),
    ("#inspector-title", "color", "--wf-text-heading"),
    (".viewport", "background-color", "--wf-viewport-bg"),
    (
        ".viewport .pane-title",
        "background-color",
        "--wf-viewport-header-bg",
    ),
    (".viewport .badge", "background-color", "--wf-badge-bg"),
    ("#preview-status", "color", "--wf-viewport-status-fg"),
    (".statusbar", "background-color", "--wf-surface-chrome"),
    (".statusbar", "color", "--wf-text-secondary"),
];

fn surfaces_match() -> (bool, String) {
    let Some(d) = doc_el() else {
        return (false, "no document".into());
    };
    let mut bad = vec![];
    let mut n = 0;
    for (sel, prop, tok) in SURFACES {
        let Some(el) = d.query_selector(sel).ok().flatten() else {
            bad.push(format!("{sel}: missing"));
            continue;
        };
        n += 1;
        let (got, want) = (computed(&el, prop), hex_rgb(&token(tok)));
        if got != want {
            bad.push(format!("{sel} {prop} {got} != {tok} {want}"));
        }
    }
    (
        bad.is_empty(),
        if bad.is_empty() {
            format!("{n} surfaces")
        } else {
            bad.join("; ")
        },
    )
}

#[derive(PartialEq, Debug)]
struct DocState {
    revision: u64,
    dirty: bool,
    view_hash: u64,
    can_undo: bool,
    can_redo: bool,
    textarea_hash: u64,
    selection: (u32, u32),
    selected: Option<String>,
    inspection: Option<p::Inspection>,
    preview_loads: u64,
}

async fn doc_state() -> Option<DocState> {
    let s = snapshot().await?;
    let ta = textarea()?;
    Some(DocState {
        revision: s.revision,
        dirty: s.dirty,
        view_hash: p::view_hash(&s.view),
        can_undo: s.can_undo,
        can_redo: s.can_redo,
        textarea_hash: p::view_hash(&ta.value()),
        selection: (
            ta.selection_start().ok().flatten().unwrap_or(u32::MAX),
            ta.selection_end().ok().flatten().unwrap_or(u32::MAX),
        ),
        selected: ui().selected.get_untracked().map(|s| s.id),
        inspection: ui().inspection.get_untracked(),
        preview_loads: ui().preview_loads.get_untracked(),
    })
}

fn choose(sel: &web_sys::HtmlSelectElement, id: &str) -> Option<()> {
    sel.set_value(id);
    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    let ev = web_sys::Event::new_with_event_init_dict("change", &init).ok()?;
    sel.dispatch_event(&ev).ok()?;
    Some(())
}

/// Theme selector through the real toolbar control, on a DIRTY document with
/// an undo history, a Scene Tree selection and a live Inspector.
async fn theme_steps(t: &p::ThemeSmoke, expect_preview: bool, r: &mut R) -> Option<()> {
    use crate::theme::{current_attribute, theme};
    use wrlforge_desktop_protocol::theme::{ThemeSetOutcome, ThemeState, THEMES};

    let st = call::<ThemeState>("theme_get", ipc::NoArgs {}).await.ok()?;
    let sel: web_sys::HtmlSelectElement = crate::element_by_id("theme-select")?;
    r.step(
        "theme: startup theme is the persisted one, applied before use",
        current_attribute().as_deref() == Some(t.expect_startup.as_str())
            && st.theme_id == t.expect_startup
            && sel.value() == t.expect_startup,
        format!(
            "html {:?}, backend {}, select {}",
            current_attribute(),
            st.theme_id,
            sel.value()
        ),
    );
    let notice = doc_el()?
        .get_element_by_id("theme-error")
        .and_then(|e| e.text_content());
    r.step(
        "theme: startup notice shown only for unusable settings",
        notice.is_some() == t.expect_notice
            && (!t.expect_notice
                || notice
                    .as_deref()
                    .is_some_and(|n| n.contains("using Tokyo Night"))),
        notice.unwrap_or_else(|| "(none)".into()),
    );
    let opts: Vec<String> = (0..sel.length())
        .filter_map(|i| sel.item(i))
        .filter_map(|o| o.get_attribute("value"))
        .collect();
    let labelled = doc_el()?
        .query_selector("label[for=\"theme-select\"]")
        .ok()
        .flatten()
        .and_then(|l| l.text_content())
        .is_some_and(|t| t.contains("Theme"));
    r.step(
        "theme: labelled selector lists the three built-in themes",
        labelled && opts == THEMES.iter().map(|d| d.id.to_string()).collect::<Vec<_>>(),
        opts.join(", "),
    );

    // Keyboard focus is visible on the selector.
    let _ = sel.focus();
    let focused = doc_el()?
        .active_element()
        .is_some_and(|a| a.id() == "theme-select");
    let fv = sel.matches(":focus-visible").unwrap_or(false);
    let (style, width, color) = (
        computed(&sel, "outline-style"),
        computed(&sel, "outline-width"),
        computed(&sel, "outline-color"),
    );
    let ring = hex_rgb(&token("--wf-focus-ring"));
    r.step(
        "theme: focused selector shows the focus-ring token",
        focused && (!fv || (style == "solid" && width == "3px" && color == ring)),
        format!("focus-visible {fv}, outline {style} {width} {color} (ring {ring})"),
    );

    // A selection and an Inspector to preserve: the first of the first few
    // tree items that inspects.
    ui::analyze().await;
    wait_for(|| doc_el()?.query_selector(".tree-item").ok().flatten()).await?;
    let items = doc_el()?.query_selector_all(".tree-item").ok()?;
    let mut inspected = None;
    for i in 0..items.length().min(6) {
        let li: HtmlElement = items.item(i)?.dyn_into().ok()?;
        li.click();
        if wait_for(|| ui().inspection.get_untracked().map(|_| ()))
            .await
            .is_some()
        {
            inspected = Some(i);
            break;
        }
    }
    if !r.step(
        "theme: a Scene Tree item was selected and inspected",
        inspected.is_some(),
        format!("tree item #{inspected:?}"),
    ) {
        return None;
    }
    settle().await;
    if expect_preview {
        // Let any pending live preview finish so it is not mistaken for a
        // theme-triggered reload.
        ipc::sleep(900).await;
        wait_for(|| (!ui().preview_status.get_untracked().starts_with("updating")).then_some(()))
            .await;
    }
    let before = doc_state().await?;
    r.step(
        "theme: baseline is a dirty document with history and a selection",
        before.dirty && before.can_undo && before.selected.is_some() && before.inspection.is_some(),
        format!("rev {}, selected {:?}", before.revision, before.selected),
    );

    let mut order: Vec<&str> = THEMES
        .iter()
        .map(|d| d.id)
        .filter(|id| *id != t.expect_startup)
        .collect();
    order.push(t.expect_startup.as_str());
    if order.last() != Some(&t.final_theme.as_str()) {
        order.push(t.final_theme.as_str());
    }
    let mut editor_bgs = vec![];
    for id in order {
        choose(&sel, id)?;
        let applied = current_attribute().as_deref() == Some(id);
        let (ok_surfaces, detail) = surfaces_match();
        let bg = token("--wf-surface-editor");
        if !editor_bgs.contains(&bg) {
            editor_bgs.push(bg.clone());
        }
        r.step(
            &format!("theme {id}: applied immediately to every checked surface"),
            applied && ok_surfaces,
            format!("html {:?}; {detail}", current_attribute()),
        );
        // Wait for Rust's reply (persist or visible failure).
        let replied = wait_for(|| {
            let e = theme().error.get_untracked();
            if t.expect_save_failure {
                e.filter(|m| m.contains("NOT saved")).map(|_| ())
            } else {
                e.is_none().then_some(())
            }
        })
        .await;
        ipc::sleep(60).await;
        let backend = call::<ThemeState>("theme_get", ipc::NoArgs {}).await.ok()?;
        let shown = doc_el()?
            .get_element_by_id("theme-error")
            .and_then(|e| e.text_content());
        r.step(
            &format!(
                "theme {id}: {}",
                if t.expect_save_failure {
                    "save failure is visible and non-fatal"
                } else {
                    "persisted by Rust"
                }
            ),
            replied.is_some()
                && backend.theme_id == id
                && current_attribute().as_deref() == Some(id)
                && (shown.is_some() == t.expect_save_failure),
            shown.unwrap_or_else(|| "(no error shown)".into()),
        );
        let now = doc_state().await?;
        r.step(
            &format!("theme {id}: document, history, selection, Inspector and preview unchanged"),
            now == before,
            if now == before {
                format!("rev {}", now.revision)
            } else {
                format!("{before:?} -> {now:?}")
            },
        );
    }
    r.step(
        "theme: the three themes paint distinct editor surfaces",
        editor_bgs.len() == THEMES.len(),
        editor_bgs.join(" "),
    );
    if expect_preview {
        ipc::sleep(900).await; // past the live-preview debounce
        let now = doc_state().await?;
        r.step(
            "theme: switching never reloaded the X_ITE scene",
            now.preview_loads == before.preview_loads,
            format!("{} loads", now.preview_loads),
        );
    }

    // Rust refuses ids outside the registry, through the real IPC.
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        theme_id: String,
    }
    let out = call::<ThemeSetOutcome>(
        "theme_set",
        Args {
            theme_id: "solarized".into(),
        },
    )
    .await;
    let after = call::<ThemeState>("theme_get", ipc::NoArgs {}).await.ok()?;
    r.step(
        "theme: backend rejects an unknown theme id and keeps the current one",
        matches!(out, Ok(ThemeSetOutcome::Rejected { .. })) && after.theme_id == t.final_theme,
        format!("{out:?}"),
    );
    if !t.expect_save_failure {
        theme().error.set(None);
    }
    Some(())
}
