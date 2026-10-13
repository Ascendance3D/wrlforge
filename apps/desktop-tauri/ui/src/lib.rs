// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge desktop UI — Leptos (client-side rendering) compiled to Wasm.
//!
//! The UI holds NO document authority: it renders projections the Rust
//! backend computes and sends view edits back. See `editor.rs`.

mod about;
mod camera;
mod clipboard;
mod command;
mod create;
mod dock;
mod editor;
mod extrusion;
mod find;
mod findbar;
mod gizmo;
mod highlight;
mod ipc;
mod menu;
mod native;
mod panels;
mod pick;
mod recovery;
mod smoke;
mod syntax;
mod theme;
mod ui;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;

use crate::command::CommandButton;
use crate::ui::ui;
use wrlforge_desktop_protocol::command::{self as cmd, CommandId};

#[wasm_bindgen(start)]
pub fn start() {
    recovery::init();
    // Before mount: the About state must not belong to a render effect
    // (an effect re-run would dispose a signal it created).
    about::about();
    // UI-DOCK-1: the layout state, before any render effect exists.
    dock::init();
    // UI-MENU-1: the menu state, before mount for the same reason.
    menu::init();
    // B2: the Find / Go to Line bar state, before mount likewise.
    findbar::init();
    leptos::mount::mount_to_body(App);
    dock::install();
    // UI-MENU-1: the one window-level shortcut handler and menu access.
    menu::install();
    pick::install();
    gizmo::install();
    highlight::install();
    ui::start_external_watch();
    syntax::install();
    // The UI stays hidden (style.css) until `<html data-theme>` is set, so a
    // persisted Storm/Light choice never flashes Tokyo Night widgets first.
    theme::reveal_fallback();
    spawn_local(async {
        // The saved layout first, so the revealed UI is already in place.
        dock::load().await;
        theme::init().await;
        native::install().await;
        // UI-MENU-1: the macOS application menu's About / Quit (once).
        menu::listen_native().await;
        if let Ok(Some(o)) =
            ipc::call::<Option<p::OpenOutcome>>("startup_document", ipc::NoArgs {}).await
        {
            // B2-CAMERA: a file named at startup is framed once, like Open.
            camera::arm_auto_fit();
            ui::apply_open(o, "Opened").await;
        }
        // RUST-STAB-1: offer work left by a WRL Forge that did not exit
        // cleanly. Nothing is restored without the user's choice.
        recovery::check().await;
        recovery::start_watch();
        if let Ok(Some(plan)) =
            ipc::call::<Option<p::SmokePlan>>("smoke_plan", ipc::NoArgs {}).await
        {
            // D4: the GUI suites were written for origin placement.
            ui::ui().place_origin.set(true);
            smoke::run(plan).await;
        } else if let Ok(Some(plan)) =
            ipc::call::<Option<smoke::RecoveryPlan>>("smoke_recovery_plan", ipc::NoArgs {}).await
        {
            // D4: the GUI suites were written for origin placement.
            ui::ui().place_origin.set(true);
            smoke::run_recovery(plan).await;
        } else if let Ok(Some(plan)) =
            ipc::call::<Option<smoke::DockPlan>>("smoke_dock_plan", ipc::NoArgs {}).await
        {
            // D4: the GUI suites were written for origin placement.
            ui::ui().place_origin.set(true);
            smoke::run_dock(plan).await;
        } else if let Ok(Some(plan)) =
            ipc::call::<Option<smoke::MenuPlan>>("smoke_menu_plan", ipc::NoArgs {}).await
        {
            // D4: the GUI suites were written for origin placement.
            ui::ui().place_origin.set(true);
            smoke::run_menu(plan).await;
        }
    });
}

/// UI-DOCK-1: the Source panel. It holds the ONLY editing textarea; the dock
/// mounts it once and never moves it (see `dock.rs`).
#[component]
pub(crate) fn SourcePanel() -> impl IntoView {
    let u = ui();
    let has_doc = move || u.doc.with(|d| d.is_some());
    view! {
        <section class="editor-col" aria-label="Source editor">
            <div class="pane-title">
                "Source"
                <span class="muted">{move || u.doc.with(|d| d.as_ref().map(|d| format!(" — {}", d.display_path)).unwrap_or_default())}</span>
            </div>
            // B2: Find / Replace / Go to Line (hidden while closed).
            <findbar::FindBar/>
            <div class="source-row">
            // B2: line numbers. Only the lines near the viewport are
            // written (syntax.rs); the gutter follows the scroll.
            <div class="source-gutter" aria-hidden="true">
                <pre id="source-gutter" class="source-gutter-nums source-metrics"></pre>
            </div>
            // The textarea is the ONLY editing control (its glyphs are
            // transparent); the aria-hidden layer behind it paints the
            // same text in syntax colors and takes no input.
            <div class="source-wrap">
                <pre id="source-hl" class="source-hl source-metrics" aria-hidden="true" data-state="pending"></pre>
                <textarea id="source" class="source source-metrics" spellcheck="false" wrap="off"
                    autocomplete="off" autocapitalize="off" aria-label="VRML source"
                    placeholder="Start a New World (Ctrl+N) or open a .wrl / .wrz file (Ctrl+O). Plain and gzip VRML97 are supported."
                    readonly=move || !has_doc()
                    on:input=move |_| editor::on_input()
                    on:beforeinput=on_beforeinput
                    on:keyup=move |_| editor::update_cursor()
                    on:click=move |_| editor::update_cursor()
                    on:select=move |_| editor::update_cursor()
                    on:scroll=move |_| syntax::on_scroll()
                ></textarea>
            </div>
            </div>
        </section>
    }
}

