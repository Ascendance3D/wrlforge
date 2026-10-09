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

#[derive(Default)]
pub struct SmokeState(Mutex<Option<Plan>>, Mutex<Option<CreateArm>>);

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

    /// Smoke only: the Save As destination (instead of the native dialog).
    pub fn save_as_override(&self) -> Option<PathBuf> {
        self.1.lock().ok()?.as_ref().map(|a| a.save_path.clone())
    }

    /// Smoke only: the Open choice -- the file this run saved, once it exists.
    pub fn open_override(&self) -> Option<PathBuf> {
        let g = self.1.lock().ok()?;
        let a = g.as_ref()?;
        Some(a.save_path.clone()).filter(|p| p.exists())
    }

    /// Smoke only: the discard-confirmation answer. An unplanned question
    /// is answered Cancel (never discards).
    pub fn confirm_override(&self) -> Option<bool> {
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
        })
    }
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
