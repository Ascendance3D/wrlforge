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

use std::path::PathBuf;
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

#[derive(Default)]
pub struct SmokeState(Mutex<Option<Plan>>);

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
    pub fn plan(&self) -> Option<p::SmokePlan> {
        self.0.lock().ok()?.as_ref().map(|pl| p::SmokePlan {
            insert_text: INSERT.into(),
            expect_preview: pl.expect_preview,
            inspector_value: pl.inspector.then(|| INSPECTOR_VALUE.to_string()),
            theme: pl.theme.clone(),
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
    fn expected_text_inserts_with_the_document_ending() {
        assert_eq!(
            expected_text("a\r\nb\r\n"),
            format!("a\r\n{INSERT}\r\nb\r\n")
        );
        assert_eq!(expected_text("a\nb"), format!("a\n{INSERT}\nb"));
        assert_eq!(expected_text("a"), format!("a\n{INSERT}"));
    }
}
