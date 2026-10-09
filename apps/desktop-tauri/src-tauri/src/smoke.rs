// SPDX-License-Identifier: GPL-3.0-or-later
//! `--smoke <file> [--smoke-report <json>]`: an end-to-end self-test through
//! the REAL window, WebView, Wasm UI and IPC.
//!
//! Rust opens `<file>` (always a disposable copy -- the harness never points
//! this at a user file). The UI drives its own widget: it checks the editor
//! shows the document, types a line through a genuine `input` event, undoes,
//! redoes, saves, selects a Scene Tree item, and tries the preview. It reports
//! each step back; Rust then independently verifies the bytes on disk and the
//! backup, prints one JSON report, and exits 0 (all pass) or 1.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::AppHandle;
use wrlforge_desktop_protocol as p;

use crate::files::{self, Format};
use wrlforge_document::EolCounts;

pub const INSERT: &str = "# smoke: añadido ✓ 😀";
/// The value the `--smoke-inspector` run writes through the Inspector.
pub const INSPECTOR_VALUE: &str = "0.25";

struct Plan {
    path: PathBuf,
    original_bytes: Vec<u8>,
    original_text: String,
    format: Format,
    report_path: Option<PathBuf>,
    expect_preview: bool,
    inspector: bool,
    theme: Option<p::ThemeSmoke>,
    settings_path: Option<PathBuf>,
    /// The settings file as it was before the run (None = absent).
    settings_before: Option<Vec<u8>>,
}

/// `--smoke-create <dir>` (VISUAL-1). The directory is a disposable one
/// under the system temp dir; Rust names the single file the workflow
/// saves, so the native Save As / Open dialogs and the discard confirmation
/// are answered by Rust from this plan, never by the WebView.
struct CreateArm {
    save_path: PathBuf,
    report_path: Option<PathBuf>,
    /// New World confirmations, in order: Cancel first, then Discard.
    confirms: VecDeque<bool>,
    /// Confirmations asked so far (Rust checks the count at the end).
    asked: usize,
}

pub const CREATE_FILE: &str = "visual1-world.wrl";
pub const CREATE_TRANSLATION: [&str; 3] = ["1.5", "0.5", "-1"];
pub const CREATE_COLOR: [&str; 3] = ["0.1", "0.6", "0.9"];

/// `--smoke-pick <dir>` (VISUAL-2). `dir` holds `plan.json` and the
/// fixture files `smoke-pick-plan.cjs` wrote. The UI opens a fixture by
/// INDEX; the path never leaves Rust.
struct PickArm {
    report_path: Option<PathBuf>,
    save_path: PathBuf,
    /// (path, the exact bytes at arm time): picks must never change them.
    files: Vec<(PathBuf, Vec<u8>)>,
    fixtures: Vec<p::PickFixture>,
}

pub const PICK_FILE: &str = "visual2-world.wrl";

/// `--smoke-move <dir>` (VISUAL-3A). Rust writes the source-format fixtures
/// into the disposable `dir` and keeps their original bytes; the UI opens
/// them by INDEX and moves one object in each, then saves.
struct MoveArm {
    report_path: Option<PathBuf>,
    save_path: PathBuf,
    /// (path, original bytes, label).
    fixtures: Vec<(PathBuf, Vec<u8>, String)>,
}

pub const MOVE_FILE: &str = "visual3a-world.wrl";

/// (file, label, text): LF / CRLF + BOM / lone CR, comments, Unicode, and a
/// nested Transform the gizmo must refuse.
pub fn move_fixtures() -> Vec<(&'static str, &'static str, String)> {
    use wrlforge_vrml::create::{template, Primitive};
    let doc = |bom: &str, eol: &str, def: &str, extra: &str| {
        let body = template(Primitive::Box, def, eol).replace(
            &format!("scale 1 1 1{eol}"),
            &format!("scale 1 1 1 # größe ✓ 😀{eol}"),
        );
        format!(
            "{bom}#VRML V2.0 utf8{eol}# 世界 — VISUAL-3A fixture ✓{eol}WorldInfo {{ title \"héllo 😀\" }}{eol}{body}{eol}{extra}# fin ✓{eol}"
        )
    };
    let nested = |eol: &str| {
        format!(
            "Transform {{ translation 0 -2.5 0 children [{eol}  DEF Inner Transform {{ translation 0 0 0 children [ Shape {{ appearance Appearance {{ material Material {{ diffuseColor 0.9 0.8 0.1 }} }} geometry Sphere {{ radius 0.4 }} }} ] }}{eol}] }}{eol}"
        )
    };
    vec![
        (
            "fx-crlf-bom.wrl",
            "BOM + CRLF + comments + Unicode",
            doc("\u{feff}", "\r\n", "Box_ü", &nested("\r\n")),
        ),
        ("fx-cr.wrl", "lone CR + Unicode", doc("", "\r", "Box_ü", "")),
        (
            "fx-lf.wrl",
            "LF + comments",
            doc("", "\n", "Box_1", "").replace("translation 0 0 0", "translation 0.0 0.0 0.0"),
        ),
    ]
}

