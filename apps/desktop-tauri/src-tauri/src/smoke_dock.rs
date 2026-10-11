// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-DOCK-1 docking smoke (`smoke.sh --dock`). Three processes over one
//! disposable directory and one disposable config directory:
//!
//! 1. `--smoke-dock DIR`: the UI drives docking, tabs, dividers, presets,
//!    close / restore and the safety cases with REAL X input. On finish Rust
//!    checks that the fixture on disk is byte-identical, that the layout is
//!    in `settings.json` and NOT in any `.wrl` or recovery record, and writes
//!    the persisted layout to `DIR/expected-layout.json`.
//! 2. `--smoke-dock-restore DIR`: a restart must restore exactly that layout.
//! 3. `--smoke-dock-damaged DIR`: `smoke.sh` damages the saved layout first;
//!    the app must start, use the Source preset and show a notice.
//!
//! Test-only. Every directory must be under the system temp directory.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::layout::{Layout, LayoutWire};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Main,
    Restore,
    Damaged,
}

/// The plan the UI receives (`smoke_dock_plan`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub phase: Phase,
    /// Real X input is available (the harness's own Xvfb).
    pub real_pointer: bool,
    /// Restore phase: the layout the first process persisted.
    pub expected: Option<LayoutWire>,
}

struct Arm {
    dir: PathBuf,
    phase: Phase,
    report_path: Option<PathBuf>,
    fixture: PathBuf,
    original: Vec<u8>,
    settings: Option<PathBuf>,
    config: Option<PathBuf>,
    expected: Option<LayoutWire>,
}

static ARM: Mutex<Option<Arm>> = Mutex::new(None);

pub const FIXTURE: &str = "dock-world.wrl";
const EXPECTED: &str = "expected-layout.json";

