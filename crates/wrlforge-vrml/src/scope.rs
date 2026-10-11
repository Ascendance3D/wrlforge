// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-2A: read-only lexical scope graph for VRML97 node names.
//!
//! Built from ONE parse result and thrown away with it. It never changes the
//! source text, a source span or document identity, and nothing is cached
//! across revisions. Load and save never depend on it.
//!
//! The rules come from ISO/IEC 14772-1, checked against the WD1.5 research in
//! `src/vrml/scope-graph.js` (not translated line by line):
//!
//! - 4.6.2: a USE refers to a node DEF'd BEFORE it. With repeated names, the
//!   standard picks the closest preceding DEF.
//! - 4.8.4: a PROTO body is a DEF/USE scope separate from the scene and from
//!   any nested PROTO, in both directions. A body lookup has no parent scope.
//! - 4.10.2: nodes named by a ROUTE shall be defined before the ROUTE.
//! - 4.4.4: a self-reference is a cycle only in the transformation hierarchy;
//!   a Script's descendants are outside it.
//!
//! Three namespaces stay in three tables: DEF node names, PROTO/EXTERNPROTO
//! type names, and interface members. Built-in node types are schema facts,
//! never declarations here.
//!
//! TWO ANSWERS PER REFERENCE, NEVER MERGED:
//!
//! - `status` + `target`: the EDIT answer. `target` is set only for
//!   `Status::Resolved`, when exactly one DEF is provable. Repeated names give
//!   `Ambiguous`. Damaged source gives `Recovered`. No nearest-node guess, no
//!   ranking, no type narrowing.
//! - `language_target`: what 4.6.2 says the reference means (closest
//!   preceding DEF). Informational only. It never feeds edit identity.

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::{Code, Severity};
use crate::parser::ParseResult;
use crate::tokenizer::Range;

/// Stable reason codes. Documented in `docs/architecture/RUST_STAB_2A_SCOPE.md`.
pub mod reason {
    pub const OK: &str = "ok";
    pub const MISSING_NAME: &str = "missing-name";
    pub const DUPLICATE_DEF_IN_SCOPE: &str = "duplicate-def-in-scope";
    pub const USE_BEFORE_DEF: &str = "use-before-def";
    pub const ROUTE_NODE_NOT_DEFINED_BEFORE_ROUTE: &str = "route-node-not-defined-before-route";
    pub const DEF_NOT_DECLARED_IN_SCOPE: &str = "def-not-declared-in-scope";
    pub const DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY: &str = "def-not-visible-across-proto-boundary";
    pub const SELF_REFERENTIAL_USE: &str = "self-referential-use";
    pub const SELF_REFERENCE_OUTSIDE_TRANSFORMATION_HIERARCHY: &str =
        "self-reference-outside-transformation-hierarchy";
    pub const DOCUMENT_PARSE_INCOMPLETE: &str = "document-parse-incomplete";
    pub const DOCUMENT_STRUCTURE_UNBALANCED: &str = "document-structure-unbalanced";
    pub const SCOPE_RECOVERED: &str = "scope-recovered";
    pub const PROTO_SCOPE_NOT_PROVABLE: &str = "proto-scope-not-provable";
    pub const PROTO_BODY_NOT_PROVABLE: &str = "proto-body-not-provable";
    pub const REFERENCE_IN_PROTO_INTERFACE_DEFAULT: &str = "reference-in-proto-interface-default";
    pub const NAME_DECLARED_IN_PROTO_INTERFACE_DEFAULT: &str =
        "name-declared-in-proto-interface-default";
}

