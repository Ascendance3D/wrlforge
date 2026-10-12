// SPDX-License-Identifier: GPL-3.0-or-later
//! The Extrusion Inspector's read side (VISUAL-4A): every Extrusion field
//! (ISO/IEC 14772-1 6.18) with its authored value or its VRML97 default, and
//! plain-language notes on value counts, closure and winding.
//!
//! Read-only and pure. Nothing here writes a default into the document: a
//! field the author did not write is reported as `authored: false` with the
//! schema default as its effective value. The notes describe the authored
//! data; they never change it and never apply a vendor rule (WD.md section 9:
//! a Spazz3D export is interpreted by the VRML97 rule like any other file).

use crate::ast::{Ast, Node};
use crate::field_edit::duplicate_names;
use crate::mf_edit::{default_list, describe_list, is_closed};
use crate::node_schema;

/// The fields in Inspector order.
pub const FIELDS: [&str; 10] = [
    "crossSection",
    "spine",
    "scale",
    "orientation",
    "beginCap",
    "endCap",
    "solid",
    "ccw",
    "convex",
    "creaseAngle",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Effective {
    List(Vec<Vec<String>>),
    Single(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: &'static str,
    pub field_type: &'static str,
    /// The field is written in the source (exactly once).
    pub authored: bool,
    /// Index into `node.fields` when authored.
    pub index: Option<usize>,
    /// The effective value: authored when readable, else the default.
    pub effective: Option<Effective>,
    pub default_text: Option<&'static str>,
    /// Why an authored value cannot be read, if so.
    pub problem: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warning => "warning",
            Level::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub level: Level,
    pub field: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub fields: Vec<FieldInfo>,
    pub notes: Vec<Note>,
    /// Effective outline, as numbers (for the 2D view).
    pub outline_closed: bool,
}

fn field_info(node: &Node, name: &'static str, src: &str, dups: &[&str]) -> Option<FieldInfo> {
    let record = node_schema::get_field_schema("Extrusion", name)?;
    let index = node
        .fields
        .iter()
        .position(|f| matches!(f, Ast::Field(f) if f.name == name));
    let mut problem = None;
    let authored_value = index.and_then(|i| {
        if dups.contains(&name) {
            problem = Some(crate::field_edit::reason::FIELD_DUPLICATED);
            return None;
        }
        match describe_list(node, i, src) {
            Some(Ok(l)) => Some(Effective::List(l.lexemes())),
            Some(Err(r)) => {
                problem = Some(r);
                None
            }
            None => match &node.fields[i] {
                Ast::Field(f) => match f.value.as_deref() {
                    Some(v @ (Ast::Bool { .. } | Ast::Numbers { .. })) => {
                        Some(Effective::Single(v.range().slice(src).to_string()))
                    }
                    _ => {
                        problem = Some(crate::field_edit::reason::VALUE_SHAPE);
                        None
                    }
                },
                _ => None,
            },
        }
    });
    let effective = authored_value.or_else(|| {
        if index.is_some() {
            return None;
        }
        default_list("Extrusion", name)
            .map(Effective::List)
            .or_else(|| {
                record
                    .default_text
                    .map(|t| Effective::Single(t.to_string()))
            })
    });
    Some(FieldInfo {
        name,
        field_type: record.field_type,
        authored: index.is_some(),
        index,
        effective,
        default_text: record.default_text,
        problem,
    })
}

fn numbers(e: Option<&Effective>) -> Option<Vec<Vec<f64>>> {
    match e? {
        Effective::List(l) => l
            .iter()
            .map(|t| t.iter().map(|s| s.parse::<f64>().ok()).collect())
            .collect(),
        Effective::Single(_) => None,
    }
}

/// Twice the signed area of an (x, z) polygon (shoelace).
fn signed_area2(p: &[Vec<f64>]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (&p[i], &p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// Read every Extrusion field of `node`. `None` if it is not an Extrusion.
pub fn inspect(node: &Node, src: &str) -> Option<Info> {
    if node.node_type != "Extrusion" {
        return None;
    }
    let dups = duplicate_names(node);
    let fields: Vec<FieldInfo> = FIELDS
        .iter()
        .filter_map(|n| field_info(node, n, src, &dups))
        .collect();
    let get = |n: &str| fields.iter().find(|f| f.name == n);
    let mut notes = Vec::new();
    let mut note = |level, field, text: String| notes.push(Note { level, field, text });

    let spine = numbers(get("spine").and_then(|f| f.effective.as_ref()));
    let outline = numbers(get("crossSection").and_then(|f| f.effective.as_ref()));
    let n_spine = spine.as_ref().map(|s| s.len());
    if let Some(n) = n_spine.filter(|n| *n < 2) {
        note(
            Level::Error,
            "spine",
            format!("The spine has {n} point(s); VRML97 needs at least 2, so nothing is drawn."),
        );
    }
    if let Some(s) = spine.as_ref().filter(|s| is_closed(s) && s.len() > 2) {
        note(
            Level::Info,
            "spine",
            format!("The spine is closed (point 1 equals point {}).", s.len()),
        );
    }
    let mut outline_closed = false;
    if let Some(o) = &outline {
        if o.len() < 2 {
            note(
                Level::Error,
                "crossSection",
                format!(
                    "The outline has {} point(s); VRML97 needs at least 2.",
                    o.len()
                ),
            );
        } else {
            outline_closed = is_closed(o);
            let pts = if outline_closed {
                &o[..o.len() - 1]
            } else {
                &o[..]
            };
            let winding = if pts.len() >= 3 {
                let a = signed_area2(pts);
                if a > 0.0 {
                    ", clockwise on screen (x right, z down)"
                } else if a < 0.0 {
                    ", counter-clockwise on screen (x right, z down)"
                } else {
                    ", zero area"
                }
            } else {
                ""
            };
            note(
                Level::Info,
                "crossSection",
                format!(
                    "{} outline, {} point(s){winding}.",
                    if outline_closed { "Closed" } else { "Open" },
                    o.len()
                ),
            );
        }
    }
    for f in ["scale", "orientation"] {
        let (Some(k), Some(n)) = (
            numbers(get(f).and_then(|x| x.effective.as_ref())).map(|v| v.len()),
            n_spine,
        ) else {
            continue;
        };
        if k != 1 && k != n {
            note(
                Level::Warning,
                f,
                format!(
                    "{f} has {k} value(s) for {n} spine point(s); VRML97 expects 1 value or one per spine point."
                ),
            );
        }
    }
    if let Some(o) = numbers(get("orientation").and_then(|x| x.effective.as_ref())) {
        let off = o
            .iter()
            .filter(|r| {
                let len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
                (len - 1.0).abs() > 1e-6
            })
            .count();
        if off > 0 {
            note(
                Level::Info,
                "orientation",
                format!("{off} axis value(s) are not unit length; browsers normalize them. WRL Forge keeps them as written."),
            );
        }
    }
    for f in &fields {
        if let Some(p) = f.problem {
            note(
                Level::Warning,
                f.name,
                format!("{} is read-only here: {p}.", f.name),
            );
        }
    }
    Some(Info {
        fields,
        notes,
        outline_closed,
    })
}

/// The Extrusion view of the ONE node at exactly `[from, to)`, or `None`
/// (not an Extrusion, or no single node there).
pub fn inspect_at(p: &crate::parser::ParseResult, src: &str, from: u64, to: u64) -> Option<Info> {
    match crate::field_edit::nodes_at(p, from, to).as_slice() {
        [one] => inspect(one.node, src),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn info(src: &str) -> Info {
        let p = parse(src);
        let Some(Ast::Node(n)) = p.tree.statements.first() else {
            panic!("no node")
        };
        inspect(n, src).unwrap()
    }

    #[test]
    fn authored_and_default_values_are_kept_apart() {
        let i = info("#VRML V2.0 utf8\nExtrusion { spine [ 0 0 0, 0 2 0, 0 4 0 ] scale [ 1 1, 2 2 ] ccw FALSE }\n");
        let f = |n: &str| i.fields.iter().find(|f| f.name == n).unwrap().clone();
        assert!(f("spine").authored && f("ccw").authored && !f("solid").authored);
        assert_eq!(f("ccw").effective, Some(Effective::Single("FALSE".into())));
        assert_eq!(f("solid").effective, Some(Effective::Single("TRUE".into())));
        assert_eq!(
            f("orientation").effective,
            Some(Effective::List(vec![vec![
                "0".into(),
                "0".into(),
                "1".into(),
                "0".into()
            ]]))
        );
        assert!(!f("crossSection").authored);
        assert!(i.outline_closed);
        assert!(i
            .notes
            .iter()
            .any(|n| n.field == "scale" && n.level == Level::Warning));
        assert!(i.notes.iter().any(|n| n
            .text
            .starts_with("Closed outline, 5 point(s), counter-clockwise")));
    }

    #[test]
    fn counts_axes_and_unreadable_values_are_reported() {
        let i = info("#VRML V2.0 utf8\nExtrusion { spine [ 0 0 0 ] crossSection [ 0 0, 1 0, 1 1 ] orientation [ 0 0 2 1 ] creaseAngle [ 1 ] }\n");
        assert!(i
            .notes
            .iter()
            .any(|n| n.field == "spine" && n.level == Level::Error));
        assert!(i
            .notes
            .iter()
            .any(|n| n.text.starts_with("Open outline, 3 point(s), clockwise")));
        assert!(i
            .notes
            .iter()
            .any(|n| n.field == "orientation" && n.text.contains("not unit length")));
        let ca = i.fields.iter().find(|f| f.name == "creaseAngle").unwrap();
        assert!(ca.authored && ca.effective.is_none() && ca.problem.is_some());
    }
}
