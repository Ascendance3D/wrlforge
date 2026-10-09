// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A direct manipulation: the translation handle of a top-level
//! Transform, as exact-source edits.
//!
//! Two questions, both answered from ONE parse of the current text:
//!
//! * `translate_target` -- may this node be moved by the translation gizmo,
//!   and where is it? The node is named by its exact UTF-16 span (the Scene
//!   Tree / pick identity of one revision). It must be a standard `Transform`
//!   that is a TOP-LEVEL statement (its parent frame is the world, so world
//!   axes are its own translation axes), carries an explicit, editable SFVec3f
//!   `translation`. If it has a DEF name, that name must be defined once in
//!   the whole file, never USEd and never a ROUTE destination (an instanced or
//!   animated node's rendered position is not its source position).
//!
//!   The live preview finds the rendered node by its TOP-LEVEL STATEMENT
//!   INDEX (`root_index`): the VRML97 parser appends every top-level node
//!   statement (a node, a `DEF`, a `USE`, `NULL`) to the scene's root nodes in
//!   source order, and PROTO / EXTERNPROTO / ROUTE statements add none. The
//!   preview renders exactly this text, so the index names this node.
//! * `plan_translate` -- the edit for one axis. It goes through the typed
//!   field-edit planner (`field_edit::plan_field_edit`): the other two
//!   components are sent as their exact current lexemes, so only the moved
//!   component's token can change, and the planner's round-trip proof
//!   (same node, same field, intended values) applies unchanged.
//!
//! Design reference (conceptually informed only, no code taken): White Dune's
//! `Node::getHandle()` reports a handle position for a field and
//! `Node::setHandle()` turns a handle position into a new field value. Here
//! the "handle" is one translation axis and the new value becomes a source
//! token edit, never a write into a scene model.

use crate::ast::{Ast, Node};
use crate::field_edit::{self, ComponentValue, Input, Plan, Request};
use crate::parser::{parse, ParseResult};

pub mod reason {
    pub const NOT_TRANSFORM: &str = "node-is-not-a-transform";
    pub const NOT_TOP_LEVEL: &str = "transform-is-not-top-level";
    pub const DEF_NOT_UNIQUE: &str = "def-name-not-unique";
    pub const INSTANCED: &str = "transform-is-used-elsewhere";
    pub const ROUTED: &str = "transform-is-a-route-destination";
    pub const NO_TRANSLATION: &str = "translation-not-explicitly-authored";
    pub const TRANSLATION_READ_ONLY: &str = "translation-not-editable";
    pub const FRAME_UNREADABLE: &str = "transform-frame-not-readable";
    pub const VALUE_NOT_FINITE: &str = "value-not-finite";
    pub const VALUE_OUT_OF_RANGE: &str = "value-out-of-range";
    pub const OTHER_AXIS_CHANGED: &str = "edit-changes-another-axis";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
}

/// A Transform the gizmo may move.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub node_from: u64,
    pub node_to: u64,
    pub def: Option<String>,
    /// The node's position among the top-level node statements (the
    /// preview's root nodes).
    pub root_index: usize,
    /// Index of the `translation` field in `node.fields`.
    pub field_index: usize,
    pub translation: [f64; 3],
    /// The exact source lexemes of the three components.
    pub lexemes: [String; 3],
    /// World position of the Transform's local origin (where the gizmo is
    /// drawn): `T + C + R·SR·S·SR⁻¹·(−C)`.
    pub origin: [f64; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub reason: &'static str,
    pub message: String,
}

fn refuse(reason: &'static str, message: impl Into<String>) -> Refusal {
    Refusal {
        reason,
        message: message.into(),
    }
}

