// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-2B: typed reference analysis over the RUST-STAB-2A scope graph.
//!
//! Read-only, like the graph. It answers four questions from ONE parse:
//!
//! 1. Which declaration does a node instance's type name bind (4.8.1, 4.8.4)?
//! 2. Which interface member does a name bind (4.7, 4.8.2, 4.9.2)?
//! 3. Is a ROUTE a legal eventOut -> eventIn connection of one type (4.10.2)?
//! 4. Is an `IS` connection legal under 4.8.3 and Table 4.4?
//!
//! The same gate as the graph governs every answer: an answer may be lost,
//! ambiguous or withheld, but it never names a different declaration. An
//! unproven node, type or interface never produces a confident event or type
//! verdict. No nearest name, no ranking, no implicit type conversion, and no
//! EXTERNPROTO URL or Script source is ever read.
//!
//! Language meaning stays apart from edit identity: nothing here feeds a
//! selection, a gizmo or a source edit.

use std::collections::HashMap;

use super::*;
use crate::ast::{Ast, Field, Interface, Node};
use crate::node_schema::{self, NodeSchema, Profile};

/// ISO/IEC 14772-1 Annex A.2 `fieldType`: the 20 VRML97 field type tokens.
/// Types compare by exact token equality against this list (4.8.3, 4.10.2).
const FIELD_TYPES: [&str; 20] = [
    "MFColor",
    "MFFloat",
    "MFInt32",
    "MFNode",
    "MFRotation",
    "MFString",
    "MFTime",
    "MFVec2f",
    "MFVec3f",
    "SFBool",
    "SFColor",
    "SFFloat",
    "SFImage",
    "SFInt32",
    "SFNode",
    "SFRotation",
    "SFString",
    "SFTime",
    "SFVec2f",
    "SFVec3f",
];

/// A VRML97 interface access kind (Annex A `restrictedInterfaceDeclaration`
/// and `interfaceDeclaration`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Field,
    ExposedField,
    EventIn,
    EventOut,
}

impl Access {
    pub fn parse(s: &str) -> Option<Access> {
        match s {
            "field" => Some(Access::Field),
            "exposedField" => Some(Access::ExposedField),
            "eventIn" => Some(Access::EventIn),
            "eventOut" => Some(Access::EventOut),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Access::Field => "field",
            Access::ExposedField => "exposedField",
            Access::EventIn => "eventIn",
            Access::EventOut => "eventOut",
        }
    }

    /// 4.10.2: can this endpoint send events (eventOut, or the eventOut
    /// part of an exposedField)?
    fn can_send(self) -> bool {
        matches!(self, Access::EventOut | Access::ExposedField)
    }

    /// 4.10.2: can this endpoint receive events?
    fn can_receive(self) -> bool {
        matches!(self, Access::EventIn | Access::ExposedField)
    }
}

/// ISO Table 4.4. Rows: the prototype DEFINITION (the node field written
/// left of `IS`). Columns: the prototype DECLARATION (the interface member).
/// Seven legal cells of sixteen.
fn table_4_4(definition: Access, declaration: Access) -> bool {
    match definition {
        Access::ExposedField => true,
        other => other == declaration,
    }
}

/// Where an interface entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The committed VRML97 node schema (ISO clause 6).
    BuiltinSchema,
    /// A PROTO interface (index into `ScopeGraph::types`).
    ProtoInterface(usize),
    /// An EXTERNPROTO interface as declared in THIS document.
    ExternProtoInterface(usize),
    /// A built-in Script node's own interface, by its statement range.
    ScriptInterface(Range),
}

// --- type resolution ---------------------------------------------------------

/// The type answer for one node instance.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeRef {
    pub type_name: String,
    pub type_range: Range,
    /// The whole node statement (`DEF` included). Equals `DefDecl::node_range`.
    pub node_range: Range,
    /// `None` inside a PROTO interface default value.
    pub scope: Option<usize>,
    /// The innermost enclosing node instance.
    pub parent: Option<usize>,
    pub status: Status,
    pub reason: &'static str,
    /// The PROTO/EXTERNPROTO declaration (index into `types`). Set ONLY for
    /// a `Resolved` user-defined type.
    pub target: Option<usize>,
    /// True ONLY for a `Resolved` built-in node type.
    pub builtin: bool,
    /// Declarations that support the answer (duplicates, a later one).
    pub candidates: Vec<usize>,
    pub detail: Option<&'static str>,
}

impl TypeRef {
    pub(super) fn site(n: &Node, scope: Option<usize>, parent: Option<usize>) -> TypeRef {
        TypeRef {
            type_name: n.node_type.clone(),
            type_range: n.type_range,
            node_range: n.range,
            scope,
            parent,
            status: Status::Unsupported,
            reason: reason::TYPE_IN_PROTO_INTERFACE_DEFAULT,
            target: None,
            builtin: false,
            candidates: vec![],
            detail: None,
        }
    }

