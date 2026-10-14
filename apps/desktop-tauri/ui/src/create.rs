// SPDX-License-Identifier: GPL-3.0-or-later
//! The toolbar Create control (VISUAL-1): a menu button that lists the
//! primitives. It names a primitive and nothing else; Rust generates and
//! inserts the source (`ui::create`).
//!
//! Keyboard: Enter / Space / ArrowDown on the button opens the menu and
//! focuses the first item; ArrowUp / ArrowDown move, Home / End jump, Enter
//! creates, Escape (or Tab) closes and returns focus to the button.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::command::CommandId;

use crate::ui::ui;

fn item_id(prim: p::Primitive) -> String {
    format!("create-{}", prim.label().to_lowercase())
}

fn focus(id: &str) {
    if let Some(el) = crate::element_by_id::<web_sys::HtmlElement>(id) {
        let _ = el.focus();
    }
}

#[component]
pub fn CreateMenu() -> impl IntoView {
    let u = ui();
    let open = RwSignal::new(false);
    let has_doc = move || u.doc.with(|d| d.is_some());
    let close = move |refocus: bool| {
        open.set(false);
        if refocus {
            focus("btn-create");
        }
    };
    let toggle = move || {
        let now = !open.get_untracked();
        open.set(now);
        if now {
            // After the menu renders.
            spawn_local(async {
                crate::ipc::sleep(0).await;
                focus(&item_id(p::Primitive::ALL[0]));
            });
        }
    };
    let on_menu_key = move |ev: web_sys::KeyboardEvent| {
        let ids: Vec<String> = p::Primitive::ALL.into_iter().map(item_id).collect();
        let cur = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .map(|e| e.id())
            .and_then(|id| ids.iter().position(|i| *i == id))
            .unwrap_or(0);
        let n = ids.len();
        let next = match ev.key().as_str() {
            "ArrowDown" => Some((cur + 1) % n),
            "ArrowUp" => Some((cur + n - 1) % n),
            "Home" => Some(0),
            "End" => Some(n - 1),
            "Escape" => {
                ev.prevent_default();
                close(true);
                None
            }
            "Tab" => {
                close(false);
                None
            }
            _ => None,
        };
        if let Some(i) = next {
            ev.prevent_default();
            focus(&ids[i]);
        }
    };
    view! {
        <div class="create-menu">
            <button id="btn-create" aria-haspopup="menu"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-controls="create-list"
                // D2: no hover tooltip while Create is usable. WebKitGTK keeps
                // a shown tooltip on screen after `title` changes, so one shown
                // before the click covered "Box" in the opened list. The
                // reason is kept while disabled (the list cannot open then).
                title=move || if has_doc() {
                    ""
                } else {
                    "Open a file or start a New World first"
                }
                disabled=move || !has_doc()
                on:click=move |_| toggle()
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "ArrowDown" && !open.get_untracked() {
                        ev.prevent_default();
                        toggle();
                    }
                }>
                "Create ▾"
            </button>
            {move || open.get().then(|| view! {
                <div id="create-list" class="create-list" role="menu" aria-label="Create primitive"
                    on:keydown=on_menu_key>
                    {p::Primitive::ALL.into_iter().map(|prim| view! {
                        // UI-MENU-1: the same registry command as Create → <prim>.
                        <button id=item_id(prim) role="menuitem" class="create-item"
                            data-command=CommandId::create_for(prim).key()
                            title=crate::command::title(CommandId::create_for(prim))
                            on:click=move |_| {
                                close(true);
                                crate::command::run(CommandId::create_for(prim));
                            }>
                            <span class=format!("glyph glyph-{}", prim.label().to_lowercase()) aria-hidden="true"></span>
                            {prim.label()}
                        </button>
                    }).collect_view()}
                    // D4: the explicit origin placement.
                    <label class="create-origin" title="Place new objects at 0 0 0 instead of in front of the camera">
                        <input type="checkbox" id="create-place-origin"
                            prop:checked=move || u.place_origin.get()
                            on:change=move |ev| u.place_origin.set(event_target_checked(&ev))/>
                        " Place at origin"
                    </label>
                </div>
            })}
        </div>
        {move || u.create_error.get().map(|e| view! {
            <span class="create-error" id="create-error" role="alert" title=e.clone()>{e.clone()}</span>
        })}
    }
}
