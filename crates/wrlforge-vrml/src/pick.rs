// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-proven viewport picking (VISUAL-2): a plain-data X_ITE hit snapshot
//! -> the exact authored node -> an existing Scene Tree item, or an explicit
//! refusal.
//!
//! A Rust translation of `src/editor/viewport-pick.js` (WD2-D, merged in
//! PR #117 and independently QA'd with `WRONG_SOURCE_SELECTIONS = 0`), with
//! the same steps, statuses and reason ids. Pure: no X_ITE, no I/O. The
//! snapshot comes from the one narrow X_ITE adapter
//! (`src/preview/xite-pick-adapter.js`); this module is the authority that
//! decides what it means.
//!
//! WHERE THE PROOF COMES FROM:
//!   1. Provenance: X_ITE's own parser reported that runtime object R came
//!      from the node statement at `[start, end)` of the EXACT preview text.
//!   2. Exact join: `[start, end)` (shifted by the preview offset, see
//!      [`resolve`]) equals ONE AST `Node` range, both ends. There is no
//!      nearest / containing / overlapping lookup.
//!   3. Uniqueness: R and every document ancestor have exactly ONE source
//!      occurrence and ONE live runtime parent, so R is rendered once and "the
//!      hit Shape" is "the clicked occurrence". A USE anywhere refuses.
//!   4. Agreement: the runtime chain equals the AST containment chain, link
//!      by link, by node identity.
//!
//! NO FALLBACK. Every failed step is a refusal. Nothing here retries by name,
//! type, index, offset proximity, similarity or position.
//!
//! Generation and revision checks (is this snapshot of the text the document
//! holds NOW?) belong to the caller, which owns the session; this module is
//! given a parse of exactly the text the snapshot's generation rendered.

use std::collections::HashMap;

use crate::ast::{Ast, Document, Node};
use crate::diagnostics::Severity;
use crate::parser::ParseResult;
use crate::scene::{self, Kind, SceneTree};

/// One result status. The strings are the WD2-D protocol values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Proven,
    NoHit,
    RefusedAmbiguous,
    RefusedExternal,
    RefusedSensorConflict,
    RefusedStale,
    Unsupported,
    CompatibilityDisabled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Proven => "PROVEN",
            Status::NoHit => "NO_HIT",
            Status::RefusedAmbiguous => "REFUSED_AMBIGUOUS",
            Status::RefusedExternal => "REFUSED_EXTERNAL",
            Status::RefusedSensorConflict => "REFUSED_SENSOR_CONFLICT",
            Status::RefusedStale => "REFUSED_STALE",
            Status::Unsupported => "UNSUPPORTED",
            Status::CompatibilityDisabled => "COMPATIBILITY_DISABLED",
        }
    }
}

/// Stable reason ids (the WD2-D `REASON` table).
pub mod reason {
    pub const SHAPE: &str = "shape";
    pub const PROMOTED: &str = "shape-promoted-to-proven-simple-object";
    pub const NO_GEOMETRY: &str = "no-geometry-under-pointer";
    pub const SEVERAL_OCCURRENCES: &str = "runtime-node-referenced-by-several-source-occurrences";
    pub const SEVERAL_PARENTS: &str = "runtime-node-has-several-live-parents";
    pub const OTHER_DOCUMENT: &str = "hit-belongs-to-another-document";
    pub const SENSOR: &str = "pointing-device-sensor-under-pointer";
    pub const OTHER_GENERATION: &str = "hit-from-another-preview-generation";
    pub const SOURCE_CHANGED: &str = "source-changed-since-preview";
    pub const LAST_VALID_SCENE: &str = "preview-shows-last-valid-scene";
    pub const SCENE_REPLACED: &str = "preview-scene-replaced";
    pub const PROTO_INSTANCE: &str = "proto-instance";
    pub const SYNTAX_ERRORS: &str = "document-has-syntax-errors";
    pub const PARSE_CAPPED: &str = "document-parse-capped";
    pub const NO_PROVENANCE: &str = "runtime-node-has-no-parse-provenance";
    pub const DETACHED: &str = "runtime-node-detached";
    pub const PARENT_OUTSIDE_DOCUMENT: &str = "runtime-parent-outside-document";
    pub const CHAIN_UNBOUNDED: &str = "runtime-chain-unbounded";
    pub const JOIN_TYPE: &str = "exact-span-join-type-disagrees";
    pub const CHAIN_DISAGREES: &str = "runtime-chain-disagrees-with-source-containment";
    pub const NOT_IN_TREE: &str = "logical-node-not-in-scene-tree";
    pub const GENERATION_UNPROVABLE: &str = "generation-unprovable";
    pub const SPAN_OUT_OF_RANGE: &str = "provenance-span-out-of-range";
    /// VISUAL-3A1 runtime binding: the located runtime node's provenance is
    /// not exactly the authored node it was asked for.
    pub const BIND_SPAN: &str = "runtime-node-is-not-the-authored-node";
    /// VISUAL-3A1 runtime binding: zero or several runtime nodes carry the
    /// authored node's exact span.
    pub const BIND_NOT_UNIQUE: &str = "runtime-node-for-span-not-unique";
}