    /// Is this instance PROVEN to be the built-in node `name`? A PROTO that
    /// takes a built-in name is not.
    pub fn is_builtin(&self, name: &str) -> bool {
        self.status == Status::Resolved && self.builtin && self.type_name == name
    }
}

/// Scope index -> the PROTO declaration that opens it as a body.
fn body_owners(g: &ScopeGraph) -> Vec<Option<usize>> {
    let mut out = vec![None; g.scopes.len()];
    for (i, t) in g.types.iter().enumerate() {
        if let Some(b) = t.body_scope {
            out[b] = Some(i);
        }
    }
    out
}

/// The type-scope chain from `scope` outward (4.8.4: a nested PROTO body sees
/// the declarations of the scopes that enclose it). Every link must be
/// provable: a damaged link could hold a same-name declaration.
fn type_chain(
    g: &ScopeGraph,
    owners: &[Option<usize>],
    scope: usize,
) -> Result<Vec<usize>, (Status, &'static str)> {
    let mut out = Vec::new();
    let mut at = scope;
    loop {
        if out.len() > g.scopes.len() {
            return Err((Status::Recovered, reason::TYPE_SCOPE_CHAIN_NOT_PROVABLE));
        }
        if let Some(why) = g.scopes[at].recovered {
            return Err((Status::Recovered, why));
        }
        out.push(at);
        match g.scopes[at].kind {
            ScopeKind::Document => return Ok(out),
            ScopeKind::ProtoBody => match owners[at].and_then(|t| g.types[t].scope) {
                Some(p) => at = p,
                // A PROTO written inside an interface default value.
                None => return Err((Status::Unsupported, reason::TYPE_IN_PROTO_INTERFACE_DEFAULT)),
            },
        }
    }
}

struct TypeAnswer {
    status: Status,
    reason: &'static str,
    target: Option<usize>,
    builtin: bool,
    candidates: Vec<usize>,
    detail: Option<&'static str>,
}

fn answer(status: Status, reason: &'static str) -> TypeAnswer {
    TypeAnswer {
        status,
        reason,
        target: None,
        builtin: false,
        candidates: vec![],
        detail: None,
    }
}

fn resolve_type(
    g: &ScopeGraph,
    owners: &[Option<usize>],
    by_name: &HashMap<&str, Vec<usize>>,
    t: &TypeRef,
) -> TypeAnswer {
    let name = t.type_name.as_str();
    if name.is_empty() {
        return answer(Status::Invalid, reason::MISSING_NAME);
    }
    let Some(scope) = t.scope else {
        return answer(Status::Unsupported, reason::TYPE_IN_PROTO_INTERFACE_DEFAULT);
    };
    // The proof gate, first: a damaged chain withholds every answer, the
    // built-in answer included (it claims no declaration is in scope).
    if let Some(why) = g.document_recovered {
        return answer(Status::Recovered, why);
    }
    let chain = match type_chain(g, owners, scope) {
        Ok(c) => c,
        Err((s, why)) => return answer(s, why),
    };
    let decls: Vec<usize> = by_name
        .get(name)
        .map(|v| {
            v.iter()
                .copied()
                .filter(|&d| g.types[d].scope.is_some_and(|s| chain.contains(&s)))
                .collect()
        })
        .unwrap_or_default();
    // 4.8.4: a prototype may not be instantiated inside its own
    // implementation. Checked before order: such an instance breaks both.
    if let Some(&d) = decls
        .iter()
        .find(|&&d| g.types[d].kind == TypeKind::Proto && contains(g.types[d].range, t.node_range))
    {
        return TypeAnswer {
            candidates: vec![d],
            ..answer(Status::Invalid, reason::RECURSIVE_PROTO_INSTANCE)
        };
    }
    // 4.8.1: type names shall be unique. Decided on the NAME alone over the
    // whole chain, before position or kind.
    if decls.len() > 1 {
        return TypeAnswer {
            candidates: decls,
            ..answer(Status::Ambiguous, reason::DUPLICATE_PROTO_DECLARATION)
        };
    }
    if let Some(&d) = decls.first() {
        // 4.8.4: instantiable anywhere AFTER the definition is complete.
        if g.types[d].range.end.offset <= t.node_range.start.offset {
            return TypeAnswer {
                status: Status::Resolved,
                reason: reason::OK,
                target: Some(d),
                builtin: false,
                candidates: vec![d],
                detail: node_schema::is_vrml97_node(name).then_some(detail::PROTO_SHADOWS_BUILTIN),
            };
        }
        // A built-in spelling used before a later declaration is NOT
        // silently the built-in: the file's own declaration exists.
        return TypeAnswer {
            candidates: vec![d],
            ..answer(Status::Invalid, reason::PROTO_INSTANCE_BEFORE_DECLARATION)
        };
    }
    if node_schema::is_vrml97_node(name) {
        return TypeAnswer {
            builtin: true,
            ..answer(Status::Resolved, reason::NODE_TYPE_IS_BUILTIN)
        };
    }
    answer(Status::Unresolved, reason::NODE_TYPE_UNKNOWN)
}

