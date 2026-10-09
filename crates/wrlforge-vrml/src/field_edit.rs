// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed field-value editing (TAURI-RUST-MIGRATION-2). Rust translation of
//! `src/vrml/field-edit.js` (Phase WD2-B): the same nine editable types, the
//! same stable reason ids, the same Annex A lexical gates, the same
//! token-span-only patching and the same round-trip proof.
//!
//! THE DOCUMENT IS THE TEXT (WD.md section 2). A changed scalar token is
//! replaced by a new token; a changed component of a vector, color or rotation
//! is replaced in that component's own span; a changed string replaces only the
//! string token. Every other byte is never examined and never rewritten.
//!
//! FAIL CLOSED. A field is editable only when every fact is PROVEN from the
//! current parse and the generated schema; anything uncertain is read-only with
//! a stable reason id. Nothing guesses a span or searches text for a node.
//!
//! ## Node identity in the Rust lane
//!
//! The JS module proves node membership by object identity in a WD1.4 parse
//! session, and re-anchors through a verified Tier 1 transaction. Neither has
//! been ported. This module uses the narrower proof the Rust desktop can make:
//!
//! * the caller names a node by its exact UTF-16 span in ONE revision of the
//!   canonical text (the service rejects any other revision first);
//! * the span must match EXACTLY ONE node in a full walk of that parse -- zero
//!   is `node-not-found-at-span`, more than one is `node-identity-ambiguous`;
//! * every edit lies strictly inside that node, so after the edit the same node
//!   starts at the same offset and ends at `to + delta`; the re-parse must find
//!   exactly one node there, with the same type, DEF name and field.
//!
//! The Rust lane is STRICTER than the JS one where WD1.5 scope semantics are
//! still missing: a node inside a PROTO body, an EXTERNPROTO or any interface
//! declaration is refused (`node-inside-proto-scope`). USE, ROUTE, PROTO and
//! EXTERNPROTO items are never node instances. Constraints follow the schema
//! header exactly: numeric bounds are enforced, symbolic bounds and notes are
//! never turned into numbers, and nothing is ever clamped.

use crate::ast::{Ast, Node};
use crate::diagnostics::{Code, Severity};
use crate::node_schema::{self, Constraints, FieldSchema};
use crate::parser::{parse, ParseResult};
use crate::tokenizer::{tokenize, Numeric, TokKind};

/// Stable reason ids -- the exact strings of `FIELD_EDIT_REASON` in
/// `src/vrml/field-edit.js`, plus three Rust-lane identity reasons.
pub mod reason {
    pub const OK: &str = "ok";
    pub const NOT_A_NODE: &str = "not-a-node-instance";
    pub const PARSE_INCOMPLETE: &str = "document-parse-incomplete";
    pub const SYNTAX_ERRORS: &str = "document-has-syntax-errors";
    pub const NODE_INCOMPLETE: &str = "node-incomplete";
    pub const UNKNOWN_NODE_TYPE: &str = "node-type-not-standard-vrml97";
    pub const PROTO_INSTANCE: &str = "node-type-is-a-proto-name";
    pub const FIELD_NOT_PRESENT: &str = "field-not-explicitly-authored";
    pub const FIELD_DUPLICATED: &str = "field-authored-more-than-once";
    pub const FIELD_UNKNOWN: &str = "field-not-in-schema";
    pub const FIELD_X3D_ONLY: &str = "field-is-x3d-only";
    pub const FIELD_NOT_STORED: &str = "field-is-not-a-stored-field";
    pub const IS_BOUND: &str = "field-is-is-bound";
    pub const TYPE_UNSUPPORTED: &str = "field-type-not-editable-in-wd2b";
    pub const VALUE_MISSING: &str = "field-value-missing";
    pub const VALUE_SHAPE: &str = "field-value-shape-does-not-match-type";
    pub const VALUE_TOKEN_INVALID: &str = "field-value-token-invalid";
    pub const INPUT_SHAPE: &str = "input-shape-invalid";
    pub const INPUT_NOT_NUMBER: &str = "input-not-a-number";
    pub const INPUT_NOT_FINITE: &str = "input-not-finite";
    pub const INPUT_NOT_INTEGER: &str = "input-not-an-integer";
    pub const INPUT_OUT_OF_RANGE: &str = "input-out-of-range";
    pub const INPUT_NOT_BOOLEAN: &str = "input-not-a-boolean";
    pub const INPUT_NOT_STRING: &str = "input-not-a-string";
    pub const INPUT_STRING_UNENCODABLE: &str = "input-string-not-round-trippable";
    pub const TRANSACTION_REJECTED: &str = "transaction-rejected";
    pub const ROUND_TRIP_FAILED: &str = "round-trip-verification-failed";
    // --- Rust lane: span-proven identity (no WD1.4/WD1.5 port yet) ---------
    pub const NODE_NOT_FOUND: &str = "node-not-found-at-span";
    pub const NODE_AMBIGUOUS: &str = "node-identity-ambiguous";
    pub const NODE_IN_PROTO_SCOPE: &str = "node-inside-proto-scope";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Number,
    Str,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Number => "number",
            Kind::Str => "string",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeSpec {
    pub kind: Kind,
    /// For `Kind::Number`: integer (SFInt32) or float.
    pub int32: bool,
    pub labels: &'static [&'static str],
}

