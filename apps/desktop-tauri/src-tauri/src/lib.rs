// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge desktop (Tauri 2). TAURI-RUST-MIGRATION-1.
//!
//! No Electron, no Node.js runtime. File ownership, the canonical document,
//! parsing and projections are native Rust; the WebView hosts the Rust/Wasm UI
//! and (temporarily) the X_ITE preview.

pub mod commands;
pub mod files;
pub mod native;
pub mod recovery;
pub mod resources;
pub mod service;
pub mod settings;
pub mod smoke;
pub mod smoke_dock;
pub mod smoke_menu;
mod app_menu;
pub mod smoke_recovery;
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
    /// `--smoke-extrusion <dir>`: the VISUAL-4A Extrusion run.
    smoke_extrusion: Option<PathBuf>,
    /// `--smoke-rotscale <dir>`: the VISUAL-4B Rotate / Scale run.
    smoke_rotscale: Option<PathBuf>,
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
    /// RUST-STAB-1: `--smoke-recovery-{crash,restore,later} <dir>`.
    smoke_recovery: Option<(smoke_recovery::Phase, PathBuf)>,
    /// UI-DOCK-1: `--smoke-dock[-restore|-damaged] <dir>`.
    smoke_dock: Option<(smoke_dock::Phase, PathBuf)>,
    /// UI-MENU-1: `--smoke-menu <dir>`.
    smoke_menu: Option<PathBuf>,
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
        smoke_extrusion: None,
        smoke_rotscale: None,
        smoke_no_preview: false,
        smoke_inspector: false,
        smoke_theme: None,
        config_dir: None,
        native_viewport: false,
        native_fifo_only: false,
        smoke_native: None,
        smoke_native_control: false,
        smoke_recovery: None,
        smoke_dock: None,
        smoke_menu: None,
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
            Some("--smoke-extrusion") => a.smoke_extrusion = it.next().map(PathBuf::from),
            Some("--smoke-rotscale") => a.smoke_rotscale = it.next().map(PathBuf::from),
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
            Some("--smoke-dock") => {
                a.smoke_dock = it
                    .next()
                    .map(|d| (smoke_dock::Phase::Main, PathBuf::from(d)))
            }
            Some("--smoke-dock-restore") => {
                a.smoke_dock = it
                    .next()
                    .map(|d| (smoke_dock::Phase::Restore, PathBuf::from(d)))
            }
            Some("--smoke-dock-damaged") => {
                a.smoke_dock = it
                    .next()
                    .map(|d| (smoke_dock::Phase::Damaged, PathBuf::from(d)))
            }
            Some("--smoke-menu") => a.smoke_menu = it.next().map(PathBuf::from),
            Some("--smoke-recovery-crash") => {
                a.smoke_recovery = it.next().map(|d| (smoke_recovery::Phase::Crash, d.into()))
            }
            Some("--smoke-recovery-restore") => {
                a.smoke_recovery = it.next().map(|d| (smoke_recovery::Phase::Restore, d.into()))
            }
            Some("--smoke-recovery-later") => {
                a.smoke_recovery = it.next().map(|d| (smoke_recovery::Phase::Later, d.into()))
            }
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

/// The X_ITE version pinned in `xite/PIN`: the default 3D renderer.
pub fn xite_version() -> &'static str {
    include_str!("../../xite/PIN")
        .lines()
        .find_map(|l| l.strip_prefix("version="))
        .map(str::trim)
        .unwrap_or("unknown")
}

/// APP-SHELL-0: what About shows. The version comes from Cargo; the
/// `config_version` is what Tauri read from `tauri.conf.json`.
pub fn app_info<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> p::command::AppInfo {
    use p::command as c;
    p::command::AppInfo {
        name: c::APP_NAME.into(),
        version: c::APP_VERSION.into(),
        channel: c::APP_CHANNEL.into(),
        display: c::app_display_name(),
        config_version: app.package_info().version.to_string(),
        identifier: app.config().identifier.clone(),
        tauri_version: tauri::VERSION.into(),
        renderer: format!("X_ITE {}", xite_version()),
        license: "GPL-3.0-or-later".into(),
    }
}

