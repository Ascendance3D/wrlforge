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
    /// The selected Scene Tree item, valid in exactly one revision.
    pub selected: RwSignal<Option<Selected>>,
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

/// A Scene Tree selection. Item ids are SOURCE spans of one revision, so
/// the id is meaningless without the session and revision it came from.
/// Every acknowledged change carries it forward through Rust's exact change
/// mapping, or clears it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    pub session: u64,
    pub revision: u64,
    pub id: String,
}

impl Selected {
    pub fn is(&self, id: &str) -> bool {
        self.id == id
    }
}

thread_local! {
    static UI: OnceCell<Ui> = const { OnceCell::new() };
}

/// Why offsets from a projection of (`session`, `revision`) may NOT be used
/// now, or `None` when they may: the projection is of exactly the text the
/// widget shows and Rust holds (no edit in flight, no newer revision).
pub fn stale_reason(session: u64, revision: u64) -> Option<String> {
    let (cur_s, cur_r, busy) = CORE.with_borrow(|c| (c.session, c.revision, c.busy));
    if cur_s != Some(session) {
        Some("another document is open".into())
    } else if busy {
        Some("an edit is being applied".into())
    } else if cur_r != revision {
        Some(format!(
            "it shows revision {revision}, the document is at {cur_r}"
        ))
    } else {
        None
    }
}

/// Select a source span that a projection of (`session`, `revision`)
/// reported. Refused -- nothing selected, nothing changed -- when that
/// projection is not of the current text. Returns whether it selected.
pub fn select_from(what: &str, session: u64, revision: u64, from: u64, to: u64) -> bool {
    match stale_reason(session, revision) {
        None => {
            editor::select(from, to);
            true
        }
        Some(why) => {
            flash(&format!(
                "{what} is updating ({why}); selection not changed."
            ));
            false
        }
    }
}

/// A Scene Tree click / Enter on an item rendered from the analysis of
/// (`session`, `revision`). The rendered DOM may lag the newest analysis, so
/// the revision is the one the item was RENDERED with, and it must also be
/// the analysis currently held and the document's current revision.
pub fn tree_select(session: u64, revision: u64, id: String, from: u64, to: u64) -> bool {
    let held = ui()
        .analysis
        .with_untracked(|a| a.as_ref().map(|a| (a.session, a.revision)));
    if held != Some((session, revision)) {
        flash("Scene Tree is updating (a newer analysis is being shown); selection not changed.");
        return false;
    }
    if !select_from("Scene Tree", session, revision, from, to) {
        // Catch the tree up now instead of after the debounce.
        spawn_local(analyze());
        return false;
    }
    let u = ui();
    let sel = Selected {
        session,
        revision,
        id,
    };
    if u.selected.get_untracked().as_ref() != Some(&sel) {
        u.field_error.set(None);
        // Never show the previous item's fields under the new selection.
        u.inspection.set(None);
    }
    u.selected.set(Some(sel.clone()));
    spawn_local(inspect(sel));
    true
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
    let title_changes = u.dirty.get_untracked() != dirty;
    u.revision.set(revision);
    // Unchanged flags are not re-set: no re-render per keystroke for them.
    if title_changes {
        u.dirty.set(dirty);
    }
    if u.can_undo.get_untracked() != can_undo {
        u.can_undo.set(can_undo);
    }
    if u.can_redo.get_untracked() != can_redo {
        u.can_redo.set(can_redo);
    }
    // The window title shows only the name and the dirty mark: ask Rust for
    // it when that mark changes, not on every keystroke.
    if title_changes {
        if let Some(doc) = u.doc.get_untracked() {
            set_title(&doc);
        }
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
            if let Some(sel) = apply_analysis(a) {
                inspect(sel).await;
            }
        }
        Err(e) => flash(&format!("Analysis failed: {e}")),
    }
}

