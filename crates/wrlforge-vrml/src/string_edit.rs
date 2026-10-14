// SPDX-License-Identifier: GPL-3.0-or-later
//! MFString list editing (B2 texture assignment, Spazz3D Texture page /
//! Flux texture panel): `ImageTexture.url`, `Inline.url`, `Anchor.url`,
//! `WorldInfo.info` and every other stored VRML97 `MFString` field, as exact
//! source-text patches.
//!
//! THE DOCUMENT IS THE TEXT (WD.md section 2). A list is read from the parse
//! tree as STRING tokens with their exact spans. An edit replaces one string
//! token, or inserts / removes one token and the separator next to it. The
//! form the author wrote is kept: `"a"` stays bare until a second value is
//! added, `[ "a" ]` stays bracketed when it shrinks, and separators,
//! comments, line endings and indentation are copied, never re-printed.
//!
//! Identity and gates are the field-edit ones (`field_edit::node_gate`,
//! including the B1 PROTO-body consent `confirm_shared`). The field must be
//! authored exactly once, not IS-bound, a VRML97 `field` / `exposedField` of
//! type `MFString`, written as `"a"`, `[ ]` or `[ "a" "b" ... ]`.
//!
//! A request is a SEQUENCE of operations, planned one after another on the
//! text the previous one produced (each re-parsed and re-proven), then
//! reduced to ONE span edit that must lie inside the original value. The
//! final parse must show the same node at the same start, the same field at
//! the same index, and exactly the intended decoded strings. One request is
//! one transaction, so one undo step.
//!
//! `plan_add_texture` writes `texture ImageTexture { url "<url>" }` into an
//! `Appearance` that has no `texture` field, as one proven insertion.

use crate::ast::{Ast, Field, Node};
use crate::field_edit::{
    byte_at, duplicate_names, encode_string, has_blocking_syntax_error, node_gate, nodes_at,
    reason as fr, splice, utf16_len, SpanEdit,
};
use crate::mf_edit::{body_append, clean, diff_edit};
use crate::node_schema;
use crate::parser::{parse, ParseResult};

/// Stable reason ids of this module (the field-edit ids are reused too).
pub mod reason {
    pub const NOT_MFSTRING: &str = "field-type-not-mfstring";
    pub const LIST_FORM: &str = "string-list-form-not-supported";
    pub const INDEX: &str = "string-index-out-of-range";
    pub const LIST_CHANGED: &str = "string-list-changed";
    pub const NO_OPS: &str = "no-operations";
    pub const SCOPE: &str = "edit-outside-the-value";
    pub const NOT_APPEARANCE: &str = "node-is-not-an-appearance";
    pub const TEXTURE_AUTHORED: &str = "texture-already-authored";
    pub const URL_EMPTY: &str = "texture-url-empty";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `[ ... ]`; `open` is the offset just after `[`, `close` that of `]`.
    Bracketed { open: u64, close: u64 },
    /// One string without brackets.
    Bare,
}

/// One STRING token of a list.
#[derive(Debug, Clone, PartialEq)]
pub struct StrToken {
    pub from: u64,
    pub to: u64,
    /// The decoded value.
    pub value: String,
    byte_from: usize,
    byte_to: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StringList {
    pub form: Form,
    pub items: Vec<StrToken>,
    /// UTF-16 span of the whole value.
    pub from: u64,
    pub to: u64,
}

impl StringList {
    pub fn values(&self) -> Vec<String> {
        self.items.iter().map(|t| t.value.clone()).collect()
    }
}

fn str_token(a: &Ast, src: &str) -> Result<StrToken, &'static str> {
    let Ast::Str { value, range } = a else {
        return Err(reason::LIST_FORM);
    };
    let raw = range.slice(src);
    if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
        return Err(fr::VALUE_TOKEN_INVALID);
    }
    Ok(StrToken {
        from: range.start.offset as u64,
        to: range.end.offset as u64,
        value: value.clone(),
        byte_from: range.start.byte as usize,
        byte_to: range.end.byte as usize,
    })
}

