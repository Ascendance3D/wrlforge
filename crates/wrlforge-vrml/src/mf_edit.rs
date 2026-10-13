// SPDX-License-Identifier: GPL-3.0-or-later
//! Multi-value field editing (VISUAL-4A): `MFVec2f`, `MFVec3f` and
//! `MFRotation` lists, as exact source-text patches.
//!
//! THE DOCUMENT IS THE TEXT (WD.md section 2). A list is read from the parse
//! tree as tuples of NUMBER tokens with their exact spans. An edit changes
//! only the tokens a value names, or inserts / removes one tuple and the
//! separator next to it. Nothing re-prints the list, normalizes a number,
//! reorders points, closes or opens an outline, or writes a default.
//!
//! Identity and gates are the field-edit ones (`field_edit::node_gate`):
//! the node is named by its exact UTF-16 span in ONE revision; exactly one
//! node must sit there, outside every PROTO / EXTERNPROTO / interface scope,
//! in a document without blocking syntax errors. The field must be authored
//! exactly once, not IS-bound, a VRML97 `field` / `exposedField` of one of
//! the three list types.
//!
//! Accepted value forms (ISO/IEC 14772-1 5.1 / 5.8):
//!
//! * `[ ]` -- an empty list;
//! * `[ a b, c d, ... ]` -- a bracketed list of NUMBER tokens whose count is a
//!   multiple of the tuple size (commas are whitespace);
//! * `a b` -- one bare tuple (a single value written without brackets).
//!
//! Anything else (nested brackets, strings, a bare run of more than one
//! tuple, a malformed or non-finite number) is refused with a reason id.
//!
//! An edit request is a SEQUENCE of operations (one Inspector action, or one
//! whole outline-editing session). Each operation is planned against the
//! text the previous one produced, re-parsed and re-proven; the result is
//! then reduced to ONE span edit (the exact differing region) that must lie
//! inside the original value. The final parse must show the same node at the
//! same start, the same field at the same index, and exactly the intended
//! lexemes. One request is one transaction, so one undo step.

use crate::ast::{Ast, Field, Node};
use crate::field_edit::{
    byte_at, duplicate_names, encode_number, has_blocking_syntax_error, node_gate, nodes_at,
    reason as fr, splice, utf16_len, Bounds, Kind, SpanEdit, TypeSpec,
};
use crate::node_schema;
use crate::parser::{parse, ParseResult};
use crate::tokenizer::{Numeric, TokKind};

/// Stable reason ids of this module (the field-edit ids are reused too).
pub mod reason {
    pub const NOT_A_LIST_TYPE: &str = "field-type-not-an-editable-list";
    pub const LIST_FORM: &str = "list-form-not-supported";
    pub const INDEX: &str = "list-index-out-of-range";
    pub const COUNT_CHANGED: &str = "list-count-changed";
    pub const POINT_HAS_COMMENT: &str = "list-point-contains-a-comment";
    pub const ZERO_AXIS: &str = "rotation-axis-is-zero";
    pub const TOO_FEW: &str = "list-would-have-too-few-values";
    pub const WOULD_OPEN: &str = "outline-would-open";
    pub const NO_OPS: &str = "no-operations";
    pub const FIELD_ALREADY_AUTHORED: &str = "field-already-authored";
    pub const FIELD_NOT_ADDABLE: &str = "field-cannot-be-added";
    pub const NO_BODY: &str = "node-body-not-proven";
    pub const SCOPE: &str = "edit-outside-the-value";
}

/// The list shape of a field type, or `None` for other types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListSpec {
    pub labels: &'static [&'static str],
    pub rotation: bool,
}

impl ListSpec {
    pub fn arity(&self) -> usize {
        self.labels.len()
    }
}

pub fn list_spec(field_type: &str) -> Option<ListSpec> {
    Some(match field_type {
        "MFVec2f" => ListSpec {
            labels: &["X", "Y"],
            rotation: false,
        },
        "MFVec3f" => ListSpec {
            labels: &["X", "Y", "Z"],
            rotation: false,
        },
        "MFRotation" => ListSpec {
            labels: &["X", "Y", "Z", "Angle"],
            rotation: true,
        },
        _ => return None,
    })
}

const FLOAT: TypeSpec = TypeSpec {
    kind: Kind::Number,
    int32: false,
    labels: &["Value"],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `[ ... ]`; `open` is the offset just after `[`, `close` that of `]`.
    Bracketed { open: u64, close: u64 },
    /// One tuple without brackets.
    Bare,
}