/// Detail codes: non-binding classification, never a status.
pub mod detail {
    /// A ROUTE inside an MFNode array (Cybertown/Blaxxun compatibility).
    pub const ROUTE_IN_MFNODE_ARRAY: &str = "route-in-mfnode-array";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Exactly one DEF is provable. The only status with an edit target.
    Resolved,
    /// Provably no visible DEF (a defect in the source).
    Unresolved,
    /// More than one DEF of the name is visible. The standard picks the
    /// closest preceding one; WRL Forge refuses to edit through it.
    Ambiguous,
    /// The reference breaks a standard rule (USE before DEF, cycle).
    Invalid,
    /// The parse is damaged where the answer depends on it. Withheld.
    Recovered,
    /// A valid construct whose scope this analysis does not decide.
    Unsupported,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Resolved => "resolved",
            Status::Unresolved => "unresolved",
            Status::Ambiguous => "ambiguous",
            Status::Invalid => "invalid",
            Status::Recovered => "recovered",
            Status::Unsupported => "unsupported",
        }
    }

    /// The three-way display class: "proven" | "unresolved" | "uncertain".
    pub fn class(self) -> &'static str {
        match self {
            Status::Resolved => "proven",
            Status::Unresolved | Status::Invalid => "unresolved",
            Status::Ambiguous | Status::Recovered | Status::Unsupported => "uncertain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Document,
    ProtoBody,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub kind: ScopeKind,
    /// The document range, or the whole PROTO statement range.
    pub range: Range,
    /// The PROTO name for a body scope.
    pub owner_name: Option<String>,
    /// `Some(reason)` when this scope cannot be proven.
    pub recovered: Option<&'static str>,
}

/// One `DEF name Type { ... }` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct DefDecl {
    pub name: String,
    /// Index into `ScopeGraph::scopes`; `usize::MAX` in `unscoped_defs`.
    pub scope: usize,
    pub name_range: Range,
    /// The whole node statement, `DEF` keyword included. Equals the Scene
    /// Tree item span of the node.
    pub node_range: Range,
    pub node_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Proto,
    ExternProto,
}

/// A PROTO or EXTERNPROTO type name. Built-in types never appear here.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub kind: TypeKind,
    pub name: Option<String>,
    /// `None` inside a PROTO interface default value.
    pub scope: Option<usize>,
    pub range: Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberOwner {
    /// Index into `ScopeGraph::types`.
    Type(usize),
    /// A Script node, by its statement range.
    Script(Range),
}

/// One interface declaration (`field SFFloat r`), owned by a PROTO,
/// EXTERNPROTO or Script interface. Its own namespace.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub owner: MemberOwner,
    pub access: String,
    pub field_type: Option<String>,
    pub name: Option<String>,
    pub name_range: Option<Range>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Use,
    RouteSource,
    RouteDestination,
}

impl RefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Use => "use",
            RefKind::RouteSource => "route-source",
            RefKind::RouteDestination => "route-destination",
        }
    }
}

/// One node-name reference and its answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub kind: RefKind,
    pub name: Option<String>,
    /// The name token. `None` when the parse has no name.
    pub name_range: Option<Range>,
    /// The USE statement, or the whole ROUTE statement.
    pub statement_range: Range,
    /// `None` for a reference inside a PROTO interface default value.
    pub scope: Option<usize>,
    pub status: Status,
    pub reason: &'static str,
    /// The EDIT target (index into `defs`). Set only for `Resolved`.
    pub target: Option<usize>,
    /// The ISO 4.6.2 meaning (closest preceding DEF). Informational only.
    pub language_target: Option<usize>,
    /// DEF declarations that support the answer (all duplicates, or the
    /// later declarations of a forward reference).
    pub candidates: Vec<usize>,
    pub detail: Option<&'static str>,
    inside_script: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeGraph {
    pub scopes: Vec<Scope>,
    pub defs: Vec<DefDecl>,
    pub types: Vec<TypeDecl>,
    pub members: Vec<Member>,
    pub references: Vec<Reference>,
    /// DEFs inside PROTO interface default values: their scope is not
    /// decided, so they are in no scope table.
    pub unscoped_defs: Vec<DefDecl>,
    /// A hard parse limit or unbalanced structure: no scope is provable.
    pub document_recovered: Option<&'static str>,
}

// --- construction --------------------------------------------------------

#[derive(Clone, Copy)]
struct Ctx {
    /// `None` inside a PROTO interface default value.
    scope: Option<usize>,
    inside_script: bool,
}

struct Builder {
    g: ScopeGraph,
}

fn contains(outer: Range, inner: Range) -> bool {
    outer.start.offset <= inner.start.offset && inner.end.offset <= outer.end.offset
}

impl Builder {
    fn add_scope(&mut self, kind: ScopeKind, range: Range, owner: Option<String>) -> usize {
        self.g.scopes.push(Scope {
            kind,
            range,
            owner_name: owner,
            recovered: None,
        });
        self.g.scopes.len() - 1
    }

    fn statements(&mut self, list: &[Ast], ctx: Ctx) {
        for s in list {
            self.statement(s, ctx, false);
        }
    }