/// Resolve every node instance's type. Runs before node-name resolution.
pub(super) fn resolve_types(g: &mut ScopeGraph) {
    let owners = body_owners(g);
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, t) in g.types.iter().enumerate() {
        if let Some(n) = &t.name {
            by_name.entry(n.as_str()).or_default().push(i);
        }
    }
    let answers: Vec<TypeAnswer> = g
        .instances
        .iter()
        .map(|t| resolve_type(g, &owners, &by_name, t))
        .collect();
    for (t, a) in g.instances.iter_mut().zip(answers) {
        t.status = a.status;
        t.reason = a.reason;
        t.target = a.target;
        t.builtin = a.builtin;
        t.candidates = a.candidates;
        t.detail = a.detail;
    }
}

// --- interfaces ----------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Iface {
    Builtin(&'static NodeSchema),
    /// The built-in Script: schema fields plus its own declarations.
    Script(&'static NodeSchema, Range, Option<usize>),
    /// A PROTO or EXTERNPROTO declaration.
    Declared(usize),
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    access: Access,
    field_type: String,
    /// A 4.7 `set_zzz` / `zzz_changed` name of an exposedField.
    alias: bool,
    origin: Origin,
}

#[derive(Debug, Clone)]
struct Entries {
    list: Vec<Entry>,
    /// Absence proves nothing (4.9.2: the declaration may be a subset).
    externproto: bool,
}

impl Entries {
    fn find(&self, name: &str) -> Vec<&Entry> {
        self.list.iter().filter(|e| e.name == name).collect()
    }
}

type Withheld = (Status, &'static str);

/// 4.7: an exposedField `zzz` also occupies `set_zzz` (eventIn) and
/// `zzz_changed` (eventOut). Two entries with one name have no winner.
fn push_expanded(out: &mut Vec<Entry>, name: &str, access: Access, ty: &str, origin: Origin) {
    out.push(Entry {
        name: name.to_string(),
        access,
        field_type: ty.to_string(),
        alias: false,
        origin,
    });
    if access == Access::ExposedField {
        for (n, a) in [
            (format!("set_{name}"), Access::EventIn),
            (format!("{name}_changed"), Access::EventOut),
        ] {
            out.push(Entry {
                name: n,
                access: a,
                field_type: ty.to_string(),
                alias: true,
                origin,
            });
        }
    }
}

fn push_declared(
    g: &ScopeGraph,
    owner: MemberOwner,
    origin: Origin,
    out: &mut Vec<Entry>,
) -> Result<(), Withheld> {
    for m in g.members_of(owner) {
        let (Some(name), Some(ty), Some(a)) = (&m.name, &m.field_type, Access::parse(&m.access))
        else {
            return Err((Status::Recovered, reason::INTERFACE_NOT_PROVABLE));
        };
        push_expanded(out, name, a, ty, origin);
    }
    Ok(())
}

fn iface_of(g: &ScopeGraph, inst: usize) -> Result<Iface, Withheld> {
    let t = &g.instances[inst];
    match t.status {
        Status::Resolved => {}
        Status::Recovered => return Err((Status::Recovered, t.reason)),
        Status::Ambiguous => return Err((Status::Ambiguous, reason::NODE_TYPE_NOT_PROVEN)),
        _ => return Err((Status::Unsupported, reason::NODE_TYPE_NOT_PROVEN)),
    }
    if t.builtin {
        let s = node_schema::get_node_schema(&t.type_name)
            .ok_or((Status::Unsupported, reason::NODE_TYPE_NOT_PROVEN))?;
        return Ok(if t.type_name == "Script" {
            Iface::Script(s, t.node_range, t.scope)
        } else {
            Iface::Builtin(s)
        });
    }
    t.target
        .map(Iface::Declared)
        .ok_or((Status::Unsupported, reason::NODE_TYPE_NOT_PROVEN))
}

fn entries_of(g: &ScopeGraph, iface: Iface) -> Result<Entries, Withheld> {
    if let Some(why) = g.document_recovered {
        return Err((Status::Recovered, why));
    }
    let mut list = Vec::new();
    if let Iface::Builtin(s) | Iface::Script(s, ..) = iface {
        // VRML97 fields only: an X3D-only field is not a VRML97 event.
        for f in s.fields.iter().filter(|f| f.in_profile(Profile::Vrml97)) {
            if let Some(a) = f.vrml97_declaration.and_then(Access::parse) {
                push_expanded(&mut list, f.name, a, f.field_type, Origin::BuiltinSchema);
            }
        }
    }
    let mut externproto = false;
    match iface {
        Iface::Builtin(_) => {}
        Iface::Script(_, range, scope) => {
            if let Some(why) = scope.and_then(|s| g.scopes[s].recovered) {
                return Err((Status::Recovered, why));
            }
            push_declared(
                g,
                MemberOwner::Script(range),
                Origin::ScriptInterface(range),
                &mut list,
            )?;
        }
        Iface::Declared(ti) => {
            let t = &g.types[ti];
            for s in [t.scope, t.body_scope].into_iter().flatten() {
                if let Some(why) = g.scopes[s].recovered {
                    return Err((Status::Recovered, why));
                }
            }
            let origin = match t.kind {
                TypeKind::Proto => Origin::ProtoInterface(ti),
                TypeKind::ExternProto => {
                    externproto = true;
                    Origin::ExternProtoInterface(ti)
                }
            };
            push_declared(g, MemberOwner::Type(ti), origin, &mut list)?;
        }
    }
    Ok(Entries { list, externproto })
}

/// Per-build lookups: interface entries are computed once per instance.
struct Cache {
    instance_at: HashMap<(u32, u32), usize>,
    entries: HashMap<usize, Result<Entries, Withheld>>,
}

impl Cache {
    fn new(g: &ScopeGraph) -> Cache {
        let mut instance_at = HashMap::new();
        for (i, t) in g.instances.iter().enumerate() {
            instance_at.insert((t.node_range.start.offset, t.node_range.end.offset), i);
        }
        Cache {
            instance_at,
            entries: HashMap::new(),
        }
    }

    fn instance_of(&self, node_range: Range) -> Option<usize> {
        self.instance_at
            .get(&(node_range.start.offset, node_range.end.offset))
            .copied()
    }

    fn entries(&mut self, g: &ScopeGraph, inst: usize) -> Result<Entries, Withheld> {
        self.entries
            .entry(inst)
            .or_insert_with(|| iface_of(g, inst).and_then(|i| entries_of(g, i)))
            .clone()
    }
}

// --- ROUTE ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RouteSite {
    pub(super) range: Range,
    /// Indices into `ScopeGraph::references`.
    pub(super) source: usize,
    pub(super) destination: usize,
    pub(super) from_event: Option<String>,
    pub(super) from_event_range: Option<Range>,
    pub(super) to_event: Option<String>,
    pub(super) to_event_range: Option<Range>,
}

/// One ROUTE endpoint event answer. Asked only after its node is proven.
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnd {
    /// The event name as written.
    pub name: Option<String>,
    pub name_range: Option<Range>,
    pub status: Status,
    pub reason: &'static str,
    /// The interface entry it bound (differs from `name` after the 4.10.2
    /// shorthand). Only for `Resolved`.
    pub bound_name: Option<String>,
    pub access: Option<Access>,
    pub field_type: Option<String>,
    pub origin: Option<Origin>,
    pub detail: Option<&'static str>,
}

/// The typed answer for one ROUTE statement: six separate questions (two
/// nodes, two events, two directions) and one type verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteCheck {
    pub range: Range,
    /// Indices into `ScopeGraph::references` (the 2A node answers).
    pub source_node: usize,
    pub destination_node: usize,
    pub source: EventEnd,
    pub destination: EventEnd,
    /// The whole-ROUTE verdict. `Resolved` only when both nodes, both
    /// events, both directions and the exact type are proven.
    pub status: Status,
    pub reason: &'static str,
}