/// One NUMBER token of a list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListToken {
    pub from: u64,
    pub to: u64,
    /// The exact source lexeme.
    pub text: String,
    pub value: f64,
    /// UTF-8 byte offsets (to slice the gaps between tokens).
    byte_from: usize,
    byte_to: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListValue {
    pub spec: ListSpec,
    pub form: Form,
    /// Tuples in source order; each holds `spec.arity()` tokens.
    pub tuples: Vec<Vec<ListToken>>,
    /// UTF-16 span of the whole value.
    pub from: u64,
    pub to: u64,
}

impl ListValue {
    /// The lexemes, tuple by tuple.
    pub fn lexemes(&self) -> Vec<Vec<String>> {
        self.tuples
            .iter()
            .map(|t| t.iter().map(|k| k.text.clone()).collect())
            .collect()
    }
    pub fn values(&self) -> Vec<Vec<f64>> {
        self.tuples
            .iter()
            .map(|t| t.iter().map(|k| k.value).collect())
            .collect()
    }
}

/// Read a list value. `Err` is a reason id.
pub fn read_list(
    spec: ListSpec,
    value: Option<&Ast>,
    src: &str,
) -> Result<ListValue, &'static str> {
    let v = value.ok_or(fr::VALUE_MISSING)?;
    let tok = |n: &crate::ast::Num| -> Result<ListToken, &'static str> {
        if !n.valid || !n.value.is_finite() || !matches!(n.numeric, Numeric::Int | Numeric::Float) {
            return Err(fr::VALUE_TOKEN_INVALID);
        }
        Ok(ListToken {
            from: n.range.start.offset as u64,
            to: n.range.end.offset as u64,
            text: n.range.slice(src).to_string(),
            value: n.value,
            byte_from: n.range.start.byte as usize,
            byte_to: n.range.end.byte as usize,
        })
    };
    let chunk = |toks: Vec<ListToken>| -> Vec<Vec<ListToken>> {
        toks.chunks(spec.arity()).map(|c| c.to_vec()).collect()
    };
    match v {
        Ast::Array { items, range } => {
            let raw = range.slice(src);
            if !raw.starts_with('[') || !raw.ends_with(']') || raw.len() < 2 {
                return Err(reason::LIST_FORM);
            }
            let mut toks = Vec::with_capacity(items.len());
            for i in items {
                match i {
                    Ast::Number(n) => toks.push(tok(n)?),
                    _ => return Err(reason::LIST_FORM),
                }
            }
            if toks.len() % spec.arity() != 0 {
                return Err(fr::VALUE_SHAPE);
            }
            Ok(ListValue {
                spec,
                form: Form::Bracketed {
                    open: range.start.offset as u64 + 1,
                    close: range.end.offset as u64 - 1,
                },
                tuples: chunk(toks),
                from: range.start.offset as u64,
                to: range.end.offset as u64,
            })
        }
        Ast::Numbers { values, range } => {
            if values.len() != spec.arity() {
                return Err(reason::LIST_FORM);
            }
            let toks = values.iter().map(tok).collect::<Result<Vec<_>, _>>()?;
            Ok(ListValue {
                spec,
                form: Form::Bare,
                tuples: chunk(toks),
                from: range.start.offset as u64,
                to: range.end.offset as u64,
            })
        }
        _ => Err(reason::LIST_FORM),
    }
}

/// Field-level gate for a list field of `node` at `index`. Returns the field
/// and the schema-proven list spec plus numeric bounds.
fn list_field<'a>(
    node: &'a Node,
    index: usize,
    name: &str,
) -> Result<(&'a Field, ListSpec, Option<Bounds>), &'static str> {
    let Some(Ast::Field(f)) = node.fields.get(index) else {
        return Err(fr::FIELD_NOT_PRESENT);
    };
    if f.name != name {
        return Err(fr::FIELD_NOT_PRESENT);
    }
    if duplicate_names(node).contains(&f.name.as_str()) {
        return Err(fr::FIELD_DUPLICATED);
    }
    if f.is_binding || matches!(f.value.as_deref(), Some(Ast::Is { .. })) {
        return Err(fr::IS_BOUND);
    }
    let record =
        node_schema::get_field_schema(&node.node_type, &f.name).ok_or(fr::FIELD_UNKNOWN)?;
    if !record.profiles.contains(&"vrml97") {
        return Err(fr::FIELD_X3D_ONLY);
    }
    if !matches!(record.vrml97_declaration, Some("field" | "exposedField")) {
        return Err(fr::FIELD_NOT_STORED);
    }
    let spec = list_spec(record.field_type).ok_or(reason::NOT_A_LIST_TYPE)?;
    Ok((f, spec, Bounds::of(record.constraints)))
}

/// Describe one authored list field for the Inspector (`None` when the field
/// is not of a list type). `Err` is the read-only reason.
pub fn describe_list(
    node: &Node,
    index: usize,
    src: &str,
) -> Option<Result<ListValue, &'static str>> {
    let Some(Ast::Field(f)) = node.fields.get(index) else {
        return None;
    };
    let record = node_schema::get_field_schema(&node.node_type, &f.name)?;
    list_spec(record.field_type)?;
    Some(
        list_field(node, index, &f.name)
            .and_then(|(f, spec, _)| read_list(spec, f.value.as_deref(), src)),
    )
}

