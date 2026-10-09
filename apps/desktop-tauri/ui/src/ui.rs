// SPDX-License-Identifier: GPL-3.0-or-later
//! Reactive display state and the document-level actions (open, save, reload,
//! analysis, preview). Display only: every value here is a copy of something
//! the Rust backend reported.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wrlforge_desktop_protocol as p;

use crate::editor::{self, CORE};
use crate::ipc::{self, call, NoArgs, Session};

#[derive(Clone, Copy)]
pub struct Ui {
    pub doc: RwSignal<Option<p::DocumentInfo>>,
    pub revision: RwSignal<u64>,
    pub dirty: RwSignal<bool>,
    pub can_undo: RwSignal<bool>,
    pub can_redo: RwSignal<bool>,
    pub cursor: RwSignal<(u32, u32, u32)>,
    pub message: RwSignal<String>,
    pub analysis: RwSignal<Option<p::Analysis>>,
    pub selected: RwSignal<Option<String>>,
    pub inspection: RwSignal<Option<p::Inspection>>,
    /// The last Inspector refusal, shown until the next selection or edit.
    pub field_error: RwSignal<Option<String>>,
    pub conflict: RwSignal<Option<String>>,
    pub preview_status: RwSignal<String>,
    pub preview_enabled: RwSignal<bool>,
    pub last_save: RwSignal<Option<String>>,
    /// X_ITE scene loads this run (lets tests prove a theme switch never
    /// reloads the preview).
    pub preview_loads: RwSignal<u64>,
}

thread_local! {
    static UI: OnceCell<Ui> = const { OnceCell::new() };
}

pub fn ui() -> Ui {
    UI.with(|u| {
        *u.get_or_init(|| Ui {
            doc: RwSignal::new(None),
            revision: RwSignal::new(0),
            dirty: RwSignal::new(false),
            can_undo: RwSignal::new(false),
            can_redo: RwSignal::new(false),
            cursor: RwSignal::new((1, 1, 0)),
            message: RwSignal::new(String::new()),
            analysis: RwSignal::new(None),
            selected: RwSignal::new(None),
            inspection: RwSignal::new(None),
            field_error: RwSignal::new(None),
            conflict: RwSignal::new(None),
            preview_status: RwSignal::new("idle".into()),
            preview_enabled: RwSignal::new(true),
            last_save: RwSignal::new(None),
            preview_loads: RwSignal::new(0),
        })
    })
}

pub fn flash(m: &str) {
    ui().message.set(m.to_string());
}

pub fn cursor(line: u32, col: u32, sel: u32) {
    ui().cursor.set((line, col, sel));
}

pub fn doc_loaded(doc: &p::DocumentInfo) {
    let u = ui();
    u.doc.set(Some(doc.clone()));
    u.conflict.set(None);
    u.selected.set(None);
    u.inspection.set(None);
    state_changed(doc.revision, doc.dirty, doc.can_undo, doc.can_redo);
    set_title(doc);
}

/// The native window title is owned by Rust (`window_title`); the UI only
/// says which session it shows.
fn set_title(doc: &p::DocumentInfo) {
    let session = doc.session;
    spawn_local(async move {
        let _ = call::<()>("window_title", Session { session }).await;
    });
}

/// Every acknowledged change lands here; schedules derived views.
pub fn state_changed(revision: u64, dirty: bool, can_undo: bool, can_redo: bool) {
    let u = ui();
    u.revision.set(revision);
    u.dirty.set(dirty);
    u.can_undo.set(can_undo);
    u.can_redo.set(can_redo);
    if let Some(doc) = u.doc.get_untracked() {
        set_title(&doc);
    }
    let generation = CORE.with_borrow(|c| c.generation);
    spawn_local(async move {
        ipc::sleep(250).await;
        if CORE.with_borrow(|c| c.generation == generation && c.analyzed != Some(revision)) {
            analyze().await;
        }
    });
    spawn_local(async move {
        ipc::sleep(700).await; // the JS preview-scheduler debounce
        if CORE.with_borrow(|c| c.generation) == generation && ui().preview_enabled.get_untracked()
        {
            preview(false).await;
        }
    });
}

