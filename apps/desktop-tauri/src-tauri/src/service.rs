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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use wrlforge_desktop_protocol as p;
use wrlforge_document::{map_span, view_of, Applied, Document, SpanChange, ViewEdit};
use wrlforge_text::Edit;
use wrlforge_vrml::{
    create, extrusion, field_edit, highlight, manipulate, mf_edit, scene, string_edit,
    structure_edit,
};

use crate::files::{self, FileError, Format, SaveOptions, Stamp};
use crate::recovery::{self, Recovery};
use crate::resources;

pub struct Session {
    /// `None` for a new world that has no file yet (VISUAL-1). A path is
    /// assigned only by a verified Save As.
    path: Option<PathBuf>,
    format: Format,
    /// The disk state at open / last save; `None` while untitled.
    stamp: Option<Stamp>,
    doc: Document,
    /// TEXTURE-LOCAL-1: the `wrlres` token of the newest preview generation.
    /// Only this token is served; a newer generation, a close or a reopen
    /// (a new session) retires it.
    resource_token: Option<String>,
    /// RUST-STAB-1: this session's recovery record id. The record exists on
    /// disk only while the document is dirty (and a write has completed).
    recovery_id: String,
    /// The recovery change sequence of the current state.
    recovery_seq: u64,
    /// RUST-STAB-1: the safe file name of a restored record (untitled).
    recovered_name: Option<String>,
}

impl Session {
    fn new(path: Option<PathBuf>, format: Format, stamp: Option<Stamp>, doc: Document) -> Self {
        Session {
            path,
            format,
            stamp,
            doc,
            resource_token: None,
            recovery_id: recovery::new_id(),
            recovery_seq: 0,
            recovered_name: None,
        }
    }
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
    /// RUST-STAB-1: crash recovery; absent until the app attaches it.
    recovery: OnceLock<Arc<Recovery>>,
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
/// The protocol's gizmo tool / axis as the planner's.
fn manip_tool(t: p::gizmo::Tool) -> manipulate::Tool {
    match t {
        p::gizmo::Tool::Move => manipulate::Tool::Move,
        p::gizmo::Tool::Rotate => manipulate::Tool::Rotate,
        p::gizmo::Tool::Scale => manipulate::Tool::Scale,
    }
}

fn manip_axis(a: p::gizmo::Axis) -> manipulate::Axis {
    match a {
        p::gizmo::Axis::X => manipulate::Axis::X,
        p::gizmo::Axis::Y => manipulate::Axis::Y,
        p::gizmo::Axis::Z => manipulate::Axis::Z,
    }
}

fn preview_offset(text: &str) -> u64 {
    u64::from(text.starts_with('\u{FEFF}'))
}

/// B3 (LV2): the PROTO instance `instance` (a node item of `old`) in `new`,
/// after `changes` (the exact changes that turned `old` into `new`) edited
/// the shared body node `body` (a span of `old`). Proven, never searched:
/// the instance must be a document-scene instance of the body's PROTO in
/// `old`, its span must map through the changes (`map_span`), its bytes
/// must be unchanged, and `new` must hold an instance of the same PROTO at
/// exactly the mapped span. `Err` is the reason.
fn carry_instance(
    old: &str,
    new: &str,
    body: (u64, u64),
    instance: &str,
    changes: &[SpanChange],
) -> Result<p::InstanceRef, &'static str> {
    let (f, t) = node_span(instance).ok_or("instance-not-a-node")?;
    let po = wrlforge_vrml::parse(old);
    let proto = field_edit::shared_definition(&po, body.0, body.1)
        .ok_or("edited-node-not-in-a-proto-body")?
        .proto;
    let is_instance = |p: &wrlforge_vrml::ParseResult, src: &str, f: u64, t: u64| {
        let n = field_edit::inspect_node_fields(p, src, f, t);
        n.shared.is_none() && n.node_type.as_deref() == Some(proto.as_str())
    };
    if !is_instance(&po, old, f, t) {
        return Err("instance-not-proven-in-old-revision");
    }
    let (nf, nt) = map_span(f, t, changes).ok_or("change-touches-instance")?;
    let slice = |s: &str, a: u64, b: u64| -> Option<String> {
        let a = wrlforge_text::offsets::utf16_to_byte(s, a as usize).ok()?;
        let b = wrlforge_text::offsets::utf16_to_byte(s, b as usize).ok()?;
        s.get(a..b).map(str::to_string)
    };
    match (slice(old, f, t), slice(new, nf, nt)) {
        (Some(a), Some(b)) if a == b => {}
        _ => return Err("instance-bytes-changed"),
    }
    let pn = wrlforge_vrml::parse(new);
    if !is_instance(&pn, new, nf, nt) {
        return Err("instance-not-proven-in-new-revision");
    }
    let off = preview_offset(new);
    if nf < off {
        return Err("instance-before-preview-start");
    }
    Ok(p::InstanceRef {
        item: format!("node-{nf}-{nt}"),
        preview_from: nf - off,
        preview_to: nt - off,
    })
}

pub fn node_span(item: &str) -> Option<(u64, u64)> {
    let rest = item.strip_prefix("node-")?;
    let (a, b) = rest.split_once('-')?;
    let from = a.parse().ok()?;
    let to = b.parse().ok()?;
    (from < to).then_some((from, to))
}

fn extrusion_info(i: extrusion::Info) -> p::ExtrusionInfo {
    p::ExtrusionInfo {
        fields: i
            .fields
            .into_iter()
            .map(|f| {
                let (tuples, single) = match f.effective {
                    Some(extrusion::Effective::List(l)) => (Some(l), None),
                    Some(extrusion::Effective::Single(s)) => (None, Some(s)),
                    None => (None, None),
                };
                p::ExtrusionField {
                    name: f.name.into(),
                    field_type: f.field_type.into(),
                    authored: f.authored,
                    field_index: f.index.map(|i| i as u32),
                    tuples,
                    single,
                    default_text: f.default_text.map(Into::into),
                    problem: f.problem.map(Into::into),
                }
            })
            .collect(),
        notes: i
            .notes
            .into_iter()
            .map(|n| p::ExtrusionNote {
                level: n.level.as_str().into(),
                field: n.field.into(),
                text: n.text,
            })
            .collect(),
        outline_closed: i.outline_closed,
    }
}

fn node_fields(
    n: field_edit::NodeFields,
    vm: &ViewMap,
    extrusion: Option<extrusion::Info>,
) -> p::NodeFields {
    p::NodeFields {
        shared: n.shared.as_ref().map(|s| p::SharedProto {
            proto: s.proto.clone(),
            instances: s.instances as u32,
            name_ambiguous: s.name_ambiguous,
        }),
        extrusion: extrusion.map(extrusion_info),
        editable: n.editable,
        reason: n.reason.into(),
        node_type: n.node_type,
        fields: n
            .fields
            .into_iter()
            .map(|f| p::EditableField {
                // The value span of an SFNode field is its node statement;
                // the UI only selects it if the Scene Tree holds that id.
                node_item: (f.field_type == Some("SFNode") && !f.is_binding)
                    .then(|| format!("node-{}-{}", f.from, f.to)),
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
                list: f.list.map(|l| p::ListField {
                    labels: l.spec.labels.iter().map(|s| s.to_string()).collect(),
                    rotation: l.spec.rotation,
                    form: match l.form {
                        mf_edit::Form::Bracketed { .. } => "bracketed".into(),
                        mf_edit::Form::Bare => "bare".into(),
                    },
                    tuples: l.lexemes(),
                }),
                strings: f.strings.map(|l| p::StringListField {
                    form: match l.form {
                        string_edit::Form::Bracketed { .. } => "bracketed".into(),
                        string_edit::Form::Bare => "bare".into(),
                    },
                    values: l.values(),
                }),
            })
            .collect(),
    }
}

/// The MFString plan in the shape `apply_list_plan` applies (no lexemes).
fn string_plan(plan: string_edit::Plan) -> mf_edit::Plan {
    match plan {
        string_edit::Plan::Unchanged => mf_edit::Plan::Unchanged,
        string_edit::Plan::Refused {
            reason,
            message,
            op_index,
        } => mf_edit::Plan::Refused {
            reason,
            message,
            op_index,
            component_index: None,
        },
        string_edit::Plan::Ready {
            edit,
            new_text,
            node_from,
            node_to,
            ..
        } => mf_edit::Plan::Ready {
            edit,
            new_text,
            node_from,
            node_to,
            lexemes: Vec::new(),
        },
    }
}

/// B2: the url to write for an image the user chose in the document's
/// folder: relative, `/`-separated, with `%`, `#` and `?` percent-encoded
/// (the preview decodes them; the WebView encodes the rest). Proven by
/// resolving the url exactly as the preview does: it must name the chosen
/// file (same real path) and carry an image signature. `Err` is the message
/// for the user. A file outside the folder tree is refused, never copied.
pub fn texture_url(folder: &Path, chosen: &Path) -> Result<String, String> {
    let outside = || {
        "The image is outside the document's folder. Put it in the folder (or a subfolder) of the .wrl and choose it again; files are not copied.".to_string()
    };
    let root = folder
        .canonicalize()
        .map_err(|e| format!("The document's folder cannot be read: {e}."))?;
    let real = chosen
        .canonicalize()
        .map_err(|e| format!("The image cannot be read: {e}."))?;
    let rel = real.strip_prefix(&root).map_err(|_| outside())?;
    let mut segs = Vec::new();
    for c in rel.components() {
        let std::path::Component::Normal(c) = c else {
            return Err(outside());
        };
        let c = c
            .to_str()
            .ok_or_else(|| "The image's name is not valid UTF-8.".to_string())?;
        let mut seg = String::with_capacity(c.len());
        for ch in c.chars() {
            match ch {
                '%' => seg.push_str("%25"),
                '#' => seg.push_str("%23"),
                '?' => seg.push_str("%3F"),
                '\\' | '"' | '\n' | '\r' => {
                    return Err("The image's name holds a character a texture url cannot carry; rename it.".into())
                }
                _ => seg.push(ch),
            }
        }
        segs.push(seg);
    }
    if segs.is_empty() {
        return Err(outside());
    }
    let url = segs.join("/");
    let decoded = resources::decode_segments(&url)
        .map_err(|e| format!("The image's name cannot be used as a url ({}).", e.as_str()))?;
    let resolved = resources::resolve(&root, &decoded)
        .map_err(|e| format!("The image cannot be served to the preview ({}).", e.as_str()))?;
    if resolved != real {
        return Err(outside());
    }
    resources::check_url(&url, Some(&root))
        .map_err(|e| format!("The file is not a usable image ({}).", e.as_str()))?;
    Ok(url)
}

fn refused_field(
    reason: &str,
    message: Option<String>,
    component: Option<u32>,
) -> p::FieldEditOutcome {
    p::FieldEditOutcome::Refused {
        reason: reason.into(),
        message,
        component_index: component,
    }
}