/// The default value of a list field, from the schema's default text.
pub fn default_list(node_type: &str, field: &str) -> Option<Vec<Vec<String>>> {
    let record = node_schema::get_field_schema(node_type, field)?;
    let spec = list_spec(record.field_type)?;
    let text = record.default_text?;
    let lexed = crate::tokenizer::tokenize(text);
    let nums: Vec<String> = lexed
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokKind::Number { .. }))
        .map(|t| t.lexeme(text).to_string())
        .collect();
    nums.len()
        .is_multiple_of(spec.arity())
        .then(|| nums.chunks(spec.arity()).map(|c| c.to_vec()).collect())
}

// ---------------------------------------------------------------------------
// Requests and plans
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Replace the values of tuple `index` (raw user text per component).
    Set {
        index: usize,
        components: Vec<String>,
    },
    /// Insert a new tuple BEFORE `index` (`index == len` appends).
    Insert {
        index: usize,
        components: Vec<String>,
    },
    /// Remove tuple `index`.
    Delete { index: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub node_from: u64,
    pub node_to: u64,
    pub field_index: usize,
    pub field_name: String,
    /// The number of tuples the caller saw; any other count is refused.
    pub expected_count: usize,
    pub ops: Vec<Op>,
    /// B1: the user confirmed editing a shared PROTO definition.
    pub confirm_shared: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Ready {
        /// Exactly one edit: the differing region.
        edit: SpanEdit,
        new_text: String,
        /// The node's span in `new_text` (start unchanged).
        node_from: u64,
        node_to: u64,
        /// The list after the edit, tuple by tuple (exact lexemes).
        lexemes: Vec<Vec<String>>,
    },
    Unchanged,
    Refused {
        reason: &'static str,
        message: String,
        /// The operation that was refused (index into `ops`), if any.
        op_index: Option<usize>,
        component_index: Option<usize>,
    },
}

fn refused(reason: &'static str, message: impl Into<String>) -> Plan {
    Plan::Refused {
        reason,
        message: message.into(),
        op_index: None,
        component_index: None,
    }
}

fn refused_at(
    reason: &'static str,
    message: impl Into<String>,
    op: usize,
    comp: Option<usize>,
) -> Plan {
    Plan::Refused {
        reason,
        message: message.into(),
        op_index: Some(op),
        component_index: comp,
    }
}

fn reason_text(r: &'static str) -> String {
    match r {
        fr::NODE_NOT_FOUND => {
            "The node is not at that place in this revision; re-select it.".into()
        }
        fr::NODE_AMBIGUOUS => "More than one node has this span; the node cannot be proven.".into(),
        fr::NODE_IN_PROTO_SCOPE => "Nodes inside a PROTO are not edited here.".into(),
        fr::PROTO_INSTANCE => "This node is a PROTO instance.".into(),
        fr::SYNTAX_ERRORS => "Fix the syntax errors first.".into(),
        fr::PARSE_INCOMPLETE => "The document exceeds the parser limits.".into(),
        fr::IS_BOUND => "The field is bound with IS.".into(),
        fr::FIELD_DUPLICATED => "The field is written more than once.".into(),
        reason::LIST_FORM => "The value is not a plain list of numbers.".into(),
        fr::VALUE_SHAPE => "The number of values does not fit the field type.".into(),
        fr::VALUE_TOKEN_INVALID => "The value holds a malformed number.".into(),
        other => other.into(),
    }
}

/// Is a gap between two tokens only whitespace and commas (no comment)?
pub(crate) fn clean(gap: &str) -> bool {
    gap.chars()
        .all(|c| matches!(c, ' ' | '\t' | '\r' | '\n' | ','))
}