/// The execution context the adapter classified a runtime node into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    /// The preview generation's own scene.
    Document,
    /// Another `X3DScene` (an Inline / external world).
    ExternalScene,
    /// Any other execution context: a PROTO instance body.
    ProtoBody,
    /// X_ITE's own layer-0 holders, identified by object identity.
    WorldInfrastructure,
    /// No execution context.
    None,
}

impl Ctx {
    /// The adapter's `ctxKind` strings. Anything unknown is `None`.
    pub fn parse(s: &str) -> Ctx {
        match s {
            "document" => Ctx::Document,
            "external-scene" => Ctx::ExternalScene,
            "proto-body" => Ctx::ProtoBody,
            "world-infrastructure" => Ctx::WorldInfrastructure,
            _ => Ctx::None,
        }
    }
}

/// A runtime parent, as the adapter labelled it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parent {
    /// Another runtime node in the same snapshot graph.
    Node(String),
    /// The generation's own scene (a root node).
    Scene,
    /// Some other execution context.
    OtherContext,
}

impl Parent {
    pub fn parse(s: &str) -> Parent {
        match s {
            "SCENE" => Parent::Scene,
            "OTHER_CONTEXT" => Parent::OtherContext,
            other => Parent::Node(other.to_string()),
        }
    }
}

/// One runtime node of the hit snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub type_name: Option<String>,
    pub ctx: Ctx,
    /// Parse-provenance spans `[start, end)` in UTF-16 units of the PREVIEW
    /// text: one per node statement the parser built this object from.
    pub occurrences: Vec<(u64, u64)>,
    pub parents: Vec<Parent>,
}

/// A geometry hit, in plain data. Labels are local to one snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub shape: String,
    pub ctx: Ctx,
    /// Type names of the pointing-device sensors (and Anchors) X_ITE would
    /// activate at this point.
    pub sensors: Vec<Option<String>>,
    pub graph: HashMap<String, GraphNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Shape,
    SimpleObject,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Shape => "shape",
            Role::SimpleObject => "simple-object",
        }
    }
}

/// An exact source node: UTF-16 span of the canonical text and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNode {
    pub from: u64,
    pub to: u64,
    pub node_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proven {
    /// The hit Shape's own statement.
    pub shape: SourceNode,
    /// What is selected: the Shape, or its simple-object Transform.
    pub logical: SourceNode,
    pub role: Role,
    /// The logical node's Scene Tree item id.
    pub item: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Proven(Proven),
    Refused { status: Status, reason: String },
}

impl Resolution {
    pub fn status(&self) -> Status {
        match self {
            Resolution::Proven(_) => Status::Proven,
            Resolution::Refused { status, .. } => *status,
        }
    }
    pub fn reason(&self) -> &str {
        match self {
            Resolution::Proven(p) => match p.role {
                Role::Shape => reason::SHAPE,
                Role::SimpleObject => reason::PROMOTED,
            },
            Resolution::Refused { reason, .. } => reason,
        }
    }
}

pub fn refuse(status: Status, reason: impl Into<String>) -> Resolution {
    Resolution::Refused {
        status,
        reason: reason.into(),
    }
}

/// `exact-span-join-found-<n>`.
fn join_found(n: usize) -> String {
    format!("exact-span-join-found-{n}")
}

// ---- source side ------------------------------------------------------------

/// The nearest enclosing Node / Document / Proto, by identity.
#[derive(Clone, Copy)]
enum Encl {
    Doc,
    Node(*const Node),
    Proto,
}

