// SPDX-License-Identifier: GPL-3.0-or-later
//! The WRL Forge desktop IPC vocabulary: data only, shared by the Tauri backend
//! (`src-tauri`) and the Rust/Wasm UI (`ui`) so the two cannot drift.
//!
//! Security shape: the UI addresses documents by an opaque Rust-minted
//! `session` number. No request carries a filesystem path; every path is chosen
//! by a native dialog (or the launch argument) on the Rust side and stays there.
//! `display_path` is informational text for the title/status bar only.
//!
//! Offsets named `view_*` are UTF-16 code units in the EDITOR VIEW (line breaks
//! projected to `\n`, see `wrlforge-document`).

use serde::{Deserialize, Serialize};

pub mod syntax;
pub mod theme;

pub type SessionId = u64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub session: SessionId,
    pub name: String,
    pub display_path: String,
    /// "plain" | "gzip"
    pub format: String,
    pub view: String,
    pub revision: u64,
    pub dirty: bool,
    pub eol: String,
    pub eol_mixed: bool,
    pub bom: bool,
    pub bytes_on_disk: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    /// A new world that has no file yet (VISUAL-1): Save goes through Save
    /// As, and `display_path` is a label, not a path.
    #[serde(default)]
    pub untitled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OpenOutcome {
    Opened { doc: DocumentInfo },
    Canceled,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditRequest {
    pub session: SessionId,
    pub base_revision: u64,
    pub from: u64,
    pub to: u64,
    pub insert: String,
    /// The selected Scene Tree item id, valid at `base_revision`. Rust maps
    /// it through the exact change (`EditOutcome::Applied::item`).
    #[serde(default)]
    pub item: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocState {
    pub revision: u64,
    pub dirty: bool,
    pub view_len: u64,
    pub view_hash: u64,
    pub caret: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum EditOutcome {
    Applied {
        state: DocState,
        /// The request's item carried through the exact change, or `None`
        /// when it cannot be proven (or none was sent).
        #[serde(default)]
        item: Option<String>,
    },
    /// Refused; the buffer is unchanged. The UI must resync its widget from
    /// `doc_snapshot` before sending another edit.
    Refused { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum HistoryOutcome {
    Applied {
        state: DocState,
        view: String,
        /// The selected node's item id carried through the exact change, or
        /// `None` when it cannot be proven (or nothing was selected).
        item: Option<String>,
    },
    Nothing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SaveOutcome {
    Saved {
        bytes_written: u64,
        /// File name of the backup taken before the replace, if any.
        backup: Option<String>,
        /// True when an unchanged gzip artifact was kept without any write.
        preserved: bool,
        doc: DocumentInfo,
    },
    /// The file changed on disk since it was opened or last saved. Nothing was
    /// written. `reason`: "deleted" | "size" | "content".
    Conflict {
        reason: String,
    },
    Canceled,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalStatus {
    pub changed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub view_from: u64,
    pub view_to: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub detail: Option<String>,
    pub depth: u32,
    pub view_from: u64,
    pub view_to: u64,
    pub use_status: Option<String>,
    pub child_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    /// The session and revision this ONE parse was made from. The UI applies
    /// highlights and diagnostic marks only when both, and the view hash,
    /// still match what the editor shows.
    pub session: SessionId,
    pub revision: u64,
    pub view_hash: u64,
    pub view_len: u64,
    /// Syntax spans from the same parse, `syntax::encode`d, VIEW offsets.
    pub highlights: Vec<u32>,
    pub highlights_truncated: bool,
    /// Depth-first source order; the document root is omitted.
    pub items: Vec<SceneItem>,
    pub diagnostics: Vec<Diagnostic>,
    /// "flat": USE resolution is the non-authoritative flat scope.
    pub resolution_scope: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InspectorRow {
    pub name: String,
    pub kind: String,
    pub source: String,
    pub elided: bool,
    pub view_from: u64,
    pub view_to: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub revision: u64,
    pub id: String,
    pub title: String,
    pub rows: Vec<InspectorRow>,
    /// Typed field descriptors when the item is a node; `None` otherwise.
    pub node: Option<NodeFields>,
}

/// The reply to `doc_inspect`. An item id names a source span of ONE
/// revision, so the request says which revision it came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum InspectOutcome {
    Found {
        inspection: Inspection,
    },
    /// No item with this id in that revision.
    Missing,
    /// The document is no longer at the requested revision. Nothing was
    /// inspected; the caller must use a Scene Tree of the current revision.
    Stale {
        current: u64,
    },
}

/// One component of an editable value, as Rust read it from the source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldComponent {
    pub label: String,
    /// Bool: `TRUE`/`FALSE`; number: the exact source lexeme; string: decoded.
    pub text: String,
    pub bool_value: Option<bool>,
}

/// One explicitly authored field of a node (`wrlforge_vrml::field_edit`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditableField {
    /// Index into the node's body statements; sent back with an edit.
    pub index: u32,
    pub name: String,
    pub field_type: Option<String>,
    pub declaration: Option<String>,
    /// "bool" | "number" | "string" when the type is editable.
    pub kind: Option<String>,
    pub editable: bool,
    /// A stable reason id (`ok` when editable).
    pub reason: String,
    pub components: Vec<FieldComponent>,
    /// Human text of the schema's numeric bounds, if any.
    pub bounds: Option<String>,
    pub constraint_note: Option<String>,
    pub value_excerpt: String,
    pub view_from: u64,
    pub view_to: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeFields {
    pub editable: bool,
    pub reason: String,
    pub node_type: Option<String>,
    pub fields: Vec<EditableField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FieldInput {
    Bool { value: bool },
    Text { value: String },
}

/// An Inspector edit. The node is named by the Scene Tree item id of the
/// SAME revision; Rust re-proves the node and field before any change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldEditRequest {
    pub session: SessionId,
    pub base_revision: u64,
    pub item: String,
    pub field_index: u32,
    pub field_name: String,
    pub components: Vec<FieldInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FieldEditOutcome {
    /// Applied as one undo step. `view` is the new editor view.
    Applied {
        state: DocState,
        view: String,
        changed: Vec<u32>,
        /// The same node's Scene Tree item id in the NEW revision (its end
        /// moved by the edit; its start did not).
        item: String,
    },
    /// The value already holds exactly this text; nothing changed.
    Unchanged,
    /// Refused; the buffer and revision are unchanged.
    Refused {
        reason: String,
        message: Option<String>,
        component_index: Option<u32>,
    },
}

/// The primitives the Create control offers (VISUAL-1). A closed set: the
/// UI names a primitive, never a template or source text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Primitive {
    Box,
    Sphere,
    Cylinder,
    Cone,
}

impl Primitive {
    pub const ALL: [Primitive; 4] = [
        Primitive::Box,
        Primitive::Sphere,
        Primitive::Cylinder,
        Primitive::Cone,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Primitive::Box => "Box",
            Primitive::Sphere => "Sphere",
            Primitive::Cylinder => "Cylinder",
            Primitive::Cone => "Cone",
        }
    }
}

/// Create one primitive object at top level of the document, as it is at
/// `base_revision`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub session: SessionId,
    pub base_revision: u64,
    pub primitive: Primitive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CreateOutcome {
    /// Inserted as ONE undo step. `item` is the new Transform's Scene Tree
    /// id in the NEW revision (`state.revision`), proven by a re-parse.
    Applied {
        state: DocState,
        view: String,
        item: String,
        def_name: String,
    },
    /// Refused; the buffer and revision are unchanged.
    Refused { reason: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSource {
    pub revision: u64,
    /// The canonical text (not the view projection), minus a leading BOM.
    pub text: String,
}

/// Launch-time smoke test plan (`--smoke`), consumed by the UI. Never set in a
/// normal launch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmokePlan {
    pub insert_text: String,
    pub expect_preview: bool,
    /// `--smoke-inspector`: also drive an Inspector edit that sets component
    /// 2 (G / Y) of the first editable `diffuseColor` (else `translation`)
    /// to this value, through the real Inspector controls.
    pub inspector_value: Option<String>,
    /// `--smoke-theme`: also drive the toolbar theme selector (UI-THEME-1).
    #[serde(default)]
    pub theme: Option<ThemeSmoke>,
    /// UI-SYNTAX-1 visual check: when non-zero, the run pauses this long at
    /// each alignment state with the textarea's own glyphs made visible over
    /// the color layer, so an external screenshot can prove registration.
    #[serde(default)]
    pub syntax_align_hold_ms: u64,
    /// `--smoke-create` (VISUAL-1): run the New World → Create workflow
    /// instead of the file workflow.
    #[serde(default)]
    pub create: Option<CreateSmoke>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateSmoke {
    /// The Box's new translation, typed into the Inspector.
    pub translation: Vec<String>,
    /// The Box Material's new diffuseColor, typed into the Inspector.
    pub color: Vec<String>,
    /// Every built-in theme id, cycled with the document open.
    pub themes: Vec<String>,
    /// When non-zero, pause this long at named checkpoints (screenshots).
    pub hold_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSmoke {
    /// The theme the window must show at startup (from the settings file).
    pub expect_startup: String,
    /// A startup notice (corrupt / unknown settings) must be visible.
    pub expect_notice: bool,
    /// The theme the run selects last; persisted for the next run.
    pub final_theme: String,
    /// The settings directory is unwritable: saves must fail visibly.
    pub expect_save_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmokeStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmokeReport {
    pub steps: Vec<SmokeStep>,
}

/// FNV-1a over the UTF-16 code units of the editor view, truncated to 53 bits
/// so it survives the JSON number round trip exactly. A drift detector shared
/// by both sides, not a security hash.
pub fn view_hash(view: &str) -> u64 {
    view_hash_units(view.encode_utf16())
}

/// `view_hash` over UTF-16 code units the caller already holds.
pub fn view_hash_units(units: impl IntoIterator<Item = u16>) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for u in units {
        h ^= u as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h & ((1u64 << 53) - 1)
}
