// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A / 4B direct manipulation: the translation, rotation and scale
//! handles of a top-level Transform, as exact-source edits.
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
//! * `plan_rotate` / `plan_scale` (VISUAL-4B) -- the edit for one rotation
//!   ring or one scale handle. Rotation is about a PARENT (= world, the
//!   Transform is top-level) axis through the pivot `T + C`: the new rotation
//!   is `Rot(axis, delta) · R`, composed as quaternions (never Euler angles).
//!   When the authored axis is already parallel to the ring axis only the
//!   angle token changes; otherwise the whole value is written, and every
//!   component whose value does not change keeps its exact lexeme. Scale
//!   multiplies one component (or all three) by a factor and writes only the
//!   components that change. The written text is proven to reproduce the
//!   intended transform; a zero, negative or imprecise result is refused.
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
    pub const NO_ROTATION: &str = "rotation-not-explicitly-authored";
    pub const ROTATION_READ_ONLY: &str = "rotation-not-editable";
    pub const NO_SCALE: &str = "scale-not-explicitly-authored";
    pub const SCALE_READ_ONLY: &str = "scale-not-editable";
    pub const SCALE_NOT_POSITIVE: &str = "scale-not-positive";
    pub const FACTOR_INVALID: &str = "scale-factor-invalid";
    pub const RESULT_SINGULAR: &str = "result-would-be-singular";
    pub const RESULT_IMPRECISE: &str = "result-not-representable";
    /// B3: a Transform in a PROTO body whose parent frame is not provably
    /// the instance's own frame (nested under a transforming node, in a
    /// nested PROTO, or driven through `IS`).
    pub const BODY_FRAME: &str = "transform-not-at-proto-instance-frame";
}

/// Which manipulation the target is asked for: each tool needs ITS field
/// explicitly authored (a missing field is never written silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move,
    Rotate,
    Scale,
}

impl Tool {
    pub fn field(self) -> &'static str {
        match self {
            Tool::Move => "translation",
            Tool::Rotate => "rotation",
            Tool::Scale => "scale",
        }
    }
    fn missing(self) -> &'static str {
        match self {
            Tool::Move => reason::NO_TRANSLATION,
            Tool::Rotate => reason::NO_ROTATION,
            Tool::Scale => reason::NO_SCALE,
        }
    }
    fn read_only(self) -> &'static str {
        match self {
            Tool::Move => reason::TRANSLATION_READ_ONLY,
            Tool::Rotate => reason::ROTATION_READ_ONLY,
            Tool::Scale => reason::SCALE_READ_ONLY,
        }
    }
    fn field_type(self) -> &'static str {
        match self {
            Tool::Rotate => "SFRotation",
            _ => "SFVec3f",
        }
    }
    fn verb(self) -> &'static str {
        match self {
            Tool::Move => "moved",
            Tool::Rotate => "rotated",
            Tool::Scale => "scaled",
        }
    }
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
    pub fn unit(self) -> [f64; 3] {
        let mut v = [0.0; 3];
        v[self.index()] = 1.0;
        v
    }
}

/// A Transform the gizmo may move.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub node_from: u64,
    pub node_to: u64,
    pub def: Option<String>,
    /// The node's position among the top-level node statements (the
    /// preview's root nodes). `None` for a PROTO body Transform (B3): it has
    /// one runtime node per instance, none of them a root node.
    pub root_index: Option<usize>,
    /// B3: the Transform is part of the BODY of this shared PROTO
    /// definition; a commit edits every instance and needs consent.
    pub shared: Option<field_edit::SharedDefinition>,
    /// Index of the tool's field (`translation`, `rotation` or `scale`) in
    /// `node.fields`.
    pub field_index: usize,
    /// The effective frame: authored values, or ISO defaults when absent.
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub scale: [f64; 3],
    pub scale_orientation: [f64; 4],
    pub center: [f64; 3],
    /// The exact source lexemes of the tool field's components.
    pub lexemes: Vec<String>,
    /// World position of the Transform's local origin (where the Move gizmo
    /// is drawn): `T + C + R·SR·S·SR⁻¹·(−C)`.
    pub origin: [f64; 3],
    /// World position of the rotation / scale pivot: the local `center`
    /// point, which every rotation and scale leaves fixed: `T + C`.
    pub pivot: [f64; 3],
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

