// SPDX-License-Identifier: GPL-3.0-or-later
//! Scene-tree and inspector READ projections. Partial translation of
//! `src/vrml/scene-tree.js` (item selection rules and id format) plus a
//! read-only field listing for the inspector.
//!
//! Both are derived from one parse of the canonical text and are disposable.
//! Ids are `<kind>-<startUtf16>-<endUtf16>`, exactly the JS format: session
//! stable for one text, never persisted, never authoritative node identity.
//!
//! Reference status (USE and ROUTE node names) comes from the RUST-STAB-2A
//! lexical scope graph (`crate::scope`) via `build_scene_tree_scoped`
//! (`resolution_scope = "lexical"`). `build_scene_tree` alone gives the plain
//! structure with no reference answers (`resolution_scope = "none"`). The old
//! flat DEF table is gone. A reference target is an item id only when the
//! graph proves exactly one DEF; it is never runtime object identity.

use std::collections::HashMap;

use crate::ast::*;
use crate::parser::ParseResult;
use crate::scope::{self, ScopeGraph};
use crate::tokenizer::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Document,
    Node,
    Use,
    Proto,
    ExternProto,
    Route,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Document => "Document",
            Kind::Node => "Node",
            Kind::Use => "Use",
            Kind::Proto => "Proto",
            Kind::ExternProto => "ExternProto",
            Kind::Route => "Route",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub label: String,
    pub detail: Option<String>,
    /// UTF-16 source span `[from, to)`.
    pub from: u64,
    pub to: u64,
    pub depth: u32,
    pub children: Vec<String>,
    /// For a USE: the scope-graph status (`scope::Status::as_str`). `None`
    /// until `build_scene_tree_scoped` annotates the tree.
    pub use_status: Option<&'static str>,
    /// Scope answers: one for a USE, two (source, destination) for a ROUTE.
    pub refs: Vec<RefSummary>,
    /// RUST-STAB-2B typed answer: the whole-ROUTE verdict for a ROUTE, or the
    /// type binding of a node whose type is not a proven built-in.
    pub typed: Option<TypedSummary>,
}

/// One typed answer, projected for display. Never an edit target.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedSummary {
    /// "route" | "type".
    pub subject: &'static str,
    /// `scope::Status::as_str`.
    pub status: &'static str,
    /// "proven" | "unresolved" | "uncertain".
    pub class: &'static str,
    pub reason: &'static str,
    /// Display text: the event types of a ROUTE (`SFFloat -> SFFloat`) or the
    /// declaration kind of a type (`PROTO Pulse`).
    pub summary: Option<String>,
}

/// One reference answer, projected for display.
#[derive(Debug, Clone, PartialEq)]
pub struct RefSummary {
    /// "use" | "route-source" | "route-destination".
    pub role: &'static str,
    pub name: Option<String>,
    /// `scope::Status::as_str`.
    pub status: &'static str,
    /// "proven" | "unresolved" | "uncertain".
    pub class: &'static str,
    pub reason: &'static str,
    /// The DEF node's item id. ONLY for a proven, unique reference whose
    /// DEF node is a Scene Tree item. Never runtime object identity.
    pub target: Option<String>,
    /// The ISO 4.6.2 meaning (closest preceding DEF) as an item id, for
    /// display only. Never a selection or edit target.
    pub language_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneTree {
    pub root: String,
    /// Depth-first source order (root first).
    pub order: Vec<String>,
    pub items: HashMap<String, Item>,
    pub resolution_scope: &'static str,
}

pub fn item_id(kind: Kind, range: Range) -> String {
    format!(
        "{}-{}-{}",
        kind.as_str().to_lowercase(),
        range.start.offset,
        range.end.offset
    )
}

struct Ctx {
    order: Vec<String>,
    items: HashMap<String, Item>,
}

impl Ctx {
    fn push(&mut self, parent: &str, item: Item) -> String {
        let id = item.id.clone();
        if let Some(p) = self.items.get_mut(parent) {
            p.children.push(id.clone());
        }
        self.order.push(id.clone());
        self.items.insert(id.clone(), item);
        id
    }