pub fn run() {
    // APP-SHELL-0: report the identity without opening a window.
    if std::env::args_os().nth(1).is_some_and(|a| a == "--version") {
        println!("{}", p::command::app_display_name());
        return;
    }
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
        .manage(commands::CloseGate::default())
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
            // RUST-STAB-1: crash recovery in the app's private DATA dir (or
            // `<config-dir>/recovery` for tests; never the user's).
            let rec = match args.config_dir.clone() {
                Some(d) => recovery::Recovery::open(&d.join(recovery::DIR_NAME), Default::default()),
                None => match app.path().app_data_dir() {
                    Ok(d) => recovery::Recovery::open(&d.join(recovery::DIR_NAME), Default::default()),
                    Err(e) => recovery::Recovery::unavailable(&e.to_string()),
                },
            };
            let rec = std::sync::Arc::new(rec);
            app.state::<service::Service>().attach_recovery(rec.clone());
            {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("wrlforge-recovery".into())
                    .spawn(move || {
                        while rec.wait_due() {
                            handle.state::<service::Service>().recovery_pass();
                        }
                    })
                    .map_err(|e| format!("recovery worker: {e}"))?;
            }
            if let Some((phase, dir)) = args.smoke_recovery.clone() {
                smoke_recovery::arm(&dir, phase, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-recovery: {e}"))?;
                if phase == smoke_recovery::Phase::Crash {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || smoke_recovery::run_crash(handle));
                }
            }
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
            if let Some(dir) = args.smoke_extrusion.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_extrusion(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-extrusion: {e}"))?;
            }
            if let Some(dir) = args.smoke_rotscale.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_rotscale(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-rotscale: {e}"))?;
            }
            if let Some(dir) = args.smoke_texture.clone() {
                app.state::<smoke::SmokeState>()
                    .arm_texture(&dir, args.smoke_report.clone())
                    .map_err(|e| format!("smoke-texture: {e}"))?;
            }
            if let Some((phase, dir)) = args.smoke_dock.clone() {
                smoke_dock::arm(
                    &dir,
                    phase,
                    args.smoke_report.clone(),
                    settings_path.clone(),
                    args.config_dir.clone(),
                )
                .map_err(|e| format!("smoke-dock: {e}"))?;
            }
            if let Some(dir) = args.smoke_menu.clone() {
                smoke_menu::arm(&dir, args.smoke_report.clone(), args.config_dir.clone())
                    .map_err(|e| format!("smoke-menu: {e}"))?;
            }
            // UI-MENU-1: the standard macOS application menu (About, Hide,
            // Quit; Edit with the platform clipboard items). Other platforms
            // use the in-window menu bar only.
            #[cfg(target_os = "macos")]
            {
                let menu = app_menu::build(app.handle())?;
                app.set_menu(menu)?;
                app.on_menu_event(app_menu::on_event);
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
            // APP-SHELL-0: the window shows the version until a document
            // titles it (`window_title`).
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title(&p::command::app_display_name());
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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // RUST-STAB-1: unsaved work is never closed away silently.
                let app = window.app_handle();
                if !app.state::<commands::CloseGate>().confirmed() {
                    let dirty = app.state::<service::Service>().dirty_sessions();
                    if !dirty.is_empty() {
                        api.prevent_close();
                        let w = window.clone();
                        std::thread::spawn(move || commands::confirm_close(w, dirty));
                        return;
                    }
                }
                // The native viewport's window must outlive its render thread.
                if viewport::defer_close(0) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::startup_document,
            commands::confirm_shared_edit,
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
            commands::doc_edit_list,
            commands::doc_add_field,
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
            commands::doc_rotate,
            commands::doc_scale,
            commands::window_title,
            commands::app_info,
            commands::theme_get,
            commands::theme_set,
            commands::layout_get,
            commands::layout_set,
            commands::smoke_plan,
            commands::smoke_open_fixture,
            commands::smoke_real_click,
            commands::smoke_real_pointer,
            commands::smoke_real_resize,
            commands::smoke_finish,
            commands::smoke_window_capture,
            commands::smoke_texture_delay,
            commands::smoke_texture_capture,
            commands::smoke_resource_log,
            commands::recovery_list,
            commands::recovery_restore,
            commands::recovery_discard,
            commands::recovery_status,
            commands::smoke_recovery_plan,
            commands::smoke_dock_plan,
            commands::smoke_recovery_check,
            commands::smoke_request_close,
            // UI-MENU-1.
            commands::app_quit,
            commands::smoke_menu_plan,
            commands::smoke_menu_recovery_digest,
            commands::smoke_menu_disk,
            commands::smoke_menu_native,
        ])
        .build(context)
        .expect("error while building WRL Forge")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if viewport::defer_close(code.unwrap_or(0)) {
                    api.prevent_exit();
                }
            }
            // RUST-STAB-1: a normal exit writes any scheduled recovery copy
            // (unsaved work that was not explicitly discarded stays offered).
            tauri::RunEvent::Exit => {
                let svc = app.state::<service::Service>();
                svc.recovery_pass();
                if let Some(r) = svc.recovery() {
                    r.stop();
                }
            }
            _ => {}
        });
}
