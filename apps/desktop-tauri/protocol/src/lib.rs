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

pub mod gizmo;
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

/// VISUAL-3A: may the translation gizmo move `item` as it is in `revision`?
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTargetRequest {
    pub session: SessionId,
    pub revision: u64,
    pub item: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TranslateTargetOutcome {
    /// A top-level Transform with an explicit translation; a DEF name, if
    /// any, is unique, never USEd and never ROUTEd to. The preview LOCATES
    /// its runtime node by the exact provenance span `[preview_from,
    /// preview_to)` (UTF-16, preview text), and Rust must then PROVE that
    /// runtime node (`doc_translate_prove`) before anything binds.
    /// `root_index` and `def_name` are secondary consistency assertions
    /// only; neither establishes identity.
    Ready {
        revision: u64,
        item: String,
        def_name: Option<String>,
        root_index: u32,
        preview_from: u64,
        preview_to: u64,
        translation: [f64; 3],
        /// World position of the Transform's local origin.
        origin: [f64; 3],
    },
    Refused {
        reason: String,
        message: String,
    },
    /// The document is at another revision; nothing was examined.
    Stale {
        current: u64,
    },
}

/// VISUAL-3A1: prove that the runtime node the preview located for `item`
/// (a plain-data `snapshotSpan` snapshot of the generation that rendered
/// exactly `revision` / `preview_hash`) IS that authored Transform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslateProveRequest {
    pub session: SessionId,
    pub revision: u64,
    pub preview_hash: u64,
    /// The UI's preview generation number (diagnostic).
    pub generation: u64,
    pub item: String,
    pub snapshot: PickSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TranslateProveOutcome {
    /// The runtime node is exactly the authored top-level Transform.
    Proven {
        revision: u64,
        item: String,
    },
    Refused {
        reason: String,
        message: String,
    },
    /// The document is no longer the text the generation rendered.
    Stale {
        current: u64,
    },
}

/// VISUAL-3A1 camera carry: map the UTF-16 PREVIEW span `[from, to)` of
/// `from_revision` through the exact changes that produced `to_revision`
/// (which must be the current revision). Read-only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCarryRequest {
    pub session: SessionId,
    pub from_revision: u64,
    pub to_revision: u64,
    pub from: u64,
    pub to: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PreviewCarryOutcome {
    /// The same authored text, at `[from, to)` of `to_revision`'s preview.
    Mapped { from: u64, to: u64 },
    /// Not provable (unknown revision, a change touching the span, a
    /// possible BOM change): nothing is carried.
    Lost { reason: String },
}

/// VISUAL-3A: one completed gizmo drag. Rust formats `value` (at most
/// `decimals` places), re-proves the node and writes ONE token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
    pub session: SessionId,
    pub base_revision: u64,
    pub item: String,
    pub axis: gizmo::Axis,
    pub value: f64,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TranslateOutcome {
    /// Applied as ONE undo step. `item` is the same Transform in the NEW
    /// revision; `text` is the token written.
    Applied {
        state: DocState,
        view: String,
        item: String,
        text: String,
    },
    /// The value rounds to the current one: nothing changed.
    Unchanged,
    /// Refused; the buffer and revision are unchanged.
    Refused { reason: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSource {
    pub revision: u64,
    /// The canonical text (not the view projection), minus a leading BOM.
    pub text: String,
    /// `preview_hash(text)`: a viewport pick names the exact text its
    /// preview generation rendered by this hash plus `revision`.
    #[serde(default)]
    pub hash: u64,
    /// TEXTURE-LOCAL-1: the base URL X_ITE resolves relative URLs against
    /// (`wrlres://localhost/<token>/`). Rust issues a new token for every
    /// generation; a document without a folder gets one that is never
    /// valid. The text itself is never rewritten.
    #[serde(default)]
    pub resource_base: String,
    /// `ImageTexture` nodes with no URL the preview can load.
    #[serde(default)]
    pub texture_warnings: Vec<TextureWarning>,
}

/// One `ImageTexture` the preview cannot load (TEXTURE-LOCAL-1). `detail`
/// names the authored URLs and why each failed; never an absolute path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextureWarning {
    /// The node's source line (as the parser reports it).
    pub line: u32,
    /// `ImageTexture` or `ImageTexture <DEF name>`.
    pub node: String,
    pub detail: String,
}

/// One answered `wrlres` request (TEXTURE-LOCAL-1 smoke evidence). `name`
/// is the path below the token, never a filesystem path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLogEntry {
    pub session: Option<SessionId>,
    pub name: String,
    pub status: u16,
    pub outcome: String,
    pub bytes: u64,
}

/// The drift hash of a preview text (FNV-1a over its UTF-16 units, as
/// `view_hash`). Not a security hash: the revision is the primary check.
pub fn preview_hash(text: &str) -> u64 {
    view_hash(text)
}

/// A parse-provenance span `[start, end)`, UTF-16 units of the PREVIEW text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PickSpan {
    pub start: u64,
    pub end: u64,
}