/// Read an MFString value. `Err` is a reason id.
pub fn read_strings(value: Option<&Ast>, src: &str) -> Result<StringList, &'static str> {
    let v = value.ok_or(fr::VALUE_MISSING)?;
    match v {
        Ast::Array { items, range } => {
            let raw = range.slice(src);
            if raw.len() < 2 || !raw.starts_with('[') || !raw.ends_with(']') {
                return Err(reason::LIST_FORM);
            }
            let items = items
                .iter()
                .map(|i| str_token(i, src))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(StringList {
                form: Form::Bracketed {
                    open: range.start.offset as u64 + 1,
                    close: range.end.offset as u64 - 1,
                },
                items,
                from: range.start.offset as u64,
                to: range.end.offset as u64,
            })
        }
        Ast::Str { range, .. } => Ok(StringList {
            form: Form::Bare,
            items: vec![str_token(v, src)?],
            from: range.start.offset as u64,
            to: range.end.offset as u64,
        }),
        _ => Err(reason::LIST_FORM),
    }
}

/// Field-level gate for an MFString field of `node` at `index`.
fn string_field<'a>(node: &'a Node, index: usize, name: &str) -> Result<&'a Field, &'static str> {
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
    if record.field_type != "MFString" {
        return Err(reason::NOT_MFSTRING);
    }
    Ok(f)
}

/// Describe one authored MFString field for the Inspector (`None` when the
/// field is not of type MFString). `Err` is the read-only reason.
pub fn describe_strings(
    node: &Node,
    index: usize,
    src: &str,
) -> Option<Result<StringList, &'static str>> {
    let Some(Ast::Field(f)) = node.fields.get(index) else {
        return None;
    };
    let record = node_schema::get_field_schema(&node.node_type, &f.name)?;
    if record.field_type != "MFString" {
        return None;
    }
    Some(string_field(node, index, &f.name).and_then(|f| read_strings(f.value.as_deref(), src)))
}

// ---------------------------------------------------------------------------
// Requests and plans
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Replace string `index` (decoded text; Rust encodes it).
    Set { index: usize, value: String },
    /// Insert a new string BEFORE `index` (`index == len` appends).
    Insert { index: usize, value: String },
    /// Remove string `index`.
    Delete { index: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub node_from: u64,
    pub node_to: u64,
    pub field_index: usize,
    pub field_name: String,
    /// The decoded strings the caller saw; any other list is refused.
    pub expected: Vec<String>,
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
        /// The decoded strings after the edit.
        values: Vec<String>,
    },
    Unchanged,
    Refused {
        reason: &'static str,
        message: String,
        /// The operation that was refused (index into `ops`), if any.
        op_index: Option<usize>,
    },
}

fn refused(reason: &'static str, message: impl Into<String>) -> Plan {
    Plan::Refused {
        reason,
        message: message.into(),
        op_index: None,
    }
}

fn refused_at(reason: &'static str, message: impl Into<String>, op: usize) -> Plan {
    Plan::Refused {
        reason,
        message: message.into(),
        op_index: Some(op),
    }
}

fn reason_text(r: &'static str) -> String {
    match r {
        fr::NODE_NOT_FOUND => {
            "The node is not at that place in this revision; re-select it.".into()
        }
        fr::NODE_AMBIGUOUS => "More than one node has this span; the node cannot be proven.".into(),
        fr::NODE_IN_PROTO_SCOPE => "Nodes inside a PROTO interface are not edited here.".into(),
        fr::SHARED_DEFINITION_UNCONFIRMED => {
            "Editing the shared PROTO definition was not confirmed.".into()
        }
        fr::PROTO_INSTANCE => "This node is a PROTO instance.".into(),
        fr::SYNTAX_ERRORS => "Fix the syntax errors first.".into(),
        fr::PARSE_INCOMPLETE => "The document exceeds the parser limits.".into(),
        fr::IS_BOUND => "The field is bound with IS.".into(),
        fr::FIELD_DUPLICATED => "The field is written more than once.".into(),
        reason::NOT_MFSTRING => "The field is not a list of strings.".into(),
        reason::LIST_FORM => "The value is not a plain list of strings.".into(),
        fr::VALUE_TOKEN_INVALID => "The value holds a malformed string.".into(),
        other => other.into(),
    }
}

