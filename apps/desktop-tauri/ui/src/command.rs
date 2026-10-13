// SPDX-License-Identifier: GPL-3.0-or-later
//! APP-SHELL-0: the one command path for the toolbar and the keyboard.
//!
//! The registry (`wrlforge_desktop_protocol::command`) says what each
//! command is; this module says how the UI runs it. Every action calls the
//! same function the toolbar called before the registry, so Rust still
//! makes every document change, check and refusal.

use std::cell::Cell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wrlforge_desktop_protocol::command::{
    self as c, Availability, CommandContext, CommandId, FocusKind,
};
use wrlforge_desktop_protocol::gizmo::Tool;
use wrlforge_desktop_protocol as p;

use crate::ui::{self, ui};
use crate::{about, clipboard, dock, editor, extrusion, findbar, gizmo, menu, panels, theme};

/// The current state, read reactively (button `disabled` follows it). The
/// focus facts are those of the control that had the focus before a menu
/// took it (`clipboard::focus_snapshot`).
pub fn context() -> CommandContext {
    let u = ui();
    // Tracked so a menu or button re-reads the DOM facts on these changes.
    let _ = (
        u.revision.get(),
        u.inspection.with(|_| ()),
        menu::focus_tick(),
    );
    let (focus, text_selected) = clipboard::focus_snapshot();
    CommandContext {
        has_document: u.doc.with(|d| d.is_some()),
        can_undo: u.can_undo.get(),
        can_redo: u.can_redo.get(),
        shown: dock::shown(),
        focus,
        text_selected,
        native_viewport: u.native.with(|s| s.native_shown()),
        outline_ready: outline_button().is_some(),
        node_selected: u.selected.with(|s| s.as_ref().is_some_and(|s| s.id.starts_with("node-"))),
        interact: u.interact_mode.get(),
    }
}

fn context_untracked() -> CommandContext {
    let u = ui();
    let (focus, text_selected) = clipboard::focus_snapshot();
    CommandContext {
        has_document: u.doc.with_untracked(|d| d.is_some()),
        can_undo: u.can_undo.get_untracked(),
        can_redo: u.can_redo.get_untracked(),
        shown: dock::shown_untracked(),
        focus,
        text_selected,
        native_viewport: u.native.with_untracked(|s| s.native_shown()),
        outline_ready: outline_button().is_some(),
        node_selected: u
            .selected
            .with_untracked(|s| s.as_ref().is_some_and(|s| s.id.starts_with("node-"))),
        interact: u.interact_mode.get_untracked(),
    }
}

/// The state now, not tracked (a menu activation or a key press).
pub fn context_now() -> CommandContext {
    context_untracked()
}

/// The Inspector's enabled "Edit outline…" button: the one place that holds
/// the Extrusion outline the editor opens with.
fn outline_button() -> Option<web_sys::HtmlElement> {
    let b = crate::element_by_id::<web_sys::HtmlElement>("btn-outline")?;
    (!b.has_attribute("disabled")).then_some(b)
}

/// The checked state of a menu item, from the existing UI signals.
pub fn checked(id: CommandId) -> Option<bool> {
    let u = ui();
    match id.spec().check {
        c::Check::None => None,
        c::Check::Pick => Some(u.pick_mode.get()),
        c::Check::Gizmo(t) => Some(match t {
            Tool::Move => u.move_mode.get(),
            Tool::Rotate => u.rotate_mode.get(),
            Tool::Scale => u.scale_mode.get(),
        }),
        c::Check::Theme(t) => Some(theme::current() == t),
        c::Check::LivePreview => Some(u.preview_enabled.get()),
        c::Check::Interact => Some(u.interact_mode.get()),
    }
}

thread_local! {
    /// Smoke: commands run (each key press must run at most one).
    static RUNS: Cell<u64> = const { Cell::new(0) };
}

/// Smoke: how many commands `run` has started (refusals included).
pub fn run_count() -> u64 {
    RUNS.with(|r| r.get())
}

/// Reactive: true when the toolbar / menu item is enabled.
pub fn enabled(id: CommandId) -> bool {
    id.availability(context()) == Availability::Enabled
}

pub fn platform() -> c::Platform {
    let mac = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Mac"));
    if mac {
        c::Platform::MacOs
    } else {
        c::Platform::Linux
    }
}

/// Tooltip with the platform's shortcut label.
pub fn title(id: CommandId) -> String {
    id.title(platform())
}