/// Visit every AST item, including PROTO bodies and interface defaults.
fn visit<'a>(a: &'a Ast, f: &mut dyn FnMut(&'a Ast)) {
    f(a);
    match a {
        Ast::Node(n) => {
            for i in &n.interfaces {
                if let Some(d) = &i.default {
                    visit(d, f);
                }
            }
            n.fields.iter().for_each(|x| visit(x, f));
        }
        Ast::Field(fl) => {
            if let Some(v) = &fl.value {
                visit(v, f);
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| visit(i, f)),
        Ast::Proto(p) => {
            for i in &p.interfaces {
                if let Some(d) = &i.default {
                    visit(d, f);
                }
            }
            p.body.iter().for_each(|b| visit(b, f));
        }
        Ast::ExternProto(e) => {
            for i in &e.interfaces {
                if let Some(d) = &i.default {
                    visit(d, f);
                }
            }
        }
        _ => {}
    }
}

/// (DEFs of `name`, USEs of `name`, ROUTEs into `name`) anywhere in the file.
/// Counted strictly: a PROTO body's own scope is not exempted.
fn name_uses(p: &ParseResult, name: &str) -> (usize, usize, usize) {
    let (mut defs, mut uses, mut routes) = (0, 0, 0);
    for s in &p.tree.statements {
        visit(s, &mut |a| match a {
            Ast::Node(n) if n.def.as_deref() == Some(name) => defs += 1,
            Ast::Use { name: Some(u), .. } if u == name => uses += 1,
            Ast::Route(r) if r.to.node.as_deref() == Some(name) => routes += 1,
            _ => {}
        });
    }
    (defs, uses, routes)
}

/// The top-level node at exactly `[from, to)` and its index among the
/// top-level NODE statements (node, USE, NULL), counted in source order.
fn top_level_at(p: &ParseResult, from: u64, to: u64) -> Option<(usize, &Node)> {
    let mut index = 0;
    for s in &p.tree.statements {
        match s {
            Ast::Node(n)
                if n.range.start.offset as u64 == from && n.range.end.offset as u64 == to =>
            {
                return Some((index, n));
            }
            Ast::Node(_) | Ast::Use { .. } | Ast::Null { .. } => index += 1,
            _ => {}
        }
    }
    None
}

type M3 = [[f64; 3]; 3];

fn rotation(axis: [f64; 3], angle: f64) -> Option<M3> {
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if angle == 0.0 {
        return Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }
    if !l.is_finite() || l <= 0.0 {
        return None;
    }
    let [x, y, z] = axis.map(|v| v / l);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    Some([
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ])
}

fn apply(m: &M3, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

fn transpose(m: &M3) -> M3 {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| m[c][r]))
}

/// The world position of the Transform's local origin from its authored
/// (or default) frame fields: `T + C + R·SR·S·SR⁻¹·(−C)` (ISO 14772-1 6.52).
pub fn origin_of(
    t: [f64; 3],
    c: [f64; 3],
    r: [f64; 4],
    s: [f64; 3],
    sr: [f64; 4],
) -> Option<[f64; 3]> {
    let rm = rotation([r[0], r[1], r[2]], r[3])?;
    let srm = rotation([sr[0], sr[1], sr[2]], sr[3])?;
    let neg_c = c.map(|v| -v);
    let a = apply(&transpose(&srm), neg_c);
    let b = [a[0] * s[0], a[1] * s[1], a[2] * s[2]];
    let d = apply(&rm, apply(&srm, b));
    let o = [0, 1, 2].map(|i| t[i] + c[i] + d[i]);
    o.iter().all(|v| v.is_finite()).then_some(o)
}

/// Whether the gizmo may move the node at `[from, to)`, and where it is.
pub fn translate_target(src: &str, from: u64, to: u64) -> Result<Target, Refusal> {
    let p = parse(src);
    target_in(&p, src, from, to)
}

fn target_in(p: &ParseResult, src: &str, from: u64, to: u64) -> Result<Target, Refusal> {
    let fields = field_edit::inspect_node_fields(p, src, from, to);
    if !fields.editable {
        return Err(refuse(
            fields.reason,
            "This object cannot be moved: its source cannot be edited safely.",
        ));
    }
    if fields.node_type.as_deref() != Some("Transform") {
        return Err(refuse(
            reason::NOT_TRANSFORM,
            "Only a Transform can be moved. Select the object's Transform.",
        ));
    }
    let Some((root_index, node)) = top_level_at(p, from, to) else {
        return Err(refuse(
            reason::NOT_TOP_LEVEL,
            "Only a top-level Transform can be moved yet (a nested Transform needs its parent's coordinate frame). Use the Inspector.",
        ));
    };
    let def = node.def.clone();
    if let Some(def) = &def {
        let (defs, uses, routes) = name_uses(p, def);
        if defs != 1 {
            return Err(refuse(
                reason::DEF_NOT_UNIQUE,
                format!("DEF {def} is defined {defs} times; the moved object would be ambiguous."),
            ));
        }
        if uses > 0 {
            return Err(refuse(
                reason::INSTANCED,
                format!(
                    "{def} is also USEd elsewhere; moving it would move every copy. Use the Inspector."
                ),
            ));
        }
        if routes > 0 {
            return Err(refuse(
                reason::ROUTED,
                format!("{def} receives ROUTE events (it may be animated); its preview position is not its source position."),
            ));
        }
    }
    let named = |name: &str| {
        fields
            .fields
            .iter()
            .filter(|f| f.name == name)
            .collect::<Vec<_>>()
    };
    let tr = named("translation");
    let tr =
        match tr.as_slice() {
            [] => return Err(refuse(
                reason::NO_TRANSLATION,
                "This Transform has no authored translation field. Add one in the source first.",
            )),
            [one] => *one,
            _ => {
                return Err(refuse(
                    reason::TRANSLATION_READ_ONLY,
                    "translation is authored more than once.",
                ))
            }
        };
    if !tr.editable || tr.field_type != Some("SFVec3f") || tr.components.len() != 3 {
        return Err(refuse(
            reason::TRANSLATION_READ_ONLY,
            format!("translation cannot be edited ({}).", tr.reason),
        ));
    }
    let nums = |f: &field_edit::FieldDescriptor| -> Option<Vec<f64>> {
        f.components
            .iter()
            .map(|c| match c.value {
                ComponentValue::Num(v) if v.is_finite() => Some(v),
                _ => None,
            })
            .collect()
    };
    // The other frame fields: authored ones must be readable; absent ones
    // take their ISO defaults.
    let frame = |name: &str, default: &[f64]| -> Result<Vec<f64>, Refusal> {
        match named(name).as_slice() {
            [] => Ok(default.to_vec()),
            [f] if f.editable => nums(f)
                .filter(|v| v.len() == default.len())
                .ok_or_else(|| refuse(reason::FRAME_UNREADABLE, format!("{name} cannot be read."))),
            _ => Err(refuse(
                reason::FRAME_UNREADABLE,
                format!("{name} cannot be read, so the object's origin is unknown."),
            )),
        }
    };
    let t = nums(tr)
        .ok_or_else(|| refuse(reason::TRANSLATION_READ_ONLY, "translation is not numeric."))?;
    let c = frame("center", &[0.0, 0.0, 0.0])?;
    let r = frame("rotation", &[0.0, 0.0, 1.0, 0.0])?;
    let s = frame("scale", &[1.0, 1.0, 1.0])?;
    let sr = frame("scaleOrientation", &[0.0, 0.0, 1.0, 0.0])?;
    let translation = [t[0], t[1], t[2]];
    let origin = origin_of(
        translation,
        [c[0], c[1], c[2]],
        [r[0], r[1], r[2], r[3]],
        [s[0], s[1], s[2]],
        [sr[0], sr[1], sr[2], sr[3]],
    )
    .ok_or_else(|| refuse(reason::FRAME_UNREADABLE, "The rotation axis is zero."))?;
    Ok(Target {
        node_from: from,
        node_to: to,
        def,
        root_index,
        field_index: tr.index,
        translation,
        lexemes: [0, 1, 2].map(|i| tr.components[i].text.clone()),
        origin,
    })
}

