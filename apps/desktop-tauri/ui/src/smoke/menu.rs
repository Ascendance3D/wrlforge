// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-MENU-1 menu run (`--smoke-menu`).
//!
//! Every menu open, item choice and shortcut is REAL X input inside the
//! harness's own Xvfb. The run proves that:
//! * opening and closing every menu changes no source text, revision,
//!   selection, Inspector, preview or panel object;
//! * the keyboard reaches the bar (F10, Alt, Alt+letter), moves in it (arrows,
//!   Home, End), activates (Enter, Space), closes (Escape) and gives the focus
//!   back with the Source selection intact;
//! * shortcuts run from the Scene Tree, the viewport and an Inspector field
//!   (where Undo stays the field's own), exactly once per key press, and never
//!   with Ctrl+Alt (AltGr), an IME composition, an open dialog or a drag;
//! * menu and toolbar give the same result for Save, Save As, Undo, Redo,
//!   Create, the tools, the theme and Live Preview;
//! * menu Cut / Copy / Paste reach the Rust document through the editor's
//!   own edit path (or are reported as not available);
//! * a Cut or Paste whose clipboard answer arrives late (a controlled
//!   test-only delay) edits nothing when the Source selection, the text or
//!   the document changed meanwhile, and a failed clipboard edits nothing;
//! * a native application-menu event (`menu-command`, as the macOS About /
//!   Quit items send) is validated against the registry and runs through
//!   `command::run` once, also after a layout change; Quit asks through the
//!   CloseGate;
//! * the About dialog takes the focus when it opens, before a (test-only
//!   delayed) `app_info` answer; Escape and Close work while it loads; a
//!   late answer never reopens it or replaces a later open; closing gives the
//!   focus back; the document does not change;
//! * World and reserved items never run;
//! * Quit (menu and Ctrl+Q) asks through the CloseGate, Cancel keeps the
//!   application, the document and the recovery records unchanged;
//! * `WRONG_SOURCE_SELECTIONS = 0`.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent, KeyboardEventInit};
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::command::{self as c, CommandId, MENUS, NOT_MIGRATED};
use wrlforge_desktop_protocol::gizmo::Axis;

use super::create::{all, el, wait_ms};
use super::dock::{key, open_fixture, pick_check, probes_ok, set_probes, PICKS, WRONG};
use super::gizmo::{gz_ready, press_and_move, real};
use super::pick::{calibrate, ready};
use super::{click, doc_state, snapshot, DocState, R};
use crate::editor::{textarea, CORE};
use crate::ipc::{self, call};
use crate::ui::ui;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub real_pointer: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Disk {
    #[allow(dead_code)]
    name: String,
    text: Option<String>,
    asked: u32,
    files: Vec<String>,
}

const FIXTURE: &str = "menu-world.wrl";

async fn disk(name: &str) -> Option<Disk> {
    #[derive(serde::Serialize)]
    struct A<'a> {
        name: &'a str,
    }
    call::<Disk>("smoke_menu_disk", A { name }).await.ok()
}

async fn digest() -> Vec<(String, u64, String)> {
    call::<Vec<(String, u64, String)>>("smoke_menu_recovery_digest", ipc::NoArgs {})
        .await
        .unwrap_or_default()
}

fn active_id() -> String {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|e| e.id())
        .unwrap_or_default()
}

fn center(id: &str) -> Option<(f64, f64)> {
    let r = crate::element_by_id::<web_sys::Element>(id)?.get_bounding_client_rect();
    (r.width() > 0.0).then(|| (r.left() + r.width() / 2.0, r.top() + r.height() / 2.0))
}

async fn real_click_id(id: &str) -> Option<()> {
    let (x, y) = center(id)?;
    real("click", x, y).await.ok()?;
    ipc::sleep(250).await;
    Some(())
}

fn list_open(slug: &str) -> bool {
    crate::element_by_id::<HtmlElement>(&format!("menu-{slug}-list")).is_some()
}

fn any_open() -> bool {
    MENUS.iter().any(|m| list_open(&m.label.to_lowercase()))
}

/// Open menu `slug` with a real click.
async fn open_click(slug: &str) -> Option<()> {
    real_click_id(&format!("menu-{slug}")).await?;
    wait_ms(2000, || list_open(slug).then_some(())).await
}

fn dom_id(c: CommandId) -> String {
    format!("mi-{}", c.key().replace('.', "-"))
}

/// Choose `c` from menu `slug` with real clicks; the menu closes.
async fn choose(slug: &str, c: CommandId) -> Option<()> {
    open_click(slug).await?;
    real_click_id(&dom_id(c)).await?;
    wait_ms(2000, || (!any_open()).then_some(())).await
}

fn attr(id: &str, a: &str) -> Option<String> {
    crate::element_by_id::<web_sys::Element>(id)?.get_attribute(a)
}

async fn settle() {
    super::settle().await;
    ipc::sleep(150).await;
}

/// Insert `text` at the end of the first line, as typing does.
async fn type_edit(text: &str) -> Option<()> {
    let ta = textarea()?;
    let v = ta.value();
    let at = v.encode_utf16().position(|u| u == b'\n' as u16)? as u32;
    let _ = ta.focus();
    ta.set_range_text_with_start_and_end(text, at, at).ok()?;
    let ev = web_sys::InputEvent::new("input").ok()?;
    ta.dispatch_event(&ev).ok()?;
    settle().await;
    Some(())
}

/// The UTF-16 range of `needle` in the textarea.
fn find(needle: &str) -> Option<(u32, u32)> {
    let v: Vec<u16> = textarea()?.value().encode_utf16().collect();
    let n: Vec<u16> = needle.encode_utf16().collect();
    let at = v.windows(n.len()).position(|w| w == n.as_slice())? as u32;
    Some((at, at + n.len() as u32))
}

fn select_source(a: u32, b: u32) -> Option<()> {
    let ta = textarea()?;
    let _ = ta.focus();
    ta.set_selection_range(a, b).ok()?;
    Some(())
}

/// The editor shows exactly what Rust holds.
async fn in_sync() -> Option<(u64, bool)> {
    let s = snapshot().await?;
    let ta = textarea()?;
    Some((s.revision, s.view == ta.value()))
}

/// A synthetic keydown on the focused element (only for what Xvfb cannot
/// produce: an IME composition).
fn synthetic(k: &str, ctrl: bool, composing: bool) {
    let init = KeyboardEventInit::new();
    init.set_key(k);
    init.set_ctrl_key(ctrl);
    init.set_is_composing(composing);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let target = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element());
    if let (Some(t), Ok(ev)) = (
        target,
        KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init),
    ) {
        let _ = t.dispatch_event(&ev);
    }
}

fn runs() -> u64 {
    crate::command::run_count()
}

fn st(d: &Option<DocState>) -> String {
    d.as_ref()
        .map(|d| {
            format!(
                "rev {} dirty {} undo {} redo {}",
                d.revision, d.dirty, d.can_undo, d.can_redo
            )
        })
        .unwrap_or_else(|| "none".into())
}

pub async fn run(plan: &Plan, r: &mut R) -> Option<()> {
    wait_ms(5000, || crate::element_by_id::<HtmlElement>("menubar")).await?;
    // The focus-return checks below are about the Source panel: run them in
    // the Source preset (the default is now Modeling, Source as a tab).
    crate::dock::set_preset(p::layout::PresetId::Source);
    ipc::sleep(800).await;
    structure(r);
    if !r.step(
        "real X pointer input is available (harness Xvfb)",
        plan.real_pointer,
        "the menu run needs real input; synthetic events are not accepted as proof",
    ) {
        return None;
    }
    open_fixture(r).await?;
    click("btn-select")?;
    ready(10_000).await?;
    calibrate(r).await?;
    pick_check("baseline", [0.0, 0.0, 0.0], "Transform Crate", r).await?;
    set_probes();

    open_close(r).await?;
    keyboard(r).await?;
    activation(r).await?;
    shortcuts(r).await?;
    blocked(r).await?;
    menu_focus_keys(r).await?;
    equivalence(r).await?;
    clipboard(r).await?;
    clipboard_races(r).await?;
    native_menu(r).await?;
    about_dialog(r).await?;
    tools_and_view(r).await?;
    world_and_reserved(r).await?;
    quit(r).await?;
    pick_check("after the menu run", [0.0, 0.0, 0.0], "Transform Crate", r).await?;
    r.step(
        "WRONG_SOURCE_SELECTIONS = 0 over the whole run",
        WRONG.get() == 0 && PICKS.get() > 0,
        format!("{} picks, {} wrong", PICKS.get(), WRONG.get()),
    );
    Some(())
}