    fn emit(&mut self, ast: &Ast, parent: &str, depth: u32, src: &str) {
        match ast {
            Ast::Node(n) => self.emit_node(n, parent, depth, src),
            Ast::Use { name, range, .. } => {
                self.push(
                    parent,
                    Item {
                        id: item_id(Kind::Use, *range),
                        kind: Kind::Use,
                        label: format!("USE {}", name.as_deref().unwrap_or("?")),
                        detail: None,
                        from: range.start.offset as u64,
                        to: range.end.offset as u64,
                        depth,
                        children: vec![],
                        use_status: None,
                        refs: vec![],
                        typed: None,
                    },
                );
            }
            Ast::Proto(p) => {
                let id = self.push(
                    parent,
                    Item {
                        id: item_id(Kind::Proto, p.range),
                        kind: Kind::Proto,
                        label: format!("PROTO {}", p.name.as_deref().unwrap_or("?")),
                        detail: Some(format!("{} interface(s)", p.interfaces.len())),
                        from: p.range.start.offset as u64,
                        to: p.range.end.offset as u64,
                        depth,
                        children: vec![],
                        use_status: None,
                        refs: vec![],
                        typed: None,
                    },
                );
                for b in &p.body {
                    self.emit(b, &id, depth + 1, src);
                }
            }
            Ast::ExternProto(e) => {
                self.push(
                    parent,
                    Item {
                        id: item_id(Kind::ExternProto, e.range),
                        kind: Kind::ExternProto,
                        label: format!("EXTERNPROTO {}", e.name.as_deref().unwrap_or("?")),
                        detail: Some(format!("{} interface(s)", e.interfaces.len())),
                        from: e.range.start.offset as u64,
                        to: e.range.end.offset as u64,
                        depth,
                        children: vec![],
                        use_status: None,
                        refs: vec![],
                        typed: None,
                    },
                );
            }
            Ast::Route(r) => {
                let end = |e: &RouteEnd| {
                    format!(
                        "{}.{}",
                        e.node.as_deref().unwrap_or("?"),
                        e.event.as_deref().unwrap_or("?")
                    )
                };
                self.push(
                    parent,
                    Item {
                        id: item_id(Kind::Route, r.range),
                        kind: Kind::Route,
                        label: format!("ROUTE {} → {}", end(&r.from), end(&r.to)),
                        detail: None,
                        from: r.range.start.offset as u64,
                        to: r.range.end.offset as u64,
                        depth,
                        children: vec![],
                        use_status: None,
                        refs: vec![],
                        typed: None,
                    },
                );
            }
            _ => {}
        }
    }

    fn emit_node(&mut self, n: &Node, parent: &str, depth: u32, src: &str) {
        let label = match &n.def {
            Some(d) => format!("{} {}", n.node_type, d),
            None => n.node_type.clone(),
        };
        let id = self.push(
            parent,
            Item {
                id: item_id(Kind::Node, n.range),
                kind: Kind::Node,
                label,
                detail: n.def.as_ref().map(|_| "DEF".to_string()),
                from: n.range.start.offset as u64,
                to: n.range.end.offset as u64,
                depth,
                children: vec![],
                use_status: None,
                refs: vec![],
                typed: None,
            },
        );
        for f in &n.fields {
            match f {
                // Body statements are real scene items.
                Ast::Route(_) | Ast::Proto(_) | Ast::ExternProto(_) => {
                    self.emit(f, &id, depth + 1, src)
                }
                Ast::Field(field) => {
                    if let Some(v) = &field.value {
                        self.emit_value(v, &id, depth + 1, src);
                    }
                }
                _ => {}
            }
        }
    }

