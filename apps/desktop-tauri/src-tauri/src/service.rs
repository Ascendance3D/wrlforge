// SPDX-License-Identifier: GPL-3.0-or-later
//! The document service: Rust-owned sessions, one canonical buffer each.
//!
//! Replaces the Electron main-process editor lane (`src/editor/session.js`,
//! `session-store.js`, `editor-controller.js`, the `editor:*` IPC handlers in
//! `main.js`) for the Tauri application. Tauri-free, so every behaviour here is
//! tested with plain `cargo test`; `commands.rs` is a thin IPC shim over it.
//!
//! Security shape: a session holds the ONLY copy of its path. The WebView
//! names a document by its opaque session number and can never supply or
//! widen a path. A path enters exclusively through `open_path` / `save_as`,
//! which the shim calls with a native-dialog result or the launch argument.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use wrlforge_desktop_protocol as p;
use wrlforge_document::{Applied, Document, ViewEdit};
use wrlforge_vrml::scene;

use crate::files::{self, FileError, Format, SaveOptions, Stamp};

pub struct Session {
    path: PathBuf,
    format: Format,
    stamp: Stamp,
    doc: Document,
}

pub struct Service {
    sessions: Mutex<HashMap<p::SessionId, Session>>,
    next: AtomicU64,
    clock: fn() -> SystemTime,
}

impl Default for Service {
    fn default() -> Self {
        Service::new(SystemTime::now)
    }
}

/// Source UTF-16 -> view UTF-16, for many lookups over one text: every CRLF
/// before an offset shortens the view by one.
pub struct ViewMap {
    crlf_lf_at: Vec<u64>,
}

impl ViewMap {
    pub fn new(text: &str) -> Self {
        let mut v = Vec::new();
        let mut u16 = 0u64;
        let mut prev_cr = false;
        for c in text.chars() {
            if c == '\n' && prev_cr {
                v.push(u16);
            }
            prev_cr = c == '\r';
            u16 += c.len_utf16() as u64;
        }
        ViewMap { crlf_lf_at: v }
    }
    pub fn view(&self, source: u64) -> u64 {
        // LFs of CRLF pairs strictly before `source` are not in the view.
        let n = self.crlf_lf_at.partition_point(|&lf| lf < source) as u64;
        source - n
    }
}

fn err(m: impl std::fmt::Display) -> String {
    m.to_string()
}

impl Service {
    pub fn new(clock: fn() -> SystemTime) -> Self {
        Service {
            sessions: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            clock,
        }
    }

    fn with<R>(&self, id: p::SessionId, f: impl FnOnce(&mut Session) -> R) -> Result<R, String> {
        let mut map = self.sessions.lock().map_err(err)?;
        let s = map
            .get_mut(&id)
            .ok_or_else(|| format!("unknown document session {id}"))?;
        Ok(f(s))
    }

    fn info(id: p::SessionId, s: &Session) -> p::DocumentInfo {
        let counts = s.doc.eol_counts();
        p::DocumentInfo {
            session: id,
            name: s
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            display_path: s.path.display().to_string(),
            format: s.format.as_str().into(),
            view: s.doc.view(),
            revision: s.doc.revision(),
            dirty: s.doc.dirty(),
            eol: s.doc.eol().name().into(),
            eol_mixed: counts.mixed(),
            bom: s.doc.text().starts_with('\u{FEFF}'),
            bytes_on_disk: s.stamp.size,
            can_undo: s.doc.can_undo(),
            can_redo: s.doc.can_redo(),
        }
    }

    fn state(s: &Session, a: Applied) -> p::DocState {
        p::DocState {
            revision: a.revision,
            dirty: a.dirty,
            view_len: a.view_len,
            view_hash: p::view_hash(&s.doc.view()),
            caret: a.caret,
            can_undo: s.doc.can_undo(),
            can_redo: s.doc.can_redo(),
        }
    }