impl Encl {
    fn is_node(self, n: &Node) -> bool {
        matches!(self, Encl::Node(p) if std::ptr::eq(p, n))
    }
}

struct Index<'a> {
    by_span: HashMap<(u64, u64), Vec<&'a Node>>,
    enclosing: HashMap<*const Node, Encl>,
}

fn index(doc: &Document) -> Index<'_> {
    let mut ix = Index {
        by_span: HashMap::new(),
        enclosing: HashMap::new(),
    };
    for s in &doc.statements {
        visit(s, Encl::Doc, &mut ix);
    }
    ix
}

fn visit<'a>(ast: &'a Ast, encl: Encl, ix: &mut Index<'a>) {
    match ast {
        Ast::Node(n) => {
            let r = n.range;
            ix.by_span
                .entry((r.start.offset as u64, r.end.offset as u64))
                .or_default()
                .push(n);
            ix.enclosing.insert(n as *const Node, encl);
            let me = Encl::Node(n as *const Node);
            for f in &n.fields {
                visit(f, me, ix);
            }
            for i in &n.interfaces {
                if let Some(d) = &i.default {
                    visit(d, me, ix);
                }
            }
        }
        Ast::Field(f) => {
            if let Some(v) = &f.value {
                visit(v, encl, ix);
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| visit(i, encl, ix)),
        Ast::Proto(p) => {
            let me = Encl::Proto;
            for i in &p.interfaces {
                if let Some(d) = &i.default {
                    visit(d, me, ix);
                }
            }
            for b in &p.body {
                visit(b, me, ix);
            }
        }
        Ast::ExternProto(e) => {
            if let Some(u) = &e.url {
                visit(u, encl, ix);
            }
        }
        _ => {}
    }
}

/// The geometry node types whose WD2-C / VISUAL-1 simple object promotes a
/// Shape hit to its Transform. VISUAL-1 creates all four.
const PRIMITIVES: [&str; 4] = ["Box", "Sphere", "Cylinder", "Cone"];

fn fields_named<'a>(n: &'a Node, name: &str) -> Vec<&'a crate::ast::Field> {
    n.fields
        .iter()
        .filter_map(|f| match f {
            Ast::Field(f) if f.name == name => Some(f),
            _ => None,
        })
        .collect()
}

/// The single node value of a field authored exactly once, else `None`. A
/// USE is not a node instance and is never returned.
fn single_node_value<'a>(n: &'a Node, name: &str) -> Option<&'a Node> {
    let fs = fields_named(n, name);
    if fs.len() != 1 || fs[0].is_binding {
        return None;
    }
    match fs[0].value.as_deref() {
        Some(Ast::Node(v)) => Some(v),
        _ => None,
    }
}

/// `src/vrml/simple-object.js` `recognize`: a Transform with exactly one
/// `children` value, a Shape, whose geometry is an authored primitive node.
/// Returns that Shape.
pub fn simple_object_shape(t: &Node) -> Option<&Node> {
    if t.node_type != "Transform" {
        return None;
    }
    let children = fields_named(t, "children");
    if children.len() != 1 || children[0].is_binding {
        return None;
    }
    let shape = match children[0].value.as_deref() {
        Some(Ast::Array { items, .. }) => {
            if items.len() != 1 {
                return None;
            }
            match &items[0] {
                Ast::Node(s) => s,
                _ => return None,
            }
        }
        Some(Ast::Node(s)) => s,
        _ => return None,
    };
    if shape.node_type != "Shape" {
        return None;
    }
    let geometry = single_node_value(shape, "geometry")?;
    PRIMITIVES
        .contains(&geometry.node_type.as_str())
        .then_some(shape)
}

fn source_node(n: &Node) -> SourceNode {
    SourceNode {
        from: n.range.start.offset as u64,
        to: n.range.end.offset as u64,
        node_type: n.node_type.clone(),
    }
}