pub async fn analyze() {
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    match call::<p::Analysis>("doc_analyze", Session { session }).await {
        Ok(a) => {
            // A reply for a document that is no longer open, or one older than
            // the analysis already shown, is discarded.
            if CORE.with_borrow(|c| c.session) != Some(a.session)
                || ui().analysis.with_untracked(|cur| {
                    cur.as_ref()
                        .is_some_and(|c| c.session == a.session && c.revision > a.revision)
                })
            {
                return;
            }
            CORE.with_borrow_mut(|c| c.analyzed = Some(a.revision));
            // Colours and underlines only if this parse is of the text on
            // screen (session, revision, view hash); otherwise a newer
            // analysis is already scheduled.
            crate::syntax::apply(&a);
            if a.highlights_truncated {
                flash("Large document: syntax colours cover the first 400,000 tokens only.");
            }
            let u = ui();
            let still = u
                .selected
                .get_untracked()
                .filter(|id| a.items.iter().any(|i| &i.id == id));
            u.analysis.set(Some(a));
            if still.is_none() {
                u.selected.set(None);
                u.inspection.set(None);
            } else if let Some(id) = still {
                inspect(id).await;
            }
        }
        Err(e) => flash(&format!("Analysis failed: {e}")),
    }
}

pub async fn inspect(id: String) {
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    #[derive(serde::Serialize)]
    struct A {
        session: u64,
        item: String,
    }
    match call::<Option<p::Inspection>>(
        "doc_inspect",
        A {
            session,
            item: id.clone(),
        },
    )
    .await
    {
        Ok(i) => {
            let u = ui();
            if u.selected.get_untracked().as_deref() != Some(id.as_str()) {
                u.field_error.set(None);
            }
            u.selected.set(Some(id));
            u.inspection.set(i);
        }
        Err(e) => flash(&format!("Inspector failed: {e}")),
    }
}

/// Send one Inspector field edit to Rust. The request names the node by the
/// item id of the revision the Inspector was built from; Rust refuses it if
/// the document moved on. The UI adopts nothing until Rust applies it.
pub async fn edit_field(field_index: u32, field_name: String, components: Vec<p::FieldInput>) {
    editor::idle().await;
    let u = ui();
    let (Some(session), Some(insp)) = (
        CORE.with_borrow(|c| c.session),
        u.inspection.get_untracked(),
    ) else {
        return;
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::FieldEditRequest,
    }
    let request = p::FieldEditRequest {
        session,
        base_revision: insp.revision,
        item: insp.id.clone(),
        field_index,
        field_name: field_name.clone(),
        components,
    };
    match call::<p::FieldEditOutcome>("doc_edit_field", A { request }).await {
        Ok(p::FieldEditOutcome::Applied {
            state, view, item, ..
        }) => {
            u.field_error.set(None);
            editor::adopt_change(&state, view);
            u.selected.set(Some(item.clone()));
            inspect(item).await;
            flash(&format!(
                "{field_name} changed (revision {}).",
                state.revision
            ));
        }
        Ok(p::FieldEditOutcome::Unchanged) => flash(&format!("{field_name}: no change.")),
        Ok(p::FieldEditOutcome::Refused {
            reason,
            message,
            component_index,
        }) => {
            let at = component_index
                .map(|i| format!(" (component {})", i + 1))
                .unwrap_or_default();
            let m = format!(
                "Not changed — {field_name}{at}: {}",
                message.unwrap_or_else(|| reason.clone())
            );
            u.field_error.set(Some(format!("{m} [{reason}]")));
            flash(&m);
        }
        Err(e) => {
            u.field_error.set(Some(format!("Field edit failed: {e}")));
            flash(&format!("Field edit failed: {e}"));
        }
    }
}

/// Load the current revision into X_ITE. Without `force`, a revision already
/// on screen is not reloaded (each reload builds a whole new X_ITE scene).
pub async fn preview(force: bool) {
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    let (rev, done) = CORE.with_borrow(|c| (c.revision, c.previewed));
    if !force && done == Some(rev) {
        return;
    }
    let u = ui();
    // Only the newest load may report: a superseded X_ITE load ends with
    // "Replacing world aborted" and must not overwrite a newer status.
    let seq = CORE.with_borrow_mut(|c| {
        c.preview_seq += 1;
        c.preview_seq
    });
    let newest = move || CORE.with_borrow(|c| c.preview_seq == seq);
    u.preview_status.set("updating…".into());
    match call::<p::PreviewSource>("doc_preview_source", Session { session }).await {
        Ok(src) => {
            CORE.with_borrow_mut(|c| c.previewed = Some(src.revision));
            let status = ipc::preview_load(&src.text).await;
            u.preview_loads.update(|n| *n += 1);
            if newest() {
                u.preview_status
                    .set(format!("rev {} · {status}", src.revision));
            }
        }
        Err(e) if newest() => u.preview_status.set(format!("error: {e}")),
        Err(_) => {}
    }
}

