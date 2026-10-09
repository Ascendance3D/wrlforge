// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge desktop (Tauri 2). TAURI-RUST-MIGRATION-1.
//!
//! No Electron, no Node.js runtime. File ownership, the canonical document,
//! parsing and projections are native Rust; the WebView hosts the Rust/Wasm UI
//! and (temporarily) the X_ITE preview.

pub mod commands;
pub mod files;
pub mod service;
pub mod smoke;

use std::path::PathBuf;

use tauri::Manager;

struct Args {
    open: Option<PathBuf>,
    smoke: Option<PathBuf>,
    smoke_report: Option<PathBuf>,
    smoke_no_preview: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        open: None,
        smoke: None,
        smoke_report: None,
        smoke_no_preview: false,
    };
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--smoke") => a.smoke = it.next().map(PathBuf::from),
            Some("--smoke-report") => a.smoke_report = it.next().map(PathBuf::from),
            Some("--smoke-no-preview") => a.smoke_no_preview = true,
            _ if a.open.is_none() => a.open = Some(PathBuf::from(arg)),
            _ => {}
        }
    }
    a
}

pub fn run() {
    let args = parse_args();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(service::Service::default())
        .manage(commands::Startup::default())
        .manage(smoke::SmokeState::default())
        .setup(move |app| {
            let target = args.smoke.clone().or(args.open.clone());
            if let Some(path) = target {
                let outcome = app.state::<service::Service>().open_path(&path);
                *app.state::<commands::Startup>().0.lock().unwrap() = Some(outcome);
            }
            if let Some(path) = args.smoke.clone() {
                app.state::<smoke::SmokeState>()
                    .arm(path, args.smoke_report.clone(), !args.smoke_no_preview)
                    .map_err(|e| format!("smoke: {e}"))?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::startup_document,
            commands::open_document,
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
            commands::doc_preview_source,
            commands::window_title,
            commands::smoke_plan,
            commands::smoke_finish,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WRL Forge");
}