/// The seven menus, their items and their accessible structure.
fn structure(r: &mut R) {
    let bar = el("#menubar");
    let labels: Vec<String> = all("#menubar > .menubar-menu > .menubar-button")
        .iter()
        .map(|b| b.text_content().unwrap_or_default())
        .collect();
    r.step(
        "menu bar: File | Edit | View | Create | Tools | World | Help (role=menubar, menuitem buttons with aria-haspopup)",
        bar.as_ref().and_then(|b| b.get_attribute("role")).as_deref() == Some("menubar")
            && labels == ["File", "Edit", "View", "Create", "Tools", "World", "Help"]
            && all("#menubar .menubar-button")
                .iter()
                .all(|b| b.get_attribute("role").as_deref() == Some("menuitem")
                    && b.get_attribute("aria-haspopup").as_deref() == Some("menu")),
        format!("{labels:?}"),
    );
    let ids: Vec<String> = MENUS
        .iter()
        .map(|m| format!("menu-{}", m.label.to_lowercase()))
        .collect();
    r.step(
        "menu bar: one roving tab stop (File) and Alt mnemonics announced",
        attr(&ids[0], "tabindex").as_deref() == Some("0")
            && ids[1..]
                .iter()
                .all(|i| attr(i, "tabindex").as_deref() == Some("-1"))
            && attr(&ids[0], "aria-keyshortcuts").as_deref() == Some("Alt+F"),
        "",
    );
}

/// Opening and closing each menu (real clicks, real Escape) changes nothing.
async fn open_close(r: &mut R) -> Option<()> {
    let (a, b) = find("Crate")?;
    select_source(a, b)?;
    let before = doc_state().await;
    let loads = ui().preview_loads.get_untracked();
    let mut opened = Vec::new();
    for m in MENUS {
        let slug = m.label.to_lowercase();
        open_click(&slug).await?;
        let n = all(&format!("#menu-{slug}-list [role^=menuitem]")).len();
        opened.push(format!("{}:{n}", m.label));
        // Escape once: back to the menu button; twice: back to the editor.
        real("escape", 0.0, 0.0).await.ok()?;
        ipc::sleep(150).await;
        real("escape", 0.0, 0.0).await.ok()?;
        ipc::sleep(150).await;
    }
    settle().await;
    let after = doc_state().await;
    r.step(
        "opening and closing every menu changes no source, revision, selection, Inspector or preview (real clicks + Escape)",
        before.is_some() && before == after && ui().preview_loads.get_untracked() == loads,
        format!("{} · {}", opened.join(" "), st(&after)),
    );
    r.step(
        "menus recreate no panel and no X_ITE canvas (object probes intact)",
        probes_ok(),
        "",
    );
    r.step(
        "after Escape the focus is back in Source with the same selection",
        active_id() == "source" && after.as_ref().map(|d| d.selection) == Some((a, b)),
        format!(
            "active {} sel {:?}",
            active_id(),
            after.map(|d| d.selection)
        ),
    );
    // A real outside click closes an open menu.
    open_click("view").await?;
    let sb = el(".statusbar")?.get_bounding_client_rect();
    let (x, y) = (sb.left() + 40.0, sb.top() + sb.height() / 2.0);
    real("click", x, y).await.ok()?;
    ipc::sleep(250).await;
    r.step("a click outside the menu closes it", !any_open(), "");
    Some(())
}

/// F10, Alt, Alt+letter, arrows, Home / End, Escape and focus restoration.
async fn keyboard(r: &mut R) -> Option<()> {
    let (a, b) = find("Crate")?;
    select_source(a, b)?;
    let before = doc_state().await;
    key("F10").await?;
    let f10 = active_id();
    key("Right").await?;
    let right = active_id();
    key("End").await?;
    let end = active_id();
    key("Home").await?;
    let home = active_id();
    key("Left").await?;
    let wrap = active_id();
    r.step(
        "F10 focuses the menu bar; Right / End / Home / Left move between menus (wrapping)",
        f10 == "menu-file"
            && right == "menu-edit"
            && end == "menu-help"
            && home == "menu-file"
            && wrap == "menu-help",
        format!("{f10} {right} {end} {home} {wrap}"),
    );
    key("Home").await?;
    key("Down").await?;
    let first = active_id();
    key("Down").await?;
    let second = active_id();
    key("End").await?;
    let last = active_id();
    key("Home").await?;
    let top = active_id();
    key("Up").await?;
    let up_wrap = active_id();
    r.step(
        "Down opens the menu on its first item; Down / End / Home / Up move between items",
        list_open("file")
            && first == dom_id(CommandId::FileNewWorld)
            && second == dom_id(CommandId::FileOpen)
            && last == dom_id(CommandId::AppQuit)
            && top == first
            && up_wrap == last,
        format!("{first} {second} {last} {top} {up_wrap}"),
    );
    key("Right").await?;
    let next_menu = list_open("edit") && !list_open("file");
    key("Escape").await?;
    let esc1 = active_id();
    key("Escape").await?;
    let esc2 = active_id();
    let after = doc_state().await;
    r.step(
        "Right in an open menu opens the next; Escape closes to its button, Escape again restores the Source focus and selection",
        next_menu
            && esc1 == "menu-edit"
            && esc2 == "source"
            && after.as_ref().map(|d| d.selection) == Some((a, b))
            && before.as_ref().map(|d| (d.revision, d.view_hash)) == after.as_ref().map(|d| (d.revision, d.view_hash)),
        format!("{esc1} {esc2} {:?}", after.as_ref().map(|d| d.selection)),
    );
    key("alt").await?;
    let alt = active_id();
    key("Escape").await?;
    let alt_back = active_id();
    key("alt+v").await?;
    let alt_v = list_open("view");
    key("Escape").await?;
    key("Escape").await?;
    r.step(
        "Alt alone focuses the menu bar (Escape returns); Alt+V opens View",
        alt == "menu-file" && alt_back == "source" && alt_v && active_id() == "source",
        format!("{alt} {alt_back} {alt_v} {}", active_id()),
    );
    Some(())
}

/// Enter and Space activate; a layout item changes no document.
async fn activation(r: &mut R) -> Option<()> {
    let before = doc_state().await;
    let shown0 = crate::dock::shown_untracked().contains(p::layout::PanelId::Diagnostics);
    // View → (Panels) Diagnostics with the keyboard: Space hides it.
    key("alt+v").await?;
    let target = "mi-panel-diagnostics";
    for _ in 0..12 {
        if active_id() == target {
            break;
        }
        key("Down").await?;
    }
    key("space").await?;
    ipc::sleep(300).await;
    let hidden = !crate::dock::shown_untracked().contains(p::layout::PanelId::Diagnostics);
    let restored = active_id();
    key("alt+v").await?;
    for _ in 0..12 {
        if active_id() == target {
            break;
        }
        key("Down").await?;
    }
    let hide_disabled = attr(&dom_id(CommandId::ViewHideDiagnostics), "aria-disabled");
    key("Return").await?;
    ipc::sleep(300).await;
    let shown = crate::dock::shown_untracked().contains(p::layout::PanelId::Diagnostics);
    settle().await;
    let after = doc_state().await;
    r.step(
        "Space hides Diagnostics, Enter shows it again (View menu, keyboard); the focus returns to Source",
        shown0 && hidden && shown && restored == "source",
        format!("hidden {hidden} shown {shown} focus {restored} {hide_disabled:?}"),
    );
    r.step(
        "a layout menu action changes no document state",
        before.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
            == after.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
            && probes_ok(),
        st(&after),
    );
    tab_focus(r).await?;
    Some(())
}

/// PR #142 review: hiding a panel whose tab has the focus moves the focus to
/// the tab now active in the same group (Delete key and View menu).
async fn tab_focus(r: &mut R) -> Option<()> {
    use p::layout::PanelId;
    let before = doc_state().await;
    choose("view", CommandId::ViewPresetModeling).await?;
    ipc::sleep(300).await;
    // Modeling: Diagnostics and Source share one tab group.
    let tab: HtmlElement = crate::element_by_id("dock-tab-diagnostics")?;
    let _ = tab.focus();
    key("Delete").await?;
    let by_key = active_id();
    crate::command::run(CommandId::ViewShowDiagnostics);
    ipc::sleep(300).await;
    let tab: HtmlElement = crate::element_by_id("dock-tab-diagnostics")?;
    let _ = tab.focus();
    open_click("view").await?;
    real_click_id("mi-panel-diagnostics").await?;
    ipc::sleep(300).await;
    let by_menu = active_id();
    let hidden = !crate::dock::shown_untracked().contains(PanelId::Diagnostics);
    crate::command::run(CommandId::ViewShowDiagnostics);
    choose("view", CommandId::ViewPresetSource).await?;
    ipc::sleep(300).await;
    settle().await;
    let after = doc_state().await;
    r.step(
        "a focused tab hidden by Delete or by View → Diagnostics passes the focus to the group's new active tab (Source), not the body",
        by_key == "dock-tab-source" && by_menu == "dock-tab-source" && hidden,
        format!("key {by_key:?} menu {by_menu:?}"),
    );
    r.step(
        "preset changes and tab hiding change no document state; no panel object is recreated",
        before.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
            == after.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
            && probes_ok(),
        st(&after),
    );
    let _ = textarea()?.focus();
    Some(())
}

