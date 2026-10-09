// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge native VRML97 document services (TAURI-RUST-MIGRATION-1).
//!
//! Direct Rust translations of `src/vrml/tokenizer.js`, `src/vrml/parser.js`
//! and the read side of `src/vrml/scene-tree.js`. Pure: text in, tokens + tree +
//! diagnostics + projections out. No filesystem, no network, no clock.
//!
//! Public offsets are UTF-16 code units, half-open, matching `src/vrml/*.js`
//! and `wrlforge-text`. The tree is a derived projection; nothing here prints
//! a tree back to text.
#![forbid(unsafe_code)]

pub mod ast;
pub mod diagnostics;
pub mod field_edit;
pub mod highlight;
pub mod node_schema;
pub mod parser;
pub mod scene;
pub mod tokenizer;

pub use parser::{parse, parse_with, Limits, ParseResult};
