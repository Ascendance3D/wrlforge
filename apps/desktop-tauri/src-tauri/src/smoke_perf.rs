// SPDX-License-Identifier: GPL-3.0-or-later
//! Viewport frame-timing smoke (`--smoke-perf <dir> --smoke-perf-file <wrl>`).
//! Copies one model into the disposable `dir`, opens the copy and records
//! animation-frame statistics while the UI drives orbit (real X input where
//! the harness allows it), selection, camera commands and Inspector edits.
//! The run reports numbers; it compares GPU and software rendering only
//! when the harness runs it twice. It never saves: the copy must stay
//! byte-identical on disk.
//!
//! Test-only. The directory must be under the system temp directory.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use wrlforge_desktop_protocol as p;

/// The plan the UI receives (`smoke_perf_plan`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub real_pointer: bool,
}

struct Arm {
    report_path: Option<PathBuf>,
    fixture: PathBuf,
    original: Vec<u8>,
}

static ARM: Mutex<Option<Arm>> = Mutex::new(None);

pub const FIXTURE: &str = "perf-world.wrl";

fn temp_dir_checked(dir: &Path) -> Result<PathBuf, String> {
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let tmp = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !dir.starts_with(&tmp) || dir == tmp || !dir.is_dir() {
        return Err(format!(
            "refusing a non-temporary directory: {}",
            dir.display()
        ));
    }
    Ok(dir)
}

pub fn arm(dir: &Path, report_path: Option<PathBuf>, model: &Path) -> Result<(), String> {
    let dir = temp_dir_checked(dir)?;
    let fixture = dir.join(FIXTURE);
    if fixture.exists() {
        return Err(format!("{} already exists", fixture.display()));
    }
    let original = std::fs::read(model).map_err(|e| format!("{}: {e}", model.display()))?;
    std::fs::write(&fixture, &original).map_err(|e| e.to_string())?;
    *ARM.lock().unwrap() = Some(Arm {
        report_path,
        fixture,
        original,
    });
    Ok(())
}

pub fn armed() -> bool {
    ARM.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn plan() -> Option<Plan> {
    ARM.lock().ok()?.as_ref()?;
    Some(Plan {
        real_pointer: crate::smoke::real_pointer_available(),
    })
}

pub fn fixture(index: usize) -> Option<PathBuf> {
    let g = ARM.lock().ok()?;
    let a = g.as_ref()?;
    (index == 0).then(|| a.fixture.clone())
}

pub fn finish(app: &AppHandle, report: &p::SmokeReport) -> bool {
    let Some(a) = ARM.lock().ok().and_then(|mut g| g.take()) else {
        return false;
    };
    let mut steps = report.steps.clone();
    let disk = std::fs::read(&a.fixture).unwrap_or_default();
    steps.push(p::SmokeStep {
        name: "Rust: the model copy on disk is byte-identical (the run never saves)".into(),
        ok: disk == a.original,
        detail: format!("{} B on disk, {} B at arm", disk.len(), a.original.len()),
    });
    let all = !steps.is_empty() && steps.iter().all(|s| s.ok);
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "pass": all,
        "suite": "perf",
        "steps": steps,
    }))
    .unwrap();
    println!("{json}");
    if let Some(rp) = a.report_path {
        let _ = std::fs::write(rp, &json);
    }
    app.exit(if all { 0 } else { 1 });
    true
}
