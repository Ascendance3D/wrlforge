// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri IPC shim over `service::Service`. Thin on purpose: no document or
//! file logic lives here, only dialog plumbing and argument passing.
//!
//! No command accepts a filesystem path from the WebView. Open and Save As
//! obtain their path from a native dialog run by Rust; the launch argument is
//! read by Rust at startup.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use wrlforge_desktop_protocol as p;

use crate::service::Service;
use crate::settings::SettingsStore;
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
    // `--smoke-create` only: the launch argument's file stands in for the
    // dialog (Rust-chosen, never a WebView path).
    if let Some(path) = app.state::<SmokeState>().open_override() {
        return app.state::<Service>().open_path(&path);
    }
    let picked = app
        .dialog()
        .file()
        .set_title("Open VRML97 world or item")
        .add_filter("VRML97 (.wrl, .wrz, gzip)", FILTER_EXT)
        .blocking_pick_file();
    let Some(fp) = picked else {
        return p::OpenOutcome::Canceled;
    };
    match fp.into_path() {
        Ok(path) => app.state::<Service>().open_path(&path),
        Err(e) => p::OpenOutcome::Failed {
            message: format!("unsupported dialog result: {e}"),
        },
    }
}

/// File → New World. `replace` is the session the window shows now. If Rust
/// says it holds unsaved changes, a native confirmation runs first; Cancel
/// returns `Canceled` and leaves that document exactly as it was. The new
/// world has no path until Save As.
#[tauri::command]
pub async fn new_document(app: AppHandle, replace: Option<p::SessionId>) -> p::OpenOutcome {
    let svc = app.state::<Service>();
    if let Some(id) = replace {
        let name = svc.snapshot(id).map(|d| d.name).unwrap_or_default();
        match svc.is_dirty(id) {
            Ok(true) => {
                let discard = match app.state::<SmokeState>().confirm_override() {
                    Some(answer) => answer,
                    None => app
                        .dialog()
                        .message(format!(
                            "\"{name}\" has unsaved changes. Discard them and start a new world?"
                        ))
                        .title("New World")
                        .kind(MessageDialogKind::Warning)
                        .buttons(MessageDialogButtons::OkCancelCustom(
                            "Discard changes".into(),
                            "Cancel".into(),
                        ))
                        .blocking_show(),
                };
                if !discard {
                    return p::OpenOutcome::Canceled;
                }
            }
            Ok(false) => {}
            // An unknown session holds nothing to lose.
            Err(_) => {}
        }
    }
    svc.new_world()
}

