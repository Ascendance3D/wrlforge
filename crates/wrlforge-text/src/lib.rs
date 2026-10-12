// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge pure source-text core (RUST-1).
//!
//! Adopted from the RUST-0 spike (`spikes/rust-core-feasibility/`). Pure
//! computation only: no filesystem, network, clock, environment, process or
//! platform I/O. The exact source text supplied by the JavaScript owner is the
//! document; everything here is a derived, disposable projection of it.
//!
//! Public offsets are UTF-16 code units (`u64`), half-open `[from, to)`, exactly
//! as in `src/vrml/edit.js`. UTF-8 byte offsets are an internal detail.
#![forbid(unsafe_code)]

pub mod edit;
pub mod line_index;
pub mod offsets;
pub mod snapshot;

pub use edit::{apply_edits, map_offset, map_range, Affinity, Edit, EditCode, EditError};
pub use line_index::{LineCol, LineEndings, LineError, LineIndex};
pub use offsets::{decode_utf16_strict, EncodingError, OffsetError};
pub use snapshot::{Span, SpanError, TextSnapshot};
