// SPDX-License-Identifier: GPL-3.0-or-later
//! B2: Delete node and Duplicate node, as exact source-text patches.
//!
//! THE DOCUMENT IS THE TEXT (WD.md section 2). Delete removes exactly the
//! node's source span plus the whitespace that belonged to it; Duplicate
//! inserts a verbatim copy of the node's source text right after it. Nothing
//! is regenerated from a tree and no unrelated byte is touched.
//!
//! Identity is the exact UTF-16 node span (`field_edit::nodes_at`): exactly
//! one node or a refusal. The scope rules follow `field_edit::node_gate`
//! (syntax errors, parser caps, PROTO bodies need the user's consent for the
//! shared definition, interface / EXTERNPROTO scopes are refused), except
//! that a PROTO instance or non-standard node type may be deleted or
//! duplicated: the operation is structural and never reads the node's fields.
//!
//! FAIL CLOSED:
//!
//! * a node may be removed only from a statement list (the document or a
//!   PROTO body) or from an MFNode array; a single-node field value is
//!   refused, and so is the first node of a PROTO body (it is the PROTO's
//!   type);
//! * Delete is refused when any DEF name inside the node is spelled by a USE
//!   or ROUTE outside it, in any scope (conservative: names, not bindings);
//! * Duplicate renames every DEF in the copy to a free name (`Name_2`, ...)
//!   no identifier token in the document spells; a USE in the copy is renamed
//!   only when the scope graph RESOLVES it to a DEF inside the node; a ROUTE
//!   naming a renamed DEF, an unresolved USE of one, or a PROTO declaration
//!   inside the node is refused;
//! * the result is re-parsed and every statement, node, field, USE and ROUTE
//!   must be exactly where the edit says (old ones shifted, removed ones gone,
//!   the copy's at the new span). Otherwise the plan is refused.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Node};
use crate::create::{dominant_eol, incomplete};
use crate::field_edit::{
    has_blocking_syntax_error, nodes_at, proto_names, reason as fr, splice, utf16_len, walk_doc,
    SpanEdit,
};
use crate::parser::{parse, ParseResult};
use crate::scope;
use crate::tokenizer::{Range, TokKind};