impl TypeSpec {
    pub fn arity(&self) -> usize {
        self.labels.len()
    }
}

const V: &[&str] = &["Value"];

/// The WD2-B editable types, exactly `TYPE_SPECS` in `field-edit.js`.
pub fn type_spec(field_type: &str) -> Option<TypeSpec> {
    let num = |labels| TypeSpec {
        kind: Kind::Number,
        int32: false,
        labels,
    };
    Some(match field_type {
        "SFBool" => TypeSpec {
            kind: Kind::Bool,
            int32: false,
            labels: V,
        },
        "SFInt32" => TypeSpec {
            kind: Kind::Number,
            int32: true,
            labels: V,
        },
        "SFFloat" | "SFTime" => num(V),
        "SFVec2f" => num(&["X", "Y"]),
        "SFVec3f" => num(&["X", "Y", "Z"]),
        "SFColor" => num(&["R", "G", "B"]),
        "SFRotation" => num(&["X", "Y", "Z", "Angle"]),
        "SFString" => TypeSpec {
            kind: Kind::Str,
            int32: false,
            labels: V,
        },
        _ => return None,
    })
}

pub const EDITABLE_TYPES: &[&str] = &[
    "SFBool",
    "SFInt32",
    "SFFloat",
    "SFTime",
    "SFVec2f",
    "SFVec3f",
    "SFColor",
    "SFRotation",
    "SFString",
];

const INT32_MIN: f64 = -2147483648.0;
const INT32_MAX: f64 = 2147483647.0;
const VALUE_EXCERPT_MAX: usize = 120;

/// The numeric bounds the schema represents as numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Option<f64>,
    pub min_inclusive: bool,
    pub max: Option<f64>,
    pub max_inclusive: bool,
}

