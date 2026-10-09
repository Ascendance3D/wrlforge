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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OpenOutcome {
    Opened { doc: DocumentInfo },
    Cancelled,
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
    },
    /// Refused; the buffer is unchanged. The UI must resync its widget from
    /// `doc_snapshot` before sending another edit.
    Refused {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum HistoryOutcome {
    Applied { state: DocState, view: String },
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
    Cancelled,
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
    pub revision: u64,
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
    let mut h: u64 = 0xcbf29ce484222325;
    for u in view.encode_utf16() {
        h ^= u as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h & ((1u64 << 53) - 1)
}