/// Shortcuts outside the Source editor, once per key press.
async fn shortcuts(r: &mut R) -> Option<()> {
    // Scene Tree focus: Ctrl+S saves.
    type_edit("\n# menu edit 1").await?;
    let tree: HtmlElement = el("ul.tree")?.dyn_into().ok()?;
    let _ = tree.focus();
    let n = runs();
    key("ctrl+s").await?;
    let saved = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let d = disk(FIXTURE).await?;
    let s = snapshot().await?;
    r.step(
        "Ctrl+S with the Scene Tree focused saves (one command run; the file holds the document)",
        saved.is_some() && runs() == n + 1 && d.text.as_deref() == Some(s.view.as_str()),
        format!("runs +{} dirty {}", runs() - n, ui().dirty.get_untracked()),
    );
    // Viewport tab focus: Ctrl+Z undoes once.
    type_edit("\n# menu edit 2").await?;
    let rev = CORE.with_borrow(|c| c.revision);
    let tab: HtmlElement = crate::element_by_id("dock-tab-viewport")?;
    let _ = tab.focus();
    let n = runs();
    key("ctrl+z").await?;
    settle().await;
    let s2 = snapshot().await?;
    r.step(
        "Ctrl+Z with the 3D Viewport tab focused undoes exactly one step (one command run)",
        runs() == n + 1 && s2.revision > rev && s2.view == s.view && s2.can_redo,
        format!("runs +{} rev {rev} -> {}", runs() - n, s2.revision),
    );
    // Inspector field: typing stays in the field; Ctrl+Z stays the field's own.
    type_edit("\n# menu edit 3").await?;
    let input: web_sys::HtmlInputElement = wait_ms(4000, || {
        el("#dock-panel-inspector input[type=text]").and_then(|e| e.dyn_into().ok())
    })
    .await?;
    let _ = input.focus();
    let v0 = input.value();
    let s3 = snapshot().await?;
    key("End").await?;
    key("a").await?;
    key("b").await?;
    let typed = input.value();
    let n = runs();
    key("ctrl+z").await?;
    settle().await;
    let s4 = snapshot().await?;
    r.step(
        "Inspector field: typing goes into the field; Ctrl+Z there never reaches Rust history",
        typed == format!("{v0}ab")
            && runs() == n
            && s4.revision == s3.revision
            && s4.view == s3.view
            && s4.dirty,
        format!(
            "{v0:?} -> {typed:?} · runs +{} rev {} -> {}",
            runs() - n,
            s3.revision,
            s4.revision
        ),
    );
    let n = runs();
    key("ctrl+s").await?;
    let saved = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let s5 = snapshot().await?;
    r.step(
        "Inspector field: Ctrl+S still saves (document command); the unapplied field value is not applied",
        saved.is_some() && runs() == n + 1 && s5.view == s4.view,
        format!("runs +{}", runs() - n),
    );
    // Put the field back (nothing was applied).
    input.set_value(&v0);
    let _ = textarea()?.focus();
    Some(())
}

/// AltGr, IME composition, a dialog and a drag block every shortcut.
async fn blocked(r: &mut R) -> Option<()> {
    type_edit("\n# menu edit 4").await?;
    let ta = textarea()?;
    let _ = ta.focus();
    let text0 = ta.value();
    let n = runs();
    key("ctrl+alt+s").await?;
    key("ctrl+alt+z").await?;
    settle().await;
    r.step(
        "Ctrl+Alt (AltGr) + S / Z runs no command and leaves the text alone",
        runs() == n && ui().dirty.get_untracked() && ta.value() == text0,
        format!("runs +{}", runs() - n),
    );
    let n = runs();
    synthetic("s", true, true);
    synthetic("Process", true, false);
    settle().await;
    r.step(
        "an IME composition key (isComposing / Process) runs no command (synthetic: Xvfb has no IME)",
        runs() == n && ui().dirty.get_untracked(),
        format!("runs +{}", runs() - n),
    );
    // The About dialog owns its keys.
    crate::command::run(CommandId::HelpAbout);
    wait_ms(3000, || (active_id() == "about-close").then_some(())).await?;
    let n = runs();
    key("ctrl+s").await?;
    key("F10").await?;
    settle().await;
    let in_dialog = active_id();
    r.step(
        "with the About dialog focused, Ctrl+S and F10 do nothing (the dialog keeps its keys)",
        runs() == n && ui().dirty.get_untracked() && in_dialog == "about-close" && !any_open(),
        format!("runs +{} focus {in_dialog}", runs() - n),
    );
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(200).await;
    // A Move drag owns the pointer and the keyboard shortcuts.
    let u = ui();
    if !u.move_mode.get_untracked() {
        click("btn-move")?;
    }
    gz_ready(8000).await?;
    let before = doc_state().await;
    let cancels = crate::gizmo::GIZMO.with_borrow(|g| g.cancels);
    let at = press_and_move(Axis::X, 90.0, 5).await?;
    let dragging = crate::gizmo::dragging();
    let n = runs();
    key("ctrl+s").await?;
    key("ctrl+z").await?;
    key("alt+f").await?;
    let menu_during = any_open();
    key("F10").await?;
    let bar_during = crate::menu::bar_has_focus();
    real("escape", at.0, at.1).await.ok()?;
    let canceled = wait_ms(2000, || {
        (crate::gizmo::GIZMO.with_borrow(|g| g.cancels) > cancels).then_some(())
    })
    .await;
    real("up", at.0, at.1).await.ok()?;
    settle().await;
    let after = doc_state().await;
    r.step(
        "during a Move drag, Ctrl+S / Ctrl+Z / Alt+F / F10 do nothing; Escape still cancels the drag; the source is unchanged",
        dragging
            && runs() == n
            && !menu_during
            && !bar_during
            && canceled.is_some()
            && before.as_ref().map(|d| (d.revision, d.view_hash, d.dirty)) == after.as_ref().map(|d| (d.revision, d.view_hash, d.dirty)),
        format!("dragging {dragging} runs +{} menu {menu_during} canceled {}", runs() - n, canceled.is_some()),
    );
    click("btn-move")?;
    // Save so the next block starts clean.
    crate::command::run(CommandId::FileSave);
    wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await?;
    Some(())
}