    /// `in_array` marks the lenient ROUTE/PROTO-in-MFNode compatibility path.
    fn statement(&mut self, s: &Ast, ctx: Ctx, in_array: bool) {
        match s {
            Ast::Node(n) => self.node(n, ctx),
            Ast::Use {
                name,
                name_range,
                range,
            } => self.add_ref(RefKind::Use, name, *name_range, *range, ctx, None),
            Ast::Proto(p) => self.proto(p, ctx),
            Ast::ExternProto(e) => self.extern_proto(e, ctx),
            Ast::Route(r) => {
                let d = in_array.then_some(detail::ROUTE_IN_MFNODE_ARRAY);
                self.add_ref(
                    RefKind::RouteSource,
                    &r.from.node,
                    r.from.node_range,
                    r.range,
                    ctx,
                    d,
                );
                self.add_ref(
                    RefKind::RouteDestination,
                    &r.to.node,
                    r.to.node_range,
                    r.range,
                    ctx,
                    d,
                );
            }
            _ => {}
        }
    }

    fn value(&mut self, v: &Ast, ctx: Ctx) {
        match v {
            Ast::Array { items, .. } => {
                for i in items {
                    self.value(i, ctx);
                }
            }
            Ast::Node(_)
            | Ast::Use { .. }
            | Ast::Proto(_)
            | Ast::ExternProto(_)
            | Ast::Route(_) => self.statement(v, ctx, true),
            _ => {}
        }
    }

    fn add_ref(
        &mut self,
        kind: RefKind,
        name: &Option<String>,
        name_range: Option<Range>,
        statement_range: Range,
        ctx: Ctx,
        detail: Option<&'static str>,
    ) {
        self.g.references.push(Reference {
            kind,
            name: name.clone(),
            name_range,
            statement_range,
            scope: ctx.scope,
            status: Status::Unsupported,
            reason: reason::REFERENCE_IN_PROTO_INTERFACE_DEFAULT,
            target: None,
            language_target: None,
            candidates: vec![],
            detail,
            inside_script: ctx.inside_script,
        });
    }

    fn members(&mut self, owner: MemberOwner, list: &[Interface]) {
        for i in list {
            self.g.members.push(Member {
                owner,
                access: i.access.clone(),
                field_type: i.field_type.clone(),
                name: i.name.clone(),
                name_range: i.name_range,
            });
        }
    }

    fn extern_proto(&mut self, e: &ExternProto, ctx: Ctx) {
        let ti = self.g.types.len();
        self.g.types.push(TypeDecl {
            kind: TypeKind::ExternProto,
            name: e.name.clone(),
            scope: ctx.scope,
            range: e.range,
        });
        self.members(MemberOwner::Type(ti), &e.interfaces);
    }

    fn proto(&mut self, p: &Proto, ctx: Ctx) {
        let ti = self.g.types.len();
        self.g.types.push(TypeDecl {
            kind: TypeKind::Proto,
            name: p.name.clone(),
            scope: ctx.scope,
            range: p.range,
        });
        self.members(MemberOwner::Type(ti), &p.interfaces);
        // Interface defaults: which DEF/USE scope owns them is not settled by
        // the standard text, so they are visited with NO scope.
        let none = Ctx {
            scope: None,
            inside_script: ctx.inside_script,
        };
        for i in &p.interfaces {
            if let Some(d) = &i.default {
                self.value(d, none);
            }
        }
        let body = self.add_scope(ScopeKind::ProtoBody, p.range, p.name.clone());
        if p.name.is_none() {
            self.g.scopes[body].recovered = Some(reason::PROTO_SCOPE_NOT_PROVABLE);
        } else if !p
            .body
            .iter()
            .any(|b| matches!(b, Ast::Node(_) | Ast::Use { .. }))
        {
            // Annex A: a PROTO body holds at least one node statement. One
            // without is truncated or invalid; its scope is not trusted.
            self.g.scopes[body].recovered = Some(reason::PROTO_BODY_NOT_PROVABLE);
        }
        self.statements(
            &p.body,
            Ctx {
                scope: Some(body),
                inside_script: ctx.inside_script,
            },
        );
    }