fn event_end(
    g: &ScopeGraph,
    cache: &mut Cache,
    node: &Reference,
    event: &Option<String>,
    event_range: Option<Range>,
    source: bool,
) -> EventEnd {
    let e = EventEnd {
        name: event.clone(),
        name_range: event_range,
        status: Status::Unsupported,
        reason: reason::ROUTE_NODE_NOT_PROVEN,
        bound_name: None,
        access: None,
        field_type: None,
        origin: None,
        detail: None,
    };
    let withhold = |mut e: EventEnd, (s, r): Withheld| {
        e.status = s;
        e.reason = r;
        e
    };
    // An uncertain node never yields a confident event answer, and a lost
    // node never comes back as a missing event.
    match node.status {
        Status::Resolved => {}
        Status::Recovered => return withhold(e, (Status::Recovered, node.reason)),
        Status::Ambiguous => {
            return withhold(e, (Status::Ambiguous, reason::ROUTE_NODE_NOT_PROVEN))
        }
        _ => return e,
    }
    let Some(inst) = g
        .edit_target(node)
        .and_then(|d| cache.instance_of(d.node_range))
    else {
        return withhold(e, (Status::Unsupported, reason::NODE_TYPE_NOT_PROVEN));
    };
    let entries = match cache.entries(g, inst) {
        Ok(x) => x,
        Err(w) => return withhold(e, w),
    };
    let Some(name) = event else {
        return withhold(e, (Status::Invalid, reason::MISSING_NAME));
    };
    let capable = |a: Access| {
        if source {
            a.can_send()
        } else {
            a.can_receive()
        }
    };
    let bind = |mut e: EventEnd, x: &Entry, d: Option<&'static str>| {
        e.status = Status::Resolved;
        e.reason = reason::OK;
        e.bound_name = Some(x.name.clone());
        e.access = Some(x.access);
        e.field_type = Some(x.field_type.clone());
        e.origin = Some(x.origin);
        e.detail = d.or(x.alias.then_some(detail::EVENT_VIA_EXPOSEDFIELD_ALIAS));
        e
    };
    let ambiguous = (Status::Ambiguous, reason::DUPLICATE_INTERFACE_MEMBER);
    let unknowable = (
        Status::Unsupported,
        reason::EXTERNPROTO_INTERFACE_NOT_LOCALLY_VERIFIABLE,
    );
    // 4.10.2: the written name first.
    let first = entries.find(name);
    match first.as_slice() {
        [_, _, ..] => return withhold(e, ambiguous),
        [x] if capable(x.access) => return bind(e, x, None),
        [] if entries.externproto => return withhold(e, unknowable),
        _ => {}
    }
    // The fallback fires only when the required event is PROVABLY absent:
    // the interface is fully known and lacks it, or holds the name with
    // another access.
    let alt = if source {
        format!("{name}_changed")
    } else {
        format!("set_{name}")
    };
    let second = entries.find(&alt);
    match second.as_slice() {
        [_, _, ..] => return withhold(e, ambiguous),
        [x] if capable(x.access) => return bind(e, x, Some(detail::EVENT_VIA_SHORTHAND)),
        [] if entries.externproto => return withhold(e, unknowable),
        _ => {}
    }
    if first.is_empty() && second.is_empty() {
        return withhold(e, (Status::Unresolved, reason::ROUTE_EVENT_UNKNOWN));
    }
    let why = if source {
        reason::ROUTE_SOURCE_NOT_AN_EVENT_OUT
    } else {
        reason::ROUTE_DESTINATION_NOT_AN_EVENT_IN
    };
    withhold(e, (Status::Invalid, why))
}