/// One runtime node of a viewport hit, as plain data from the X_ITE adapter
/// (`xite-pick-adapter.js`). No runtime object crosses the boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickGraphNode {
    #[serde(rename = "type", default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub ctx_kind: String,
    #[serde(default)]
    pub occurrences: Vec<PickSpan>,
    /// Labels of other graph nodes, `SCENE` or `OTHER_CONTEXT`.
    #[serde(default)]
    pub parents: Vec<String>,
}

/// The adapter's plain-data pick snapshot. `outcome`: `hit` | `no-hit` |
/// `stale` | `external` | `unsupported` | `disabled`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PickSnapshot {
    pub outcome: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub ctx_kind: Option<String>,
    #[serde(default)]
    pub sensors: Vec<Option<String>>,
    #[serde(default)]
    pub graph: std::collections::BTreeMap<String, PickGraphNode>,
}

/// Resolve one viewport pick. `revision` and `preview_hash` name the exact
/// text the hit's preview generation rendered; Rust refuses the pick unless
/// the document holds exactly that text now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickRequest {
    pub session: SessionId,
    pub revision: u64,
    pub preview_hash: u64,
    /// The UI's preview generation number (diagnostic; echoed back).
    pub generation: u64,
    pub snapshot: PickSnapshot,
}

/// One exact source node of a proven pick: canonical UTF-16 span and the
/// same span in the editor view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickNode {
    pub from: u64,
    pub to: u64,
    pub view_from: u64,
    pub view_to: u64,
    pub node_type: String,
}

/// The reply to `doc_pick`. Only `PROVEN` carries an item; every other
/// status selects nothing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickOutcome {
    /// `PROVEN` | `NO_HIT` | `REFUSED_AMBIGUOUS` | `REFUSED_EXTERNAL` |
    /// `REFUSED_SENSOR_CONFLICT` | `REFUSED_STALE` | `UNSUPPORTED` |
    /// `COMPATIBILITY_DISABLED`.
    pub status: String,
    pub reason: String,
    /// Short user-facing text; empty for `PROVEN` and `NO_HIT`.
    pub message: String,
    pub generation: u64,
    /// The document revision the pick was resolved against.
    pub revision: u64,
    /// The Scene Tree item id (of `revision`) to select.
    pub item: Option<String>,
    /// `shape` | `simple-object`.
    pub role: Option<String>,
    pub logical: Option<PickNode>,
    pub shape: Option<PickNode>,
}

impl PickOutcome {
    pub fn is_proven(&self) -> bool {
        self.status == "PROVEN" && self.item.is_some()
    }
}

/// NATIVE-RENDER-1: the native viewport's state. Hidden: it is on only when
/// `settings.json` has `"viewport": {"renderer": "native-experimental"}` or the app was
/// launched with `--native-viewport`. X_ITE stays the default and the
/// fallback (FIFO-only Wayland, device loss, any start failure).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeState {
    /// The native viewport was requested for this run.
    pub requested: bool,
    /// `off` | `starting` | `ready` | `failed`. The UI shows the X_ITE
    /// preview in every state except `starting` and `ready`.
    pub state: String,
    pub reason: Option<String>,
    /// Adapter / backend / present mode, for the status line.
    pub info: Option<String>,
}