#[derive(Default)]
pub struct SmokeState(
    Mutex<Option<Plan>>,
    Mutex<Option<CreateArm>>,
    Mutex<Option<PickArm>>,
    Mutex<Option<MoveArm>>,
);

impl SmokeState {
    pub fn arm(
        &self,
        path: PathBuf,
        report_path: Option<PathBuf>,
        expect_preview: bool,
        inspector: bool,
        theme: Option<p::ThemeSmoke>,
        settings_path: Option<PathBuf>,
    ) -> Result<(), String> {
        let settings_before = settings_path.as_ref().and_then(|sp| std::fs::read(sp).ok());
        let original_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let (original_text, format) = files::decode(&original_bytes).map_err(|e| e.to_string())?;
        *self.0.lock().unwrap() = Some(Plan {
            path,
            original_bytes,
            original_text,
            format,
            report_path,
            expect_preview,
            inspector,
            theme,
            settings_path,
            settings_before,
        });
        Ok(())
    }
    /// Arm the VISUAL-1 workflow in `dir`, which must be an existing
    /// directory under the system temp dir and must not hold the file yet.
    pub fn arm_create(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = dir.canonicalize().map_err(|e| e.to_string())?;
        let tmp = std::env::temp_dir()
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !dir.starts_with(&tmp) || dir == tmp {
            return Err(format!(
                "refusing a non-temporary directory: {}",
                dir.display()
            ));
        }
        let save_path = dir.join(CREATE_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        *self.1.lock().unwrap() = Some(CreateArm {
            save_path,
            report_path,
            confirms: VecDeque::from([false, true]),
            asked: 0,
        });
        Ok(())
    }

    /// Arm the VISUAL-2 picking run from `dir/plan.json` (written by
    /// `smoke-pick-plan.cjs`). Every fixture must be a plain file directly
    /// in `dir` (no separators, no symlinks), and `dir` must be temporary.
    pub fn arm_pick(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        #[derive(serde::Deserialize)]
        struct Fx {
            id: String,
            file: String,
            camera: [f64; 3],
            clicks: Vec<p::PickClick>,
        }
        #[derive(serde::Deserialize)]
        struct Plan {
            fixtures: Vec<Fx>,
        }
        let raw = std::fs::read_to_string(dir.join("plan.json")).map_err(|e| e.to_string())?;
        let plan: Plan = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let mut files = Vec::new();
        let mut fixtures = Vec::new();
        for f in plan.fixtures {
            if f.file.is_empty() || f.file.contains(['/', '\\']) || f.file.starts_with('.') {
                return Err(format!("bad fixture file name {:?}", f.file));
            }
            let path = dir.join(&f.file);
            let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err(format!("{} is not a plain file", f.file));
            }
            files.push((
                path.clone(),
                std::fs::read(&path).map_err(|e| e.to_string())?,
            ));
            fixtures.push(p::PickFixture {
                id: f.id,
                camera: f.camera,
                clicks: f.clicks,
            });
        }
        let save_path = dir.join(PICK_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        *self.2.lock().unwrap() = Some(PickArm {
            report_path,
            save_path,
            files,
            fixtures,
        });
        Ok(())
    }

    /// Arm the VISUAL-3A gizmo run in `dir` (temporary, must not hold the
    /// fixture or world files yet). The fixtures are written here, by Rust.
    pub fn arm_move(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        let save_path = dir.join(MOVE_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        let mut fixtures = Vec::new();
        for (file, label, text) in move_fixtures() {
            let path = dir.join(file);
            if path.exists() {
                return Err(format!("{} already exists", path.display()));
            }
            std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
            fixtures.push((path, text.into_bytes(), label.to_string()));
        }
        *self.3.lock().unwrap() = Some(MoveArm {
            report_path,
            save_path,
            fixtures,
        });
        Ok(())
    }

    /// Smoke only: fixture `index` of the picking or gizmo run.
    pub fn pick_fixture(&self, index: usize) -> Option<PathBuf> {
        if let Some(a) = self.3.lock().ok()?.as_ref() {
            return a.fixtures.get(index).map(|(p, _, _)| p.clone());
        }
        let g = self.2.lock().ok()?;
        g.as_ref()?.files.get(index).map(|(p, _)| p.clone())
    }

    /// Smoke only: the Save As destination (instead of the native dialog).
    pub fn save_as_override(&self) -> Option<PathBuf> {
        if let Some(p) = self.3.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p);
        }
        if let Some(p) = self.2.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p);
        }
        self.1.lock().ok()?.as_ref().map(|a| a.save_path.clone())
    }

    /// Smoke only: the Open choice -- the file this run saved, once it exists.
    pub fn open_override(&self) -> Option<PathBuf> {
        if let Some(p) = self.3.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p).filter(|p| p.exists());
        }
        let g = self.1.lock().ok()?;
        let a = g.as_ref()?;
        Some(a.save_path.clone()).filter(|p| p.exists())
    }

    /// Smoke only: the discard-confirmation answer. An unplanned question
    /// is answered Cancel (never discards).
    pub fn confirm_override(&self) -> Option<bool> {
        // The picking and gizmo runs start a fresh New World per theme: discard.
        if self.2.lock().ok()?.is_some() || self.3.lock().ok()?.is_some() {
            return Some(true);
        }
        let mut g = self.1.lock().ok()?;
        let a = g.as_mut()?;
        a.asked += 1;
        Some(a.confirms.pop_front().unwrap_or(false))
    }

    pub fn plan(&self) -> Option<p::SmokePlan> {
        let hold = std::env::var("WRLFORGE_SMOKE_ALIGN_HOLD_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0u64)
            .min(10_000);
        if let Some(a) = self.3.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                pick: None,
                gizmo: Some(p::GizmoSmoke {
                    themes: p::theme::THEMES.iter().map(|t| t.id.to_string()).collect(),
                    fixtures: a.fixtures.iter().map(|(_, _, l)| l.clone()).collect(),
                    hold_ms: hold,
                    real_pointer: real_pointer_env().is_ok(),
                }),
            });
        }
        if let Some(a) = self.2.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                gizmo: None,
                pick: Some(p::PickSmoke {
                    fixtures: a.fixtures.clone(),
                    themes: p::theme::THEMES.iter().map(|t| t.id.to_string()).collect(),
                    hold_ms: hold,
                    real_pointer: real_pointer_env().is_ok(),
                }),
            });
        }
        if self.1.lock().ok()?.is_some() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: Some(p::CreateSmoke {
                    translation: CREATE_TRANSLATION.map(String::from).to_vec(),
                    color: CREATE_COLOR.map(String::from).to_vec(),
                    themes: p::theme::THEMES.iter().map(|t| t.id.to_string()).collect(),
                    hold_ms: hold,
                }),
                pick: None,
                gizmo: None,
            });
        }
        self.0.lock().ok()?.as_ref().map(|pl| p::SmokePlan {
            insert_text: INSERT.into(),
            expect_preview: pl.expect_preview,
            inspector_value: pl.inspector.then(|| INSPECTOR_VALUE.to_string()),
            theme: pl.theme.clone(),
            // Opt-in visual registration pauses (never set by default).
            syntax_align_hold_ms: hold,
            create: None,
            pick: None,
            gizmo: None,
        })
    }
}