pub fn open() {
    spawn_local(async {
        if confirm_discard().await {
            match call::<p::OpenOutcome>("open_document", NoArgs {}).await {
                Ok(o) => apply_open(o, "Opened").await,
                Err(e) => flash(&format!("Open failed: {e}")),
            }
        }
    });
}

async fn confirm_discard() -> bool {
    // A dirty buffer is never discarded silently. Without a modal dialog
    // (WebView alerts block the IPC), the Open action simply refuses.
    if ui().dirty.get_untracked() {
        flash("Save or undo your changes before opening another file.");
        return false;
    }
    true
}

pub async fn apply_open(o: p::OpenOutcome, verb: &str) {
    match o {
        p::OpenOutcome::Opened { doc } => {
            if let Some(old) = CORE
                .with_borrow(|c| c.session)
                .filter(|&s| s != doc.session)
            {
                let _ = call::<()>("close_document", Session { session: old }).await;
            }
            editor::adopt(&doc);
            flash(&format!(
                "{verb} {} ({}, {}{}{})",
                doc.name,
                doc.format,
                doc.eol,
                if doc.eol_mixed {
                    ", mixed line endings"
                } else {
                    ""
                },
                if doc.bom { ", BOM" } else { "" }
            ));
        }
        p::OpenOutcome::Cancelled => flash("Open cancelled — nothing changed."),
        p::OpenOutcome::Failed { message } => flash(&format!("Could not open: {message}")),
    }
}

pub fn save(as_new: bool) {
    spawn_local(async move {
        let _ = save_now(as_new).await;
    });
}

pub async fn save_now(as_new: bool) -> Option<p::SaveOutcome> {
    editor::idle().await;
    let session = CORE.with_borrow(|c| c.session)?;
    let cmd = if as_new { "doc_save_as" } else { "doc_save" };
    let r = call::<p::SaveOutcome>(cmd, Session { session }).await;
    let u = ui();
    match &r {
        Ok(p::SaveOutcome::Saved {
            bytes_written,
            backup,
            preserved,
            doc,
        }) => {
            u.doc.set(Some(doc.clone()));
            state_changed(doc.revision, doc.dirty, doc.can_undo, doc.can_redo);
            let m = if *preserved {
                "Already saved ✓ — the gzip artifact already holds this text; nothing written."
                    .to_string()
            } else {
                format!(
                    "Saved {} ({bytes_written} B, {}). Backup: {}",
                    doc.name,
                    doc.format,
                    backup.clone().unwrap_or_else(|| "none (new file)".into())
                )
            };
            u.last_save.set(Some(m.clone()));
            flash(&m);
        }
        Ok(p::SaveOutcome::Conflict { reason }) => {
            u.conflict.set(Some(reason.clone()));
            flash(&format!("Not saved: the file changed on disk ({reason})."));
        }
        Ok(p::SaveOutcome::Cancelled) => flash("Save cancelled — nothing written."),
        Ok(p::SaveOutcome::Failed { message }) => flash(&format!("Not saved: {message}")),
        Err(e) => flash(&format!("Save failed: {e}")),
    }
    r.ok()
}

pub fn reload() {
    spawn_local(async {
        let Some(session) = CORE.with_borrow(|c| c.session) else {
            return;
        };
        match call::<p::OpenOutcome>("doc_reload", Session { session }).await {
            Ok(o) => apply_open(o, "Reloaded").await,
            Err(e) => flash(&format!("Reload failed: {e}")),
        }
    });
}

/// Poll for external changes (the JS editor polls too).
pub fn start_external_watch() {
    spawn_local(async {
        loop {
            ipc::sleep(3000).await;
            let Some(session) = CORE.with_borrow(|c| c.session) else {
                continue;
            };
            if ui().conflict.get_untracked().is_some() {
                continue;
            }
            if let Ok(st) =
                call::<p::ExternalStatus>("doc_check_external", Session { session }).await
            {
                if st.changed {
                    ui().conflict.set(Some(st.reason));
                }
            }
        }
    });
}