/// Native textarea undo would edit the widget behind the document's back:
/// it runs the registry's Undo / Redo instead.
fn on_beforeinput(ev: web_sys::InputEvent) {
    let id = match ev.input_type().as_str() {
        "historyUndo" => CommandId::EditUndo,
        "historyRedo" => CommandId::EditRedo,
        _ => return,
    };
    ev.prevent_default();
    command::run(id);
}

#[component]
fn App() -> impl IntoView {
    let u = ui();
    // APP-SHELL-0: every File / Edit button runs through the registry.
    view! {
        <div class="app">
            // UI-MENU-1: File | Edit | View | Create | Tools | World | Help.
            <menu::MenuBar/>
            <header class="toolbar" role="toolbar" aria-label="File and edit commands">
                <span class="brand" id="app-identity">{cmd::app_display_name()}</span>
                <CommandButton command=CommandId::FileNewWorld dom_id="btn-new"/>
                <CommandButton command=CommandId::FileOpen dom_id="btn-open"/>
                <CommandButton command=CommandId::FileSave dom_id="btn-save"/>
                <CommandButton command=CommandId::FileSaveAs dom_id="btn-save-as" secondary=true/>
                <CommandButton command=CommandId::FileClose dom_id="btn-close" secondary=true/>
                <span class="sep"></span>
                <CommandButton command=CommandId::EditUndo dom_id="btn-undo" secondary=true/>
                <CommandButton command=CommandId::EditRedo dom_id="btn-redo" secondary=true/>
                <span class="sep"></span>
                <create::CreateMenu/>
                <span class="sep"></span>
                <dock::LayoutMenu/>
                <span class="spacer"></span>
                <theme::ThemePicker/>
                <span class="sep"></span>
                <span class="profile" title="Profiles are not yet migrated to Rust">
                    "Profile: Generic VRML97 · Mall / World profiles not migrated"
                </span>
                <span class="sep"></span>
                <CommandButton command=CommandId::HelpAbout dom_id="btn-about" secondary=true text="About"/>
            </header>
            {move || u.conflict.get().map(|reason| view! {
                <div class="banner" role="alert">
                    <span>{format!("The file changed on disk ({reason}). Your buffer was NOT written.")}</span>
                    <CommandButton command=CommandId::FileReload dom_id="btn-reload" text="Reload from disk (discard my edits)"/>
                    <CommandButton command=CommandId::FileSaveAs dom_id="btn-conflict-save-as" secondary=true text="Save mine As…"/>
                    <button class="secondary" on:click=move |_| u.conflict.set(None)>"Dismiss"</button>
                </div>
            })}
            <recovery::RecoveryWarning/>
            // UI-DOCK-1: the dockable workspace (five permanent panels).
            <dock::Workspace/>
            // VISUAL-4A: the non-modal outline editor (hidden until opened).
            <extrusion::OutlineEditor/>
            // RUST-STAB-1: the startup recovery choice (hidden when empty).
            <recovery::RecoveryDialog/>
            // APP-SHELL-0: Help → About (hidden until opened).
            <about::AboutDialog/>
            <footer class="statusbar" role="status">
                <span>{move || u.doc.with(|d| d.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "No document".into()))}</span>
                <span>{move || if u.dirty.get() { "● Modified" } else { "Saved" }}</span>
                <span>{move || u.doc.with(|d| d.as_ref().map(|d| format!("{} · {}{}{}", d.format, d.eol,
                    if d.eol_mixed { " (mixed)" } else { "" }, if d.bom { " · BOM" } else { "" })).unwrap_or_default())}</span>
                <span>{move || { let (l, c, s) = u.cursor.get();
                    if s > 0 { format!("Ln {l}, Col {c} ({s} sel)") } else { format!("Ln {l}, Col {c}") } }}</span>
                <span>{move || format!("rev {}", u.revision.get())}</span>
                <span class="msg">{move || u.message.get()}</span>
                <span class="spacer"></span>
                <span class="muted">"Rust core · Tauri 2 · no Electron"</span>
            </footer>
        </div>
    }
}

pub(crate) fn element_by_id<T: JsCast>(id: &str) -> Option<T> {
    web_sys::window()?
        .document()?
        .get_element_by_id(id)?
        .dyn_into()
        .ok()
}
