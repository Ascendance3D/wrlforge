// SPDX-License-Identifier: GPL-3.0-or-later
//! APP-SHELL-0: Help → About. A non-modal panel (a WebView alert would
//! block the IPC). It shows what Rust reports (`app_info`) and the version
//! this UI was built with; a difference is shown, never hidden.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
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
}

/// Created once by `start()` before the UI mounts, outside any owner.
pub fn about() -> RwSignal<Option<About>> {
    ABOUT.with(|a| *a.get_or_init(|| RwSignal::new(None)))
}

pub fn show() {
    about().set(Some(About::Loading));
    spawn_local(async {
        let next = match call::<AppInfo>("app_info", ipc::NoArgs {}).await {
            Ok(info) => About::Ready(info),
            Err(e) => About::Failed(e),
        };
        // Closed while loading: stay closed.
        if about().get_untracked().is_some() {
            about().set(Some(next));
        }
        if let Some(b) = crate::element_by_id::<web_sys::HtmlElement>("about-close") {
            let _ = b.focus();
        }
    });
}

pub fn hide() {
    about().set(None);
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
    move || {
        about().get().map(|state| {
            let body = match state {
                About::Loading => view! { <p class="muted">"Loading…"</p> }.into_any(),
                About::Failed(e) => view! { <p class="about-warning">{format!("Could not read the version: {e}")}</p> }.into_any(),
                About::Ready(info) => {
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
                    <p class="muted">"Standards-first VRML97 model and code editor. This 1.4.x Rust build is a Beta; 1.5.0 is reserved for the complete Rust release on Linux, Windows and macOS."</p>
                    <div class="about-actions">
                        <button id="about-close" class="secondary" on:click=move |_| hide()>"Close"</button>
                    </div>
                </div>
            }
        })
    }
}
