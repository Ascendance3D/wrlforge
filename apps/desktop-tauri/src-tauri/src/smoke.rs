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

struct Plan {
    path: PathBuf,
    original_bytes: Vec<u8>,
    original_text: String,
    format: Format,
    report_path: Option<PathBuf>,
    expect_preview: bool,
}

#[derive(Default)]
pub struct SmokeState(Mutex<Option<Plan>>);

impl SmokeState {
    pub fn arm(
        &self,
        path: PathBuf,
        report_path: Option<PathBuf>,
        expect_preview: bool,
    ) -> Result<(), String> {
        let original_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let (original_text, format) = files::decode(&original_bytes).map_err(|e| e.to_string())?;
        *self.0.lock().unwrap() = Some(Plan {
            path,
            original_bytes,
            original_text,
            format,
            report_path,
            expect_preview,
        });
        Ok(())
    }
    pub fn plan(&self) -> Option<p::SmokePlan> {
        self.0.lock().ok()?.as_ref().map(|pl| p::SmokePlan {
            insert_text: INSERT.into(),
            expect_preview: pl.expect_preview,
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
        let (ok, detail) = match &decoded {
            Ok((t, f)) if *f == pl.format && *t == want => {
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
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(&serde_json::json!({ "pass": all, "file": pl.path.file_name().map(|n| n.to_string_lossy().into_owned()), "steps": steps })).unwrap();
        println!("{json}");
        if let Some(rp) = pl.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
