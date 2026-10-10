// SPDX-License-Identifier: GPL-3.0-or-later
//! The document service: Rust-owned sessions, one canonical buffer each.
//!
//! Replaces the Electron main-process editor lane (`src/editor/session.js`,
//! `session-store.js`, `editor-controller.js`, the `editor:*` IPC handlers in
//! `main.js`) for the Tauri application. Tauri-free, so every behavior here is
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
use wrlforge_document::{map_span, view_of, Applied, Document, SpanChange, ViewEdit};
use wrlforge_text::Edit;
use wrlforge_vrml::{create, field_edit, highlight, manipulate, scene};

use crate::files::{self, FileError, Format, SaveOptions, Stamp};

pub struct Session {
    /// `None` for a new world that has no file yet (VISUAL-1). A path is
    /// assigned only by a verified Save As.
    path: Option<PathBuf>,
    format: Format,
    /// The disk state at open / last save; `None` while untitled.
    stamp: Option<Stamp>,
    doc: Document,
}

/// The largest runtime graph a pick snapshot may carry (the adapter's own
/// climb bound).
const MAX_PICK_GRAPH: usize = 256;

/// The file name a new world suggests in Save As, and shows until saved.
pub const UNTITLED_NAME: &str = "untitled.wrl";

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

/// The UTF-16 source span named by a Scene Tree NODE item id
/// (`node-<from>-<to>`). Any other kind is not a node instance.
/// A plain-data adapter snapshot (`pick` or `snapshotSpan`) as the resolver's
/// hit, rooted at the runtime node labelled `shape`.
fn hit_of(shape: &str, snap: &p::PickSnapshot) -> wrlforge_vrml::pick::Hit {
    use wrlforge_vrml::pick as pk;
    pk::Hit {
        shape: shape.to_string(),
        ctx: pk::Ctx::parse(snap.ctx_kind.as_deref().unwrap_or("")),
        sensors: snap.sensors.clone(),
        graph: snap
            .graph
            .iter()
            .map(|(k, g)| {
                (
                    k.clone(),
                    pk::GraphNode {
                        type_name: g.type_name.clone(),
                        ctx: pk::Ctx::parse(&g.ctx_kind),
                        occurrences: g.occurrences.iter().map(|o| (o.start, o.end)).collect(),
                        parents: g.parents.iter().map(|q| pk::Parent::parse(q)).collect(),
                    },
                )
            })
            .collect(),
    }
}

/// The `doc_pick` / native pick reply for one resolution. A proven
/// resolution needs the view map of the text it was resolved against.
pub(crate) fn pick_outcome(
    res: wrlforge_vrml::pick::Resolution,
    generation: u64,
    revision: u64,
    vm: Option<&ViewMap>,
) -> p::PickOutcome {
    use wrlforge_vrml::pick as pk;
    let status = res.status();
    let reason = res.reason().to_string();
    let node = |n: &pk::SourceNode| {
        let vm = vm.expect("a proven pick carries its view map");
        p::PickNode {
            from: n.from,
            to: n.to,
            view_from: vm.view(n.from),
            view_to: vm.view(n.to),
            node_type: n.node_type.clone(),
        }
    };
    let (item, role, logical, shape) = match &res {
        pk::Resolution::Proven(pr) => (
            Some(pr.item.clone()),
            Some(pr.role.as_str().to_string()),
            Some(node(&pr.logical)),
            Some(node(&pr.shape)),
        ),
        _ => (None, None, None, None),
    };
    p::PickOutcome {
        status: status.as_str().into(),
        message: pk::refusal_text(status, &reason),
        reason,
        generation,
        revision,
        item,
        role,
        logical,
        shape,
    }
}

/// UTF-16 units the canonical text has before its preview text: 1 for a
/// leading U+FEFF (the preview omits it), else 0.
fn preview_offset(text: &str) -> u64 {
    u64::from(text.starts_with('\u{FEFF}'))
}

pub fn node_span(item: &str) -> Option<(u64, u64)> {
    let rest = item.strip_prefix("node-")?;
    let (a, b) = rest.split_once('-')?;
    let from = a.parse().ok()?;
    let to = b.parse().ok()?;
    (from < to).then_some((from, to))
}