impl Bounds {
    fn of(c: Option<&Constraints>) -> Option<Bounds> {
        let c = c?;
        let min = c.min.filter(|v| v.is_finite());
        let max = c.max.filter(|v| v.is_finite());
        if min.is_none() && max.is_none() {
            return None;
        }
        Some(Bounds {
            min,
            min_inclusive: min.is_some() && c.min_inclusive == Some(true),
            max,
            max_inclusive: max.is_some() && c.max_inclusive == Some(true),
        })
    }
    fn admits(&self, v: f64) -> bool {
        let low = self
            .min
            .is_none_or(|m| if self.min_inclusive { v >= m } else { v > m });
        let high = self
            .max
            .is_none_or(|m| if self.max_inclusive { v <= m } else { v < m });
        low && high
    }
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(m) = self.min {
            parts.push(format!(
                "{} {m}",
                if self.min_inclusive { "≥" } else { ">" }
            ));
        }
        if let Some(m) = self.max {
            parts.push(format!(
                "{} {m}",
                if self.max_inclusive { "≤" } else { "<" }
            ));
        }
        parts.join(" and ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue {
    Bool(bool),
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub label: &'static str,
    /// Bool: `TRUE`/`FALSE`; number: the EXACT source lexeme; string: decoded.
    pub text: String,
    pub value: ComponentValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDescriptor {
    /// Index into `node.fields` (body ROUTE/PROTO statements count too).
    pub index: usize,
    pub name: String,
    pub field_type: Option<&'static str>,
    pub declaration: Option<&'static str>,
    pub kind: Option<Kind>,
    pub editable: bool,
    pub reason: &'static str,
    pub is_binding: bool,
    pub components: Vec<Component>,
    pub bounds: Option<Bounds>,
    pub constraint_note: Option<&'static str>,
    pub value_excerpt: String,
    /// UTF-16 source span of the value (or the field when it has none).
    pub from: u64,
    pub to: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeFields {
    pub editable: bool,
    pub reason: &'static str,
    pub node_type: Option<String>,
    pub def: Option<String>,
    pub fields: Vec<FieldDescriptor>,
}

/// One user-supplied component: a boolean for SFBool, text otherwise (a
/// number is entered as text and validated here, never in the UI).
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Bool(bool),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The node's exact UTF-16 span in the current text.
    pub node_from: u64,
    pub node_to: u64,
    pub field_index: usize,
    pub field_name: String,
    pub components: Vec<Input>,
}

/// One UTF-16 source span replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanEdit {
    pub from: u64,
    pub to: u64,
    pub insert: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Ready {
        /// Canonical order: ascending, non-overlapping.
        edits: Vec<SpanEdit>,
        new_text: String,
        changed: Vec<usize>,
    },
    Unchanged,
    Refused {
        reason: &'static str,
        message: Option<String>,
        component_index: Option<usize>,
    },
}

fn refuse(reason: &'static str) -> Plan {
    Plan::Refused {
        reason,
        message: None,
        component_index: None,
    }
}

fn refuse_at(reason: &'static str, i: usize, message: String) -> Plan {
    Plan::Refused {
        reason,
        message: Some(message),
        component_index: Some(i),
    }
}

// ---------------------------------------------------------------------------
// Document / node gates
// ---------------------------------------------------------------------------

/// A syntax ERROR other than the header codes moves structural boundaries.
pub fn has_blocking_syntax_error(p: &ParseResult) -> bool {
    p.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && !matches!(d.code, Code::MissingHeader | Code::InvalidHeader)
    })
}

struct Found<'a> {
    node: &'a Node,
    in_proto_scope: bool,
}

fn walk<'a>(ast: &'a Ast, in_proto: bool, f: &mut dyn FnMut(&'a Ast, bool)) {
    f(ast, in_proto);
    match ast {
        Ast::Node(n) => {
            for i in &n.interfaces {
                if let Some(d) = &i.default {
                    walk(d, true, f);
                }
            }
            for x in &n.fields {
                walk(x, in_proto, f);
            }
        }
        Ast::Field(fl) => {
            if let Some(v) = &fl.value {
                walk(v, in_proto, f);
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| walk(i, in_proto, f)),
        Ast::Proto(p) => {
            for i in &p.interfaces {
                if let Some(d) = &i.default {
                    walk(d, true, f);
                }
            }
            p.body.iter().for_each(|b| walk(b, true, f));
        }
        Ast::ExternProto(e) => {
            for i in &e.interfaces {
                if let Some(d) = &i.default {
                    walk(d, true, f);
                }
            }
            if let Some(u) = &e.url {
                walk(u, true, f);
            }
        }
        _ => {}
    }
}

fn walk_doc<'a>(p: &'a ParseResult, f: &mut dyn FnMut(&'a Ast, bool)) {
    for s in &p.tree.statements {
        walk(s, false, f);
    }
}

/// Every node whose span is exactly `[from, to)` -- the identity proof.
fn nodes_at(p: &ParseResult, from: u64, to: u64) -> Vec<Found<'_>> {
    let mut out = Vec::new();
    walk_doc(p, &mut |a, in_proto| {
        if let Ast::Node(n) = a {
            if n.range.start.offset as u64 == from && n.range.end.offset as u64 == to {
                out.push(Found {
                    node: n,
                    in_proto_scope: in_proto,
                });
            }
        }
    });
    out
}

