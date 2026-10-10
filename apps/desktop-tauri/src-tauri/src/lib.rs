// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge desktop (Tauri 2). TAURI-RUST-MIGRATION-1.
//!
//! No Electron, no Node.js runtime. File ownership, the canonical document,
//! parsing and projections are native Rust; the WebView hosts the Rust/Wasm UI
//! and (temporarily) the X_ITE preview.

pub mod commands;
pub mod files;
pub mod native;
pub mod resources;
pub mod service;
pub mod settings;
pub mod smoke;
pub mod viewport;

use std::path::PathBuf;

use tauri::Manager;
use wrlforge_desktop_protocol as p;

struct Args {
    open: Option<PathBuf>,
    smoke: Option<PathBuf>,
    smoke_report: Option<PathBuf>,
    /// `--smoke-create <dir>`: the VISUAL-1 New World → Create workflow.
    smoke_create: Option<PathBuf>,
    /// `--smoke-pick <dir>`: the VISUAL-2 viewport picking run.
    smoke_pick: Option<PathBuf>,
    /// `--smoke-move <dir>`: the VISUAL-3A translation-gizmo run.
    smoke_move: Option<PathBuf>,
    /// `--smoke-texture <dir>`: the TEXTURE-LOCAL-1 local texture run.
    smoke_texture: Option<PathBuf>,
    smoke_no_preview: bool,
    smoke_inspector: bool,
    smoke_theme: Option<p::ThemeSmoke>,
    /// Override the settings directory (tests only; never the user's).
    config_dir: Option<PathBuf>,
    /// NATIVE-RENDER-1 test flag: start the hidden native viewport.
    native_viewport: bool,
    /// Test flag: pretend the surface offers only FIFO (D8 path).
    native_fifo_only: bool,
    /// `--smoke-native <dir>`: the NATIVE-RENDER-1 self-test run.
    smoke_native: Option<PathBuf>,
    /// With `--smoke-native`: the X_ITE-only control (no native viewport).
    smoke_native_control: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        open: None,
        smoke: None,
        smoke_report: None,
        smoke_create: None,
        smoke_pick: None,
        smoke_move: None,
        smoke_texture: None,
        smoke_no_preview: false,
        smoke_inspector: false,
        smoke_theme: None,
        config_dir: None,
        native_viewport: false,
        native_fifo_only: false,
        smoke_native: None,
        smoke_native_control: false,
    };
    let mut theme_final = None;
    let mut theme_expect = None;
    let (mut theme_notice, mut theme_fails) = (false, false);
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--smoke") => a.smoke = it.next().map(PathBuf::from),
            Some("--smoke-report") => a.smoke_report = it.next().map(PathBuf::from),
            Some("--smoke-create") => a.smoke_create = it.next().map(PathBuf::from),
            Some("--smoke-pick") => a.smoke_pick = it.next().map(PathBuf::from),
            Some("--smoke-move") => a.smoke_move = it.next().map(PathBuf::from),
            Some("--smoke-texture") => a.smoke_texture = it.next().map(PathBuf::from),
            Some("--smoke-no-preview") => a.smoke_no_preview = true,
            Some("--smoke-inspector") => a.smoke_inspector = true,
            Some("--smoke-theme") => theme_final = it.next().and_then(|s| s.into_string().ok()),
            Some("--smoke-theme-expect") => {
                theme_expect = it.next().and_then(|s| s.into_string().ok())
            }
            Some("--smoke-theme-notice") => theme_notice = true,
            Some("--smoke-theme-save-fails") => theme_fails = true,
            Some("--config-dir") => a.config_dir = it.next().map(PathBuf::from),
            Some("--native-viewport") => a.native_viewport = true,
            Some("--native-force-fifo-only") => a.native_fifo_only = true,
            Some("--smoke-native") => a.smoke_native = it.next().map(PathBuf::from),
            Some("--smoke-native-control") => a.smoke_native_control = true,
            _ if a.open.is_none() => a.open = Some(PathBuf::from(arg)),
            _ => {}
        }
    }
    a.smoke_theme = theme_final.map(|final_theme| p::ThemeSmoke {
        expect_startup: theme_expect.unwrap_or_else(|| p::theme::DEFAULT_THEME.into()),
        expect_notice: theme_notice,
        final_theme,
        expect_save_failure: theme_fails,
    });
    a
}