    /// Open a Rust-chosen path into a new session.
    pub fn open_path(&self, path: &Path) -> p::OpenOutcome {
        let loaded = match files::load(path) {
            Ok(l) => l,
            Err(e) => {
                return p::OpenOutcome::Failed {
                    message: format!("{}: {e}", path.display()),
                }
            }
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let s = Session {
            path: path.to_path_buf(),
            format: loaded.format,
            stamp: loaded.stamp,
            doc: Document::new(loaded.text),
        };
        let doc = Self::info(id, &s);
        match self.sessions.lock() {
            Ok(mut m) => {
                m.insert(id, s);
                p::OpenOutcome::Opened { doc }
            }
            Err(e) => p::OpenOutcome::Failed { message: err(e) },
        }
    }

    pub fn close(&self, id: p::SessionId) -> Result<(), String> {
        self.sessions.lock().map_err(err)?.remove(&id);
        Ok(())
    }

    pub fn snapshot(&self, id: p::SessionId) -> Result<p::DocumentInfo, String> {
        self.with(id, |s| Self::info(id, s))
    }

    pub fn edit(&self, r: &p::EditRequest) -> Result<p::EditOutcome, String> {
        self.with(r.session, |s| {
            let e = ViewEdit {
                from: r.from,
                to: r.to,
                insert: r.insert.clone(),
            };
            match s.doc.apply_view_edit(r.base_revision, &e) {
                Ok(a) => p::EditOutcome::Applied {
                    state: Self::state(s, a),
                },
                Err(e) => p::EditOutcome::Refused {
                    message: e.to_string(),
                },
            }
        })
    }

    pub fn undo(&self, id: p::SessionId) -> Result<p::HistoryOutcome, String> {
        self.history(id, true)
    }
    pub fn redo(&self, id: p::SessionId) -> Result<p::HistoryOutcome, String> {
        self.history(id, false)
    }
    fn history(&self, id: p::SessionId, undo: bool) -> Result<p::HistoryOutcome, String> {
        self.with(id, |s| {
            let r = if undo { s.doc.undo() } else { s.doc.redo() };
            match r {
                Ok(a) => p::HistoryOutcome::Applied {
                    state: Self::state(s, a),
                    view: s.doc.view(),
                },
                Err(_) => p::HistoryOutcome::Nothing,
            }
        })
    }

    /// Save to the session's own path: conflict-checked, gzip-preserving.
    pub fn save(&self, id: p::SessionId) -> p::SaveOutcome {
        let now = (self.clock)();
        let r = self.with(id, |s| {
            let opts = SaveOptions {
                preserve_existing_gzip: true,
                ..Default::default()
            };
            let result =
                files::safe_save(&s.path, s.doc.text(), s.format, Some(&s.stamp), opts, now);
            Self::after_save(id, s, result)
        });
        r.unwrap_or_else(|m| p::SaveOutcome::Failed { message: m })
    }

    /// Save to a NEW Rust-chosen path (native dialog). The user confirmed the
    /// destination in the dialog, so no conflict baseline applies to it. The
    /// session adopts the new path only after a verified write.
    pub fn save_as(&self, id: p::SessionId, path: &Path) -> p::SaveOutcome {
        let now = (self.clock)();
        let r = self.with(id, |s| {
            let opts = SaveOptions {
                allow_overwrite: true,
                ..Default::default()
            };
            let result = files::safe_save(path, s.doc.text(), s.format, None, opts, now);
            if result.is_ok() {
                s.path = path.to_path_buf();
            }
            Self::after_save(id, s, result)
        });
        r.unwrap_or_else(|m| p::SaveOutcome::Failed { message: m })
    }

    fn after_save(
        id: p::SessionId,
        s: &mut Session,
        result: Result<files::Saved, FileError>,
    ) -> p::SaveOutcome {
        match result {
            Ok(saved) => {
                s.stamp = saved.stamp;
                s.doc.mark_saved();
                p::SaveOutcome::Saved {
                    bytes_written: saved.bytes_written,
                    backup: saved
                        .backup
                        .and_then(|b| b.file_name().map(|n| n.to_string_lossy().into_owned())),
                    preserved: saved.preserved,
                    doc: Self::info(id, s),
                }
            }
            Err(FileError::External { reason }) => p::SaveOutcome::Conflict {
                reason: reason.into(),
            },
            Err(e) => p::SaveOutcome::Failed {
                message: e.to_string(),
            },
        }
    }

    pub fn check_external(&self, id: p::SessionId) -> Result<p::ExternalStatus, String> {
        self.with(id, |s| {
            let c = files::poll_external_change(&s.stamp, &s.path);
            p::ExternalStatus {
                changed: c != files::Change::Unchanged,
                reason: c.reason().into(),
            }
        })
    }

    /// Discard buffer edits and re-read the file (the conflict "Reload" path).
    pub fn reload(&self, id: p::SessionId) -> p::OpenOutcome {
        let r = self.with(id, |s| match files::load(&s.path) {
            Ok(l) => {
                s.format = l.format;
                s.stamp = l.stamp;
                s.doc.reset(l.text);
                p::OpenOutcome::Opened {
                    doc: Self::info(id, s),
                }
            }
            Err(e) => p::OpenOutcome::Failed {
                message: e.to_string(),
            },
        });
        r.unwrap_or_else(|m| p::OpenOutcome::Failed { message: m })
    }

    pub fn analyze(&self, id: p::SessionId) -> Result<p::Analysis, String> {
        self.with(id, |s| {
            let text = s.doc.text();
            let parsed = wrlforge_vrml::parse(text);
            let tree = scene::build_scene_tree(&parsed.tree, text);
            let vm = ViewMap::new(text);
            let items = tree
                .order
                .iter()
                .skip(1)
                .map(|iid| {
                    let it = &tree.items[iid];
                    p::SceneItem {
                        id: it.id.clone(),
                        kind: it.kind.as_str().into(),
                        label: it.label.clone(),
                        detail: it.detail.clone(),
                        depth: it.depth,
                        view_from: vm.view(it.from),
                        view_to: vm.view(it.to),
                        use_status: it.use_status.map(Into::into),
                        child_count: it.children.len() as u32,
                    }
                })
                .collect();
            let diagnostics = parsed
                .diagnostics
                .iter()
                .map(|d| p::Diagnostic {
                    code: d.code.as_str().into(),
                    severity: d.severity.as_str().into(),
                    message: d.message.clone(),
                    line: d.range.start.line,
                    column: d.range.start.column,
                    view_from: vm.view(d.range.start.offset as u64),
                    view_to: vm.view(d.range.end.offset as u64),
                })
                .collect();
            p::Analysis {
                revision: s.doc.revision(),
                items,
                diagnostics,
                resolution_scope: tree.resolution_scope.into(),
                truncated: parsed.truncated,
            }
        })
    }

    pub fn inspect(&self, id: p::SessionId, item: &str) -> Result<Option<p::Inspection>, String> {
        self.with(id, |s| {
            let text = s.doc.text();
            let parsed = wrlforge_vrml::parse(text);
            let vm = ViewMap::new(text);
            scene::inspect(&parsed.tree, text, item).map(|i| p::Inspection {
                revision: s.doc.revision(),
                id: i.id,
                title: i.title,
                rows: i
                    .rows
                    .into_iter()
                    .map(|r| p::InspectorRow {
                        name: r.name,
                        kind: r.kind,
                        source: r.source,
                        elided: r.elided,
                        view_from: vm.view(r.from),
                        view_to: vm.view(r.to),
                    })
                    .collect(),
            })
        })
    }

    /// Preview input: the canonical text, minus a leading U+FEFF. X_ITE's
    /// VRML parser rejects a BOM before `#VRML`; stripping it here is a
    /// display projection only -- the document and the file keep the BOM.
    pub fn preview_source(&self, id: p::SessionId) -> Result<p::PreviewSource, String> {
        self.with(id, |s| {
            let t = s.doc.text();
            p::PreviewSource {
                revision: s.doc.revision(),
                text: t.strip_prefix('\u{FEFF}').unwrap_or(t).to_string(),
            }
        })
    }

    /// Test/smoke support: the exact canonical text.
    pub fn text(&self, id: p::SessionId) -> Result<String, String> {
        self.with(id, |s| s.doc.text().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};

    fn clock() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_791_500_000)
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wrlforge-tauri-svc-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn opened(o: p::OpenOutcome) -> p::DocumentInfo {
        match o {
            p::OpenOutcome::Opened { doc } => doc,
            other => panic!("not opened: {other:?}"),
        }
    }

    fn edit(svc: &Service, id: u64, rev: u64, from: u64, to: u64, ins: &str) -> p::DocState {
        match svc
            .edit(&p::EditRequest {
                session: id,
                base_revision: rev,
                from,
                to,
                insert: ins.into(),
            })
            .unwrap()
        {
            p::EditOutcome::Applied { state } => state,
            other => panic!("refused: {other:?}"),
        }
    }

    #[test]
    fn open_edit_save_reopen_crlf_bom_unicode_gzip() {
        let dir = tmp("e2e");
        let src = "\u{FEFF}#VRML V2.0 utf8\r\n# é 😀\r\nDEF A Transform { children [ Shape { geometry Box {} } ] }\r\n";
        let path = dir.join("item.wrl");
        fs::write(&path, files::encode(src, Format::Gzip).unwrap()).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        assert_eq!(doc.format, "gzip");
        assert!(doc.bom);
        assert_eq!(doc.eol, "CRLF");
        assert_eq!(doc.view, src.replace("\r\n", "\n"));
        // Insert a line after the header in VIEW coordinates.
        let at = doc.view.find('\n').unwrap();
        let at16 = doc.view[..at].encode_utf16().count() as u64;
        let st = edit(&svc, doc.session, 0, at16, at16, "\n# añadido ✓");
        assert!(st.dirty);
        let expected = src.replacen("utf8\r\n", "utf8\r\n# añadido ✓\r\n", 1);
        assert_eq!(svc.text(doc.session).unwrap(), expected);
        let p::SaveOutcome::Saved {
            backup, preserved, ..
        } = svc.save(doc.session)
        else {
            panic!()
        };
        assert!(!preserved && backup.is_some());
        let raw = fs::read(&path).unwrap();
        assert!(files::is_gzip(&raw));
        // Reopen in a fresh service: exact text.
        let svc2 = Service::new(clock);
        let again = opened(svc2.open_path(&path));
        assert_eq!(svc2.text(again.session).unwrap(), expected);
        assert!(!again.dirty);
        // Saving the unchanged reopened gzip document writes NOTHING.
        let before = fs::read(&path).unwrap();
        let p::SaveOutcome::Saved {
            preserved,
            bytes_written,
            ..
        } = svc2.save(again.session)
        else {
            panic!()
        };
        assert!(preserved);
        assert_eq!(bytes_written, 0);
        assert_eq!(fs::read(&path).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn external_change_is_reported_and_never_silently_overwritten() {
        let dir = tmp("ext");
        let path = dir.join("a.wrl");
        fs::write(&path, "#VRML V2.0 utf8\n").unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        edit(&svc, doc.session, 0, 0, 0, "# mine\n");
        fs::write(&path, "#VRML V2.0 utf8\n# theirs\n").unwrap();
        assert!(svc.check_external(doc.session).unwrap().changed);
        assert_eq!(
            svc.save(doc.session),
            p::SaveOutcome::Conflict {
                reason: "size".into()
            }
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#VRML V2.0 utf8\n# theirs\n"
        );
        // Reload adopts theirs and drops the buffer edits.
        let re = opened(svc.reload(doc.session));
        assert_eq!(re.view, "#VRML V2.0 utf8\n# theirs\n");
        assert!(!re.dirty);
        // A stale edit against the pre-reload revision is refused.
        let r = svc
            .edit(&p::EditRequest {
                session: doc.session,
                base_revision: 1,
                from: 0,
                to: 0,
                insert: "x".into(),
            })
            .unwrap();
        assert!(matches!(r, p::EditOutcome::Refused { .. }));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_redo_through_the_service() {
        let dir = tmp("undo");
        let path = dir.join("a.wrl");
        fs::write(&path, "A\r\nB").unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        edit(&svc, doc.session, 0, 1, 2, "");
        assert_eq!(svc.text(doc.session).unwrap(), "AB");
        let p::HistoryOutcome::Applied { view, state } = svc.undo(doc.session).unwrap() else {
            panic!()
        };
        assert_eq!(view, "A\nB");
        assert!(!state.dirty);
        assert_eq!(svc.text(doc.session).unwrap(), "A\r\nB");
        assert_eq!(svc.undo(doc.session).unwrap(), p::HistoryOutcome::Nothing);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn analysis_maps_spans_to_view_coordinates() {
        let dir = tmp("an");
        let path = dir.join("a.wrl");
        fs::write(
            &path,
            "#VRML V2.0 utf8\r\n\r\nDEF A Group { children [ USE A ] }\r\n",
        )
        .unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a = svc.analyze(doc.session).unwrap();
        assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
        assert_eq!(a.items.len(), 2);
        let g = &a.items[0];
        assert_eq!(
            &doc.view[g.view_from as usize..g.view_to as usize],
            "DEF A Group { children [ USE A ] }"
        );
        let insp = svc.inspect(doc.session, &g.id).unwrap().unwrap();
        assert_eq!(insp.title, "Group A");
        assert_eq!(insp.rows[1].name, "children");
        assert_eq!(insp.rows[1].source, "[ USE A ]");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_sessions_and_unreadable_files_fail_closed() {
        let svc = Service::new(clock);
        assert!(svc.snapshot(99).is_err());
        assert!(matches!(svc.save(99), p::SaveOutcome::Failed { .. }));
        assert!(matches!(
            svc.open_path(Path::new("/nonexistent/x.wrl")),
            p::OpenOutcome::Failed { .. }
        ));
    }

    #[test]
    fn view_map_matches_document_mapping() {
        let t = "a\r\n😀\r\nb\rc\n";
        let vm = ViewMap::new(t);
        let d = Document::new(t.into());
        for s in [0u64, 1, 3, 5, 7, 8, 9, 10, 11] {
            assert_eq!(vm.view(s), d.source_to_view(s), "source {s}");
        }
    }
}