    fn node(&mut self, n: &Node, ctx: Ctx) {
        if let (Some(name), Some(name_range)) = (&n.def, n.def_range) {
            let d = DefDecl {
                name: name.clone(),
                scope: ctx.scope.unwrap_or(usize::MAX),
                name_range,
                node_range: n.range,
                node_type: n.node_type.clone(),
            };
            match ctx.scope {
                Some(_) => self.g.defs.push(d),
                None => self.g.unscoped_defs.push(d),
            }
        }
        let inner = Ctx {
            scope: ctx.scope,
            inside_script: ctx.inside_script || n.node_type == "Script",
        };
        if n.node_type == "Script" {
            self.members(MemberOwner::Script(n.range), &n.interfaces);
        }
        // A node's own interface defaults belong to the node's scope.
        for i in &n.interfaces {
            if let Some(d) = &i.default {
                self.value(d, inner);
            }
        }
        // ROUTE/PROTO/EXTERNPROTO body statements live in `fields` too.
        for f in &n.fields {
            match f {
                Ast::Field(field) => {
                    if !field.is_binding {
                        if let Some(v) = &field.value {
                            self.value(v, inner);
                        }
                    }
                }
                other => self.statement(other, inner, false),
            }
        }
    }

    // --- recovery --------------------------------------------------------

    fn mark_recovery(&mut self, parse: &ParseResult) {
        if parse.truncated || parse.depth_capped {
            self.g.document_recovered = Some(reason::DOCUMENT_PARSE_INCOMPLETE);
        }
        for d in &parse.diagnostics {
            if d.severity != Severity::Error {
                continue;
            }
            match d.code {
                // A missing header changes no statement structure.
                Code::MissingHeader => continue,
                // These move every scope boundary after them: an unclosed
                // brace absorbs the rest of the file.
                Code::UnclosedBrace
                | Code::UnclosedBracket
                | Code::UnterminatedString
                | Code::MaxDepth
                | Code::MaxNodes => {
                    self.g
                        .document_recovered
                        .get_or_insert(reason::DOCUMENT_STRUCTURE_UNBALANCED);
                    continue;
                }
                _ => {}
            }
            // Innermost containing scope only; ranges nest strictly.
            let best = self
                .g
                .scopes
                .iter()
                .enumerate()
                .filter(|(_, s)| contains(s.range, d.range))
                .min_by_key(|(_, s)| s.range.end.offset - s.range.start.offset)
                .map(|(i, _)| i);
            match best {
                Some(i) => {
                    self.g.scopes[i]
                        .recovered
                        .get_or_insert(reason::SCOPE_RECOVERED);
                }
                // Damage no scope contains cannot be placed: fail closed.
                None => {
                    self.g
                        .document_recovered
                        .get_or_insert(reason::SCOPE_RECOVERED);
                }
            }
        }
        if let Some(r) = self.g.document_recovered {
            for s in &mut self.g.scopes {
                s.recovered = Some(r);
            }
        }
    }
}

// --- resolution ----------------------------------------------------------

struct Index {
    /// (scope, name) -> DEF indices in source order.
    by_scope: HashMap<(usize, String), Vec<usize>>,
    /// name -> scopes declaring it.
    scopes_of: HashMap<String, Vec<usize>>,
    unscoped_names: HashMap<String, ()>,
}

