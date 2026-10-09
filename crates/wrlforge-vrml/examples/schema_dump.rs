// SPDX-License-Identifier: GPL-3.0-or-later
//! Print the Rust node schema as canonical JSON for
//! `scripts/check-rust-node-schema-parity.js`.
fn main() {
    println!("{}", wrlforge_vrml::node_schema::to_canonical_json());
}
