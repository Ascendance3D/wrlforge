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

use crate::native::{Native, Shown};
use crate::service::Service;
use crate::settings::SettingsStore;
use crate::viewport;
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
                // RUST-STAB-1: an explicit discard removes the recovery copy.
                let _ = svc.discard_recovery(id);
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
    // UI-MENU-1 smoke: a planned Save As choice, or a canceled dialog.
    if let Some(choice) = crate::smoke_menu::save_as_choice() {
        return match choice {
            Some(path) => app.state::<Service>().save_as(session, &path),
            None => p::SaveOutcome::Canceled,
        };
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

/// VISUAL-4A: a list-field edit (one Inspector action or one outline
/// session). Synchronous like `doc_edit_field`.
#[tauri::command]
pub fn doc_edit_list(
    svc: State<'_, Service>,
    request: p::ListEditRequest,
) -> Result<p::FieldEditOutcome, String> {
    svc.edit_list(&request)
}

/// VISUAL-4A: write one absent field because the user asked.
#[tauri::command]
pub fn doc_add_field(
    svc: State<'_, Service>,
    request: p::AddFieldRequest,
) -> Result<p::FieldEditOutcome, String> {
    svc.add_field(&request)
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

/// VISUAL-4B: commit one rotation drag (one undo step).
#[tauri::command]
pub fn doc_rotate(
    svc: State<'_, Service>,
    request: p::RotateRequest,
) -> Result<p::TranslateOutcome, String> {
    svc.rotate(&request)
}

/// VISUAL-4B: commit one scale drag (one undo step).
#[tauri::command]
pub fn doc_scale(
    svc: State<'_, Service>,
    request: p::ScaleRequest,
) -> Result<p::TranslateOutcome, String> {
    svc.scale(&request)
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

/// NATIVE-RENDER-1: the native viewport's state (hidden; default off).
#[tauri::command]
pub fn native_state() -> p::NativeState {
    viewport::state()
}

/// NATIVE-RENDER-1: project the session's current text and show it in the
/// native viewport. Async: it parses. A damaged text keeps the last valid
/// projection on screen.
#[tauri::command]
pub async fn native_show(
    app: AppHandle,
    svc: State<'_, Service>,
    native: State<'_, Native>,
    session: p::SessionId,
) -> Result<p::NativeShown, String> {
    if !native.active.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("the native viewport is off".into());
    }
    match native.show(&svc, session)? {
        Shown::New(scene, info) => {
            app.run_on_main_thread(move || viewport::set_scene(scene))
                .map_err(|e| e.to_string())?;
            Ok(info)
        }
        Shown::KeptLastValid(info) => Ok(info),
    }
}

/// NATIVE-RENDER-1: highlight the Scene Tree selection (`item` of
/// `revision`) in the native viewport. Display only.
#[tauri::command]
pub async fn native_select(
    app: AppHandle,
    svc: State<'_, Service>,
    native: State<'_, Native>,
    session: p::SessionId,
    revision: u64,
    item: Option<String>,
) -> Result<u32, String> {
    if !native.active.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(0);
    }
    let (generation, ids) = native.selected_ids(&svc, session, revision, item.as_deref());
    let n = ids.len() as u32;
    app.run_on_main_thread(move || viewport::select(generation, ids))
        .map_err(|e| e.to_string())?;
    Ok(n)
}

#[tauri::command]
pub fn native_reset_camera() {
    viewport::reset_camera();
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

/// `--smoke-texture` only: hold every `wrlres` read `ms` (at most 5 s) so a
/// document switch lands while a request is in flight.
#[tauri::command]
pub fn smoke_texture_delay(smoke: State<'_, SmokeState>, ms: u64) -> Result<(), String> {
    if !smoke.texture_armed() {
        return Err("no texture smoke run is armed".into());
    }
    crate::resources::TEST_DELAY_MS.store(ms.min(5_000), std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// `--smoke-texture` only: store a viewport capture (PNG bytes) as evidence.
#[tauri::command]
pub fn smoke_texture_capture(
    smoke: State<'_, SmokeState>,
    id: String,
    png: Vec<u8>,
) -> Result<String, String> {
    smoke.texture_capture(&id, &png)
}

/// `--smoke-texture` only: the answered `wrlres` requests.
#[tauri::command]
pub fn smoke_resource_log(
    smoke: State<'_, SmokeState>,
) -> Result<Vec<p::ResourceLogEntry>, String> {
    if !smoke.texture_armed() {
        return Err("no texture smoke run is armed".into());
    }
    Ok(crate::resources::log_snapshot())
}

/// `--smoke-extrusion` only (inside the harness Xvfb): a whole-screen PNG.
#[tauri::command]
pub async fn smoke_window_capture(app: AppHandle, id: String) -> Result<String, String> {
    app.state::<SmokeState>().window_capture(&id)
}

#[tauri::command]
pub fn smoke_finish(app: AppHandle, report: p::SmokeReport) {
    crate::smoke::finish(&app, report);
}

/// APP-SHELL-0: the version, channel and default renderer, for About.
#[tauri::command]
pub fn app_info(app: AppHandle) -> p::command::AppInfo {
    crate::app_info(&app)
}

/// Title the main window from the session's own state:
/// `● name — WRL Forge 1.4.5 (Beta)`.
#[tauri::command]
pub fn window_title(app: AppHandle, session: p::SessionId) -> Result<(), String> {
    let doc = app.state::<Service>().snapshot(session)?;
    let title = format!(
        "{}{} — {}",
        if doc.dirty { "● " } else { "" },
        doc.name,
        p::command::app_display_name()
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

/// UI-DOCK-1: the workspace layout to apply, with a notice when the saved
/// one could not be used. Application state only: no session is involved.
#[tauri::command]
pub fn layout_get(settings: State<'_, SettingsStore>) -> p::layout::LayoutState {
    settings.layout_state()
}

/// UI-DOCK-1: validate a whole layout and persist it in `settings.json`.
/// Never reaches a document, a sidecar file or a recovery record.
#[tauri::command]
pub fn layout_set(
    settings: State<'_, SettingsStore>,
    layout: p::layout::LayoutWire,
) -> p::layout::LayoutSetOutcome {
    settings.set_layout(&layout)
}

// ---- RUST-STAB-1: crash recovery -----------------------------------------

/// Records left by a WRL Forge process that exited without saving.
#[tauri::command]
pub fn recovery_list(svc: State<'_, Service>) -> p::RecoveryList {
    svc.recovery_list()
}

/// Restore one record into a new untitled, unsaved document.
#[tauri::command]
pub fn recovery_restore(svc: State<'_, Service>, id: String) -> p::OpenOutcome {
    svc.recovery_restore(&id)
}

/// The recovery warning to show now (storage unavailable, failed write).
#[tauri::command]
pub fn recovery_status(svc: State<'_, Service>) -> Option<String> {
    svc.recovery().and_then(|r| r.warning())
}

/// Discard one offered record, after a native confirmation.
#[tauri::command]
pub async fn recovery_discard(app: AppHandle, id: String) -> p::RecoveryDiscardOutcome {
    let svc = app.state::<Service>();
    let list = svc.recovery_list();
    let what = if let Some(e) = list.entries.iter().find(|e| e.id == id) {
        format!("the recovered work for \"{}\"", e.name)
    } else if list.damaged.iter().any(|d| d.id == id) {
        "this damaged recovery record".to_string()
    } else {
        return p::RecoveryDiscardOutcome::Failed {
            message: "that recovery record is no longer offered".into(),
        };
    };
    let yes = match app.state::<SmokeState>().confirm_override() {
        Some(answer) => answer,
        None => app
            .dialog()
            .message(format!("Discard {what}? This cannot be undone."))
            .title("Discard recovered work")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Discard".into(),
                "Cancel".into(),
            ))
            .blocking_show(),
    };
    if !yes {
        return p::RecoveryDiscardOutcome::Canceled;
    }
    match svc.recovery_discard(&id) {
        Ok(()) => p::RecoveryDiscardOutcome::Discarded,
        Err(message) => p::RecoveryDiscardOutcome::Failed { message },
    }
}

/// RUST-STAB-1 smoke only.
#[tauri::command]
pub fn smoke_recovery_plan() -> Option<crate::smoke_recovery::Plan> {
    crate::smoke_recovery::plan()
}

/// UI-DOCK-1: the docking smoke plan (`--smoke-dock*` only).
#[tauri::command]
pub fn smoke_dock_plan() -> Option<crate::smoke_dock::Plan> {
    crate::smoke_dock::plan()
}

#[tauri::command]
pub fn smoke_recovery_check(
    app: AppHandle,
    session: Option<p::SessionId>,
    what: String,
    name: String,
) -> Result<p::SmokeStep, String> {
    if !crate::smoke_recovery::armed() {
        return Err("not a recovery smoke run".into());
    }
    Ok(crate::smoke_recovery::check(&app, session, &what, &name))
}

/// Ask the main window to close, exactly like the title-bar button.
#[tauri::command]
pub fn smoke_request_close(app: AppHandle) -> Result<(), String> {
    if !crate::smoke_recovery::armed() {
        return Err("not a recovery smoke run".into());
    }
    let w = app.get_webview_window("main").ok_or("no main window")?;
    w.close().map_err(|e| e.to_string())
}

/// UI-MENU-1: File → Quit, Ctrl+Q (⌘Q) and the macOS application menu.
/// It closes the main window exactly like the title-bar button, so the
/// `CloseRequested` handler and its [`CloseGate`] decide: unsaved work is
/// asked about, and Cancel keeps the application open and the recovery
/// records as they are. There is no second close path.
#[tauri::command]
pub fn app_quit(app: AppHandle) -> Result<(), String> {
    let w = app.get_webview_window("main").ok_or("no main window")?;
    w.close().map_err(|e| e.to_string())
}

/// UI-MENU-1: the menu smoke plan (`--smoke-menu` only).
#[tauri::command]
pub fn smoke_menu_plan() -> Option<crate::smoke_menu::Plan> {
    crate::smoke_menu::plan()
}

/// UI-MENU-1 smoke: the recovery records (name, length, hash).
#[tauri::command]
pub fn smoke_menu_recovery_digest() -> Result<Vec<(String, u64, String)>, String> {
    if !crate::smoke_menu::armed() {
        return Err("not a menu smoke run".into());
    }
    Ok(crate::smoke_menu::recovery_digest())
}

/// UI-MENU-1 smoke: send a native application-menu event, as a macOS menu
/// item does (`app_menu::forward`). `raw` sends the key unchecked, so the
/// run proves the UI validates it too.
#[tauri::command]
pub fn smoke_menu_native(app: AppHandle, key: String, raw: bool) -> Result<bool, String> {
    if !crate::smoke_menu::armed() {
        return Err("not a menu smoke run".into());
    }
    if raw {
        use tauri::Emitter;
        return app
            .emit(p::command::MENU_EVENT, key)
            .map(|_| true)
            .map_err(|e| e.to_string());
    }
    Ok(crate::app_menu::forward(&app, &key))
}

/// UI-MENU-1 smoke: a file of the run as it is on disk.
#[tauri::command]
pub fn smoke_menu_disk(name: String) -> Result<crate::smoke_menu::Disk, String> {
    crate::smoke_menu::disk(&name)
}

/// Window close with unsaved documents: ask once; Discard removes their
/// recovery copies and closes, Cancel keeps everything as it is.
#[derive(Default)]
pub struct CloseGate {
    confirmed: std::sync::atomic::AtomicBool,
    asking: std::sync::atomic::AtomicBool,
}

impl CloseGate {
    pub fn confirmed(&self) -> bool {
        self.confirmed.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub fn confirm_close(window: tauri::Window, dirty: Vec<(p::SessionId, String)>) {
    use std::sync::atomic::Ordering;
    let app = window.app_handle().clone();
    let gate = app.state::<CloseGate>();
    if gate.asking.swap(true, Ordering::SeqCst) {
        return;
    }
    let names: Vec<String> = dirty.iter().map(|(_, n)| format!("\"{n}\"")).collect();
    let yes = match app.state::<SmokeState>().confirm_override() {
        Some(answer) => answer,
        None => app
            .dialog()
            .message(format!(
                "{} {} unsaved changes. Discard them and close WRL Forge?",
                names.join(", "),
                if names.len() == 1 { "has" } else { "have" }
            ))
            .title("Close WRL Forge")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Discard changes".into(),
                "Cancel".into(),
            ))
            .blocking_show(),
    };
    crate::smoke_recovery::note_close_asked(yes);
    if yes {
        let svc = app.state::<Service>();
        for (id, _) in &dirty {
            let _ = svc.discard_recovery(*id);
        }
        gate.confirmed.store(true, Ordering::SeqCst);
        let _ = window.close();
    }
    gate.asking.store(false, Ordering::SeqCst);
}