fn resolve(g: &ScopeGraph, idx: &Index, r: &Reference) -> Resolution {
    let res = |status, reason| Resolution {
        status,
        reason,
        target: None,
        language_target: None,
        candidates: vec![],
    };
    let Some(name) = &r.name else {
        return res(Status::Invalid, reason::MISSING_NAME);
    };
    let Some(scope) = r.scope else {
        return res(
            Status::Unsupported,
            reason::REFERENCE_IN_PROTO_INTERFACE_DEFAULT,
        );
    };
    // The proof gate, asked once and first: a damaged scope withholds every
    // answer, positive, negative and ambiguous alike.
    if let Some(why) = g.scopes[scope].recovered {
        return res(Status::Recovered, why);
    }
    // 4.6.2 "preceding": a USE sees DEFs whose name starts before it.
    // 4.10.2: a ROUTE sees nodes defined before the ROUTE statement.
    let before = match r.kind {
        RefKind::Use => r.name_range.map(|n| n.start.offset).unwrap_or(0),
        _ => r.statement_range.start.offset,
    };
    let all = idx
        .by_scope
        .get(&(scope, name.clone()))
        .cloned()
        .unwrap_or_default();
    let visible: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&d| g.defs[d].name_range.start.offset < before)
        .collect();
    // 4.6.2 closest preceding: the last visible one in source order.
    let language = visible.last().copied();

    if visible.len() > 1 {
        // Decided on the NAME alone, before any type is considered.
        return Resolution {
            status: Status::Ambiguous,
            reason: reason::DUPLICATE_DEF_IN_SCOPE,
            target: None,
            language_target: language,
            candidates: visible,
        };
    }
    if let Some(d) = language {
        let def = &g.defs[d];
        if r.kind == RefKind::Use && contains(def.node_range, r.statement_range) {
            // 4.4.4: a cycle only inside the transformation hierarchy.
            if r.inside_script {
                return Resolution {
                    status: Status::Resolved,
                    reason: reason::SELF_REFERENCE_OUTSIDE_TRANSFORMATION_HIERARCHY,
                    target: Some(d),
                    language_target: Some(d),
                    candidates: vec![d],
                };
            }
            return Resolution {
                status: Status::Invalid,
                reason: reason::SELF_REFERENTIAL_USE,
                target: None,
                language_target: Some(d),
                candidates: vec![d],
            };
        }
        return Resolution {
            status: Status::Resolved,
            reason: reason::OK,
            target: Some(d),
            language_target: Some(d),
            candidates: vec![d],
        };
    }
    if !all.is_empty() {
        let (status, why) = match r.kind {
            RefKind::Use => (Status::Invalid, reason::USE_BEFORE_DEF),
            _ => (
                Status::Unresolved,
                reason::ROUTE_NODE_NOT_DEFINED_BEFORE_ROUTE,
            ),
        };
        return Resolution {
            status,
            reason: why,
            target: None,
            language_target: None,
            candidates: all,
        };
    }
    // A name declared where this analysis decides no scope: not "missing".
    if idx.unscoped_names.contains_key(name) {
        return res(
            Status::Unsupported,
            reason::NAME_DECLARED_IN_PROTO_INTERFACE_DEFAULT,
        );
    }
    // 4.8.4, both directions: declared, but behind a PROTO boundary.
    let elsewhere = idx
        .scopes_of
        .get(name)
        .is_some_and(|s| s.iter().any(|&s| s != scope));
    if elsewhere {
        return res(
            Status::Unresolved,
            reason::DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY,
        );
    }
    res(Status::Unresolved, reason::DEF_NOT_DECLARED_IN_SCOPE)
}

struct Resolution {
    status: Status,
    reason: &'static str,
    target: Option<usize>,
    language_target: Option<usize>,
    candidates: Vec<usize>,
}

/// Build the scope graph for one parse. Never panics; malformed input gives
/// `Recovered` or `Unsupported` answers instead.
pub fn build(parse: &ParseResult) -> ScopeGraph {
    let mut b = Builder {
        g: ScopeGraph {
            scopes: vec![],
            defs: vec![],
            types: vec![],
            members: vec![],
            references: vec![],
            unscoped_defs: vec![],
            document_recovered: None,
        },
    };
    let doc = b.add_scope(ScopeKind::Document, parse.tree.range, None);
    b.statements(
        &parse.tree.statements,
        Ctx {
            scope: Some(doc),
            inside_script: false,
        },
    );
    b.mark_recovery(parse);
    let mut g = b.g;

    let mut idx = Index {
        by_scope: HashMap::new(),
        scopes_of: HashMap::new(),
        unscoped_names: HashMap::new(),
    };
    for (i, d) in g.defs.iter().enumerate() {
        idx.by_scope
            .entry((d.scope, d.name.clone()))
            .or_default()
            .push(i);
        let s = idx.scopes_of.entry(d.name.clone()).or_default();
        if !s.contains(&d.scope) {
            s.push(d.scope);
        }
    }
    for d in &g.unscoped_defs {
        idx.unscoped_names.insert(d.name.clone(), ());
    }
    // DEFs are pushed in walk order, which is source order; keep it explicit.
    for v in idx.by_scope.values_mut() {
        v.sort_by_key(|&i| g.defs[i].name_range.start.offset);
    }
    let answers: Vec<Resolution> = g.references.iter().map(|r| resolve(&g, &idx, r)).collect();
    for (r, a) in g.references.iter_mut().zip(answers) {
        r.status = a.status;
        r.reason = a.reason;
        r.target = a.target;
        r.language_target = a.language_target;
        r.candidates = a.candidates;
    }
    g
}