/// Run a command. A command that needs a document says so instead of
/// doing nothing; history refusals come from Rust (`Nothing to undo`).
pub fn run(id: CommandId) {
    RUNS.with(|r| r.set(r.get() + 1));
    if let Some(why) = id.refusal(context_untracked()) {
        ui::flash(&format!("{} is not available: {why}.", id.spec().label));
        return;
    }
    match id {
        CommandId::FileNewWorld => ui::new_world(),
        CommandId::FileOpen => ui::open(),
        CommandId::FileSave => ui::save(false),
        CommandId::FileSaveAs => ui::save(true),
        CommandId::FileClose => ui::close(),
        CommandId::FileReload => ui::reload(),
        CommandId::EditUndo => spawn_local(editor::history(true)),
        CommandId::EditRedo => spawn_local(editor::history(false)),
        CommandId::HelpAbout => about::show(),
        // UI-DOCK-1: layout only; the document is never reached.
        CommandId::ViewShowSceneTree
        | CommandId::ViewShowInspector
        | CommandId::ViewShowDiagnostics => dock::show(id.panel().expect("panel command")),
        CommandId::ViewHideSceneTree
        | CommandId::ViewHideInspector
        | CommandId::ViewHideDiagnostics => dock::hide(id.panel().expect("panel command")),
        CommandId::ViewFocusViewport | CommandId::ViewFocusSource => {
            dock::focus(id.panel().expect("panel command"))
        }
        CommandId::ViewPresetModeling | CommandId::ViewPresetSource => {
            dock::set_preset(id.preset().expect("preset command"))
        }
        CommandId::ViewResetLayout => dock::reset(),
        // UI-MENU-1. Each arm calls the function the toolbar already used.
        CommandId::AppQuit => spawn_local(async {
            // The close goes through the window's CloseGate (unsaved work
            // is asked about; Cancel keeps everything).
            if let Err(e) = crate::ipc::call::<()>("app_quit", crate::ipc::NoArgs {}).await {
                ui::flash(&format!("Quit failed: {e}"));
            }
        }),
        CommandId::EditCut => spawn_local(clipboard::cut()),
        CommandId::EditCopy => spawn_local(clipboard::copy()),
        CommandId::EditPaste => spawn_local(clipboard::paste()),
        CommandId::EditSelectAll => clipboard::select_all(),
        CommandId::ViewThemeTokyoNight
        | CommandId::ViewThemeTokyoNightStorm
        | CommandId::ViewThemeTokyoNightLight => {
            theme::select(id.theme().expect("theme command").to_string())
        }
        CommandId::ViewLivePreview => ui().preview_enabled.update(|v| *v = !*v),
        CommandId::ViewUpdatePreview => spawn_local(ui::preview(true)),
        // B2-CAMERA: the renderer's camera only (no source, dirty or undo).
        CommandId::ViewFitAll => crate::camera::fit(),
        CommandId::ViewFrameSelection => crate::camera::frame_selection(),
        CommandId::EditClearSelection => {
            crate::ui::clear_selection();
        }
        CommandId::ViewFront
        | CommandId::ViewBack
        | CommandId::ViewLeft
        | CommandId::ViewRight
        | CommandId::ViewTop
        | CommandId::ViewBottom => {
            crate::camera::standard(id.standard_view().expect("standard view command"))
        }
        CommandId::ViewResetCamera => crate::camera::reset(),
        CommandId::CreateBox
        | CommandId::CreateSphere
        | CommandId::CreateCylinder
        | CommandId::CreateCone
        | CommandId::CreateExtrusion => {
            spawn_local(ui::create(id.primitive().expect("create command")))
        }
        CommandId::ToolSelect => panels::toggle_select(),
        CommandId::ToolMove => panels::toggle_gizmo(Tool::Move),
        CommandId::ToolRotate => panels::toggle_gizmo(Tool::Rotate),
        CommandId::ToolScale => panels::toggle_gizmo(Tool::Scale),
        CommandId::ToolInteract => panels::toggle_interact(),
        // The Inspector's own button holds the outline data: run its handler.
        CommandId::ToolExtrusionOutline => {
            if let Some(b) = outline_button() {
                b.click();
            }
        }
        // B2: Rust plans, proves and applies; one undo step.
        CommandId::EditDeleteNode => spawn_local(ui::structure_edit(
            p::StructureOp::Delete,
        )),
        CommandId::EditDuplicateNode => spawn_local(ui::structure_edit(
            p::StructureOp::Duplicate,
        )),
        // B2: the Source panel's bar (UI only; replacements go through
        // the editor's doc_edit path).
        CommandId::EditFind => findbar::open(findbar::Mode::Find),
        CommandId::EditReplace => findbar::open(findbar::Mode::Replace),
        CommandId::EditGoToLine => findbar::open(findbar::Mode::GoTo),
        // Planned: refused above, never reached.
        CommandId::ViewZoomIn
        | CommandId::ViewZoomOut
        | CommandId::ViewZoomReset
        | CommandId::WorldMallValidate
        | CommandId::WorldMallRepack
        | CommandId::WorldProjectOpen
        | CommandId::WorldProjectScan
        | CommandId::WorldProjectPreview
        | CommandId::WorldProjectPackage
        | CommandId::HelpShortcuts
        | CommandId::HelpLicenses => {}
    }
}