fn proto_names(p: &ParseResult) -> Vec<&str> {
    let mut out = Vec::new();
    walk_doc(p, &mut |a, _| match a {
        Ast::Proto(x) => out.extend(x.name.as_deref()),
        Ast::ExternProto(x) => out.extend(x.name.as_deref()),
        _ => {}
    });
    out
}

/// Locate the node and apply the node-level gate. `Err` is a reason id.
fn node_gate(p: &ParseResult, from: u64, to: u64) -> Result<&Node, &'static str> {
    let found = nodes_at(p, from, to);
    let f = match found.len() {
        0 => return Err(reason::NODE_NOT_FOUND),
        1 => &found[0],
        _ => return Err(reason::NODE_AMBIGUOUS),
    };
    if p.truncated || p.depth_capped {
        return Err(reason::PARSE_INCOMPLETE);
    }
    if has_blocking_syntax_error(p) {
        return Err(reason::SYNTAX_ERRORS);
    }
    let n = f.node;
    if n.incomplete {
        return Err(reason::NODE_INCOMPLETE);
    }
    if !node_schema::is_vrml97_node(&n.node_type) {
        return Err(reason::UNKNOWN_NODE_TYPE);
    }
    if proto_names(p).contains(&n.node_type.as_str()) {
        return Err(reason::PROTO_INSTANCE);
    }
    if f.in_proto_scope {
        return Err(reason::NODE_IN_PROTO_SCOPE);
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// Field gate + descriptor
// ---------------------------------------------------------------------------

struct Tok {
    from: u64,
    to: u64,
    text: String,
    value: ComponentValue,
}

fn components_of(
    spec: &TypeSpec,
    value: Option<&Ast>,
    src: &str,
) -> Result<Vec<Tok>, &'static str> {
    let v = value.ok_or(reason::VALUE_MISSING)?;
    let span = |r: crate::tokenizer::Range| (r.start.offset as u64, r.end.offset as u64);
    match spec.kind {
        Kind::Bool => match v {
            Ast::Bool { value, range } => {
                let (from, to) = span(*range);
                Ok(vec![Tok {
                    from,
                    to,
                    text: if *value { "TRUE" } else { "FALSE" }.into(),
                    value: ComponentValue::Bool(*value),
                }])
            }
            _ => Err(reason::VALUE_SHAPE),
        },
        Kind::Str => match v {
            Ast::Str { value, range } => {
                let raw = range.slice(src);
                if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
                    return Err(reason::VALUE_TOKEN_INVALID);
                }
                let (from, to) = span(*range);
                Ok(vec![Tok {
                    from,
                    to,
                    text: value.clone(),
                    value: ComponentValue::Str(value.clone()),
                }])
            }
            _ => Err(reason::VALUE_SHAPE),
        },
        Kind::Number => {
            // A field value number run is always `Numbers` (as JS `NUMBERS`);
            // `[1 2 3]` is an array and is the wrong shape for an SF type.
            let Ast::Numbers { values, .. } = v else {
                return Err(reason::VALUE_SHAPE);
            };
            if values.len() != spec.arity() {
                return Err(reason::VALUE_SHAPE);
            }
            values
                .iter()
                .map(|n| {
                    let ok_kind = if spec.int32 {
                        matches!(n.numeric, Numeric::Int | Numeric::Hex)
                    } else {
                        matches!(n.numeric, Numeric::Int | Numeric::Float)
                    };
                    if !n.valid || !n.value.is_finite() || !ok_kind {
                        return Err(reason::VALUE_TOKEN_INVALID);
                    }
                    let (from, to) = span(n.range);
                    Ok(Tok {
                        from,
                        to,
                        text: n.range.slice(src).to_string(),
                        value: ComponentValue::Num(n.value),
                    })
                })
                .collect()
        }
    }
}