/// Stable reason ids. The identity / scope ones are `field_edit`'s.
pub mod reason {
    pub use crate::field_edit::reason::{
        NODE_AMBIGUOUS, NODE_INCOMPLETE, NODE_IN_PROTO_SCOPE, NODE_NOT_FOUND, PARSE_INCOMPLETE,
        PROTO_NAME_AMBIGUOUS, SHARED_DEFINITION_UNCONFIRMED, SYNTAX_ERRORS,
    };
    /// The node is the single node value of a field (`geometry Box {}`).
    pub const FIELD_VALUE: &str = "node-is-a-single-node-field-value";
    /// The first node of a PROTO body: it is the PROTO's node type.
    pub const PROTO_TYPE_NODE: &str = "node-defines-the-proto-type";
    /// A DEF inside the node is named by a USE / ROUTE outside it.
    pub const DEF_REFERENCED: &str = "def-name-referenced-elsewhere";
    /// Renaming a DEF in the copy cannot be proven safe.
    pub const RENAME_NOT_PROVABLE: &str = "def-rename-not-provable";
    /// The node holds a PROTO / EXTERNPROTO declaration.
    pub const CONTAINS_DECLARATION: &str = "node-contains-proto-declaration";
    pub const NO_FREE_NAME: &str = "no-free-def-name";
    pub const VERIFY_FAILED: &str = "structure-edit-not-proven";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Ready {
        /// Exactly one span edit (UTF-16, source coordinates).
        edit: SpanEdit,
        new_text: String,
        /// Duplicate: the copy's exact span in `new_text`. Delete: `None`.
        node: Option<(u64, u64)>,
        /// Duplicate: `(old, new)` DEF names renamed in the copy.
        renamed: Vec<(String, String)>,
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

/// Where the node sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// A document statement.
    Statement,
    /// A PROTO body statement; `first`: the body's first node.
    ProtoBody { first: bool },
    /// An item of an MFNode array value.
    ArrayItem,
    /// The single node value of a field.
    FieldValue,
}

#[derive(Clone, Copy)]
enum List {
    Doc,
    Body,
    Array,
}

fn exact(a: &Ast, from: u64, to: u64) -> bool {
    matches!(a, Ast::Node(n) if n.range.start.offset as u64 == from && n.range.end.offset as u64 == to)
}

fn in_list(items: &[Ast], kind: List, from: u64, to: u64) -> Option<Slot> {
    let first = items.iter().position(|a| matches!(a, Ast::Node(_)));
    for (i, it) in items.iter().enumerate() {
        if exact(it, from, to) {
            return Some(match kind {
                List::Doc => Slot::Statement,
                List::Body => Slot::ProtoBody {
                    first: first == Some(i),
                },
                List::Array => Slot::ArrayItem,
            });
        }
        if let Some(s) = inside(it, from, to) {
            return Some(s);
        }
    }
    None
}

fn inside(a: &Ast, from: u64, to: u64) -> Option<Slot> {
    match a {
        Ast::Node(n) => {
            for i in &n.interfaces {
                if let Some(s) = i.default.as_deref().and_then(|d| value(d, from, to)) {
                    return Some(s);
                }
            }
            n.fields.iter().find_map(|f| inside(f, from, to))
        }
        Ast::Field(f) => f.value.as_deref().and_then(|v| value(v, from, to)),
        Ast::Proto(p) => p
            .interfaces
            .iter()
            .find_map(|i| i.default.as_deref().and_then(|d| value(d, from, to)))
            .or_else(|| in_list(&p.body, List::Body, from, to)),
        Ast::ExternProto(e) => e
            .interfaces
            .iter()
            .find_map(|i| i.default.as_deref().and_then(|d| value(d, from, to))),
        _ => None,
    }
}

/// A field (or interface default) value.
fn value(v: &Ast, from: u64, to: u64) -> Option<Slot> {
    if exact(v, from, to) {
        return Some(Slot::FieldValue);
    }
    match v {
        Ast::Array { items, .. } => in_list(items, List::Array, from, to),
        other => inside(other, from, to),
    }
}

/// The node at `[from, to)` and its slot, after the document and scope
/// gates. `Err` is a refusal.
fn gate(p: &ParseResult, from: u64, to: u64, confirm_shared: bool) -> Result<(&Node, Slot), Plan> {
    let found = nodes_at(p, from, to);
    let f = match found.len() {
        0 => {
            return Err(refused(
                fr::NODE_NOT_FOUND,
                "The selected node is not in the current text; re-select it.",
            ))
        }
        1 => &found[0],
        _ => {
            return Err(refused(
                fr::NODE_AMBIGUOUS,
                "More than one node has this span; nothing changed.",
            ))
        }
    };
    if p.truncated || p.depth_capped {
        return Err(refused(
            fr::PARSE_INCOMPLETE,
            "The document exceeds the parser limits; its structure cannot be proven.",
        ));
    }
    if has_blocking_syntax_error(p) || p.tree.statements.iter().any(incomplete) {
        return Err(refused(
            fr::SYNTAX_ERRORS,
            "Fix the syntax errors first: node boundaries are not provable.",
        ));
    }
    if f.node.incomplete {
        return Err(refused(fr::NODE_INCOMPLETE, "The node is incomplete."));
    }
    if f.other_proto_scope() {
        return Err(refused(
            fr::NODE_IN_PROTO_SCOPE,
            "The node is in a PROTO interface, an EXTERNPROTO or a Script interface default.",
        ));
    }
    if let Some(name) = f.shared_proto {
        if proto_names(p).iter().filter(|n| **n == name).count() != 1 {
            return Err(refused(
                fr::PROTO_NAME_AMBIGUOUS,
                format!("PROTO {name} is declared more than once."),
            ));
        }
        if !confirm_shared {
            return Err(refused(
                fr::SHARED_DEFINITION_UNCONFIRMED,
                format!("The node is part of the shared definition PROTO {name}; confirm first."),
            ));
        }
    }
    let Some(slot) = in_list(&p.tree.statements, List::Doc, from, to) else {
        return Err(refused(
            fr::NODE_NOT_FOUND,
            "The node's place in the document is not provable.",
        ));
    };
    if slot == Slot::FieldValue {
        return Err(refused(
            reason::FIELD_VALUE,
            "The node is the single value of a field; delete or duplicate it inside a [ ] list, \
             or edit the field in the Source.",
        ));
    }
    Ok((f.node, slot))
}

fn within(r: Range, from: u64, to: u64) -> bool {
    r.start.offset as u64 >= from && r.end.offset as u64 <= to
}

/// One verifiable fact about a parse: (from, to, kind, detail).
type Sig = (u64, u64, &'static str, String);

fn sig_of(a: &Ast) -> Sig {
    let r = a.range();
    let detail = match a {
        Ast::Node(n) => format!("{}/{}", n.node_type, n.def.as_deref().unwrap_or("")),
        Ast::Use { name, .. } | Ast::Is { name, .. } => name.clone().unwrap_or_default(),
        Ast::Field(f) => format!("{}/{}", f.name, f.is_binding),
        Ast::Route(rt) => format!(
            "{:?}.{:?}>{:?}.{:?}",
            rt.from.node, rt.from.event, rt.to.node, rt.to.event
        ),
        Ast::Proto(p) => p.name.clone().unwrap_or_default(),
        Ast::ExternProto(e) => e.name.clone().unwrap_or_default(),
        _ => String::new(),
    };
    (
        r.start.offset as u64,
        r.end.offset as u64,
        a.kind_name(),
        detail,
    )
}

fn sigs(p: &ParseResult) -> Vec<Sig> {
    let mut out = Vec::new();
    walk_doc(p, &mut |a, _| out.push(sig_of(a)));
    out
}

fn sound(p: &ParseResult) -> bool {
    !(p.truncated
        || p.depth_capped
        || has_blocking_syntax_error(p)
        || p.tree.statements.iter().any(incomplete))
}

fn same(mut a: Vec<Sig>, mut b: Vec<Sig>) -> bool {
    a.sort();
    b.sort();
    a == b
}

fn is_hspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b',')
}

