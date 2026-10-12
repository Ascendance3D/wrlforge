// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed reads of authored field values. A field authored twice, bound with
//! `IS`, of the wrong shape or with an invalid/non-finite number is an error:
//! the caller does not draw the node (never a guess at the "intended" value).

use wrlforge_vrml::ast::{Ast, Node};

pub type R<T> = Result<T, &'static str>;

/// The one value of field `name`, `None` when not authored.
pub fn field<'a>(n: &'a Node, name: &str) -> R<Option<&'a Ast>> {
    let mut found = None;
    for f in &n.fields {
        let Ast::Field(f) = f else { continue };
        if f.name != name {
            continue;
        }
        if found.is_some() {
            return Err("duplicate-field");
        }
        if f.is_binding {
            return Err("is-binding");
        }
        found = Some(f.value.as_deref().ok_or("field-without-value")?);
    }
    Ok(found)
}

fn numbers(v: &Ast, n: usize) -> R<Vec<f64>> {
    let Ast::Numbers { values, .. } = v else { return Err("value-shape") };
    if values.len() != n {
        return Err("value-shape");
    }
    values
        .iter()
        .map(|x| if x.valid && x.value.is_finite() { Ok(x.value) } else { Err("value-invalid") })
        .collect()
}

pub fn f(n: &Node, name: &str, default: f64) -> R<f64> {
    match field(n, name)? {
        None => Ok(default),
        Some(v) => Ok(numbers(v, 1)?[0]),
    }
}

pub fn vec3(n: &Node, name: &str, default: [f64; 3]) -> R<[f64; 3]> {
    match field(n, name)? {
        None => Ok(default),
        Some(v) => {
            let x = numbers(v, 3)?;
            Ok([x[0], x[1], x[2]])
        }
    }
}

pub fn rotation(n: &Node, name: &str) -> R<[f64; 4]> {
    match field(n, name)? {
        None => Ok([0.0, 0.0, 1.0, 0.0]),
        Some(v) => {
            let x = numbers(v, 4)?;
            Ok([x[0], x[1], x[2], x[3]])
        }
    }
}

pub fn boolean(n: &Node, name: &str, default: bool) -> R<bool> {
    match field(n, name)? {
        None => Ok(default),
        Some(Ast::Bool { value, .. }) => Ok(*value),
        Some(_) => Err("value-shape"),
    }
}

/// SFColor: three components, each in [0, 1] (ISO 5.5).
pub fn color(n: &Node, name: &str, default: [f32; 3]) -> R<[f32; 3]> {
    match field(n, name)? {
        None => Ok(default),
        Some(v) => {
            let x = numbers(v, 3)?;
            if x.iter().any(|c| !(0.0..=1.0).contains(c)) {
                return Err("value-out-of-range");
            }
            Ok([x[0] as f32, x[1] as f32, x[2] as f32])
        }
    }
}

/// An SFFloat that must be in [0, 1].
pub fn unit(n: &Node, name: &str, default: f32) -> R<f32> {
    let v = f(n, name, default as f64)?;
    if !(0.0..=1.0).contains(&v) {
        return Err("value-out-of-range");
    }
    Ok(v as f32)
}

/// An SFFloat that must be > 0 (primitive dimensions).
pub fn positive(n: &Node, name: &str, default: f64) -> R<f64> {
    let v = f(n, name, default)?;
    if v <= 0.0 {
        return Err("value-out-of-range");
    }
    Ok(v)
}

/// The child nodes of an SFNode/MFNode field: authored nodes, plus the USE
/// statements (returned separately: never drawn in NATIVE-RENDER-1).
pub enum Child<'a> {
    Node(&'a Node),
    Use { span: (u64, u64) },
}

pub fn children<'a>(n: &'a Node, name: &str) -> R<Vec<Child<'a>>> {
    let one = |a: &'a Ast| -> Option<Child<'a>> {
        match a {
            Ast::Node(x) => Some(Child::Node(x)),
            Ast::Use { range, .. } => Some(Child::Use { span: (range.start.offset as u64, range.end.offset as u64) }),
            _ => None,
        }
    };
    Ok(match field(n, name)? {
        None | Some(Ast::Null { .. }) => Vec::new(),
        Some(Ast::Array { items, .. }) => items.iter().filter_map(one).collect(),
        Some(a) => one(a).into_iter().collect(),
    })
}

pub fn span(n: &Node) -> (u64, u64) {
    (n.range.start.offset as u64, n.range.end.offset as u64)
}