fn is_defect(s: Status) -> bool {
    matches!(s, Status::Unresolved | Status::Invalid)
}

fn route_check(g: &ScopeGraph, cache: &mut Cache, site: &RouteSite) -> RouteCheck {
    let (sn, dn) = (&g.references[site.source], &g.references[site.destination]);
    let source = event_end(g, cache, sn, &site.from_event, site.from_event_range, true);
    let destination = event_end(g, cache, dn, &site.to_event, site.to_event_range, false);
    let (status, why) = if let Some(e) = [&source, &destination]
        .into_iter()
        .find(|e| e.status != Status::Resolved && is_defect(e.status))
    {
        // A proven event defect on one side survives the other side.
        (e.status, e.reason)
    } else if let Some(n) = [sn, dn].into_iter().find(|n| n.status != Status::Resolved) {
        (n.status, n.reason)
    } else if let Some(e) = [&source, &destination]
        .into_iter()
        .find(|e| e.status != Status::Resolved)
    {
        (e.status, e.reason)
    } else {
        let (a, b) = (
            source.field_type.as_deref().unwrap_or(""),
            destination.field_type.as_deref().unwrap_or(""),
        );
        if !FIELD_TYPES.contains(&a) || !FIELD_TYPES.contains(&b) {
            (Status::Unsupported, reason::FIELD_TYPE_UNKNOWN)
        } else if a != b {
            // 4.10.2: the types shall match exactly. No conversion.
            (Status::Invalid, reason::ROUTE_TYPE_MISMATCH)
        } else {
            (Status::Resolved, reason::OK)
        }
    };
    RouteCheck {
        range: site.range,
        source_node: site.source,
        destination_node: site.destination,
        source,
        destination,
        status,
        reason: why,
    }
}

// --- IS --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(super) struct IsSite {
    /// The node instance that holds the connection.
    instance: usize,
    /// `Some((access, type))` for a Script interface declaration
    /// `eventIn SFFloat x IS y`; `None` for a node field `f IS y`.
    declared: Option<(String, Option<String>)>,
    field: Option<String>,
    field_range: Option<Range>,
    is_name: Option<String>,
    is_range: Option<Range>,
    range: Range,
    scope: Option<usize>,
    proto_owner: Option<usize>,
}

