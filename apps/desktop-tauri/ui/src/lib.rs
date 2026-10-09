// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge desktop UI — Leptos (client-side rendering) compiled to Wasm.
//!
//! The UI holds NO document authority: it renders projections the Rust
//! backend computes and sends view edits back. See `editor.rs`.

mod editor;
mod ipc;
mod panels;
mod smoke;
mod syntax;
mod theme;
mod ui;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;

use crate::ui::ui;

#[wasm_bindgen(start)]
pub fn start() {
    leptos::mount::mount_to_body(App);
    ui::start_external_watch();
    syntax::install();
    // The UI stays hidden (style.css) until `<html data-theme>` is set, so a
    // persisted Storm/Light choice never flashes Tokyo Night widgets first.
    theme::reveal_fallback();
    spawn_local(async {
        theme::init().await;
        if let Ok(Some(o)) =
            ipc::call::<Option<p::OpenOutcome>>("startup_document", ipc::NoArgs {}).await
        {
            ui::apply_open(o, "Opened").await;
        }
        if let Ok(Some(plan)) =
            ipc::call::<Option<p::SmokePlan>>("smoke_plan", ipc::NoArgs {}).await
        {
            smoke::run(plan).await;
        }
    });
}

fn on_keydown(ev: web_sys::KeyboardEvent) {
    if !(ev.ctrl_key() || ev.meta_key()) {
        return;
    }
    match ev.key().to_lowercase().as_str() {
        "z" => {
            ev.prevent_default();
            spawn_local(editor::history(!ev.shift_key()));
        }
        "y" => {
            ev.prevent_default();
            spawn_local(editor::history(false));
        }
        "s" => {
            ev.prevent_default();
            ui::save(ev.shift_key());
        }
        "o" => {
            ev.prevent_default();
            ui::open();
        }
        _ => {}
    }
}

/// Native textarea undo would edit the widget behind the document's back.
fn on_beforeinput(ev: web_sys::InputEvent) {
    match ev.input_type().as_str() {
        "historyUndo" => {
            ev.prevent_default();
            spawn_local(editor::history(true));
        }
        "historyRedo" => {
            ev.prevent_default();
            spawn_local(editor::history(false));
        }
        _ => {}
    }
}

#[component]
fn App() -> impl IntoView {
    let u = ui();
    let has_doc = move || u.doc.with(|d| d.is_some());
    view! {
        <div class="app">
            <header class="toolbar" role="toolbar" aria-label="File and edit commands">
                <span class="brand">"WRL Forge"</span>
                <button id="btn-open" on:click=move |_| ui::open()>"Open…"</button>
                <button id="btn-save" disabled=move || !has_doc() on:click=move |_| ui::save(false)>"Save"</button>
                <button id="btn-save-as" class="secondary" disabled=move || !has_doc() on:click=move |_| ui::save(true)>"Save As…"</button>
                <span class="sep"></span>
                <button id="btn-undo" class="secondary" disabled=move || !u.can_undo.get()
                    on:click=move |_| spawn_local(editor::history(true))>"Undo"</button>
                <button id="btn-redo" class="secondary" disabled=move || !u.can_redo.get()
                    on:click=move |_| spawn_local(editor::history(false))>"Redo"</button>
                <span class="spacer"></span>
                <theme::ThemePicker/>
                <span class="sep"></span>
                <span class="profile" title="Profiles are not yet migrated to Rust">
                    "Profile: Generic VRML97 · Mall / World profiles not migrated"
                </span>
            </header>
            {move || u.conflict.get().map(|reason| view! {
                <div class="banner" role="alert">
                    <span>{format!("The file changed on disk ({reason}). Your buffer was NOT written.")}</span>
                    <button on:click=move |_| ui::reload()>"Reload from disk (discard my edits)"</button>
                    <button class="secondary" on:click=move |_| ui::save(true)>"Save mine As…"</button>
                    <button class="secondary" on:click=move |_| u.conflict.set(None)>"Dismiss"</button>
                </div>
            })}
            <main class="workspace">
                <panels::SceneTree/>
                <section class="editor-col" aria-label="Source editor">
                    <div class="pane-title">
                        "Source"
                        <span class="muted">{move || u.doc.with(|d| d.as_ref().map(|d| format!(" — {}", d.display_path)).unwrap_or_default())}</span>
                    </div>
                    // The textarea is the ONLY editing control (its glyphs are
                    // transparent); the aria-hidden layer behind it paints the
                    // same text in syntax colours and takes no input.
                    <div class="source-wrap">
                        <pre id="source-hl" class="source-hl source-metrics" aria-hidden="true" data-state="pending"></pre>
                        <textarea id="source" class="source source-metrics" spellcheck="false" wrap="off"
                            autocomplete="off" autocapitalize="off" aria-label="VRML source"
                            placeholder="Open a .wrl / .wrz file (Ctrl+O). Plain and gzip VRML97 are supported."
                            readonly=move || !has_doc()
                            on:input=move |_| editor::on_input()
                            on:beforeinput=on_beforeinput
                            on:keydown=on_keydown
                            on:keyup=move |_| editor::update_cursor()
                            on:click=move |_| editor::update_cursor()
                            on:select=move |_| editor::update_cursor()
                            on:scroll=move |_| syntax::on_scroll()
                        ></textarea>
                    </div>
                    <panels::Diagnostics/>
                </section>
                <section class="right-col">
                    <panels::Viewport/>
                    <panels::Inspector/>
                </section>
            </main>
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