/// Grouping nodes that add no coordinate transformation to their
/// `children` (ISO/IEC 14772-1 6.1 Anchor, 6.8 Collision, 6.21 Group).
const FRAME_NEUTRAL: [&str; 3] = ["Group", "Anchor", "Collision"];

/// B3: whether a node of `node_type` adds no coordinate transformation to
/// its `children` (the only ancestors a PROTO body Transform may have).
pub fn is_frame_neutral(node_type: &str) -> bool {
    FRAME_NEUTRAL.contains(&node_type)
}

/// Fields of a body Transform that may be bound with `IS` without changing
/// (or animating) its frame.
const IS_FRAME_NEUTRAL: [&str; 5] = [
    "children",
    "addChildren",
    "removeChildren",
    "bboxCenter",
    "bboxSize",
];

/// B3: the Transform at exactly `[from, to)` when it is a statement of the
/// BODY of a top-level PROTO, or reached from one only through the
/// `children` of frame-neutral grouping nodes whose DEF names (if any) are
/// never USEd. Its parent frame is then the instance's own frame. `Err` is
/// the plain-language reason.
fn body_frame_node(p: &ParseResult, from: u64, to: u64) -> Result<&Node, &'static str> {
    fn at(n: &Node, from: u64, to: u64) -> bool {
        n.range.start.offset as u64 == from && n.range.end.offset as u64 == to
    }
    fn contains(n: &Node, from: u64, to: u64) -> bool {
        (n.range.start.offset as u64) <= from && to <= n.range.end.offset as u64
    }
    /// The node at the span under `n` through neutral `children` only.
    fn down<'a>(
        p: &ParseResult,
        n: &'a Node,
        from: u64,
        to: u64,
    ) -> Result<&'a Node, &'static str> {
        if at(n, from, to) {
            return Ok(n);
        }
        if !FRAME_NEUTRAL.contains(&n.node_type.as_str()) {
            return Err("it is inside a node that changes its coordinate frame");
        }
        if let Some(d) = &n.def {
            if name_uses(p, d).1 > 0 {
                return Err("an enclosing node is USEd elsewhere");
            }
        }
        for f in &n.fields {
            let Ast::Field(fl) = f else { continue };
            let items: Vec<&Ast> = match fl.value.as_deref() {
                Some(Ast::Array { items, .. }) => items.iter().collect(),
                Some(v) => vec![v],
                None => vec![],
            };
            for it in items {
                if let Ast::Node(c) = it {
                    if contains(c, from, to) {
                        if fl.name != "children" {
                            return Err("it is not in a children field");
                        }
                        return down(p, c, from, to);
                    }
                }
            }
        }
        Err("its place in the PROTO body is not proven")
    }
    for s in &p.tree.statements {
        let Ast::Proto(pr) = s else { continue };
        for b in &pr.body {
            let Ast::Node(n) = b else { continue };
            if !contains(n, from, to) {
                continue;
            }
            let t = down(p, n, from, to)?;
            for f in &t.fields {
                if let Ast::Field(fl) = f {
                    if matches!(fl.value.as_deref(), Some(Ast::Is { .. }))
                        && !IS_FRAME_NEUTRAL.contains(&fl.name.as_str())
                    {
                        return Err("its frame is bound to the PROTO interface (IS)");
                    }
                }
            }
            return Ok(t);
        }
    }
    Err("it is not directly in the body of a top-level PROTO")
}

/// Whether the gizmo may move the node at `[from, to)`, and where it is.
pub fn translate_target(src: &str, from: u64, to: u64) -> Result<Target, Refusal> {
    manip_target(src, from, to, Tool::Move)
}

/// Whether `tool` may manipulate the node at `[from, to)`, and its frame.
pub fn manip_target(src: &str, from: u64, to: u64, tool: Tool) -> Result<Target, Refusal> {
    let p = parse(src);
    target_in(&p, src, from, to, tool)
}