fn node_fields(n: field_edit::NodeFields, vm: &ViewMap) -> p::NodeFields {
    p::NodeFields {
        editable: n.editable,
        reason: n.reason.into(),
        node_type: n.node_type,
        fields: n
            .fields
            .into_iter()
            .map(|f| p::EditableField {
                index: f.index as u32,
                name: f.name,
                field_type: f.field_type.map(Into::into),
                declaration: f.declaration.map(Into::into),
                kind: f.kind.map(|k| k.as_str().into()),
                editable: f.editable,
                reason: f.reason.into(),
                components: f
                    .components
                    .into_iter()
                    .map(|c| p::FieldComponent {
                        label: c.label.into(),
                        text: c.text,
                        bool_value: match c.value {
                            field_edit::ComponentValue::Bool(b) => Some(b),
                            _ => None,
                        },
                    })
                    .collect(),
                bounds: f.bounds.map(|b| b.text()),
                constraint_note: f.constraint_note.map(Into::into),
                value_excerpt: f.value_excerpt,
                view_from: vm.view(f.from),
                view_to: vm.view(f.to),
            })
            .collect(),
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
            name: match &s.path {
                Some(p) => p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                None => UNTITLED_NAME.into(),
            },
            display_path: match &s.path {
                Some(p) => p.display().to_string(),
                None => "New world (not saved yet)".into(),
            },
            format: s.format.as_str().into(),
            view: s.doc.view(),
            revision: s.doc.revision(),
            dirty: s.doc.dirty(),
            eol: s.doc.eol().name().into(),
            eol_mixed: counts.mixed(),
            bom: s.doc.text().starts_with('\u{FEFF}'),
            bytes_on_disk: s.stamp.as_ref().map_or(0, |st| st.size),
            can_undo: s.doc.can_undo(),
            can_redo: s.doc.can_redo(),
            untitled: s.path.is_none(),
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
        self.insert(Session {
            path: Some(path.to_path_buf()),
            format: loaded.format,
            stamp: Some(loaded.stamp),
            doc: Document::new(loaded.text),
        })
    }

    /// A new, empty VRML97 world in a new session. It has NO path and no
    /// file until a Save As: nothing is written, no temporary file exists.
    /// It is in the canonical document engine like any opened file, and is
    /// not dirty until it is changed.
    pub fn new_world(&self) -> p::OpenOutcome {
        self.insert(Session {
            path: None,
            format: Format::Plain,
            stamp: None,
            doc: Document::new(create::NEW_WORLD.into()),
        })
    }

    /// Whether `id` holds changes that are not on disk (the New World
    /// confirmation asks Rust, never the UI's copy of the flag).
    pub fn is_dirty(&self, id: p::SessionId) -> Result<bool, String> {
        self.with(id, |s| s.doc.dirty())
    }

    fn insert(&self, s: Session) -> p::OpenOutcome {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
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
                Ok(a) => {
                    // An empty no-op edit changes nothing (same revision): the
                    // item stays as it was.
                    let item = if a.revision == r.base_revision {
                        r.item.clone()
                    } else {
                        Self::carry(r.item.as_deref(), s.doc.last_changes())
                    };
                    p::EditOutcome::Applied {
                        state: Self::state(s, a),
                        item,
                    }
                }
                Err(e) => p::EditOutcome::Refused {
                    message: e.to_string(),
                },
            }
        })
    }

    pub fn undo(&self, id: p::SessionId) -> Result<p::HistoryOutcome, String> {
        self.history(id, true, None)
    }
    pub fn redo(&self, id: p::SessionId) -> Result<p::HistoryOutcome, String> {
        self.history(id, false, None)
    }
    /// Undo/redo carrying the selected Scene Tree item across the change.
    /// The node span is mapped through the EXACT changes applied
    /// (`wrlforge_document::map_span`); a change touching its boundary loses
    /// the selection (`item: None`). Never a search or a nearest match.
    pub fn history_with_item(
        &self,
        id: p::SessionId,
        undo: bool,
        item: Option<&str>,
    ) -> Result<p::HistoryOutcome, String> {
        self.history(id, undo, item)
    }
    fn history(
        &self,
        id: p::SessionId,
        undo: bool,
        item: Option<&str>,
    ) -> Result<p::HistoryOutcome, String> {
        self.with(id, |s| {
            let r = if undo { s.doc.undo() } else { s.doc.redo() };
            match r {
                Ok(a) => {
                    let item = Self::carry(item, s.doc.last_changes());
                    p::HistoryOutcome::Applied {
                        state: Self::state(s, a),
                        view: s.doc.view(),
                        item,
                    }
                }
                Err(_) => p::HistoryOutcome::Nothing,
            }
        })
    }

    /// Carry a node item id through the exact changes just applied. A
    /// change touching its boundary, or a non-node item, loses it (`None`):
    /// never a search or a nearest match.
    fn carry(item: Option<&str>, changes: &[SpanChange]) -> Option<String> {
        item.and_then(node_span)
            .and_then(|(f, t)| map_span(f, t, changes))
            .map(|(f, t)| format!("node-{f}-{t}"))
    }

    /// Save to the session's own path: conflict-checked, gzip-preserving.
    pub fn save(&self, id: p::SessionId) -> p::SaveOutcome {
        let now = (self.clock)();
        let r = self.with(id, |s| {
            let (Some(path), Some(stamp)) = (s.path.clone(), s.stamp.as_ref()) else {
                // An untitled world has no destination: only Save As (a
                // native dialog) may give it one.
                return p::SaveOutcome::Failed {
                    message: "this world has no file yet; use Save As".into(),
                };
            };
            let opts = SaveOptions {
                preserve_existing_gzip: true,
                ..Default::default()
            };
            let result = files::safe_save(&path, s.doc.text(), s.format, Some(stamp), opts, now);
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
                s.path = Some(path.to_path_buf());
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
                s.stamp = Some(saved.stamp);
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
        self.with(id, |s| match (&s.stamp, &s.path) {
            (Some(stamp), Some(path)) => {
                let c = files::poll_external_change(stamp, path);
                p::ExternalStatus {
                    changed: c != files::Change::Unchanged,
                    reason: c.reason().into(),
                }
            }
            // No file: nothing on disk can change behind the buffer.
            _ => p::ExternalStatus {
                changed: false,
                reason: files::Change::Unchanged.reason().into(),
            },
        })
    }

    /// Discard buffer edits and re-read the file (the conflict "Reload" path).
    pub fn reload(&self, id: p::SessionId) -> p::OpenOutcome {
        let r = self.with(id, |s| match s.path.as_deref().map(files::load) {
            None => p::OpenOutcome::Failed {
                message: "this world has no file to reload".into(),
            },
            Some(Ok(l)) => {
                s.format = l.format;
                s.stamp = Some(l.stamp);
                s.doc.reset(l.text);
                p::OpenOutcome::Opened {
                    doc: Self::info(id, s),
                }
            }
            Some(Err(e)) => p::OpenOutcome::Failed {
                message: e.to_string(),
            },
        });
        r.unwrap_or_else(|m| p::OpenOutcome::Failed { message: m })
    }

    /// A copy of one revision's text, taken under the session lock. Parsing
    /// then runs WITHOUT the lock, so an edit never waits behind a parse.
    pub(crate) fn text_at(&self, id: p::SessionId) -> Result<(String, u64), String> {
        self.with(id, |s| (s.doc.text().to_string(), s.doc.revision()))
    }

    pub fn analyze(&self, id: p::SessionId) -> Result<p::Analysis, String> {
        let (text, revision) = self.text_at(id)?;
        let text = text.as_str();
        {
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
            // Highlights come from THIS parse; no second parse for color.
            let spans = highlight::highlight(&parsed, text);
            let highlights_truncated = spans.len() > p::syntax::MAX_SPANS;
            let highlights = p::syntax::encode(
                spans
                    .iter()
                    .take(p::syntax::MAX_SPANS)
                    .map(|h| p::syntax::Span {
                        from: vm.view(h.from as u64),
                        to: vm.view(h.to as u64),
                        class: h.class as u8,
                    })
                    .filter(|s| s.to > s.from),
            );
            let view = view_of(text);
            Ok(p::Analysis {
                session: id,
                revision,
                view_hash: p::view_hash(&view),
                view_len: view.encode_utf16().count() as u64,
                highlights,
                highlights_truncated,
                items,
                diagnostics,
                resolution_scope: tree.resolution_scope.into(),
                truncated: parsed.truncated,
            })
        }
    }

    /// Inspect `item` as it is in `revision`. Item ids are source spans of
    /// one revision, so any other revision is refused (`Stale`), never
    /// re-interpreted against the current text.
    pub fn inspect(
        &self,
        id: p::SessionId,
        item: &str,
        revision: u64,
    ) -> Result<p::InspectOutcome, String> {
        let (text, current) = self.text_at(id)?;
        if revision != current {
            return Ok(p::InspectOutcome::Stale { current });
        }
        let text = text.as_str();
        {
            let parsed = wrlforge_vrml::parse(text);
            let vm = ViewMap::new(text);
            let node = node_span(item).map(|(from, to)| {
                node_fields(
                    field_edit::inspect_node_fields(&parsed, text, from, to),
                    &vm,
                )
            });
            let found = scene::inspect(&parsed.tree, text, item).map(|i| p::Inspection {
                revision,
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
                node,
            });
            Ok(match found {
                Some(inspection) => p::InspectOutcome::Found { inspection },
                None => p::InspectOutcome::Missing,
            })
        }
    }

    /// An Inspector field edit. Rust re-proves everything: the revision, the
    /// node (exactly one node at the item's span in THIS revision), the field,
    /// the type and the value. A ready plan is applied as ONE transaction
    /// whose result must equal the planned, round-trip-verified text.
    pub fn edit_field(&self, r: &p::FieldEditRequest) -> Result<p::FieldEditOutcome, String> {
        self.with(r.session, |s| {
            let refused = |reason: &str, message: Option<String>| p::FieldEditOutcome::Refused {
                reason: reason.into(),
                message,
                component_index: None,
            };
            if r.base_revision != s.doc.revision() {
                return refused(
                    "parse-session-is-stale",
                    Some(format!(
                        "The document changed (revision {} → {}); re-select the node.",
                        r.base_revision,
                        s.doc.revision()
                    )),
                );
            }
            let Some((node_from, node_to)) = node_span(&r.item) else {
                return refused(field_edit::reason::NOT_A_NODE, None);
            };
            let req = field_edit::Request {
                node_from,
                node_to,
                field_index: r.field_index as usize,
                field_name: r.field_name.clone(),
                components: r
                    .components
                    .iter()
                    .map(|c| match c {
                        p::FieldInput::Bool { value } => field_edit::Input::Bool(*value),
                        p::FieldInput::Text { value } => field_edit::Input::Text(value.clone()),
                    })
                    .collect(),
            };
            match field_edit::plan_field_edit(s.doc.text(), &req) {
                field_edit::Plan::Unchanged => p::FieldEditOutcome::Unchanged,
                field_edit::Plan::Refused {
                    reason,
                    message,
                    component_index,
                } => p::FieldEditOutcome::Refused {
                    reason: reason.into(),
                    message,
                    component_index: component_index.map(|i| i as u32),
                },
                field_edit::Plan::Ready {
                    edits,
                    new_text,
                    changed,
                } => {
                    // Every edit lies inside the node: its start is fixed and
                    // its end moves by the total length change.
                    let delta: i64 = edits
                        .iter()
                        .map(|e| e.insert.encode_utf16().count() as i64 - (e.to - e.from) as i64)
                        .sum();
                    let item = format!("node-{node_from}-{}", node_to as i64 + delta);
                    let edits: Vec<Edit> = edits
                        .into_iter()
                        .map(|e| Edit {
                            from: e.from,
                            to: e.to,
                            insert: e.insert,
                        })
                        .collect();
                    match s
                        .doc
                        .apply_source_transaction(r.base_revision, &edits, &new_text)
                    {
                        Ok(a) => p::FieldEditOutcome::Applied {
                            state: Self::state(s, a),
                            view: s.doc.view(),
                            changed: changed.into_iter().map(|i| i as u32).collect(),
                            item,
                        },
                        Err(e) => refused(
                            field_edit::reason::TRANSACTION_REJECTED,
                            Some(e.to_string()),
                        ),
                    }
                }
            }
        })
    }

    /// Create one primitive object (VISUAL-1). Rust re-proves everything:
    /// the revision, a top-level insertion point, a free DEF name and -- by
    /// re-parsing the result -- the exact new structure at the exact span.
    /// The insert is applied as ONE transaction (one undo step) whose result
    /// must equal the planned text. The reply's item is the new Transform's
    /// Scene Tree id in the new revision.
    pub fn create(&self, r: &p::CreateRequest) -> Result<p::CreateOutcome, String> {
        self.with(r.session, |s| {
            if r.base_revision != s.doc.revision() {
                return p::CreateOutcome::Refused {
                    reason: "parse-session-is-stale".into(),
                    message: format!(
                        "The document changed (revision {} → {}); nothing was created.",
                        r.base_revision,
                        s.doc.revision()
                    ),
                };
            }
            let prim = match r.primitive {
                p::Primitive::Box => create::Primitive::Box,
                p::Primitive::Sphere => create::Primitive::Sphere,
                p::Primitive::Cylinder => create::Primitive::Cylinder,
                p::Primitive::Cone => create::Primitive::Cone,
            };
            match create::plan_create(s.doc.text(), prim) {
                create::Plan::Refused { reason, message } => p::CreateOutcome::Refused {
                    reason: reason.into(),
                    message,
                },
                create::Plan::Ready {
                    at,
                    insert,
                    new_text,
                    node_from,
                    node_to,
                    def_name,
                } => {
                    let edit = Edit {
                        from: at,
                        to: at,
                        insert,
                    };
                    match s
                        .doc
                        .apply_source_transaction(r.base_revision, &[edit], &new_text)
                    {
                        Ok(a) => p::CreateOutcome::Applied {
                            state: Self::state(s, a),
                            view: s.doc.view(),
                            item: format!("node-{node_from}-{node_to}"),
                            def_name,
                        },
                        Err(e) => p::CreateOutcome::Refused {
                            reason: field_edit::reason::TRANSACTION_REJECTED.into(),
                            message: e.to_string(),
                        },
                    }
                }
            }
        })
    }

    /// VISUAL-3A: whether the translation gizmo may move `item` as it is in
    /// `revision` (read-only). Any other revision is `Stale`, never
    /// re-interpreted against the current text.
    pub fn translate_target(
        &self,
        r: &p::TranslateTargetRequest,
    ) -> Result<p::TranslateTargetOutcome, String> {
        let (text, current) = self.text_at(r.session)?;
        if r.revision != current {
            return Ok(p::TranslateTargetOutcome::Stale { current });
        }
        let Some((from, to)) = node_span(&r.item) else {
            return Ok(p::TranslateTargetOutcome::Refused {
                reason: field_edit::reason::NOT_A_NODE.into(),
                message: "Select an object (a Transform) to move it.".into(),
            });
        };
        Ok(match manipulate::translate_target(&text, from, to) {
            Ok(t) => p::TranslateTargetOutcome::Ready {
                revision: current,
                item: r.item.clone(),
                def_name: t.def,
                root_index: t.root_index as u32,
                preview_from: from - preview_offset(&text),
                preview_to: to - preview_offset(&text),
                translation: t.translation,
                origin: t.origin,
            },
            Err(e) => p::TranslateTargetOutcome::Refused {
                reason: e.reason.into(),
                message: e.message,
            },
        })
    }

    /// VISUAL-3A1: prove that the runtime node the preview located for
    /// `item` IS that authored, movable, top-level Transform. Read-only.
    ///
    /// The generation must have rendered exactly the text held now
    /// (revision + preview hash); the item must still be a movable target;
    /// the snapshot must be a span lookup whose runtime chain proves, link by
    /// link (`pick::resolve_node`, the VISUAL-2 proof), the exact authored
    /// node at the item's span, directly under the scene. Root index, DEF
    /// name and translation value play no part in this proof.
    pub fn translate_prove(
        &self,
        r: &p::TranslateProveRequest,
    ) -> Result<p::TranslateProveOutcome, String> {
        use wrlforge_vrml::pick as pk;
        let (text, current) = self.text_at(r.session)?;
        let refused = |reason: &str, message: &str| p::TranslateProveOutcome::Refused {
            reason: reason.into(),
            message: message.into(),
        };
        let preview = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        if r.revision != current || p::preview_hash(preview) != r.preview_hash {
            return Ok(p::TranslateProveOutcome::Stale { current });
        }
        let Some((from, to)) = node_span(&r.item) else {
            return Ok(refused(field_edit::reason::NOT_A_NODE, "Not an object."));
        };
        if let Err(e) = manipulate::translate_target(&text, from, to) {
            return Ok(refused(e.reason, &e.message));
        }
        let unprovable = "The preview cannot prove which rendered object is this Transform, so it cannot be moved here. Use the Inspector.";
        let snap = &r.snapshot;
        let (Some(shape), "found", 1..=MAX_PICK_GRAPH) = (
            snap.shape.as_deref(),
            snap.outcome.as_str(),
            snap.graph.len(),
        ) else {
            let why = snap
                .reason
                .clone()
                .unwrap_or_else(|| pk::reason::BIND_NOT_UNIQUE.to_string());
            return Ok(refused(&why, unprovable));
        };
        let parsed = wrlforge_vrml::parse(&text);
        let hit = hit_of(shape, snap);
        Ok(
            match pk::resolve_node(&hit, &parsed, preview_offset(&text), from, to) {
                Ok(chain) if chain.len() == 1 && chain[0].node_type == "Transform" => {
                    p::TranslateProveOutcome::Proven {
                        revision: current,
                        item: r.item.clone(),
                    }
                }
                Ok(_) => refused(manipulate::reason::NOT_TOP_LEVEL, unprovable),
                Err(res) => refused(res.reason(), unprovable),
            },
        )
    }

    /// VISUAL-3A1 camera carry: the preview span `[from, to)` of
    /// `from_revision`, mapped through the exact logged changes to the
    /// current revision, or `Lost`. Never a search. A change at offset 0 may
    /// add or remove a BOM (which shifts the preview projection), so it
    /// loses the span too.
    pub fn preview_carry(
        &self,
        r: &p::PreviewCarryRequest,
    ) -> Result<p::PreviewCarryOutcome, String> {
        let lost = |why: &str| p::PreviewCarryOutcome::Lost { reason: why.into() };
        self.with(r.session, |s| {
            if r.to_revision != s.doc.revision() {
                return lost("not-the-current-revision");
            }
            let Some(changes) = s.doc.changes_since(r.from_revision) else {
                return lost("revision-not-in-change-log");
            };
            if changes.iter().any(|c| c.from == 0) {
                return lost("change-at-document-start");
            }
            let bom = preview_offset(s.doc.text());
            match map_span(r.from + bom, r.to + bom, &changes) {
                Some((f, t)) => p::PreviewCarryOutcome::Mapped {
                    from: f - bom,
                    to: t - bom,
                },
                None => lost("change-touches-span"),
            }
        })
    }

    /// VISUAL-3A: commit one gizmo drag. Rust re-proves everything (the
    /// revision, the Transform at the item's span, the explicit translation)
    /// and writes ONE token, as ONE transaction whose result must equal the
    /// planned, round-trip-verified text.
    pub fn translate(&self, r: &p::TranslateRequest) -> Result<p::TranslateOutcome, String> {
        self.with(r.session, |s| {
            let refused = |reason: &str, message: String| p::TranslateOutcome::Refused {
                reason: reason.into(),
                message,
            };
            if r.base_revision != s.doc.revision() {
                return refused(
                    "parse-session-is-stale",
                    format!(
                        "The document changed (revision {} → {}); the object was not moved.",
                        r.base_revision,
                        s.doc.revision()
                    ),
                );
            }
            let Some((node_from, node_to)) = node_span(&r.item) else {
                return refused(field_edit::reason::NOT_A_NODE, "Not an object.".into());
            };
            let axis = match r.axis {
                p::gizmo::Axis::X => manipulate::Axis::X,
                p::gizmo::Axis::Y => manipulate::Axis::Y,
                p::gizmo::Axis::Z => manipulate::Axis::Z,
            };
            let (plan, text) = manipulate::plan_translate(
                s.doc.text(),
                node_from,
                node_to,
                axis,
                r.value,
                r.decimals,
            );
            match plan {
                field_edit::Plan::Unchanged => p::TranslateOutcome::Unchanged,
                field_edit::Plan::Refused {
                    reason, message, ..
                } => refused(reason, message.unwrap_or_else(|| reason.to_string())),
                field_edit::Plan::Ready {
                    edits, new_text, ..
                } => {
                    // The edit lies inside the node: its start is fixed and
                    // its end moves by the length change.
                    let delta: i64 = edits
                        .iter()
                        .map(|e| e.insert.encode_utf16().count() as i64 - (e.to - e.from) as i64)
                        .sum();
                    let item = format!("node-{node_from}-{}", node_to as i64 + delta);
                    let edits: Vec<Edit> = edits
                        .into_iter()
                        .map(|e| Edit {
                            from: e.from,
                            to: e.to,
                            insert: e.insert,
                        })
                        .collect();
                    match s
                        .doc
                        .apply_source_transaction(r.base_revision, &edits, &new_text)
                    {
                        Ok(a) => p::TranslateOutcome::Applied {
                            state: Self::state(s, a),
                            view: s.doc.view(),
                            item,
                            text: text.unwrap_or_default(),
                        },
                        Err(e) => refused(field_edit::reason::TRANSACTION_REJECTED, e.to_string()),
                    }
                }
            }
        })
    }

    /// Preview input: the canonical text, minus a leading U+FEFF. X_ITE's
    /// VRML parser rejects a BOM before `#VRML`; stripping it here is a
    /// display projection only -- the document and the file keep the BOM.
    pub fn preview_source(&self, id: p::SessionId) -> Result<p::PreviewSource, String> {
        self.with(id, |s| {
            let t = s.doc.text();
            let text = t.strip_prefix('\u{FEFF}').unwrap_or(t).to_string();
            p::PreviewSource {
                revision: s.doc.revision(),
                hash: p::preview_hash(&text),
                text,
            }
        })
    }

    /// Resolve one viewport pick (VISUAL-2) against the document AS IT IS
    /// NOW. Read-only: nothing here changes the buffer, the revision, the
    /// dirty flag or the undo history.
    ///
    /// The snapshot's preview generation rendered (`revision`,
    /// `preview_hash`); unless the document still holds exactly that text,
    /// the pick is `REFUSED_STALE`. Otherwise the provenance spans are joined
    /// to ONE parse of that text (`wrlforge_vrml::pick`), never re-matched.
    pub fn pick(&self, r: &p::PickRequest) -> Result<p::PickOutcome, String> {
        use wrlforge_vrml::pick::{self as pk, reason, Status};
        let (text, revision) = self.text_at(r.session)?;
        let done = |res: pk::Resolution, vm: Option<&ViewMap>| pick_outcome(res, r.generation, revision, vm);
        let snap = &r.snapshot;
        let why = |d: &str| snap.reason.clone().unwrap_or_else(|| d.to_string());
        if snap.outcome == "disabled" {
            let res = pk::refuse(Status::CompatibilityDisabled, why("compatibility-unproven"));
            return Ok(done(res, None));
        }
        // The generation must have rendered exactly the text held now.
        let preview = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        if r.revision != revision || p::preview_hash(preview) != r.preview_hash {
            let res = pk::refuse(Status::RefusedStale, reason::SOURCE_CHANGED);
            return Ok(done(res, None));
        }
        let res = match snap.outcome.as_str() {
            "stale" => pk::refuse(Status::RefusedStale, why(reason::LAST_VALID_SCENE)),
            "external" => pk::refuse(Status::RefusedExternal, why(reason::OTHER_DOCUMENT)),
            "no-hit" => pk::refuse(Status::NoHit, why(reason::NO_GEOMETRY)),
            "hit" => match (&snap.shape, snap.graph.len()) {
                (Some(shape), 1..=MAX_PICK_GRAPH) => {
                    let hit = hit_of(shape, snap);
                    let parsed = wrlforge_vrml::parse(&text);
                    let tree = scene::build_scene_tree(&parsed.tree, &text);
                    // The preview omits a leading U+FEFF: one UTF-16 unit.
                    let offset = u64::from(preview.len() != text.len());
                    let vm = ViewMap::new(&text);
                    return Ok(done(pk::resolve(&hit, &parsed, &tree, offset), Some(&vm)));
                }
                (None, _) => pk::refuse(Status::NoHit, reason::NO_GEOMETRY),
                _ => pk::refuse(Status::Unsupported, reason::CHAIN_UNBOUNDED),
            },
            _ => pk::refuse(Status::Unsupported, why(reason::GENERATION_UNPROVABLE)),
        };
        Ok(done(res, None))
    }

    /// Test/smoke support: the exact canonical text.
    pub fn text(&self, id: p::SessionId) -> Result<String, String> {
        self.with(id, |s| s.doc.text().to_string())
    }
}

#[cfg(test)]
impl Service {
    /// A session holding `text` (no path), for tests in other modules.
    pub(crate) fn open_text_for_test(&self, text: &str) -> p::SessionId {
        match self.insert(Session {
            path: None,
            format: Format::Plain,
            stamp: None,
            doc: Document::new(text.into()),
        }) {
            p::OpenOutcome::Opened { doc } => doc.session,
            _ => panic!("open failed"),
        }
    }

    /// One view edit at the current revision.
    pub(crate) fn edit_for_test(&self, id: p::SessionId, from: u64, to: u64, insert: &str) {
        let base_revision = self.text_at(id).unwrap().1;
        let r = p::EditRequest { session: id, base_revision, from, to, insert: insert.into(), item: None };
        assert!(matches!(self.edit(&r).unwrap(), p::EditOutcome::Applied { .. }));
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

    fn found(r: Result<p::InspectOutcome, String>) -> p::Inspection {
        match r.unwrap() {
            p::InspectOutcome::Found { inspection } => inspection,
            other => panic!("not found: {other:?}"),
        }
    }

    /// One edit carrying a selected item; returns (state, carried item).
    fn edit_item(
        svc: &Service,
        id: u64,
        rev: u64,
        (from, to, ins): (u64, u64, &str),
        item: &str,
    ) -> (p::DocState, Option<String>) {
        match svc
            .edit(&p::EditRequest {
                session: id,
                base_revision: rev,
                from,
                to,
                insert: ins.into(),
                item: Some(item.into()),
            })
            .unwrap()
        {
            p::EditOutcome::Applied { state, item } => (state, item),
            other => panic!("refused: {other:?}"),
        }
    }

    /// UI-EDITOR-2: an item id is a source span of ONE revision. Inspect
    /// refuses any other revision; an edit carries the selection through the
    /// exact change or loses it -- including the case where a DIFFERENT
    /// node now sits at the old id's span.
    #[test]
    fn inspect_and_selection_are_bound_to_one_revision() {
        let dir = tmp("rev-bound");
        let path = dir.join("two.wrl");
        let src = "#VRML V2.0 utf8\r\nDEF A Group {}\r\nDEF B Group { children [] }\r\n";
        fs::write(&path, src).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a0 = svc.analyze(doc.session).unwrap();
        let (ia, ib) = (&a0.items[0], &a0.items[1]);
        assert_eq!(
            (ia.label.as_str(), ib.label.as_str()),
            ("Group A", "Group B")
        );
        assert_eq!(found(svc.inspect(doc.session, &ia.id, 0)).title, "Group A");

        // Typing inside B (view offsets; CRLF source) keeps B, end moved.
        let inside = ib.view_from + "DEF B Group { ".len() as u64;
        let (st, carried) = edit_item(&svc, doc.session, 0, (inside, inside, "x"), &ib.id);
        let a1 = svc.analyze(doc.session).unwrap();
        let b1 = a1.items.iter().find(|i| i.label == "Group B").unwrap();
        assert_eq!(carried.as_deref(), Some(b1.id.as_str()));
        assert_eq!(st.revision, 1);
        // The old revision's ids are refused, never re-read against rev 1.
        assert_eq!(
            svc.inspect(doc.session, &ib.id, 0).unwrap(),
            p::InspectOutcome::Stale { current: 1 }
        );
        // An empty no-op edit keeps the item and the revision.
        let (st, same) = edit_item(&svc, doc.session, 1, (0, 0, ""), &b1.id);
        assert_eq!((st.revision, same.as_deref()), (1, Some(b1.id.as_str())));

        // Delete "DEF A Group {}\n": B moves onto A's old span. A selected
        // A is LOST (the change crosses it), not carried onto B.
        let a_start = ia.view_from;
        let (st, lost) = edit_item(
            &svc,
            doc.session,
            1,
            (a_start, a_start + "DEF A Group {}\n".len() as u64, ""),
            &a1.items[0].id,
        );
        assert_eq!((st.revision, lost), (2, None));
        let a2 = svc.analyze(doc.session).unwrap();
        assert_eq!(a2.items.len(), 1);
        // B now has A's old span start. Inspecting A's rev-1 id at rev 2 is
        // refused, so B's fields can never appear for A's selection.
        assert_eq!(
            svc.inspect(doc.session, &a1.items[0].id, 1).unwrap(),
            p::InspectOutcome::Stale { current: 2 }
        );
        assert_eq!(
            found(svc.inspect(doc.session, &a2.items[0].id, 2)).title,
            "Group B"
        );
        // A boundary-touching edit loses the selection.
        let b2 = &a2.items[0];
        let (_, lost) = edit_item(
            &svc,
            doc.session,
            2,
            (b2.view_from, b2.view_from, "#"),
            &b2.id,
        );
        assert_eq!(lost, None);
        let _ = fs::remove_dir_all(&dir);
    }

    fn edit(svc: &Service, id: u64, rev: u64, from: u64, to: u64, ins: &str) -> p::DocState {
        match svc
            .edit(&p::EditRequest {
                session: id,
                base_revision: rev,
                from,
                to,
                insert: ins.into(),
                item: None,
            })
            .unwrap()
        {
            p::EditOutcome::Applied { state, .. } => state,
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
                item: None,
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
        let p::HistoryOutcome::Applied { view, state, .. } = svc.undo(doc.session).unwrap() else {
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
        let insp = found(svc.inspect(doc.session, &g.id, a.revision));
        assert_eq!(insp.title, "Group A");
        assert_eq!(insp.rows[1].name, "children");
        assert_eq!(insp.rows[1].source, "[ USE A ]");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn syntax_class_codes_match_the_protocol_registry() {
        assert_eq!(highlight::Class::ALL.len(), p::syntax::SYNTAX_CLASSES.len());
        for c in highlight::Class::ALL {
            assert_eq!(c.as_str(), p::syntax::SYNTAX_CLASSES[c as usize].0);
        }
    }

    #[test]
    fn analysis_highlights_map_to_view_coordinates() {
        let dir = tmp("hl");
        let path = dir.join("h.wrl");
        // BOM, CRLF, lone CR, a multiline CRLF string, astral characters.
        let src = "\u{FEFF}#VRML V2.0 utf8\r\n# é😀\rDEF 😀A Transform {\r\n translation 1 -2 3e1\r\n}\r\nWorldInfo { info [ \"a\r\n😀 DEF\" ] }\r\nROUTE 😀A.translation TO X.y\r\n";
        fs::write(&path, src).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a = svc.analyze(doc.session).unwrap();
        assert_eq!(a.session, doc.session);
        assert_eq!(a.revision, doc.revision);
        assert_eq!(a.view_hash, p::view_hash(&doc.view));
        assert_eq!(a.view_len, doc.view.encode_utf16().count() as u64);
        assert!(!a.highlights_truncated);
        let spans = p::syntax::decode(&a.highlights, a.view_len).expect("well-formed");
        let v: Vec<u16> = doc.view.encode_utf16().collect();
        let got: Vec<(String, &str)> = spans
            .iter()
            .map(|s| {
                (
                    String::from_utf16(&v[s.from as usize..s.to as usize]).unwrap(),
                    p::syntax::SYNTAX_CLASSES[s.class as usize].0,
                )
            })
            .collect();
        let want: Vec<(&str, &str)> = vec![
            ("#VRML V2.0 utf8", "header"),
            ("# é😀", "comment"),
            ("DEF", "keyword"),
            ("😀A", "def-name"),
            ("Transform", "node-type"),
            ("{", "punctuation"),
            ("translation", "field"),
            ("1", "number"),
            ("-2", "number"),
            ("3e1", "number"),
            ("}", "punctuation"),
            ("WorldInfo", "node-type"),
            ("{", "punctuation"),
            ("info", "field"),
            ("[", "punctuation"),
            ("\"a\n😀 DEF\"", "string"),
            ("]", "punctuation"),
            ("}", "punctuation"),
            ("ROUTE", "route"),
            ("😀A", "def-ref"),
            (".", "punctuation"),
            ("translation", "field"),
            ("TO", "route"),
            ("X", "def-ref"),
            (".", "punctuation"),
            ("y", "field"),
        ];
        let got_ref: Vec<(&str, &str)> = got.iter().map(|(t, c)| (t.as_str(), *c)).collect();
        assert_eq!(got_ref, want);

        // A new revision gets a new analysis; the old one no longer matches.
        let st = edit(&svc, doc.session, doc.revision, 0, 0, " ");
        let b = svc.analyze(doc.session).unwrap();
        assert_eq!(b.revision, st.revision);
        assert_ne!(b.view_hash, a.view_hash);
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

    fn field_edit(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        field: (u32, &str),
        values: &[&str],
    ) -> p::FieldEditOutcome {
        svc.edit_field(&p::FieldEditRequest {
            session: id,
            base_revision: rev,
            item: item.into(),
            field_index: field.0,
            field_name: field.1.into(),
            components: values
                .iter()
                .map(|v| p::FieldInput::Text {
                    value: v.to_string(),
                })
                .collect(),
        })
        .unwrap()
    }

    #[test]
    fn inspector_field_edit_end_to_end_bom_crlf_cr_unicode_gzip() {
        let dir = tmp("field");
        // BOM, CRLF, a lone CR and non-ASCII text in one file.
        let src = "\u{FEFF}#VRML V2.0 utf8\r\n# Tëst 😀\rDEF Ä Transform { translation 1 2 3\r\n  children Shape { appearance Appearance { material Material { diffuseColor 0.8 0.8 0.8 } } } }\r\nWorldInfo { title \"日本\" }\r\n";
        let path = dir.join("real.wrl");
        fs::write(&path, files::encode(src, Format::Gzip).unwrap()).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a = svc.analyze(doc.session).unwrap();
        assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
        let mat = a.items.iter().find(|i| i.label == "Material").unwrap();
        let insp = found(svc.inspect(doc.session, &mat.id, a.revision));
        let nf = insp.node.unwrap();
        assert!(nf.editable, "{}", nf.reason);
        let f = &nf.fields[0];
        assert_eq!(
            (f.name.as_str(), f.editable, f.field_type.as_deref()),
            ("diffuseColor", true, Some("SFColor"))
        );
        assert_eq!(f.bounds.as_deref(), Some("≥ 0 and ≤ 1"));
        // The edit: one component changes, one undo step.
        let p::FieldEditOutcome::Applied {
            state,
            view,
            changed,
            item,
        } = field_edit(
            &svc,
            doc.session,
            0,
            &mat.id,
            (0, "diffuseColor"),
            &["0.8", "0.25", "0.8"],
        )
        else {
            panic!("not applied")
        };
        assert_eq!(changed, [1]);
        // The returned id is the same Material in the new revision.
        let a2 = svc.analyze(doc.session).unwrap();
        let mat2 = a2.items.iter().find(|i| i.label == "Material").unwrap();
        assert_eq!(mat2.id, item);
        let want = src.replace("diffuseColor 0.8 0.8 0.8", "diffuseColor 0.8 0.25 0.8");
        assert_eq!(svc.text(doc.session).unwrap(), want);
        assert_eq!(view, Document::new(want.clone()).view());
        assert!(state.dirty && state.can_undo && state.revision == 1);
        // Stale revision, out-of-range, wrong node kind, wrong field: refused, no change.
        for (rev, item, field, vals) in [
            (
                0u64,
                mat.id.as_str(),
                (0u32, "diffuseColor"),
                vec!["1", "1", "1"],
            ),
            (1, mat.id.as_str(), (0, "diffuseColor"), vec!["2", "0", "0"]),
            (1, "use-1-2", (0, "diffuseColor"), vec!["1", "1", "1"]),
            (1, mat.id.as_str(), (0, "shininess"), vec!["1"]),
        ] {
            let out = field_edit(&svc, doc.session, rev, item, field, &vals);
            assert!(
                matches!(out, p::FieldEditOutcome::Refused { .. }),
                "{out:?}"
            );
        }
        assert_eq!(svc.text(doc.session).unwrap(), want);
        // Undo / redo are exact.
        // Undo / redo carry the selected node through the exact changes.
        let p::HistoryOutcome::Applied { item: back, .. } = svc
            .history_with_item(doc.session, true, Some(&item))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(back.as_deref(), Some(mat.id.as_str()));
        assert_eq!(svc.text(doc.session).unwrap(), src);
        let p::HistoryOutcome::Applied { item: fwd, .. } = svc
            .history_with_item(doc.session, false, back.as_deref())
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(fwd.as_deref(), Some(item.as_str()));
        assert_eq!(svc.text(doc.session).unwrap(), want);
        // Transform via its own item (a DEF node, non-ASCII name).
        let a = svc.analyze(doc.session).unwrap();
        let tr = a.items.iter().find(|i| i.label == "Transform Ä").unwrap();
        let p::FieldEditOutcome::Applied { .. } = field_edit(
            &svc,
            doc.session,
            3,
            &tr.id,
            (0, "translation"),
            &["1", "-2.5", "3"],
        ) else {
            panic!("transform edit not applied")
        };
        let want = want.replace("translation 1 2 3", "translation 1 -2.5 3");
        assert_eq!(svc.text(doc.session).unwrap(), want);
        // Save and reopen keep everything, BOM and line endings included.
        assert!(matches!(
            svc.save(doc.session),
            p::SaveOutcome::Saved { .. }
        ));
        let again = opened(Service::new(clock).open_path(&path));
        assert_eq!(again.view, Document::new(want.clone()).view());
        let svc2 = Service::new(clock);
        let d2 = opened(svc2.open_path(&path));
        assert_eq!(svc2.text(d2.session).unwrap(), want);
        let _ = fs::remove_dir_all(&dir);
    }

    fn create(svc: &Service, id: u64, rev: u64, prim: p::Primitive) -> p::CreateOutcome {
        svc.create(&p::CreateRequest {
            session: id,
            base_revision: rev,
            primitive: prim,
        })
        .unwrap()
    }

    fn created(o: p::CreateOutcome) -> (p::DocState, String, String) {
        match o {
            p::CreateOutcome::Applied {
                state,
                item,
                def_name,
                ..
            } => (state, item, def_name),
            other => panic!("not created: {other:?}"),
        }
    }

    /// VISUAL-1: New World → Create Box → Inspector translation and color
    /// → Undo / Redo → Save (refused: no file) → Save As → reopen.
    #[test]
    fn new_world_create_edit_undo_save_as_reopen() {
        let dir = tmp("visual1");
        let svc = Service::new(clock);
        let doc = opened(svc.new_world());
        assert!(doc.untitled && !doc.dirty && doc.revision == 0);
        assert_eq!(doc.view, "#VRML V2.0 utf8\n");
        assert_eq!(doc.name, UNTITLED_NAME);
        // Nothing on disk exists for it; no external change can be reported.
        assert!(!svc.check_external(doc.session).unwrap().changed);
        assert!(matches!(
            svc.reload(doc.session),
            p::OpenOutcome::Failed { .. }
        ));

        let (st, item, name) = created(create(&svc, doc.session, 0, p::Primitive::Box));
        assert_eq!((st.revision, st.dirty, st.can_undo), (1, true, true));
        assert_eq!(name, "Box_1");
        // The returned item is the Scene Tree's own id for the Transform.
        let a = svc.analyze(doc.session).unwrap();
        assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
        let tr = a.items.iter().find(|i| i.id == item).unwrap();
        assert_eq!(tr.label, "Transform Box_1");
        // The Inspector edits the authored transform fields.
        let insp = found(svc.inspect(doc.session, &item, 1));
        let nf = insp.node.unwrap();
        let names: Vec<_> = nf
            .fields
            .iter()
            .filter(|f| f.editable)
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(names, ["translation", "rotation", "scale"]);
        let p::FieldEditOutcome::Applied { item: item2, .. } = field_edit(
            &svc,
            doc.session,
            1,
            &item,
            (0, "translation"),
            &["1.5", "0", "-2"],
        ) else {
            panic!("translation not applied")
        };
        // ... and the Material's diffuse color.
        let a = svc.analyze(doc.session).unwrap();
        let mat = a.items.iter().find(|i| i.label == "Material").unwrap();
        let insp = found(svc.inspect(doc.session, &mat.id, 2));
        let dc = &insp.node.unwrap().fields[0];
        assert_eq!(dc.name, "diffuseColor");
        assert!(matches!(
            field_edit(
                &svc,
                doc.session,
                2,
                &mat.id,
                (dc.index, "diffuseColor"),
                &["0.1", "0.9", "0.1"]
            ),
            p::FieldEditOutcome::Applied { .. }
        ));
        let want = svc.text(doc.session).unwrap();
        assert!(want.contains("translation 1.5 0 -2"));
        assert!(want.contains("diffuseColor 0.1 0.9 0.1"));
        // Undo the color, redo it; undo everything back to the empty world.
        svc.undo(doc.session).unwrap();
        assert!(svc
            .text(doc.session)
            .unwrap()
            .contains("diffuseColor 0.8 0.3 0.2"));
        svc.redo(doc.session).unwrap();
        assert_eq!(svc.text(doc.session).unwrap(), want);
        let p::HistoryOutcome::Applied { item: kept, .. } = svc
            .history_with_item(doc.session, true, Some(&item2))
            .unwrap()
        else {
            panic!()
        };
        // The color undo lies strictly inside the Transform: carried.
        let kept = kept.expect("an edit inside the node keeps it");
        let p::HistoryOutcome::Applied { item: kept, .. } = svc
            .history_with_item(doc.session, true, Some(&kept))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(kept.as_deref(), Some(item.as_str()));
        // Undoing the Create removes the node itself: the selection is lost.
        let p::HistoryOutcome::Applied { item: lost, .. } = svc
            .history_with_item(doc.session, true, kept.as_deref())
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(lost, None);
        assert_eq!(svc.text(doc.session).unwrap(), create::NEW_WORLD);
        // One Create is one undo step: redo it whole.
        for _ in 0..3 {
            svc.redo(doc.session).unwrap();
        }
        assert_eq!(svc.text(doc.session).unwrap(), want);

        // Save with no file is refused; Save As assigns the path.
        assert!(matches!(
            svc.save(doc.session),
            p::SaveOutcome::Failed { .. }
        ));
        let path = dir.join("world.wrl");
        let p::SaveOutcome::Saved { doc: saved, .. } = svc.save_as(doc.session, &path) else {
            panic!("save as failed")
        };
        assert!(!saved.untitled && !saved.dirty && saved.name == "world.wrl");
        assert_eq!(fs::read_to_string(&path).unwrap(), want);
        // A later save is the ordinary backup-first, conflict-checked save.
        let r = svc.snapshot(doc.session).unwrap().revision;
        created(create(&svc, doc.session, r, p::Primitive::Cone));
        let p::SaveOutcome::Saved { backup, .. } = svc.save(doc.session) else {
            panic!("save failed")
        };
        assert!(backup.is_some());
        let final_text = svc.text(doc.session).unwrap();
        // Reopen: the same objects, in a fresh service.
        let svc2 = Service::new(clock);
        let again = opened(svc2.open_path(&path));
        assert_eq!(svc2.text(again.session).unwrap(), final_text);
        let a = svc2.analyze(again.session).unwrap();
        let labels: Vec<_> = a
            .items
            .iter()
            .filter(|i| i.depth == 1)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(labels, ["Transform Box_1", "Transform Cone_1"]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Creating in an existing file changes nothing but the appended object:
    /// BOM, CRLF, a lone CR, non-ASCII text and comments stay byte-exact.
    #[test]
    fn create_in_existing_file_is_exact_and_refusals_change_nothing() {
        let dir = tmp("visual1-exist");
        let src = "\u{FEFF}#VRML V2.0 utf8\r\n# Tëst 😀\rDEF Box_1 Transform { translation 1 2 3 }\r\nDEF Box_1b Group {}\r\n";
        let path = dir.join("e.wrl");
        fs::write(&path, files::encode(src, Format::Gzip).unwrap()).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        // Stale base revision: refused, nothing changed.
        assert!(matches!(
            create(&svc, doc.session, 7, p::Primitive::Box),
            p::CreateOutcome::Refused { .. }
        ));
        assert_eq!(
            (
                svc.text(doc.session).unwrap().as_str(),
                svc.snapshot(doc.session).unwrap().revision
            ),
            (src, 0)
        );
        for (i, prim) in p::Primitive::ALL.into_iter().enumerate() {
            let before = svc.text(doc.session).unwrap();
            let (st, item, _) = created(create(&svc, doc.session, i as u64, prim));
            assert_eq!(st.revision, i as u64 + 1);
            let after = svc.text(doc.session).unwrap();
            assert!(after.starts_with(&before), "prefix changed");
            let added = &after[before.len()..];
            assert!(added.starts_with("\r\nDEF "));
            assert!(!added.replace("\r\n", "").contains(['\r', '\n']));
            let a = svc.analyze(doc.session).unwrap();
            assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
            assert!(a.items.iter().any(|it| it.id == item));
        }
        assert!(svc
            .text(doc.session)
            .unwrap()
            .contains("DEF Box_2 Transform"));
        // Four creates, four undo steps, back to the exact original.
        for _ in 0..4 {
            svc.undo(doc.session).unwrap();
        }
        assert_eq!(svc.text(doc.session).unwrap(), src);
        assert!(!svc.snapshot(doc.session).unwrap().dirty);
        // A document whose end is not provably top level is refused.
        let r = svc.snapshot(doc.session).unwrap().revision;
        let view_end = svc
            .snapshot(doc.session)
            .unwrap()
            .view
            .encode_utf16()
            .count() as u64;
        edit(
            &svc,
            doc.session,
            r,
            view_end,
            view_end,
            "Group { children [",
        );
        let broken = svc.text(doc.session).unwrap();
        let out = create(&svc, doc.session, r + 1, p::Primitive::Sphere);
        assert!(
            matches!(&out, p::CreateOutcome::Refused { reason, .. } if reason == create::reason::SYNTAX_ERROR),
            "{out:?}"
        );
        assert_eq!(svc.text(doc.session).unwrap(), broken);
        assert_eq!(svc.snapshot(doc.session).unwrap().revision, r + 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A snapshot in the X_ITE adapter's shape for a simple object: Shape
    /// -> Transform -> scene, provenance = the items' SOURCE spans minus
    /// the preview offset (a stripped BOM).
    fn pick_req(
        svc: &Service,
        id: u64,
        transform: &str,
        shape: &str,
        shift: u64,
    ) -> p::PickRequest {
        let src = svc.preview_source(id).unwrap();
        let span = |item: &str| {
            let (a, b) = node_span(item).unwrap();
            p::PickSpan {
                start: a - shift,
                end: b - shift,
            }
        };
        let g = |ty: &str, occ: p::PickSpan, parent: &str| p::PickGraphNode {
            type_name: Some(ty.into()),
            ctx_kind: "document".into(),
            occurrences: vec![occ],
            parents: vec![parent.into()],
        };
        p::PickRequest {
            session: id,
            revision: src.revision,
            preview_hash: src.hash,
            generation: 7,
            snapshot: p::PickSnapshot {
                outcome: "hit".into(),
                reason: None,
                shape: Some("n1".into()),
                ctx_kind: Some("document".into()),
                sensors: vec![],
                graph: [
                    ("n1".to_string(), g("Shape", span(shape), "n2")),
                    ("n2".to_string(), g("Transform", span(transform), "SCENE")),
                ]
                .into_iter()
                .collect(),
            },
        }
    }

    /// (doc snapshot, exact text): what a pick must never change.
    fn untouched(svc: &Service, id: u64) -> (p::DocumentInfo, String) {
        (svc.snapshot(id).unwrap(), svc.text(id).unwrap())
    }

    #[test]
    fn pick_is_source_proven_read_only_and_bound_to_the_rendered_text() {
        let dir = tmp("pick");
        let body = "# é 😀 comment\nTransform { children [ Shape { geometry Box { } } ] }\nTransform { children [ Shape { geometry Box { } } ] }\n";
        for (name, bom, nl) in [
            ("lf.wrl", "", "\n"),
            ("crlf.wrl", "", "\r\n"),
            ("cr.wrl", "", "\r"),
            ("bom.wrl", "\u{FEFF}", "\r\n"),
        ] {
            let text = format!("{bom}#VRML V2.0 utf8\n{body}").replace('\n', nl);
            let path = dir.join(name);
            fs::write(&path, &text).unwrap();
            let svc = &Service::new(clock);
            let id = opened(svc.open_path(&path)).session;
            let a = svc.analyze(id).unwrap();
            let ts: Vec<_> = a.items.iter().filter(|i| i.label == "Transform").collect();
            let ss: Vec<_> = a.items.iter().filter(|i| i.label == "Shape").collect();
            assert_eq!((ts.len(), ss.len()), (2, 2), "{name}");
            let shift = bom.encode_utf16().count() as u64;
            let before = untouched(svc, id);
            // The SECOND of two byte-identical objects: exactly that one.
            let out = svc
                .pick(&pick_req(svc, id, &ts[1].id, &ss[1].id, shift))
                .unwrap();
            assert!(out.is_proven(), "{name}: {out:?}");
            assert_eq!(out.item.as_deref(), Some(ts[1].id.as_str()), "{name}");
            assert_eq!(out.role.as_deref(), Some("simple-object"));
            let l = out.logical.as_ref().unwrap();
            assert_eq!(
                (l.view_from, l.view_to),
                (ts[1].view_from, ts[1].view_to),
                "{name}"
            );
            assert_eq!(out.generation, 7);
            assert_eq!(
                untouched(svc, id),
                before,
                "{name}: a pick changed the document"
            );
            // Provenance NOT shifted for the stripped BOM never joins.
            if shift > 0 {
                let out = svc
                    .pick(&pick_req(svc, id, &ts[1].id, &ss[1].id, 0))
                    .unwrap();
                assert_eq!(out.status, "UNSUPPORTED", "{out:?}");
            }
        }
    }

    #[test]
    fn stale_or_refused_picks_select_nothing_and_change_nothing() {
        let svc = Service::new(clock);
        let doc = opened(svc.new_world());
        let id = doc.session;
        let (_, box_t, _) = created(create(&svc, id, 0, p::Primitive::Box));
        let (_, sph_t, _) = created(create(&svc, id, 1, p::Primitive::Sphere));
        let a = svc.analyze(id).unwrap();
        let shape_in = |t: &str| {
            let (f, e) = node_span(t).unwrap();
            a.items
                .iter()
                .find(|i| {
                    i.label == "Shape" && node_span(&i.id).is_some_and(|(a, b)| a > f && b < e)
                })
                .unwrap()
                .id
                .clone()
        };
        let (box_s, sph_s) = (shape_in(&box_t), shape_in(&sph_t));
        let before = untouched(&svc, id);
        let b = svc.pick(&pick_req(&svc, id, &box_t, &box_s, 0)).unwrap();
        let s = svc.pick(&pick_req(&svc, id, &sph_t, &sph_s, 0)).unwrap();
        assert_eq!(b.item.as_deref(), Some(box_t.as_str()));
        assert_eq!(s.item.as_deref(), Some(sph_t.as_str()));
        assert_eq!(untouched(&svc, id), before);

        // A request of another revision, or of other text, is stale.
        let mut r = pick_req(&svc, id, &box_t, &box_s, 0);
        r.revision -= 1;
        let out = svc.pick(&r).unwrap();
        assert_eq!(
            (out.status.as_str(), out.item.is_none()),
            ("REFUSED_STALE", true)
        );
        let mut r = pick_req(&svc, id, &box_t, &box_s, 0);
        r.preview_hash ^= 1;
        assert_eq!(svc.pick(&r).unwrap().status, "REFUSED_STALE");
        // A pick taken before an edit lands after it: stale, never re-mapped.
        let late = pick_req(&svc, id, &box_t, &box_s, 0);
        let st = edit(&svc, id, 2, 0, 0, "#c\n");
        assert_eq!(st.revision, 3);
        let out = svc.pick(&late).unwrap();
        assert_eq!(out.status, "REFUSED_STALE", "{out:?}");
        assert!(!out.message.is_empty());
        // Adapter outcomes map one to one; none selects anything.
        let cur = untouched(&svc, id);
        for (outcome, status) in [
            ("disabled", "COMPATIBILITY_DISABLED"),
            ("stale", "REFUSED_STALE"),
            ("external", "REFUSED_EXTERNAL"),
            ("no-hit", "NO_HIT"),
            ("unsupported", "UNSUPPORTED"),
            ("bogus", "UNSUPPORTED"),
        ] {
            let mut r = pick_req(&svc, id, &box_t, &box_s, 0);
            r.snapshot = p::PickSnapshot {
                outcome: outcome.into(),
                ..Default::default()
            };
            let out = svc.pick(&r).unwrap();
            assert_eq!(
                (out.status.as_str(), out.item.as_deref()),
                (status, None),
                "{outcome}"
            );
        }
        // A hit with no graph, or an oversized one, proves nothing.
        let mut r = pick_req(&svc, id, &box_t, &box_s, 0);
        r.snapshot.graph.clear();
        assert_eq!(svc.pick(&r).unwrap().status, "UNSUPPORTED");
        assert_eq!(untouched(&svc, id), cur);
        assert!(svc
            .pick(&p::PickRequest {
                session: 999,
                ..late
            })
            .is_err());
    }

    #[test]
    fn node_span_accepts_only_node_items() {
        assert_eq!(node_span("node-12-40"), Some((12, 40)));
        for bad in [
            "use-1-2",
            "node-5-5",
            "node-9-3",
            "node-1",
            "node--1-2",
            "node-a-b",
            "x",
        ] {
            assert_eq!(node_span(bad), None, "{bad}");
        }
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

    // ---- VISUAL-3A: translation gizmo ---------------------------------------

    fn target(svc: &Service, id: u64, rev: u64, item: &str) -> p::TranslateTargetOutcome {
        svc.translate_target(&p::TranslateTargetRequest {
            session: id,
            revision: rev,
            item: item.into(),
        })
        .unwrap()
    }

    fn mv(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        axis: p::gizmo::Axis,
        v: f64,
    ) -> p::TranslateOutcome {
        svc.translate(&p::TranslateRequest {
            session: id,
            base_revision: rev,
            item: item.into(),
            axis,
            value: v,
            decimals: 3,
        })
        .unwrap()
    }

    fn moved(o: p::TranslateOutcome) -> (p::DocState, String, String) {
        match o {
            p::TranslateOutcome::Applied {
                state, item, text, ..
            } => (state, item, text),
            other => panic!("not moved: {other:?}"),
        }
    }

    /// A plain-data `snapshotSpan` result: one located runtime node.
    fn located(ty: &str, span: (u64, u64), parents: &[&str]) -> p::PickSnapshot {
        let mut graph = std::collections::BTreeMap::new();
        graph.insert(
            "n1".to_string(),
            p::PickGraphNode {
                type_name: Some(ty.into()),
                ctx_kind: "document".into(),
                occurrences: vec![p::PickSpan {
                    start: span.0,
                    end: span.1,
                }],
                parents: parents.iter().map(|s| s.to_string()).collect(),
            },
        );
        p::PickSnapshot {
            outcome: "found".into(),
            shape: Some("n1".into()),
            ctx_kind: Some("document".into()),
            graph,
            ..Default::default()
        }
    }

    /// VISUAL-3A1: twin anonymous Transforms with EQUAL translations and
    /// geometry. Each binds only to its own runtime node; the other's node,
    /// a stale generation, a non-`found` snapshot or a changed text never
    /// binds. A Viewpoint span carries through a move, undo and redo.
    #[test]
    fn gizmo_binding_is_proven_by_provenance_and_viewpoints_carry_exactly() {
        let dir = tmp("visual3a1");
        let path = dir.join("twins.wrl");
        let twin = "Transform { translation 1 2 3 children [ Shape { geometry Box { } } ] }";
        let vp = "Viewpoint { position 0 0 9 description \"v ✓\" }";
        let text = format!("\u{FEFF}#VRML V2.0 utf8\r\n# é\r\n{twin}\r\n{twin}\r\n{vp}\r\n");
        fs::write(&path, &text).unwrap();
        let (o, svc) = svc_open(&path);
        let doc = opened(o);
        let id = doc.session;
        let rev0 = doc.revision;
        let u16at = |b: usize| text[..b].encode_utf16().count() as u64;
        let t1 = text.find(twin).unwrap();
        let t2 = text.rfind(twin).unwrap();
        let span = |b: usize, len: usize| (u16at(b), u16at(b + len));
        let (s1, s2) = (span(t1, twin.len()), span(t2, twin.len()));
        let preview = text.strip_prefix('\u{FEFF}').unwrap();
        let hash = p::preview_hash(preview);
        let prove = |rev: u64, hash: u64, (f, t): (u64, u64), snap: p::PickSnapshot| {
            svc.translate_prove(&p::TranslateProveRequest {
                session: id,
                revision: rev,
                preview_hash: hash,
                generation: 1,
                item: format!("node-{f}-{t}"),
                snapshot: snap,
            })
            .unwrap()
        };
        let mut wrong = 0;
        for (me, other) in [(s1, s2), (s2, s1)] {
            let item = format!("node-{}-{}", me.0, me.1);
            let (pf, pt) = match target(&svc, id, rev0, &item) {
                p::TranslateTargetOutcome::Ready {
                    def_name,
                    preview_from,
                    preview_to,
                    translation,
                    ..
                } => {
                    assert_eq!(def_name, None);
                    assert_eq!(translation, [1.0, 2.0, 3.0]);
                    (preview_from, preview_to)
                }
                o => panic!("{o:?}"),
            };
            assert_eq!((pf, pt), (me.0 - 1, me.1 - 1), "BOM projection");
            let own = located("Transform", (pf, pt), &["SCENE"]);
            assert!(matches!(
                prove(rev0, hash, me, own.clone()),
                p::TranslateProveOutcome::Proven { .. }
            ));
            // The OTHER twin's runtime node, offered for this item.
            let theirs = located("Transform", (other.0 - 1, other.1 - 1), &["SCENE"]);
            match prove(rev0, hash, me, theirs) {
                p::TranslateProveOutcome::Proven { .. } => wrong += 1,
                p::TranslateProveOutcome::Refused { reason, .. } => {
                    assert_eq!(reason, wrlforge_vrml::pick::reason::BIND_SPAN)
                }
                o => panic!("{o:?}"),
            }
            // Stale text or generation, or no unique runtime node.
            assert!(matches!(
                prove(rev0, hash ^ 1, me, own.clone()),
                p::TranslateProveOutcome::Stale { .. }
            ));
            let mut none = own.clone();
            none.outcome = "unsupported".into();
            none.reason = Some("runtime-node-for-span-not-unique".into());
            assert!(matches!(
                prove(rev0, hash, me, none),
                p::TranslateProveOutcome::Refused { reason, .. } if reason == "runtime-node-for-span-not-unique"
            ));
            // A runtime node that is NOT directly under the scene.
            let mut nested = located("Transform", (pf, pt), &["n2"]);
            nested.graph.insert(
                "n2".into(),
                p::PickGraphNode {
                    type_name: Some("Group".into()),
                    ctx_kind: "document".into(),
                    occurrences: vec![],
                    parents: vec!["SCENE".into()],
                },
            );
            assert!(matches!(
                prove(rev0, hash, me, nested),
                p::TranslateProveOutcome::Refused { .. }
            ));
        }
        assert_eq!(wrong, 0, "WRONG_RUNTIME_MANIPULATIONS");

        // Camera carry: the Viewpoint's preview span through a move of the
        // FIRST twin (which grows "1" -> "1.5"), then undo and redo.
        let v = text.find(vp).unwrap();
        let pv = (u16at(v) - 1, u16at(v + vp.len()) - 1);
        let carry = |from_rev: u64, to_rev: u64, (f, t): (u64, u64)| {
            svc.preview_carry(&p::PreviewCarryRequest {
                session: id,
                from_revision: from_rev,
                to_revision: to_rev,
                from: f,
                to: t,
            })
            .unwrap()
        };
        let preview_slice = |(f, t): (u64, u64)| {
            let now = svc.text(id).unwrap();
            let u: Vec<u16> = now
                .strip_prefix('\u{FEFF}')
                .unwrap()
                .encode_utf16()
                .collect();
            String::from_utf16(&u[f as usize..t as usize]).unwrap()
        };
        assert_eq!(
            carry(rev0, rev0, pv),
            p::PreviewCarryOutcome::Mapped {
                from: pv.0,
                to: pv.1
            }
        );
        let (st, _, _) = moved(mv(
            &svc,
            id,
            rev0,
            &format!("node-{}-{}", s1.0, s1.1),
            p::gizmo::Axis::X,
            1.5,
        ));
        let rev1 = st.revision;
        let m1 = match carry(rev0, rev1, pv) {
            p::PreviewCarryOutcome::Mapped { from, to } => (from, to),
            o => panic!("{o:?}"),
        };
        assert_eq!(m1, (pv.0 + 2, pv.1 + 2));
        assert_eq!(preview_slice(m1), vp);
        // The moved Transform's own span changed at its inside: carried too;
        // a span the change crosses is lost, never guessed.
        assert!(matches!(
            carry(rev0, rev1, (s1.0 - 1, s1.1 - 1)),
            p::PreviewCarryOutcome::Mapped { .. }
        ));
        assert!(matches!(
            carry(rev0, rev1, (s1.0 - 1 + 24, s1.0 - 1 + 30)),
            p::PreviewCarryOutcome::Lost { .. }
        ));
        // Only to the CURRENT revision, only from a logged one.
        assert!(matches!(
            carry(rev0, rev0, pv),
            p::PreviewCarryOutcome::Lost { .. }
        ));
        assert!(matches!(
            carry(rev1 + 5, rev1, pv),
            p::PreviewCarryOutcome::Lost { .. }
        ));
        svc.undo(id).unwrap();
        let rev2 = rev1 + 1;
        assert_eq!(
            carry(rev1, rev2, m1),
            p::PreviewCarryOutcome::Mapped {
                from: pv.0,
                to: pv.1
            }
        );
        assert_eq!(preview_slice(pv), vp);
        svc.redo(id).unwrap();
        assert_eq!(
            carry(rev0, rev2 + 1, pv),
            p::PreviewCarryOutcome::Mapped {
                from: m1.0,
                to: m1.1
            }
        );
        fs::remove_dir_all(&dir).ok();
    }

    fn svc_open(path: &Path) -> (p::OpenOutcome, Service) {
        let svc = Service::new(clock);
        (svc.open_path(path), svc)
    }

    /// New World → Box → X, Y, Z drags (positive and negative) → each is ONE
    /// token and ONE undo step → exact Undo / Redo → stale / wrong-node /
    /// zero-distance / invalid refusals change nothing → Save As → reopen.
    #[test]
    fn gizmo_moves_are_exact_single_undo_steps_and_survive_save_and_reopen() {
        use p::gizmo::Axis;
        let dir = tmp("visual3a");
        let svc = Service::new(clock);
        let doc = opened(svc.new_world());
        let id = doc.session;
        let (st, item, _) = created(create(&svc, id, doc.revision, p::Primitive::Box));
        let rev0 = st.revision;
        let base = svc.text(id).unwrap();
        match target(&svc, id, rev0, &item) {
            p::TranslateTargetOutcome::Ready {
                def_name,
                translation,
                origin,
                ..
            } => {
                assert_eq!(def_name.as_deref(), Some("Box_1"));
                assert_eq!(translation, [0.0; 3]);
                assert_eq!(origin, [0.0; 3]);
            }
            other => panic!("{other:?}"),
        }
        // A stale target query examines nothing.
        assert_eq!(
            target(&svc, id, rev0 - 1, &item),
            p::TranslateTargetOutcome::Stale { current: rev0 }
        );

        let mut texts = vec![base.clone()];
        let (mut rev, mut it) = (rev0, item.clone());
        for (axis, v, tok) in [
            (Axis::X, 2.5, "2.5"),
            (Axis::Y, -1.25, "-1.25"),
            (Axis::Z, 0.75, "0.75"),
            (Axis::X, -3.0, "-3"),
        ] {
            let before = svc.text(id).unwrap();
            let (st, item2, text) = moved(mv(&svc, id, rev, &it, axis, v));
            assert_eq!(text, tok);
            assert_eq!(st.revision, rev + 1, "one revision per drag");
            let after = svc.text(id).unwrap();
            // Exactly one token differs.
            let pre = before
                .bytes()
                .zip(after.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let suf = before
                .bytes()
                .rev()
                .zip(after.bytes().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let changed_old = &before[pre..before.len() - suf.min(before.len() - pre)];
            assert!(
                !changed_old.contains(char::is_whitespace),
                "{changed_old:?}"
            );
            assert!(after.contains("translation "), "{after}");
            texts.push(after);
            rev = st.revision;
            it = item2;
        }
        let last = texts.last().unwrap().clone();
        assert!(last.contains("translation -3 -1.25 0.75"), "{last}");

        // Refusals change nothing.
        let snap = |svc: &Service| (svc.text(id).unwrap(), svc.snapshot(id).unwrap().revision);
        let s0 = snap(&svc);
        assert!(matches!(
            mv(&svc, id, rev - 1, &it, Axis::X, 9.0),
            p::TranslateOutcome::Refused { ref reason, .. } if reason == "parse-session-is-stale"
        ));
        assert!(matches!(
            mv(&svc, id, rev, &it, Axis::X, -3.0),
            p::TranslateOutcome::Unchanged
        ));
        assert!(matches!(
            mv(&svc, id, rev, &it, Axis::X, -3.0001),
            p::TranslateOutcome::Unchanged
        ));
        assert!(matches!(
            mv(&svc, id, rev, &it, Axis::Y, f64::NAN),
            p::TranslateOutcome::Refused { .. }
        ));
        assert!(matches!(
            mv(&svc, id, rev, "tree-route-0-1", Axis::Y, 1.0),
            p::TranslateOutcome::Refused { .. }
        ));
        // Wrong node: the Box's Shape span (a real node, not a Transform).
        let a = svc.analyze(id).unwrap();
        let shape = a
            .items
            .iter()
            .find(|i| i.label.starts_with("Shape"))
            .unwrap()
            .id
            .clone();
        assert!(matches!(
            mv(&svc, id, rev, &shape, Axis::Y, 1.0),
            p::TranslateOutcome::Refused { ref reason, .. } if reason == "node-is-not-a-transform"
        ));
        assert!(matches!(
            target(&svc, id, rev, &shape),
            p::TranslateTargetOutcome::Refused { .. }
        ));
        assert_eq!(
            snap(&svc),
            s0,
            "refusals left source and revision unchanged"
        );

        // Exact undo / redo, one drag per step.
        for want in texts.iter().rev().skip(1) {
            assert!(matches!(
                svc.undo(id).unwrap(),
                p::HistoryOutcome::Applied { .. }
            ));
            assert_eq!(&svc.text(id).unwrap(), want);
        }
        for want in texts.iter().skip(1) {
            assert!(matches!(
                svc.redo(id).unwrap(),
                p::HistoryOutcome::Applied { .. }
            ));
            assert_eq!(&svc.text(id).unwrap(), want);
        }

        // Save As, reopen: the moved translation persists, byte for byte.
        let path = dir.join("moved.wrl");
        assert!(matches!(
            svc.save_as(id, &path),
            p::SaveOutcome::Saved { .. }
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), last);
        let svc2 = Service::new(clock);
        let again = opened(svc2.open_path(&path));
        assert_eq!(svc2.text(again.session).unwrap(), last);
        let a2 = svc2.analyze(again.session).unwrap();
        let tf = a2
            .items
            .iter()
            .find(|i| i.label == "Transform Box_1")
            .unwrap();
        match target(&svc2, again.session, again.revision, &tf.id) {
            p::TranslateTargetOutcome::Ready { translation, .. } => {
                assert_eq!(translation, [-3.0, -1.25, 0.75])
            }
            other => panic!("{other:?}"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// A moved file keeps its BOM, CRLF / lone CR line endings, comments and
    /// Unicode byte for byte; only the translation token differs on disk.
    #[test]
    fn gizmo_move_on_disk_changes_one_token_bom_crlf_cr_unicode() {
        use p::gizmo::Axis;
        for (tag, eol, bom) in [
            ("lf", "\n", ""),
            ("crlf", "\r\n", "\u{feff}"),
            ("cr", "\r", ""),
        ] {
            let dir = tmp(&format!("visual3a-{tag}"));
            let body = create::template(create::Primitive::Box, "Box_ü", eol);
            let src = format!("{bom}#VRML V2.0 utf8{eol}# 世界 ✓ 😀{eol}{body}{eol}# fin{eol}");
            let path = dir.join("w.wrl");
            fs::write(&path, &src).unwrap();
            let svc = Service::new(clock);
            let doc = opened(svc.open_path(&path));
            let a = svc.analyze(doc.session).unwrap();
            let tf = a
                .items
                .iter()
                .find(|i| i.label == "Transform Box_ü")
                .unwrap()
                .id
                .clone();
            let (_, _, text) = moved(mv(&svc, doc.session, doc.revision, &tf, Axis::Y, 1.5));
            assert_eq!(text, "1.5");
            assert!(matches!(
                svc.save(doc.session),
                p::SaveOutcome::Saved { .. }
            ));
            let disk = fs::read(&path).unwrap();
            let want = src.replacen("translation 0 0 0", "translation 0 1.5 0", 1);
            assert_eq!(disk, want.as_bytes(), "{tag}");
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
