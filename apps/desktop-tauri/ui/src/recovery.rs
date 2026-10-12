// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-1: the startup recovery choice and the recovery warning.
//!
//! The UI holds no recovery data of its own: Rust lists the records, and
//! Restore / Discard name a record by its opaque id. Nothing is restored
//! without a click; Later simply hides the dialog and keeps every record.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;
use wrlforge_desktop_protocol as p;

use crate::ipc::{self, call, NoArgs};
use crate::ui::{self, flash, ui};

thread_local! {
    static LIST: RwSignal<Option<p::RecoveryList>> = RwSignal::new(None);
    static WARNING: RwSignal<Option<String>> = RwSignal::new(None);
}

pub(crate) fn list() -> RwSignal<Option<p::RecoveryList>> {
    LIST.with(|s| *s)
}

fn warning() -> RwSignal<Option<String>> {
    WARNING.with(|s| *s)
}

/// Create the signals before mounting, outside every reactive owner: a
/// signal first created inside a view closure is disposed when that
/// closure re-runs.
pub fn init() {
    let _ = (list(), warning());
}

#[derive(serde::Serialize)]
struct Id {
    id: String,
}

/// Ask Rust for records left by an earlier WRL Forge that did not exit
/// cleanly; show the dialog only when there is something to decide.
pub async fn check() {
    if let Ok(l) = call::<p::RecoveryList>("recovery_list", NoArgs {}).await {
        warning().set(l.warning.clone());
        let any = !l.entries.is_empty() || !l.damaged.is_empty();
        list().set(any.then_some(l));
    }
}

/// Poll the recovery warning (a failed write can happen at any time).
pub fn start_watch() {
    spawn_local(async {
        loop {
            ipc::sleep(3000).await;
            if let Ok(w) = call::<Option<String>>("recovery_status", NoArgs {}).await {
                if warning().get_untracked() != w {
                    warning().set(w);
                }
            }
        }
    });
}

pub async fn restore(id: String) {
    // Same rule as Open: the shown document's unsaved changes are never
    // replaced silently.
    if ui().dirty.get_untracked() {
        flash("Save or undo your changes before restoring recovered work.");
        return;
    }
    match call::<p::OpenOutcome>("recovery_restore", Id { id }).await {
        Ok(o @ p::OpenOutcome::Opened { .. }) => ui::apply_open(o, "Restored").await,
        Ok(p::OpenOutcome::Failed { message }) => flash(&format!("Not restored: {message}")),
        Ok(p::OpenOutcome::Canceled) => {}
        Err(e) => flash(&format!("Restore failed: {e}")),
    }
    check().await;
}

pub async fn discard(id: String) {
    match call::<p::RecoveryDiscardOutcome>("recovery_discard", Id { id }).await {
        Ok(p::RecoveryDiscardOutcome::Discarded) => flash("Recovered work discarded."),
        Ok(p::RecoveryDiscardOutcome::Canceled) => {
            flash("Discard canceled — the recovered work is kept.")
        }
        Ok(p::RecoveryDiscardOutcome::Failed { message }) => {
            flash(&format!("Not discarded: {message}"))
        }
        Err(e) => flash(&format!("Discard failed: {e}")),
    }
    check().await;
}

pub fn later() {
    list().set(None);
    flash("Recovered work kept. WRL Forge will offer it again at the next start.");
}

/// Local date and time, `YYYY-MM-DD HH:MM` (no locale API: WebKitGTK
/// rejects some locale arguments).
fn when(ms: u64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(ms as f64));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes()
    )
}

fn original_note(now: Option<&str>) -> Option<&'static str> {
    match now {
        Some("changed") => Some("The original file changed after this work was recovered."),
        Some("missing") => Some("The original file is no longer there."),
        _ => None,
    }
}

#[component]
pub fn RecoveryWarning() -> impl IntoView {
    move || {
        warning().get().map(|w| {
            view! {
                <div id="recovery-warning" class="banner" role="alert"><span>{w}</span></div>
            }
        })
    }
}

#[component]
pub fn RecoveryDialog() -> impl IntoView {
    move || {
        list().get().map(|l| {
            let entries = l
                .entries
                .into_iter()
                .map(|e| {
                    let (r, d) = (e.id.clone(), e.id.clone());
                    let kind = if e.untitled { "new world" } else { "file" };
                    let meta = format!(
                        "{kind} · {} · {} B · recovered {}",
                        e.format,
                        e.bytes,
                        when(e.saved_at_ms)
                    );
                    let note = original_note(e.original_now.as_deref());
                    let name_attr = e.name.clone();
                    view! {
                        <li class="recovery-entry" data-id=e.id.clone() data-name=name_attr>
                            <div class="recovery-text">
                                <span class="recovery-name">{e.name}</span>
                                <span class="muted">{meta}</span>
                                {note.map(|n| view! { <span class="recovery-note">{n}</span> })}
                            </div>
                            <button class="recovery-restore"
                                on:click=move |_| spawn_local(restore(r.clone()))>"Restore"</button>
                            <button class="secondary recovery-discard"
                                on:click=move |_| spawn_local(discard(d.clone()))>"Discard…"</button>
                        </li>
                    }
                })
                .collect_view();
            let damaged = l
                .damaged
                .into_iter()
                .map(|x| {
                    let d = x.id.clone();
                    view! {
                        <li class="recovery-entry recovery-damaged" data-id=x.id.clone()>
                            <div class="recovery-text">
                                <span class="recovery-name">"Damaged recovery record"</span>
                                <span class="muted">{x.reason}</span>
                            </div>
                            <button class="secondary recovery-discard"
                                on:click=move |_| spawn_local(discard(d.clone()))>"Discard…"</button>
                        </li>
                    }
                })
                .collect_view();
            view! {
                <div id="recovery-dialog" class="recovery-dialog" role="dialog" aria-labelledby="recovery-title">
                    <h3 id="recovery-title">"Recover unsaved work"</h3>
                    <p class="muted">"WRL Forge closed before this work was saved. Restore opens it as a new, unsaved document; the original file is not changed. Use Save As to keep it."</p>
                    <ul class="recovery-list">{entries}{damaged}</ul>
                    <div class="recovery-actions">
                        <button id="recovery-later" class="secondary" on:click=move |_| later()>"Later"</button>
                    </div>
                </div>
            }
        })
    }
}