/// The separator to put next to a new string: an existing clean gap between
/// two strings (preferring the one at the insertion point), else one space.
fn separator(src: &str, l: &StringList, at: usize) -> String {
    let gap = |i: usize| -> Option<&str> {
        let a = l.items.get(i)?;
        let b = l.items.get(i + 1)?;
        Some(&src[a.byte_to..b.byte_from])
    };
    let n = l.items.len();
    [at.checked_sub(1), Some(at), at.checked_sub(2)]
        .into_iter()
        .flatten()
        .chain(0..n)
        .filter_map(gap)
        .find(|g| clean(g) && !g.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| " ".into())
}

/// Plan ONE operation against `src` (where `l` was read). `encoded` is the
/// quoted token for Set / Insert.
fn plan_op(
    src: &str,
    l: &StringList,
    op: &Op,
    encoded: &str,
) -> Result<Vec<SpanEdit>, (&'static str, String)> {
    let n = l.items.len();
    let at = |from: u64, insert: String| SpanEdit {
        from,
        to: from,
        insert,
    };
    match op {
        Op::Set { index, value } => {
            let t = l
                .items
                .get(*index)
                .ok_or((reason::INDEX, "No such entry.".into()))?;
            if &t.value == value {
                return Ok(Vec::new());
            }
            Ok(vec![SpanEdit {
                from: t.from,
                to: t.to,
                insert: encoded.to_string(),
            }])
        }
        Op::Insert { index, .. } => {
            if *index > n {
                return Err((reason::INDEX, "No such position.".into()));
            }
            match l.form {
                // One bare string becomes a bracketed list; its own token is
                // kept, only brackets and the new string are inserted.
                Form::Bare => {
                    let (before, after) = if *index == 0 {
                        (format!("[ {encoded} "), " ]".to_string())
                    } else {
                        ("[ ".to_string(), format!(" {encoded} ]"))
                    };
                    Ok(vec![at(l.from, before), at(l.to, after)])
                }
                Form::Bracketed { open, close } => {
                    if n == 0 {
                        let insert = if close == open {
                            format!(" {encoded} ")
                        } else {
                            format!(" {encoded}")
                        };
                        return Ok(vec![at(open, insert)]);
                    }
                    let sep = separator(src, l, *index);
                    if *index < n {
                        Ok(vec![at(l.items[*index].from, format!("{encoded}{sep}"))])
                    } else {
                        Ok(vec![at(l.items[n - 1].to, format!("{sep}{encoded}"))])
                    }
                }
            }
        }
        Op::Delete { index } => {
            let k = *index;
            let t = l
                .items
                .get(k)
                .ok_or((reason::INDEX, "No such entry.".into()))?;
            let Form::Bracketed { open, close } = l.form else {
                // The only value of a bare list: the list becomes `[ ]`.
                return Ok(vec![SpanEdit {
                    from: l.from,
                    to: l.to,
                    insert: "[ ]".into(),
                }]);
            };
            if n == 1 {
                // The last entry: a clean interior becomes one space; one
                // holding a comment keeps everything but the token.
                let bad = || {
                    (
                        fr::TRANSACTION_REJECTED,
                        "The edit span is invalid.".to_string(),
                    )
                };
                let ob = byte_at(src, open).ok_or_else(bad)?;
                let cb = byte_at(src, close).ok_or_else(bad)?;
                let interior_clean = clean(&src[ob..t.byte_from]) && clean(&src[t.byte_to..cb]);
                return Ok(vec![if interior_clean {
                    SpanEdit {
                        from: open,
                        to: close,
                        insert: " ".into(),
                    }
                } else {
                    SpanEdit {
                        from: t.from,
                        to: t.to,
                        insert: String::new(),
                    }
                }]);
            }
            let gap_after = l
                .items
                .get(k + 1)
                .map(|nx| (&src[t.byte_to..nx.byte_from], nx.from));
            let gap_before = k
                .checked_sub(1)
                .map(|p| &l.items[p])
                .map(|pv| (&src[pv.byte_to..t.byte_from], pv.to));
            let (from, to) = match (gap_after, gap_before) {
                (Some((g, next_from)), _) if clean(g) => (t.from, next_from),
                (_, Some((g, prev_to))) if clean(g) => (prev_to, t.to),
                _ => (t.from, t.to),
            };
            Ok(vec![SpanEdit {
                from,
                to,
                insert: String::new(),
            }])
        }
    }
}