impl IsSite {
    pub(super) fn field(instance: usize, f: &Field, ctx: Ctx) -> IsSite {
        let (is_name, is_range) = match f.value.as_deref() {
            Some(Ast::Is {
                name, name_range, ..
            }) => (name.clone(), *name_range),
            _ => (None, None),
        };
        IsSite {
            instance,
            declared: None,
            field: Some(f.name.clone()),
            field_range: Some(f.name_range),
            is_name,
            is_range,
            range: f.range,
            scope: ctx.scope,
            proto_owner: ctx.proto_owner,
        }
    }

    pub(super) fn interface(instance: usize, i: &Interface, ctx: Ctx) -> IsSite {
        IsSite {
            instance,
            declared: Some((i.access.clone(), i.field_type.clone())),
            field: i.name.clone(),
            field_range: i.name_range,
            is_name: i.is.clone(),
            is_range: i.is_range,
            range: i.range,
            scope: ctx.scope,
            proto_owner: ctx.proto_owner,
        }
    }
}

/// The answer for one `IS` connection.
#[derive(Debug, Clone, PartialEq)]
pub struct IsBinding {
    /// From the node field name to the interface name.
    pub range: Range,
    pub field: Option<String>,
    pub field_range: Option<Range>,
    pub is_name: Option<String>,
    pub is_range: Option<Range>,
    /// The innermost enclosing PROTO (4.8.4), index into `types`.
    pub owner: Option<usize>,
    pub status: Status,
    pub reason: &'static str,
    /// The node side (Table 4.4 row): effective access and type.
    pub definition: Option<(Access, String)>,
    /// The interface side (Table 4.4 column): effective access and type.
    pub declaration: Option<(Access, String)>,
    pub detail: Option<&'static str>,
}

fn is_binding(g: &ScopeGraph, cache: &mut Cache, s: &IsSite) -> IsBinding {
    let mut b = IsBinding {
        range: s.range,
        field: s.field.clone(),
        field_range: s.field_range,
        is_name: s.is_name.clone(),
        is_range: s.is_range,
        owner: s.proto_owner,
        status: Status::Unsupported,
        reason: reason::IS_IN_PROTO_INTERFACE_DEFAULT,
        definition: None,
        declaration: None,
        detail: None,
    };
    let done = |mut b: IsBinding, (st, r): Withheld| {
        b.status = st;
        b.reason = r;
        b
    };
    let Some(scope) = s.scope else {
        return b;
    };
    if let Some(why) = g.document_recovered.or(g.scopes[scope].recovered) {
        return done(b, (Status::Recovered, why));
    }
    // 4.8.3: IS statements appear inside a prototype definition.
    let Some(owner) = s.proto_owner else {
        return done(b, (Status::Invalid, reason::IS_OUTSIDE_PROTO_DEFINITION));
    };
    let (Some(is_name), Some(field)) = (&s.is_name, &s.field) else {
        return done(b, (Status::Invalid, reason::MISSING_NAME));
    };
    // The declaration side: ONLY the innermost PROTO's interface. Never an
    // outer or unrelated interface, never by matching names elsewhere.
    let decl = match entries_of(g, Iface::Declared(owner)) {
        Ok(e) => e,
        Err(w) => return done(b, w),
    };
    let decl_entry = match decl.find(is_name).as_slice() {
        [x] => (*x).clone(),
        [] => {
            return done(
                b,
                (Status::Unresolved, reason::IS_INTERFACE_MEMBER_NOT_DECLARED),
            )
        }
        _ => return done(b, (Status::Ambiguous, reason::DUPLICATE_INTERFACE_MEMBER)),
    };
    b.declaration = Some((decl_entry.access, decl_entry.field_type.clone()));
    // The definition side: the node's own field or event.
    let def = if let Some((acc, ty)) = &s.declared {
        // Script-only syntax: only on a node PROVEN to be the built-in Script.
        if !g.instances[s.instance].is_builtin("Script") {
            return done(
                b,
                (
                    Status::Unsupported,
                    reason::INTERFACE_DECLARATION_ON_NON_SCRIPT,
                ),
            );
        }
        match (Access::parse(acc), ty) {
            (Some(a), Some(t)) => (a, t.clone(), false),
            _ => return done(b, (Status::Recovered, reason::INTERFACE_NOT_PROVABLE)),
        }
    } else {
        let entries = match cache.entries(g, s.instance) {
            Ok(e) => e,
            Err(w) => return done(b, w),
        };
        match entries.find(field).as_slice() {
            [x] => (x.access, x.field_type.clone(), x.alias),
            [] if entries.externproto => {
                return done(
                    b,
                    (
                        Status::Unsupported,
                        reason::EXTERNPROTO_INTERFACE_NOT_LOCALLY_VERIFIABLE,
                    ),
                )
            }
            [] => return done(b, (Status::Unresolved, reason::IS_NODE_FIELD_UNKNOWN)),
            _ => return done(b, (Status::Ambiguous, reason::DUPLICATE_INTERFACE_MEMBER)),
        }
    };
    b.definition = Some((def.0, def.1.clone()));
    b.detail = (def.2 || decl_entry.alias).then_some(detail::EVENT_VIA_EXPOSEDFIELD_ALIAS);
    if !table_4_4(def.0, decl_entry.access) {
        return done(b, (Status::Invalid, reason::IS_ACCESS_MISMATCH));
    }
    let (a, t) = (def.1.as_str(), decl_entry.field_type.as_str());
    if !FIELD_TYPES.contains(&a) || !FIELD_TYPES.contains(&t) {
        return done(b, (Status::Unsupported, reason::FIELD_TYPE_UNKNOWN));
    }
    if a != t {
        // 4.8.3: the types shall match. No SF/MF or other conversion.
        return done(b, (Status::Invalid, reason::IS_TYPE_MISMATCH));
    }
    done(b, (Status::Resolved, reason::OK))
}