/// The document-level commands a shortcut may run while a text field other
/// than Source has the focus. Undo, Redo and the clipboard keys stay the
/// field's own there.
const FIELD_SAFE: &[CommandId] = &[
    CommandId::FileNewWorld,
    CommandId::FileOpen,
    CommandId::FileSave,
    CommandId::FileSaveAs,
    CommandId::AppQuit,
    // B2: they open the Source bar; the field's text is untouched.
    CommandId::EditFind,
    CommandId::EditReplace,
    CommandId::EditGoToLine,
];

/// UI-MENU-1: why a key press must not run an application shortcut.
fn shortcut_blocked(ev: &web_sys::KeyboardEvent) -> Option<&'static str> {
    if ev.default_prevented() {
        return Some("handled");
    }
    // IME composition: the key belongs to the input method.
    if ev.is_composing() || ev.key_code() == 229 || ev.key() == "Process" {
        return Some("composing");
    }
    // AltGr arrives as Ctrl+Alt on Windows (and Linux layouts): it types
    // text, it is never a primary-modifier shortcut.
    if ev.alt_key() || ev.get_modifier_state("AltGraph") {
        return Some("alt");
    }
    modal_or_drag()
}

/// A dialog (About, the recovery choice, the outline editor) owns its keys,
/// and a drag owns the pointer: no application shortcut and no menu access.
pub fn modal_or_drag() -> Option<&'static str> {
    if active_element()
        .and_then(|e| e.closest("[role=dialog],[role=alertdialog]").ok().flatten())
        .is_some()
    {
        return Some("dialog");
    }
    if gizmo::dragging() || dock::dragging() || extrusion::dragging() {
        return Some("drag");
    }
    None
}

/// B2-CAMERA: the focus is inside the 3D Viewport pane (its canvas or a
/// toolbar button), not in a text field, list or check box there.
fn viewport_focused() -> bool {
    let Some(el) = active_element() else {
        return false;
    };
    let tag = el.tag_name().to_ascii_uppercase();
    if matches!(tag.as_str(), "INPUT" | "SELECT" | "TEXTAREA") {
        return false;
    }
    el.closest("section.viewport").ok().flatten().is_some()
}

fn active_element() -> Option<web_sys::Element> {
    web_sys::window()?.document()?.active_element()
}

/// UI-MENU-1: the ONE application shortcut handler (window, bubble phase).
/// It reads the registry's shortcut table only.
///
/// With the focus in the menu bar (a bar button or an open menu) the menu
/// lets the shortcut through. The command acts on the control the menu took
/// the focus from, so that control decides the text-field rule; the menu
/// closes and gives the focus back before the command runs.
pub fn on_window_keydown(ev: web_sys::KeyboardEvent) {
    if shortcut_blocked(&ev).is_some() {
        return;
    }
    let primary = ev.ctrl_key() || ev.meta_key();
    let Some(id) = c::from_shortcut(&ev.key(), primary, ev.shift_key()) else {
        // B2-CAMERA: an unmodified viewport key (F / Home) while the 3D
        // Viewport has the focus. Never in the menu bar or a form control.
        if let Some(id) = c::from_viewport_key(&ev.key(), primary || ev.shift_key()) {
            if !menu::bar_has_focus() && !menu::is_open() && viewport_focused() {
                ev.prevent_default();
                // Escape with nothing selected has nothing to clear: no
                // "not available" notice, no command run.
                if id == CommandId::EditClearSelection && id.refusal(context_untracked()).is_some() {
                    return;
                }
                run(id);
            }
        }
        return;
    };
    let in_menu = menu::bar_has_focus() || menu::is_open();
    let target = if in_menu {
        clipboard::last().map(Into::into)
    } else {
        active_element()
    };
    let kind = clipboard::focus_kind(target);
    let field = kind == FocusKind::TextField;
    // B2: node structure commands never fire from a text control: Ctrl+D
    // while typing in Source must not duplicate the selected node.
    if kind != FocusKind::Other
        && matches!(id, CommandId::EditDuplicateNode | CommandId::EditDeleteNode)
    {
        return;
    }
    if field && !FIELD_SAFE.contains(&id) {
        return;
    }
    ev.prevent_default();
    if in_menu {
        menu::close(true);
    }
    run(id);
}

/// A toolbar button bound to a registry command.
#[component]
pub fn CommandButton(
    command: CommandId,
    #[prop(optional)] secondary: bool,
    #[prop(optional, into)] dom_id: Option<String>,
    /// Button text when it differs from the menu label.
    #[prop(optional, into)]
    text: Option<String>,
) -> impl IntoView {
    let text = text.unwrap_or_else(|| command.spec().label.to_string());
    view! {
        <button id=dom_id class:secondary=secondary data-command=command.key()
            data-tip=title(command)
            disabled=move || !enabled(command)
            on:click=move |_| run(command)>{text}</button>
    }
}