/// Validate and encode one tuple of raw component texts.
fn encode_tuple(
    spec: ListSpec,
    bounds: Option<&Bounds>,
    raw: &[String],
    current: Option<&[(String, f64)]>,
    op: usize,
) -> Result<Vec<(String, f64)>, Plan> {
    if raw.len() != spec.arity() {
        return Err(refused_at(
            fr::INPUT_SHAPE,
            format!("Enter {} values.", spec.arity()),
            op,
            None,
        ));
    }
    let mut out = Vec::with_capacity(raw.len());
    for (i, r) in raw.iter().enumerate() {
        // An unchanged component keeps its exact lexeme, unvalidated (an
        // unusual authored value is preserved, never re-judged).
        if let Some(c) = current.and_then(|c| c.get(i)).filter(|c| c.0 == r.trim()) {
            out.push(c.clone());
            continue;
        }
        // Rotation angles are unbounded; the schema range applies otherwise.
        let b = if spec.rotation { None } else { bounds };
        match encode_number(&FLOAT, r, b) {
            Ok(t) => out.push(t),
            Err((reason, m)) => {
                return Err(refused_at(
                    reason,
                    format!("{}: {m}", spec.labels[i]),
                    op,
                    Some(i),
                ))
            }
        }
    }
    let axis_changed = current.is_none_or(|c| c.iter().zip(&out).take(3).any(|(a, b)| a.0 != b.0));
    if spec.rotation && axis_changed && out[..3].iter().all(|(_, v)| *v == 0.0) {
        return Err(refused_at(
            reason::ZERO_AXIS,
            "A rotation axis cannot be 0 0 0.",
            op,
            Some(0),
        ));
    }
    Ok(out)
}

/// The separator to put next to a new tuple: an existing clean gap between
/// two tuples (preferring the one at the insertion point), else `", "`.
fn separator(src: &str, l: &ListValue, at: usize) -> String {
    let gap = |i: usize| -> Option<&str> {
        let a = l.tuples.get(i)?.last()?;
        let b = l.tuples.get(i + 1)?.first()?;
        Some(&src[a.byte_to..b.byte_from])
    };
    let n = l.tuples.len();
    let prefer = [at.checked_sub(1), Some(at), at.checked_sub(2)];
    prefer
        .into_iter()
        .flatten()
        .chain(0..n)
        .filter_map(gap)
        .find(|g| clean(g) && !g.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| ", ".into())
}