    /// SFNode / MFNode values only. ROUTE/PROTO inside an MFNode array are the
    /// Cybertown compatibility pattern and are deliberately NOT scene items.
    fn emit_value(&mut self, v: &Ast, parent: &str, depth: u32, src: &str) {
        match v {
            Ast::Node(_) | Ast::Use { .. } => self.emit(v, parent, depth, src),
            Ast::Array { items, .. } => {
                for i in items {
                    if matches!(i, Ast::Node(_) | Ast::Use { .. }) {
                        self.emit(i, parent, depth, src);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn build_scene_tree(doc: &Document, src: &str) -> SceneTree {
    let root = Item {
        id: item_id(Kind::Document, doc.range),
        kind: Kind::Document,
        label: "Document".into(),
        detail: None,
        from: doc.range.start.offset as u64,
        to: doc.range.end.offset as u64,
        depth: 0,
        children: vec![],
        use_status: None,
        refs: vec![],
        typed: None,
    };
    let root_id = root.id.clone();
    let mut ctx = Ctx {
        order: vec![root_id.clone()],
        items: HashMap::from([(root_id.clone(), root)]),
    };
    for s in &doc.statements {
        ctx.emit(s, &root_id, 1, src);
    }
    SceneTree {
        root: root_id,
        order: ctx.order,
        items: ctx.items,
        resolution_scope: "none",
    }
}

/// The Scene Tree plus the lexical scope graph of the SAME parse, with every
/// USE and ROUTE item annotated from the graph.
pub fn build_scene_tree_scoped(parse: &ParseResult, src: &str) -> (SceneTree, ScopeGraph) {
    let mut tree = build_scene_tree(&parse.tree, src);
    let graph = scope::build(parse);
    let summary = |r: &scope::Reference, items: &HashMap<String, Item>| {
        // A target is offered only when the graph proves it AND the DEF node
        // is a real item of this tree.
        let as_item = |d: Option<&scope::DefDecl>| {
            d.map(|d| item_id(Kind::Node, d.node_range))
                .filter(|id| items.contains_key(id))
        };
        RefSummary {
            role: r.kind.as_str(),
            name: r.name.clone(),
            status: r.status.as_str(),
            class: r.status.class(),
            reason: r.reason,
            target: as_item(graph.edit_target(r)),
            language_target: as_item(graph.language_target(r)),
        }
    };
    // Statement span -> reference indices (one USE, or a ROUTE's two).
    let mut by_span: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (i, r) in graph.references.iter().enumerate() {
        let key = (
            r.statement_range.start.offset as u64,
            r.statement_range.end.offset as u64,
        );
        by_span.entry(key).or_default().push(i);
    }
    let mut refs: Vec<(String, Vec<RefSummary>)> = Vec::new();
    for (id, it) in &tree.items {
        let want: &[scope::RefKind] = match it.kind {
            Kind::Use => &[scope::RefKind::Use],
            Kind::Route => &[
                scope::RefKind::RouteSource,
                scope::RefKind::RouteDestination,
            ],
            _ => continue,
        };
        let Some(found) = by_span.get(&(it.from, it.to)) else {
            continue;
        };
        let picked: Vec<RefSummary> = want
            .iter()
            .filter_map(|k| {
                let mut m = found.iter().filter(|&&i| graph.references[i].kind == *k);
                // Exactly one reference of this role at this span, or none.
                match (m.next(), m.next()) {
                    (Some(&i), None) => Some(summary(&graph.references[i], &tree.items)),
                    _ => None,
                }
            })
            .collect();
        if picked.len() == want.len() {
            refs.push((id.clone(), picked));
        }
    }
    for (id, r) in refs {
        let it = tree.items.get_mut(&id).expect("item id from this tree");
        if it.kind == Kind::Use {
            it.use_status = r.first().map(|r| r.status);
        }
        it.refs = r;
    }
    // RUST-STAB-2B: typed answers, matched by exact statement span only.
    let mut typed: HashMap<(u64, u64), TypedSummary> = HashMap::new();
    for rc in &graph.routes {
        let summary = match (&rc.source.field_type, &rc.destination.field_type) {
            (Some(a), Some(b)) => Some(format!("{a} -> {b}")),
            _ => None,
        };
        typed.insert(
            (rc.range.start.offset as u64, rc.range.end.offset as u64),
            TypedSummary {
                subject: "route",
                status: rc.status.as_str(),
                class: rc.status.class(),
                reason: rc.reason,
                summary,
            },
        );
    }
    for t in graph.instances.iter().filter(|t| !t.builtin) {
        let summary = t.target.map(|d| {
            let decl = &graph.types[d];
            let kw = match decl.kind {
                scope::TypeKind::Proto => "PROTO",
                scope::TypeKind::ExternProto => "EXTERNPROTO",
            };
            format!("{kw} {}", decl.name.as_deref().unwrap_or("?"))
        });
        typed.insert(
            (
                t.node_range.start.offset as u64,
                t.node_range.end.offset as u64,
            ),
            TypedSummary {
                subject: "type",
                status: t.status.as_str(),
                class: t.status.class(),
                reason: t.reason,
                summary,
            },
        );
    }
    for it in tree.items.values_mut() {
        let want = match it.kind {
            Kind::Route => "route",
            Kind::Node => "type",
            _ => continue,
        };
        if let Some(t) = typed.get(&(it.from, it.to)).filter(|t| t.subject == want) {
            it.typed = Some(t.clone());
        }
    }
    tree.resolution_scope = "lexical";
    (tree, graph)
}

// --- inspector (read only) ---------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct InspectorRow {
    pub name: String,
    /// Value kind (`numbers`, `string`, `node`, `is`, ...) or the interface access.
    pub kind: String,
    /// The EXACT source text of the value (never reformatted), possibly elided
    /// for display; `elided` says so.
    pub source: String,
    pub elided: bool,
    pub from: u64,
    pub to: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inspection {
    pub id: String,
    pub title: String,
    pub rows: Vec<InspectorRow>,
}

const MAX_VALUE_CHARS: usize = 240;

fn row(name: String, kind: &str, range: Range, src: &str) -> InspectorRow {
    let full = range.slice(src);
    let (source, elided) = if full.chars().count() > MAX_VALUE_CHARS {
        (full.chars().take(MAX_VALUE_CHARS).collect::<String>(), true)
    } else {
        (full.to_string(), false)
    };
    InspectorRow {
        name,
        kind: kind.to_string(),
        source,
        elided,
        from: range.start.offset as u64,
        to: range.end.offset as u64,
    }
}

fn find<'a>(ast: &'a Ast, id: &str) -> Option<&'a Ast> {
    let kind = match ast {
        Ast::Node(_) => Some(Kind::Node),
        Ast::Use { .. } => Some(Kind::Use),
        Ast::Proto(_) => Some(Kind::Proto),
        Ast::ExternProto(_) => Some(Kind::ExternProto),
        Ast::Route(_) => Some(Kind::Route),
        _ => None,
    };
    if let Some(k) = kind {
        if item_id(k, ast.range()) == id {
            return Some(ast);
        }
    }
    match ast {
        Ast::Node(n) => n.fields.iter().find_map(|f| find(f, id)),
        Ast::Field(f) => f.value.as_deref().and_then(|v| find(v, id)),
        Ast::Array { items, .. } => items.iter().find_map(|i| find(i, id)),
        Ast::Proto(p) => p.body.iter().find_map(|b| find(b, id)),
        _ => None,
    }
}

fn interface_rows(ifs: &[Interface], src: &str, rows: &mut Vec<InspectorRow>) {
    for i in ifs {
        let name = format!(
            "{} {} {}",
            i.access,
            i.field_type.as_deref().unwrap_or("?"),
            i.name.as_deref().unwrap_or("?")
        );
        let range = i
            .default
            .as_ref()
            .map(|d| d.range())
            .or(i.is_range)
            .unwrap_or(i.range);
        rows.push(row(name, "interface", range, src));
    }
}

pub fn inspect(doc: &Document, src: &str, id: &str) -> Option<Inspection> {
    let ast = doc.statements.iter().find_map(|s| find(s, id))?;
    let mut rows = Vec::new();
    let title = match ast {
        Ast::Node(n) => {
            if let (Some(d), Some(r)) = (&n.def, n.def_range) {
                rows.push(row("DEF".into(), "name", r, src));
                let _ = d;
            }
            interface_rows(&n.interfaces, src, &mut rows);
            for f in &n.fields {
                match f {
                    Ast::Field(field) => {
                        let (kind, range) = match &field.value {
                            Some(v) => (v.kind_name(), v.range()),
                            None => ("missing", field.name_range),
                        };
                        rows.push(row(field.name.clone(), kind, range, src));
                    }
                    other => rows.push(row(
                        other.kind_name().to_uppercase(),
                        other.kind_name(),
                        other.range(),
                        src,
                    )),
                }
            }
            match &n.def {
                Some(d) => format!("{} {}", n.node_type, d),
                None => n.node_type.clone(),
            }
        }
        Ast::Use { name, range, .. } => {
            rows.push(row("USE".into(), "use", *range, src));
            format!("USE {}", name.as_deref().unwrap_or("?"))
        }
        Ast::Proto(p) => {
            interface_rows(&p.interfaces, src, &mut rows);
            format!("PROTO {}", p.name.as_deref().unwrap_or("?"))
        }
        Ast::ExternProto(e) => {
            interface_rows(&e.interfaces, src, &mut rows);
            if let Some(u) = &e.url {
                rows.push(row("url".into(), u.kind_name(), u.range(), src));
            }
            format!("EXTERNPROTO {}", e.name.as_deref().unwrap_or("?"))
        }
        Ast::Route(r) => {
            rows.push(row("from".into(), "endpoint", r.from.range, src));
            rows.push(row("to".into(), "endpoint", r.to.range, src));
            "ROUTE".into()
        }
        _ => return None,
    };
    Some(Inspection {
        id: id.to_string(),
        title,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const SRC: &str = "#VRML V2.0 utf8\r\nDEF T Transform {\r\n  translation 1 2 3\r\n  children [ Shape { geometry Box { size 1 1 1 } } USE T USE Nope\r\n    ROUTE A.b TO C.d ]\r\n  ROUTE X.y TO Z.w\r\n}\r\nPROTO P [ field SFFloat r 1 ] { Sphere { radius IS r } }\r\n";

    #[test]
    fn tree_follows_js_inclusion_rules() {
        let r = parse(SRC);
        let (t, _) = build_scene_tree_scoped(&r, SRC);
        let labels: Vec<String> = t.order.iter().map(|i| t.items[i].label.clone()).collect();
        assert_eq!(
            labels,
            [
                "Document",
                "Transform T",
                "Shape",
                "Box",
                "USE T",
                "USE Nope",
                "ROUTE X.y → Z.w",
                "PROTO P",
                "Sphere"
            ]
        );
        let uses: Vec<_> = t
            .order
            .iter()
            .filter_map(|i| t.items[i].use_status)
            .collect();
        // `USE T` sits inside `DEF T`: a self-reference (ISO 4.4.4), not a
        // valid edit target. `USE Nope` names no DEF in scope.
        assert_eq!(uses, ["invalid", "unresolved"]);
        assert_eq!(t.resolution_scope, "lexical");
    }

    #[test]
    fn ids_use_utf16_offsets_and_inspector_shows_exact_source() {
        let src = "#VRML V2.0 utf8\n# é😀\nTransform { translation 1  2\r\n3 }";
        let r = parse(src);
        let t = build_scene_tree(&r.tree, src);
        let node = &t.items[&t.order[1]];
        let start16 = src[..src.find("Transform").unwrap()].encode_utf16().count() as u64;
        assert_eq!(node.from, start16);
        assert!(node.id.starts_with(&format!("node-{start16}-")));
        let insp = inspect(&r.tree, src, &node.id).unwrap();
        assert_eq!(insp.rows[0].name, "translation");
        assert_eq!(insp.rows[0].kind, "numbers");
        assert_eq!(insp.rows[0].source, "1  2\r\n3");
    }

    #[test]
    fn unknown_id_inspects_to_none() {
        let r = parse(SRC);
        assert!(inspect(&r.tree, SRC, "node-0-1").is_none());
    }

    #[test]
    fn use_reference_status_from_scoped_tree() {
        let src = "#VRML V2.0 utf8\nDEF A Group {}\nUSE A\nPROTO P [ ] { DEF B Group {} }\nUSE B\n";
        let r = parse(src);
        let (t, _) = build_scene_tree_scoped(&r, src);

        // Find USE A
        let use_a_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "USE A")
            .unwrap();
        let use_a = &t.items[use_a_id];
        assert_eq!(use_a.use_status, Some("resolved"));
        assert_eq!(use_a.refs.len(), 1);
        let ref_a = &use_a.refs[0];
        assert_eq!(ref_a.role, "use");

        let def_a_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "Group A")
            .unwrap();
        assert_eq!(ref_a.target.as_ref(), Some(def_a_id));

        // Find USE B
        let use_b_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "USE B")
            .unwrap();
        let use_b = &t.items[use_b_id];
        assert_eq!(use_b.use_status, Some("unresolved"));
        assert_eq!(use_b.refs.len(), 1);
        let ref_b = &use_b.refs[0];
        assert_eq!(ref_b.role, "use");
        assert_eq!(ref_b.target, None);
    }

    #[test]
    fn route_reference_status_from_scoped_tree() {
        let src = "#VRML V2.0 utf8\nDEF S Group { }\nDEF D Group { }\nROUTE S.a TO D.b\nROUTE S.a TO M.b\n";
        let r = parse(src);
        let (t, _) = build_scene_tree_scoped(&r, src);

        let s_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "Group S")
            .unwrap();
        let d_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "Group D")
            .unwrap();

        let route1_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "ROUTE S.a → D.b")
            .unwrap();
        let route1 = &t.items[route1_id];
        assert_eq!(route1.refs.len(), 2);

        let ref_s1 = route1
            .refs
            .iter()
            .find(|r| r.role == "route-source")
            .unwrap();
        assert_eq!(ref_s1.status, "resolved");
        assert_eq!(ref_s1.target.as_ref(), Some(s_id));

        let ref_d1 = route1
            .refs
            .iter()
            .find(|r| r.role == "route-destination")
            .unwrap();
        assert_eq!(ref_d1.status, "resolved");
        assert_eq!(ref_d1.target.as_ref(), Some(d_id));

        let route2_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "ROUTE S.a → M.b")
            .unwrap();
        let route2 = &t.items[route2_id];
        assert_eq!(route2.refs.len(), 2);