pub fn run() {
    let args = parse_args();
    let mut context = tauri::generate_context!();
    if args.smoke_native.is_some() {
        // TEST-ONLY: a native self-test never takes the keyboard focus.
        for w in context.config_mut().app.windows.iter_mut() {
            w.focus = false;
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(service::Service::default())
        .manage(commands::Startup::default())
        .manage(smoke::SmokeState::default())
        .manage(native::Native::default())
        // TEXTURE-LOCAL-1: read-only preview resources (see resources.rs).
        // The WebView names a token, never a folder; Rust decides the file.
        .register_asynchronous_uri_scheme_protocol(resources::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            let origin = request
                .headers()
                .get("origin")
                .and_then(|o| o.to_str().ok())
                .filter(|o| matches!(*o, "tauri://localhost" | "http://tauri.localhost"))
                .map(str::to_string);
            std::thread::spawn(move || {
                let served = resources::serve(&app.state::<service::Service>(), &path);
                let mut b = tauri::http::Response::builder()
                    .status(served.status)
                    .header("Content-Type", served.mime)
                    .header("Cache-Control", "no-store")
                    .header("X-Content-Type-Options", "nosniff");
                if let Some(o) = origin {
                    b = b.header("Access-Control-Allow-Origin", o);
                }
                let resp = b.body(served.body).unwrap_or_else(|_| {
                    tauri::http::Response::builder()
                        .status(500)
                        .body(Vec::new())
                        .unwrap()
                });
                responder.respond(resp);
            });
        })
        .setup(move |app| {
            // Application preferences: Tauri's per-app config directory.
            let store = match args.config_dir.clone() {
                Some(d) => settings::SettingsStore::open(d),
                None => match app.path().app_config_dir() {
                    Ok(d) => settings::SettingsStore::open(d),
                    Err(e) => settings::SettingsStore::unavailable(&e.to_string()),
                },
            };
            let settings_path = store.path();
            let native_requested =
                (args.native_viewport || args.smoke_native.is_some() || store.native_viewport())
                    && !args.smoke_native_control;
            app.manage(store);
            let target = args.smoke.clone().or(args.open.clone());
            if let Some(path) = target {
                let outcome = app.state::<service::Service>().open_path(&path);
                *app.state::<commands::Startup>().0.lock().unwrap() = Some(outcome);
            }
            if let Some(dir) = args.smoke_create.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_create(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-create: {e}"))?;
            }
            if let Some(dir) = args.smoke_pick.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_pick(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-pick: {e}"))?;
            }
            if let Some(dir) = args.smoke_move.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_move(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-move: {e}"))?;
            }
            if let Some(dir) = args.smoke_texture.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_texture(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-texture: {e}"))?;
            }
            if let Some(path) = args.smoke.clone() {
                app.state::<smoke::SmokeState>()
                    .arm(
                        path,
                        args.smoke_report.clone(),
                        !args.smoke_no_preview,
                        args.smoke_inspector,
                        args.smoke_theme.clone(),
                        settings_path,
                    )
                    .map_err(|e| format!("smoke: {e}"))?;
            }
            // NATIVE-RENDER-1 (hidden): last, so the window layout exists.
            let installed = viewport::install(
                app.handle(),
                viewport::Options {
                    requested: native_requested,
                    force_fifo_only: args.native_fifo_only,
                },
            );
            app.state::<native::Native>()
                .active
                .store(installed, std::sync::atomic::Ordering::SeqCst);
            if let Some(dir) = args.smoke_native.clone() {
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                viewport::smoke::arm(
                    app.handle(),
                    dir,
                    args.smoke_report.clone(),
                    args.smoke_native_control,
                );
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                let _ = dir;
            }
            Ok(())
        })
        .on_window_event(|_window, event| {
            // The native viewport's window must outlive its render thread.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if viewport::defer_close(0) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::startup_document,
            commands::open_document,
            commands::new_document,
            commands::doc_create,
            commands::close_document,
            commands::doc_snapshot,
            commands::doc_edit,
            commands::doc_undo,
            commands::doc_redo,
            commands::doc_save,
            commands::doc_save_as,
            commands::doc_check_external,
            commands::doc_reload,
            commands::doc_analyze,
            commands::doc_inspect,
            commands::doc_edit_field,
            commands::doc_preview_source,
            commands::native_state,
            commands::native_show,
            commands::native_select,
            commands::native_reset_camera,
            commands::doc_pick,
            commands::doc_translate_target,
            commands::doc_translate_prove,
            commands::doc_preview_carry,
            commands::doc_translate,
            commands::window_title,
            commands::theme_get,
            commands::theme_set,
            commands::smoke_plan,
            commands::smoke_open_fixture,
            commands::smoke_real_click,
            commands::smoke_real_pointer,
            commands::smoke_real_resize,
            commands::smoke_finish,
            commands::smoke_texture_delay,
            commands::smoke_texture_capture,
            commands::smoke_resource_log,
        ])
        .build(context)
        .expect("error while building WRL Forge")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if viewport::defer_close(code.unwrap_or(0)) {
                    api.prevent_exit();
                }
            }
        });
}