/// Adopt one analysis reply. A reply for a document that is no longer open,
/// or older than the analysis already shown, is discarded (returns `None`
/// with nothing changed). Returns the selection to re-inspect, if any.
pub fn apply_analysis(a: p::Analysis) -> Option<Selected> {
    let u = ui();
    if CORE.with_borrow(|c| c.session) != Some(a.session)
        || u.analysis.with_untracked(|cur| {
            cur.as_ref()
                .is_some_and(|c| c.session == a.session && c.revision > a.revision)
        })
    {
        CORE.with_borrow_mut(|c| c.stale_analyses += 1);
        return None;
    }
    CORE.with_borrow_mut(|c| c.analyzed = Some(a.revision));
    // Colors and underlines only if this parse is of the text on screen
    // (session, revision, view hash); otherwise a newer analysis is already
    // scheduled.
    crate::syntax::apply(&a);
    if a.highlights_truncated {
        flash("Large document: syntax colors cover the first 400,000 tokens only.");
    }
    // The selection is checked against the analysis of ITS OWN revision only:
    // an id that is absent there is gone; an analysis of another revision
    // says nothing about it.
    let sel = u
        .selected
        .get_untracked()
        .filter(|s| s.session == a.session && s.revision == a.revision);
    let keep = sel.as_ref().map(|s| a.items.iter().any(|i| s.is(&i.id)));
    u.analysis.set(Some(a));
    match keep {
        Some(true) => sel,
        Some(false) => {
            u.selected.set(None);
            u.inspection.set(None);
            None
        }
        None => None,
    }
}

/// Ask Rust for the Inspector data of `sel` in `sel.revision`. Only the
/// newest request may land, and only while the selection and the document
/// are still exactly what it asked about.
pub async fn inspect(sel: Selected) {
    let seq = CORE.with_borrow_mut(|c| {
        c.inspect_seq += 1;
        c.inspect_seq
    });
    #[derive(serde::Serialize)]
    struct A {
        session: u64,
        item: String,
        revision: u64,
    }
    let r = call::<p::InspectOutcome>(
        "doc_inspect",
        A {
            session: sel.session,
            item: sel.id.clone(),
            revision: sel.revision,
        },
    )
    .await;
    match r {
        Ok(o) => {
            apply_inspection(seq, &sel, o);
        }
        Err(e) => flash(&format!("Inspector failed: {e}")),
    }
}

/// Adopt one Inspector reply, or discard it (returns false, nothing changed)
/// when a newer request was made, the selection moved, or the reply is not of
/// the selection's revision.
pub fn apply_inspection(seq: u64, sel: &Selected, o: p::InspectOutcome) -> bool {
    let u = ui();
    let newest = CORE.with_borrow(|c| c.inspect_seq == seq);
    let still = u.selected.get_untracked().as_ref() == Some(sel);
    let ok = newest
        && still
        && match &o {
            p::InspectOutcome::Found { inspection } => {
                inspection.revision == sel.revision && inspection.id == sel.id
            }
            p::InspectOutcome::Missing => true,
            p::InspectOutcome::Stale { .. } => false,
        };
    if !ok {
        CORE.with_borrow_mut(|c| c.stale_inspections += 1);
        return false;
    }
    match o {
        p::InspectOutcome::Found { inspection } => u.inspection.set(Some(inspection)),
        _ => {
            u.selected.set(None);
            u.inspection.set(None);
        }
    }
    true
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
    // Fields of an older revision are not sent (Rust would refuse them too).
    if let Some(why) = stale_reason(session, insp.revision) {
        let m = format!("Not changed — {field_name}: the Inspector is updating ({why}).");
        u.field_error.set(Some(m.clone()));
        flash(&m);
        return;
    }
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
            let sel = Selected {
                session,
                revision: state.revision,
                id: item,
            };
            u.selected.set(Some(sel.clone()));
            inspect(sel).await;
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
        p::OpenOutcome::Canceled => flash("Open canceled — nothing changed."),
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
        Ok(p::SaveOutcome::Canceled) => flash("Save canceled — nothing written."),
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