fn stale_field(base: u64, current: u64) -> p::FieldEditOutcome {
    refused_field(
        "parse-session-is-stale",
        Some(format!(
            "The document changed (revision {base} → {current}); re-select the node."
        )),
        None,
    )
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
            recovery: OnceLock::new(),
        }
    }

    /// Attach crash recovery (once, at startup).
    pub fn attach_recovery(&self, rec: Arc<Recovery>) {
        let _ = self.recovery.set(rec);
    }

    pub fn recovery(&self) -> Option<&Arc<Recovery>> {
        self.recovery.get()
    }

    /// Every session access goes through here, so EVERY committed change
    /// (typing, Inspector, Create, Move / Rotate / Scale, Extrusion, list
    /// edits, Undo / Redo, Reload) reaches recovery: a new revision is a
    /// new recovery state. The canonical document is never touched by it.
    fn with<R>(&self, id: p::SessionId, f: impl FnOnce(&mut Session) -> R) -> Result<R, String> {
        let mut map = self.sessions.lock().map_err(err)?;
        let s = map
            .get_mut(&id)
            .ok_or_else(|| format!("unknown document session {id}"))?;
        let revision = s.doc.revision();
        let out = f(s);
        if s.doc.revision() != revision {
            self.recovery_note(id, s);
        }
        Ok(out)
    }

    /// The session reached a new state: a dirty one is scheduled for a
    /// recovery write; a clean one (saved, reloaded, undone to the saved
    /// text) has its record removed NOW, which also blocks any delayed write.
    fn recovery_note(&self, id: p::SessionId, s: &mut Session) {
        let Some(rec) = self.recovery.get() else {
            return;
        };
        s.recovery_seq = rec.next_seq();
        if s.doc.dirty() {
            rec.mark(id);
        } else {
            let _ = rec.retire(&s.recovery_id, s.recovery_seq);
        }
    }

    fn info(id: p::SessionId, s: &Session) -> p::DocumentInfo {
        let counts = s.doc.eol_counts();
        p::DocumentInfo {
            session: id,
            name: Self::name(s),
            display_path: match (&s.path, &s.recovered_name) {
                (Some(p), _) => p.display().to_string(),
                (None, Some(_)) => "Recovered work (not saved yet; use Save As)".into(),
                (None, None) => "New world (not saved yet)".into(),
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
            recovered: s.path.is_none() && s.recovered_name.is_some(),
        }
    }

    fn name(s: &Session) -> String {
        match (&s.path, &s.recovered_name) {
            (Some(p), _) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            (None, Some(n)) => n.clone(),
            (None, None) => UNTITLED_NAME.into(),
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
        self.insert(Session::new(
            Some(path.to_path_buf()),
            loaded.format,
            Some(loaded.stamp),
            Document::new(loaded.text),
        ))
    }

    /// A new, empty VRML97 world in a new session. It has NO path and no
    /// file until a Save As: nothing is written, no temporary file exists.
    /// It is in the canonical document engine like any opened file, and is
    /// not dirty until it is changed.
    pub fn new_world(&self) -> p::OpenOutcome {
        self.insert(Session::new(
            None,
            Format::Plain,
            None,
            Document::new(create::NEW_WORLD.into()),
        ))
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

    /// D1: the folder of the session's file, if it has one on disk.
    pub fn folder(&self, id: p::SessionId) -> Option<std::path::PathBuf> {
        self.with(id, |s| s.path.as_deref().and_then(Path::parent).map(Path::to_path_buf))
            .ok()
            .flatten()
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
        self.history(id, true, None, None)
    }
    pub fn redo(&self, id: p::SessionId) -> Result<p::HistoryOutcome, String> {
        self.history(id, false, None, None)
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
        self.history(id, undo, item, None)
    }
    /// LV2-C: [`Self::history_with_item`] that also carries the selected
    /// PROTO body node's rendered instance (`instance`: its preview span in
    /// the current revision) through the exact change, proven exactly like
    /// a gizmo commit's (`carry_instance`). Never a search.
    pub fn history_with_instance(
        &self,
        id: p::SessionId,
        undo: bool,
        item: Option<&str>,
        instance: Option<(u64, u64)>,
    ) -> Result<p::HistoryOutcome, String> {
        self.history(id, undo, item, instance)
    }
    fn history(
        &self,
        id: p::SessionId,
        undo: bool,
        item: Option<&str>,
        instance: Option<(u64, u64)>,
    ) -> Result<p::HistoryOutcome, String> {
        self.with(id, |s| {
            let old = instance.map(|_| s.doc.text().to_string());
            let r = if undo { s.doc.undo() } else { s.doc.redo() };
            match r {
                Ok(a) => {
                    let carried = Self::carry(item, s.doc.last_changes());
                    let (instance, instance_lost) =
                        match (instance, old, item.and_then(node_span)) {
                            (Some((f, t)), Some(old), Some(body)) => {
                                let off = preview_offset(&old);
                                match carry_instance(
                                    &old,
                                    s.doc.text(),
                                    body,
                                    &format!("node-{}-{}", f + off, t + off),
                                    s.doc.last_changes(),
                                ) {
                                    Ok(r) => (Some(r), None),
                                    Err(why) => (None, Some(why.to_string())),
                                }
                            }
                            (Some(_), _, _) => (None, Some("instance-without-a-node".into())),
                            _ => (None, None),
                        };
                    p::HistoryOutcome::Applied {
                        state: Self::state(s, a),
                        view: s.doc.view(),
                        item: carried,
                        instance,
                        instance_lost,
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
            let out = Self::after_save(id, s, result);
            if matches!(out, p::SaveOutcome::Saved { .. }) {
                self.recovery_note(id, s);
            }
            out
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
            let out = Self::after_save(id, s, result);
            if matches!(out, p::SaveOutcome::Saved { .. }) {
                self.recovery_note(id, s);
            }
            out
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
            // One parse: the tree, the scope graph and the diagnostics below
            // all describe exactly this revision's text.
            let (tree, graph) = scene::build_scene_tree_scoped(&parsed, text);
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
                        refs: it
                            .refs
                            .iter()
                            .map(|r| p::SceneRef {
                                role: r.role.into(),
                                name: r.name.clone(),
                                status: r.status.into(),
                                class: r.class.into(),
                                reason: r.reason.into(),
                                target: r.target.clone(),
                                language_target: r.language_target.clone(),
                            })
                            .collect(),
                        typed: it.typed.as_ref().map(|t| p::TypedCheck {
                            subject: t.subject.into(),
                            status: t.status.into(),
                            class: t.class.into(),
                            reason: t.reason.into(),
                            summary: t.summary.clone(),
                        }),
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
            let scope_diagnostics = graph
                .diagnostics()
                .into_iter()
                .map(|d| p::Diagnostic {
                    code: d.code.into(),
                    severity: d.severity.into(),
                    message: d.message,
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
                scope_diagnostics,
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
                    extrusion::inspect_at(&parsed, text, from, to),
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
                confirm_shared: r.confirm_shared,
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
                p::Primitive::Extrusion => create::Primitive::Extrusion,
            };
            match create::plan_create_at(s.doc.text(), prim, r.at) {
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

    /// B2: Delete / Duplicate the node at the item's span. Rust re-proves the
    /// revision, the node (exactly one at that span), its place, its DEF
    /// references and -- by re-parsing -- the whole result. Applied as ONE
    /// transaction (one undo step) whose result must equal the planned text.
    pub fn structure_edit(
        &self,
        r: &p::StructureEditRequest,
    ) -> Result<p::StructureEditOutcome, String> {
        self.with(r.session, |s| {
            let refused = |reason: &str, message: String| p::StructureEditOutcome::Refused {
                reason: reason.into(),
                message,
            };
            if r.base_revision != s.doc.revision() {
                return refused(
                    "parse-session-is-stale",
                    format!(
                        "The document changed (revision {} → {}); re-select the node.",
                        r.base_revision,
                        s.doc.revision()
                    ),
                );
            }
            let Some((from, to)) = node_span(&r.item) else {
                return refused(
                    field_edit::reason::NOT_A_NODE,
                    "The selection is not a node.".into(),
                );
            };
            let plan = match r.op {
                p::StructureOp::Delete => (
                    structure_edit::plan_delete(s.doc.text(), from, to, r.confirm_shared),
                    None,
                ),
                p::StructureOp::Duplicate => match r.offset {
                    Some(o) if o.valid() => structure_edit::plan_duplicate_offset(
                        s.doc.text(),
                        from,
                        to,
                        r.confirm_shared,
                        usize::from(o.axis),
                        o.step,
                    ),
                    Some(_) => {
                        return refused(
                            field_edit::reason::INPUT_SHAPE,
                            "The Duplicate offset is not a usable axis and distance.".into(),
                        )
                    }
                    // LV2-C: no proven size, no guessed offset.
                    None => {
                        let plan = structure_edit::plan_duplicate(
                            s.doc.text(),
                            from,
                            to,
                            r.confirm_shared,
                        );
                        let ready = matches!(plan, structure_edit::Plan::Ready { .. });
                        (
                            plan,
                            ready.then(|| structure_edit::Placement::InPlace {
                                why: p::duplicate::NO_SIZE.into(),
                            }),
                        )
                    }
                },
            };
            let (plan, placement) = plan;
            match plan {
                structure_edit::Plan::Refused { reason, message } => refused(reason, message),
                structure_edit::Plan::Ready {
                    edit,
                    new_text,
                    node,
                    renamed,
                } => {
                    let edit = Edit {
                        from: edit.from,
                        to: edit.to,
                        insert: edit.insert,
                    };
                    match s
                        .doc
                        .apply_source_transaction(r.base_revision, &[edit], &new_text)
                    {
                        Ok(a) => p::StructureEditOutcome::Applied {
                            state: Self::state(s, a),
                            view: s.doc.view(),
                            item: node.map(|(f, t)| format!("node-{f}-{t}")),
                            renamed,
                            placement: placement.map(|p| p.describe()),
                        },
                        Err(e) => {
                            refused(field_edit::reason::TRANSACTION_REJECTED, e.to_string())
                        }
                    }
                }
            }
        })
    }

    /// VISUAL-4A: a list-field edit (`MFVec2f` / `MFVec3f` / `MFRotation`).
    /// Rust re-proves the revision, the node at the item's span, the field and
    /// the tuple count; plans every operation on the text the previous one
    /// produced; and applies the ONE resulting span edit as ONE transaction
    /// (one undo step) whose result must equal the planned text.
    pub fn edit_list(&self, r: &p::ListEditRequest) -> Result<p::FieldEditOutcome, String> {
        self.with(r.session, |s| {
            if r.base_revision != s.doc.revision() {
                return stale_field(r.base_revision, s.doc.revision());
            }
            let Some((node_from, node_to)) = node_span(&r.item) else {
                return refused_field(field_edit::reason::NOT_A_NODE, None, None);
            };
            let req = mf_edit::Request {
                confirm_shared: r.confirm_shared,
                node_from,
                node_to,
                field_index: r.field_index as usize,
                field_name: r.field_name.clone(),
                expected_count: r.expected_count as usize,
                ops: r
                    .ops
                    .iter()
                    .map(|o| match o {
                        p::ListOp::Set { index, components } => mf_edit::Op::Set {
                            index: *index as usize,
                            components: components.clone(),
                        },
                        p::ListOp::Insert { index, components } => mf_edit::Op::Insert {
                            index: *index as usize,
                            components: components.clone(),
                        },
                        p::ListOp::Delete { index } => mf_edit::Op::Delete {
                            index: *index as usize,
                        },
                    })
                    .collect(),
            };
            let plan = mf_edit::plan_list_edit(s.doc.text(), &req);
            Self::apply_list_plan(s, r.base_revision, plan)
        })
    }

    /// VISUAL-4A: write one ABSENT field into the node because the user asked
    /// (never automatically). One transaction, one undo step.
    pub fn add_field(&self, r: &p::AddFieldRequest) -> Result<p::FieldEditOutcome, String> {
        self.with(r.session, |s| {
            if r.base_revision != s.doc.revision() {
                return stale_field(r.base_revision, s.doc.revision());
            }
            let Some((node_from, node_to)) = node_span(&r.item) else {
                return refused_field(field_edit::reason::NOT_A_NODE, None, None);
            };
            let req = mf_edit::AddRequest {
                confirm_shared: r.confirm_shared,
                node_from,
                node_to,
                field_name: r.field_name.clone(),
                value: match &r.value {
                    p::AddFieldValue::Bool { value } => mf_edit::AddValue::Bool(*value),
                    p::AddFieldValue::Number { value } => mf_edit::AddValue::Number(value.clone()),
                    p::AddFieldValue::List { tuples } => mf_edit::AddValue::List(tuples.clone()),
                    p::AddFieldValue::Tuple { values } => mf_edit::AddValue::Tuple(values.clone()),
                },
            };
            let plan = mf_edit::plan_add_field(s.doc.text(), &req);
            Self::apply_list_plan(s, r.base_revision, plan)
        })
    }

    /// B2: an MFString edit (`url`, `info`, ...). Rust re-proves the
    /// revision, the node, the field and the exact list shown, plans every
    /// operation and applies the ONE resulting span edit as ONE transaction.
    pub fn edit_strings(&self, r: &p::StringEditRequest) -> Result<p::FieldEditOutcome, String> {
        self.with(r.session, |s| {
            if r.base_revision != s.doc.revision() {
                return stale_field(r.base_revision, s.doc.revision());
            }
            let Some((node_from, node_to)) = node_span(&r.item) else {
                return refused_field(field_edit::reason::NOT_A_NODE, None, None);
            };
            let req = string_edit::Request {
                confirm_shared: r.confirm_shared,
                node_from,
                node_to,
                field_index: r.field_index as usize,
                field_name: r.field_name.clone(),
                expected: r.expected.clone(),
                ops: r
                    .ops
                    .iter()
                    .map(|o| match o {
                        p::StringOp::Set { index, value } => string_edit::Op::Set {
                            index: *index as usize,
                            value: value.clone(),
                        },
                        p::StringOp::Insert { index, value } => string_edit::Op::Insert {
                            index: *index as usize,
                            value: value.clone(),
                        },
                        p::StringOp::Delete { index } => string_edit::Op::Delete {
                            index: *index as usize,
                        },
                    })
                    .collect(),
            };
            let plan = string_edit::plan_string_edit(s.doc.text(), &req);
            Self::apply_list_plan(s, r.base_revision, string_plan(plan))
        })
    }

    /// B2: add `texture ImageTexture { url "<url>" }` to an Appearance that
    /// has none, because the user chose an image. One transaction.
    pub fn add_texture(&self, r: &p::AddTextureRequest) -> Result<p::FieldEditOutcome, String> {
        self.with(r.session, |s| {
            if r.base_revision != s.doc.revision() {
                return stale_field(r.base_revision, s.doc.revision());
            }
            let Some((node_from, node_to)) = node_span(&r.item) else {
                return refused_field(field_edit::reason::NOT_A_NODE, None, None);
            };
            let plan = string_edit::plan_add_texture(
                s.doc.text(),
                &string_edit::AddTextureRequest {
                    node_from,
                    node_to,
                    url: r.url.clone(),
                    confirm_shared: r.confirm_shared,
                },
            );
            Self::apply_list_plan(s, r.base_revision, string_plan(plan))
        })
    }

    fn apply_list_plan(s: &mut Session, base: u64, plan: mf_edit::Plan) -> p::FieldEditOutcome {
        match plan {
            mf_edit::Plan::Unchanged => p::FieldEditOutcome::Unchanged,
            mf_edit::Plan::Refused {
                reason,
                message,
                op_index,
                component_index,
            } => refused_field(
                reason,
                Some(match op_index {
                    Some(i) if i > 0 => format!("step {}: {message}", i + 1),
                    _ => message,
                }),
                component_index.map(|i| i as u32),
            ),
            mf_edit::Plan::Ready {
                edit,
                new_text,
                node_from,
                node_to,
                ..
            } => {
                let edit = Edit {
                    from: edit.from,
                    to: edit.to,
                    insert: edit.insert,
                };
                match s.doc.apply_source_transaction(base, &[edit], &new_text) {
                    Ok(a) => p::FieldEditOutcome::Applied {
                        state: Self::state(s, a),
                        view: s.doc.view(),
                        changed: Vec::new(),
                        item: format!("node-{node_from}-{node_to}"),
                    },
                    Err(e) => refused_field(
                        field_edit::reason::TRANSACTION_REJECTED,
                        Some(e.to_string()),
                        None,
                    ),
                }
            }
        }
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
                message: "Select an object (a Transform) to change it.".into(),
            });
        };
        Ok(
            match manipulate::manip_target(&text, from, to, manip_tool(r.tool)) {
                Ok(t) => p::TranslateTargetOutcome::Ready {
                    revision: current,
                    item: r.item.clone(),
                    def_name: t.def,
                    root_index: t.root_index.map(|i| i as u32),
                    preview_from: from - preview_offset(&text),
                    preview_to: to - preview_offset(&text),
                    translation: t.translation,
                    origin: t.origin,
                    rotation: t.rotation,
                    scale: t.scale,
                    scale_orientation: t.scale_orientation,
                    center: t.center,
                    pivot: t.pivot,
                    shared: t.shared.as_ref().map(|s| p::SharedProto {
                        proto: s.proto.clone(),
                        instances: s.instances as u32,
                        name_ambiguous: s.name_ambiguous,
                    }),
                },
                Err(e) => p::TranslateTargetOutcome::Refused {
                    reason: e.reason.into(),
                    message: e.message,
                },
            },
        )
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
        let target = match manipulate::manip_target(&text, from, to, manip_tool(r.tool)) {
            Ok(t) => t,
            Err(e) => return Ok(refused(e.reason, &e.message)),
        };
        let unprovable = "The preview cannot prove which rendered object is this Transform, so it cannot be changed here. Use the Inspector.";
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
                Ok(chain)
                    if target.shared.is_none()
                        && chain.len() == 1
                        && chain[0].node_type == "Transform" =>
                {
                    p::TranslateProveOutcome::Proven {
                        revision: current,
                        item: r.item.clone(),
                        instance: None,
                        parent_frame: None,
                    }
                }
                // B3: a PROTO body Transform. The runtime node is ONE
                // instance's copy; its proven chain must be the Transform,
                // frame-neutral body groups only, then a document instance
                // of exactly that PROTO. LV2: the instance may sit under
                // further document nodes; its parent frame is composed from
                // their SOURCE frames (`instance_parent_frame`), each link
                // proven by this same chain -- never guessed.
                Ok(chain) if target.shared.is_some() => {
                    let sh = target.shared.as_ref().expect("checked");
                    Self::prove_body_chain(&text, &parsed, r, current, sh, &chain)
                }
                Ok(_) => refused(manipulate::reason::NOT_TOP_LEVEL, unprovable),
                Err(res) => refused(res.reason(), unprovable),
            },
        )
    }

    /// B3 (LV2): the shared-body half of `translate_prove`. `chain` is the
    /// proven runtime chain of ONE instance's copy (the Transform first).
    fn prove_body_chain(
        text: &str,
        parsed: &wrlforge_vrml::ParseResult,
        r: &p::TranslateProveRequest,
        current: u64,
        sh: &field_edit::SharedDefinition,
        chain: &[&wrlforge_vrml::ast::Node],
    ) -> p::TranslateProveOutcome {
        let refused = |reason: &str, message: String| p::TranslateProveOutcome::Refused {
            reason: reason.into(),
            message,
        };
        let span = |n: &wrlforge_vrml::ast::Node| {
            (n.range.start.offset as u64, n.range.end.offset as u64)
        };
        let in_body = |n: &wrlforge_vrml::ast::Node| {
            let (f, t) = span(n);
            field_edit::shared_definition(parsed, f, t).is_some()
        };
        // The first link outside the PROTO body is the instance.
        let k = chain.iter().position(|n| !in_body(n));
        let Some(k) = k.filter(|&k| {
            k >= 1
                && chain[0].node_type == "Transform"
                && chain[1..k]
                    .iter()
                    .all(|n| manipulate::is_frame_neutral(&n.node_type))
                && chain[k].node_type == sh.proto
        }) else {
            return refused(
                manipulate::reason::BODY_FRAME,
                format!("The preview cannot prove that this rendered copy is inside an instance of PROTO {}. Use the Inspector.", sh.proto),
            );
        };
        let inst = span(chain[k]);
        let inst_item = format!("node-{}-{}", inst.0, inst.1);
        if let Some(want) = &r.instance {
            if *want != inst_item {
                return refused(
                    "proto-instance-changed",
                    format!("The rendered copy belongs to another instance of PROTO {} than the one the handles were on; nothing is bound.", sh.proto),
                );
            }
        }
        let ancestors: Vec<(u64, u64)> = chain[k + 1..].iter().map(|n| span(n)).collect();
        match manipulate::instance_parent_frame(text, &ancestors, manip_tool(r.tool)) {
            Ok(m) => {
                let off = preview_offset(text);
                p::TranslateProveOutcome::Proven {
                    revision: current,
                    item: r.item.clone(),
                    instance: Some(p::InstanceRef {
                        item: inst_item,
                        preview_from: inst.0 - off,
                        preview_to: inst.1 - off,
                    }),
                    parent_frame: (m != manipulate::IDENTITY4).then_some(m),
                }
            }
            Err(e) => refused(e.reason, e.message),
        }
    }

    /// B2 selection highlight: the exact preview span the preview must
    /// locate `item` by. Read-only; any other revision is `Stale`.
    pub fn highlight_target(
        &self,
        r: &p::HighlightTargetRequest,
    ) -> Result<p::HighlightTargetOutcome, String> {
        let (text, current) = self.text_at(r.session)?;
        if r.revision != current {
            return Ok(p::HighlightTargetOutcome::Stale { current });
        }
        let off = preview_offset(&text);
        match node_span(&r.item) {
            Some((from, to)) if from >= off => {
                let parsed = wrlforge_vrml::parse(&text);
                // LV2: a node that is never drawn has nothing to outline;
                // say so instead of failing later for another reason.
                let ty = field_edit::inspect_node_fields(&parsed, &text, from, to).node_type;
                if let Some(t) = ty.filter(|t| {
                    p::highlight::node_role(t) == p::highlight::NodeRole::NotRendered
                }) {
                    return Ok(p::HighlightTargetOutcome::Refused {
                        reason: p::highlight::refusal::NOT_RENDERED.into(),
                        message: format!("A {t} is not drawn, so it has no shape to outline."),
                    });
                }
                Ok(p::HighlightTargetOutcome::Ready {
                    revision: current,
                    item: r.item.clone(),
                    preview_from: from - off,
                    preview_to: to - off,
                    // B3: a PROTO body node has one runtime copy per instance.
                    body: field_edit::shared_definition(&parsed, from, to).is_some(),
                })
            }
            _ => Ok(p::HighlightTargetOutcome::Refused {
                reason: field_edit::reason::NOT_A_NODE.into(),
                message: "Only a node instance can be outlined.".into(),
            }),
        }
    }

    /// B2 selection highlight: prove that the runtime node the preview
    /// located for `item` IS that authored node (any depth), with the same
    /// proof the gizmo binds through (`pick::resolve_node`: exact span join,
    /// unique occurrences, runtime chain == source containment chain). The
    /// proven ancestors are returned so the preview can compose the frame;
    /// an ancestor whose frame effect is unknown (Billboard, a PROTO
    /// instance, ...) refuses. Read-only.
    pub fn highlight_prove(
        &self,
        r: &p::HighlightProveRequest,
    ) -> Result<p::HighlightProveOutcome, String> {
        use wrlforge_vrml::pick as pk;
        let (text, current) = self.text_at(r.session)?;
        let refused = |reason: &str, message: &str| p::HighlightProveOutcome::Refused {
            reason: reason.into(),
            message: message.into(),
        };
        let preview = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        if r.revision != current || p::preview_hash(preview) != r.preview_hash {
            return Ok(p::HighlightProveOutcome::Stale { current });
        }
        let Some((from, to)) = node_span(&r.item) else {
            return Ok(refused(field_edit::reason::NOT_A_NODE, "Not an object."));
        };
        let unprovable = "The preview cannot prove which rendered object this is.";
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
        let off = preview_offset(&text);
        Ok(match pk::resolve_node(&hit, &parsed, off, from, to) {
            Ok(chain) => {
                let mut ancestors = Vec::with_capacity(chain.len().saturating_sub(1));
                let span = |n: &wrlforge_vrml::ast::Node| {
                    (n.range.start.offset as u64, n.range.end.offset as u64)
                };
                let in_body = |n: &wrlforge_vrml::ast::Node| {
                    let (f, t) = span(n);
                    field_edit::shared_definition(&parsed, f, t).is_some()
                };
                // LV2: a non-geometry node (Appearance, Material, a texture,
                // Coordinate, ...) has no bounds of its own. The outline is
                // of the Shape that owns it -- the FIRST Shape of its proven
                // chain -- labeled as the owner; the frame chain starts
                // above that Shape.
                let mut owner = None;
                let mut first = 1;
                if p::highlight::node_role(&chain[0].node_type) == p::highlight::NodeRole::Property {
                    let Some(k) = chain.iter().position(|n| n.node_type == "Shape") else {
                        return Ok(refused(
                            p::highlight::refusal::NO_OWNER,
                            "This node is not drawn by itself and no Shape that uses it was proven.",
                        ));
                    };
                    let (f, t) = span(chain[k]);
                    if f < off {
                        return Ok(refused(pk::reason::SPAN_OUT_OF_RANGE, unprovable));
                    }
                    owner = Some(p::HighlightAncestor {
                        preview_from: f - off,
                        preview_to: t - off,
                        node_type: "Shape".into(),
                        proto_instance: false,
                    });
                    first = k + 1;
                }
                for (i, n) in chain.iter().enumerate().skip(first) {
                    // B3: the instance a PROTO body chain was proven through
                    // (the chain proof joined the body link to exactly this
                    // document instance): it renders as its body root, the
                    // previous link, and adds no frame.
                    let proto_instance = !wrlforge_vrml::node_schema::is_vrml97_node(&n.node_type)
                        && in_body(chain[i - 1])
                        && !in_body(n);
                    if !proto_instance && p::highlight::ancestor_kind(&n.node_type).is_none() {
                        return Ok(refused(
                            p::highlight::refusal::ANCESTOR,
                            &format!(
                                "It is inside a {}, whose placement the outline cannot follow.",
                                n.node_type
                            ),
                        ));
                    }
                    let (f, t) = (n.range.start.offset as u64, n.range.end.offset as u64);
                    if f < off {
                        return Ok(refused(pk::reason::SPAN_OUT_OF_RANGE, unprovable));
                    }
                    ancestors.push(p::HighlightAncestor {
                        preview_from: f - off,
                        preview_to: t - off,
                        node_type: n.node_type.clone(),
                        proto_instance,
                    });
                }
                p::HighlightProveOutcome::Proven {
                    revision: current,
                    item: r.item.clone(),
                    node_type: chain[0].node_type.clone(),
                    ancestors,
                    owner,
                }
            }
            Err(res) => refused(res.reason(), unprovable),
        })
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
            let (plan, text) = manipulate::plan_translate_with(
                s.doc.text(),
                node_from,
                node_to,
                manip_axis(r.axis),
                r.value,
                r.decimals,
                r.confirm_shared,
            );
            Self::apply_manip(s, r.base_revision, node_from, node_to, plan, text, r.instance.as_deref())
        })
    }

    /// VISUAL-4B: commit one rotation drag (`rotation := Rot(axis, delta) ·
    /// rotation`, about the pivot). Same proof and transaction rules as
    /// `translate`.
    pub fn rotate(&self, r: &p::RotateRequest) -> Result<p::TranslateOutcome, String> {
        self.with(r.session, |s| {
            let (node_from, node_to) = match Self::manip_preflight(s, r.base_revision, &r.item) {
                Ok(span) => span,
                Err(o) => return o,
            };
            let (plan, text) = manipulate::plan_rotate_with(
                s.doc.text(),
                node_from,
                node_to,
                manip_axis(r.axis),
                r.delta,
                r.confirm_shared,
            );
            Self::apply_manip(s, r.base_revision, node_from, node_to, plan, text, r.instance.as_deref())
        })
    }

    /// VISUAL-4B: commit one scale drag (one component or uniform).
    pub fn scale(&self, r: &p::ScaleRequest) -> Result<p::TranslateOutcome, String> {
        self.with(r.session, |s| {
            let (node_from, node_to) = match Self::manip_preflight(s, r.base_revision, &r.item) {
                Ok(span) => span,
                Err(o) => return o,
            };
            let (plan, text) = manipulate::plan_scale_with(
                s.doc.text(),
                node_from,
                node_to,
                r.axis.map(manip_axis),
                r.factor,
                r.confirm_shared,
            );
            Self::apply_manip(s, r.base_revision, node_from, node_to, plan, text, r.instance.as_deref())
        })
    }

    /// The revision and item checks every gizmo commit starts with.
    fn manip_preflight(
        s: &Session,
        base_revision: u64,
        item: &str,
    ) -> Result<(u64, u64), p::TranslateOutcome> {
        if base_revision != s.doc.revision() {
            return Err(p::TranslateOutcome::Refused {
                reason: "parse-session-is-stale".into(),
                message: format!(
                    "The document changed (revision {} → {}); the object was not changed.",
                    base_revision,
                    s.doc.revision()
                ),
            });
        }
        node_span(item).ok_or_else(|| p::TranslateOutcome::Refused {
            reason: field_edit::reason::NOT_A_NODE.into(),
            message: "Not an object.".into(),
        })
    }

    /// Apply a gizmo plan as ONE transaction whose result must equal the
    /// planned, round-trip-verified text.
    fn apply_manip(
        s: &mut Session,
        base_revision: u64,
        node_from: u64,
        node_to: u64,
        plan: field_edit::Plan,
        text: Option<String>,
        instance: Option<&str>,
    ) -> p::TranslateOutcome {
        let refused = |reason: &str, message: String| p::TranslateOutcome::Refused {
            reason: reason.into(),
            message,
        };
        match plan {
            field_edit::Plan::Unchanged => p::TranslateOutcome::Unchanged,
            field_edit::Plan::Refused {
                reason, message, ..
            } => refused(reason, message.unwrap_or_else(|| reason.to_string())),
            field_edit::Plan::Ready {
                edits, new_text, ..
            } => {
                // The edits lie inside the node: its start is fixed and its
                // end moves by the length change.
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
                let old = instance.map(|_| s.doc.text().to_string());
                match s
                    .doc
                    .apply_source_transaction(base_revision, &edits, &new_text)
                {
                    Ok(a) => {
                        // B3 (LV2): carry the gizmo's PROTO instance through
                        // the EXACT changes just applied (Tier-1).
                        let (instance, instance_lost) = match (instance, old) {
                            (Some(i), Some(old)) => match carry_instance(
                                &old,
                                s.doc.text(),
                                (node_from, node_to),
                                i,
                                s.doc.last_changes(),
                            ) {
                                Ok(r) => (Some(r), None),
                                Err(why) => (None, Some(why.to_string())),
                            },
                            _ => (None, None),
                        };
                        p::TranslateOutcome::Applied {
                            state: Self::state(s, a),
                            view: s.doc.view(),
                            item,
                            text: text.unwrap_or_default(),
                            instance,
                            instance_lost,
                        }
                    }
                    Err(e) => refused(field_edit::reason::TRANSACTION_REJECTED, e.to_string()),
                }
            }
        }
    }

    /// Preview input: the canonical text, minus a leading U+FEFF. X_ITE's
    /// VRML parser rejects a BOM before `#VRML`; stripping it here is a
    /// display projection only -- the document and the file keep the BOM.
    ///
    /// TEXTURE-LOCAL-1: every call starts a new preview generation with a
    /// NEW resource token (the previous one stops being served) and checks
    /// the text's `ImageTexture` URLs against the document folder. The text
    /// is not changed: only X_ITE's base URL points at the token.
    pub fn preview_source(&self, id: p::SessionId) -> Result<p::PreviewSource, String> {
        let (text, revision, folder, token) = self.with(id, |s| {
            let t = s.doc.text();
            let text = t.strip_prefix('\u{FEFF}').unwrap_or(t).to_string();
            let folder = s.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
            let token = folder.as_ref().map(|_| resources::new_token());
            s.resource_token = token.clone();
            (text, s.doc.revision(), folder, token)
        })?;
        // The URL check stats files: outside the session lock.
        let texture_warnings = resources::check_textures(&text, folder.as_deref());
        let route_warnings = invalid_routes(&text);
        Ok(p::PreviewSource {
            revision,
            hash: p::preview_hash(&text),
            text,
            resource_base: resources::base_url(
                token.as_deref().unwrap_or(resources::NO_FOLDER_TOKEN),
            ),
            texture_warnings,
            route_warnings,
        })
    }

    /// The folder `token` may read from: the document folder of the ONE open
    /// session whose CURRENT token it is, looked up now (after a Save As it
    /// is the new folder). Anything else is `Stale`.
    pub fn resource_folder(&self, token: &str) -> Result<(p::SessionId, PathBuf), resources::Refusal> {
        use resources::Refusal;
        let map = self.sessions.lock().map_err(|_| Refusal::Unreadable)?;
        let (id, s) = map
            .iter()
            .find(|(_, s)| s.resource_token.as_deref() == Some(token))
            .ok_or(Refusal::Stale)?;
        let folder = s
            .path
            .as_deref()
            .and_then(Path::parent)
            .ok_or(Refusal::Untitled)?;
        Ok((*id, folder.to_path_buf()))
    }

    /// Whether `token` is still the current token of session `id` (checked
    /// again after a read: a reply for a replaced generation is refused).
    pub fn resource_token_current(&self, id: p::SessionId, token: &str) -> bool {
        self.sessions
            .lock()
            .ok()
            .and_then(|m| m.get(&id).map(|s| s.resource_token.as_deref() == Some(token)))
            .unwrap_or(false)
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

/// RUST-STAB-1: the recovery side of the service. Kept apart from the Save
/// path: nothing here writes a document file or changes undo history.
impl Service {
    /// Write every session with an unwritten dirty state. The text is copied
    /// under the session lock; the disk write runs without it. Returns the
    /// number of records replaced.
    pub fn recovery_pass(&self) -> usize {
        let Some(rec) = self.recovery.get() else {
            return 0;
        };
        let mut written = 0;
        for id in rec.take_pending() {
            let snap = self.with(id, |s| {
                s.doc.dirty().then(|| recovery::Snapshot {
                    record: s.recovery_id.clone(),
                    sequence: s.recovery_seq,
                    revision: s.doc.revision(),
                    format: s.format.as_str(),
                    name: Self::name(s),
                    original: s
                        .path
                        .as_ref()
                        .zip(s.stamp.as_ref())
                        .map(|(p, st)| recovery::Original::of(p, &st.bytes, st.mtime)),
                    text: s.doc.text().to_string(),
                })
            });
            // A closed session or a clean one has nothing to write.
            if let Ok(Some(snap)) = snap {
                if rec.write(&snap) == Ok(true) {
                    written += 1;
                }
            }
        }
        written
    }

    /// The user explicitly discarded `id`'s unsaved changes (New World →
    /// Discard, quit → Discard): remove its record now.
    pub fn discard_recovery(&self, id: p::SessionId) -> Result<(), String> {
        let Some(rec) = self.recovery.get() else {
            return Ok(());
        };
        let record = self.with(id, |s| s.recovery_id.clone())?;
        rec.retire(&record, rec.next_seq())
    }

    /// Sessions holding unsaved changes, with their display names.
    pub fn dirty_sessions(&self) -> Vec<(p::SessionId, String)> {
        let Ok(map) = self.sessions.lock() else {
            return vec![];
        };
        let mut v: Vec<_> = map
            .iter()
            .filter(|(_, s)| s.doc.dirty())
            .map(|(&id, s)| (id, Self::name(s)))
            .collect();
        v.sort();
        v
    }

    /// Records left by a WRL Forge process that exited without saving.
    pub fn recovery_list(&self) -> p::RecoveryList {
        let Some(rec) = self.recovery.get() else {
            return p::RecoveryList::default();
        };
        let warning = rec.warning();
        let Some(store) = rec.store() else {
            return p::RecoveryList {
                warning,
                ..Default::default()
            };
        };
        let scan = store.scan();
        p::RecoveryList {
            entries: scan
                .offered
                .into_iter()
                .map(|o| p::RecoveryEntry {
                    id: o.header.record,
                    name: o.header.name,
                    untitled: o.header.untitled,
                    format: o.header.format,
                    saved_at_ms: o.header.saved_at_ms,
                    bytes: o.header.text_bytes,
                    original_now: o.original_now.map(str::to_string),
                })
                .collect(),
            damaged: scan
                .damaged
                .into_iter()
                .map(|d| p::RecoveryDamaged {
                    id: d.id,
                    reason: d.reason,
                })
                .collect(),
            warning,
        }
    }

    /// Restore: the record's exact text in a NEW untitled, dirty session in
    /// the record's format. Nothing on disk is written except the record
    /// itself (re-owned by this process, same text), so the original file
    /// is untouched and only Save As can give the work a file.
    pub fn recovery_restore(&self, record: &str) -> p::OpenOutcome {
        let failed = |m: String| p::OpenOutcome::Failed { message: m };
        let Some(rec) = self.recovery.get() else {
            return failed("crash recovery is not available".into());
        };
        let Some(store) = rec.store() else {
            return failed("crash recovery is not available".into());
        };
        if !store.scan().offered.iter().any(|o| o.header.record == record) {
            return failed("that recovery record is not available to restore".into());
        }
        let (h, text) = match store.read(record) {
            Ok(r) => r,
            Err(e) => return failed(format!("the recovery record cannot be read: {e}")),
        };
        let format = if h.format == "gzip" {
            Format::Gzip
        } else {
            Format::Plain
        };
        let mut s = Session::new(None, format, None, Document::new_unsaved(text));
        s.recovery_id = h.record.clone();
        s.recovery_seq = rec.next_seq();
        s.recovered_name = Some(recovery::safe_name(&h.name));
        // Take ownership of the record (same exact text), so no other
        // window offers it while this one holds the work.
        let snap = recovery::Snapshot {
            record: h.record,
            sequence: s.recovery_seq,
            revision: s.doc.revision(),
            format: format.as_str(),
            name: h.name,
            original: h.original,
            text: s.doc.text().to_string(),
        };
        match rec.write(&snap) {
            Ok(true) => self.insert(s),
            Ok(false) => failed("that recovery record changed while it was restored".into()),
            Err(e) => failed(format!("the recovery record cannot be re-owned: {e}")),
        }
    }

    /// The text of `id`'s own recovery record on disk, if one exists
    /// (read-only; smoke evidence).
    pub fn recovery_record_text(&self, id: p::SessionId) -> Option<String> {
        let rec = self.recovery.get()?;
        let record = self.with(id, |s| s.recovery_id.clone()).ok()?;
        rec.store()?.read(&record).ok().map(|(_, t)| t)
    }

    /// Discard one record offered at startup (after the user confirmed).
    pub fn recovery_discard(&self, record: &str) -> Result<(), String> {
        let rec = self.recovery.get().ok_or("crash recovery is not available")?;
        let store = rec.store().ok_or("crash recovery is not available")?;
        store.remove_orphan(record)
    }
}

#[cfg(test)]
impl Service {
    /// A session holding `text` (no path), for tests in other modules.
    pub(crate) fn open_text_for_test(&self, text: &str) -> p::SessionId {
        match self.insert(Session::new(None, Format::Plain, None, Document::new(text.into()))) {
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

/// XITE-16-UPGRADE-2: the ROUTEs the scope graph PROVES invalid in `text`
/// (the exact preview text). X_ITE 16.5.0 drops such a ROUTE and renders the
/// rest; 15.1.10 rejected the scene. Only a whole-ROUTE verdict of
/// `Unresolved` or `Invalid` counts: an ambiguous, recovered or unsupported
/// answer proves nothing and is never reported. Nothing is inferred from what
/// X_ITE drew, and the text is not changed.
fn invalid_routes(text: &str) -> Vec<p::RouteWarning> {
    let parsed = wrlforge_vrml::parse(text);
    let graph = wrlforge_vrml::scope::build(&parsed);
    let findings = graph.diagnostics();
    graph
        .routes
        .iter()
        .filter(|r| r.status.class() == "unresolved")
        .map(|r| p::RouteWarning {
            line: r.range.start.line,
            detail: first_finding_inside(&findings, r.range.start.offset, r.range.end.offset)
                .map(|d| format!("{}: {}", d.code, d.message))
                .unwrap_or_else(|| r.reason.to_string()),
        })
        .collect()
}

/// STACK-REVIEW-1 (PR #140 r4242714062): the first finding, in `findings`
/// order, whose range lies inside `start..=end`. `ScopeGraph::diagnostics`
/// sorts by (start, end, code), so a binary search finds the first finding
/// that starts at or after `start`, and the scan stops at the first one that
/// starts after `end` (a finding cannot end before it starts). The answer is
/// the same one a linear `find` gives, without a full scan per ROUTE.
fn first_finding_inside(
    findings: &[wrlforge_vrml::scope::ScopeDiagnostic],
    start: u32,
    end: u32,
) -> Option<&wrlforge_vrml::scope::ScopeDiagnostic> {
    debug_assert!(findings
        .windows(2)
        .all(|w| w[0].range.start.offset <= w[1].range.start.offset));
    let first = findings.partition_point(|d| d.range.start.offset < start);
    findings[first..]
        .iter()
        .take_while(|d| d.range.start.offset <= end)
        .find(|d| d.range.end.offset <= end)
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

    /// XITE-16-UPGRADE-2: the preview's ROUTE warning comes only from a
    /// PROVEN whole-ROUTE defect in the exact preview text. Uncertain answers
    /// never warn, the source and its history are untouched, and Save works.
    /// The pre-STACK-REVIEW-1 lookup, kept verbatim as the reference that
    /// the indexed lookup must reproduce exactly.
    fn invalid_routes_linear(text: &str) -> Vec<(u32, String)> {
        let parsed = wrlforge_vrml::parse(text);
        let graph = wrlforge_vrml::scope::build(&parsed);
        let findings = graph.diagnostics();
        graph
            .routes
            .iter()
            .filter(|r| r.status.class() == "unresolved")
            .map(|r| {
                let inside = |d: &&wrlforge_vrml::scope::ScopeDiagnostic| {
                    d.range.start.offset >= r.range.start.offset
                        && d.range.end.offset <= r.range.end.offset
                };
                (
                    r.range.start.line,
                    findings
                        .iter()
                        .find(inside)
                        .map(|d| format!("{}: {}", d.code, d.message))
                        .unwrap_or_else(|| r.reason.to_string()),
                )
            })
            .collect()
    }

    fn indexed(text: &str) -> Vec<(u32, String)> {
        invalid_routes(text)
            .into_iter()
            .map(|w| (w.line, w.detail))
            .collect()
    }

    /// Many ROUTEs of every verdict: valid, each proven-invalid kind,
    /// ambiguous, unsupported (EXTERNPROTO), before-DEF and across a PROTO.
    fn many_routes(n: usize) -> String {
        let mut t = String::from(
            "#VRML V2.0 utf8\nEXTERNPROTO E [ eventOut SFFloat out ] \"e.wrl\"\nDEF A E { }\n\
             DEF D TimeSensor { }\nDEF D TimeSensor { }\n\
             PROTO Q [ ] { DEF In TimeSensor { } }\n",
        );
        for i in 0..n {
            t.push_str(&format!(
                "ROUTE Late{i}.fraction_changed TO P{i}.set_fraction\n"
            ));
            t.push_str(&format!("DEF T{i} TimeSensor {{ }}\nDEF P{i} PositionInterpolator {{ }}\nDEF Late{i} TimeSensor {{ }}\n"));
            t.push_str(&format!(
                "ROUTE T{i}.fraction_changed TO P{i}.set_fraction\n"
            ));
            t.push_str(&format!("ROUTE T{i}.isActive TO T{i}.enabled\n"));
            t.push_str(&format!(
                "ROUTE Ghost{i}.fraction_changed TO P{i}.set_fraction\n"
            ));
            t.push_str(&format!("ROUTE T{i}.nosuch TO P{i}.set_fraction\n"));
            t.push_str(&format!("ROUTE P{i}.set_fraction TO T{i}.set_enabled\n"));
            t.push_str(&format!(
                "ROUTE T{i}.fraction_changed TO P{i}.value_changed\n"
            ));
            t.push_str(&format!(
                "ROUTE P{i}.value_changed TO T{i}.set_cycleInterval\n"
            ));
            t.push_str(&format!("ROUTE D.fraction_changed TO P{i}.set_fraction\n"));
            t.push_str(&format!("ROUTE A.other TO P{i}.set_fraction\n"));
            t.push_str(&format!("ROUTE In.fraction_changed TO P{i}.set_fraction\n"));
        }
        t
    }

    #[test]
    fn route_warning_lookup_matches_the_linear_reference() {
        for n in [0, 1, 3, 40] {
            let t = many_routes(n);
            let want = invalid_routes_linear(&t);
            assert_eq!(indexed(&t), want, "n={n}");
            if n > 0 {
                // 7 proven-invalid ROUTEs per block; the valid, ambiguous,
                // unsupported ones never warn.
                assert_eq!(want.len(), 7 * n, "n={n}: {want:?}");
            }
        }
        // Every repository .wrl fixture (plain text) gives the same warnings.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut stack = vec![
            root.join("test/fixtures"),
            root.join("apps/desktop-tauri/src-tauri/tests/fixtures"),
        ];
        let (mut files, mut warned) = (0, 0);
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|x| x == "wrl") {
                    let Ok(t) = fs::read_to_string(&path) else {
                        continue;
                    };
                    let want = invalid_routes_linear(&t);
                    assert_eq!(indexed(&t), want, "{}", path.display());
                    files += 1;
                    warned += usize::from(!want.is_empty());
                }
            }
        }
        assert!(files > 50 && warned >= 2, "files={files} warned={warned}");
    }

    /// Timing only: `cargo test --release -- --ignored --nocapture route_warning_lookup_timing`
    #[test]
    #[ignore]
    fn route_warning_lookup_timing() {
        for n in [250, 1000, 4000] {
            let t = many_routes(n);
            let s = std::time::Instant::now();
            let old = invalid_routes_linear(&t);
            let t_old = s.elapsed();
            let s = std::time::Instant::now();
            let new = indexed(&t);
            let t_new = s.elapsed();
            assert_eq!(new, old);
            let s = std::time::Instant::now();
            let g = wrlforge_vrml::scope::build(&wrlforge_vrml::parse(&t));
            let _ = g.diagnostics();
            let t_base = s.elapsed();
            println!(
                "ROUTE-TIMING n_blocks={n} routes={} invalid={} linear={t_old:?} indexed={t_new:?} parse+scope+diagnostics={t_base:?}",
                11 * n, old.len()
            );
        }
    }

    #[test]
    fn route_warnings_report_only_proven_invalid_routes() {
        let lines = |t: &str| invalid_routes(t).iter().map(|w| w.line).collect::<Vec<_>>();
        // The two fixtures X_ITE 16.5.0 now renders partially.
        let m6 =
            include_str!("../tests/fixtures/flux-regression/malformed/m6_route_before_def.wrl");
        let w = invalid_routes(m6);
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].line, 4);
        assert!(w[0].detail.starts_with("SCOPE003: "), "{w:?}");
        let inv = include_str!("../../../../test/fixtures/vrml/invalid-route.wrl");
        let w = invalid_routes(inv);
        assert_eq!(lines(inv), [4, 5], "{w:?}");
        assert!(
            w.iter().all(|w| w.detail.starts_with("SCOPE001: ")),
            "{w:?}"
        );
        // Proven event, direction and type defects warn too.
        let head = "#VRML V2.0 utf8\nDEF T TimeSensor { }\nDEF P PositionInterpolator { }\n";
        for (route, code) in [
            ("ROUTE T.nosuch TO P.set_fraction\n", "SCOPE101"),
            ("ROUTE P.set_fraction TO T.set_enabled\n", "SCOPE102"),
            ("ROUTE T.fraction_changed TO P.value_changed\n", "SCOPE103"),
            ("ROUTE P.value_changed TO T.set_cycleInterval\n", "SCOPE104"),
        ] {
            let w = invalid_routes(&format!("{head}{route}"));
            assert_eq!(w.len(), 1, "{route}: {w:?}");
            assert!(w[0].detail.starts_with(code), "{route}: {w:?}");
        }
        // Valid ROUTEs (including the 4.10.2 shorthand) never warn.
        let ok = format!(
            "{head}ROUTE T.fraction_changed TO P.set_fraction\nROUTE T.isActive TO T.enabled\n"
        );
        assert!(invalid_routes(&ok).is_empty(), "{:?}", invalid_routes(&ok));
        // Uncertain answers prove nothing: an ambiguous DEF, a damaged parse,
        // an EXTERNPROTO the URL would decide.
        let amb = "#VRML V2.0 utf8\nDEF T TimeSensor { }\nDEF T TimeSensor { }\nDEF P PositionInterpolator { }\nROUTE T.fraction_changed TO P.set_fraction\n";
        assert!(invalid_routes(amb).is_empty(), "{:?}", invalid_routes(amb));
        let damaged = "#VRML V2.0 utf8\nDEF P PositionInterpolator { }\nROUTE Ghost.fraction_changed TO P.set_fraction\nGroup { children [\n";
        assert!(
            invalid_routes(damaged).is_empty(),
            "{:?}",
            invalid_routes(damaged)
        );
        let ext = "#VRML V2.0 utf8\nEXTERNPROTO E [ eventOut SFFloat out ] \"e.wrl\"\nDEF A E { }\nDEF P PositionInterpolator { }\nROUTE A.other TO P.set_fraction\n";
        assert!(invalid_routes(ext).is_empty(), "{:?}", invalid_routes(ext));
        // Through the service: BOM + CRLF source, the preview text is the
        // source minus the BOM, the warning arrives, nothing else changes.
        let d = tmp("route-warn");
        let src = format!("\u{FEFF}{}", inv.replace('\n', "\r\n"));
        fs::write(d.join("r.wrl"), &src).unwrap();
        let bytes = fs::read(d.join("r.wrl")).unwrap();
        let svc = Service::new(clock);
        let id = opened(svc.open_path(&d.join("r.wrl"))).session;
        let ps = svc.preview_source(id).unwrap();
        assert_eq!(ps.text, src.trim_start_matches('\u{FEFF}'));
        assert_eq!(
            ps.route_warnings.iter().map(|w| w.line).collect::<Vec<_>>(),
            [4, 5]
        );
        let info = svc.snapshot(id).unwrap();
        assert_eq!(
            (info.revision, info.dirty, info.can_undo),
            (0, false, false)
        );
        assert_eq!(svc.text(id).unwrap(), src);
        // Advisory only: Save As writes the exact bytes.
        match svc.save_as(id, &d.join("copy.wrl")) {
            p::SaveOutcome::Saved { .. } => {}
            o => panic!("{o:?}"),
        }
        assert_eq!(fs::read(d.join("copy.wrl")).unwrap(), bytes);
        assert_eq!(fs::read(d.join("r.wrl")).unwrap(), bytes);
        let _ = fs::remove_dir_all(&d);
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

    /// B2 texture assignment: an MFString url edit and Add texture are one
    /// undo step each, exact text both ways, revision- and list-checked.
    #[test]
    fn texture_url_edits_apply_undo_redo_exactly() {
        let svc = Service::new(clock);
        let text = "#VRML V2.0 utf8\r\nShape {\r\n  appearance Appearance {\r\n    texture ImageTexture { url [ \"a.png\" \"b.png\" ] }\r\n  }\r\n}\r\nShape { appearance Appearance { material Material {} } }\r\n";
        let id = svc.open_text_for_test(text);
        let span = |t: &str, needle: &str| {
            let b = t.find(needle).unwrap();
            let f = t[..b].encode_utf16().count();
            format!("node-{f}-{}", f + needle.encode_utf16().count())
        };
        let tex = "ImageTexture { url [ \"a.png\" \"b.png\" ] }";
        let p::InspectOutcome::Found { inspection } = svc.inspect(id, &span(text, tex), 0).unwrap()
        else {
            panic!()
        };
        let f = &inspection.node.unwrap().fields[0];
        assert!(f.editable, "{}", f.reason);
        let strings = f.strings.clone().expect("an MFString list");
        assert_eq!((strings.form.as_str(), strings.values.clone()), ("bracketed", vec!["a.png".to_string(), "b.png".to_string()]));
        let req = |rev: u64, expected: Vec<String>, ops| p::StringEditRequest {
            session: id,
            base_revision: rev,
            item: span(text, tex),
            field_index: 0,
            field_name: "url".into(),
            expected,
            ops,
            confirm_shared: false,
        };
        // A list other than the one shown is refused and changes nothing.
        let o = svc
            .edit_strings(&req(0, vec!["x".into()], vec![p::StringOp::Delete { index: 0 }]))
            .unwrap();
        assert!(matches!(o, p::FieldEditOutcome::Refused { ref reason, .. } if reason == "string-list-changed"), "{o:?}");
        assert_eq!(svc.text(id).unwrap(), text);
        // Set one entry and remove the other: ONE edit.
        let o = svc
            .edit_strings(&req(
                0,
                strings.values.clone(),
                vec![
                    p::StringOp::Set { index: 0, value: "tex/wood \"1\".png".into() },
                    p::StringOp::Delete { index: 1 },
                ],
            ))
            .unwrap();
        let p::FieldEditOutcome::Applied { state, .. } = o else {
            panic!("{o:?}")
        };
        let one = text.replacen("[ \"a.png\" \"b.png\" ]", "[ \"tex/wood \\\"1\\\".png\" ]", 1);
        assert_eq!(svc.text(id).unwrap(), one);
        assert_eq!(state.revision, 1);
        // A stale revision is refused.
        let o = svc
            .edit_strings(&req(0, strings.values.clone(), vec![p::StringOp::Delete { index: 0 }]))
            .unwrap();
        assert!(matches!(o, p::FieldEditOutcome::Refused { ref reason, .. } if reason == "parse-session-is-stale"));
        // Add texture to the second Appearance.
        let app = "Appearance { material Material {} }";
        let o = svc
            .add_texture(&p::AddTextureRequest {
                session: id,
                base_revision: 1,
                item: span(&one, app),
                url: "img/brick.png".into(),
                confirm_shared: false,
            })
            .unwrap();
        assert!(matches!(o, p::FieldEditOutcome::Applied { .. }), "{o:?}");
        let two = one.replacen(
            app,
            "Appearance { material Material {} texture ImageTexture { url \"img/brick.png\" } }",
            1,
        );
        assert_eq!(svc.text(id).unwrap(), two);
        // An Appearance that has a texture is refused.
        let o = svc
            .add_texture(&p::AddTextureRequest {
                session: id,
                base_revision: 2,
                item: span(&two, "Appearance {\r\n    texture"),
                url: "c.png".into(),
                confirm_shared: false,
            })
            .unwrap();
        assert!(matches!(o, p::FieldEditOutcome::Refused { .. }), "{o:?}");
        // One undo step each, exact text both ways.
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), one);
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), text);
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), one);
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), two);
    }

    /// B2: the chosen image must be inside the document's folder tree; the
    /// url is relative, `/`-separated and resolves to exactly that file.
    #[test]
    fn texture_url_is_relative_and_confined() {
        let root = tmp("texurl");
        let doc = root.join("world");
        fs::create_dir_all(doc.join("tex/sub")).unwrap();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0";
        for f in ["a.png", "tex/sub/b c.png", "tex/50%#1.png"] {
            fs::write(doc.join(f), png).unwrap();
        }
        fs::write(root.join("outside.png"), png).unwrap();
        fs::write(doc.join("notes.txt"), "x").unwrap();
        fs::write(doc.join("fake.png"), "not an image").unwrap();
        assert_eq!(texture_url(&doc, &doc.join("a.png")).unwrap(), "a.png");
        assert_eq!(
            texture_url(&doc, &doc.join("tex/sub/b c.png")).unwrap(),
            "tex/sub/b c.png"
        );
        assert_eq!(
            texture_url(&doc, &doc.join("tex/50%#1.png")).unwrap(),
            "tex/50%25%231.png"
        );
        // Through `..` back inside is fine (the real path decides).
        assert_eq!(
            texture_url(&doc, &doc.join("tex/../a.png")).unwrap(),
            "a.png"
        );
        let e = texture_url(&doc, &root.join("outside.png")).unwrap_err();
        assert!(e.contains("outside the document's folder"), "{e}");
        assert!(texture_url(&doc, &doc.join("notes.txt")).is_err());
        assert!(texture_url(&doc, &doc.join("fake.png")).is_err());
        assert!(texture_url(&doc, &doc.join("missing.png")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("outside.png"), doc.join("link.png")).unwrap();
            let e = texture_url(&doc, &doc.join("link.png")).unwrap_err();
            assert!(e.contains("outside the document's folder"), "{e}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// B2: Delete and Duplicate are one undo step each, exact text both
    /// ways, revision-checked, and refused without consent in a PROTO body.
    #[test]
    fn delete_and_duplicate_apply_undo_redo_exactly() {
        let svc = Service::new(clock);
        let text = "#VRML V2.0 utf8\r\nGroup {\r\n  children [\r\n    DEF A Shape {}\r\n    Transform {}\r\n  ]\r\n}\r\n";
        let id = svc.open_text_for_test(text);
        let span = |t: &str, needle: &str| {
            let b = t.find(needle).unwrap();
            let f = t[..b].encode_utf16().count();
            format!("node-{f}-{}", f + needle.encode_utf16().count())
        };
        let req = |rev: u64, item: String, op| p::StructureEditRequest {
            session: id,
            base_revision: rev,
            item,
            op,
            confirm_shared: false,
            offset: Some(p::duplicate::Offset { axis: 0, step: 2.5 }),
        };
        // Duplicate: the copy is selected, its DEF renamed, and (P0-B) it
        // is placed beside the original: a Shape is wrapped in a Transform
        // moved by the requested offset.
        let o = svc
            .structure_edit(&req(0, span(text, "DEF A Shape {}"), p::StructureOp::Duplicate))
            .unwrap();
        let p::StructureEditOutcome::Applied { state, item, renamed, .. } = o else {
            panic!("{o:?}")
        };
        let copy = "Transform { translation 2.5 0 0 children [ DEF A_2 Shape {} ] }";
        let dup = text.replacen("DEF A Shape {}\r\n", &format!("DEF A Shape {{}}\r\n    {copy}\r\n"), 1);
        assert_eq!(svc.text(id).unwrap(), dup);
        assert_eq!((state.revision, state.can_undo), (1, true));
        assert_eq!(item, Some(span(&dup, copy)));
        assert_eq!(renamed, vec![("A".to_string(), "A_2".to_string())]);
        // A stale revision is refused and changes nothing.
        let stale = svc
            .structure_edit(&req(0, span(&dup, "Transform {}"), p::StructureOp::Delete))
            .unwrap();
        assert!(matches!(stale, p::StructureEditOutcome::Refused { ref reason, .. } if reason == "parse-session-is-stale"));
        assert_eq!(svc.text(id).unwrap(), dup);
        // Delete.
        let o = svc
            .structure_edit(&req(1, span(&dup, "Transform {}"), p::StructureOp::Delete))
            .unwrap();
        let p::StructureEditOutcome::Applied { state, item, .. } = o else {
            panic!("{o:?}")
        };
        let del = dup.replacen("    Transform {}\r\n", "", 1);
        assert_eq!(svc.text(id).unwrap(), del);
        assert_eq!((state.revision, item), (2, None));
        // One undo step each, exact text both ways.
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), dup);
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), text);
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), dup);
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), del);

        // A PROTO body node needs consent.
        let proto = "#VRML V2.0 utf8\nPROTO P [] { Group { children [ Shape {} ] } }\nP {}\n";
        let id = svc.open_text_for_test(proto);
        let mut r = p::StructureEditRequest {
            session: id,
            base_revision: 0,
            item: span(proto, "Shape {}"),
            op: p::StructureOp::Delete,
            confirm_shared: false,
            offset: None,
        };
        let o = svc.structure_edit(&r).unwrap();
        assert!(matches!(o, p::StructureEditOutcome::Refused { ref reason, .. }
            if reason == field_edit::reason::SHARED_DEFINITION_UNCONFIRMED));
        r.confirm_shared = true;
        assert!(matches!(svc.structure_edit(&r).unwrap(), p::StructureEditOutcome::Applied { .. }));
        assert_eq!(
            svc.text(id).unwrap(),
            "#VRML V2.0 utf8\nPROTO P [] { Group { children [ ] } }\nP {}\n"
        );
    }

    /// LV2-C: Undo / Redo keep a selected PROTO body node on the SAME
    /// rendered instance only when Rust proves that instance through the
    /// exact change; otherwise the instance is reported lost (the UI then
    /// clears the selection). Never the nearest or another instance.
    #[test]
    fn undo_redo_carry_the_selected_proto_instance_or_lose_it() {
        let svc = Service::new(clock);
        let text = "#VRML V2.0 utf8\nPROTO P [] { Transform { translation 0 0 0 } }\nP {}\nP {}\n";
        let id = svc.open_text_for_test(text);
        let u16at = |t: &str, needle: &str, nth: usize| -> (u64, u64) {
            let b = t.match_indices(needle).nth(nth).unwrap().0;
            let f = t[..b].encode_utf16().count() as u64;
            (f, f + needle.encode_utf16().count() as u64)
        };
        let edit = |rev: u64, (f, t): (u64, u64), insert: &str| {
            svc.edit(&p::EditRequest {
                session: id,
                base_revision: rev,
                from: f,
                to: t,
                insert: insert.into(),
                item: None,
            })
            .unwrap()
        };
        // Revision 1: the shared body's translation changes.
        edit(0, u16at(text, "0 0 0", 0), "1 0 0");
        let v1 = svc.text(id).unwrap();
        let body = u16at(&v1, "Transform { translation 1 0 0 }", 0);
        let second = u16at(&v1, "P {}", 1);
        let item = format!("node-{}-{}", body.0, body.1);
        let o = svc.history_with_instance(id, true, Some(&item), Some(second)).unwrap();
        let p::HistoryOutcome::Applied { item: carried, instance, instance_lost, .. } = o else {
            panic!("{o:?}")
        };
        let v0 = svc.text(id).unwrap();
        assert_eq!(v0, text);
        let want = u16at(text, "P {}", 1);
        assert_eq!(instance_lost, None);
        let i = instance.expect("the second instance is carried");
        assert_eq!(i.item, format!("node-{}-{}", want.0, want.1));
        assert_eq!((i.preview_from, i.preview_to), want);
        assert!(carried.is_some());
        // Redo, then a change that rewrites the instance itself: undoing it
        // cannot prove that instance, so it is reported lost.
        let p::HistoryOutcome::Applied { state, .. } = svc.redo(id).unwrap() else {
            panic!("redo")
        };
        let v1 = svc.text(id).unwrap();
        edit(state.revision, u16at(&v1, "P {}", 1), "P { }");
        let v2 = svc.text(id).unwrap();
        let body = u16at(&v2, "Transform { translation 1 0 0 }", 0);
        let item = format!("node-{}-{}", body.0, body.1);
        let o = svc
            .history_with_instance(id, true, Some(&item), Some(u16at(&v2, "P { }", 0)))
            .unwrap();
        let p::HistoryOutcome::Applied { instance, instance_lost, .. } = o else {
            panic!("{o:?}")
        };
        assert_eq!(instance, None);
        assert!(instance_lost.is_some());
    }

    /// LV2-C: without a proven size there is no guessed offset: the copy is
    /// placed exactly over the original and the reason is reported. An
    /// unusable offset is refused and changes nothing.
    #[test]
    fn duplicate_without_a_proven_size_is_not_moved() {
        let svc = Service::new(clock);
        let text = "#VRML V2.0 utf8\nShape { geometry Box {} }\n";
        let id = svc.open_text_for_test(text);
        let b = text.find("Shape").unwrap();
        let item = format!("node-{b}-{}", text.len() - 1);
        let req = |offset| p::StructureEditRequest {
            session: id,
            base_revision: 0,
            item: item.clone(),
            op: p::StructureOp::Duplicate,
            confirm_shared: false,
            offset,
        };
        let bad = svc
            .structure_edit(&req(Some(p::duplicate::Offset { axis: 3, step: 1.0 })))
            .unwrap();
        assert!(matches!(bad, p::StructureEditOutcome::Refused { .. }), "{bad:?}");
        assert_eq!(svc.text(id).unwrap(), text);
        let o = svc.structure_edit(&req(None)).unwrap();
        let p::StructureEditOutcome::Applied { placement, .. } = o else {
            panic!("{o:?}")
        };
        assert_eq!(
            placement.as_deref(),
            Some("the copy was not moved: the preview has no proven size for the selected node")
        );
        assert_eq!(
            svc.text(id).unwrap(),
            "#VRML V2.0 utf8\nShape { geometry Box {} }\nShape { geometry Box {} }\n"
        );
    }

    /// P0-B: Duplicate moves the COPY beside the original by about the
    /// measured size, selects it, and is one undo step back to the exact
    /// bytes (CRLF, number spelling, outside USE untouched).
    #[test]
    fn duplicate_places_the_copy_beside_the_original() {
        let svc = Service::new(clock);
        let text = "#VRML V2.0 utf8\r\nDEF Car Transform {\r\n  translation 1.50 0 -2\r\n  children [ Shape { geometry Box { size 2 1 4 } } ]\r\n}\r\nTransform { children [ USE Car ] }\r\n";
        let id = svc.open_text_for_test(text);
        let b = text.find("DEF Car").unwrap();
        let e = text.find("}\r\nTransform").unwrap() + 1;
        let item = format!("node-{b}-{e}");
        let o = svc
            .structure_edit(&p::StructureEditRequest {
                session: id,
                base_revision: 0,
                item,
                op: p::StructureOp::Duplicate,
                confirm_shared: false,
                // A 4-unit object: 1.2 x 4 = 4.8, rounded up to 5.
                offset: Some(p::duplicate::Offset {
                    axis: 0,
                    step: p::duplicate::step(4.0).unwrap(),
                }),
            })
            .unwrap();
        let p::StructureEditOutcome::Applied { state, item, renamed, placement, .. } = o else {
            panic!("{o:?}")
        };
        let copy = "DEF Car_2 Transform {\r\n  translation 6.5 0 -2\r\n  children [ Shape { geometry Box { size 2 1 4 } } ]\r\n}";
        let want = text.replacen("}\r\nTransform", &format!("}}\r\n{copy}\r\nTransform"), 1);
        assert_eq!(svc.text(id).unwrap(), want);
        assert_eq!(renamed, vec![("Car".to_string(), "Car_2".to_string())]);
        assert_eq!(placement.as_deref(), Some("the copy's translation X is now 6.5"));
        let f = want.find(copy).unwrap();
        assert_eq!(item, Some(format!("node-{f}-{}", f + copy.len())));
        assert_eq!((state.revision, state.can_undo), (1, true));
        // The original and the outside USE are byte-identical.
        assert!(want.starts_with(&text[..e]));
        assert!(want.ends_with("Transform { children [ USE Car ] }\r\n"));
        // One undo step restores the exact bytes; redo re-applies them.
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), text);
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), want);
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
            confirm_shared: false,
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

    /// B1: a node in a PROTO body (the Chevy shape: gzip, `IS`-bound
    /// coordinates, two instances) is the shared definition. Without consent
    /// nothing changes; with it, exactly the edited tokens change, and Undo,
    /// Redo, Save and reopen are exact.
    #[test]
    fn b1_shared_proto_definition_edit_needs_consent_and_round_trips() {
        let dir = tmp("b1-proto");
        let src = "#VRML V2.0 utf8\nPROTO Car [ exposedField MFVec3f point [ 0 0 0, 1 0 0, 0 1 0 ] ]\n{ # PROTO body\n\tGroup { children [\n\t\tDEF Body Transform { children [\n\t\t\tShape {\n\t\t\t\tgeometry IndexedFaceSet { coord Coordinate { point IS point } coordIndex [ 0, 1, 2, -1 ] }\n\t\t\t\tappearance Appearance { material Material { diffuseColor 0.5490 0.3176 0.1765 } }\n\t\t\t}\n\t\t] }\n\t] }\n} # End PROTO body\nCar { }\nTransform { translation 3 0 0 children Car { } }\n";
        let path = dir.join("car.wrl");
        fs::write(&path, files::encode(src, Format::Gzip).unwrap()).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a = svc.analyze(doc.session).unwrap();
        let mat = a.items.iter().find(|i| i.label == "Material").unwrap();
        let nf = found(svc.inspect(doc.session, &mat.id, a.revision)).node.unwrap();
        assert!(nf.editable, "{}", nf.reason);
        let shared = nf.shared.clone().expect("shared definition");
        assert_eq!((shared.proto.as_str(), shared.instances), ("Car", 2));
        // The IS-bound Coordinate is not editable from the body.
        let coord = a.items.iter().find(|i| i.label == "Coordinate").unwrap();
        let cf = found(svc.inspect(doc.session, &coord.id, a.revision)).node.unwrap();
        assert!(cf.fields.iter().all(|f| !f.editable));
        // No consent: refused, nothing changes.
        let out = field_edit(&svc, doc.session, 0, &mat.id, (0, "diffuseColor"), &["0.1", "0.2", "0.9"]);
        assert!(matches!(
            &out,
            p::FieldEditOutcome::Refused { reason, .. } if reason == "shared-proto-definition-not-confirmed"
        ), "{out:?}");
        assert_eq!(svc.text(doc.session).unwrap(), src);
        // Consent: exactly the three tokens change.
        let req = |rev: u64, item: &str| p::FieldEditRequest {
            confirm_shared: true,
            session: doc.session,
            base_revision: rev,
            item: item.into(),
            field_index: 0,
            field_name: "diffuseColor".into(),
            components: ["0.1", "0.2", "0.9"]
                .iter()
                .map(|v| p::FieldInput::Text { value: v.to_string() })
                .collect(),
        };
        let p::FieldEditOutcome::Applied { item, .. } = svc.edit_field(&req(0, &mat.id)).unwrap() else {
            panic!("not applied")
        };
        let want = src.replace("diffuseColor 0.5490 0.3176 0.1765", "diffuseColor 0.1 0.2 0.9");
        assert_eq!(svc.text(doc.session).unwrap(), want);
        // A stale revision is refused even with consent.
        assert!(matches!(
            svc.edit_field(&req(0, &mat.id)).unwrap(),
            p::FieldEditOutcome::Refused { .. }
        ));
        // Undo / Redo exact.
        svc.history_with_item(doc.session, true, Some(&item)).unwrap();
        assert_eq!(svc.text(doc.session).unwrap(), src);
        svc.history_with_item(doc.session, false, Some(&mat.id)).unwrap();
        assert_eq!(svc.text(doc.session).unwrap(), want);
        // Save (gzip kept) and reopen.
        assert!(matches!(svc.save(doc.session), p::SaveOutcome::Saved { .. }));
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
        let again = opened(Service::new(clock).open_path(&path));
        let svc2 = Service::new(clock);
        let d2 = opened(svc2.open_path(&path));
        assert_eq!(svc2.text(d2.session).unwrap(), want);
        let _ = again;
        let _ = fs::remove_dir_all(&dir);
    }

    /// PR #146 review: Rust keeps no consent. Every shared-definition
    /// mutation needs `confirm_shared` in its OWN request: after an applied
    /// consented edit, the next request without it is refused; after the
    /// definition changed, still refused; a list edit is gated the same.
    #[test]
    fn shared_proto_consent_is_per_request_never_remembered() {
        let dir = tmp("pr146-consent");
        let src = "#VRML V2.0 utf8\nPROTO Car [ ]\n{\n  Transform { children [ Shape {\n    appearance Appearance { material Material { diffuseColor 0.5 0.5 0.5 } }\n    geometry Extrusion { crossSection [ 1 0, 0 1, -1 0, 1 0 ] spine [ 0 0 0, 0 1 0 ] }\n  } ] }\n}\nCar { }\nCar { }\n";
        let path = dir.join("car.wrl");
        fs::write(&path, src).unwrap();
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let req = |rev: u64, item: &str, yes: bool, v: [&str; 3]| p::FieldEditRequest {
            confirm_shared: yes,
            session: doc.session,
            base_revision: rev,
            item: item.into(),
            field_index: 0,
            field_name: "diffuseColor".into(),
            components: v.iter().map(|v| p::FieldInput::Text { value: v.to_string() }).collect(),
        };
        let material = |svc: &Service| {
            let a = svc.analyze(doc.session).unwrap();
            (a.items.iter().find(|i| i.label == "Material").unwrap().id.clone(), a.revision)
        };
        let refused = |o: p::FieldEditOutcome| {
            matches!(&o, p::FieldEditOutcome::Refused { reason, .. } if reason == "shared-proto-definition-not-confirmed")
        };
        // 1. A consented edit applies.
        let (m, rev) = material(&svc);
        assert!(matches!(svc.edit_field(&req(rev, &m, true, ["0.1", "0.1", "0.1"])).unwrap(), p::FieldEditOutcome::Applied { .. }));
        let after1 = svc.text(doc.session).unwrap();
        // 2. The NEXT edit of the same definition without consent: refused.
        let (m, rev) = material(&svc);
        assert!(refused(svc.edit_field(&req(rev, &m, false, ["0.2", "0.2", "0.2"])).unwrap()));
        assert_eq!(svc.text(doc.session).unwrap(), after1);
        // 3. The definition changes (Undo); still refused without consent.
        svc.history_with_item(doc.session, true, None).unwrap();
        assert_eq!(svc.text(doc.session).unwrap(), src);
        let (m, rev) = material(&svc);
        assert!(refused(svc.edit_field(&req(rev, &m, false, ["0.3", "0.3", "0.3"])).unwrap()));
        assert_eq!(svc.text(doc.session).unwrap(), src);
        // 4. A stale base revision with consent: refused, unchanged.
        let o = svc.edit_field(&req(rev + 7, &m, true, ["0.4", "0.4", "0.4"])).unwrap();
        assert!(matches!(o, p::FieldEditOutcome::Refused { .. }), "{o:?}");
        assert_eq!(svc.text(doc.session).unwrap(), src);
        // 5. A list edit of the shared Extrusion is gated the same way.
        let a = svc.analyze(doc.session).unwrap();
        let ex = a.items.iter().find(|i| i.label == "Extrusion").unwrap().id.clone();
        let ins = found(svc.inspect(doc.session, &ex, a.revision)).node.unwrap();
        let (fi, f) = ins.fields.iter().enumerate().find(|(_, f)| f.name == "spine").unwrap();
        let list = |yes: bool| p::ListEditRequest {
            confirm_shared: yes,
            session: doc.session,
            base_revision: a.revision,
            item: ex.clone(),
            field_index: fi as u32,
            field_name: f.name.clone(),
            expected_count: 2,
            ops: vec![p::ListOp::Delete { index: 1 }],
        };
        let o = svc.edit_list(&list(false)).unwrap();
        assert!(refused(o.clone()), "{o:?}");
        assert_eq!(svc.text(doc.session).unwrap(), src);
        let _ = fs::remove_dir_all(&dir);
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
        svc.create(&p::CreateRequest { at: None,
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
    /// The single differing region of `a` → `b`: (removed, inserted).
    fn diff(a: &str, b: &str) -> (String, String) {
        let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut pre = 0;
        while pre < x.len() && pre < y.len() && x[pre] == y[pre] {
            pre += 1;
        }
        let mut suf = 0;
        while suf < x.len() - pre
            && suf < y.len() - pre
            && x[x.len() - 1 - suf] == y[y.len() - 1 - suf]
        {
            suf += 1;
        }
        (
            x[pre..x.len() - suf].iter().collect(),
            y[pre..y.len() - suf].iter().collect(),
        )
    }

    fn list_edit(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        f: &p::ExtrusionField,
        ops: Vec<p::ListOp>,
    ) -> p::FieldEditOutcome {
        svc.edit_list(&p::ListEditRequest {
            confirm_shared: false,
            session: id,
            base_revision: rev,
            item: item.into(),
            field_index: f.field_index.expect("authored"),
            field_name: f.name.clone(),
            expected_count: f.tuples.as_ref().map(|t| t.len() as u32).unwrap_or(0),
            ops,
        })
        .unwrap()
    }

    fn applied(o: p::FieldEditOutcome) -> (p::DocState, String) {
        match o {
            p::FieldEditOutcome::Applied { state, item, .. } => (state, item),
            other => panic!("not applied: {other:?}"),
        }
    }

    /// The Extrusion item of the current revision and its Inspector view.
    fn extrusion(svc: &Service, id: u64) -> (u64, String, p::ExtrusionInfo) {
        let a = svc.analyze(id).unwrap();
        let item = a
            .items
            .iter()
            .find(|i| i.label.starts_with("Extrusion"))
            .expect("an Extrusion item")
            .id
            .clone();
        let ins = found(svc.inspect(id, &item, a.revision));
        let node = ins.node.expect("node fields");
        assert!(node.editable, "{}", node.reason);
        (a.revision, item, node.extrusion.expect("extrusion view"))
    }

    fn xf<'a>(e: &'a p::ExtrusionInfo, name: &str) -> &'a p::ExtrusionField {
        e.fields.iter().find(|f| f.name == name).unwrap()
    }

    /// VISUAL-4A: Create Extrusion → edit outline, spine, scale, orientation
    /// (each one undo step, each diff exactly the intended tokens) → stale
    /// refusal → Undo all → Redo all → Save As → reopen, in LF, CRLF and
    /// BOM + CRLF documents with comments, DEF/USE and a PROTO.
    #[test]
    fn extrusion_create_edit_undo_redo_save_reopen_is_exact() {
        let dir = tmp("visual4a");
        let bodies = [
            ("lf", "#VRML V2.0 utf8\n# keep me\nPROTO Keep [ field SFFloat k 1 ] { Group {} }\nDEF Ä Transform { translation 1 2 3 }\nTransform { children [ USE Ä ] }\nKeep { k 2 }\n"),
            ("crlf", "#VRML V2.0 utf8\r\n# keep me ✓\r\nDEF A Group {}\r\nGroup { children [ USE A ] }\r\n"),
            ("bom", "\u{FEFF}#VRML V2.0 utf8\r\n# Tëst 😀\r\nDEF A Group {}\r\n"),
        ];
        for (tag, src) in bodies {
            let path = dir.join(format!("{tag}.wrl"));
            fs::write(&path, src).unwrap();
            let svc = Service::new(clock);
            let doc = opened(svc.open_path(&path));
            let id = doc.session;
            let (st, _, name) = created(create(&svc, id, 0, p::Primitive::Extrusion));
            assert_eq!((st.revision, name.as_str()), (1, "Extrusion_1"));
            let after_create = svc.text(id).unwrap();
            assert!(after_create.starts_with(src));
            let eol = if tag == "lf" { "\n" } else { "\r\n" };
            assert!(after_create[src.len()..].contains(&format!("geometry Extrusion {{{eol}")));

            let (rev, item, e) = extrusion(&svc, id);
            assert_eq!(rev, 1);
            assert_eq!(xf(&e, "crossSection").tuples.as_ref().unwrap().len(), 7);
            assert!(e.outline_closed);
            assert!(!xf(&e, "scale").authored && !xf(&e, "solid").authored);
            assert_eq!(xf(&e, "solid").single.as_deref(), Some("TRUE"));
            assert_eq!(
                xf(&e, "scale").tuples,
                Some(vec![vec!["1".to_string(), "1".to_string()]])
            );

            // 1. One cross-section coordinate.
            let before = svc.text(id).unwrap();
            let (st, item) = applied(list_edit(
                &svc,
                id,
                rev,
                &item,
                xf(&e, "crossSection"),
                vec![p::ListOp::Set {
                    index: 1,
                    components: vec!["0.5".into(), "-0.9".into()],
                }],
            ));
            assert_eq!(st.revision, 2);
            assert_eq!(
                svc.text(id).unwrap(),
                before.replacen("0.5 -0.866,", "0.5 -0.9,", 1)
            );
            assert_eq!(
                diff(&before, &svc.text(id).unwrap()),
                ("866".into(), "9".into())
            );
            // 2. One spine coordinate.
            let (rev, _, e) = extrusion(&svc, id);
            let before = svc.text(id).unwrap();
            let (_, item) = applied(list_edit(
                &svc,
                id,
                rev,
                &item,
                xf(&e, "spine"),
                vec![p::ListOp::Set {
                    index: 1,
                    components: vec!["0".into(), "3".into(), "0".into()],
                }],
            ));
            assert_eq!(
                svc.text(id).unwrap(),
                before.replacen("spine [ 0 -1 0, 0 1 0 ]", "spine [ 0 -1 0, 0 3 0 ]", 1)
            );
            // 3. Scale: explicitly authored, then edited.
            let (rev, _, _) = extrusion(&svc, id);
            let before = svc.text(id).unwrap();
            let (_, item) = applied(
                svc.add_field(&p::AddFieldRequest {
            confirm_shared: false,
                    session: id,
                    base_revision: rev,
                    item: item.clone(),
                    field_name: "scale".into(),
                    value: p::AddFieldValue::List {
                        tuples: vec![vec!["1".into(), "1".into()], vec!["1".into(), "1".into()]],
                    },
                })
                .unwrap(),
            );
            assert_eq!(
                svc.text(id).unwrap(),
                before.replacen(
                    "0 3 0 ]",
                    &format!("0 3 0 ]{eol}        scale [ 1 1, 1 1 ]"),
                    1
                )
            );
            let (rev, _, e) = extrusion(&svc, id);
            let before = svc.text(id).unwrap();
            let (_, item) = applied(list_edit(
                &svc,
                id,
                rev,
                &item,
                xf(&e, "scale"),
                vec![p::ListOp::Set {
                    index: 1,
                    components: vec!["0.25".into(), "1".into()],
                }],
            ));
            assert_eq!(
                svc.text(id).unwrap(),
                before.replacen("scale [ 1 1, 1 1 ]", "scale [ 1 1, 0.25 1 ]", 1)
            );
            // 4. Orientation: authored, then edited.
            let (rev, _, _) = extrusion(&svc, id);
            let (_, item) = applied(
                svc.add_field(&p::AddFieldRequest {
            confirm_shared: false,
                    session: id,
                    base_revision: rev,
                    item: item.clone(),
                    field_name: "orientation".into(),
                    value: p::AddFieldValue::List {
                        tuples: vec![vec!["0".into(), "0".into(), "1".into(), "0".into()]],
                    },
                })
                .unwrap(),
            );
            let (rev, _, e) = extrusion(&svc, id);
            let before = svc.text(id).unwrap();
            let (_, item) = applied(list_edit(
                &svc,
                id,
                rev,
                &item,
                xf(&e, "orientation"),
                vec![p::ListOp::Set {
                    index: 0,
                    components: vec!["0".into(), "1".into(), "0".into(), "0.785398".into()],
                }],
            ));
            assert_eq!(
                svc.text(id).unwrap(),
                before.replacen(
                    "orientation [ 0 0 1 0 ]",
                    "orientation [ 0 1 0 0.785398 ]",
                    1
                )
            );
            // 5. An outline session: add, move, delete = ONE undo step.
            let (rev, _, e) = extrusion(&svc, id);
            let before = svc.text(id).unwrap();
            let (st, item) = applied(list_edit(
                &svc,
                id,
                rev,
                &item,
                xf(&e, "crossSection"),
                vec![
                    p::ListOp::Insert {
                        index: 2,
                        components: vec!["0".into(), "-1.2".into()],
                    },
                    p::ListOp::Set {
                        index: 2,
                        components: vec!["0".into(), "-1.3".into()],
                    },
                    p::ListOp::Delete { index: 4 },
                ],
            ));
            assert_eq!(st.revision, rev + 1);
            let (rev2, _, e2) = extrusion(&svc, id);
            assert_eq!(xf(&e2, "crossSection").tuples.as_ref().unwrap().len(), 7);
            svc.undo(id).unwrap();
            assert_eq!(svc.text(id).unwrap(), before);
            svc.redo(id).unwrap();
            // 6. Stale revision and stale count are refused; nothing changes.
            let now = svc.text(id).unwrap();
            assert!(matches!(
                list_edit(
                    &svc,
                    id,
                    rev,
                    &item,
                    xf(&e2, "spine"),
                    vec![p::ListOp::Delete { index: 0 }]
                ),
                p::FieldEditOutcome::Refused { .. }
            ));
            let mut f = xf(&e2, "spine").clone();
            f.tuples = Some(vec![]);
            assert!(matches!(
                list_edit(
                    &svc,
                    id,
                    rev2 + 1,
                    &item,
                    &f,
                    vec![p::ListOp::Delete { index: 0 }]
                ),
                p::FieldEditOutcome::Refused { .. }
            ));
            // 7. Malformed number and outline rules.
            assert!(matches!(
                list_edit(
                    &svc,
                    id,
                    rev2 + 1,
                    &item,
                    xf(&e2, "crossSection"),
                    vec![p::ListOp::Set {
                        index: 1,
                        components: vec!["1..0".into(), "0".into()]
                    }]
                ),
                p::FieldEditOutcome::Refused { .. }
            ));
            assert!(matches!(
                list_edit(
                    &svc,
                    id,
                    rev2 + 1,
                    &item,
                    xf(&e2, "crossSection"),
                    vec![p::ListOp::Delete { index: 0 }]
                ),
                p::FieldEditOutcome::Refused { .. }
            ));
            assert_eq!(svc.text(id).unwrap(), now);
            let final_text = now;
            // Undo everything (create + 7 edits) → the original bytes.
            let mut undos = 0;
            while matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }) {
                undos += 1;
            }
            assert_eq!((undos, svc.text(id).unwrap().as_str()), (8, src));
            while matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }) {}
            assert_eq!(svc.text(id).unwrap(), final_text);
            // Save As → reopen: the same bytes and the same Extrusion view.
            let out = dir.join(format!("{tag}-saved.wrl"));
            assert!(matches!(
                svc.save_as(id, &out),
                p::SaveOutcome::Saved { .. }
            ));
            assert_eq!(fs::read_to_string(&out).unwrap(), final_text);
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                src,
                "the original file is untouched"
            );
            let svc2 = Service::new(clock);
            let d2 = opened(svc2.open_path(&out));
            let (_, _, e3) = extrusion(&svc2, d2.session);
            let (_, _, e4) = extrusion(&svc, id);
            assert_eq!(e3, e4);
        }
    }

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
        // One undo step per create, back to the exact original.
        for _ in 0..p::Primitive::ALL.len() {
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
            tool: p::gizmo::Tool::Move,
        })
        .unwrap()
    }

    // ---- VISUAL-4B: rotate and scale ----------------------------------------

    fn target_for(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        tool: p::gizmo::Tool,
    ) -> p::TranslateTargetOutcome {
        svc.translate_target(&p::TranslateTargetRequest {
            session: id,
            revision: rev,
            item: item.into(),
            tool,
        })
        .unwrap()
    }

    /// A Ready target with its revision set to 0 (to compare two sessions).
    fn at_rev0(o: p::TranslateTargetOutcome) -> p::TranslateTargetOutcome {
        match o {
            p::TranslateTargetOutcome::Ready {
                item,
                def_name,
                root_index,
                preview_from,
                preview_to,
                translation,
                origin,
                rotation,
                scale,
                scale_orientation,
                center,
                pivot,
                shared,
                ..
            } => p::TranslateTargetOutcome::Ready {
                revision: 0,
                item,
                def_name,
                root_index,
                preview_from,
                preview_to,
                translation,
                origin,
                rotation,
                scale,
                scale_orientation,
                center,
                pivot,
                shared,
            },
            other => other,
        }
    }

    fn rot(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        axis: p::gizmo::Axis,
        d: f64,
    ) -> p::TranslateOutcome {
        svc.rotate(&p::RotateRequest { instance: None,
            confirm_shared: false,
            session: id,
            base_revision: rev,
            item: item.into(),
            axis,
            delta: d,
        })
        .unwrap()
    }

    fn scl(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        axis: Option<p::gizmo::Axis>,
        f: f64,
    ) -> p::TranslateOutcome {
        svc.scale(&p::ScaleRequest { instance: None,
            confirm_shared: false,
            session: id,
            base_revision: rev,
            item: item.into(),
            axis,
            factor: f,
        })
        .unwrap()
    }

    #[test]
    fn ui_preview_rotation_equals_the_committed_rule() {
        // The UI shows `protocol::gizmo::rotate_about` while dragging; Rust
        // commits `manipulate::rotate_about`. They must be the same rule.
        use p::gizmo::Axis as A;
        for old in [
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 2.0, -0.5, 0.8],
            [0.0, 0.0, -1.0, 3.0],
        ] {
            for (pa, ma) in [
                (A::X, manipulate::Axis::X),
                (A::Y, manipulate::Axis::Y),
                (A::Z, manipulate::Axis::Z),
            ] {
                for d in [-2.0, 0.3, 4.0] {
                    let a = p::gizmo::rotate_about(old, pa, d).unwrap();
                    let b = manipulate::rotate_about(old, ma, d).unwrap();
                    assert!(
                        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12),
                        "{a:?} {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rotate_scale_undo_redo_save_reopen_is_exact() {
        use p::gizmo::{Axis as A, Tool};
        let dir = tmp("visual4b");
        let bodies = [
            ("lf", "#VRML V2.0 utf8\n# keep me\nDEF Kept Group {}\n"),
            (
                "crlf",
                "#VRML V2.0 utf8\r\n# keep me ✓\r\nDEF Kept Group {}\r\n",
            ),
            (
                "bom",
                "\u{FEFF}#VRML V2.0 utf8\r\n# Tëst 😀\r\nDEF Kept Group {}\r\n",
            ),
        ];
        for (tag, src) in bodies {
            let path = dir.join(format!("{tag}.wrl"));
            fs::write(&path, src).unwrap();
            let svc = Service::new(clock);
            let id = opened(svc.open_path(&path)).session;
            let (st, item, _) = created(create(&svc, id, 0, p::Primitive::Box));
            let created_text = svc.text(id).unwrap();
            let mut rev = st.revision;
            let mut item = item;
            let p::TranslateTargetOutcome::Ready {
                rotation,
                scale,
                pivot,
                ..
            } = target_for(&svc, id, rev, &item, Tool::Rotate)
            else {
                panic!("not a rotate target")
            };
            assert_eq!(
                (rotation, scale, pivot),
                ([0.0, 1.0, 0.0, 0.0], [1.0; 3], [0.0; 3])
            );
            let mut texts = vec![svc.text(id).unwrap()];
            type Step = fn(&Service, u64, u64, &str) -> p::TranslateOutcome;
            let steps: [Step; 7] = [
                |s, id, r, i| rot(s, id, r, i, A::Y, 0.5),
                |s, id, r, i| rot(s, id, r, i, A::X, 0.25),
                |s, id, r, i| rot(s, id, r, i, A::Z, -1.0),
                |s, id, r, i| scl(s, id, r, i, Some(A::X), 2.0),
                |s, id, r, i| scl(s, id, r, i, Some(A::Y), 0.5),
                |s, id, r, i| scl(s, id, r, i, Some(A::Z), 3.0),
                |s, id, r, i| scl(s, id, r, i, None, 1.5),
            ];
            for step in steps {
                let (st, it, _) = moved(step(&svc, id, rev, &item));
                assert_eq!(st.revision, rev + 1, "one revision per drag");
                rev = st.revision;
                item = it;
                texts.push(svc.text(id).unwrap());
            }
            let fin = svc.text(id).unwrap();
            assert!(fin.contains("scale 3 0.75 4.5"), "{fin}");
            assert!(fin.contains("translation 0 0 0"));
            // Only the Box's rotation and scale lines changed.
            let changed: Vec<_> = created_text
                .split_inclusive('\n')
                .zip(fin.split_inclusive('\n'))
                .filter(|(a, b)| a != b)
                .map(|(a, _)| a.trim().split(' ').next().unwrap().to_string())
                .collect();
            assert_eq!(changed, ["rotation", "scale"]);
            // Stale revision: refused, nothing changes.
            assert!(matches!(
                rot(&svc, id, rev - 1, &item, A::X, 1.0),
                p::TranslateOutcome::Refused { .. }
            ));
            assert!(matches!(
                scl(&svc, id, rev - 1, &item, None, 2.0),
                p::TranslateOutcome::Refused { .. }
            ));
            assert_eq!(svc.text(id).unwrap(), fin);
            // One undo step per drag, each restoring the exact previous text.
            for want in texts.iter().rev().skip(1) {
                svc.undo(id).unwrap();
                assert_eq!(&svc.text(id).unwrap(), want);
            }
            svc.undo(id).unwrap();
            assert_eq!(svc.text(id).unwrap(), src);
            svc.redo(id).unwrap();
            for want in texts.iter().skip(1) {
                svc.redo(id).unwrap();
                assert_eq!(&svc.text(id).unwrap(), want);
            }
            // Save As → reopen: the same bytes and the same frame.
            let out = dir.join(format!("{tag}-saved.wrl"));
            assert!(matches!(
                svc.save_as(id, &out),
                p::SaveOutcome::Saved { .. }
            ));
            assert_eq!(fs::read_to_string(&out).unwrap(), fin);
            assert_eq!(fs::read_to_string(&path).unwrap(), src);
            let svc2 = Service::new(clock);
            let d2 = opened(svc2.open_path(&out));
            assert_eq!(
                target_for(&svc2, d2.session, 0, &item, Tool::Scale),
                at_rev0(target_for(
                    &svc,
                    id,
                    svc.snapshot(id).unwrap().revision,
                    &item,
                    Tool::Scale
                ))
            );
        }
    }

    #[test]
    fn rotate_scale_refusals_change_nothing() {
        use p::gizmo::{Axis as A, Tool};
        let dir = tmp("visual4b-refuse");
        let src = "#VRML V2.0 utf8\nDEF A Transform { translation 1 0 0 }\nDEF B Transform { rotation 0 1 0 0 scale 1 -1 1 }\nDEF C Transform { rotation 0 1 0 0 scale 1 1 1 }\nGroup { children USE C }\nTransform { children DEF N Transform { rotation 0 0 1 0 scale 1 1 1 } }\n";
        let path = dir.join("r.wrl");
        fs::write(&path, src).unwrap();
        let svc = Service::new(clock);
        let id = opened(svc.open_path(&path)).session;
        // Item ids are exact UTF-16 node spans.
        let item = |def: &str| {
            let at = src.find(&format!("DEF {def} Transform")).unwrap();
            let mut depth = 0;
            let mut end = at;
            for (i, c) in src[at..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = at + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            format!("node-{at}-{end}")
        };
        let refused = |o: p::TranslateTargetOutcome| match o {
            p::TranslateTargetOutcome::Refused { reason, .. } => reason,
            other => panic!("not refused: {other:?}"),
        };
        let t = |d: &str, tool| target_for(&svc, id, 0, &item(d), tool);
        assert_eq!(
            refused(t("A", Tool::Rotate)),
            "rotation-not-explicitly-authored"
        );
        assert_eq!(
            refused(t("A", Tool::Scale)),
            "scale-not-explicitly-authored"
        );
        assert_eq!(refused(t("B", Tool::Scale)), "scale-not-positive");
        assert!(matches!(
            t("B", Tool::Rotate),
            p::TranslateTargetOutcome::Ready { .. }
        ));
        assert_eq!(refused(t("C", Tool::Rotate)), "transform-is-used-elsewhere");
        assert_eq!(refused(t("N", Tool::Scale)), "transform-is-not-top-level");
        for o in [
            rot(&svc, id, 0, &item("A"), A::X, 1.0),
            scl(&svc, id, 0, &item("B"), None, 2.0),
            rot(&svc, id, 0, &item("C"), A::Y, 1.0),
            scl(&svc, id, 0, &item("N"), Some(A::X), 2.0),
            rot(&svc, id, 0, "node-1-2", A::X, 1.0),
        ] {
            assert!(matches!(o, p::TranslateOutcome::Refused { .. }), "{o:?}");
        }
        assert!(matches!(
            target_for(&svc, id, 5, &item("A"), Tool::Rotate),
            p::TranslateTargetOutcome::Stale { current: 0 }
        ));
        assert_eq!(svc.text(id).unwrap(), src);
        assert_eq!(svc.snapshot(id).unwrap().revision, 0);
        // The explicit authoring workflow: write rotation, then rotate.
        let (st, it) = applied(
            svc.add_field(&p::AddFieldRequest {
            confirm_shared: false,
                session: id,
                base_revision: 0,
                item: item("A"),
                field_name: "rotation".into(),
                value: p::AddFieldValue::Tuple {
                    values: vec!["0".into(), "0".into(), "1".into(), "0".into()],
                },
            })
            .unwrap(),
        );
        assert_eq!(
            svc.text(id).unwrap(),
            src.replacen(
                "translation 1 0 0 }",
                "translation 1 0 0 rotation 0 0 1 0 }",
                1
            )
        );
        let (_, _, text) = moved(rot(&svc, id, st.revision, &it, A::Z, 0.5));
        assert_eq!(text, "0 0 1 0.5");
    }

    fn mv(
        svc: &Service,
        id: u64,
        rev: u64,
        item: &str,
        axis: p::gizmo::Axis,
        v: f64,
    ) -> p::TranslateOutcome {
        svc.translate(&p::TranslateRequest { instance: None,
            confirm_shared: false,
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

    /// B3: a Transform in a shared PROTO body. The target is Ready (no root
    /// index, the shared definition named); the runtime proof accepts the
    /// PICKED instance's copy only when its chain is the Transform, neutral
    /// body groups and a TOP-LEVEL instance; a commit is refused without
    /// consent and with it writes exactly one token of the body, as one
    /// undo step.
    #[test]
    fn gizmo_on_a_shared_proto_body_transform_needs_consent_and_one_token() {
        use p::gizmo::Axis as A;
        let dir = tmp("b3-proto-gizmo");
        let path = dir.join("car.wrl");
        let xf = "DEF CarXForm Transform { translation 0 0 0 rotation 0 1 0 0 scale 1 1 1 children [ Shape { geometry Box { } } ] }";
        let group = format!("Group {{\n\t\tbboxSize IS bboxSize\n\t\tchildren [\n\t\t\t{xf}\n\t\t]\n\t}}");
        let inst = "Car { }";
        let text = format!(
            "#VRML V2.0 utf8\nPROTO Car [ field SFVec3f bboxSize 1 1 1 ] {{\n\t{group}\n}}\n{inst}\nTransform {{ translation 5 0 0 children Car {{ }} }}\n"
        );
        fs::write(&path, &text).unwrap();
        let (o, svc) = svc_open(&path);
        let doc = opened(o);
        let id = doc.session;
        let rev0 = doc.revision;
        let u16at = |b: usize| text[..b].encode_utf16().count() as u64;
        let span_of = |needle: &str| {
            let b = text.find(needle).unwrap();
            (u16at(b), u16at(b + needle.len()))
        };
        let (xs, gs, is) = (span_of(xf), span_of(&group), span_of(inst));
        let item = format!("node-{}-{}", xs.0, xs.1);
        match target(&svc, id, rev0, &item) {
            p::TranslateTargetOutcome::Ready {
                root_index,
                shared,
                def_name,
                ..
            } => {
                assert_eq!(root_index, None);
                assert_eq!(def_name.as_deref(), Some("CarXForm"));
                let sh = shared.expect("shared definition");
                assert_eq!((sh.proto.as_str(), sh.instances), ("Car", 2));
            }
            o => panic!("{o:?}"),
        }
        // The runtime chain of one instance's copy, as snapshotBodySpan
        // reports it: Transform -> Group -> BODY:instance -> SCENE.
        let node = |ty: &str, ctx: &str, sp: (u64, u64), parents: &[&str]| p::PickGraphNode {
            type_name: Some(ty.into()),
            ctx_kind: ctx.into(),
            occurrences: vec![p::PickSpan { start: sp.0, end: sp.1 }],
            parents: parents.iter().map(|s| s.to_string()).collect(),
        };
        let body_snap = |instance_parent: &str, extra: Option<p::PickGraphNode>| {
            let mut graph = std::collections::BTreeMap::new();
            graph.insert("n1".to_string(), node("Transform", "proto-body", xs, &["n2"]));
            graph.insert("n2".to_string(), node("Group", "proto-body", gs, &["BODY:n3"]));
            graph.insert("n3".to_string(), node("Car", "document", is, &[instance_parent]));
            if let Some(e) = extra {
                graph.insert("n4".to_string(), e);
            }
            p::PickSnapshot {
                outcome: "found".into(),
                shape: Some("n1".into()),
                ctx_kind: Some("proto-body".into()),
                graph,
                ..Default::default()
            }
        };
        let hash = p::preview_hash(&text);
        let prove = |snap: p::PickSnapshot| {
            svc.translate_prove(&p::TranslateProveRequest { instance: None,
                session: id,
                revision: rev0,
                preview_hash: hash,
                generation: 1,
                item: item.clone(),
                snapshot: snap,
                tool: p::gizmo::Tool::Move,
            })
            .unwrap()
        };
        match prove(body_snap("SCENE", None)) {
            p::TranslateProveOutcome::Proven {
                instance,
                parent_frame,
                ..
            } => {
                // The top-level instance: the world frame, the instance named.
                assert_eq!(parent_frame, None);
                assert_eq!(
                    instance.map(|i| i.item),
                    Some(format!("node-{}-{}", is.0, is.1))
                );
            }
            o => panic!("{o:?}"),
        }
        // LV2: the instance nested in a Transform: proven through that
        // Transform, whose SOURCE frame (translation 5 0 0) is the parent.
        let nested = text.rfind("Transform {").unwrap();
        let nested_end = text.rfind('}').unwrap() + 1;
        let ns = (u16at(nested), u16at(nested_end));
        let inner = text[..nested_end].rfind("Car { }").unwrap();
        let mut snap = body_snap("n4", Some(node("Transform", "document", ns, &["SCENE"])));
        if let Some(n3) = snap.graph.get_mut("n3") {
            n3.occurrences = vec![p::PickSpan {
                start: u16at(inner),
                end: u16at(inner + inst.len()),
            }];
        }
        match prove(snap.clone()) {
            p::TranslateProveOutcome::Proven {
                instance,
                parent_frame,
                ..
            } => {
                let m = parent_frame.expect("a parent frame");
                assert_eq!(&m[12..15], &[5.0, 0.0, 0.0]);
                assert_eq!(
                    instance.map(|i| (i.preview_from, i.preview_to)),
                    Some((u16at(inner), u16at(inner + inst.len())))
                );
            }
            o => panic!("{o:?}"),
        }
        // A continuation naming the OTHER instance is refused.
        match svc
            .translate_prove(&p::TranslateProveRequest {
                instance: Some(format!("node-{}-{}", is.0, is.1)),
                session: id,
                revision: rev0,
                preview_hash: hash,
                generation: 1,
                item: item.clone(),
                snapshot: snap,
                tool: p::gizmo::Tool::Move,
            })
            .unwrap()
        {
            p::TranslateProveOutcome::Refused { reason, .. } => {
                assert_eq!(reason, "proto-instance-changed")
            }
            o => panic!("{o:?}"),
        }
        // The selection highlight of the body Shape: located as the picked
        // instance's copy; the proven chain marks the instance link (no
        // frame of its own), so the outline composes the body frames only.
        let shp = span_of("Shape { geometry Box { } }");
        let sitem = format!("node-{}-{}", shp.0, shp.1);
        match svc
            .highlight_target(&p::HighlightTargetRequest {
                session: id,
                revision: rev0,
                item: sitem.clone(),
            })
            .unwrap()
        {
            p::HighlightTargetOutcome::Ready { body, .. } => assert!(body),
            o => panic!("{o:?}"),
        }
        let mut hs = body_snap("SCENE", None);
        hs.graph.insert("n0".to_string(), node("Shape", "proto-body", shp, &["n1"]));
        hs.shape = Some("n0".into());
        match svc
            .highlight_prove(&p::HighlightProveRequest {
                session: id,
                revision: rev0,
                preview_hash: hash,
                generation: 1,
                item: sitem,
                snapshot: hs,
            })
            .unwrap()
        {
            p::HighlightProveOutcome::Proven { ancestors, .. } => {
                let got: Vec<(&str, bool)> = ancestors
                    .iter()
                    .map(|a| (a.node_type.as_str(), a.proto_instance))
                    .collect();
                assert_eq!(got, [("Transform", false), ("Group", false), ("Car", true)]);
            }
            o => panic!("{o:?}"),
        }
        // No consent: nothing changes.
        match mv(&svc, id, rev0, &item, A::X, 1.5) {
            p::TranslateOutcome::Refused { reason, .. } => assert_eq!(
                reason,
                field_edit::reason::SHARED_DEFINITION_UNCONFIRMED
            ),
            o => panic!("{o:?}"),
        }
        assert_eq!(svc.text(id).unwrap(), text);
        // Consent: one token of the PROTO body, one undo step.
        let (st, it, written) = moved(
            svc.translate(&p::TranslateRequest { instance: None,
                session: id,
                base_revision: rev0,
                item: item.clone(),
                axis: A::X,
                value: 1.5,
                decimals: 3,
                confirm_shared: true,
            })
            .unwrap(),
        );
        assert_eq!(written, "1.5");
        let after = text.replacen(
            "CarXForm Transform { translation 0 0 0",
            "CarXForm Transform { translation 1.5 0 0",
            1,
        );
        assert_eq!(svc.text(id).unwrap(), after);
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), text);
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), after);
        // Rotate and scale go the same way.
        let rev = svc.text_at(id).unwrap().1;
        assert!(matches!(
            rot(&svc, id, rev, &it, A::Y, 0.5),
            p::TranslateOutcome::Refused { .. }
        ));
        let r = svc
            .rotate(&p::RotateRequest { instance: None,
                session: id,
                base_revision: rev,
                item: it.clone(),
                axis: A::Y,
                delta: 0.5,
                confirm_shared: true,
            })
            .unwrap();
        let (st2, it2, _) = moved(r);
        assert!(svc.text(id).unwrap().contains("rotation 0 1 0 0.5 scale 1 1 1"));
        let r = svc
            .scale(&p::ScaleRequest { instance: None,
                session: id,
                base_revision: st2.revision,
                item: it2,
                axis: None,
                factor: 2.0,
                confirm_shared: true,
            })
            .unwrap();
        moved(r);
        assert!(svc.text(id).unwrap().contains("scale 2 2 2 children"));
        let _ = st;
    }

    /// B3 (LV2): continuation. A commit naming the gizmo's PROTO instance
    /// carries it into the new revision through the exact changes (Tier-1):
    /// the same bytes, the same PROTO, at the mapped span (shifted by the
    /// edit before it). A span that is not
    /// an instance of that PROTO is reported lost, never re-targeted.
    #[test]
    fn a_shared_body_commit_carries_the_gizmo_instance_or_says_it_is_lost() {
        use p::gizmo::Axis as A;
        let dir = tmp("b3-proto-carry");
        let path = dir.join("car.wrl");
        let xf = "DEF CarXForm Transform { translation 0 0 0 rotation 0 1 0 0 scale 1 1 1 children [ DEF S Shape { geometry Box { } } USE S ] }";
        let text = format!(
            "#VRML V2.0 utf8\nPROTO Car [ ] {{\n\tGroup {{ children [ {xf} ] }}\n}}\nCar {{ }} # first\nTransform {{ translation 5 0 0 children Car {{ }} }}\n"
        );
        fs::write(&path, &text).unwrap();
        let (o, svc) = svc_open(&path);
        let doc = opened(o);
        let id = doc.session;
        let u16at = |t: &str, b: usize| t[..b].encode_utf16().count() as u64;
        let xb = text.find(xf).unwrap();
        let item = format!("node-{}-{}", u16at(&text, xb), u16at(&text, xb + xf.len()));
        let ib = text.rfind("Car { }").unwrap();
        let inst = format!("node-{}-{}", u16at(&text, ib), u16at(&text, ib + 7));
        let commit = |rev: u64, item: &str, instance: Option<String>, value: f64| {
            svc.translate(&p::TranslateRequest {
                session: id,
                base_revision: rev,
                item: item.into(),
                axis: A::X,
                value,
                decimals: 3,
                confirm_shared: true,
                instance,
            })
            .unwrap()
        };
        let (rev1, item1, carried, lost) = match commit(doc.revision, &item, Some(inst.clone()), 12.25) {
            p::TranslateOutcome::Applied {
                state,
                item,
                instance,
                instance_lost,
                ..
            } => (state.revision, item, instance, instance_lost),
            o => panic!("{o:?}"),
        };
        let after = svc.text(id).unwrap();
        // "0" -> "12.25": the later instance shifts by 4 UTF-16 units.
        let nb = after.rfind("Car { }").unwrap();
        let want = format!("node-{}-{}", u16at(&after, nb), u16at(&after, nb + 7));
        assert_eq!(lost, None);
        let c = carried.expect("carried");
        assert_eq!(c.item, want);
        assert_ne!(c.item, inst);
        assert_eq!((c.preview_from, c.preview_to), (u16at(&after, nb), u16at(&after, nb + 7)));
        // The other instance is NOT what was carried.
        let other = after.find("Car { } # first").unwrap();
        assert_ne!(c.item, format!("node-{}-{}", u16at(&after, other), u16at(&after, other + 7)));
        // Not an instance of the PROTO (the outer Transform): lost.
        let tb = after.rfind("Transform {").unwrap();
        let te = after.rfind('}').unwrap() + 1;
        let outer = format!("node-{}-{}", u16at(&after, tb), u16at(&after, te));
        match commit(rev1, &item1, Some(outer), 3.5) {
            p::TranslateOutcome::Applied {
                instance,
                instance_lost,
                ..
            } => {
                assert_eq!(instance, None);
                assert_eq!(instance_lost.as_deref(), Some("instance-not-proven-in-old-revision"));
            }
            o => panic!("{o:?}"),
        }
        // No instance asked: none reported.
        let rev2 = svc.text_at(id).unwrap().1;
        let item2 = {
            let t = svc.text(id).unwrap();
            let b = t.find("DEF CarXForm").unwrap();
            let e = b + t[b..].find(" USE S ] }").unwrap() + " USE S ] }".len();
            format!("node-{}-{}", u16at(&t, b), u16at(&t, e))
        };
        match commit(rev2, &item2, None, 7.0) {
            p::TranslateOutcome::Applied {
                instance,
                instance_lost,
                ..
            } => assert_eq!((instance, instance_lost), (None, None)),
            o => panic!("{o:?}"),
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
            svc.translate_prove(&p::TranslateProveRequest { instance: None,
                session: id,
                revision: rev,
                preview_hash: hash,
                generation: 1,
                item: format!("node-{f}-{t}"),
                snapshot: snap,
                tool: p::gizmo::Tool::Move,
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

    /// TEXTURE-LOCAL-1: the resource token is per generation and per open
    /// session; the folder follows Save As; preview text and source are
    /// untouched; untitled documents get no folder.
    #[test]
    fn preview_resources_follow_the_session_and_never_touch_the_source() {
        use crate::resources::{self, Refusal};
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        let a = tmp("res-a");
        let b = tmp("res-b");
        let text = "#VRML V2.0 utf8\nShape { appearance Appearance { texture ImageTexture { url \"t.png\" } } geometry Box { } }\n";
        fs::write(a.join("w.wrl"), text).unwrap();
        fs::write(a.join("t.png"), PNG).unwrap();
        let bytes = fs::read(a.join("w.wrl")).unwrap();
        let svc = Service::new(clock);
        let id = opened(svc.open_path(&a.join("w.wrl"))).session;
        let token = |src: &p::PreviewSource| {
            src.resource_base
                .strip_prefix("wrlres://localhost/")
                .and_then(|r| r.strip_suffix('/'))
                .unwrap()
                .to_string()
        };
        let s1 = svc.preview_source(id).unwrap();
        assert_eq!(s1.text, text, "the preview text is the source text");
        assert!(s1.texture_warnings.is_empty(), "{:?}", s1.texture_warnings);
        let t1 = token(&s1);
        assert_eq!(svc.resource_folder(&t1).unwrap().1, a);
        let served = resources::serve(&svc, &format!("/{t1}/t.png"));
        assert_eq!((served.status, served.mime), (200, "image/png"));
        assert_eq!(served.body, PNG);
        // A newer generation retires the old token.
        let t2 = token(&svc.preview_source(id).unwrap());
        assert_ne!(t1, t2);
        assert_eq!(svc.resource_folder(&t1).unwrap_err(), Refusal::Stale);
        assert_eq!(resources::serve(&svc, &format!("/{t1}/t.png")).status, 403);
        // Save As moves later loads to the new folder (no texture there).
        match svc.save_as(id, &b.join("moved.wrl")) {
            p::SaveOutcome::Saved { .. } => {}
            o => panic!("{o:?}"),
        }
        assert_eq!(svc.resource_folder(&t2).unwrap().1, b);
        assert_eq!(resources::serve(&svc, &format!("/{t2}/t.png")).status, 404);
        let moved = svc.preview_source(id).unwrap();
        assert_eq!(moved.texture_warnings.len(), 1);
        fs::write(b.join("t.png"), PNG).unwrap();
        let t3 = token(&svc.preview_source(id).unwrap());
        assert_eq!(resources::serve(&svc, &format!("/{t3}/t.png")).status, 200);
        // The source and its history are unchanged by any of it.
        let info = svc.snapshot(id).unwrap();
        assert_eq!((info.revision, info.dirty, info.can_undo), (0, false, false));
        assert_eq!(svc.text(id).unwrap(), text);
        assert_eq!(fs::read(a.join("w.wrl")).unwrap(), bytes);
        // Closing the session revokes its token.
        svc.close(id).unwrap();
        assert_eq!(resources::serve(&svc, &format!("/{t3}/t.png")).status, 403);
        // An untitled world has no folder: its base is never valid.
        let nw = opened(svc.new_world()).session;
        let src = svc.preview_source(nw).unwrap();
        assert_eq!(src.resource_base, resources::base_url(resources::NO_FOLDER_TOKEN));
        assert_eq!(resources::serve(&svc, "/none/t.png").status, 403);
        fs::remove_dir_all(&a).unwrap();
        fs::remove_dir_all(&b).unwrap();
    }

    fn svc_open(path: &Path) -> (p::OpenOutcome, Service) {
        let svc = Service::new(clock);
        (svc.open_path(path), svc)
    }

    /// B2 selection highlight: a nested Shape proves through its exact
    /// runtime chain and reports its ancestors (direct parent first, preview
    /// spans); the twin's node, a stale text, a Billboard ancestor and a
    /// non-node item all refuse. Nothing is written.
    #[test]
    fn highlight_proves_nested_nodes_and_refuses_everything_else() {
        let dir = tmp("b2-highlight");
        let path = dir.join("nested.wrl");
        let shape = "Shape { geometry Box { } }";
        let t = format!("Transform {{ translation 1 0 0 children [ {shape} ] }}");
        let bb = format!("Billboard {{ children [ {shape} ] }}");
        let text = format!("\u{FEFF}#VRML V2.0 utf8\n{t}\n{t}\n{bb}\n");
        fs::write(&path, &text).unwrap();
        let (o, svc) = svc_open(&path);
        let doc = opened(o);
        let (id, rev) = (doc.session, doc.revision);
        let u16at = |b: usize| text[..b].encode_utf16().count() as u64;
        let span = |b: usize, len: usize| (u16at(b), u16at(b + len));
        let t1 = text.find(&t).unwrap();
        let t2 = text.rfind(&t).unwrap();
        let (tr1, tr2) = (span(t1, t.len()), span(t2, t.len()));
        let sh1 = span(t1 + t.find(shape).unwrap(), shape.len());
        let sh2 = span(t2 + t.find(shape).unwrap(), shape.len());
        let b0 = text.find(&bb).unwrap();
        let bspan = span(b0, bb.len());
        let shb = span(b0 + bb.find(shape).unwrap(), shape.len());
        let hash = p::preview_hash(text.strip_prefix('\u{FEFF}').unwrap());
        let pv = |(f, t): (u64, u64)| (f - 1, t - 1);
        // A located Shape under a located parent under the scene.
        let chain = |me: (u64, u64), parent: (u64, u64), parent_ty: &str| {
            let mut s = located("Shape", pv(me), &["n2"]);
            s.graph.insert(
                "n2".into(),
                p::PickGraphNode {
                    type_name: Some(parent_ty.into()),
                    ctx_kind: "document".into(),
                    occurrences: vec![p::PickSpan {
                        start: pv(parent).0,
                        end: pv(parent).1,
                    }],
                    parents: vec!["SCENE".into()],
                },
            );
            s
        };
        let prove = |rev: u64, hash: u64, item: &str, snap: p::PickSnapshot| {
            svc.highlight_prove(&p::HighlightProveRequest {
                session: id,
                revision: rev,
                preview_hash: hash,
                generation: 1,
                item: item.into(),
                snapshot: snap,
            })
            .unwrap()
        };
        let item = |(f, t): (u64, u64)| format!("node-{f}-{t}");
        // The target is the exact preview span (BOM projected).
        match svc
            .highlight_target(&p::HighlightTargetRequest {
                session: id,
                revision: rev,
                item: item(sh1),
            })
            .unwrap()
        {
            p::HighlightTargetOutcome::Ready {
                preview_from,
                preview_to,
                ..
            } => assert_eq!((preview_from, preview_to), pv(sh1)),
            o => panic!("{o:?}"),
        }
        assert!(matches!(
            svc.highlight_target(&p::HighlightTargetRequest {
                session: id,
                revision: rev + 1,
                item: item(sh1),
            })
            .unwrap(),
            p::HighlightTargetOutcome::Stale { .. }
        ));
        assert!(matches!(
            svc.highlight_target(&p::HighlightTargetRequest {
                session: id,
                revision: rev,
                item: "use-1-2".into(),
            })
            .unwrap(),
            p::HighlightTargetOutcome::Refused { .. }
        ));
        for (me, parent) in [(sh1, tr1), (sh2, tr2)] {
            match prove(rev, hash, &item(me), chain(me, parent, "Transform")) {
                p::HighlightProveOutcome::Proven {
                    node_type,
                    ancestors,
                    ..
                } => {
                    assert_eq!(node_type, "Shape");
                    assert_eq!(
                        ancestors,
                        vec![p::HighlightAncestor {
                            preview_from: pv(parent).0,
                            preview_to: pv(parent).1,
                            node_type: "Transform".into(),
                            proto_instance: false,
                        }]
                    );
                }
                o => panic!("{o:?}"),
            }
        }
        // The twin's runtime node offered for this item never proves.
        assert!(matches!(
            prove(rev, hash, &item(sh1), chain(sh2, tr2, "Transform")),
            p::HighlightProveOutcome::Refused { .. }
        ));
        // A top-level node proves with no ancestors.
        assert!(matches!(
            prove(rev, hash, &item(tr1), located("Transform", pv(tr1), &["SCENE"])),
            p::HighlightProveOutcome::Proven { ancestors, .. } if ancestors.is_empty()
        ));
        // Stale text.
        assert!(matches!(
            prove(rev, hash ^ 1, &item(sh1), chain(sh1, tr1, "Transform")),
            p::HighlightProveOutcome::Stale { .. }
        ));
        // A Billboard ancestor: proven chain, unknown frame -> refused.
        assert!(matches!(
            prove(rev, hash, &item(shb), chain(shb, bspan, "Billboard")),
            p::HighlightProveOutcome::Refused { reason, .. } if reason == p::highlight::refusal::ANCESTOR
        ));
        // No unique runtime node for the span (e.g. a PROTO body node).
        let mut none = chain(sh1, tr1, "Transform");
        none.outcome = "unsupported".into();
        none.reason = Some("runtime-node-for-span-not-unique".into());
        assert!(matches!(
            prove(rev, hash, &item(sh1), none),
            p::HighlightProveOutcome::Refused { reason, .. } if reason == "runtime-node-for-span-not-unique"
        ));
        assert_eq!(svc.text_at(id).unwrap(), (text.clone(), rev), "read-only");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// LV2: a non-geometry node (Material) proves through its chain and is
    /// outlined as its OWNING Shape (the ancestors start above the Shape);
    /// one with no Shape in its chain refuses with that reason, not as a
    /// Billboard; a node that is never drawn refuses at the target with
    /// its own reason. Nothing is written.
    #[test]
    fn highlight_outlines_the_owning_shape_of_a_material_and_names_true_reasons() {
        let dir = tmp("lv2-highlight-owner");
        let path = dir.join("owner.wrl");
        let mat = "Material { diffuseColor 1 0 0 }";
        let app = format!("Appearance {{ material {mat} }}");
        let shape = format!("Shape {{ appearance {app} geometry Box {{ }} }}");
        let t = format!("Transform {{ children [ {shape} ] }}");
        let vp = "Viewpoint { position 0 0 10 }";
        let lone = "Material { }";
        let text = format!("#VRML V2.0 utf8\n{t}\n{vp}\n{lone}\n");
        fs::write(&path, &text).unwrap();
        let (o, svc) = svc_open(&path);
        let doc = opened(o);
        let (id, rev) = (doc.session, doc.revision);
        let span = |b: usize, len: usize| (b as u64, (b + len) as u64);
        let t0 = text.find(&t).unwrap();
        let sh0 = t0 + t.find(&shape).unwrap();
        let ap0 = sh0 + shape.find(&app).unwrap();
        let m0 = ap0 + app.find(mat).unwrap();
        let (tsp, shsp, apsp, msp) = (
            span(t0, t.len()),
            span(sh0, shape.len()),
            span(ap0, app.len()),
            span(m0, mat.len()),
        );
        let vpsp = span(text.find(vp).unwrap(), vp.len());
        let lonesp = span(text.rfind(lone).unwrap(), lone.len());
        let hash = p::preview_hash(&text);
        let item = |(f, t): (u64, u64)| format!("node-{f}-{t}");
        let node = |ty: &str, sp: (u64, u64), parents: &[&str]| p::PickGraphNode {
            type_name: Some(ty.into()),
            ctx_kind: "document".into(),
            occurrences: vec![p::PickSpan { start: sp.0, end: sp.1 }],
            parents: parents.iter().map(|s| s.to_string()).collect(),
        };
        let mut snap = located("Material", msp, &["n2"]);
        snap.graph.insert("n2".into(), node("Appearance", apsp, &["n3"]));
        snap.graph.insert("n3".into(), node("Shape", shsp, &["n4"]));
        snap.graph.insert("n4".into(), node("Transform", tsp, &["SCENE"]));
        let prove = |it: &str, snap: p::PickSnapshot| {
            svc.highlight_prove(&p::HighlightProveRequest {
                session: id,
                revision: rev,
                preview_hash: hash,
                generation: 1,
                item: it.into(),
                snapshot: snap,
            })
            .unwrap()
        };
        let anc = |sp: (u64, u64), ty: &str| p::HighlightAncestor {
            preview_from: sp.0,
            preview_to: sp.1,
            node_type: ty.into(),
            proto_instance: false,
        };
        match prove(&item(msp), snap) {
            p::HighlightProveOutcome::Proven {
                node_type,
                ancestors,
                owner,
                ..
            } => {
                assert_eq!(node_type, "Material");
                assert_eq!(owner, Some(anc(shsp, "Shape")));
                assert_eq!(ancestors, vec![anc(tsp, "Transform")]);
            }
            o => panic!("{o:?}"),
        }
        // A Material with no Shape in its proven chain.
        match prove(&item(lonesp), located("Material", lonesp, &["SCENE"])) {
            p::HighlightProveOutcome::Refused { reason, .. } => {
                assert_eq!(reason, p::highlight::refusal::NO_OWNER)
            }
            o => panic!("{o:?}"),
        }
        // A rendered node keeps no owner.
        assert!(matches!(
            prove(&item(tsp), located("Transform", tsp, &["SCENE"])),
            p::HighlightProveOutcome::Proven { owner: None, .. }
        ));
        // A Viewpoint is never drawn: refused at the target, by that reason.
        match svc
            .highlight_target(&p::HighlightTargetRequest {
                session: id,
                revision: rev,
                item: item(vpsp),
            })
            .unwrap()
        {
            p::HighlightTargetOutcome::Refused { reason, message } => {
                assert_eq!(reason, p::highlight::refusal::NOT_RENDERED);
                assert!(message.contains("Viewpoint"), "{message}");
            }
            o => panic!("{o:?}"),
        }
        // The Material itself is a valid target (outlined through its owner).
        assert!(matches!(
            svc.highlight_target(&p::HighlightTargetRequest {
                session: id,
                revision: rev,
                item: item(msp),
            })
            .unwrap(),
            p::HighlightTargetOutcome::Ready { .. }
        ));
        assert_eq!(svc.text_at(id).unwrap(), (text.clone(), rev), "read-only");
        fs::remove_dir_all(&dir).unwrap();
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

/// RUST-STAB-1: recovery through the real service paths.
#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::recovery::{Recovery, Schedule};
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wrlforge-tauri-rec-{tag}-{}-{}",
            std::process::id(),
            recovery::new_id()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// One "process": a service with recovery in `dir/recovery`. Dropping
    /// it releases the instance lock exactly like a process exit.
    fn process(dir: &Path) -> Service {
        let svc = Service::default();
        let rec = Recovery::open(&dir.join("recovery"), Schedule::default());
        assert!(rec.warning().is_none(), "{:?}", rec.warning());
        svc.attach_recovery(Arc::new(rec));
        svc
    }

    fn opened(o: p::OpenOutcome) -> p::DocumentInfo {
        match o {
            p::OpenOutcome::Opened { doc } => doc,
            other => panic!("not opened: {other:?}"),
        }
    }

    fn edit(svc: &Service, id: u64, at: u64, ins: &str) -> p::DocState {
        let rev = svc.snapshot(id).unwrap().revision;
        match svc
            .edit(&p::EditRequest {
                session: id,
                base_revision: rev,
                from: at,
                to: at,
                insert: ins.into(),
                item: None,
            })
            .unwrap()
        {
            p::EditOutcome::Applied { state, .. } => state,
            other => panic!("refused: {other:?}"),
        }
    }

    fn records(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir.join("recovery"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("rec-"))
            .collect();
        v.sort();
        v
    }

    /// The text of the single record for session `id`, read from disk.
    fn on_disk(svc: &Service, id: u64) -> Option<String> {
        let rec = svc.recovery().unwrap();
        let rid = svc.with(id, |s| s.recovery_id.clone()).unwrap();
        rec.store().unwrap().read(&rid).ok().map(|(_, t)| t)
    }

    const TRICKY: &str = "\u{FEFF}#VRML V2.0 utf8\r\n# caf\u{e9} \u{1F600}\r\nDEF A Transform { translation 0 0 0 }\rUSE A\n";

    #[test]
    fn crash_then_restore_is_exact_for_every_form_and_untitled() {
        let d = tmp("restore");
        let docs = d.join("docs");
        fs::create_dir_all(&docs).unwrap();
        let plain = docs.join("plain.wrl");
        let gz = docs.join("packed.wrl");
        let crlf = docs.join("tricky.wrl");
        fs::write(&plain, "#VRML V2.0 utf8\nShape { geometry Box {} }\n").unwrap();
        fs::write(&gz, files::encode("#VRML V2.0 utf8\nGroup {}\n", Format::Gzip).unwrap()).unwrap();
        fs::write(&crlf, TRICKY).unwrap();
        let originals: Vec<Vec<u8>> = [&plain, &gz, &crlf].iter().map(|p| fs::read(p).unwrap()).collect();

        let mut expected: Vec<(String, String, bool)> = vec![];
        {
            let svc = process(&d);
            for (path, at, ins) in [(&plain, 16u64, "# one\n"), (&gz, 0, "#x"), (&crlf, 1, "\u{2603}")] {
                let doc = opened(svc.open_path(path));
                edit(&svc, doc.session, at, ins);
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                expected.push((name, svc.text(doc.session).unwrap(), false));
            }
            // A new world, never saved, with a created object.
            let nw = opened(svc.new_world());
            let o = svc
                .create(&p::CreateRequest { at: None,
                    session: nw.session,
                    base_revision: nw.revision,
                    primitive: p::Primitive::Box,
                })
                .unwrap();
            assert!(matches!(o, p::CreateOutcome::Applied { .. }));
            expected.push((UNTITLED_NAME.into(), svc.text(nw.session).unwrap(), true));
            // A clean document never produces a record.
            opened(svc.open_path(&plain));
            assert_eq!(svc.recovery_pass(), 4);
            assert_eq!(records(&d).len(), 4);
            // The process dies here (no save, no clean shutdown path).
        }
        let svc = process(&d);
        let list = svc.recovery_list();
        assert!(list.damaged.is_empty(), "{:?}", list.damaged);
        assert_eq!(list.entries.len(), 4);
        for (name, text, untitled) in &expected {
            let e = list.entries.iter().find(|e| &e.name == name).unwrap();
            assert_eq!(e.untitled, *untitled);
            assert!(!e.name.contains('/'));
            assert_eq!(e.bytes, text.len() as u64);
            if !untitled {
                assert_eq!(e.original_now.as_deref(), Some("unchanged"));
            }
            let doc = opened(svc.recovery_restore(&e.id));
            assert!(doc.untitled && doc.recovered && doc.dirty, "{doc:?}");
            assert_eq!(doc.name, *name);
            assert_eq!(doc.format, if name == "packed.wrl" { "gzip" } else { "plain" });
            assert_eq!(svc.text(doc.session).unwrap().as_bytes(), text.as_bytes(), "{name}");
            // Restoring twice is refused: this window owns it now.
            assert!(matches!(svc.recovery_restore(&e.id), p::OpenOutcome::Failed { .. }));
            // A plain Save cannot reach any file: Save As is required.
            assert!(matches!(svc.save(doc.session), p::SaveOutcome::Failed { .. }));
        }
        // The originals are still byte-identical.
        for (p, b) in [&plain, &gz, &crlf].iter().zip(&originals) {
            assert_eq!(&fs::read(p).unwrap(), b);
        }
        assert_eq!(records(&d).len(), 4, "records stay until Save As or Discard");
        // Nothing is offered to this process for its own restored work.
        assert!(svc.recovery_list().entries.is_empty());
    }

    #[test]
    fn restored_work_edits_undoes_and_saves_as_its_own_format() {
        let d = tmp("saveas");
        let gz = d.join("item.wrl");
        let text = "#VRML V2.0 utf8\r\nGroup { }\r\n";
        fs::write(&gz, files::encode(text, Format::Gzip).unwrap()).unwrap();
        let before = fs::read(&gz).unwrap();
        let typed;
        {
            let svc = process(&d);
            let doc = opened(svc.open_path(&gz));
            edit(&svc, doc.session, 0, "# kept\n");
            assert!(svc.text(doc.session).unwrap().starts_with("# kept\r\n"));
            typed = svc.text(doc.session).unwrap();
            svc.recovery_pass();
        }
        let svc = process(&d);
        let e = svc.recovery_list().entries.remove(0);
        let doc = opened(svc.recovery_restore(&e.id));
        let id = doc.session;
        // Normal editing and Undo on the recovered text.
        edit(&svc, id, 0, "X");
        assert_ne!(svc.text(id).unwrap(), typed);
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        assert_eq!(svc.text(id).unwrap(), typed);
        assert!(svc.snapshot(id).unwrap().dirty, "undo does not make it saved");
        svc.recovery_pass();
        assert_eq!(on_disk(&svc, id).unwrap(), typed, "the record follows the newest revision");
        // Save As writes gzip, then the record is complete (removed).
        let out = d.join("out.wrl");
        assert!(matches!(svc.save_as(id, &out), p::SaveOutcome::Saved { .. }));
        let raw = fs::read(&out).unwrap();
        assert!(files::is_gzip(&raw));
        assert_eq!(files::decode(&raw).unwrap().0, typed);
        assert!(records(&d).is_empty());
        assert_eq!(fs::read(&gz).unwrap(), before, "the original never changed");
    }

    #[test]
    fn every_document_change_reaches_recovery_and_save_completes_it() {
        let d = tmp("changes");
        let svc = process(&d);
        let nw = opened(svc.new_world());
        let id = nw.session;
        assert_eq!(svc.recovery_pass(), 0, "an unchanged new world is clean");
        let (state, item) = match svc
            .create(&p::CreateRequest { at: None, session: id, base_revision: 0, primitive: p::Primitive::Box })
            .unwrap()
        {
            p::CreateOutcome::Applied { state, item, .. } => (state, item),
            other => panic!("{other:?}"),
        };
        let check = |what: &str| {
            assert_eq!(svc.recovery_pass(), 1, "{what} scheduled a write");
            assert_eq!(on_disk(&svc, id).unwrap(), svc.text(id).unwrap(), "{what}");
        };
        check("create");
        let o = svc
            .translate(&p::TranslateRequest { instance: None,
                confirm_shared: false,
                session: id,
                base_revision: state.revision,
                item: item.clone(),
                axis: p::gizmo::Axis::X,
                value: 1.5,
                decimals: 3,
            })
            .unwrap();
        let (rev, item) = match o {
            p::TranslateOutcome::Applied { state, item, .. } => (state.revision, item),
            other => panic!("{other:?}"),
        };
        check("move");
        for (what, o) in [
            (
                "rotate",
                svc.rotate(&p::RotateRequest { instance: None, confirm_shared: false, session: id, base_revision: rev, item: item.clone(), axis: p::gizmo::Axis::Y, delta: 0.5 }),
            ),
        ] {
            match o.unwrap() {
                p::TranslateOutcome::Applied { .. } => check(what),
                // A created Box may lack the field: a refusal changes nothing.
                _ => assert_eq!(svc.recovery_pass(), 0),
            }
        }
        assert!(matches!(svc.undo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        check("undo");
        assert!(matches!(svc.redo(id).unwrap(), p::HistoryOutcome::Applied { .. }));
        check("redo");
        edit(&svc, id, 0, "#");
        check("typing");
        // Writing a record never changes the document or its history.
        let info = svc.snapshot(id).unwrap();
        svc.recovery_pass();
        assert_eq!(svc.snapshot(id).unwrap(), info);
        // A pending write that runs after Save cannot recreate the record.
        edit(&svc, id, 0, "#");
        let path = d.join("w.wrl");
        assert!(matches!(svc.save_as(id, &path), p::SaveOutcome::Saved { .. }));
        assert!(records(&d).is_empty(), "save completes the record at once");
        assert_eq!(svc.recovery_pass(), 0);
        assert!(records(&d).is_empty());
        // Undo back to the saved text is clean: no record; redo is dirty.
        edit(&svc, id, 0, "#");
        svc.recovery_pass();
        assert_eq!(records(&d).len(), 1);
        svc.undo(id).unwrap();
        assert!(records(&d).is_empty(), "back at the saved text");
    }

    #[test]
    fn failed_save_conflict_and_cancel_keep_the_record() {
        let d = tmp("keep");
        let path = d.join("a.wrl");
        fs::write(&path, "#VRML V2.0 utf8\n").unwrap();
        let svc = process(&d);
        let id = opened(svc.open_path(&path)).session;
        edit(&svc, id, 0, "# mine\n");
        svc.recovery_pass();
        let mine = svc.text(id).unwrap();
        // An outside change: Save is a conflict, the record stays.
        fs::write(&path, "#VRML V2.0 utf8\n# theirs\n").unwrap();
        assert_eq!(svc.check_external(id).unwrap().changed, true);
        assert!(matches!(svc.save(id), p::SaveOutcome::Conflict { .. }));
        assert_eq!(on_disk(&svc, id).unwrap(), mine);
        // A failed write (read-only folder): the record stays.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ro = d.join("ro");
            fs::create_dir_all(&ro).unwrap();
            fs::set_permissions(&ro, fs::Permissions::from_mode(0o500)).unwrap();
            let r = svc.save_as(id, &ro.join("b.wrl"));
            fs::set_permissions(&ro, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(matches!(r, p::SaveOutcome::Failed { .. }), "{r:?}");
            assert_eq!(on_disk(&svc, id).unwrap(), mine);
        }
        // Closing without a decision (or a canceled quit) keeps the record.
        svc.close(id).unwrap();
        assert_eq!(records(&d).len(), 1);
        drop(svc);
        let svc = process(&d);
        assert_eq!(svc.recovery_list().entries.len(), 1);
        // Later: nothing changes; the next start offers it again.
        drop(svc);
        let svc = process(&d);
        let e = svc.recovery_list().entries.remove(0);
        assert_eq!(e.original_now.as_deref(), Some("changed"));
        // Discard removes ONLY that record.
        let other = opened(svc.new_world()).session;
        edit(&svc, other, 0, "#");
        svc.recovery_pass();
        svc.recovery_discard(&e.id).unwrap();
        assert_eq!(records(&d).len(), 1, "this window's record is untouched");
        assert!(svc.recovery_list().entries.is_empty());
        // An explicit discard of open work removes its record.
        svc.discard_recovery(other).unwrap();
        assert!(records(&d).is_empty());
    }

    #[test]
    fn a_failed_recovery_write_warns_and_changes_nothing() {
        let d = tmp("warn");
        let svc = process(&d);
        let id = opened(svc.new_world()).session;
        edit(&svc, id, 0, "# a\n");
        svc.recovery_pass();
        let first = on_disk(&svc, id).unwrap();
        let info = svc.snapshot(id).unwrap();
        svc.recovery().unwrap().store().unwrap().fail_next_writes(1);
        edit(&svc, id, 0, "# b\n");
        let info2 = svc.snapshot(id).unwrap();
        assert_eq!(svc.recovery_pass(), 0);
        assert_eq!(svc.snapshot(id).unwrap(), info2, "the document is unchanged by the failure");
        assert_ne!(info, info2);
        assert_eq!(on_disk(&svc, id).unwrap(), first, "the older record is kept");
        assert!(svc.recovery_list().warning.unwrap().contains("could not save"));
        // The next change writes again and clears the warning.
        edit(&svc, id, 0, "# c\n");
        assert_eq!(svc.recovery_pass(), 1);
        assert!(svc.recovery_list().warning.is_none());
        // Unavailable storage: editing still works, with a warning.
        let svc2 = Service::default();
        svc2.attach_recovery(Arc::new(Recovery::unavailable("test")));
        let id2 = opened(svc2.new_world()).session;
        edit(&svc2, id2, 0, "#");
        assert_eq!(svc2.recovery_pass(), 0);
        assert!(svc2.recovery_list().warning.unwrap().contains("unavailable"));
    }

    #[test]
    fn a_damaged_record_is_reported_and_not_restorable() {
        let d = tmp("damaged");
        {
            let svc = process(&d);
            let id = opened(svc.new_world()).session;
            edit(&svc, id, 0, "#");
            svc.recovery_pass();
        }
        let name = records(&d).remove(0);
        let p = d.join("recovery").join(&name);
        let mut b = fs::read(&p).unwrap();
        let n = b.len();
        b[n - 1] ^= 0x20;
        fs::write(&p, &b).unwrap();
        let svc = process(&d);
        let l = svc.recovery_list();
        assert!(l.entries.is_empty());
        assert_eq!(l.damaged.len(), 1);
        assert!(l.damaged[0].reason.contains("checksum"), "{}", l.damaged[0].reason);
        assert!(matches!(svc.recovery_restore(&l.damaged[0].id), p::OpenOutcome::Failed { .. }));
        assert!(p.exists(), "never deleted without the user");
        svc.recovery_discard(&l.damaged[0].id).unwrap();
        assert!(!p.exists());
    }
}