fn target_in(
    p: &ParseResult,
    src: &str,
    from: u64,
    to: u64,
    tool: Tool,
) -> Result<Target, Refusal> {
    let verb = tool.verb();
    let fields = field_edit::inspect_node_fields(p, src, from, to);
    if !fields.editable {
        return Err(refuse(
            fields.reason,
            format!("This object cannot be {verb}: its source cannot be edited safely."),
        ));
    }
    if fields.node_type.as_deref() != Some("Transform") {
        return Err(refuse(
            reason::NOT_TRANSFORM,
            format!("Only a Transform can be {verb}. Select the object's Transform."),
        ));
    }
    // B3: a PROTO body Transform is manipulated as the shared definition.
    // Its parent frame must be the instance's own frame: a statement of the
    // body of a top-level PROTO, under frame-neutral grouping nodes only.
    let shared = fields.shared.clone();
    let (root_index, node) = if let Some(sh) = &shared {
        if sh.name_ambiguous {
            return Err(refuse(
                field_edit::reason::PROTO_NAME_AMBIGUOUS,
                format!("PROTO {} is declared more than once; which definition an instance uses is not proven.", sh.proto),
            ));
        }
        match body_frame_node(p, from, to) {
            Ok(n) => (None, n),
            Err(why) => {
                return Err(refuse(
                    reason::BODY_FRAME,
                    format!("This Transform in PROTO {} cannot be {verb} with the handles: {why}. Use the Inspector.", sh.proto),
                ))
            }
        }
    } else {
        let Some((i, node)) = top_level_at(p, from, to) else {
            return Err(refuse(
                reason::NOT_TOP_LEVEL,
                format!("Only a top-level Transform can be {verb} yet (a nested Transform needs its parent's coordinate frame). Use the Inspector."),
            ));
        };
        (Some(i), node)
    };
    let def = node.def.clone();
    if let Some(def) = &def {
        let (defs, uses, routes) = name_uses(p, def);
        if defs != 1 {
            return Err(refuse(
                reason::DEF_NOT_UNIQUE,
                format!("DEF {def} is defined {defs} times; the object would be ambiguous."),
            ));
        }
        if uses > 0 {
            return Err(refuse(
                reason::INSTANCED,
                format!(
                    "{def} is also USEd elsewhere; a change would change every copy. Use the Inspector."
                ),
            ));
        }
        if routes > 0 {
            return Err(refuse(
                reason::ROUTED,
                format!("{def} receives ROUTE events (it may be animated); its preview frame is not its source frame."),
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
    let name = tool.field();
    let tf = match named(name).as_slice() {
        [] => {
            return Err(refuse(
                tool.missing(),
                format!("This Transform has no authored {name} field. Write it first (Inspector: \"Write {name}\"), then use the handles."),
            ))
        }
        [one] => *one,
        _ => {
            return Err(refuse(
                tool.read_only(),
                format!("{name} is authored more than once."),
            ))
        }
    };
    let width = if tool == Tool::Rotate { 4 } else { 3 };
    if !tf.editable || tf.field_type != Some(tool.field_type()) || tf.components.len() != width {
        return Err(refuse(
            tool.read_only(),
            format!("{name} cannot be edited ({}).", tf.reason),
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
    if nums(tf).is_none() {
        return Err(refuse(tool.read_only(), format!("{name} is not numeric.")));
    }
    // The frame fields: authored ones must be readable; absent ones take
    // their ISO defaults.
    let frame = |name: &str, default: &[f64]| -> Result<Vec<f64>, Refusal> {
        match named(name).as_slice() {
            [] => Ok(default.to_vec()),
            [f] if f.editable => nums(f)
                .filter(|v| v.len() == default.len())
                .ok_or_else(|| refuse(reason::FRAME_UNREADABLE, format!("{name} cannot be read."))),
            _ => Err(refuse(
                reason::FRAME_UNREADABLE,
                format!("{name} cannot be read, so the object's frame is unknown."),
            )),
        }
    };
    let t = frame("translation", &[0.0, 0.0, 0.0])?;
    let c = frame("center", &[0.0, 0.0, 0.0])?;
    let r = frame("rotation", &[0.0, 0.0, 1.0, 0.0])?;
    let s = frame("scale", &[1.0, 1.0, 1.0])?;
    let sr = frame("scaleOrientation", &[0.0, 0.0, 1.0, 0.0])?;
    let translation = [t[0], t[1], t[2]];
    let center = [c[0], c[1], c[2]];
    let rotation = [r[0], r[1], r[2], r[3]];
    let scale = [s[0], s[1], s[2]];
    let scale_orientation = [sr[0], sr[1], sr[2], sr[3]];
    let origin = origin_of(translation, center, rotation, scale, scale_orientation)
        .ok_or_else(|| refuse(reason::FRAME_UNREADABLE, "The rotation axis is zero."))?;
    if tool == Tool::Scale && scale.iter().any(|v| *v <= 0.0) {
        return Err(refuse(
            reason::SCALE_NOT_POSITIVE,
            "The authored scale has a zero or negative component (VRML97 requires every scale value > 0). Use the Inspector.",
        ));
    }
    let pivot = [0, 1, 2].map(|i| translation[i] + center[i]);
    if !pivot.iter().all(|v| v.is_finite()) {
        return Err(refuse(reason::FRAME_UNREADABLE, "The pivot is not finite."));
    }
    Ok(Target {
        node_from: from,
        node_to: to,
        def,
        root_index,
        shared,
        field_index: tf.index,
        translation,
        rotation,
        scale,
        scale_orientation,
        center,
        lexemes: tf.components.iter().map(|c| c.text.clone()).collect(),
        origin,
        pivot,
    })
}

/// B3: the target for a COMMIT: `manip_target`, then the field-edit node
/// gate with the caller's consent -- a shared PROTO definition is refused
/// without it (`SHARED_DEFINITION_UNCONFIRMED`), exactly as an Inspector
/// edit is.
fn commit_target(
    src: &str,
    from: u64,
    to: u64,
    tool: Tool,
    confirm_shared: bool,
) -> Result<Target, Refusal> {
    let p = parse(src);
    let target = target_in(&p, src, from, to, tool)?;
    if let Err(r) = field_edit::node_gate(&p, from, to, confirm_shared) {
        let message = match (r, &target.shared) {
            (field_edit::reason::SHARED_DEFINITION_UNCONFIRMED, Some(sh)) => format!(
                "This Transform is part of the shared definition PROTO {} ({} instance(s)); changing it changes every instance and was not confirmed.",
                sh.proto, sh.instances
            ),
            _ => format!("This object cannot be {}: its source cannot be edited safely.", tool.verb()),
        };
        return Err(refuse(r, message));
    }
    Ok(target)
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
    plan_translate_with(src, from, to, axis, value, decimals, false)
}

/// `plan_translate` with the user's consent to edit a shared PROTO
/// definition (B3). Without it a PROTO body Transform is refused.
pub fn plan_translate_with(
    src: &str,
    from: u64,
    to: u64,
    axis: Axis,
    value: f64,
    decimals: u8,
    confirm_shared: bool,
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
    let target = match commit_target(src, from, to, Tool::Move, confirm_shared) {
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
        confirm_shared,
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

// ---- VISUAL-4B: rotation and scale -----------------------------------------

/// Decimal places of a written rotation angle (radians): 0.0001 rad is
/// about 0.006 degrees.
pub const ANGLE_DECIMALS: u8 = 4;
/// Decimal places of a written rotation axis component.
pub const AXIS_DECIMALS: u8 = 6;
/// The largest difference between any element of the rotation matrix the
/// written text describes and the intended one.
const ROTATION_TOLERANCE: f64 = 5e-4;

type Quat = [f64; 4]; // w, x, y, z

fn quat_of(r: [f64; 4]) -> Option<Quat> {
    let [x, y, z, a] = r;
    if a == 0.0 {
        return Some([1.0, 0.0, 0.0, 0.0]);
    }
    let l = (x * x + y * y + z * z).sqrt();
    if !l.is_finite() || l <= 0.0 || !a.is_finite() {
        return None;
    }
    let (s, c) = (a / 2.0).sin_cos();
    Some([c, s * x / l, s * y / l, s * z / l])
}

fn quat_mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// `Rot(axis, delta) · old` as an axis-angle value (axis unit length, angle
/// in [0, π]); `None` if `old` is unreadable or a value is not finite. When
/// the result is the identity, `old`'s own axis is kept with angle 0.
pub fn rotate_about(old: [f64; 4], axis: Axis, delta: f64) -> Option<[f64; 4]> {
    let mut d = [0.0; 4];
    d[3] = delta;
    d[axis.index()] = 1.0;
    let q = quat_mul(quat_of(d)?, quat_of(old)?);
    let q = if q[0] < 0.0 { q.map(|v| -v) } else { q };
    let s = (q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let out = if s <= 1e-12 {
        let l = (old[0] * old[0] + old[1] * old[1] + old[2] * old[2]).sqrt();
        if l > 0.0 && l.is_finite() {
            [old[0] / l, old[1] / l, old[2] / l, 0.0]
        } else {
            let u = axis.unit();
            [u[0], u[1], u[2], 0.0]
        }
    } else {
        [q[1] / s, q[2] / s, q[3] / s, 2.0 * s.atan2(q[0])]
    };
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// The value written for one component: the old lexeme when the new value
/// equals the old one after rounding, else the rounded new value.
fn component_text(old_lexeme: &str, old: f64, new: f64, decimals: u8) -> Option<String> {
    let t = format_component(new, decimals)?;
    let v = t.parse::<f64>().ok()?;
    Some(if v == old { old_lexeme.to_string() } else { t })
}

/// Decimal places for a written scale value: four significant digits, at
/// most six places.
pub fn scale_decimals(v: f64) -> u8 {
    if !v.is_finite() || v <= 0.0 {
        return 6;
    }
    (3.0 - v.log10().floor()).clamp(0.0, 6.0) as u8
}

fn ready_plan(
    src: &str,
    target: &Target,
    texts: &[String],
    allowed: &[usize],
    confirm_shared: bool,
) -> (Plan, Option<String>) {
    let tool_field = if texts.len() == 4 {
        "rotation"
    } else {
        "scale"
    };
    if texts.iter().zip(&target.lexemes).all(|(a, b)| a == b) {
        return (Plan::Unchanged, None);
    }
    let req = Request {
        confirm_shared,
        node_from: target.node_from,
        node_to: target.node_to,
        field_index: target.field_index,
        field_name: tool_field.into(),
        components: texts.iter().map(|t| Input::Text(t.clone())).collect(),
    };
    let plan = field_edit::plan_field_edit(src, &req);
    if let Plan::Ready { changed, .. } = &plan {
        if changed.iter().any(|c| !allowed.contains(c)) {
            return (
                Plan::Refused {
                    reason: reason::OTHER_AXIS_CHANGED,
                    message: Some(
                        "The edit would change another component; nothing was changed.".into(),
                    ),
                    component_index: None,
                },
                None,
            );
        }
    }
    (plan, Some(texts.join(" ")))
}

fn refused_plan(reason: &'static str, message: impl Into<String>) -> (Plan, Option<String>) {
    (
        Plan::Refused {
            reason,
            message: Some(message.into()),
            component_index: None,
        },
        None,
    )
}

/// The exact-source edit rotating the Transform at `[from, to)` by `delta`
/// radians about the parent (world) `axis` through its pivot `T + C`:
/// `rotation := Rot(axis, delta) · rotation`. Returns the plan and the
/// written value text.
///
/// * The authored axis parallel to `axis` (the other two components exactly
///   0): only the angle token changes (`angle ± delta`, never wrapped).
/// * Otherwise the composed value is written (unit axis, angle in [0, π]);
///   each component whose value does not change keeps its exact lexeme.
///
/// The written text must reproduce the intended rotation matrix within
/// `ROTATION_TOLERANCE`, and its axis must not be zero.
pub fn plan_rotate(
    src: &str,
    from: u64,
    to: u64,
    axis: Axis,
    delta: f64,
) -> (Plan, Option<String>) {
    plan_rotate_with(src, from, to, axis, delta, false)
}

/// `plan_rotate` with consent to edit a shared PROTO definition (B3).
pub fn plan_rotate_with(
    src: &str,
    from: u64,
    to: u64,
    axis: Axis,
    delta: f64,
    confirm_shared: bool,
) -> (Plan, Option<String>) {
    let target = match commit_target(src, from, to, Tool::Rotate, confirm_shared) {
        Ok(t) => t,
        Err(r) => return refused_plan(r.reason, r.message),
    };
    if !delta.is_finite() {
        return refused_plan(
            reason::VALUE_NOT_FINITE,
            "The rotation angle is not finite.",
        );
    }
    let old = target.rotation;
    let i = axis.index();
    let others_zero = (0..3).filter(|k| *k != i).all(|k| old[k] == 0.0);
    let parallel = others_zero && old[i] != 0.0;
    let mut allowed = vec![3];
    let texts: Option<Vec<String>> = if parallel {
        let sign = old[i].signum();
        let mut t: Vec<String> = target.lexemes[..3].to_vec();
        let angle = component_text(
            &target.lexemes[3],
            old[3],
            old[3] + sign * delta,
            ANGLE_DECIMALS,
        );
        angle.map(|a| {
            t.push(a);
            t
        })
    } else {
        allowed = vec![0, 1, 2, 3];
        rotate_about(old, axis, delta).and_then(|new| {
            (0..4)
                .map(|k| {
                    let d = if k == 3 {
                        ANGLE_DECIMALS
                    } else {
                        AXIS_DECIMALS
                    };
                    component_text(&target.lexemes[k], old[k], new[k], d)
                })
                .collect()
        })
    };
    let Some(texts) = texts else {
        return refused_plan(reason::VALUE_NOT_FINITE, "The new rotation is not finite.");
    };
    let Some(written) = texts
        .iter()
        .map(|t| t.parse::<f64>().ok())
        .collect::<Option<Vec<f64>>>()
    else {
        return refused_plan(
            reason::VALUE_NOT_FINITE,
            "The new rotation is not a number.",
        );
    };
    if written[..3].iter().all(|v| *v == 0.0) {
        return refused_plan(
            reason::RESULT_SINGULAR,
            "The new rotation axis would be 0 0 0.",
        );
    }
    // Proof: the written value is the intended rotation.
    let intended = rotate_about(old, axis, delta);
    let close = match (
        intended.and_then(|r| rotation([r[0], r[1], r[2]], r[3])),
        rotation([written[0], written[1], written[2]], written[3]),
    ) {
        (Some(a), Some(b)) => {
            (0..3).all(|r| (0..3).all(|c| (a[r][c] - b[r][c]).abs() <= ROTATION_TOLERANCE))
        }
        _ => false,
    };
    if !close {
        return refused_plan(
            reason::RESULT_IMPRECISE,
            "The rotation cannot be written precisely enough; nothing was changed.",
        );
    }
    ready_plan(src, &target, &texts, &allowed, confirm_shared)
}

/// The exact-source edit scaling the Transform at `[from, to)` by `factor`
/// along its scale `axis` (one component), or uniformly (`None`, all three).
/// The pivot `T + C` stays fixed. Each new value is rounded to four
/// significant digits; components that do not change keep their lexemes.
/// A non-positive, non-finite or singular result is refused.
pub fn plan_scale(
    src: &str,
    from: u64,
    to: u64,
    axis: Option<Axis>,
    factor: f64,
) -> (Plan, Option<String>) {
    plan_scale_with(src, from, to, axis, factor, false)
}

/// `plan_scale` with consent to edit a shared PROTO definition (B3).
pub fn plan_scale_with(
    src: &str,
    from: u64,
    to: u64,
    axis: Option<Axis>,
    factor: f64,
    confirm_shared: bool,
) -> (Plan, Option<String>) {
    let target = match commit_target(src, from, to, Tool::Scale, confirm_shared) {
        Ok(t) => t,
        Err(r) => return refused_plan(r.reason, r.message),
    };
    if !factor.is_finite() || factor <= 0.0 {
        return refused_plan(
            reason::FACTOR_INVALID,
            "The scale factor must be a finite number greater than 0.",
        );
    }
    let comps: Vec<usize> = axis.map_or(vec![0, 1, 2], |a| vec![a.index()]);
    let mut texts = target.lexemes.clone();
    for &k in &comps {
        let old = target.scale[k];
        let new = old * factor;
        let Some(t) = component_text(&target.lexemes[k], old, new, scale_decimals(new)) else {
            return refused_plan(reason::VALUE_NOT_FINITE, "The new scale is not finite.");
        };
        match t.parse::<f64>() {
            Ok(v) if v > 0.0 && v <= MAX_COMPONENT => {}
            Ok(v) if v > MAX_COMPONENT => {
                return refused_plan(
                    reason::VALUE_OUT_OF_RANGE,
                    format!("{t} is outside the SFVec3f range."),
                )
            }
            _ => {
                return refused_plan(
                    reason::RESULT_SINGULAR,
                    "The new scale would be 0 (a singular transform); nothing was changed.",
                )
            }
        }
        texts[k] = t;
    }
    ready_plan(src, &target, &texts, &comps, confirm_shared)
}

#[cfg(test)]
mod tests;