        let ref_s2 = route2
            .refs
            .iter()
            .find(|r| r.role == "route-source")
            .unwrap();
        assert_eq!(ref_s2.status, "resolved");
        assert_eq!(ref_s2.target.as_ref(), Some(s_id));

        let ref_m2 = route2
            .refs
            .iter()
            .find(|r| r.role == "route-destination")
            .unwrap();
        assert_eq!(ref_m2.status, "unresolved");
        assert_eq!(ref_m2.target, None);
    }

    #[test]
    fn inspect_shows_exact_source_for_use_route_and_externproto() {
        let src = "#VRML V2.0 utf8\n# \u{1F600}\r\nEXTERNPROTO E [ ] [ \"a.wrl\"\r\n\"b.wrl\" ]\r\nUSE  T\r\nROUTE  X.y  TO  Z.w\r\n";
        let r = parse(src);
        let t = build_scene_tree(&r.tree, src);

        let use_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "USE T")
            .unwrap();
        let insp_use = inspect(&r.tree, src, use_id).unwrap();
        let use_row = insp_use.rows.iter().find(|r| r.name == "USE").unwrap();
        assert_eq!(use_row.source, "USE  T");

        let route_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "ROUTE X.y → Z.w")
            .unwrap();
        let insp_route = inspect(&r.tree, src, route_id).unwrap();
        let from_row = insp_route.rows.iter().find(|r| r.name == "from").unwrap();
        assert_eq!(from_row.source, "X.y");
        let to_row = insp_route.rows.iter().find(|r| r.name == "to").unwrap();
        assert_eq!(to_row.source, "Z.w");

        let ext_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "EXTERNPROTO E")
            .unwrap();
        let insp_ext = inspect(&r.tree, src, ext_id).unwrap();
        let url_row = insp_ext.rows.iter().find(|r| r.name == "url").unwrap();
        assert_eq!(url_row.source, "[ \"a.wrl\"\r\n\"b.wrl\" ]");
    }

    #[test]
    fn item_ids_use_utf16_offsets_in_proto_and_bom() {
        let src = "\u{feff}#VRML V2.0 utf8\nPROTO P [ ] { # \u{1f600}\n Group { } }\n";
        let r = parse(src);
        assert!(r.diagnostics.is_empty());
        let t = build_scene_tree(&r.tree, src);

        let group_id = t
            .order
            .iter()
            .find(|i| t.items[*i].label == "Group")
            .unwrap();
        let node = &t.items[group_id];

        let start16 = src[..src.find("Group").unwrap()].encode_utf16().count() as u64;
        let end16 = src[..src.find("Group").unwrap() + "Group { }".len()]
            .encode_utf16()
            .count() as u64;
        assert_eq!(node.from, start16);
        assert_eq!(node.id, format!("node-{start16}-{end16}"));
    }
}
