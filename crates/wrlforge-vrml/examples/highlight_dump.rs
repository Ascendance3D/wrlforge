// SPDX-License-Identifier: GPL-3.0-or-later
//! Highlight parity probe (UI-SYNTAX-1): reads UTF-8 VRML text on stdin and
//! prints one `from to class` line per highlight span (UTF-16 offsets).
//! `spikes/tauri-rust-migration-1/highlight-parity.sh` compares this with
//! the classification of `src/editor/language.js`.
use std::io::Read;

fn main() {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .expect("UTF-8 stdin");
    let parsed = wrlforge_vrml::parse(&text);
    for s in wrlforge_vrml::highlight::highlight(&parsed, &text) {
        println!("{} {} {}", s.from, s.to, s.class.as_str());
    }
}
