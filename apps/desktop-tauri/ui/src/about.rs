// SPDX-License-Identifier: GPL-3.0-or-later
//! APP-SHELL-0: Help → About. A non-modal panel (a WebView alert would
//! block the IPC). It shows what Rust reports (`app_info`) and the version
//! this UI was built with; a difference is shown, never hidden.

use std::cell::{Cell, OnceCell, RefCell};
use std::future::Future;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;
use wrlforge_desktop_protocol::command::{self as c, AppInfo};

use crate::ipc::{self, call};

#[derive(Clone, PartialEq)]
pub enum About {
    Loading,
    Ready(AppInfo),
    Failed(String),
}

thread_local! {
    static ABOUT: OnceCell<RwSignal<Option<About>>> = const { OnceCell::new() };
    /// STACK-REVIEW-1: one number per open; an `app_info` answer for an
    /// older open (closed, or closed and opened again) is dropped.
    static OPENED: Cell<u64> = const { Cell::new(0) };
    /// The control that had the focus when the dialog opened.
    static RETURN_TO: RefCell<Option<HtmlElement>> = const { RefCell::new(None) };
}

/// Created once by `start()` before the UI mounts, outside any owner.
pub fn about() -> RwSignal<Option<About>> {
    ABOUT.with(|a| *a.get_or_init(|| RwSignal::new(None)))
}

pub fn show() {
    show_with(call::<AppInfo>("app_info", ipc::NoArgs {}));
}

/// Open the dialog at once (the Close button takes the focus when it
/// mounts, before any IPC answer) and fill it in when `fetch` answers. A
/// late answer never reopens a closed dialog, and never replaces the state
/// of a later open. The smoke run passes a delayed `fetch`.
pub(crate) fn show_with(fetch: impl Future<Output = Result<AppInfo, String>> + 'static) {
    let open = OPENED.with(|o| {
        o.set(o.get() + 1);
        o.get()
    });
    if about().get_untracked().is_none() {
        let active = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element())
            .and_then(|a| a.dyn_into::<HtmlElement>().ok());
        RETURN_TO.with_borrow_mut(|r| *r = active);
    }
    about().set(Some(About::Loading));
    spawn_local(async move {
        let next = match fetch.await {
            Ok(info) => About::Ready(info),
            Err(e) => About::Failed(e),
        };
        if OPENED.with(Cell::get) == open && about().get_untracked().is_some() {
            about().set(Some(next));
        }
    });
}

/// Close the dialog and give the focus back to the control that had it
/// when the dialog opened, if that control is still in the document.
pub fn hide() {
    if about().get_untracked().is_none() {
        return;
    }
    OPENED.with(|o| o.set(o.get() + 1));
    about().set(None);
    if let Some(el) = RETURN_TO
        .with_borrow_mut(Option::take)
        .filter(|e| e.is_connected())
    {
        let _ = el.focus();
    }
}

fn rows(info: &AppInfo) -> Vec<(&'static str, String)> {
    let mut r = vec![
        ("Version", format!("{} ({})", info.version, info.channel)),
        ("Package version", info.config_version.clone()),
        ("Application ID", info.identifier.clone()),
        ("3D renderer", format!("{} (default)", info.renderer)),
        (
            "Shell",
            format!("Tauri {} · Rust/Wasm UI", info.tauri_version),
        ),
        ("License", info.license.clone()),
    ];
    if info.version != c::APP_VERSION || info.config_version != c::APP_VERSION {
        r.push((
            "Warning",
            format!(
                "version mismatch: backend {}, package {}, UI {}",
                info.version,
                info.config_version,
                c::APP_VERSION
            ),
        ));
    }
    r
}

#[component]
pub fn AboutDialog() -> impl IntoView {
    // The dialog frame and its Close button live for the whole open; only
    // the body follows Loading -> Ready / Failed, so the focus stays put.
    let open = Memo::new(move |_| about().with(Option::is_some));
    move || {
        open.get().then(|| {
            let close = NodeRef::<leptos::html::Button>::new();
            Effect::new(move |_| {
                if let Some(b) = close.get() {
                    let _ = b.focus();
                }
            });
            let body = move || match about().get() {
                None | Some(About::Loading) => view! { <p class="muted">"Loading…"</p> }.into_any(),
                Some(About::Failed(e)) => view! { <p class="about-warning">{format!("Could not read the version: {e}")}</p> }.into_any(),
                Some(About::Ready(info)) => {
                    let list = rows(&info)
                        .into_iter()
                        .map(|(k, v)| {
                            let warn = k == "Warning";
                            view! { <dt>{k}</dt><dd class:about-warning=warn>{v}</dd> }
                        })
                        .collect_view();
                    view! {
                        <p id="about-identity" class="about-identity">{info.display.clone()}</p>
                        <dl id="about-facts" class="about-facts">{list}</dl>
                    }
                    .into_any()
                }
            };
            view! {
                <div id="about-dialog" class="about-dialog" role="dialog" aria-labelledby="about-title"
                    on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { ev.prevent_default(); hide(); }>
                    <h3 id="about-title">"About WRL Forge"</h3>
                    {body}
                    <p class="muted">"Standards-first VRML97 model and code editor. This 1.4.x Rust build is a Beta; 1.5.0 is reserved for the complete Rust release on Linux and macOS 15+ only."</p>
                    <div class="about-actions">
                        <button id="about-close" class="secondary" node_ref=close on:click=move |_| hide()>"Close"</button>
                    </div>
                </div>
            }
        })
    }
}