/// The private Xvfb screen `smoke.sh --headless` creates.
const XVFB_GEOMETRY: &str = "1600 1000";

/// Real pointer input is allowed ONLY when `smoke.sh` launched this process
/// inside its own Xvfb server (`WRLFORGE_SMOKE_XVFB=1`) with a DISPLAY set.
fn real_pointer_env() -> Result<String, String> {
    if std::env::var("WRLFORGE_SMOKE_XVFB").as_deref() != Ok("1") {
        return Err("not inside the harness's Xvfb (WRLFORGE_SMOKE_XVFB unset)".into());
    }
    std::env::var("DISPLAY").map_err(|_| "no DISPLAY".to_string())
}

fn xdotool(args: &[&str]) -> Result<String, String> {
    eprintln!("smoke-pick: xdotool {}", args.join(" "));
    let out = std::process::Command::new("timeout")
        .arg("5")
        .arg("xdotool")
        .args(args)
        .output()
        .map_err(|e| format!("xdotool: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "xdotool {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl SmokeState {
    /// Smoke only (`--smoke-pick` under `smoke.sh --headless`): one REAL X
    /// primary-button click at root coordinates (`x`, `y`).
    ///
    /// Input is sent only after proving where it goes: the harness's Xvfb
    /// (environment marker + the exact screen geometry it created), a window
    /// owned by THIS process, and that window holding the input focus.
    pub fn real_click(&self, x: i32, y: i32) -> Result<String, String> {
        self.real_pointer("click", x, y)
    }

    /// Smoke only (`--smoke-pick` / `--smoke-move` under `smoke.sh
    /// --headless`): one REAL X input action at root coordinates: `click`,
    /// `down` (press at x, y), `move` (to x, y, button state unchanged), `up`
    /// (release at x, y) or `escape` (the Escape key). Same guards as a click.
    pub fn real_pointer(&self, action: &str, x: i32, y: i32) -> Result<String, String> {
        let armed = self.2.lock().map_err(|e| e.to_string())?.is_some()
            || self.3.lock().map_err(|e| e.to_string())?.is_some();
        if !armed {
            return Err("no picking or gizmo smoke run is armed".into());
        }
        let display = real_pointer_env()?;
        let geo = xdotool(&["getdisplaygeometry"])?;
        if geo != XVFB_GEOMETRY {
            return Err(format!(
                "display {display} is {geo}, not the harness Xvfb ({XVFB_GEOMETRY})"
            ));
        }
        let pid = std::process::id().to_string();
        let mine = xdotool(&["search", "--onlyvisible", "--pid", &pid])?;
        let mine: Vec<&str> = mine.lines().filter(|l| !l.is_empty()).collect();
        if mine.is_empty() {
            return Err(format!("no visible window of pid {pid}"));
        }
        // A bare Xvfb has no window manager (focus = PointerRoot): give the
        // focus to OUR window, then require that it holds it.
        let held = xdotool(&["getwindowfocus"]).ok();
        if !held.as_deref().is_some_and(|f| mine.contains(&f)) {
            xdotool(&["windowfocus", "--sync", mine[0]])?;
        }
        let focus = xdotool(&["getwindowfocus"])?;
        if !mine.contains(&focus.as_str()) {
            return Err(format!("focus {focus} is not a window of pid {pid}"));
        }
        let (xs, ys) = (x.to_string(), y.to_string());
        // No `--sync`: it waits for pointer MOTION, which never comes when
        // the pointer is already at the target (a repeated click).
        match action {
            "click" => xdotool(&["mousemove", &xs, &ys, "click", "1"])?,
            "down" => xdotool(&["mousemove", &xs, &ys, "mousedown", "1"])?,
            "move" => xdotool(&["mousemove", &xs, &ys])?,
            "up" => xdotool(&["mousemove", &xs, &ys, "mouseup", "1"])?,
            "escape" => xdotool(&["key", "Escape"])?,
            other => return Err(format!("unknown real input action {other:?}")),
        };
        Ok(format!(
            "display {display} ({geo}) window {focus} of pid {pid} at {x},{y}"
        ))
    }
}

/// `dir`, canonical, when it is an existing directory strictly under the
/// system temp dir.
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

/// The source text the UI's edit must produce: a new line inserted before the
/// first line break, using the document's dominant line ending.
pub fn expected_text(original: &str) -> String {
    let eol = EolCounts::of(original).dominant().as_str();
    match original.find(['\r', '\n']) {
        Some(i) => format!("{}{eol}{INSERT}{}", &original[..i], &original[i..]),
        None => format!("{original}{eol}{INSERT}"),
    }
}

pub fn finish(app: &AppHandle, report: p::SmokeReport) {
    use tauri::Manager;
    let state = app.state::<SmokeState>();
    if let Some(arm) = state.3.lock().unwrap().take() {
        let mut steps = report.steps;
        steps.extend(verify_move(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": MOVE_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.2.lock().unwrap().take() {
        let mut steps = report.steps;
        // Source integrity: no fixture file changed on disk.
        let changed: Vec<String> = arm
            .files
            .iter()
            .filter(|(path, bytes)| std::fs::read(path).ok().as_deref() != Some(&bytes[..]))
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        steps.push(p::SmokeStep {
            name: "rust: every fixture file is byte-identical after the picking run".into(),
            ok: changed.is_empty(),
            detail: format!("{} files checked; changed: {changed:?}", arm.files.len()),
        });
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": PICK_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.1.lock().unwrap().take() {
        let mut steps = report.steps;
        steps.extend(verify_create(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": CREATE_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    let plan = state.0.lock().unwrap().take();
    let mut steps = report.steps;
    if let Some(pl) = plan {
        let disk = std::fs::read(&pl.path).unwrap_or_default();
        let decoded = files::decode(&disk);
        let want = expected_text(&pl.original_text);
        let matches = |t: &str| {
            if pl.inspector {
                inspector_change_ok(&want, t)
            } else {
                t == want
            }
        };
        let (ok, detail) = match &decoded {
            Ok((t, f)) if *f == pl.format && matches(t) => {
                (true, format!("{} bytes, format {}", disk.len(), f.as_str()))
            }
            Ok((t, f)) => (
                false,
                format!(
                    "format {} (want {}), text {} bytes (want {})",
                    f.as_str(),
                    pl.format.as_str(),
                    t.len(),
                    want.len()
                ),
            ),
            Err(e) => (false, e.to_string()),
        };
        steps.push(p::SmokeStep {
            name: "rust: saved file decodes to exactly the expected source".into(),
            ok,
            detail,
        });
        // Exactly one backup, holding the original bytes.
        let dir = pl.path.parent().map(PathBuf::from).unwrap_or_default();
        let base = pl.path.file_name().unwrap().to_string_lossy().into_owned();
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().is_some_and(|n| {
                            n.to_string_lossy().starts_with(&format!("{base}.bak-"))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let bok = backups.len() == 1
            && std::fs::read(&backups[0]).ok().as_deref() == Some(&pl.original_bytes[..]);
        steps.push(p::SmokeStep {
            name: "rust: one backup with the original bytes".into(),
            ok: bok,
            detail: format!("{} backup(s)", backups.len()),
        });
        let temps = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().contains("wrlforge-tmp"))
                    .count()
            })
            .unwrap_or(0);
        steps.push(p::SmokeStep {
            name: "rust: no temp file left".into(),
            ok: temps == 0,
            detail: format!("{temps}"),
        });
        if let Some(t) = &pl.theme {
            steps.push(theme_settings_step(
                t,
                &pl.settings_path,
                &pl.settings_before,
            ));
        }
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(&serde_json::json!({ "pass": all, "file": pl.path.file_name().map(|n| n.to_string_lossy().into_owned()), "steps": steps })).unwrap();
        println!("{json}");
        if let Some(rp) = pl.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
    }
}

/// The bytes a gizmo move may change: exactly one translation token, every
/// other byte (BOM, line endings, comments, Unicode) identical. Returns the
/// (old, new) token.
pub fn one_translation_token(before: &[u8], after: &[u8]) -> Option<(String, String)> {
    let pre = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let max_suf = before.len().min(after.len()) - pre;
    let suf = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(max_suf)
        .take_while(|(a, b)| a == b)
        .count();
    // Widen to whole tokens (an edit may share leading / trailing digits).
    let delim = |c: u8| c.is_ascii_whitespace() || b"[]{},".contains(&c);
    let mut a = pre;
    while a > 0 && !delim(before[a - 1]) {
        a -= 1;
    }
    let (mut eb, mut ea) = (before.len() - suf, after.len() - suf);
    while eb < before.len() && !delim(before[eb]) {
        eb += 1;
        ea += 1;
    }
    let old = std::str::from_utf8(&before[a..eb]).ok()?;
    let new = std::str::from_utf8(&after[a..ea]).ok()?;
    let line_start = before[..a]
        .iter()
        .rposition(|&c| c == b'\n' || c == b'\r')
        .map_or(0, |i| i + 1);
    let line = std::str::from_utf8(&before[line_start..a]).ok()?;
    let one = !old.is_empty()
        && !new.is_empty()
        && !old.bytes().any(delim)
        && !new.bytes().any(delim)
        && line.trim_start().starts_with("translation ")
        && before[..a] == after[..a]
        && before[eb..] == after[ea..];
    one.then(|| (old.to_string(), new.to_string()))
}

fn verify_move(arm: &MoveArm) -> Vec<p::SmokeStep> {
    let mut steps = Vec::new();
    for (path, original, label) in &arm.fixtures {
        let disk = std::fs::read(path).unwrap_or_default();
        let one = one_translation_token(original, &disk);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let backups: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().is_some_and(|n| {
                            n.to_string_lossy().starts_with(&format!("{name}.bak-"))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let backup_ok =
            backups.len() == 1 && std::fs::read(&backups[0]).ok().as_deref() == Some(&original[..]);
        steps.push(p::SmokeStep {
            name: format!("rust: {label} fixture on disk differs by exactly one translation token; one backup holds the original bytes"),
            ok: one.is_some() && backup_ok,
            detail: format!("{name}: {one:?} · {} backup(s)", backups.len()),
        });
    }
    let saved = std::fs::read(&arm.save_path).ok();
    let parsed = saved
        .as_deref()
        .and_then(|b| files::decode(b).ok())
        .map(|(t, _)| t);
    let ok = parsed.as_deref().is_some_and(|t| {
        let pr = wrlforge_vrml::parse(t);
        !wrlforge_vrml::field_edit::has_blocking_syntax_error(&pr)
            && t.contains("DEF Box_1 Transform")
            && !t.contains("DEF Box_1 Transform {\n  translation 0 0 0")
    });
    steps.push(p::SmokeStep {
        name: "rust: the saved world parses and holds the moved Box_1".into(),
        ok,
        detail: format!("{} bytes", saved.map_or(0, |b| b.len())),
    });
    steps
}

/// The exact texts the VISUAL-1 workflow must leave: `first` after the
/// first Save As (Box, Inspector-edited), `last` after the final Save
/// (plus Sphere, Cone, Cylinder). Derived from the same planner the app
/// uses, then edited exactly as the Inspector edits.
pub fn create_expected() -> (String, String) {
    use wrlforge_vrml::create::{plan_create, Plan, Primitive, NEW_WORLD};
    let next = |t: &str, p| match plan_create(t, p) {
        Plan::Ready { new_text, .. } => new_text,
        Plan::Refused { message, .. } => panic!("{message}"),
    };
    let first = next(NEW_WORLD, Primitive::Box)
        .replacen(
            "translation 0 0 0",
            &format!("translation {}", CREATE_TRANSLATION.join(" ")),
            1,
        )
        .replacen(
            "diffuseColor 0.8 0.3 0.2",
            &format!("diffuseColor {}", CREATE_COLOR.join(" ")),
            1,
        );
    let mut last = first.clone();
    for p in [Primitive::Sphere, Primitive::Cone, Primitive::Cylinder] {
        last = next(&last, p);
    }
    (first, last)
}

/// Rust's independent check of the VISUAL-1 files on disk.
fn verify_create(arm: &CreateArm) -> Vec<p::SmokeStep> {
    let (first, last) = create_expected();
    let step = |name: &str, ok: bool, detail: String| p::SmokeStep {
        name: name.into(),
        ok,
        detail,
    };
    let disk = std::fs::read_to_string(&arm.save_path).unwrap_or_default();
    let parsed = wrlforge_vrml::parse(&disk);
    let dir = arm
        .save_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let backups: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with(&format!("{CREATE_FILE}.bak-")))
        .collect();
    let backup_ok = backups.len() == 1
        && std::fs::read_to_string(dir.join(backups[0]))
            .ok()
            .as_deref()
            == Some(first.as_str());
    vec![
        step(
            "rust: saved world is exactly New World + Box (edited) + Sphere + Cone + Cylinder",
            disk == last,
            format!("{} bytes (want {})", disk.len(), last.len()),
        ),
        step(
            "rust: saved world parses as VRML97 with no diagnostics and 4 top-level objects",
            parsed.diagnostics.is_empty() && parsed.tree.statements.len() == 4,
            format!(
                "{} diagnostics, {} statements",
                parsed.diagnostics.len(),
                parsed.tree.statements.len()
            ),
        ),
        step(
            "rust: the later Save took one backup holding the first Save As text",
            backup_ok,
            format!("{} backup(s)", backups.len()),
        ),
        step(
            "rust: the discard confirmation was asked exactly twice (Cancel, Discard)",
            arm.asked == 2 && arm.confirms.is_empty(),
            format!("asked {}", arm.asked),
        ),
        step(
            "rust: no temp file and no other file in the work directory",
            names.iter().all(|n| {
                n == CREATE_FILE
                    || n.starts_with(&format!("{CREATE_FILE}.bak-"))
                    || n == "report.json"
                    || n == "stdout.txt"
                    || n == "stderr.txt"
                    || n == "config"
                    || n == "shots"
            }),
            names.join(", "),
        ),
    ]
}

/// Rust's own check of the settings file after a theme run: the final theme
/// was persisted, or (save-failure run) the file is byte-identical.
fn theme_settings_step(
    t: &p::ThemeSmoke,
    path: &Option<PathBuf>,
    before: &Option<Vec<u8>>,
) -> p::SmokeStep {
    let Some(path) = path else {
        return p::SmokeStep {
            name: "rust: theme settings file".into(),
            ok: false,
            detail: "no settings path".into(),
        };
    };
    let now = std::fs::read(path).ok();
    if t.expect_save_failure {
        return p::SmokeStep {
            name: "rust: failed theme saves left the settings file byte-identical".into(),
            ok: &now == before,
            detail: format!(
                "{} -> {} bytes",
                before.as_ref().map_or(0, |b| b.len()),
                now.as_ref().map_or(0, |b| b.len())
            ),
        };
    }
    let loaded = now.as_deref().map(crate::settings::parse);
    let ok = loaded
        .as_ref()
        .is_some_and(|l| l.theme_id == t.final_theme && l.notice.is_none());
    p::SmokeStep {
        name: "rust: settings file persists the final theme (schemaVersion 1)".into(),
        ok,
        detail: format!("{:?}", loaded.map(|l| l.theme_id)),
    }
}

fn is_delim(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | '[' | ']' | '{' | '}')
}

/// The single token that differs between `a` and `b`, widened to token
/// boundaries: `(old, new, byte start in a)`. `None` if more than one token
/// (or any delimiter) changed.
pub fn one_token_change(a: &str, b: &str) -> Option<(String, String, usize)> {
    if a == b {
        return None;
    }
    let mut pre = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map(|((i, _), _)| i)
        .unwrap_or(a.len().min(b.len()));
    let mut suf = 0;
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    while suf < ab.len() - pre
        && suf < bb.len() - pre
        && ab[ab.len() - 1 - suf] == bb[bb.len() - 1 - suf]
    {
        suf += 1;
    }
    while pre > 0 && !is_delim(a[..pre].chars().next_back()?) {
        pre -= a[..pre].chars().next_back()?.len_utf8();
    }
    let widen = |t: &str, mut s: usize| {
        while s > 0 && !t.is_char_boundary(t.len() - s) {
            s -= 1;
        }
        while s > 0 && !is_delim(t[t.len() - s..].chars().next().unwrap_or(' ')) {
            let c = t[t.len() - s..].chars().next().unwrap();
            s -= c.len_utf8();
        }
        s
    };
    let suf = widen(a, suf).min(widen(b, suf));
    let old = &a[pre..a.len() - suf];
    let new = &b[pre..b.len() - suf];
    if old.is_empty() || new.is_empty() || old.contains(is_delim) || new.contains(is_delim) {
        return None;
    }
    Some((old.to_string(), new.to_string(), pre))
}

/// The Inspector smoke changed exactly one numeric token, to the planned
/// value, as the second component of `diffuseColor` or `translation`.
pub fn inspector_change_ok(before: &str, after: &str) -> bool {
    let Some((old, new, at)) = one_token_change(before, after) else {
        return false;
    };
    let prior: Vec<&str> = before[..at]
        .split(is_delim)
        .filter(|t| !t.is_empty())
        .collect();
    new == INSPECTOR_VALUE
        && old.parse::<f64>().is_ok()
        && prior.len() >= 2
        && prior[prior.len() - 1].parse::<f64>().is_ok()
        && matches!(prior[prior.len() - 2], "diffuseColor" | "translation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gizmo_fixtures_are_movable_and_the_disk_check_is_one_token_exact() {
        use wrlforge_vrml::manipulate::{plan_translate, translate_target, Axis};
        for (file, _, text) in move_fixtures() {
            let p = wrlforge_vrml::parse(&text);
            assert!(
                !wrlforge_vrml::field_edit::has_blocking_syntax_error(&p),
                "{file}"
            );
            let t = p
                .tree
                .statements
                .iter()
                .find_map(|s| match s {
                    wrlforge_vrml::ast::Ast::Node(n)
                        if n.node_type == "Transform" && n.def.is_some() =>
                    {
                        Some((n.range.start.offset as u64, n.range.end.offset as u64))
                    }
                    _ => None,
                })
                .unwrap();
            assert!(translate_target(&text, t.0, t.1).is_ok(), "{file}");
            let (plan, _) = plan_translate(&text, t.0, t.1, Axis::X, 1.75, 2);
            let wrlforge_vrml::field_edit::Plan::Ready { new_text, .. } = plan else {
                panic!("{file}: {plan:?}")
            };
            let got = one_translation_token(text.as_bytes(), new_text.as_bytes());
            assert_eq!(
                got.as_ref().map(|(_, n)| n.as_str()),
                Some("1.75"),
                "{file}"
            );
            // Any other change is not a one-token move.
            let other = new_text.replacen("fin", "fín", 1);
            assert!(
                one_translation_token(text.as_bytes(), other.as_bytes()).is_none(),
                "{file}"
            );
            assert!(one_translation_token(text.as_bytes(), text.as_bytes()).is_none());
        }
    }

    /// VISUAL-3A guard: the private X_ITE camera surfaces the gizmo needs
    /// are read ONLY in `xite-gizmo-adapter.js` (comments excluded), and that
    /// adapter touches none of the WD2-D parser / hit-test surfaces.
    #[test]
    fn private_xite_camera_access_stays_in_the_gizmo_adapter() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/static");
        let code = |t: &str| {
            t.lines()
                .map(|l| l.split("//").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let private = [
            "getActiveLayer",
            "getViewpoint",
            "getViewMatrix",
            "getProjectionMatrix",
            "getRectangle",
        ];
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path.extension().and_then(|x| x.to_str()) != Some("js") {
                continue;
            }
            let src = code(&std::fs::read_to_string(&path).unwrap());
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name == "xite-gizmo-adapter.js" {
                seen += 1;
                for p in private {
                    assert!(src.contains(p), "the adapter no longer reads {p}");
                }
                for p in [
                    "VRMLParser",
                    "nodeStatement",
                    "getHit",
                    "getParents",
                    ".touch(",
                ] {
                    assert!(!src.contains(p), "the gizmo adapter must not touch {p}");
                }
            } else {
                for p in private {
                    assert!(!src.contains(p), "{name} reads private X_ITE surface {p}");
                }
            }
        }
        assert_eq!(seen, 1);
    }

    #[test]
    fn inspector_change_check_is_token_exact() {
        let a = "Material { diffuseColor 0.046 0.062 0.118 }\r\n";
        assert!(inspector_change_ok(a, &a.replace("0.062", "0.25")));
        assert!(!inspector_change_ok(a, &a.replace("0.046", "0.25")));
        assert!(!inspector_change_ok(
            a,
            &a.replace("0.062 0.118", "0.25 0.2")
        ));
        assert!(!inspector_change_ok(
            a,
            &a.replace("\r\n", "\n").replace("0.062", "0.25")
        ));
        assert!(!inspector_change_ok(a, a));
        let t = "T { translation 1 2 3 } # é😀";
        assert!(inspector_change_ok(t, &t.replace(" 2 ", " 0.25 ")));
        assert_eq!(
            one_token_change("x 12 y", "x 0.25 y"),
            Some(("12".into(), "0.25".into(), 2))
        );
    }
    #[test]
    fn create_expected_is_the_edited_box_then_three_more_objects() {
        let (first, last) = create_expected();
        assert!(first.starts_with("#VRML V2.0 utf8\n\nDEF Box_1 Transform {\n"));
        assert!(
            first.contains("translation 1.5 0.5 -1") && first.contains("diffuseColor 0.1 0.6 0.9")
        );
        assert!(last.starts_with(&first));
        let tail = &last[first.len()..];
        let order: Vec<_> = ["DEF Sphere_1", "DEF Cone_1", "DEF Cylinder_1"]
            .iter()
            .map(|n| tail.find(n).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        assert!(wrlforge_vrml::parse(&last).diagnostics.is_empty());
    }

    #[test]
    fn expected_text_inserts_with_the_document_ending() {
        assert_eq!(
            expected_text("a\r\nb\r\n"),
            format!("a\r\n{INSERT}\r\nb\r\n")
        );
        assert_eq!(expected_text("a\nb"), format!("a\n{INSERT}\nb"));
        assert_eq!(expected_text("a"), format!("a\n{INSERT}"));
    }
}
