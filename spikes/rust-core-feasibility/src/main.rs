// SPDX-License-Identifier: GPL-3.0-or-later
//! Line-oriented differential CLI: one case per stdin line, one result per
//! stdout line. See `wire.rs` for the format.
use std::io::{self, BufRead, BufWriter, Write};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        writeln!(out, "{}", wrlforge_text_spike::wire::run_case(&line))?;
    }
    out.flush()
}