/// Steps 4-9: prove the runtime chain of `hit.shape` up to the scene and join
/// every link to exactly one AST node. Returns the AST chain, innermost first
/// (`[0]` is the node `hit.shape` came from; the last one is top-level).
fn prove_chain<'a>(
    hit: &Hit,
    parse: &'a ParseResult,
    preview_offset: u64,
) -> Result<Vec<&'a Node>, Resolution> {
    // 4/5. Execution context of the hit Shape.
    match hit.ctx {
        Ctx::Document => {}
        Ctx::ExternalScene => return Err(refuse(Status::RefusedExternal, reason::OTHER_DOCUMENT)),
        Ctx::ProtoBody => return Err(refuse(Status::Unsupported, reason::PROTO_INSTANCE)),
        _ => return Err(refuse(Status::Unsupported, reason::DETACHED)),
    }
    // 6. A damaged or capped document proves nothing.
    if parse
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(refuse(Status::Unsupported, reason::SYNTAX_ERRORS));
    }
    if parse.truncated || parse.depth_capped {
        return Err(refuse(Status::Unsupported, reason::PARSE_CAPPED));
    }

    // 7. Climb: every link uniquely provable.
    struct Link<'g> {
        occ: (u64, u64),
        type_name: Option<&'g str>,
    }
    let mut chain: Vec<Link> = Vec::new();
    let mut label: &str = &hit.shape;
    let mut reached_scene = false;
    for _ in 0..256 {
        let Some(g) = hit.graph.get(label) else {
            return Err(refuse(Status::Unsupported, reason::CHAIN_UNBOUNDED));
        };
        match g.ctx {
            Ctx::Document => {}
            Ctx::ProtoBody => return Err(refuse(Status::Unsupported, reason::PROTO_INSTANCE)),
            Ctx::ExternalScene => {
                return Err(refuse(Status::RefusedExternal, reason::OTHER_DOCUMENT))
            }
            _ => return Err(refuse(Status::Unsupported, reason::PARENT_OUTSIDE_DOCUMENT)),
        }
        match g.occurrences.len() {
            0 => return Err(refuse(Status::Unsupported, reason::NO_PROVENANCE)),
            1 => {}
            _ => {
                return Err(refuse(
                    Status::RefusedAmbiguous,
                    reason::SEVERAL_OCCURRENCES,
                ))
            }
        }
        let mut doc_parents: Vec<&str> = Vec::new();
        let mut at_scene = 0usize;
        for p in &g.parents {
            match p {
                Parent::Scene => at_scene += 1,
                Parent::OtherContext => {
                    return Err(refuse(Status::Unsupported, reason::PARENT_OUTSIDE_DOCUMENT))
                }
                Parent::Node(pl) => {
                    let Some(pg) = hit.graph.get(pl) else {
                        return Err(refuse(Status::Unsupported, reason::CHAIN_UNBOUNDED));
                    };
                    match pg.ctx {
                        Ctx::WorldInfrastructure => continue,
                        Ctx::ProtoBody => {
                            return Err(refuse(Status::Unsupported, reason::PROTO_INSTANCE))
                        }
                        Ctx::Document => doc_parents.push(pl),
                        _ => {
                            return Err(refuse(
                                Status::Unsupported,
                                reason::PARENT_OUTSIDE_DOCUMENT,
                            ))
                        }
                    }
                }
            }
        }
        match doc_parents.len() + at_scene {
            0 => return Err(refuse(Status::Unsupported, reason::DETACHED)),
            1 => {}
            _ => return Err(refuse(Status::RefusedAmbiguous, reason::SEVERAL_PARENTS)),
        }
        chain.push(Link {
            occ: g.occurrences[0],
            type_name: g.type_name.as_deref(),
        });
        if at_scene == 1 {
            reached_scene = true;
            break;
        }
        label = doc_parents[0];
    }
    if !reached_scene {
        return Err(refuse(Status::Unsupported, reason::CHAIN_UNBOUNDED));
    }

    // 8. Exact join, same type.
    let ix = index(&parse.tree);
    let mut ast: Vec<&Node> = Vec::with_capacity(chain.len());
    for link in &chain {
        let (Some(from), Some(to)) = (
            link.occ.0.checked_add(preview_offset),
            link.occ.1.checked_add(preview_offset),
        ) else {
            return Err(refuse(Status::Unsupported, reason::SPAN_OUT_OF_RANGE));
        };
        let found = ix
            .by_span
            .get(&(from, to))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if found.len() != 1 {
            return Err(refuse(Status::Unsupported, join_found(found.len())));
        }
        if link.type_name != Some(found[0].node_type.as_str()) {
            return Err(refuse(Status::Unsupported, reason::JOIN_TYPE));
        }
        ast.push(found[0]);
    }
    // 9. Runtime chain == AST containment chain, by identity.
    for (i, n) in ast.iter().enumerate() {
        let Some(&encl) = ix.enclosing.get(&(*n as *const Node)) else {
            return Err(refuse(Status::Unsupported, reason::CHAIN_DISAGREES));
        };
        let agrees = match ast.get(i + 1) {
            Some(parent) => encl.is_node(parent),
            None => matches!(encl, Encl::Doc),
        };
        if !agrees {
            return Err(refuse(Status::Unsupported, reason::CHAIN_DISAGREES));
        }
    }
    Ok(ast)
}