/// A finite value as a VRML97 float token with at most `decimals` places:
/// trailing zeros removed, never `-0`, never an exponent.
pub fn format_component(value: f64, decimals: u8) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let d = decimals.min(6) as usize;
    let mut s = format!("{value:.d$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    Some(s)
}

/// The largest magnitude an SFVec3f component may hold (single precision).
const MAX_COMPONENT: f64 = f32::MAX as f64;

/// The exact-source edit moving the Transform at `[from, to)` so that its
/// translation's `axis` component becomes `value` (rounded to `decimals`
/// places). Every other byte is unchanged; the other two components are
/// re-sent as their exact lexemes and so never change. Returns the plan and
/// the token text written.
pub fn plan_translate(
    src: &str,
    from: u64,
    to: u64,
    axis: Axis,
    value: f64,
    decimals: u8,
) -> (Plan, Option<String>) {
    let refused = |reason: &'static str, message: String| {
        (
            Plan::Refused {
                reason,
                message: Some(message),
                component_index: Some(axis.index()),
            },
            None,
        )
    };
    let target = match translate_target(src, from, to) {
        Ok(t) => t,
        Err(r) => {
            return (
                Plan::Refused {
                    reason: r.reason,
                    message: Some(r.message),
                    component_index: None,
                },
                None,
            )
        }
    };
    let Some(text) = format_component(value, decimals) else {
        return refused(
            reason::VALUE_NOT_FINITE,
            "The new position is not a finite number.".into(),
        );
    };
    let Ok(parsed) = text.parse::<f64>() else {
        return refused(
            reason::VALUE_NOT_FINITE,
            format!("\"{text}\" is not a number."),
        );
    };
    if parsed.abs() > MAX_COMPONENT {
        return refused(
            reason::VALUE_OUT_OF_RANGE,
            format!("{text} is outside the SFVec3f range."),
        );
    }
    let i = axis.index();
    // A zero-distance drag (same value after rounding) changes nothing, even
    // if the authored lexeme is spelled differently ("1.50" vs "1.5").
    if parsed == target.translation[i] {
        return (Plan::Unchanged, None);
    }
    let req = Request {
        node_from: from,
        node_to: to,
        field_index: target.field_index,
        field_name: "translation".into(),
        components: (0..3)
            .map(|k| {
                Input::Text(if k == i {
                    text.clone()
                } else {
                    target.lexemes[k].clone()
                })
            })
            .collect(),
    };
    let plan = field_edit::plan_field_edit(src, &req);
    if let Plan::Ready { changed, .. } = &plan {
        if changed.as_slice() != [i] {
            return refused(
                reason::OTHER_AXIS_CHANGED,
                "The edit would change another axis; nothing was changed.".into(),
            );
        }
    }
    (plan, Some(text))
}

#[cfg(test)]
mod tests;