thread_local! {
    /// Smoke probe: the keys a menu bar control left to the window.
    static REACHED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Two window keydown listeners that only record: the capture one notes a
/// key whose target is in the menu bar (before any handler can close the
/// menu and detach that target), the bubble one records it if it got there.
/// They handle nothing: they show which keys the menu handlers let through
/// to `command::on_window_keydown`.
fn install_reach_probe() {
    use wasm_bindgen::prelude::Closure;
    thread_local! {
        static ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static PENDING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }
    if ON.with(|o| o.replace(true)) {
        return;
    }
    let Some(w) = web_sys::window() else { return };
    let capture = Closure::<dyn Fn(KeyboardEvent)>::new(|ev: KeyboardEvent| {
        let in_bar = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|e| e.closest("#menubar").ok().flatten().is_some());
        let k = ev.key();
        let name = if ev.ctrl_key() && k != "Control" {
            format!("Ctrl+{}", k.to_lowercase())
        } else if ev.alt_key() && k != "Alt" {
            format!("Alt+{}", k.to_lowercase())
        } else {
            k
        };
        PENDING.with_borrow_mut(|p| *p = in_bar.then_some(name));
    });
    let bubble = Closure::<dyn Fn(KeyboardEvent)>::new(|_: KeyboardEvent| {
        if let Some(name) = PENDING.with_borrow_mut(Option::take) {
            REACHED.with_borrow_mut(|r| r.push(name));
        }
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
        "keydown",
        capture.as_ref().unchecked_ref(),
        &opts,
    );
    let _ = w.add_event_listener_with_callback("keydown", bubble.as_ref().unchecked_ref());
    capture.forget();
    bubble.forget();
}

fn reached() -> Vec<String> {
    REACHED.with_borrow_mut(std::mem::take)
}

fn source_selection() -> Option<(u32, u32)> {
    let ta = textarea()?;
    Some((ta.selection_start().ok()??, ta.selection_end().ok()??))
}

/// Select "Crate" in Source (the focus and the selection the menu keys
/// must give back).
fn select_crate() -> Option<(u32, u32)> {
    let (a, b) = find("Crate")?;
    select_source(a, b)?;
    Some((a, b))
}

/// Same document, same selected node, same Source selection.
fn same_state(x: &Option<DocState>, y: &Option<DocState>) -> bool {
    let k = |d: &Option<DocState>| {
        d.as_ref().map(|d| {
            (
                d.revision,
                d.view_hash,
                d.dirty,
                d.selection,
                d.selected.clone(),
            )
        })
    };
    x.is_some() && k(x) == k(y)
}

/// PR #143 review (#5484767419): keys the menu does not use reach the window
/// listener. F10 / Alt toggle the bar from a bar button and from an open
/// menu; shortcuts run once from menu focus and give the focus back.
async fn menu_focus_keys(r: &mut R) -> Option<()> {
    install_reach_probe();
    menu_toggles(r, "").await?;
    menu_shortcuts(r).await?;
    menu_field_and_text_input(r).await?;
    // The same toggles and a popup shortcut after a dock layout change.
    choose("view", CommandId::ViewPresetModeling).await?;
    ipc::sleep(300).await;
    // Modeling puts Source in a tab group: show its tab first.
    crate::command::run(CommandId::ViewFocusSource);
    ipc::sleep(300).await;
    menu_toggles(r, " (Modeling layout)").await?;
    let (a, b) = select_crate()?;
    type_edit("\n# menu focus edit L").await?;
    select_source(a, b)?;
    key("alt+f").await?;
    let open = list_open("file");
    let n = runs();
    key("ctrl+s").await?;
    let saved = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let focus = active_id();
    r.step(
        "after a layout change, Ctrl+S in an open File menu closes it and saves once; Source and its selection come back",
        open && saved.is_some() && runs() == n + 1 && !any_open() && focus == "source" && source_selection() == Some((a, b)),
        format!("open {open} runs +{} focus {focus} sel {:?}", runs() - n, source_selection()),
    );
    choose("view", CommandId::ViewPresetSource).await?;
    ipc::sleep(300).await;
    let _ = textarea()?.focus();
    Some(())
}

/// F10 and Alt alone, twice, from Source; and from an open menu.
async fn menu_toggles(r: &mut R, tag: &str) -> Option<()> {
    let (a, b) = select_crate()?;
    let start = active_id();
    let before = doc_state().await;
    let _ = reached();
    key("F10").await?;
    let f1 = active_id();
    key("F10").await?;
    let f2 = active_id();
    let f2_sel = source_selection();
    r.step(
        &format!(
            "F10 focuses the menu bar; F10 again restores the Source focus and selection{tag}"
        ),
        start == "source" && f1 == "menu-file" && f2 == "source" && f2_sel == Some((a, b)),
        format!("{start} -> {f1} -> {f2} sel {f2_sel:?}"),
    );
    key("alt").await?;
    let a1 = active_id();
    key("alt").await?;
    let a2 = active_id();
    let a2_sel = source_selection();
    r.step(
        &format!("Alt alone focuses the menu bar; Alt again restores the Source focus and selection{tag}"),
        a1 == "menu-file" && a2 == "source" && a2_sel == Some((a, b)),
        format!("{a1} -> {a2} sel {a2_sel:?}"),
    );
    // From an open menu: F10 and Alt close it and give the focus back.
    key("F10").await?;
    key("Down").await?;
    let p1 = list_open("file");
    key("F10").await?;
    let p1_back = (active_id(), any_open());
    key("alt+e").await?;
    let p2 = list_open("edit");
    key("alt").await?;
    let p2_back = (active_id(), any_open());
    r.step(
        &format!("in an open menu, F10 or Alt alone closes it and restores the Source focus and selection{tag}"),
        p1 && p2
            && p1_back == ("source".to_string(), false)
            && p2_back == ("source".to_string(), false)
            && source_selection() == Some((a, b)),
        format!("{p1} {p1_back:?} {p2} {p2_back:?}"),
    );
    // Alt+letter from a focused bar button opens that menu.
    key("F10").await?;
    key("alt+v").await?;
    let v = list_open("view");
    key("Escape").await?;
    key("Escape").await?;
    let back = active_id();
    let after = doc_state().await;
    let seen = reached();
    let nav_kept = !seen
        .iter()
        .any(|k| matches!(k.as_str(), "ArrowDown" | "Escape"));
    r.step(
        &format!("Alt+V on a focused bar button opens View; Escape twice restores Source; no document, node or selection change{tag}"),
        v && back == "source" && same_state(&before, &after),
        format!("view {v} focus {back} {}", st(&after)),
    );
    r.step(
        &format!("the keys the menu does not use reach the window listener (F10, Alt, Alt+V); the keys it uses (Down, Escape) do not{tag}"),
        seen.iter().filter(|k| *k == "F10").count() == 2
            && seen.iter().filter(|k| *k == "Alt").count() == 3
            && seen.iter().any(|k| k == "Alt+v")
            && nav_kept,
        format!("{seen:?}"),
    );
    Some(())
}

/// Registry shortcuts from a focused bar button and from an open menu:
/// one run each, the popup closes first, the Source focus comes back.
async fn menu_shortcuts(r: &mut R) -> Option<()> {
    let _ = select_crate()?;
    type_edit("\n# menu focus edit 1").await?;
    let (a, b) = select_crate()?;
    let _ = reached();
    key("F10").await?;
    let bar = active_id();
    let n = runs();
    key("ctrl+s").await?;
    let saved = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let d = disk(FIXTURE).await?;
    let s = snapshot().await?;
    let focus = active_id();
    r.step(
        "Ctrl+S with the File button focused saves once (the file holds the document); the focus and selection return to Source",
        bar == "menu-file"
            && saved.is_some()
            && runs() == n + 1
            && d.text.as_deref() == Some(s.view.as_str())
            && focus == "source"
            && source_selection() == Some((a, b)),
        format!("runs +{} focus {focus} sel {:?}", runs() - n, source_selection()),
    );
    type_edit("\n# menu focus edit 2").await?;
    let (a, b) = select_crate()?;
    // Node ids carry source offsets: take them after the edit.
    let node0 = doc_state().await?.selected;
    key("alt+f").await?;
    let open = list_open("file");
    let n = runs();
    key("ctrl+s").await?;
    let saved = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let focus = active_id();
    r.step(
        "Ctrl+S in an open File menu closes the menu, then saves once; the focus and selection return to Source",
        open && saved.is_some() && runs() == n + 1 && !any_open() && focus == "source" && source_selection() == Some((a, b)),
        format!("open {open} runs +{} focus {focus}", runs() - n),
    );
    let node1 = doc_state().await?.selected;
    // Undo from a bar button, Redo from an open menu: one step each.
    let s0 = snapshot().await?;
    type_edit("\n# menu focus edit 3").await?;
    let s1 = snapshot().await?;
    select_crate()?;
    key("F10").await?;
    let n = runs();
    key("ctrl+z").await?;
    settle().await;
    let s2 = snapshot().await?;
    key("alt+e").await?;
    key("ctrl+shift+z").await?;
    settle().await;
    let s3 = snapshot().await?;
    let sync = in_sync().await;
    r.step(
        "Ctrl+Z from the File button undoes one step; Ctrl+Shift+Z from the open Edit menu redoes one step (2 runs, Rust == editor)",
        runs() == n + 2
            && s2.view == s0.view
            && s2.can_redo
            && s3.view == s1.view
            && !any_open()
            && active_id() == "source"
            && sync.is_some_and(|s| s.1),
        format!("runs +{} undo {} redo {}", runs() - n, s2.view == s0.view, s3.view == s1.view),
    );
    let seen = reached();
    let shortcuts: Vec<&String> = seen
        .iter()
        .filter(|k| k.starts_with("Ctrl+") && *k != "Ctrl+shift")
        .collect();
    r.step(
        "each shortcut reached the window once and ran once; Save from an open menu leaves the selected source node unchanged",
        shortcuts == ["Ctrl+s", "Ctrl+s", "Ctrl+z", "Ctrl+z"] && node0.is_some() && node0 == node1,
        format!("{seen:?} node {:?} -> {:?}", node0.as_deref(), node1.as_deref()),
    );
    crate::command::run(CommandId::FileSave);
    wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await?;
    Some(())
}

/// An Inspector field keeps Ctrl+Z when a menu takes the focus from it;
/// AltGr and IME keys in the menu run nothing.
async fn menu_field_and_text_input(r: &mut R) -> Option<()> {
    type_edit("\n# menu focus edit 4").await?;
    let input: web_sys::HtmlInputElement = wait_ms(4000, || {
        el("#dock-panel-inspector input[type=text]").and_then(|e| e.dyn_into().ok())
    })
    .await?;
    let _ = input.focus();
    let v0 = input.value();
    key("End").await?;
    key("a").await?;
    key("ctrl+z").await?;
    let own0 = input.value();
    key("End").await?;
    key("a").await?;
    key("b").await?;
    let typed = input.value();
    let s0 = snapshot().await?;
    let n = runs();
    key("F10").await?;
    key("ctrl+z").await?;
    key("ctrl+shift+z").await?;
    key("alt+e").await?;
    key("ctrl+z").await?;
    key("Escape").await?;
    key("Escape").await?;
    settle().await;
    let s1 = snapshot().await?;
    let is_input = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .is_some_and(|e| e.is_same_node(Some(input.as_ref())));
    let kept = input.value();
    // The field's own history is still there.
    key("ctrl+z").await?;
    let own_undo = input.value();
    r.step(
        "from an Inspector field, Ctrl+Z / Ctrl+Shift+Z with the menu bar focused or the Edit menu open run no document history; the field keeps its text and its own Ctrl+Z",
        typed.ends_with("ab")
            && runs() == n
            && s1.revision == s0.revision
            && s1.view == s0.view
            && is_input
            && kept == typed
            && (own0 == v0) == (own_undo != typed),
        format!(
            "runs +{} rev {} -> {} focus {is_input} · field undo before the menu {v0:?}+a -> {own0:?}, after {typed:?} -> {own_undo:?}",
            runs() - n,
            s0.revision,
            s1.revision
        ),
    );
    input.set_value(&v0);
    // AltGr (Ctrl+Alt) and IME keys on a bar button and in an open menu.
    let (a, b) = select_crate()?;
    let before = doc_state().await;
    let n = runs();
    key("F10").await?;
    key("ctrl+alt+s").await?;
    key("ctrl+alt+z").await?;
    synthetic("s", true, true);
    synthetic("Process", true, false);
    let bar_kept = active_id() == "menu-file";
    key("Down").await?;
    key("ctrl+alt+s").await?;
    synthetic("s", true, true);
    synthetic("Process", true, false);
    let menu_kept = list_open("file");
    key("Escape").await?;
    key("Escape").await?;
    settle().await;
    let after = doc_state().await;
    r.step(
        "AltGr (Ctrl+Alt) and IME keys on a bar button or in an open menu run no command and change nothing (IME: synthetic)",
        runs() == n
            && bar_kept
            && menu_kept
            && active_id() == "source"
            && source_selection() == Some((a, b))
            && same_state(&before, &after),
        format!("runs +{} bar {bar_kept} menu {menu_kept} {}", runs() - n, st(&after)),
    );
    crate::command::run(CommandId::FileSave);
    wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await?;
    let _ = textarea()?.focus();
    Some(())
}

/// Menu and toolbar run the same commands with the same results.
async fn equivalence(r: &mut R) -> Option<()> {
    // Undo / Redo.
    type_edit("\n# menu edit 5").await?;
    let base = snapshot().await?;
    choose("edit", CommandId::EditUndo).await?;
    settle().await;
    let mu = snapshot().await?;
    choose("edit", CommandId::EditRedo).await?;
    settle().await;
    let mr = snapshot().await?;
    real_click_id("btn-undo").await?;
    settle().await;
    let tu = snapshot().await?;
    real_click_id("btn-redo").await?;
    settle().await;
    let tr = snapshot().await?;
    r.step(
        "Undo / Redo: the menu and the toolbar give the same text and history state",
        mu.view == tu.view
            && mr.view == tr.view
            && mr.view == base.view
            && (mu.can_undo, mu.can_redo) == (tu.can_undo, tu.can_redo)
            && (mr.can_undo, mr.can_redo) == (tr.can_undo, tr.can_redo)
            && in_sync().await.is_some_and(|s| s.1),
        format!(
            "menu {}/{} toolbar {}/{}",
            mu.revision, mr.revision, tu.revision, tr.revision
        ),
    );
    // Save.
    choose("file", CommandId::FileSave).await?;
    let a = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let da = disk(FIXTURE).await?;
    let sa = snapshot().await?;
    type_edit("\n# menu edit 6").await?;
    real_click_id("btn-save").await?;
    let b = wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await;
    let db = disk(FIXTURE).await?;
    let sb = snapshot().await?;
    r.step(
        "Save: the menu and the toolbar both write exactly the document text and clear Modified",
        a.is_some()
            && b.is_some()
            && da.text.as_deref() == Some(sa.view.as_str())
            && db.text.as_deref() == Some(sb.view.as_str()),
        "",
    );
    // Save As: menu, toolbar, then a canceled dialog.
    let files0 = db.files.clone();
    let rev0 = snapshot().await?.revision;
    choose("file", CommandId::FileSaveAs).await?;
    ipc::sleep(800).await;
    let m = disk("menu-save-as.wrl").await?;
    real_click_id("btn-save-as").await?;
    ipc::sleep(800).await;
    let t = disk("toolbar-save-as.wrl").await?;
    let s = snapshot().await?;
    r.step(
        "Save As: the menu and the toolbar write identical files holding the document text",
        m.text.is_some() && m.text == t.text && t.text.as_deref() == Some(s.view.as_str()),
        format!("{:?}", t.files),
    );
    let before = doc_state().await;
    choose("file", CommandId::FileSaveAs).await?;
    ipc::sleep(800).await;
    let c = disk(FIXTURE).await?;
    let after = doc_state().await;
    r.step(
        "Save As canceled: no file written, document state unchanged",
        c.files.len() == files0.len() + 2
            && before.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
                == after.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
            && s.revision == rev0,
        format!("{:?}", c.files),
    );
    // Create Box: menu, Undo, toolbar, Undo.
    let base = snapshot().await?;
    choose("create", CommandId::CreateBox).await?;
    let v1 = wait_ms(6000, || {
        let n = CORE.with_borrow(|c| c.revision);
        (n > base.revision).then_some(())
    })
    .await;
    settle().await;
    let m = snapshot().await?;
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let undone1 = snapshot().await?;
    real_click_id("btn-create").await?;
    wait_ms(2000, || el("#create-list")).await?;
    real_click_id("create-box").await?;
    wait_ms(6000, || {
        (CORE.with_borrow(|c| c.revision) > undone1.revision).then_some(())
    })
    .await?;
    settle().await;
    let t = snapshot().await?;
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let undone2 = snapshot().await?;
    r.step(
        "Create → Box (menu) and Create ▾ → Box (toolbar) insert the same source; Undo restores it exactly",
        v1.is_some()
            && m.view == t.view
            && m.view != base.view
            && undone1.view == base.view
            && undone2.view == base.view,
        format!("rev {} -> {} / {}", base.revision, m.revision, t.revision),
    );
    Some(())
}

/// Edit → Cut / Copy / Paste / Select All through the canonical edit path.
async fn clipboard(r: &mut R) -> Option<()> {
    let (a, b) = find("UI-MENU-1 fixture")?;
    select_source(a, b)?;
    let s0 = snapshot().await?;
    choose("edit", CommandId::EditCopy).await?;
    ipc::sleep(300).await;
    // The clipboard is checked by pasting it back from the menu: a read
    // outside a user gesture is refused by WebKit (by design).
    let copy_msg = ui().message.get_untracked();
    let s1 = snapshot().await?;
    r.step(
        "Edit → Copy puts the Source selection on the clipboard and changes no document state",
        copy_msg.starts_with("Copied 17 characters")
            && s1.revision == s0.revision
            && s1.view == s0.view,
        copy_msg,
    );
    // Paste at the end of the comment line.
    let (_, end) = find("fixture ✓")?;
    select_source(end, end)?;
    choose("edit", CommandId::EditPaste).await?;
    settle().await;
    let s2 = snapshot().await?;
    let pasted = s2.view.contains("fixture ✓UI-MENU-1 fixture");
    let sync = in_sync().await;
    r.step(
        "Edit → Paste inserts the copied text into Source through doc_edit (revision +1, Rust text == editor text)",
        pasted && s2.revision > s0.revision && sync.is_some_and(|s| s.1),
        format!("rev {} -> {} sync {sync:?} · {}", s0.revision, s2.revision, if pasted { "inserted" } else { "NOT inserted" }),
    );
    // Cut the pasted copy, then paste it back to prove the clipboard holds it.
    let (pa, pb) = find("✓UI-MENU-1 fixture")?;
    select_source(pa + 1, pb)?;
    choose("edit", CommandId::EditCut).await?;
    settle().await;
    let s3 = snapshot().await?;
    let sync = in_sync().await;
    let (_, at) = find("# fin")?;
    select_source(at, at)?;
    choose("edit", CommandId::EditPaste).await?;
    settle().await;
    let s3b = snapshot().await?;
    let back = s3b.view.contains("# finUI-MENU-1 fixture");
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let s3c = snapshot().await?;
    r.step(
        "Edit → Cut copies, then removes the selection through doc_edit (Rust text == editor text == original; the cut text pastes back)",
        s3.view == s0.view && s3.revision > s2.revision && sync.is_some_and(|s| s.1) && back && s3c.view == s0.view,
        format!("rev {} -> {} -> {} back {back}", s2.revision, s3.revision, s3b.revision),
    );
    // Select All in Source, then Cut is enabled; with the Scene Tree focused
    // the Source-only items are disabled and say why.
    let _ = textarea()?.focus();
    choose("edit", CommandId::EditSelectAll).await?;
    let ta = textarea()?;
    let all_sel = ta.selection_start().ok()?? == 0
        && ta.selection_end().ok()?? as usize == ta.value().encode_utf16().count();
    let s4 = snapshot().await?;
    r.step(
        "Edit → Select All selects the whole Source text and changes no document state",
        all_sel && s4.revision == s3c.revision && active_id() == "source",
        "",
    );
    let tree: HtmlElement = el("ul.tree")?.dyn_into().ok()?;
    let _ = tree.focus();
    open_click("edit").await?;
    let paste_off = attr(&dom_id(CommandId::EditPaste), "aria-disabled");
    let paste_title = attr(&dom_id(CommandId::EditPaste), "title").unwrap_or_default();
    let n = runs();
    real_click_id(&dom_id(CommandId::EditPaste)).await?;
    let still_open = list_open("edit");
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(200).await;
    let s5 = snapshot().await?;
    r.step(
        "with the Scene Tree focused, Paste is disabled with a reason and a click on it runs nothing",
        paste_off.as_deref() == Some("true")
            && paste_title.contains("not available")
            && runs() == n
            && still_open
            && s5.revision == s4.revision,
        paste_title,
    );
    let _ = textarea()?.focus();
    crate::command::run(CommandId::FileSave);
    wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await?;
    Some(())
}

/// The document content, without the selection (it moves in these tests):
/// revision, dirty, Rust text, undo / redo, widget text.
async fn content() -> Option<(u64, bool, String, bool, bool, String)> {
    let s = snapshot().await?;
    Some((
        s.revision,
        s.dirty,
        s.view,
        s.can_undo,
        s.can_redo,
        textarea()?.value(),
    ))
}

fn about_state() -> &'static str {
    match crate::about::about().get_untracked() {
        None => "closed",
        Some(crate::about::About::Loading) => "loading",
        Some(crate::about::About::Ready(_)) => "ready",
        Some(crate::about::About::Failed(_)) => "failed",
    }
}