/// Locate node + list after earlier operations: same start, end moved,
/// exactly one node, gates passed, same type and DEF.
fn locate<'a>(
    p: &'a ParseResult,
    req: &Request,
    to: u64,
    want: Option<(&str, Option<&str>)>,
) -> Result<(&'a Node, &'a Field), &'static str> {
    let node = node_gate(p, req.node_from, to, req.confirm_shared)?;
    if let Some((ty, def)) = want {
        if node.node_type != ty || node.def.as_deref() != def {
            return Err(fr::ROUND_TRIP_FAILED);
        }
    }
    let f = string_field(node, req.field_index, &req.field_name)?;
    Ok((node, f))
}

/// Plan an MFString edit request. Never panics for ordinary invalid input.
pub fn plan_string_edit(src: &str, req: &Request) -> Plan {
    if req.ops.is_empty() {
        return refused(reason::NO_OPS, "Nothing to change.");
    }
    let p0 = parse(src);
    let (node0, f0) = match locate(&p0, req, req.node_to, None) {
        Ok(x) => x,
        Err(r) => return refused(r, reason_text(r)),
    };
    let list0 = match read_strings(f0.value.as_deref(), src) {
        Ok(l) => l,
        Err(r) => return refused(r, reason_text(r)),
    };
    if list0.values() != req.expected {
        return refused(
            reason::LIST_CHANGED,
            "The list is not the one shown; re-select the node.",
        );
    }
    let ty = node0.node_type.clone();
    let def = node0.def.clone();
    let (value_from, value_to) = (list0.from, list0.to);

    let mut model = list0.values();
    let mut text = src.to_string();
    let mut to = req.node_to;

    for (i, op) in req.ops.iter().enumerate() {
        let p = parse(&text);
        let f = match locate(&p, req, to, Some((&ty, def.as_deref()))) {
            Ok((_, f)) => f,
            Err(r) => return refused_at(r, reason_text(r), i),
        };
        let list = match read_strings(f.value.as_deref(), &text) {
            Ok(l) => l,
            Err(r) => return refused_at(r, reason_text(r), i),
        };
        let encoded = match op {
            Op::Set { value, .. } | Op::Insert { value, .. } => match encode_string(value) {
                Ok(e) => e,
                Err((r, m)) => return refused_at(r, m, i),
            },
            Op::Delete { .. } => String::new(),
        };
        let edits = match plan_op(&text, &list, op, &encoded) {
            Ok(e) => e,
            Err((r, m)) => return refused_at(r, m, i),
        };
        match op {
            Op::Set { index, value } => model[*index] = value.clone(),
            Op::Insert { index, value } => model.insert(*index, value.clone()),
            Op::Delete { index } => {
                model.remove(*index);
            }
        }
        if edits.is_empty() {
            continue;
        }
        let Some(next) = splice(&text, &edits) else {
            return refused_at(fr::TRANSACTION_REJECTED, "The edit span is invalid.", i);
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
    let ok = string_field(again.node, req.field_index, &req.field_name)
        .ok()
        .and_then(|f| read_strings(f.value.as_deref(), &text).ok())
        .is_some_and(|l| l.values() == model);
    if !ok {
        return refused(
            fr::ROUND_TRIP_FAILED,
            "The list does not hold the intended strings; nothing changed.",
        );
    }
    Plan::Ready {
        edit,
        new_text: text,
        node_from: req.node_from,
        node_to: to,
        values: model,
    }
}

// ---------------------------------------------------------------------------
// Add a texture to an Appearance (the user asks; never automatic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct AddTextureRequest {
    /// The Appearance node's exact span.
    pub node_from: u64,
    pub node_to: u64,
    /// The decoded url string to write.
    pub url: String,
    /// B1: the user confirmed editing a shared PROTO definition.
    pub confirm_shared: bool,
}

/// Plan `texture ImageTexture { url "<url>" }` as the LAST statement of an
/// `Appearance` that has no authored `texture` field.
pub fn plan_add_texture(src: &str, req: &AddTextureRequest) -> Plan {
    let p = parse(src);
    let node = match node_gate(&p, req.node_from, req.node_to, req.confirm_shared) {
        Ok(n) => n,
        Err(r) => return refused(r, reason_text(r)),
    };
    if node.node_type != "Appearance" {
        return refused(reason::NOT_APPEARANCE, "Select an Appearance node.");
    }
    if node
        .fields
        .iter()
        .any(|f| matches!(f, Ast::Field(f) if f.name == "texture"))
    {
        return refused(
            reason::TEXTURE_AUTHORED,
            "This Appearance already has a texture field; edit it instead.",
        );
    }
    if req.url.trim().is_empty() {
        return refused(reason::URL_EMPTY, "Choose an image first.");
    }
    let encoded = match encode_string(&req.url) {
        Ok(e) => e,
        Err((r, m)) => return refused(r, m),
    };
    let stmt = format!("texture ImageTexture {{ url {encoded} }}");
    let (nf, nt) = (req.node_from, req.node_to);
    let edit = match body_append(&p, src, node, nf, nt, &stmt) {
        Ok(e) => e,
        Err((r, m)) => return refused(r, m),
    };
    let Some(new_text) = splice(src, std::slice::from_ref(&edit)) else {
        return refused(fr::TRANSACTION_REJECTED, "The edit span is invalid.");
    };
    let to = nt + utf16_len(&edit.insert);
    // Proof: same node, every earlier statement unchanged, the new texture
    // last: exactly one ImageTexture with exactly the one url string.
    let after = parse(&new_text);
    let found = nodes_at(&after, nf, to);
    let texture_ok = |f: &Field| -> bool {
        let Some(Ast::Node(t)) = f.value.as_deref() else {
            return false;
        };
        t.node_type == "ImageTexture"
            && t.def.is_none()
            && !t.incomplete
            && match t.fields.as_slice() {
                [Ast::Field(u)] => {
                    u.name == "url"
                        && !u.is_binding
                        && read_strings(u.value.as_deref(), &new_text)
                            .is_ok_and(|l| l.form == Form::Bare && l.values() == [req.url.clone()])
                }
                _ => false,
            }
    };
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
                    && matches!(n.fields.last(), Some(Ast::Field(f))
                        if f.name == "texture" && !f.is_binding && texture_ok(f))
            }
            _ => false,
        };
    if !ok {
        return refused(
            fr::ROUND_TRIP_FAILED,
            "The new texture could not be proven; nothing changed.",
        );
    }
    Plan::Ready {
        edit,
        new_text,
        node_from: nf,
        node_to: to,
        values: vec![req.url.clone()],
    }
}

#[cfg(test)]
mod tests;