/// Resolve one geometry hit against `parse` (of the canonical text whose
/// preview the hit was taken from) and its Scene Tree.
///
/// `preview_offset` is the number of UTF-16 units the canonical text has
/// BEFORE the preview text (1 when the preview omitted a leading BOM, else
/// 0): source offset = provenance offset + `preview_offset`. The caller
/// proves that the preview text is exactly the canonical text minus that
/// prefix.
pub fn resolve(
    hit: &Hit,
    parse: &ParseResult,
    tree: &SceneTree,
    preview_offset: u64,
) -> Resolution {
    // 3. Authored interaction wins.
    if !hit.sensors.is_empty() {
        return refuse(Status::RefusedSensorConflict, reason::SENSOR);
    }
    let ast = match prove_chain(hit, parse, preview_offset) {
        Ok(a) => a,
        Err(r) => return r,
    };

    // 10. Promotion: only to the recognized simple object of THIS Shape.
    let shape = ast[0];
    let (logical, role) = match ast.get(1) {
        Some(&t) if simple_object_shape(t).is_some_and(|s| std::ptr::eq(s, shape)) => {
            (t, Role::SimpleObject)
        }
        _ => (shape, Role::Shape),
    };
    // 11. The existing Scene Tree item.
    let item = scene::item_id(Kind::Node, logical.range);
    if !tree
        .items
        .get(&item)
        .is_some_and(|it| it.kind == Kind::Node)
    {
        return refuse(Status::Unsupported, reason::NOT_IN_TREE);
    }
    Resolution::Proven(Proven {
        shape: source_node(shape),
        logical: source_node(logical),
        role,
        item,
    })
}

/// VISUAL-3A1: prove that the runtime node a span lookup found
/// (`hit.shape`, from `xite-pick-adapter` `snapshotSpan`) IS the authored
/// node `[from, to)` of the canonical text, by the same chain proof a pick
/// uses (steps 4-9): exactly one source occurrence and one live parent per
/// link, every link joined to exactly one AST node of the same type, and the
/// runtime chain equal to the source containment chain up to the scene.
///
/// Returns the AST chain, innermost (`[0]`, the authored node) first: its
/// length is the node's depth (1 = a top-level statement). Never a search:
/// the link must be exactly `[from, to)`, both ends.
pub fn resolve_node<'a>(
    hit: &Hit,
    parse: &'a ParseResult,
    preview_offset: u64,
    from: u64,
    to: u64,
) -> Result<Vec<&'a Node>, Resolution> {
    let ast = prove_chain(hit, parse, preview_offset)?;
    let n = ast[0];
    if (n.range.start.offset as u64, n.range.end.offset as u64) != (from, to) {
        return Err(refuse(Status::Unsupported, reason::BIND_SPAN));
    }
    Ok(ast)
}

/// Contextual one-line UX text per refusal (the WD2-D wording).
pub fn refusal_text(status: Status, reason: &str) -> String {
    match status {
        Status::Proven | Status::NoHit => String::new(),
        Status::RefusedAmbiguous => {
            "This object is drawn by a shared (DEF/USE) node; select it in the Scene Tree.".into()
        }
        Status::RefusedExternal => {
            "This object belongs to an Inline file and cannot be selected from this document."
                .into()
        }
        Status::RefusedSensorConflict => {
            "This object is interactive (sensor or link); select it in the Scene Tree.".into()
        }
        Status::RefusedStale => "The preview is out of date; select after it updates.".into(),
        Status::Unsupported => {
            "This object cannot be selected from the preview; select it in the Scene Tree.".into()
        }
        Status::CompatibilityDisabled => format!(
            "Preview picking unavailable (X_ITE compatibility check failed: {reason}). Select objects in the Scene Tree."
        ),
    }
}

#[cfg(test)]
#[path = "pick/tests.rs"]
mod tests;