impl ScopeGraph {
    /// The DEF a reference may be EDITED through: only a `Resolved` answer.
    pub fn edit_target(&self, r: &Reference) -> Option<&DefDecl> {
        match (r.status, r.target) {
            (Status::Resolved, Some(d)) => self.defs.get(d),
            _ => None,
        }
    }

    /// The ISO 4.6.2 meaning. NEVER use this for an edit or a selection.
    pub fn language_target(&self, r: &Reference) -> Option<&DefDecl> {
        r.language_target.and_then(|d| self.defs.get(d))
    }

    /// The USE reference whose statement is exactly `range`.
    pub fn use_at(&self, range: Range) -> Option<&Reference> {
        self.references
            .iter()
            .find(|r| r.kind == RefKind::Use && r.statement_range == range)
    }

    /// The (source, destination) references of the ROUTE statement `range`.
    pub fn route_at(&self, range: Range) -> Option<(&Reference, &Reference)> {
        let src = self
            .references
            .iter()
            .find(|r| r.kind == RefKind::RouteSource && r.statement_range == range)?;
        let dst = self
            .references
            .iter()
            .find(|r| r.kind == RefKind::RouteDestination && r.statement_range == range)?;
        Some((src, dst))
    }

    /// Members declared by one owner, in source order.
    pub fn members_of(&self, owner: MemberOwner) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(move |m| m.owner == owner)
    }

    /// Advisory diagnostics. Only high-confidence findings; an uncertain
    /// answer (`Recovered`, `Unsupported`) never produces one.
    pub fn diagnostics(&self) -> Vec<ScopeDiagnostic> {
        let mut out = Vec::new();
        for r in &self.references {
            let Some(range) = r.name_range else { continue };
            let name = r.name.as_deref().unwrap_or("?");
            let what = match r.kind {
                RefKind::Use => "USE",
                RefKind::RouteSource => "ROUTE source",
                RefKind::RouteDestination => "ROUTE destination",
            };
            let (code, severity, message) = match (r.status, r.reason) {
                (Status::Unresolved, reason::DEF_NOT_DECLARED_IN_SCOPE) => (
                    code::NOT_DECLARED,
                    "warning",
                    format!("{what} '{name}' names no DEF in this scope"),
                ),
                (Status::Unresolved, reason::DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY) => (
                    code::ACROSS_PROTO,
                    "warning",
                    format!(
                        "{what} '{name}' names a DEF on the other side of a PROTO boundary (ISO 4.8.4)"
                    ),
                ),
                (Status::Invalid, reason::USE_BEFORE_DEF) => (
                    code::BEFORE_DEF,
                    "warning",
                    format!("USE '{name}' comes before its DEF (ISO 4.6.2)"),
                ),
                (Status::Unresolved, reason::ROUTE_NODE_NOT_DEFINED_BEFORE_ROUTE) => (
                    code::BEFORE_DEF,
                    "warning",
                    format!("{what} '{name}' is defined after the ROUTE (ISO 4.10.2)"),
                ),
                (Status::Invalid, reason::SELF_REFERENTIAL_USE) => (
                    code::SELF_REFERENCE,
                    "warning",
                    format!("USE '{name}' is inside the node it names (a cycle, ISO 4.4.4)"),
                ),
                (Status::Ambiguous, reason::DUPLICATE_DEF_IN_SCOPE) => (
                    code::AMBIGUOUS,
                    "info",
                    format!(
                        "{what} '{name}' has {} DEFs before it; VRML97 uses the closest one, WRL Forge will not edit through it",
                        r.candidates.len()
                    ),
                ),
                _ => continue,
            };
            out.push(ScopeDiagnostic {
                code,
                severity,
                reason: r.reason,
                message,
                range,
            });
        }
        out.sort_by_key(|d| (d.range.start.offset, d.range.end.offset, d.code));
        out
    }
}

/// Stable advisory codes. Never errors; never block Save.
pub mod code {
    pub const NOT_DECLARED: &str = "SCOPE001";
    pub const ACROSS_PROTO: &str = "SCOPE002";
    pub const BEFORE_DEF: &str = "SCOPE003";
    pub const SELF_REFERENCE: &str = "SCOPE004";
    pub const AMBIGUOUS: &str = "SCOPE010";
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeDiagnostic {
    pub code: &'static str,
    /// "warning" | "info". Scope analysis never reports an error.
    pub severity: &'static str,
    pub reason: &'static str,
    pub message: String,
    /// The reference name token.
    pub range: Range,
}

#[cfg(test)]
mod tests;