/// Plan ONE operation against `src` (where `l` was read). Returns the edits.
fn plan_op(
    src: &str,
    l: &ListValue,
    op: &Op,
    encoded: Option<&[(String, f64)]>,
) -> Result<Vec<SpanEdit>, (&'static str, String)> {
    let n = l.tuples.len();
    let tuple_text = |e: &[(String, f64)]| {
        e.iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    match op {
        Op::Set { index, .. } => {
            let tuple = l
                .tuples
                .get(*index)
                .ok_or((reason::INDEX, "No such point.".into()))?;
            Ok(tuple
                .iter()
                .zip(encoded.unwrap_or(&[]))
                .filter(|(t, (text, _))| &t.text != text)
                .map(|(t, (text, _))| SpanEdit {
                    from: t.from,
                    to: t.to,
                    insert: text.clone(),
                })
                .collect())
        }
        Op::Insert { index, .. } => {
            let new = tuple_text(encoded.unwrap_or(&[]));
            if *index > n {
                return Err((reason::INDEX, "No such position.".into()));
            }
            match l.form {
                Form::Bare => {
                    // One bare tuple becomes a bracketed list; its own tokens
                    // are kept, only brackets and the new tuple are inserted.
                    let (before, after) = if *index == 0 {
                        (format!("[ {new}, "), " ]".to_string())
                    } else {
                        ("[ ".to_string(), format!(", {new} ]"))
                    };
                    Ok(vec![
                        SpanEdit {
                            from: l.from,
                            to: l.from,
                            insert: before,
                        },
                        SpanEdit {
                            from: l.to,
                            to: l.to,
                            insert: after,
                        },
                    ])
                }
                Form::Bracketed { open, close } => {
                    if n == 0 {
                        let interior_empty = close == open;
                        let insert = if interior_empty {
                            format!(" {new} ")
                        } else {
                            format!(" {new}")
                        };
                        return Ok(vec![SpanEdit {
                            from: open,
                            to: open,
                            insert,
                        }]);
                    }
                    let sep = separator(src, l, *index);
                    if *index < n {
                        let at = l.tuples[*index][0].from;
                        Ok(vec![SpanEdit {
                            from: at,
                            to: at,
                            insert: format!("{new}{sep}"),
                        }])
                    } else {
                        let at = l.tuples[n - 1].last().map(|t| t.to).unwrap_or(open);
                        Ok(vec![SpanEdit {
                            from: at,
                            to: at,
                            insert: format!("{sep}{new}"),
                        }])
                    }
                }
            }
        }
        Op::Delete { index } => {
            let k = *index;
            let tuple = l
                .tuples
                .get(k)
                .ok_or((reason::INDEX, "No such point.".into()))?;
            let (first, last) = (&tuple[0], &tuple[tuple.len() - 1]);
            if !tuple
                .windows(2)
                .all(|w| clean(&src[w[0].byte_to..w[1].byte_from]))
            {
                return Err((
                    reason::POINT_HAS_COMMENT,
                    "This point has a comment inside it; delete it in the source.".into(),
                ));
            }
            if matches!(l.form, Form::Bare) {
                // The only value of a bare list: the list becomes `[ ]`.
                return Ok(vec![SpanEdit {
                    from: l.from,
                    to: l.to,
                    insert: "[ ]".into(),
                }]);
            }
            let gap_after = l
                .tuples
                .get(k + 1)
                .map(|nx| (&src[last.byte_to..nx[0].byte_from], nx[0].from));
            let gap_before = k.checked_sub(1).map(|p| &l.tuples[p]).map(|pv| {
                (
                    &src[pv[pv.len() - 1].byte_to..first.byte_from],
                    pv[pv.len() - 1].to,
                )
            });
            let (from, to) = match (gap_after, gap_before) {
                (Some((g, next_from)), _) if clean(g) => (first.from, next_from),
                (_, Some((g, prev_to))) if clean(g) => (prev_to, last.to),
                _ => (first.from, last.to),
            };
            Ok(vec![SpanEdit {
                from,
                to,
                insert: String::new(),
            }])
        }
    }
}

/// Is a list of numeric tuples a closed outline (first equals last)?
pub fn is_closed(values: &[Vec<f64>]) -> bool {
    values.len() >= 2 && values.first() == values.last()
}

/// Extrusion-specific count and closure rules (ISO 6.18), applied per
/// operation BEFORE it changes the list. `Err` = (reason, message).
fn extrusion_rule(
    node_type: &str,
    field: &str,
    values: &[Vec<f64>],
    op: &Op,
) -> Result<(), (&'static str, String)> {
    if node_type != "Extrusion" {
        return Ok(());
    }
    let n = values.len();
    match (field, op) {
        ("crossSection", Op::Delete { index }) => {
            let closed = is_closed(values);
            if closed && (*index == 0 || *index + 1 == n) {
                return Err((
                    reason::WOULD_OPEN,
                    "This is the closing point of a closed outline; deleting it would open the outline.".into(),
                ));
            }
            let min = if closed { 4 } else { 2 };
            if n <= min {
                return Err((
                    reason::TOO_FEW,
                    format!(
                        "A {} outline needs at least {min} points.",
                        if closed { "closed" } else { "open" }
                    ),
                ));
            }
        }
        ("crossSection", Op::Insert { index, .. })
            if is_closed(values) && (*index == 0 || *index == n) =>
        {
            return Err((
                reason::WOULD_OPEN,
                "A point before the first or after the last point would open the closed outline; insert it between two points.".into(),
            ));
        }
        ("spine", Op::Delete { .. }) if n <= 2 => {
            return Err((reason::TOO_FEW, "A spine needs at least 2 points.".into()));
        }
        ("scale" | "orientation", Op::Delete { .. }) if n <= 1 => {
            return Err((
                reason::TOO_FEW,
                format!("{field} needs at least one value."),
            ));
        }
        _ => {}
    }
    Ok(())
}

/// The exact differing region of `old` → `new` as one UTF-16 span edit.
pub(crate) fn diff_edit(old: &str, new: &str) -> Option<SpanEdit> {
    let o: Vec<u16> = old.encode_utf16().collect();
    let n: Vec<u16> = new.encode_utf16().collect();
    let mut pre = 0;
    while pre < o.len() && pre < n.len() && o[pre] == n[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < o.len() - pre && suf < n.len() - pre && o[o.len() - 1 - suf] == n[n.len() - 1 - suf]
    {
        suf += 1;
    }
    let from = pre as u64;
    let to = (o.len() - suf) as u64;
    let insert = String::from_utf16(&n[pre..n.len() - suf]).ok()?;
    // Never split a surrogate pair.
    byte_at(old, from)?;
    byte_at(old, to)?;
    Some(SpanEdit { from, to, insert })
}

/// Locate node + list in `src` after earlier operations: same start, end
/// moved by `delta`, exactly one node, gates passed.
fn locate<'a>(
    p: &'a ParseResult,
    req: &Request,
    to: u64,
    want: Option<(&str, Option<&str>)>,
) -> Result<(&'a Node, &'a Field, ListSpec, Option<Bounds>), &'static str> {
    let node = node_gate(p, req.node_from, to, req.confirm_shared)?;
    if let Some((ty, def)) = want {
        if node.node_type != ty || node.def.as_deref() != def {
            return Err(fr::ROUND_TRIP_FAILED);
        }
    }
    let (f, spec, b) = list_field(node, req.field_index, &req.field_name)?;
    Ok((node, f, spec, b))
}