/// STACK-REVIEW-1 (PR #141 r4242710837): open About with an `app_info`
/// answer held back `ms` (or failed), as a slow IPC would.
fn about_delayed(ms: i32, fail: bool) {
    crate::about::show_with(async move {
        ipc::sleep(ms).await;
        if fail {
            Err("test-only late failure".into())
        } else {
            call::<c::AppInfo>("app_info", ipc::NoArgs {}).await
        }
    });
}

/// The About dialog is keyboard-usable before `app_info` answers.
async fn about_dialog(r: &mut R) -> Option<()> {
    const LATE: i32 = 1500;
    let before = doc_state().await?;
    let ta = textarea()?;
    let _ = ta.focus();
    let sel0 = (ta.selection_start().ok()??, ta.selection_end().ok()??);
    // 1. Focus moves in at once; Escape closes it while it loads.
    about_delayed(LATE, false);
    let focused = wait_ms(400, || (active_id() == "about-close").then_some(())).await;
    let loading = about_state();
    r.step(
        "About takes the focus when it opens, before a delayed app_info answer",
        focused.is_some() && loading == "loading",
        format!("focus {} state {loading}", active_id()),
    );
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(150).await;
    let gone = crate::element_by_id::<HtmlElement>("about-dialog").is_none();
    let back = active_id();
    ipc::sleep(LATE + 400).await;
    r.step(
        "Escape closes About while it loads, the focus goes back to Source, the late answer does not reopen it",
        gone && back == "source" && about_state() == "closed"
            && crate::element_by_id::<HtmlElement>("about-dialog").is_none(),
        format!("closed {gone} focus {back} after-late {}", about_state()),
    );
    // 2. Close (a real click) while it loads.
    let _ = textarea()?.focus();
    about_delayed(LATE, false);
    wait_ms(400, || (active_id() == "about-close").then_some(())).await?;
    real_click_id("about-close").await?;
    let back = active_id();
    ipc::sleep(LATE + 400).await;
    r.step(
        "Close works while About loads, the focus goes back, the late answer does not reopen it",
        back == "source" && about_state() == "closed",
        format!("focus {back} after-late {}", about_state()),
    );
    // 3. Closed and opened again while the first answer is pending: the
    //    older (failing) answer does not replace the newer state.
    let _ = textarea()?.focus();
    about_delayed(LATE, true);
    wait_ms(400, || (active_id() == "about-close").then_some(())).await?;
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(150).await;
    about_delayed(300, false);
    ipc::sleep(LATE + 400).await;
    let state = about_state();
    r.step(
        "a late answer for an earlier open does not replace a later open's state",
        state == "ready",
        format!("state {state}"),
    );
    // 4. The answer arrives: the version shows, the focus stays on Close.
    let identity = el("#about-identity")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    let facts = el("#about-facts")
        .and_then(|e| e.text_content())
        .unwrap_or_default();
    let focus = active_id();
    r.step(
        "after the answer, About shows the application version and keeps the focus on Close",
        focus == "about-close"
            && !identity.is_empty()
            && facts.contains(c::APP_VERSION)
            && !facts.contains("Warning"),
        format!("focus {focus} identity {identity:?}"),
    );
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(150).await;
    let back = active_id();
    let ta = textarea()?;
    let sel1 = (ta.selection_start().ok()??, ta.selection_end().ok()??);
    let after = doc_state().await?;
    r.step(
        "closing About gives the focus back to Source with its selection; the document did not change",
        back == "source" && sel1 == sel0 && after == before,
        format!("focus {back} sel {sel0:?}->{sel1:?} same-doc {}", after == before),
    );
    Some(())
}