/// Two movable Transforms, a Viewpoint, Unicode, and enough lines to scroll.
pub fn fixture_text() -> String {
    let mut s = String::from(
        "#VRML V2.0 utf8\n# UI-DOCK-1 fixture ✓ — docking must never change this text\nDEF Crate Transform {\n  translation 0 0 0\n  children [ Shape { appearance Appearance { material Material { diffuseColor 0.8 0.3 0.2 } } geometry Box { } } ]\n}\nDEF Ball Transform {\n  translation 3 0 0\n  children [ Shape { appearance Appearance { material Material { diffuseColor 0.2 0.5 0.9 } } geometry Sphere { } } ]\n}\nViewpoint { position 1.5 0 10 description \"dock ✓\" }\n",
    );
    for i in 1..=140 {
        s.push_str(&format!("# scroll line {i:03} — Ünïcödé ✓\n"));
    }
    s.push_str("# fin\n");
    s
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
    phase: Phase,
    report_path: Option<PathBuf>,
    settings: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Result<(), String> {
    let dir = temp_dir_checked(dir)?;
    let fixture = dir.join(FIXTURE);
    if phase == Phase::Main {
        if fixture.exists() {
            return Err(format!("{} already exists", fixture.display()));
        }
        std::fs::write(&fixture, fixture_text()).map_err(|e| e.to_string())?;
    } else if !fixture.exists() {
        std::fs::write(&fixture, fixture_text()).map_err(|e| e.to_string())?;
    }
    let original = std::fs::read(&fixture).map_err(|e| e.to_string())?;
    let expected = match phase {
        Phase::Restore => {
            let b = std::fs::read(dir.join(EXPECTED)).map_err(|e| format!("{EXPECTED}: {e}"))?;
            Some(serde_json::from_slice(&b).map_err(|e| format!("{EXPECTED}: {e}"))?)
        }
        _ => None,
    };
    *ARM.lock().unwrap() = Some(Arm {
        dir,
        phase,
        report_path,
        fixture,
        original,
        settings,
        config,
        expected,
    });
    Ok(())
}

pub fn armed() -> bool {
    ARM.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn plan() -> Option<Plan> {
    let g = ARM.lock().ok()?;
    let a = g.as_ref()?;
    Some(Plan {
        phase: a.phase,
        real_pointer: crate::smoke::real_pointer_available(),
        expected: a.expected.clone(),
    })
}

/// The fixture, opened by index (0) like the other smoke runs.
pub fn fixture(index: usize) -> Option<PathBuf> {
    let g = ARM.lock().ok()?;
    let a = g.as_ref()?;
    (index == 0).then(|| a.fixture.clone())
}

fn step(name: &str, ok: bool, detail: impl Into<String>) -> p::SmokeStep {
    p::SmokeStep {
        name: name.into(),
        ok,
        detail: detail.into(),
    }
}

/// Files under `dir` (recursive), excluding `skip`.
fn files_under(dir: &Path, skip: &[&Path], out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        if skip.iter().any(|s| *s == path) {
            continue;
        }
        if path.is_dir() {
            files_under(&path, skip, out);
        } else {
            out.push(path);
        }
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn verify(a: &Arm) -> Vec<p::SmokeStep> {
    let mut steps = Vec::new();
    let now = std::fs::read(&a.fixture).unwrap_or_default();
    steps.push(step(
        "Rust: the fixture on disk is byte-identical (docking never wrote it)",
        now == a.original,
        format!("{} bytes", now.len()),
    ));
    // The layout lives in settings.json and nowhere else.
    let mut files = Vec::new();
    let skip: Vec<&Path> = a.settings.iter().map(|p| p.as_path()).collect();
    files_under(&a.dir, &skip, &mut files);
    if let Some(c) = &a.config {
        if !c.starts_with(&a.dir) {
            files_under(c, &skip, &mut files);
        }
    }
    let leaks: Vec<String> = files
        .iter()
        .filter(|f| {
            f.file_name().is_some_and(|n| {
                n != EXPECTED
                    && !n.to_string_lossy().ends_with(".json")
                    && !n.to_string_lossy().ends_with(".txt")
            })
        })
        .filter(|f| {
            std::fs::read(f).is_ok_and(|b| {
                contains(&b, b"panel.sceneTree")
                    || contains(&b, b"preset.source")
                    || contains(&b, b"preset.modeling")
                    || contains(&b, b"\"layout\"")
            })
        })
        .map(|f| f.display().to_string())
        .collect();
    let recs = files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "wfr"))
        .count();
    steps.push(step(
        "Rust: no layout data in any .wrl, .wrz or recovery record",
        leaks.is_empty(),
        format!(
            "{} files checked ({recs} recovery records){}",
            files.len(),
            if leaks.is_empty() {
                String::new()
            } else {
                format!("; leaks: {}", leaks.join(", "))
            }
        ),
    ));
    let saved = a
        .settings
        .as_ref()
        .and_then(|s| std::fs::read(s).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let wire = saved
        .as_ref()
        .and_then(|v| v.get(crate::settings::LAYOUT_KEY))
        .and_then(|l| serde_json::from_value::<LayoutWire>(l.clone()).ok());
    let valid = wire
        .as_ref()
        .is_some_and(|w| Layout::from_wire(w).is_ok_and(|(_, u)| u.is_empty()));
    steps.push(step(
        "Rust: settings.json holds a valid, versioned layout next to the theme",
        valid
            && saved
                .as_ref()
                .is_some_and(|v| v.get("themeId").is_some() && v.get("schemaVersion").is_some()),
        format!("{:?}", saved.as_ref().map(|v| v.get("themeId").cloned())),
    ));
    if a.phase == Phase::Main {
        if let Some(w) = &wire {
            let ok = std::fs::write(
                a.dir.join(EXPECTED),
                serde_json::to_vec_pretty(w).unwrap_or_default(),
            )
            .is_ok();
            steps.push(step(
                "Rust: expected layout recorded for the restart",
                ok,
                EXPECTED,
            ));
        }
    }
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
        "phase": a.phase,
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