/// Plan a list edit request. Never panics for ordinary invalid input.
pub fn plan_list_edit(src: &str, req: &Request) -> Plan {
    if req.ops.is_empty() {
        return refused(reason::NO_OPS, "Nothing to change.");
    }
    let p0 = parse(src);
    let (node0, f0, spec, bounds) = match locate(&p0, req, req.node_to, None) {
        Ok(x) => x,
        Err(r) => return refused(r, reason_text(r)),
    };
    let list0 = match read_list(spec, f0.value.as_deref(), src) {
        Ok(l) => l,
        Err(r) => return refused(r, reason_text(r)),
    };
    if list0.tuples.len() != req.expected_count {
        return refused(
            reason::COUNT_CHANGED,
            format!(
                "The list has {} values, not {}; re-select the node.",
                list0.tuples.len(),
                req.expected_count
            ),
        );
    }
    let ty = node0.node_type.clone();
    let def = node0.def.clone();
    let (value_from, value_to) = (list0.from, list0.to);

    // The intended list, kept as lexemes + values.
    let mut model: Vec<Vec<(String, f64)>> = list0
        .tuples
        .iter()
        .map(|t| t.iter().map(|k| (k.text.clone(), k.value)).collect())
        .collect();
    let mut text = src.to_string();
    let mut to = req.node_to;

    for (i, op) in req.ops.iter().enumerate() {
        let p = parse(&text);
        let (_, f, _, _) = match locate(&p, req, to, Some((&ty, def.as_deref()))) {
            Ok(x) => x,
            Err(r) => return refused_at(r, reason_text(r), i, None),
        };
        let list = match read_list(spec, f.value.as_deref(), &text) {
            Ok(l) => l,
            Err(r) => return refused_at(r, reason_text(r), i, None),
        };
        let values: Vec<Vec<f64>> = model
            .iter()
            .map(|t| t.iter().map(|(_, v)| *v).collect())
            .collect();
        if let Err((r, m)) = extrusion_rule(&ty, &req.field_name, &values, op) {
            return refused_at(r, m, i, None);
        }
        let encoded = match op {
            Op::Set { components, .. } | Op::Insert { components, .. } => {
                let cur = match op {
                    Op::Set { index, .. } => model.get(*index).cloned(),
                    _ => None,
                };
                match encode_tuple(spec, bounds.as_ref(), components, cur.as_deref(), i) {
                    Ok(e) => Some(e),
                    Err(plan) => return plan,
                }
            }
            Op::Delete { .. } => None,
        };
        let edits = match plan_op(&text, &list, op, encoded.as_deref()) {
            Ok(e) => e,
            Err((r, m)) => return refused_at(r, m, i, None),
        };
        match op {
            Op::Set { index, .. } => model[*index] = encoded.unwrap_or_default(),
            Op::Insert { index, .. } => model.insert(*index, encoded.unwrap_or_default()),
            Op::Delete { index } => {
                model.remove(*index);
            }
        }
        if edits.is_empty() {
            continue;
        }
        let Some(next) = splice(&text, &edits) else {
            return refused_at(
                fr::TRANSACTION_REJECTED,
                "The edit span is invalid.",
                i,
                None,
            );
        };
        let delta: i64 = edits
            .iter()
            .map(|e| utf16_len(&e.insert) as i64 - (e.to - e.from) as i64)
            .sum();
        to = (to as i64 + delta) as u64;
        text = next;
    }

    if text == src {
        return Plan::Unchanged;
    }
    let Some(edit) = diff_edit(src, &text) else {
        return refused(fr::TRANSACTION_REJECTED, "The edit span is invalid.");
    };
    // The change must lie inside the original value, strictly inside the node.
    if edit.from < value_from
        || edit.to > value_to
        || edit.from <= req.node_from
        || edit.to >= req.node_to
    {
        return refused(
            reason::SCOPE,
            "The change would reach outside the field value.",
        );
    }

    // Final proof on the result.
    let p = parse(&text);
    if has_blocking_syntax_error(&p) || p.truncated || p.depth_capped {
        return refused(
            fr::ROUND_TRIP_FAILED,
            "The result does not parse cleanly; nothing changed.",
        );
    }
    let found = nodes_at(&p, req.node_from, to);
    let [again] = found.as_slice() else {
        return refused(
            fr::ROUND_TRIP_FAILED,
            "The node could not be proven after the edit.",
        );
    };
    if again.other_proto_scope() || again.node.node_type != ty || again.node.def != def {
        return refused(fr::ROUND_TRIP_FAILED, "The node changed identity.");
    }
    let lexemes: Vec<Vec<String>> = model
        .iter()
        .map(|t| t.iter().map(|(s, _)| s.clone()).collect())
        .collect();
    let ok = list_field(again.node, req.field_index, &req.field_name)
        .ok()
        .and_then(|(f, spec, _)| read_list(spec, f.value.as_deref(), &text).ok())
        .is_some_and(|l| l.lexemes() == lexemes);
    if !ok {
        return refused(
            fr::ROUND_TRIP_FAILED,
            "The list does not hold the intended values; nothing changed.",
        );
    }
    Plan::Ready {
        edit,
        new_text: text,
        node_from: req.node_from,
        node_to: to,
        lexemes,
    }
}

