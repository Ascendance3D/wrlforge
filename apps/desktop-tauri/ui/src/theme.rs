// SPDX-License-Identifier: GPL-3.0-or-later
//! Theme selection (UI-THEME-1). An APPLICATION preference: switching sets
//! `<html data-theme>` and asks Rust to persist the id. It never touches the
//! document, its revision, history, selection, the Inspector or the preview.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wrlforge_desktop_protocol::theme::{self, ThemeSetOutcome, ThemeState, THEMES};

use crate::ipc::{call, NoArgs};

#[derive(Clone, Copy)]
pub struct Theme {
    pub id: RwSignal<String>,
    /// A visible, non-fatal problem: unreadable settings or a failed save.
    pub error: RwSignal<Option<String>>,
}

thread_local! {
    static THEME: OnceCell<Theme> = const { OnceCell::new() };
}

pub fn theme() -> Theme {
    THEME.with(|t| {
        *t.get_or_init(|| Theme {
            id: RwSignal::new(theme::DEFAULT_THEME.into()),
            error: RwSignal::new(None),
        })
    })
}

/// Set the attribute the stylesheet keys on. With no attribute the CSS
/// fallback is already Tokyo Night, so a failure here is never unstyled.
fn apply(id: &str) {
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-theme", id);
    }
}

pub fn current_attribute() -> Option<String> {
    web_sys::window()?
        .document()?
        .document_element()?
        .get_attribute("data-theme")
}

/// If the backend never answers, show the UI in the default theme anyway.
pub fn reveal_fallback() {
    spawn_local(async {
        crate::ipc::sleep(1500).await;
        if current_attribute().is_none() {
            apply(theme::DEFAULT_THEME);
        }
    });
}

/// Startup: adopt the persisted theme. Until this sets `data-theme` the body
/// is hidden, so no widget paints in the wrong theme.
pub async fn init() {
    let t = theme();
    match call::<ThemeState>("theme_get", NoArgs {}).await {
        Ok(st) if theme::is_known(&st.theme_id) => {
            apply(&st.theme_id);
            t.id.set(st.theme_id);
            t.error.set(st.notice);
        }
        Ok(st) => {
            apply(theme::DEFAULT_THEME);
            t.error.set(Some(format!(
                "Unknown theme {:?}; using Tokyo Night.",
                st.theme_id
            )));
        }
        Err(e) => {
            apply(theme::DEFAULT_THEME);
            t.error.set(Some(format!(
                "Theme settings unavailable ({e}); using Tokyo Night."
            )));
        }
    }
}

/// Apply `id` synchronously (same event-loop turn as the user's choice),
/// then persist it through Rust in the background.
pub fn select(id: String) {
    let t = theme();
    if !theme::is_known(&id) {
        t.error.set(Some(format!("Unknown theme {id:?}.")));
        return;
    }
    let previous = t.id.get_untracked();
    apply(&id);
    t.id.set(id.clone());
    spawn_local(persist(id, previous));
}

async fn persist(id: String, previous: String) {
    let t = theme();
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        theme_id: String,
    }
    match call::<ThemeSetOutcome>("theme_set", Args { theme_id: id }).await {
        Ok(ThemeSetOutcome::Saved { .. }) => t.error.set(None),
        Ok(ThemeSetOutcome::NotSaved { message, .. }) => t.error.set(Some(message)),
        Ok(ThemeSetOutcome::Rejected { message }) => {
            apply(&previous);
            t.id.set(previous);
            t.error.set(Some(message));
        }
        Err(e) => t.error.set(Some(format!(
            "Theme applied for this session but NOT saved: {e}"
        ))),
    }
}

/// Compact toolbar control: a labelled native `<select>`, so keyboard use
/// (Tab, arrows, type-ahead, Enter/Space) is the platform's own.
#[component]
pub fn ThemePicker() -> impl IntoView {
    let t = theme();
    view! {
        <label class="theme-picker" for="theme-select">
            "Theme"
            <select id="theme-select" title="Application colour theme (saved for next launch)"
                prop:value=move || t.id.get()
                on:change=move |ev| select(event_target_value(&ev))>
                {THEMES.iter().map(|d| view! {
                    <option value=d.id selected=move || t.id.get() == d.id>{d.label}</option>
                }).collect_view()}
            </select>
        </label>
        {move || t.error.get().map(|e| view! {
            <span class="theme-error" id="theme-error" role="alert" title=e.clone()>{e.clone()}</span>
        })}
    }
}
