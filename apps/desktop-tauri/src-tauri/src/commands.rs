// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri IPC shim over `service::Service`. Thin on purpose: no document or
//! file logic lives here, only dialog plumbing and argument passing.
//!
//! No command accepts a filesystem path from the WebView. Open and Save As
//! obtain their path from a native dialog run by Rust; the launch argument is
//! read by Rust at startup.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use wrlforge_desktop_protocol as p;

use crate::service::Service;
use crate::smoke::SmokeState;

/// The document opened from the launch argument, handed to the UI once.
#[derive(Default)]
pub struct Startup(pub Mutex<Option<p::OpenOutcome>>);

const FILTER_EXT: &[&str] = &["wrl", "wrz", "gz", "WRL", "WRZ"];

#[tauri::command]
pub fn startup_document(startup: State<'_, Startup>) -> Option<p::OpenOutcome> {
    startup.0.lock().ok().and_then(|mut s| s.take())
}

#[tauri::command]
pub async fn open_document(app: AppHandle) -> p::OpenOutcome {
    let picked = app
        .dialog()
        .file()
        .set_title("Open VRML97 world or item")
        .add_filter("VRML97 (.wrl, .wrz, gzip)", FILTER_EXT)
        .blocking_pick_file();
    let Some(fp) = picked else {
        return p::OpenOutcome::Cancelled;
    };
    match fp.into_path() {
        Ok(path) => app.state::<Service>().open_path(&path),
        Err(e) => p::OpenOutcome::Failed {
            message: format!("unsupported dialog result: {e}"),
        },
    }
}

#[tauri::command]
pub fn close_document(svc: State<'_, Service>, session: p::SessionId) -> Result<(), String> {
    svc.close(session)
}

#[tauri::command]
pub fn doc_snapshot(
    svc: State<'_, Service>,
    session: p::SessionId,
) -> Result<p::DocumentInfo, String> {
    svc.snapshot(session)
}

/// Synchronous on purpose: Tauri runs non-async commands on the main thread,
/// so edits apply in the order the UI sent them.
#[tauri::command]
pub fn doc_edit(
    svc: State<'_, Service>,
    request: p::EditRequest,
) -> Result<p::EditOutcome, String> {
    svc.edit(&request)
}

#[tauri::command]
pub fn doc_undo(
    svc: State<'_, Service>,
    session: p::SessionId,
    item: Option<String>,
) -> Result<p::HistoryOutcome, String> {
    svc.history_with_item(session, true, item.as_deref())
}

#[tauri::command]
pub fn doc_redo(
    svc: State<'_, Service>,
    session: p::SessionId,
    item: Option<String>,
) -> Result<p::HistoryOutcome, String> {
    svc.history_with_item(session, false, item.as_deref())
}

#[tauri::command]
pub fn doc_save(svc: State<'_, Service>, session: p::SessionId) -> p::SaveOutcome {
    svc.save(session)
}

#[tauri::command]
pub async fn doc_save_as(app: AppHandle, session: p::SessionId) -> p::SaveOutcome {
    let suggested = app
        .state::<Service>()
        .snapshot(session)
        .map(|d| d.name)
        .unwrap_or_else(|_| "untitled.wrl".into());
    let picked = app
        .dialog()
        .file()
        .set_title("Save VRML97 file as")
        .set_file_name(suggested)
        .add_filter("VRML97 (.wrl, .wrz, gzip)", FILTER_EXT)
        .blocking_save_file();
    let Some(fp) = picked else {
        return p::SaveOutcome::Cancelled;
    };
    match fp.into_path() {
        Ok(path) => app.state::<Service>().save_as(session, &path),
        Err(e) => p::SaveOutcome::Failed {
            message: format!("unsupported dialog result: {e}"),
        },
    }
}

#[tauri::command]
pub fn doc_check_external(
    svc: State<'_, Service>,
    session: p::SessionId,
) -> Result<p::ExternalStatus, String> {
    svc.check_external(session)
}

#[tauri::command]
pub fn doc_reload(svc: State<'_, Service>, session: p::SessionId) -> p::OpenOutcome {
    svc.reload(session)
}

#[tauri::command]
pub fn doc_analyze(svc: State<'_, Service>, session: p::SessionId) -> Result<p::Analysis, String> {
    svc.analyze(session)
}

#[tauri::command]
pub fn doc_inspect(
    svc: State<'_, Service>,
    session: p::SessionId,
    item: String,
) -> Result<Option<p::Inspection>, String> {
    svc.inspect(session, &item)
}

/// Synchronous for the same ordering reason as `doc_edit`.
#[tauri::command]
pub fn doc_edit_field(
    svc: State<'_, Service>,
    request: p::FieldEditRequest,
) -> Result<p::FieldEditOutcome, String> {
    svc.edit_field(&request)
}

#[tauri::command]
pub fn doc_preview_source(
    svc: State<'_, Service>,
    session: p::SessionId,
) -> Result<p::PreviewSource, String> {
    svc.preview_source(session)
}

#[tauri::command]
pub fn smoke_plan(smoke: State<'_, SmokeState>) -> Option<p::SmokePlan> {
    smoke.plan()
}

#[tauri::command]
pub fn smoke_finish(app: AppHandle, report: p::SmokeReport) {
    crate::smoke::finish(&app, report);
}

/// Title the main window from the session's own state: `● name — WRL Forge`.
#[tauri::command]
pub fn window_title(app: AppHandle, session: p::SessionId) -> Result<(), String> {
    let doc = app.state::<Service>().snapshot(session)?;
    let title = format!(
        "{}{} — WRL Forge",
        if doc.dirty { "● " } else { "" },
        doc.name
    );
    if let Some(w) = app.get_webview_window("main") {
        w.set_title(&title).map_err(|e| e.to_string())?;
    }
    Ok(())
}