fn hold(delay_ms: i32, fail: bool) {
    crate::clipboard::set_test_hold(Some(crate::clipboard::TestHold { delay_ms, fail }));
}

fn no_hold() {
    crate::clipboard::set_test_hold(None);
}

fn cstats() -> crate::clipboard::Stats {
    crate::clipboard::stats()
}

/// Wait until the clipboard operations started so far have ended.
async fn ops_done(before: crate::clipboard::Stats, n: u64, ms: u32) {
    let _ = wait_ms(ms, || {
        let s = cstats();
        (s.applied + s.stale >= before.applied + before.stale + n).then_some(())
    })
    .await;
    settle().await;
}

const HOLD_MS: i32 = 2500;

/// Late clipboard answers: a stale Cut / Paste changes nothing.
async fn clipboard_races(r: &mut R) -> Option<()> {
    let _ = textarea()?.focus();
    // 1. Cut, then the selection moves before the clipboard answers.
    let (a, b) = find("UI-MENU-1 fixture")?;
    select_source(a, b)?;
    let c0 = content().await?;
    let k0 = cstats();
    hold(HOLD_MS, false);
    choose("edit", CommandId::EditCut).await?;
    let (x, y) = find("Crate")?;
    select_source(x, y)?;
    ops_done(k0, 1, 6000).await;
    let c1 = content().await?;
    let k1 = cstats();
    r.step(
        "a delayed Cut after the Source selection moved edits nothing (no text, no revision, no undo entry)",
        c1 == c0 && k1.stale == k0.stale + 1 && k1.applied == k0.applied,
        format!("{k0:?} -> {k1:?} rev {} -> {}", c0.0, c1.0),
    );
    // 2. Paste, then the selection moves.
    let (_, e) = find("# fin")?;
    select_source(e, e)?;
    choose("edit", CommandId::EditPaste).await?;
    select_source(0, 0)?;
    ops_done(k1, 1, 6000).await;
    let c2 = content().await?;
    let k2 = cstats();
    r.step(
        "a delayed Paste after the Source selection moved edits nothing",
        c2 == c0 && k2.stale == k1.stale + 1 && k2.applied == k1.applied,
        format!("{k1:?} -> {k2:?}"),
    );
    // 3. Paste, then the user types (an edit of the same document).
    select_source(e, e)?;
    choose("edit", CommandId::EditPaste).await?;
    type_edit(" typed").await?;
    let typed = content().await?;
    ops_done(k2, 1, 6000).await;
    let c3 = content().await?;
    let k3 = cstats();
    r.step(
        "a delayed Paste after a typed Source edit keeps the typed edit only",
        c3 == typed
            && typed.0 == c0.0 + 1
            && c3.5 == c3.2
            && !c3.2.contains("# finUI-MENU-1")
            && k3.stale == k2.stale + 1,
        format!("rev {} -> {} {k3:?}", c0.0, c3.0),
    );
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let undone = content().await?;
    // 4. Cut and Paste while the document is replaced (opened again).
    crate::command::run(CommandId::FileSave);
    wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await?;
    let mut replaced_ok = true;
    let mut detail = String::new();
    for c in [CommandId::EditCut, CommandId::EditPaste] {
        let (a, b) = find("UI-MENU-1 fixture")?;
        select_source(a, b)?;
        let k = cstats();
        let s0 = CORE.with_borrow(|c| (c.session, c.generation));
        choose("edit", c).await?;
        #[derive(serde::Serialize)]
        struct A {
            index: usize,
        }
        let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: 0 })
            .await
            .ok()?;
        crate::ui::apply_open(o, "Opened").await;
        let replaced = content().await?;
        let s1 = CORE.with_borrow(|c| (c.session, c.generation));
        ops_done(k, 1, 6000).await;
        let after = content().await?;
        let k2 = cstats();
        let ok = s1 != s0
            && after == replaced
            && after.5 == after.2
            && !after.1
            && !after.3
            && k2.stale == k.stale + 1
            && k2.applied == k.applied;
        replaced_ok &= ok;
        detail += &format!("{}: {ok} rev {} undo {}; ", c.key(), after.0, after.3);
    }
    r.step(
        "a delayed Cut / Paste after the document was replaced edits nothing (new document clean, no undo entry)",
        replaced_ok && undone.2 == c0.2,
        detail,
    );
    // 5. A canceled clipboard: nothing changes, nothing is counted as applied.
    hold(300, true);
    let (a, b) = find("UI-MENU-1 fixture")?;
    select_source(a, b)?;
    let f0 = content().await?;
    let k = cstats();
    choose("edit", CommandId::EditCut).await?;
    ipc::sleep(800).await;
    let cut_msg = ui().message.get_untracked();
    select_source(b, b)?;
    choose("edit", CommandId::EditPaste).await?;
    ipc::sleep(800).await;
    let paste_msg = ui().message.get_untracked();
    settle().await;
    let f1 = content().await?;
    r.step(
        "a canceled / failed clipboard Cut or Paste edits nothing and says so",
        f1 == f0
            && cstats() == k
            && cut_msg.contains("Cut is not available")
            && paste_msg.contains("Paste is not available"),
        format!("{cut_msg} | {paste_msg}"),
    );
    // 6. Rapid Cut, Cut, Paste: the first applies, the rest are stale.
    hold(1800, false);
    select_source(a, b)?;
    let k = cstats();
    choose("edit", CommandId::EditCut).await?;
    choose("edit", CommandId::EditCut).await?;
    choose("edit", CommandId::EditPaste).await?;
    ops_done(k, 3, 9000).await;
    let g1 = content().await?;
    let k6 = cstats();
    let expect: String = {
        let v: Vec<u16> = f0.2.encode_utf16().collect();
        String::from_utf16(&[&v[..a as usize], &v[b as usize..]].concat()).ok()?
    };
    let one_rev = g1.0 == f0.0 + 1 && g1.2 == expect && g1.5 == g1.2 && g1.3;
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let g2 = content().await?;
    r.step(
        "rapid Cut, Cut, Paste: the first Cut reaches Rust as ONE edit (one undo restores it), the later two are refused as stale",
        one_rev && k6.applied == k.applied + 1 && k6.stale == k.stale + 2 && g2.2 == f0.2 && g2.5 == g2.2,
        format!("{k:?} -> {k6:?} rev {} -> {} -> {}", f0.0, g1.0, g2.0),
    );
    // 7. A Paste chosen while a Source edit is still in flight waits for it,
    // then pastes at the caret on the acknowledged text. The in-flight edit
    // is held here by keeping the editor busy (test only), then released.
    no_hold();
    let (_, at) = find("# fin")?;
    let ta = textarea()?;
    let _ = ta.focus();
    CORE.with_borrow_mut(|c| c.busy = true);
    ta.set_range_text_with_start_and_end("!", at, at).ok()?;
    ta.set_selection_range(at + 1, at + 1).ok()?;
    let k = cstats();
    let p0 = content().await?;
    choose("edit", CommandId::EditPaste).await?;
    ipc::sleep(400).await;
    let waited = cstats() == k;
    CORE.with_borrow_mut(|c| c.busy = false);
    crate::editor::on_input();
    ops_done(k, 1, 6000).await;
    let p1 = content().await?;
    let k7 = cstats();
    let both = p1.2.contains("# fin!UI-MENU-1 fixture");
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let p2 = content().await?;
    r.step(
        "a Paste chosen during a pending Source edit waits for it, then edits once (pending edit and paste are two undo entries)",
        waited
            && both
            && p1.0 == p0.0 + 2
            && p1.5 == p1.2
            && k7.applied == k.applied + 1
            && p2.2.contains("# fin!")
            && !p2.2.contains("# fin!UI-MENU-1"),
        format!("rev {} -> {} -> {} {k7:?}", p0.0, p1.0, p2.0),
    );
    crate::command::run(CommandId::EditUndo);
    settle().await;
    let p3 = content().await?;
    r.step(
        "after the clipboard races, Rust text == editor text == the saved document",
        p3.2 == f0.2 && p3.5 == p3.2,
        format!("rev {}", p3.0),
    );
    crate::command::run(CommandId::FileSave);
    wait_ms(4000, || (!ui().dirty.get_untracked()).then_some(())).await?;
    Some(())
}