fn is_eol(c: u8) -> bool {
    matches!(c, b'\n' | b'\r')
}

/// The node's byte span and the facts about its line: `(ls, te)` the
/// horizontal whitespace run before and after it, and whether those reach
/// the line start / line end.
struct Around {
    a: usize,
    b: usize,
    ls: usize,
    te: usize,
    line_start: bool,
    line_end: bool,
}

fn around(src: &str, n: &Node) -> Around {
    let s = src.as_bytes();
    let (a, b) = (n.range.start.byte as usize, n.range.end.byte as usize);
    let mut ls = a;
    while ls > 0 && is_hspace(s[ls - 1]) {
        ls -= 1;
    }
    let mut te = b;
    while te < s.len() && is_hspace(s[te]) {
        te += 1;
    }
    Around {
        a,
        b,
        ls,
        te,
        line_start: ls == 0 || is_eol(s[ls - 1]),
        line_end: te == s.len() || is_eol(s[te]),
    }
}

/// The byte range a Delete removes: the node plus the whitespace that
/// belonged to it.
fn removal(src: &str, x: &Around) -> (usize, usize) {
    let s = src.as_bytes();
    if x.line_start && x.line_end {
        // The node owns its lines: remove them with one line terminator.
        if x.te < s.len() {
            let eol = if s[x.te] == b'\r' && s.get(x.te + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            return (x.ls, x.te + eol);
        }
        // Last line without a terminator: take the preceding one instead.
        let mut p = x.ls;
        if p > 0 {
            p -= 1;
            if s[p] == b'\n' && p > 0 && s[p - 1] == b'\r' {
                p -= 1;
            }
        }
        return (p, s.len());
    }
    if x.te > x.b && !x.line_end {
        // `[ A {} B {} ]`: the node and the separator after it.
        return (x.a, x.te);
    }
    if x.line_start {
        (x.a, x.b)
    } else {
        // `children [ A {}` at a line end: the separator before it.
        (x.ls, x.b)
    }
}

/// Every DEF inside `[from, to)`: (name, name token range).
fn defs_inside(p: &ParseResult, from: u64, to: u64) -> Vec<(String, Range)> {
    let mut out = Vec::new();
    walk_doc(p, &mut |a, _| {
        if let Ast::Node(n) = a {
            if let (Some(d), Some(r)) = (&n.def, n.def_range) {
                if within(n.range, from, to) {
                    out.push((d.clone(), r));
                }
            }
        }
    });
    out
}

/// Plan the deletion of the node at `[node_from, node_to)`.
pub fn plan_delete(src: &str, node_from: u64, node_to: u64, confirm_shared: bool) -> Plan {
    let p = parse(src);
    let (node, slot) = match gate(&p, node_from, node_to, confirm_shared) {
        Ok(x) => x,
        Err(r) => return r,
    };
    if slot == (Slot::ProtoBody { first: true }) {
        return refused(
            reason::PROTO_TYPE_NODE,
            "The first node of a PROTO body defines the PROTO's type; it cannot be deleted.",
        );
    }
    // Every DEF the node takes with it must be unnamed outside it.
    let names: HashSet<String> = defs_inside(&p, node_from, node_to)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let mut blocker: Option<(String, &'static str, u32)> = None;
    walk_doc(&p, &mut |a, _| {
        if blocker.is_some() || within(a.range(), node_from, node_to) {
            return;
        }
        let hit = |n: &Option<String>| n.as_ref().filter(|n| names.contains(*n)).cloned();
        let found = match a {
            Ast::Use { name, .. } => hit(name).map(|n| (n, "USE")),
            Ast::Route(r) => hit(&r.from.node)
                .or_else(|| hit(&r.to.node))
                .map(|n| (n, "ROUTE")),
            _ => None,
        };
        if let Some((n, what)) = found {
            blocker = Some((n, what, a.range().start.line));
        }
    });
    if let Some((name, what, line)) = blocker {
        return refused(
            reason::DEF_REFERENCED,
            format!("DEF {name} is still named by a {what} on line {line}; remove that first."),
        );
    }
    let x = around(src, node);
    let (r0, r1) = removal(src, &x);
    let from = utf16_len(&src[..r0]);
    let to = from + utf16_len(&src[r0..r1]);
    let edit = SpanEdit {
        from,
        to,
        insert: String::new(),
    };
    let Some(new_text) = splice(src, std::slice::from_ref(&edit)) else {
        return refused(reason::VERIFY_FAILED, "The deletion span is not valid.");
    };
    // Verify: everything outside the node is still there, shifted exactly.
    let len = to - from;
    let map = |o: u64| -> Option<u64> {
        if o <= from {
            Some(o)
        } else if o >= to {
            Some(o - len)
        } else {
            None
        }
    };
    let expected: Option<Vec<Sig>> = sigs(&p)
        .into_iter()
        .filter(|s| !(s.0 >= node_from && s.1 <= node_to))
        .map(|(a, b, k, d)| Some((map(a)?, map(b)?, k, d)))
        .collect();
    let after = parse(&new_text);
    let proven = sound(&after) && expected.is_some_and(|e| same(e, sigs(&after)));
    if !proven {
        return refused(
            reason::VERIFY_FAILED,
            "The deletion could not be proven in the resulting document; nothing changed.",
        );
    }
    Plan::Ready {
        edit,
        new_text,
        node: None,
        renamed: Vec::new(),
    }
}

/// `Name_2`, `Name_3`, ... (a trailing `_<n>` is replaced, not extended):
/// the first spelling no identifier token in the document and no earlier
/// choice uses.
fn free_name(used: &HashSet<&str>, taken: &HashSet<String>, name: &str) -> Option<String> {
    let stem = match name.rsplit_once('_') {
        Some((s, n)) if !s.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            s
        }
        _ => name,
    };
    (2..=100_000)
        .map(|n| format!("{stem}_{n}"))
        .find(|c| !used.contains(c.as_str()) && !taken.contains(c))
}

/// Plan the duplication of the node at `[node_from, node_to)`: a copy of its
/// exact source text right after it, DEF names renamed in the copy.
pub fn plan_duplicate(src: &str, node_from: u64, node_to: u64, confirm_shared: bool) -> Plan {
    let p = parse(src);
    let (node, _slot) = match gate(&p, node_from, node_to, confirm_shared) {
        Ok(x) => x,
        Err(r) => return r,
    };
    // What the copy holds.
    let mut decl = false;
    let mut uses: Vec<(String, Range, Range)> = Vec::new();
    let mut routes: Vec<(Option<String>, Option<String>, u32)> = Vec::new();
    walk_doc(&p, &mut |a, _| {
        if !within(a.range(), node_from, node_to) {
            return;
        }
        match a {
            Ast::Proto(_) | Ast::ExternProto(_) => decl = true,
            Ast::Use {
                name: Some(n),
                name_range: Some(nr),
                range,
            } => uses.push((n.clone(), *nr, *range)),
            Ast::Route(r) => {
                routes.push((r.from.node.clone(), r.to.node.clone(), r.range.start.line))
            }
            _ => {}
        }
    });
    if decl {
        return refused(
            reason::CONTAINS_DECLARATION,
            "The node holds a PROTO or EXTERNPROTO declaration; a copy would declare it twice.",
        );
    }
    let defs = defs_inside(&p, node_from, node_to);
    let def_names: HashSet<&str> = defs.iter().map(|(n, _)| n.as_str()).collect();
    if let Some((_, _, line)) = routes.iter().find(|(f, t, _)| {
        [f, t]
            .iter()
            .any(|n| n.as_deref().is_some_and(|n| def_names.contains(n)))
    }) {
        return refused(
            reason::RENAME_NOT_PROVABLE,
            format!(
                "The ROUTE on line {line} inside the node names one of its DEFs; \
                 duplicating it would need an unproven rename."
            ),
        );
    }
    // Fresh names, one per DEF token.
    let used: HashSet<&str> = p
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokKind::Id))
        .map(|t| t.lexeme(src))
        .collect();
    let mut taken: HashSet<String> = HashSet::new();
    let mut new_name: HashMap<u32, String> = HashMap::new();
    let mut renamed = Vec::new();
    let mut edits: Vec<(Range, String)> = Vec::new();
    for (name, r) in &defs {
        let Some(n) = free_name(&used, &taken, name) else {
            return refused(reason::NO_FREE_NAME, "No free DEF name was found.");
        };
        taken.insert(n.clone());
        new_name.insert(r.start.offset, n.clone());
        renamed.push((name.clone(), n.clone()));
        edits.push((*r, n));
    }
    // A USE of a renamed name follows its DEF only when the scope graph
    // resolves it to exactly that DEF.
    if uses.iter().any(|(n, _, _)| def_names.contains(n.as_str())) {
        let g = scope::build(&p);
        for (n, nr, range) in &uses {
            if !def_names.contains(n.as_str()) {
                continue;
            }
            let target = g
                .use_at(*range)
                .and_then(|r| g.edit_target(r))
                .and_then(|d| new_name.get(&d.name_range.start.offset));
            let Some(t) = target else {
                return refused(
                    reason::RENAME_NOT_PROVABLE,
                    format!(
                        "USE {n} on line {} does not provably name a DEF inside the node.",
                        nr.start.line
                    ),
                );
            };
            edits.push((*nr, t.clone()));
        }
    }
    // The copy: the node's exact text with the renames applied.
    edits.sort_by_key(|(r, _)| r.start.byte);
    let (a, b) = (node.range.start.byte as usize, node.range.end.byte as usize);
    let mut copy = String::with_capacity(b - a + 16);
    let mut at = a;
    for (r, n) in &edits {
        let (f, t) = (r.start.byte as usize, r.end.byte as usize);
        if f < at || t > b {
            return refused(reason::VERIFY_FAILED, "The rename spans overlap.");
        }
        copy.push_str(&src[at..f]);
        copy.push_str(n);
        at = t;
    }
    copy.push_str(&src[at..b]);

    // Separated like its siblings: on its own line with the same indent, or
    // one space after it on a shared line.
    let x = around(src, node);
    let sep = if x.line_start {
        let indent: String = src[x.ls..x.a].chars().filter(|c| *c != ',').collect();
        format!("{}{indent}", dominant_eol(src))
    } else {
        " ".to_string()
    };
    let insert = format!("{sep}{copy}");
    let edit = SpanEdit {
        from: node_to,
        to: node_to,
        insert,
    };
    let Some(new_text) = splice(src, std::slice::from_ref(&edit)) else {
        return refused(reason::VERIFY_FAILED, "The insertion point is not valid.");
    };
    let new_from = node_to + utf16_len(&sep);
    let new_to = new_from + utf16_len(&copy);

    // Verify: old facts shifted past the insert, plus exactly the copy's
    // facts (parsed alone) at the new span.
    let len = new_to - node_to;
    let mut expected: Vec<Sig> = sigs(&p)
        .into_iter()
        .map(|(s, e, k, d)| {
            let s = if s < node_to { s } else { s + len };
            let e = if e <= node_to { e } else { e + len };
            (s, e, k, d)
        })
        .collect();
    let alone = parse(&copy);
    let copy_ok = sound(&alone)
        && matches!(alone.tree.statements.as_slice(),
            [Ast::Node(n)] if n.range.start.offset == 0
                && n.range.end.offset as u64 == utf16_len(&copy));
    expected.extend(
        sigs(&alone)
            .into_iter()
            .map(|(s, e, k, d)| (s + new_from, e + new_from, k, d)),
    );
    let after = parse(&new_text);
    let proven = copy_ok && sound(&after) && same(expected, sigs(&after));
    if !proven {
        return refused(
            reason::VERIFY_FAILED,
            "The copy could not be proven in the resulting document; nothing changed.",
        );
    }
    Plan::Ready {
        edit,
        new_text,
        node: Some((new_from, new_to)),
        renamed,
    }
}

#[cfg(test)]
mod tests;
