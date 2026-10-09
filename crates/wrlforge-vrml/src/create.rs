// SPDX-License-Identifier: GPL-3.0-or-later
//! Visual object creation (VISUAL-1): plan the insertion of ONE new VRML97
//! primitive object into a document, as an exact source-text patch.
//!
//! The generated structure is fixed and standard (ISO/IEC 14772-1):
//!
//! ```text
//! DEF <Name> Transform {          translation / rotation / scale authored
//!   children [
//!     Shape {
//!       appearance Appearance { material Material { diffuseColor … } }
//!       geometry Box | Sphere | Cylinder | Cone { … }
//!     }
//!   ]
//! }
//! ```
//!
//! Nothing here accepts source text or a template from a caller: the only
//! input is a closed `Primitive` choice. The planner is pure (text in, plan
//! out) and refuses rather than guesses:
//!
//! * the document must be a VRML97 (`#VRML V2.0 utf8`) file with no blocking
//!   syntax error, no recovered (incomplete) node and no parser cap, so the
//!   end of the text is PROVABLY at top level, outside every node and PROTO;
//! * the insertion is one pure insert at the end of the text (top-level
//!   scope): no existing byte changes;
//! * the DEF name is one no identifier token in the document already uses
//!   (any scope), so it can neither collide with nor capture a USE / ROUTE;
//! * the result is re-parsed and must hold the old top-level statements at
//!   their old spans plus exactly the intended structure at the intended
//!   span; otherwise the plan is refused.

use crate::ast::{Ast, Node};
use crate::field_edit::has_blocking_syntax_error;
use crate::parser::{parse, ParseResult};
use crate::tokenizer::TokKind;

/// The primitives the Create control offers. A closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// The VRML97 geometry node type, also the DEF name stem.
    pub fn node_type(self) -> &'static str {
        match self {
            Primitive::Box => "Box",
            Primitive::Sphere => "Sphere",
            Primitive::Cylinder => "Cylinder",
            Primitive::Cone => "Cone",
        }
    }

    pub fn from_id(id: &str) -> Option<Primitive> {
        Primitive::ALL.into_iter().find(|p| p.node_type() == id)
    }

    /// The geometry's explicitly authored fields (ISO 6.4 / 6.42 / 6.17 /
    /// 6.14 defaults, written out so the Inspector can show them).
    fn geometry_fields(self) -> &'static [&'static str] {
        match self {
            Primitive::Box => &["size 2 2 2"],
            Primitive::Sphere => &["radius 1"],
            Primitive::Cylinder => &["radius 1", "height 2"],
            Primitive::Cone => &["bottomRadius 1", "height 2"],
        }
    }

    fn diffuse_color(self) -> &'static str {
        match self {
            Primitive::Box => "0.8 0.3 0.2",
            Primitive::Sphere => "0.2 0.5 0.8",
            Primitive::Cylinder => "0.3 0.7 0.3",
            Primitive::Cone => "0.8 0.7 0.2",
        }
    }
}

/// The text of a new, empty VRML97 world.
pub const NEW_WORLD: &str = "#VRML V2.0 utf8\n";

pub mod reason {
    pub const NOT_VRML97: &str = "not-vrml97";
    pub const SYNTAX_ERROR: &str = "document-has-syntax-errors";
    pub const PARSE_CAPPED: &str = "document-too-large-to-prove";
    pub const NO_FREE_NAME: &str = "no-free-def-name";
    pub const VERIFY_FAILED: &str = "inserted-structure-not-proven";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Ready {
        /// One pure insert at `at` (UTF-16, source coordinates).
        at: u64,
        insert: String,
        new_text: String,
        /// The new Transform's UTF-16 source span in `new_text`.
        node_from: u64,
        node_to: u64,
        def_name: String,
    },
    Refused {
        reason: &'static str,
        message: String,
    },
}

fn refused(reason: &'static str, message: impl Into<String>) -> Plan {
    Plan::Refused {
        reason,
        message: message.into(),
    }
}

/// The object's source, indented two spaces per level, lines joined by `eol`.
pub fn template(p: Primitive, def_name: &str, eol: &str) -> String {
    let mut lines = vec![
        format!("DEF {def_name} Transform {{"),
        "  translation 0 0 0".into(),
        "  rotation 0 1 0 0".into(),
        "  scale 1 1 1".into(),
        "  children [".into(),
        "    Shape {".into(),
        "      appearance Appearance {".into(),
        "        material Material {".into(),
        format!("          diffuseColor {}", p.diffuse_color()),
        "        }".into(),
        "      }".into(),
        format!("      geometry {} {{", p.node_type()),
    ];
    lines.extend(p.geometry_fields().iter().map(|f| format!("        {f}")));
    lines.extend(["      }".into(), "    }".into(), "  ]".into(), "}".into()]);
    lines.join(eol)
}

fn utf16_len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

/// The line ending new text uses: the document's most frequent one (ties
/// prefer LF, then CRLF), LF when it has none -- the same rule as typing.
fn dominant_eol(text: &str) -> &'static str {
    let b = text.as_bytes();
    let (mut lf, mut crlf, mut cr) = (0u64, 0u64, 0u64);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    if crlf > lf && crlf >= cr {
        "\r\n"
    } else if cr > lf && cr > crlf {
        "\r"
    } else {
        "\n"
    }
}