async fn native(key: &str, raw: bool) -> Option<bool> {
    #[derive(serde::Serialize)]
    struct A<'a> {
        key: &'a str,
        raw: bool,
    }
    call::<bool>("smoke_menu_native", A { key, raw }).await.ok()
}

/// The macOS application menu's event path (`menu-command`), driven by the
/// backend's own `app_menu::forward`. The native NSMenu itself is not on
/// Linux: this proves the event path, not the macOS menu.
async fn native_menu(r: &mut R) -> Option<()> {
    let s0 = crate::menu::native_stats();
    let n0 = runs();
    let sent = native(CommandId::HelpAbout.key(), false).await?;
    let got = wait_ms(3000, || (active_id() == "about-close").then_some(())).await;
    ipc::sleep(300).await;
    let s1 = crate::menu::native_stats();
    r.step(
        "a native About menu event runs Help → About through command::run exactly once",
        sent && got.is_some()
            && s1.ran == s0.ran + 1
            && s1.received == s0.received + 1
            && runs() == n0 + 1,
        format!("{s0:?} -> {s1:?} runs +{}", runs() - n0),
    );
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(300).await;
    // A layout change does not add a listener: still one run per event.
    crate::command::run(CommandId::ViewPresetSource);
    settle().await;
    crate::command::run(CommandId::ViewResetLayout);
    settle().await;
    let n1 = runs();
    native(CommandId::HelpAbout.key(), false).await?;
    let got = wait_ms(3000, || (active_id() == "about-close").then_some(())).await;
    ipc::sleep(300).await;
    let s2 = crate::menu::native_stats();
    r.step(
        "after a layout change a native menu event still runs once (one listener)",
        got.is_some() && s2.ran == s1.ran + 1 && s2.received == s1.received + 1 && runs() == n1 + 1,
        format!("{s1:?} -> {s2:?} runs +{}", runs() - n1),
    );
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(300).await;
    // Keys outside the application menu: the backend does not send them, and
    // the UI refuses them when they arrive anyway.
    let c0 = content().await?;
    let n2 = runs();
    let fwd = native(CommandId::FileSave.key(), false).await?;
    for k in [CommandId::FileSave.key(), "file.delete-everything", ""] {
        native(k, true).await?;
    }
    ipc::sleep(600).await;
    let s3 = crate::menu::native_stats();
    let c1 = content().await?;
    r.step(
        "native menu events outside the application menu run nothing (backend and UI check the registry)",
        !fwd && s3.received == s2.received + 3 && s3.refused == s2.refused + 3 && s3.ran == s2.ran && runs() == n2 && c1 == c0,
        format!("{s2:?} -> {s3:?}"),
    );
    let _ = textarea()?.focus();
    Some(())
}

