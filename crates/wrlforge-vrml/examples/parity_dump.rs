// SPDX-License-Identifier: GPL-3.0-or-later
//! Parity probe: reads UTF-8 VRML text on stdin, prints one line per
//! diagnostic (`D code from to`) and per Node/USE/ROUTE/PROTO/EXTERNPROTO in
//! source order (`N kind name from to`). `spikes/tauri-rust-migration-1/
//! parity.sh` diffs this against the same dump from `src/vrml/parser.js`.
use std::io::Read;
use wrlforge_vrml::ast::*;

fn walk(a: &Ast, out: &mut Vec<String>) {
    let r = a.range();
    let (s, e) = (r.start.offset, r.end.offset);
    match a {
        Ast::Node(n) => {
            out.push(format!(
                "N node {}:{} {s} {e}",
                n.node_type,
                n.def.as_deref().unwrap_or("-")
            ));
            for f in &n.fields {
                walk(f, out);
            }
        }
        Ast::Field(f) => {
            if let Some(v) = &f.value {
                walk(v, out);
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| walk(i, out)),
        Ast::Use { name, .. } => {
            out.push(format!("N use {} {s} {e}", name.as_deref().unwrap_or("-")))
        }
        Ast::Route(_) => out.push(format!("N route - {s} {e}")),
        Ast::Proto(p) => {
            out.push(format!(
                "N proto {} {s} {e}",
                p.name.as_deref().unwrap_or("-")
            ));
            p.body.iter().for_each(|b| walk(b, out));
        }
        Ast::ExternProto(x) => out.push(format!(
            "N externproto {} {s} {e}",
            x.name.as_deref().unwrap_or("-")
        )),
        _ => {}
    }
}

fn main() {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .expect("utf-8 stdin");
    let r = wrlforge_vrml::parse(&text);
    for d in &r.diagnostics {
        println!(
            "D {} {} {}",
            d.code.as_str(),
            d.range.start.offset,
            d.range.end.offset
        );
    }
    let mut out = vec![];
    r.tree.statements.iter().for_each(|s| walk(s, &mut out));
    for l in out {
        println!("{l}");
    }
}
