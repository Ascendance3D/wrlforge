// SPDX-License-Identifier: GPL-3.0-or-later
//! Scene-tree and inspector READ projections. Partial translation of
//! `src/vrml/scene-tree.js` (item selection rules and id format) plus a
//! read-only field listing for the inspector.
//!
//! Both are derived from one parse of the canonical text and are disposable.
//! Ids are `<kind>-<startUtf16>-<endUtf16>`, exactly the JS format: session
//! stable for one text, never persisted, never authoritative node identity.
//!
//! USE resolution is the FLAT, non-authoritative scope the JS scene tree also
//! labels as such (`resolution_scope = "flat"`). The WD1.5 scope graph has not
//! been translated; nothing here claims its answers.

use std::collections::HashMap;

use crate::ast::*;
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
    /// For a USE: "resolved" | "unresolved" (flat scope, non-authoritative).
    pub use_status: Option<&'static str>,
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
    defs: HashMap<String, usize>,
}

fn collect_defs(ast: &Ast, defs: &mut HashMap<String, usize>) {
    match ast {
        Ast::Node(n) => {
            if let Some(d) = &n.def {
                *defs.entry(d.clone()).or_default() += 1;
            }
            for f in &n.fields {
                collect_defs(f, defs);
            }
        }
        Ast::Field(f) => {
            if let Some(v) = &f.value {
                collect_defs(v, defs);
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| collect_defs(i, defs)),
        Ast::Proto(p) => p.body.iter().for_each(|b| collect_defs(b, defs)),
        _ => {}
    }
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
                let status = match name {
                    Some(nm) if self.defs.contains_key(nm) => "resolved",
                    _ => "unresolved",
                };
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
                        use_status: Some(status),
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
    let mut defs = HashMap::new();
    for s in &doc.statements {
        collect_defs(s, &mut defs);
    }
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
    };
    let root_id = root.id.clone();
    let mut ctx = Ctx {
        order: vec![root_id.clone()],
        items: HashMap::from([(root_id.clone(), root)]),
        defs,
    };
    for s in &doc.statements {
        ctx.emit(s, &root_id, 1, src);
    }
    SceneTree {
        root: root_id,
        order: ctx.order,
        items: ctx.items,
        resolution_scope: "flat",
    }
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
        let t = build_scene_tree(&r.tree, SRC);
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
        assert_eq!(uses, ["resolved", "unresolved"]);
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
}