// ---------------------------------------------------------------------------
// Explicitly author a field that is absent (the user asks; never automatic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum AddValue {
    Bool(bool),
    /// One number (SFFloat).
    Number(String),
    /// A list of tuples.
    List(Vec<Vec<String>>),
    /// One tuple (VISUAL-4B: SFVec2f, SFVec3f, SFRotation).
    Tuple(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AddRequest {
    pub node_from: u64,
    pub node_to: u64,
    pub field_name: String,
    pub value: AddValue,
    /// B1: the user confirmed editing a shared PROTO definition.
    pub confirm_shared: bool,
}

/// Plan the insertion of `field_name value` as the LAST statement of the
/// node's body. Only an absent VRML97 `field` / `exposedField` of type
/// SFBool, SFFloat, SFVec2f, SFVec3f, SFRotation or one of the three list
/// types may be added. The value is
/// what the user chose; nothing else is written.
pub fn plan_add_field(src: &str, req: &AddRequest) -> Plan {
    let p = parse(src);
    let node = match node_gate(&p, req.node_from, req.node_to, req.confirm_shared) {
        Ok(n) => n,
        Err(r) => return refused(r, reason_text(r)),
    };
    if node
        .fields
        .iter()
        .any(|f| matches!(f, Ast::Field(f) if f.name == req.field_name))
    {
        return refused(
            reason::FIELD_ALREADY_AUTHORED,
            "The field is already in the source.",
        );
    }
    let Some(record) = node_schema::get_field_schema(&node.node_type, &req.field_name)
        .filter(|r| r.profiles.contains(&"vrml97"))
        .filter(|r| matches!(r.vrml97_declaration, Some("field" | "exposedField")))
    else {
        return refused(
            reason::FIELD_NOT_ADDABLE,
            "This field cannot be added to this node.",
        );
    };
    let bounds = Bounds::of(record.constraints);
    let value_text = match (record.field_type, &req.value) {
        ("SFBool", AddValue::Bool(b)) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        ("SFFloat", AddValue::Number(s)) => match encode_number(&FLOAT, s, bounds.as_ref()) {
            Ok((t, _)) => t,
            Err((r, m)) => return refused(r, m),
        },
        (ty @ ("SFVec2f" | "SFVec3f" | "SFRotation"), AddValue::Tuple(t)) => {
            let Some(spec) = list_spec(&format!("MF{}", &ty[2..])) else {
                return refused(reason::FIELD_NOT_ADDABLE, "The field type is not addable.");
            };
            match encode_tuple(spec, bounds.as_ref(), t, None, 0) {
                Ok(e) => e
                    .iter()
                    .map(|(s, _)| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                Err(plan) => return plan,
            }
        }
        (ty, AddValue::List(tuples)) if list_spec(ty).is_some() => {
            let spec = list_spec(ty).unwrap_or(ListSpec {
                labels: &[],
                rotation: false,
            });
            let mut parts = Vec::with_capacity(tuples.len());
            for t in tuples {
                match encode_tuple(spec, bounds.as_ref(), t, None, 0) {
                    Ok(e) => parts.push(
                        e.iter()
                            .map(|(s, _)| s.as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    Err(plan) => return plan,
                }
            }
            if parts.is_empty() {
                "[ ]".to_string()
            } else {
                format!("[ {} ]", parts.join(", "))
            }
        }
        _ => {
            return refused(
                reason::FIELD_NOT_ADDABLE,
                "The value does not fit the field type.",
            )
        }
    };

    let (nf, nt) = (req.node_from, req.node_to);
    let stmt = format!("{} {value_text}", req.field_name);
    let edit = match body_append(&p, src, node, nf, nt, &stmt) {
        Ok(e) => e,
        Err((r, m)) => return refused(r, m),
    };
    let Some(new_text) = splice(src, std::slice::from_ref(&edit)) else {
        return refused(fr::TRANSACTION_REJECTED, "The edit span is invalid.");
    };
    let to = nt + utf16_len(&edit.insert);
    // Proof: same node, every earlier statement unchanged, the new field last
    // with exactly the written value.
    let after = parse(&new_text);
    let found = nodes_at(&after, nf, to);
    let ok = !has_blocking_syntax_error(&after)
        && !after.truncated
        && !after.depth_capped
        && match found.as_slice() {
            [again] => {
                let n = again.node;
                !again.other_proto_scope()
                    && n.node_type == node.node_type
                    && n.def == node.def
                    && n.fields.len() == node.fields.len() + 1
                    && node
                        .fields
                        .iter()
                        .zip(&n.fields)
                        .all(|(a, b)| a.range() == b.range())
                    && match n.fields.last() {
                        Some(Ast::Field(f)) => {
                            f.name == req.field_name
                                && !f.is_binding
                                && f.value
                                    .as_ref()
                                    .is_some_and(|v| v.range().slice(&new_text) == value_text)
                        }
                        _ => false,
                    }
            }
            _ => false,
        };
    if !ok {
        return refused(
            fr::ROUND_TRIP_FAILED,
            "The new field could not be proven; nothing changed.",
        );
    }
    let lexemes = match &req.value {
        AddValue::List(t) => t
            .iter()
            .map(|x| x.iter().map(|s| s.trim().to_string()).collect())
            .collect(),
        AddValue::Tuple(t) => vec![t.iter().map(|s| s.trim().to_string()).collect()],
        _ => Vec::new(),
    };
    Plan::Ready {
        edit,
        new_text,
        node_from: nf,
        node_to: to,
        lexemes,
    }
}

/// The insertion that appends `stmt` as the LAST statement of the body of
/// `node` (the one node at `[nf, nt)` in `p`). The body's braces come from the
/// TOKENS, never a text search; the separator copies the node's own clean gap
/// (indentation and line ending), else one space. `Err` = (reason, message).
pub(crate) fn body_append(
    p: &ParseResult,
    src: &str,
    node: &Node,
    nf: u64,
    nt: u64,
    stmt: &str,
) -> Result<SpanEdit, (&'static str, &'static str)> {
    let no_body = (reason::NO_BODY, "The node's braces could not be proven.");
    let toks: Vec<_> = p
        .tokens
        .iter()
        .filter(|t| {
            t.range.start.offset as u64 >= node.type_range.end.offset as u64
                && (t.range.end.offset as u64) <= nt
        })
        .collect();
    let open = toks.iter().find(|t| matches!(t.kind, TokKind::LBrace));
    let close = toks.last().filter(|t| matches!(t.kind, TokKind::RBrace));
    let (Some(open), Some(close)) = (open, close) else {
        return Err(no_body);
    };
    let open_to = open.range.end.offset as u64;
    if close.range.end.offset as u64 != nt || open_to <= nf {
        return Err(no_body);
    }
    let (at, insert) = match node.fields.as_slice() {
        [] => {
            let interior = &src[open.range.end.byte as usize..close.range.start.byte as usize];
            if interior.is_empty() {
                (open_to, format!(" {stmt} "))
            } else {
                (open_to, format!(" {stmt}"))
            }
        }
        fields => {
            let last = fields[fields.len() - 1].range();
            // Copy the clean gap between the last two statements (keeps the
            // node's own indentation and line ending), else one space.
            let gap = fields
                .len()
                .checked_sub(2)
                .map(|i| &src[fields[i].range().end.byte as usize..last.start.byte as usize])
                .filter(|g| clean(g) && !g.is_empty())
                .or_else(|| {
                    let g =
                        &src[open.range.end.byte as usize..fields[0].range().start.byte as usize];
                    (clean(g) && !g.is_empty()).then_some(g)
                })
                .unwrap_or(" ");
            (last.end.offset as u64, format!("{gap}{stmt}"))
        }
    };
    Ok(SpanEdit {
        from: at,
        to: at,
        insert,
    })
}

#[cfg(test)]
mod tests;