fn excerpt(raw: &str) -> String {
    if raw.chars().count() > VALUE_EXCERPT_MAX {
        let mut s: String = raw.chars().take(VALUE_EXCERPT_MAX).collect();
        s.push('…');
        s
    } else {
        raw.to_string()
    }
}

struct Described {
    descriptor: FieldDescriptor,
    tokens: Vec<Tok>,
    spec: Option<TypeSpec>,
}

fn describe(
    node: &Node,
    index: usize,
    node_reason: Option<&'static str>,
    duplicates: &[&str],
    src: &str,
) -> Option<Described> {
    let Ast::Field(f) = node.fields.get(index)? else {
        return None;
    };
    let record: Option<&FieldSchema> = node_schema::get_field_schema(&node.node_type, &f.name);
    let vrml97 = record.filter(|r| r.profiles.contains(&"vrml97"));
    let spec = vrml97.and_then(|r| type_spec(r.field_type));
    let mut tokens = Vec::new();
    let reason = node_reason.or_else(|| {
        if duplicates.contains(&f.name.as_str()) {
            Some(reason::FIELD_DUPLICATED)
        } else if f.is_binding || matches!(f.value.as_deref(), Some(Ast::Is { .. })) {
            Some(reason::IS_BOUND)
        } else if record.is_none() {
            Some(reason::FIELD_UNKNOWN)
        } else if vrml97.is_none() {
            Some(reason::FIELD_X3D_ONLY)
        } else if !matches!(
            vrml97.and_then(|r| r.vrml97_declaration),
            Some("field" | "exposedField")
        ) {
            Some(reason::FIELD_NOT_STORED)
        } else if let Some(spec) = &spec {
            match components_of(spec, f.value.as_deref(), src) {
                Ok(t) => {
                    tokens = t;
                    None
                }
                Err(r) => Some(r),
            }
        } else {
            Some(reason::TYPE_UNSUPPORTED)
        }
    });
    let editable = reason.is_none();
    let range = f.value.as_ref().map(|v| v.range()).unwrap_or(f.range);
    let components = if editable {
        let labels = spec.map(|s| s.labels).unwrap_or(&[]);
        tokens
            .iter()
            .zip(labels)
            .map(|(t, l)| Component {
                label: l,
                text: t.text.clone(),
                value: t.value.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(Described {
        descriptor: FieldDescriptor {
            index,
            name: f.name.clone(),
            field_type: vrml97.map(|r| r.field_type),
            declaration: vrml97.and_then(|r| r.vrml97_declaration),
            kind: spec.map(|s| s.kind),
            editable,
            reason: reason.unwrap_or(reason::OK),
            is_binding: f.is_binding,
            components,
            bounds: if editable && spec.is_some_and(|s| s.kind == Kind::Number) {
                Bounds::of(vrml97.and_then(|r| r.constraints))
            } else {
                None
            },
            constraint_note: vrml97
                .and_then(|r| r.constraints)
                .and_then(|c| c.note)
                .map(|n| n.category),
            value_excerpt: excerpt(range.slice(src)),
            from: range.start.offset as u64,
            to: range.end.offset as u64,
        },
        tokens,
        spec,
    })
}

fn duplicate_names(node: &Node) -> Vec<&str> {
    let mut seen: Vec<&str> = Vec::new();
    let mut dup = Vec::new();
    for f in &node.fields {
        if let Ast::Field(f) = f {
            if seen.contains(&f.name.as_str()) {
                dup.push(f.name.as_str());
            }
            seen.push(&f.name);
        }
    }
    dup
}

/// Describe every explicitly authored field of the node at `[from, to)`.
pub fn inspect_node_fields(p: &ParseResult, src: &str, from: u64, to: u64) -> NodeFields {
    let found = nodes_at(p, from, to);
    let node = match found.as_slice() {
        [one] => one.node,
        many => {
            return NodeFields {
                editable: false,
                reason: if many.is_empty() {
                    reason::NOT_A_NODE
                } else {
                    reason::NODE_AMBIGUOUS
                },
                node_type: None,
                def: None,
                fields: Vec::new(),
            }
        }
    };
    let node_reason = node_gate(p, from, to).err();
    let dups = duplicate_names(node);
    let fields = (0..node.fields.len())
        .filter_map(|i| describe(node, i, node_reason, &dups, src).map(|d| d.descriptor))
        .collect();
    NodeFields {
        editable: node_reason.is_none(),
        reason: node_reason.unwrap_or(reason::OK),
        node_type: Some(node.node_type.clone()),
        def: node.def.clone(),
        fields,
    }
}

// ---------------------------------------------------------------------------
// Input validation + token encoding
// ---------------------------------------------------------------------------

/// VRML97 Annex A.3 `float`: `[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?`.
fn is_float_lexeme(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if int_digits == 0 && i == frac_start {
            return false;
        }
    } else if int_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

/// VRML97 Annex A.3 `int32`: `[+-]?(0[xX][0-9a-fA-F]+|\d+)`.
fn is_int32_lexeme(s: &str) -> bool {
    let t = s.strip_prefix(['+', '-']).unwrap_or(s);
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return !h.is_empty() && h.bytes().all(|c| c.is_ascii_hexdigit());
    }
    !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit())
}

/// `encodeNumber`: the candidate token for one component, or a refusal.
fn encode_number(
    spec: &TypeSpec,
    raw: &str,
    bounds: Option<&Bounds>,
) -> Result<(String, f64), (&'static str, String)> {
    let text = raw.trim();
    let not_number = |m: String| (reason::INPUT_NOT_NUMBER, m);
    if text.is_empty() {
        return Err(not_number("Enter a number.".into()));
    }
    let int = spec.int32;
    let lexical = if int {
        is_int32_lexeme(text)
    } else {
        is_float_lexeme(text)
    };
    if !lexical {
        return Err(if int && is_float_lexeme(text) {
            (
                reason::INPUT_NOT_INTEGER,
                "Enter a whole number (SFInt32).".into(),
            )
        } else {
            not_number(format!(
                "\"{text}\" is not a {}.",
                if int { "whole number" } else { "number" }
            ))
        });
    }
    // The tokenizer is the lexical authority: exactly one NUMBER token that
    // spells itself, then EOF.
    let lexed = tokenize(text);
    let value = match lexed.tokens.as_slice() {
        [tok, eof] if matches!(eof.kind, TokKind::Eof) && tok.lexeme(text) == text => {
            match tok.kind {
                TokKind::Number { value, valid, .. } => {
                    if !valid || !value.is_finite() {
                        return Err((
                            reason::INPUT_NOT_FINITE,
                            format!("\"{text}\" is not a finite number."),
                        ));
                    }
                    value
                }
                _ => return Err(not_number(format!("\"{text}\" is not a number."))),
            }
        }
        _ => return Err(not_number(format!("\"{text}\" is not a number."))),
    };
    if int {
        if value.fract() != 0.0 {
            return Err((
                reason::INPUT_NOT_INTEGER,
                "Enter a whole number (SFInt32).".into(),
            ));
        }
        if !(INT32_MIN..=INT32_MAX).contains(&value) {
            return Err((
                reason::INPUT_OUT_OF_RANGE,
                "SFInt32 must be between -2147483648 and 2147483647.".into(),
            ));
        }
    }
    if let Some(b) = bounds {
        if !b.admits(value) {
            return Err((
                reason::INPUT_OUT_OF_RANGE,
                format!("Value must be {}.", b.text()),
            ));
        }
    }
    Ok((text.to_string(), value))
}

/// `encodeString`: one quoted VRML97 string token, proven by re-tokenizing.
/// A CR is refused (the tokenizer decodes line breaks, so it cannot
/// round-trip); so is an LF, because inserting one would add a line ending
/// the document does not use -- the Inspector edits a single-line value.
pub fn encode_string(value: &str) -> Result<String, (&'static str, String)> {
    if value.contains('\r') || value.contains('\n') {
        return Err((
            reason::INPUT_STRING_UNENCODABLE,
            "Line breaks cannot be entered in an Inspector SFString.".into(),
        ));
    }
    let mut text = String::with_capacity(value.len() + 2);
    text.push('"');
    for c in value.chars() {
        if c == '\\' || c == '"' {
            text.push('\\');
        }
        text.push(c);
    }
    text.push('"');
    let lexed = tokenize(&text);
    let ok = match lexed.tokens.as_slice() {
        [tok, eof] => {
            matches!(eof.kind, TokKind::Eof)
                && tok.lexeme(&text) == text
                && matches!(&tok.kind, TokKind::Str { value: v, terminated: true } if v == value)
        }
        _ => false,
    };
    if ok {
        Ok(text)
    } else {
        Err((
            reason::INPUT_STRING_UNENCODABLE,
            "This text cannot be encoded as a VRML97 string.".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

fn utf16_len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

/// UTF-16 offset -> byte offset, refusing an offset inside a surrogate pair.
fn byte_at(src: &str, offset: u64) -> Option<usize> {
    let mut u = 0u64;
    for (i, c) in src.char_indices() {
        if u == offset {
            return Some(i);
        }
        if u > offset {
            return None;
        }
        u += c.len_utf16() as u64;
    }
    (u == offset).then_some(src.len())
}

/// Splice ascending, non-overlapping edits; `None` if any span is invalid.
fn splice(src: &str, edits: &[SpanEdit]) -> Option<String> {
    let mut out = String::with_capacity(src.len() + 16);
    let mut at = 0usize;
    for e in edits {
        let from = byte_at(src, e.from)?;
        let to = byte_at(src, e.to)?;
        if from < at || to < from {
            return None;
        }
        out.push_str(&src[at..from]);
        out.push_str(&e.insert);
        at = to;
    }
    out.push_str(&src[at..]);
    Some(out)
}

/// Prove the edit did exactly what was asked: no new structural error, the
/// SAME node (same start, end shifted by the delta, exactly one match, same
/// type and DEF), the same field at the same index, every component now
/// holding the intended token.
fn round_trip(
    node: &Node,
    req: &Request,
    spec: &TypeSpec,
    intended: &[(Option<String>, ComponentValue)],
    delta: i64,
    new_text: &str,
) -> bool {
    let parsed = parse(new_text);
    if has_blocking_syntax_error(&parsed) || parsed.truncated || parsed.depth_capped {
        return false;
    }
    let Some(to) = (req.node_to as i64).checked_add(delta).filter(|t| *t >= 0) else {
        return false;
    };
    let found = nodes_at(&parsed, req.node_from, to as u64);
    let [again] = found.as_slice() else {
        return false;
    };
    let n = again.node;
    if n.node_type != node.node_type || n.def != node.def || again.in_proto_scope {
        return false;
    }
    let Some(Ast::Field(f)) = n.fields.get(req.field_index) else {
        return false;
    };
    if f.name != req.field_name || f.is_binding {
        return false;
    }
    let Ok(toks) = components_of(spec, f.value.as_deref(), new_text) else {
        return false;
    };
    toks.len() == intended.len()
        && toks.iter().zip(intended).all(|(t, (text, value))| {
            t.value == *value && text.as_ref().is_none_or(|want| &t.text == want)
        })
}

/// Turn a typed Inspector value into a verified source edit set over `src`.
/// Never panics for ordinary invalid input.
pub fn plan_field_edit(src: &str, req: &Request) -> Plan {
    let p = parse(src);
    let node = match node_gate(&p, req.node_from, req.node_to) {
        Ok(n) => n,
        Err(r) => return refuse(r),
    };
    match node.fields.get(req.field_index) {
        Some(Ast::Field(f)) if f.name == req.field_name => {}
        _ => return refuse(reason::FIELD_NOT_PRESENT),
    }
    let dups = duplicate_names(node);
    let Some(d) = describe(node, req.field_index, None, &dups, src) else {
        return refuse(reason::FIELD_NOT_PRESENT);
    };
    if !d.descriptor.editable {
        return refuse(d.descriptor.reason);
    }
    let Some(spec) = d.spec else {
        return refuse(reason::TYPE_UNSUPPORTED);
    };
    if req.components.len() != spec.arity() {
        return refuse(reason::INPUT_SHAPE);
    }

    let mut edits = Vec::new();
    let mut intended = Vec::new();
    let mut changed = Vec::new();
    for (i, raw) in req.components.iter().enumerate() {
        let current = &d.descriptor.components[i];
        let tok = &d.tokens[i];
        let (want, insert) = match (spec.kind, raw) {
            (Kind::Bool, Input::Bool(b)) => {
                let ins = (ComponentValue::Bool(*b) != current.value)
                    .then(|| if *b { "TRUE" } else { "FALSE" }.to_string());
                ((None, ComponentValue::Bool(*b)), ins)
            }
            (Kind::Bool, _) => {
                return refuse_at(reason::INPUT_NOT_BOOLEAN, i, "Choose TRUE or FALSE.".into())
            }
            (Kind::Str, Input::Text(s)) => {
                let enc = match encode_string(s) {
                    Ok(t) => t,
                    Err((r, m)) => return refuse_at(r, i, m),
                };
                let ins = (ComponentValue::Str(s.clone()) != current.value).then_some(enc);
                ((None, ComponentValue::Str(s.clone())), ins)
            }
            (Kind::Str, _) => return refuse_at(reason::INPUT_NOT_STRING, i, "Enter text.".into()),
            (Kind::Number, Input::Text(s)) => {
                if s.trim() == current.text {
                    ((Some(current.text.clone()), current.value.clone()), None)
                } else {
                    match encode_number(&spec, s, d.descriptor.bounds.as_ref()) {
                        Ok((text, value)) => {
                            let ins = (text != current.text).then(|| text.clone());
                            ((Some(text), ComponentValue::Num(value)), ins)
                        }
                        Err((r, m)) => return refuse_at(r, i, m),
                    }
                }
            }
            (Kind::Number, _) => {
                return refuse_at(reason::INPUT_NOT_NUMBER, i, "Enter a number.".into())
            }
        };
        intended.push(want);
        if let Some(insert) = insert {
            edits.push(SpanEdit {
                from: tok.from,
                to: tok.to,
                insert,
            });
            changed.push(i);
        }
    }
    if edits.is_empty() {
        return Plan::Unchanged;
    }
    // Every edit must lie strictly inside the node: that is what keeps the
    // node's start fixed and makes the re-parse identity check sound.
    let inside = edits
        .iter()
        .all(|e| e.from > req.node_from && e.to < req.node_to);
    let ascending = edits.windows(2).all(|w| w[0].to <= w[1].from);
    if !inside || !ascending {
        return refuse(reason::TRANSACTION_REJECTED);
    }
    let Some(new_text) = splice(src, &edits) else {
        return refuse(reason::TRANSACTION_REJECTED);
    };
    let delta: i64 = edits
        .iter()
        .map(|e| utf16_len(&e.insert) as i64 - (e.to - e.from) as i64)
        .sum();
    if !round_trip(node, req, &spec, &intended, delta, &new_text) {
        return refuse(reason::ROUND_TRIP_FAILED);
    }
    Plan::Ready {
        edits,
        new_text,
        changed,
    }
}

#[cfg(test)]
mod tests;