/// Create one primitive object (VISUAL-1). Synchronous for the same
/// ordering reason as `doc_edit`.
#[tauri::command]
pub fn doc_create(
    svc: State<'_, Service>,
    request: p::CreateRequest,
) -> Result<p::CreateOutcome, String> {
    svc.create(&request)
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
        .unwrap_or_else(|_| crate::service::UNTITLED_NAME.into());
    if let Some(path) = app.state::<SmokeState>().save_as_override() {
        return app.state::<Service>().save_as(session, &path);
    }
    let picked = app
        .dialog()
        .file()
        .set_title("Save VRML97 file as")
        .set_file_name(suggested)
        .add_filter("VRML97 (.wrl, .wrz, gzip)", FILTER_EXT)
        .blocking_save_file();
    let Some(fp) = picked else {
        return p::SaveOutcome::Canceled;
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

/// Async on purpose (unlike `doc_edit`): the parse runs on a worker thread
/// over a copy of one revision, so the main thread -- and the edits queued
/// on it -- never wait behind it. The reply names its revision; the UI
/// discards it unless it is still current.
#[tauri::command]
pub async fn doc_analyze(
    svc: State<'_, Service>,
    session: p::SessionId,
) -> Result<p::Analysis, String> {
    svc.analyze(session)
}

/// Async for the same reason as `doc_analyze`.
#[tauri::command]
pub async fn doc_inspect(
    svc: State<'_, Service>,
    session: p::SessionId,
    item: String,
    revision: u64,
) -> Result<p::InspectOutcome, String> {
    svc.inspect(session, &item, revision)
}

/// Synchronous for the same ordering reason as `doc_edit`.
#[tauri::command]
pub fn doc_edit_field(
    svc: State<'_, Service>,
    request: p::FieldEditRequest,
) -> Result<p::FieldEditOutcome, String> {
    svc.edit_field(&request)
}

/// VISUAL-3A: whether the gizmo may move an item. Read-only; async like
/// `doc_inspect` because it parses.
#[tauri::command]
pub async fn doc_translate_target(
    svc: State<'_, Service>,
    request: p::TranslateTargetRequest,
) -> Result<p::TranslateTargetOutcome, String> {
    svc.translate_target(&request)
}

/// VISUAL-3A1: prove the runtime node the preview located for a Move target.
/// Read-only.
#[tauri::command]
pub async fn doc_translate_prove(
    svc: State<'_, Service>,
    request: p::TranslateProveRequest,
) -> Result<p::TranslateProveOutcome, String> {
    svc.translate_prove(&request)
}

/// VISUAL-3A1: carry a preview span (the bound Viewpoint) to the current
/// revision through the exact logged changes. Read-only.
#[tauri::command]
pub async fn doc_preview_carry(
    svc: State<'_, Service>,
    request: p::PreviewCarryRequest,
) -> Result<p::PreviewCarryOutcome, String> {
    svc.preview_carry(&request)
}

/// VISUAL-3A: commit one gizmo drag. Synchronous for the same ordering
/// reason as `doc_edit`.
#[tauri::command]
pub fn doc_translate(
    svc: State<'_, Service>,
    request: p::TranslateRequest,
) -> Result<p::TranslateOutcome, String> {
    svc.translate(&request)
}

/// Resolve one viewport pick (VISUAL-2). Read-only; async like
/// `doc_analyze` because it parses.
#[tauri::command]
pub async fn doc_pick(
    svc: State<'_, Service>,
    request: p::PickRequest,
) -> Result<p::PickOutcome, String> {
    svc.pick(&request)
}

#[tauri::command]
pub fn doc_preview_source(
    svc: State<'_, Service>,
    session: p::SessionId,
) -> Result<p::PreviewSource, String> {
    svc.preview_source(session)
}

/// `--smoke-pick` only: open fixture `index` of the armed plan. The path is
/// Rust's; the WebView names an index.
#[tauri::command]
pub fn smoke_open_fixture(
    svc: State<'_, Service>,
    smoke: State<'_, SmokeState>,
    index: usize,
) -> p::OpenOutcome {
    match smoke.pick_fixture(index) {
        Some(path) => svc.open_path(&path),
        None => p::OpenOutcome::Failed {
            message: "no picking smoke run is armed".into(),
        },
    }
}

/// `--smoke-pick` only: a REAL X pointer click at client CSS px (`x`, `y`)
/// of the main window (see `SmokeState::real_click` for the guards). Async:
/// the main thread must stay free to receive the click.
#[tauri::command]
pub async fn smoke_real_click(app: AppHandle, x: f64, y: f64) -> Result<String, String> {
    let w = app.get_webview_window("main").ok_or("no main window")?;
    let pos = w.inner_position().map_err(|e| e.to_string())?;
    let scale = w.scale_factor().map_err(|e| e.to_string())?;
    let rx = (pos.x as f64 + x * scale).round() as i32;
    let ry = (pos.y as f64 + y * scale).round() as i32;
    app.state::<SmokeState>().real_click(rx, ry)
}

/// `--smoke-pick` / `--smoke-move` only: one REAL X input action at client
/// CSS px (`x`, `y`) of the main window (`SmokeState::real_pointer`).
#[tauri::command]
pub async fn smoke_real_pointer(
    app: AppHandle,
    action: String,
    x: f64,
    y: f64,
) -> Result<String, String> {
    let w = app.get_webview_window("main").ok_or("no main window")?;
    let pos = w.inner_position().map_err(|e| e.to_string())?;
    let scale = w.scale_factor().map_err(|e| e.to_string())?;
    let rx = (pos.x as f64 + x * scale).round() as i32;
    let ry = (pos.y as f64 + y * scale).round() as i32;
    app.state::<SmokeState>().real_pointer(&action, rx, ry)
}

/// Smoke only (VISUAL-3A1): resize OUR window to `w` x `h` px with real X
/// input, under the same guards as `smoke_real_pointer`.
#[tauri::command]
pub async fn smoke_real_resize(app: AppHandle, w: i32, h: i32) -> Result<String, String> {
    app.state::<SmokeState>().real_pointer("resize", w, h)
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

/// The active theme and the built-in choices. Application state, not document
/// state: no session is involved.
#[tauri::command]
pub fn theme_get(settings: State<'_, SettingsStore>) -> p::theme::ThemeState {
    settings.state()
}

/// Validate a theme id against the built-in registry and persist it.
/// Synchronous so selections apply and persist in the order they were made.
#[tauri::command]
pub fn theme_set(
    settings: State<'_, SettingsStore>,
    theme_id: String,
) -> p::theme::ThemeSetOutcome {
    settings.set_theme(&theme_id)
}