/// Why `p` does not prove its end is at top level, or `None` when it does.
fn unprovable(p: &ParseResult) -> Option<Plan> {
    let vrml97 = p.tree.header.as_ref().is_some_and(|h| {
        h.version.as_deref() == Some("V2.0")
            && h.encoding
                .as_deref()
                .is_some_and(|e| e.eq_ignore_ascii_case("utf8"))
    });
    if !vrml97 {
        return Some(refused(
            reason::NOT_VRML97,
            "Objects can be created only in a '#VRML V2.0 utf8' document.",
        ));
    }
    if p.truncated || p.depth_capped {
        return Some(refused(
            reason::PARSE_CAPPED,
            "The document exceeds the parser limits; its structure cannot be proven.",
        ));
    }
    if has_blocking_syntax_error(p) || p.tree.statements.iter().any(incomplete) {
        return Some(refused(
            reason::SYNTAX_ERROR,
            "Fix the syntax errors first: the end of the document is not provably at top level.",
        ));
    }
    None
}

/// Any parser-recovered node anywhere under `a`.
fn incomplete(a: &Ast) -> bool {
    match a {
        Ast::Node(n) => n.incomplete || n.fields.iter().any(incomplete),
        Ast::Field(f) => f.value.as_deref().is_some_and(incomplete),
        Ast::Array { items, .. } => items.iter().any(incomplete),
        Ast::Proto(pr) => pr.body.iter().any(incomplete),
        _ => false,
    }
}

/// The first `<Stem>_<n>` (n ≥ 1) that no identifier token in the document
/// spells: not a DEF, USE, ROUTE end, PROTO, field or anything else.
fn free_name(p: &ParseResult, src: &str, stem: &str) -> Option<String> {
    let used: std::collections::HashSet<&str> = p
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokKind::Id))
        .map(|t| t.lexeme(src))
        .collect();
    (1..=100_000)
        .map(|n| format!("{stem}_{n}"))
        .find(|n| !used.contains(n.as_str()))
}

/// The value of field `name` of `n`, if authored exactly once.
fn field<'a>(n: &'a Node, name: &str) -> Option<&'a Ast> {
    let mut it = n.fields.iter().filter_map(|f| match f {
        Ast::Field(f) if f.name == name && !f.is_binding => f.value.as_deref(),
        _ => None,
    });
    let v = it.next();
    it.next().is_none().then_some(v).flatten()
}

fn node<'a>(a: Option<&'a Ast>, ty: &str) -> Option<&'a Node> {
    match a {
        Some(Ast::Node(n)) if n.node_type == ty && !n.incomplete => Some(n),
        _ => None,
    }
}

/// The intended structure, exactly: DEF name, Transform with the three
/// transform fields, one Shape child, Appearance/Material/diffuseColor and
/// the primitive geometry.
fn structure_ok(n: &Node, p: Primitive, def_name: &str) -> bool {
    let shape = match field(n, "children") {
        Some(Ast::Array { items, .. }) if items.len() == 1 => node(items.first(), "Shape"),
        _ => None,
    };
    n.node_type == "Transform"
        && n.def.as_deref() == Some(def_name)
        && n.fields.len() == 4
        && ["translation", "rotation", "scale"]
            .iter()
            .all(|f| matches!(field(n, f), Some(Ast::Numbers { .. })))
        && shape.is_some_and(|s| {
            let mat = node(field(s, "appearance"), "Appearance")
                .and_then(|a| node(field(a, "material"), "Material"));
            mat.is_some_and(|m| matches!(field(m, "diffuseColor"), Some(Ast::Numbers { .. })))
                && node(field(s, "geometry"), p.node_type()).is_some()
        })
}

/// Plan the creation of one `p` object at the end of `src` (top level).
pub fn plan_create(src: &str, p: Primitive) -> Plan {
    let parsed = parse(src);
    if let Some(r) = unprovable(&parsed) {
        return r;
    }
    let Some(def_name) = free_name(&parsed, src, p.node_type()) else {
        return refused(reason::NO_FREE_NAME, "No free DEF name was found.");
    };
    let eol = dominant_eol(src);
    // A blank line before the object; the object ends its own line.
    let ends_with_break = src.ends_with(['\n', '\r']);
    let prefix = if ends_with_break {
        eol.to_string()
    } else {
        format!("{eol}{eol}")
    };
    let body = template(p, &def_name, eol);
    let insert = format!("{prefix}{body}{eol}");
    let at = utf16_len(src);
    let new_text = format!("{src}{insert}");
    let node_from = at + utf16_len(&prefix);
    let node_to = node_from + utf16_len(&body);

    // Verify by re-parsing the RESULT: old statements unchanged, one new
    // top-level statement of exactly the intended structure and span.
    let after = parse(&new_text);
    let proven = unprovable(&after).is_none()
        && after.tree.statements.len() == parsed.tree.statements.len() + 1
        && parsed
            .tree
            .statements
            .iter()
            .zip(&after.tree.statements)
            .all(|(a, b)| a.range() == b.range())
        && match after.tree.statements.last() {
            Some(Ast::Node(n)) => {
                n.range.start.offset as u64 == node_from
                    && n.range.end.offset as u64 == node_to
                    && structure_ok(n, p, &def_name)
            }
            _ => false,
        };
    if !proven {
        return refused(
            reason::VERIFY_FAILED,
            "The new object could not be proven in the resulting document; nothing changed.",
        );
    }
    Plan::Ready {
        at,
        insert,
        new_text,
        node_from,
        node_to,
        def_name,
    }
}

#[cfg(test)]
mod tests;