/// Tools, theme and Live Preview: menu and toolbar show the same state.
async fn tools_and_view(r: &mut R) -> Option<()> {
    let loads = ui().preview_loads.get_untracked();
    let before = doc_state().await;
    choose("tools", CommandId::ToolMove).await?;
    let mv = attr("btn-move", "aria-pressed");
    open_click("tools").await?;
    let m_move = attr(&dom_id(CommandId::ToolMove), "aria-checked");
    let m_sel = attr(&dom_id(CommandId::ToolSelect), "aria-checked");
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    real_click_id("btn-rotate").await?;
    open_click("tools").await?;
    let m_rot = attr(&dom_id(CommandId::ToolRotate), "aria-checked");
    let m_move2 = attr(&dom_id(CommandId::ToolMove), "aria-checked");
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    r.step(
        "Tools → Move presses the toolbar Move button; the toolbar Rotate button checks Tools → Rotate (one tool state)",
        mv.as_deref() == Some("true")
            && m_move.as_deref() == Some("true")
            && m_sel.as_deref() == Some("false")
            && m_rot.as_deref() == Some("true")
            && m_move2.as_deref() == Some("false"),
        format!("{mv:?} {m_move:?} {m_sel:?} {m_rot:?} {m_move2:?}"),
    );
    choose("tools", CommandId::ToolSelect).await?;
    // Theme.
    choose("view", CommandId::ViewThemeTokyoNightLight).await?;
    ipc::sleep(300).await;
    let html = crate::theme::current_attribute();
    let sel = el("#theme-select")
        .and_then(|e| e.dyn_into::<web_sys::HtmlSelectElement>().ok())
        .map(|s| s.value());
    open_click("view").await?;
    let checked = attr(&dom_id(CommandId::ViewThemeTokyoNightLight), "aria-checked");
    let other = attr(&dom_id(CommandId::ViewThemeTokyoNight), "aria-checked");
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    choose("view", CommandId::ViewThemeTokyoNight).await?;
    r.step(
        "View → Tokyo Night Light sets the theme the toolbar picker shows (radio checked); the preview is not reloaded",
        html.as_deref() == Some("tokyo-night-light")
            && sel.as_deref() == Some("tokyo-night-light")
            && checked.as_deref() == Some("true")
            && other.as_deref() == Some("false")
            && ui().preview_loads.get_untracked() == loads,
        format!("{html:?} {sel:?} loads {} -> {}", loads, ui().preview_loads.get_untracked()),
    );
    // Live Preview.
    choose("view", CommandId::ViewLivePreview).await?;
    let off_box = el("#chk-live")?
        .dyn_into::<web_sys::HtmlInputElement>()
        .ok()?
        .checked();
    real_click_id("chk-live").await?;
    open_click("view").await?;
    let on_menu = attr(&dom_id(CommandId::ViewLivePreview), "aria-checked");
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    settle().await;
    let after = doc_state().await;
    r.step(
        "View → Live Preview unchecks the toolbar Live box; the box checks the menu item again",
        !off_box && on_menu.as_deref() == Some("true") && ui().preview_enabled.get_untracked(),
        format!("box {off_box} menu {on_menu:?}"),
    );
    r.step(
        "tool, theme and preview menu actions change no document state",
        before.as_ref().map(|d| (d.revision, d.view_hash, d.dirty))
            == after.as_ref().map(|d| (d.revision, d.view_hash, d.dirty)),
        st(&after),
    );
    // An optional panel closed: Hide is disabled, Show brings it back
    // without recreating the other panels.
    open_click("view").await?;
    real_click_id("mi-panel-inspector").await?;
    ipc::sleep(300).await;
    let hide_state = crate::command::enabled(CommandId::ViewHideInspector);
    open_click("view").await?;
    let unchecked = attr("mi-panel-inspector", "aria-checked");
    real_click_id("mi-panel-inspector").await?;
    ipc::sleep(300).await;
    r.step(
        "with the Inspector closed, Hide Inspector is disabled; Show brings it back with every panel object intact",
        !hide_state
            && unchecked.as_deref() == Some("false")
            && crate::dock::shown_untracked().contains(p::layout::PanelId::Inspector)
            && probes_ok(),
        "",
    );
    Some(())
}

/// World and reserved items: disabled, explained, never run.
async fn world_and_reserved(r: &mut R) -> Option<()> {
    open_click("world").await?;
    let items = all("#menu-world-list [role^=menuitem]");
    let reasons_ok = items.len() == 6
        && items.iter().all(|i| {
            i.get_attribute("aria-disabled").as_deref() == Some("true")
                && i.get_attribute("title")
                    .is_some_and(|t| t.contains(NOT_MIGRATED))
        });
    let groups: Vec<String> = all("#menu-world-list .menu-group")
        .iter()
        .map(|g| g.text_content().unwrap_or_default())
        .collect();
    let n = runs();
    let before = doc_state().await;
    key("Return").await?;
    key("space").await?;
    let first = items.first()?.id();
    real_click_id(&first).await?;
    let after = doc_state().await;
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    r.step(
        "World: Mall Item and World Project groups, every item disabled with \"Not migrated to the Rust application.\"; Enter / Space / click run nothing",
        reasons_ok && groups == ["Mall Item", "World Project"] && runs() == n && before == after,
        format!("{} items {groups:?} runs +{}", items.len(), runs() - n),
    );
    open_click("edit").await?;
    let reserved = [
        CommandId::EditFind,
        CommandId::EditReplace,
        CommandId::EditGoToLine,
    ]
    .iter()
    .all(|c| attr(&dom_id(*c), "aria-disabled").as_deref() == Some("true"));
    real("escape", 0.0, 0.0).await.ok()?;
    real("escape", 0.0, 0.0).await.ok()?;
    open_click("help").await?;
    let help = attr(&dom_id(CommandId::HelpShortcuts), "aria-disabled").as_deref() == Some("true")
        && attr(&dom_id(CommandId::HelpLicenses), "aria-disabled").as_deref() == Some("true")
        && attr(&dom_id(CommandId::HelpAbout), "aria-disabled").as_deref() == Some("false");
    real_click_id(&dom_id(CommandId::HelpAbout)).await?;
    let about = wait_ms(3000, || {
        el("#about-identity")
            .and_then(|e| e.text_content())
            .filter(|t| t == &wrlforge_desktop_protocol::command::app_display_name())
    })
    .await;
    real("escape", 0.0, 0.0).await.ok()?;
    ipc::sleep(200).await;
    r.step(
        "Edit reserves Find / Replace / Go to Line (disabled); Help → About opens the About dialog with the app identity",
        reserved && help && about.is_some(),
        format!("{about:?}"),
    );
    Some(())
}

/// Quit through the menu and Ctrl+Q: CloseGate asks, Cancel keeps all.
async fn quit(r: &mut R) -> Option<()> {
    type_edit("\n# unsaved before quit").await?;
    // The recovery record of the unsaved edit, stable.
    let mut d1 = Vec::new();
    for _ in 0..60 {
        let a = digest().await;
        ipc::sleep(700).await;
        let b = digest().await;
        if !a.is_empty() && a == b {
            d1 = a;
            break;
        }
    }
    let asked0 = disk(FIXTURE).await?.asked;
    let before = doc_state().await;
    choose("file", CommandId::AppQuit).await?;
    let mut asked1 = asked0;
    for _ in 0..40 {
        asked1 = disk(FIXTURE).await?.asked;
        if asked1 > asked0 {
            break;
        }
        ipc::sleep(100).await;
    }
    ipc::sleep(500).await;
    let mid = doc_state().await;
    let d2 = digest().await;
    let _ = textarea()?.focus();
    key("ctrl+q").await?;
    ipc::sleep(1500).await;
    let asked2 = disk(FIXTURE).await?.asked;
    // The macOS application menu's Quit: the same event path.
    let q0 = crate::menu::native_stats();
    native(CommandId::AppQuit.key(), false).await?;
    let mut asked3 = asked2;
    for _ in 0..40 {
        asked3 = disk(FIXTURE).await?.asked;
        if asked3 > asked2 {
            break;
        }
        ipc::sleep(100).await;
    }
    ipc::sleep(500).await;
    let q1 = crate::menu::native_stats();
    let after = doc_state().await;
    let d3 = digest().await;
    r.step(
        "File → Quit and Ctrl+Q with unsaved edits each ask through the CloseGate; Cancel keeps the app running",
        asked1 == asked0 + 1 && asked2 == asked1 + 1,
        format!("asked {asked0} -> {asked1} -> {asked2}"),
    );
    r.step(
        "a native Quit menu event runs Quit through command::run and asks through the CloseGate; Cancel keeps the app",
        asked3 == asked2 + 1 && q1.ran == q0.ran + 1,
        format!("asked {asked2} -> {asked3} {q0:?} -> {q1:?}"),
    );
    r.step(
        "after a canceled Quit the document is unchanged and still Modified",
        before.is_some()
            && before == mid
            && mid == after
            && after.as_ref().is_some_and(|d| d.dirty),
        st(&after),
    );
    r.step(
        "after a canceled Quit the recovery records are intact (byte hash unchanged)",
        !d1.is_empty() && d1 == d2 && d2 == d3,
        format!("{} records", d1.len()),
    );
    // PR #143 review: Ctrl+Q from a focused bar button and from an open menu.
    let (a, b) = select_crate()?;
    let before = doc_state().await;
    let n = runs();
    key("F10").await?;
    key("ctrl+q").await?;
    let mut asked4 = asked3;
    for _ in 0..40 {
        asked4 = disk(FIXTURE).await?.asked;
        if asked4 > asked3 {
            break;
        }
        ipc::sleep(100).await;
    }
    ipc::sleep(500).await;
    key("alt+f").await?;
    let open = list_open("file");
    key("ctrl+q").await?;
    let mut asked5 = asked4;
    for _ in 0..40 {
        asked5 = disk(FIXTURE).await?.asked;
        if asked5 > asked4 {
            break;
        }
        ipc::sleep(100).await;
    }
    ipc::sleep(500).await;
    let after = doc_state().await;
    let d4 = digest().await;
    r.step(
        "Ctrl+Q from a focused bar button and from an open File menu each run Quit once through the CloseGate; Cancel keeps the unsaved document, the selection and the recovery records",
        asked4 == asked3 + 1
            && asked5 == asked4 + 1
            && open
            && runs() == n + 2
            && same_state(&before, &after)
            && after.as_ref().is_some_and(|d| d.dirty)
            && source_selection() == Some((a, b))
            && d4 == d1,
        format!("asked {asked3} -> {asked4} -> {asked5} runs +{} {}", runs() - n, st(&after)),
    );
    Some(())
}