impl NativeState {
    pub fn native_shown(&self) -> bool {
        self.requested && (self.state == "starting" || self.state == "ready")
    }
}

/// A node the native viewport does not draw (canonical UTF-16 span).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeNotShown {
    pub node_type: String,
    pub from: u64,
    pub to: u64,
    pub reason: String,
}

/// The reply to `native_show`: which projection is on screen now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeShown {
    pub generation: u64,
    pub revision: u64,
    pub hash: u64,
    pub objects: u32,
    pub not_shown: Vec<NativeNotShown>,
    /// The current text has syntax errors: the previous valid projection
    /// stays on screen (picks against it are refused as stale).
    pub kept_last_valid: bool,
}

/// Rust -> WebView event `native-viewport`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NativeEvent {
    /// A resolved viewport click. The UI applies it exactly like a
    /// `doc_pick` reply (late and stale replies are refused there too).
    Pick { session: SessionId, pick: PickOutcome },
    /// Escape in the native viewport: clear the selection.
    Cleared { session: Option<SessionId> },
    State { state: NativeState },
}

/// The Tauri event name that carries `NativeEvent`.
pub const NATIVE_EVENT: &str = "native-viewport";

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
    /// `--smoke-pick` (VISUAL-2): viewport picking workflow + fixture matrix.
    #[serde(default)]
    pub pick: Option<PickSmoke>,
    /// `--smoke-move` (VISUAL-3A): translation-gizmo workflow.
    #[serde(default)]
    pub gizmo: Option<GizmoSmoke>,
    /// `--smoke-texture` (TEXTURE-LOCAL-1): local texture fixture matrix.
    #[serde(default)]
    pub texture: Option<TextureSmoke>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextureSmoke {
    pub fixtures: Vec<TextureFixture>,
}

/// One `--smoke-texture` fixture: what the RENDERED viewport must show.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextureFixture {
    pub id: String,
    pub label: String,
    pub samples: Vec<TextureSample>,
    /// `ImageTexture` nodes the Rust check must report as unloadable.
    pub warnings: usize,
}

/// A viewport point (fractions of its size) and its expected color;
/// `None` = the untextured surface (no image may be shown).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextureSample {
    pub x: f64,
    pub y: f64,
    pub color: Option<[u8; 3]>,
}

/// VISUAL-3A smoke plan. Fixture files stay on the Rust side; the UI opens
/// them by index (`smoke_open_fixture`), never by path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GizmoSmoke {
    pub themes: Vec<String>,
    /// Labels of the source-format fixtures, by index.
    pub fixtures: Vec<String>,
    pub hold_ms: u64,
    /// Real X pointer / key input is available (`smoke_real_pointer`).
    #[serde(default)]
    pub real_pointer: bool,
}

/// VISUAL-2 smoke plan. Fixture files stay on the Rust side; the UI opens
/// them by index (`smoke_open_fixture`), never by path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickSmoke {
    pub fixtures: Vec<PickFixture>,
    pub themes: Vec<String>,
    pub hold_ms: u64,
    /// Real X pointer input is available (`smoke_real_click`): only inside
    /// the harness's own Xvfb server, never on a desktop session.
    #[serde(default)]
    pub real_pointer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickFixture {
    pub id: String,
    /// The bound camera's position (the fixtures' front Viewpoint).
    pub camera: [f64; 3],
    pub clicks: Vec<PickClick>,
}

/// One oracle click: a world point and the expected answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PickClick {
    pub id: String,
    pub world: [f64; 3],
    #[serde(default)]
    pub skip: Option<String>,
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
    /// The expected selected node's SOURCE span, for `PROVEN`.
    #[serde(default)]
    pub logical: Option<[u64; 2]>,
    #[serde(default)]
    pub oracle_status: Option<String>,
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
