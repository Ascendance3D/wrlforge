// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-MENU-1 menu smoke (`smoke.sh --menu`). One process over one disposable
//! directory: the UI drives the menu bar, the shortcuts and the toolbar with
//! REAL X input where the harness's Xvfb allows it, and compares the results
//! of equivalent commands. Rust answers the questions the UI cannot check by
//! itself: the bytes on disk, the recovery records, the Save As choices and
//! the close / discard confirmations (always Cancel here).
//!
//! Test-only. Every directory must be under the system temp directory.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use wrlforge_desktop_protocol as p;

/// The plan the UI receives (`smoke_menu_plan`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub real_pointer: bool,
    /// Save As destinations, in call order (relative names in the run dir).
    pub save_as: Vec<Option<String>>,
}

struct Arm {
    dir: PathBuf,
    report_path: Option<PathBuf>,
    fixture: PathBuf,
    original: Vec<u8>,
    config: Option<PathBuf>,
    save_as: VecDeque<Option<PathBuf>>,
}

static ARM: Mutex<Option<Arm>> = Mutex::new(None);
/// Close / discard questions asked (each answered Cancel).
static ASKED: AtomicU32 = AtomicU32::new(0);

pub const FIXTURE: &str = "menu-world.wrl";
const SAVE_AS: [Option<&str>; 3] = [Some("menu-save-as.wrl"), Some("toolbar-save-as.wrl"), None];

/// A Box Transform, an Extrusion with an editable crossSection, a Viewpoint.
pub fn fixture_text() -> String {
    "#VRML V2.0 utf8\n# UI-MENU-1 fixture ✓\nDEF Crate Transform {\n  translation 0 0 0\n  children [ Shape { appearance Appearance { material Material { diffuseColor 0.8 0.3 0.2 } } geometry Box { } } ]\n}\nDEF Wall Transform {\n  translation 3 0 0\n  children [ Shape { appearance Appearance { material Material { diffuseColor 0.2 0.5 0.9 } }\n    geometry Extrusion { crossSection [ 1 0, 0 1, -1 0, 0 -1, 1 0 ] spine [ 0 0 0, 0 1 0 ] } } ]\n}\nViewpoint { position 1.5 0 10 description \"menu ✓\" }\n# fin\n".into()
}

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

pub fn arm(
    dir: &Path,
    report_path: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Result<(), String> {
    let dir = temp_dir_checked(dir)?;
    let fixture = dir.join(FIXTURE);
    if fixture.exists() {
        return Err(format!("{} already exists", fixture.display()));
    }
    std::fs::write(&fixture, fixture_text()).map_err(|e| e.to_string())?;
    let original = std::fs::read(&fixture).map_err(|e| e.to_string())?;
    let save_as = SAVE_AS.iter().map(|n| n.map(|n| dir.join(n))).collect();
    *ARM.lock().unwrap() = Some(Arm {
        dir,
        report_path,
        fixture,
        original,
        config,
        save_as,
    });
    Ok(())
}

pub fn armed() -> bool {
    ARM.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn plan() -> Option<Plan> {
    let g = ARM.lock().ok()?;
    g.as_ref()?;
    Some(Plan {
        real_pointer: crate::smoke::real_pointer_available(),
        save_as: SAVE_AS.iter().map(|n| n.map(str::to_string)).collect(),
    })
}

pub fn fixture(index: usize) -> Option<PathBuf> {
    let g = ARM.lock().ok()?;
    let a = g.as_ref()?;
    (index == 0).then(|| a.fixture.clone())
}

/// The next Save As choice: `Some(Some(path))` saves there, `Some(None)` is
/// a canceled dialog, `None` means this run is not armed.
pub fn save_as_choice() -> Option<Option<PathBuf>> {
    let mut g = ARM.lock().ok()?;
    let a = g.as_mut()?;
    Some(a.save_as.pop_front().unwrap_or(None))
}

/// Every close / discard question in this run is answered Cancel.
pub fn confirm() -> Option<bool> {
    if !armed() {
        return None;
    }
    ASKED.fetch_add(1, Ordering::SeqCst);
    Some(false)
}

fn recovery_dir() -> Option<PathBuf> {
    let g = ARM.lock().ok()?;
    let a = g.as_ref()?;
    Some(a.config.as_ref()?.join(crate::recovery::DIR_NAME))
}

/// The recovery records now: (file name, length, content hash), sorted.
pub fn recovery_digest() -> Vec<(String, u64, String)> {
    let Some(d) = recovery_dir() else {
        return Vec::new();
    };
    let mut out: Vec<(String, u64, String)> = std::fs::read_dir(&d)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let b = std::fs::read(e.path()).ok()?;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            b.hash(&mut h);
            Some((
                e.file_name().to_string_lossy().into_owned(),
                b.len() as u64,
                format!("{:016x}", h.finish()),
            ))
        })
        .collect();
    out.sort();
    out
}

/// What the run dir holds, by name: the fixture or a Save As target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub name: String,
    pub text: Option<String>,
    pub asked: u32,
    pub files: Vec<String>,
}

pub fn disk(name: &str) -> Result<Disk, String> {
    let g = ARM.lock().map_err(|e| e.to_string())?;
    let a = g.as_ref().ok_or("not a menu smoke run")?;
    let allowed = name == FIXTURE || SAVE_AS.iter().flatten().any(|n| *n == name);
    if !allowed {
        return Err(format!("{name:?} is not a file of this run"));
    }
    let text = std::fs::read(a.dir.join(name))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    let mut files: Vec<String> = std::fs::read_dir(&a.dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    Ok(Disk {
        name: name.into(),
        text,
        asked: ASKED.load(Ordering::SeqCst),
        files,
    })
}

fn step(name: &str, ok: bool, detail: impl Into<String>) -> p::SmokeStep {
    p::SmokeStep {
        name: name.into(),
        ok,
        detail: detail.into(),
    }
}

fn verify(a: &Arm) -> Vec<p::SmokeStep> {
    let now = std::fs::read(&a.fixture).unwrap_or_default();
    let text = String::from_utf8_lossy(&now);
    let mut steps = vec![step(
        "Rust: the fixture on disk is a VRML97 file the run saved (not corrupted)",
        text.starts_with("#VRML V2.0 utf8") && text.contains("DEF Crate Transform"),
        format!("{} bytes (original {})", now.len(), a.original.len()),
    )];
    // Nothing but the fixture, its backups and the planned Save As targets.
    let stray: Vec<String> = std::fs::read_dir(&a.dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n != FIXTURE
                && !n.starts_with(&format!("{FIXTURE}.bak"))
                && !SAVE_AS
                    .iter()
                    .flatten()
                    .any(|s| n == s || n.starts_with(&format!("{s}.bak")))
                && !n.ends_with(".json")
                && !n.ends_with(".txt")
        })
        .collect();
    steps.push(step(
        "Rust: a canceled Save As wrote no file",
        stray.is_empty(),
        format!("unexpected: {stray:?}"),
    ));
    steps
}

pub fn finish(app: &AppHandle, report: &p::SmokeReport) -> bool {
    let Some(a) = ARM.lock().ok().and_then(|mut g| g.take()) else {
        return false;
    };
    let mut steps = report.steps.clone();
    steps.extend(verify(&a));
    let all = !steps.is_empty() && steps.iter().all(|s| s.ok);
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "pass": all,
        "suite": "menu",
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