/// Fill `routes` and `is_bindings`. Runs after node-name resolution.
pub(super) fn check(g: &mut ScopeGraph) {
    let mut cache = Cache::new(g);
    let routes: Vec<RouteCheck> = g
        .route_sites
        .iter()
        .map(|s| route_check(g, &mut cache, s))
        .collect();
    let is_bindings: Vec<IsBinding> = g
        .is_sites
        .iter()
        .map(|s| is_binding(g, &mut cache, s))
        .collect();
    g.routes = routes;
    g.is_bindings = is_bindings;
}

// --- diagnostics -------------------------------------------------------------------

/// Is the scope a declaration is written in provable?
fn decl_scope_provable(g: &ScopeGraph, t: &TypeDecl) -> bool {
    g.document_recovered.is_none() && t.scope.is_some_and(|s| g.scopes[s].recovered.is_none())
}

fn type_label(g: &ScopeGraph, r: &Reference) -> String {
    g.edit_target(r)
        .map(|d| d.node_type.clone())
        .unwrap_or_else(|| "?".into())
}

/// Advisory typed-reference findings (`SCOPE1xx`). Only proven defects are
/// warnings; uncertain answers produce nothing.
pub(super) fn diagnostics(g: &ScopeGraph) -> Vec<ScopeDiagnostic> {
    let mut out = Vec::new();
    let mut push = |code, severity, reason, message, range: Option<Range>| {
        if let Some(range) = range {
            out.push(ScopeDiagnostic {
                code,
                severity,
                reason,
                message,
                range,
            });
        }
    };
    for r in &g.routes {
        for (e, node, side) in [
            (&r.source, &g.references[r.source_node], "source"),
            (
                &r.destination,
                &g.references[r.destination_node],
                "destination",
            ),
        ] {
            let n = node.name.as_deref().unwrap_or("?");
            let ev = e.name.as_deref().unwrap_or("?");
            let ty = type_label(g, node);
            match (e.status, e.reason) {
                (Status::Unresolved, reason::ROUTE_EVENT_UNKNOWN) => push(
                    code::ROUTE_EVENT_UNKNOWN,
                    "warning",
                    e.reason,
                    format!("ROUTE {side} '{n}.{ev}': {ty} has no event of that name (ISO 4.10.2)"),
                    e.name_range,
                ),
                (Status::Invalid, reason::ROUTE_SOURCE_NOT_AN_EVENT_OUT) => push(
                    code::ROUTE_SOURCE_DIRECTION,
                    "warning",
                    e.reason,
                    format!("ROUTE source '{n}.{ev}' is not an eventOut of {ty} (ISO 4.10.2)"),
                    e.name_range,
                ),
                (Status::Invalid, reason::ROUTE_DESTINATION_NOT_AN_EVENT_IN) => push(
                    code::ROUTE_DESTINATION_DIRECTION,
                    "warning",
                    e.reason,
                    format!("ROUTE destination '{n}.{ev}' is not an eventIn of {ty} (ISO 4.10.2)"),
                    e.name_range,
                ),
                _ => {}
            }
        }
        if (r.status, r.reason) == (Status::Invalid, reason::ROUTE_TYPE_MISMATCH) {
            push(
                code::ROUTE_TYPE_MISMATCH,
                "warning",
                r.reason,
                format!(
                    "ROUTE connects {} to {}; the types must match exactly (ISO 4.10.2)",
                    r.source.field_type.as_deref().unwrap_or("?"),
                    r.destination.field_type.as_deref().unwrap_or("?"),
                ),
                Some(r.range),
            );
        }
    }
    for b in &g.is_bindings {
        let f = b.field.as_deref().unwrap_or("?");
        let x = b.is_name.as_deref().unwrap_or("?");
        let owner = b
            .owner
            .and_then(|o| g.types[o].name.as_deref())
            .unwrap_or("?");
        match (b.status, b.reason) {
            (Status::Unresolved, reason::IS_INTERFACE_MEMBER_NOT_DECLARED) => push(
                code::IS_MEMBER_NOT_DECLARED,
                "warning",
                b.reason,
                format!("IS '{x}': PROTO '{owner}' declares no such field or event (ISO 4.8.3)"),
                b.is_range,
            ),
            (Status::Unresolved, reason::IS_NODE_FIELD_UNKNOWN) => push(
                code::IS_NODE_FIELD_UNKNOWN,
                "warning",
                b.reason,
                format!("'{f}' is not a field or event of this node (ISO 4.8.3)"),
                b.field_range,
            ),
            (Status::Invalid, reason::IS_ACCESS_MISMATCH) => {
                let (d, i) = (b.definition.as_ref(), b.declaration.as_ref());
                push(
                    code::IS_ACCESS_MISMATCH,
                    "warning",
                    b.reason,
                    format!(
                        "'{f}' ({}) IS '{x}' ({}): not allowed by ISO Table 4.4",
                        d.map(|d| d.0.as_str()).unwrap_or("?"),
                        i.map(|i| i.0.as_str()).unwrap_or("?"),
                    ),
                    Some(b.range),
                )
            }
            (Status::Invalid, reason::IS_TYPE_MISMATCH) => {
                let (d, i) = (b.definition.as_ref(), b.declaration.as_ref());
                push(
                    code::IS_TYPE_MISMATCH,
                    "warning",
                    b.reason,
                    format!(
                        "'{f}' ({}) IS '{x}' ({}): the types must match (ISO 4.8.3)",
                        d.map(|d| d.1.as_str()).unwrap_or("?"),
                        i.map(|i| i.1.as_str()).unwrap_or("?"),
                    ),
                    Some(b.range),
                )
            }
            (Status::Invalid, reason::IS_OUTSIDE_PROTO_DEFINITION) => push(
                code::IS_OUTSIDE_PROTO,
                "warning",
                b.reason,
                format!("'{f}' IS '{x}' is outside a PROTO definition (ISO 4.8.3)"),
                Some(b.range),
            ),
            _ => {}
        }
    }
    for t in &g.instances {
        let n = &t.type_name;
        match (t.status, t.reason) {
            (Status::Unresolved, reason::NODE_TYPE_UNKNOWN) => push(
                code::NODE_TYPE_UNKNOWN,
                "warning",
                t.reason,
                format!("'{n}' is not a VRML97 node type, and no PROTO or EXTERNPROTO in scope declares it"),
                Some(t.type_range),
            ),
            (Status::Invalid, reason::PROTO_INSTANCE_BEFORE_DECLARATION) => push(
                code::PROTO_BEFORE_DECLARATION,
                "warning",
                t.reason,
                format!("'{n}' is used before its declaration is complete (ISO 4.8.4)"),
                Some(t.type_range),
            ),
            (Status::Invalid, reason::RECURSIVE_PROTO_INSTANCE) => push(
                code::RECURSIVE_PROTO,
                "warning",
                t.reason,
                format!("'{n}' is used inside its own PROTO definition (ISO 4.8.4)"),
                Some(t.type_range),
            ),
            _ => {}
        }
    }
    // Declarations: 4.8.1 duplicates in ONE scope, and built-in names.
    let mut seen: HashMap<(usize, &str), usize> = HashMap::new();
    for t in &g.types {
        let (Some(name), Some(scope)) = (t.name.as_deref(), t.scope) else {
            continue;
        };
        if !decl_scope_provable(g, t) {
            continue;
        }
        let kw = match t.kind {
            TypeKind::Proto => "PROTO",
            TypeKind::ExternProto => "EXTERNPROTO",
        };
        let count = seen.entry((scope, name)).or_insert(0);
        *count += 1;
        if *count > 1 {
            push(
                code::DUPLICATE_PROTO,
                "warning",
                reason::DUPLICATE_PROTO_DECLARATION,
                format!("{kw} '{name}' repeats a node type name in this scope (ISO 4.8.1)"),
                t.name_range,
            );
        }
        if node_schema::is_vrml97_node(name) {
            push(
                code::PROTO_SHADOWS_BUILTIN,
                "info",
                detail::PROTO_SHADOWS_BUILTIN,
                format!(
                    "{kw} '{name}' has the name of a built-in node; WRL Forge binds later instances to this {kw} (ISO 4.8.1: browser results undefined)"
                ),
                t.name_range,
            );
        }
    }
    out
}
