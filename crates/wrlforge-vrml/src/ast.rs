// SPDX-License-Identifier: GPL-3.0-or-later
//! Partial syntax tree. Mirrors the shapes built by `src/vrml/ast.js` and
//! `src/vrml/parser.js`: every node carries an exact source range, and the tree
//! is a derived projection of the source text -- never printed back to text.

use crate::tokenizer::{Numeric, Range};

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub header: Option<Header>,
    pub statements: Vec<Ast>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Num {
    pub value: f64,
    pub numeric: Numeric,
    pub valid: bool,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub node_type: String,
    pub type_range: Range,
    pub def: Option<String>,
    pub def_range: Option<Range>,
    /// `Field`, `Route`, `Proto` and `ExternProto` body statements, in order.
    pub fields: Vec<Ast>,
    pub interfaces: Vec<Interface>,
    pub incomplete: bool,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub name_range: Range,
    pub value: Option<Box<Ast>>,
    pub is_binding: bool,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub access: String,
    pub field_type: Option<String>,
    pub field_type_range: Option<Range>,
    pub name: Option<String>,
    pub name_range: Option<Range>,
    pub default: Option<Box<Ast>>,
    pub is: Option<String>,
    pub is_range: Option<Range>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteEnd {
    pub node: Option<String>,
    pub node_range: Option<Range>,
    pub event: Option<String>,
    pub event_range: Option<Range>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub from: RouteEnd,
    pub to: RouteEnd,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proto {
    pub name: Option<String>,
    pub name_range: Option<Range>,
    pub interfaces: Vec<Interface>,
    pub body: Vec<Ast>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternProto {
    pub name: Option<String>,
    pub name_range: Option<Range>,
    pub interfaces: Vec<Interface>,
    pub url: Option<Box<Ast>>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Node(Node),
    Use {
        name: Option<String>,
        name_range: Option<Range>,
        range: Range,
    },
    Route(Route),
    Proto(Proto),
    ExternProto(ExternProto),
    Field(Field),
    Str {
        value: String,
        range: Range,
    },
    Bool {
        value: bool,
        range: Range,
    },
    Null {
        range: Range,
    },
    Number(Num),
    Numbers {
        values: Vec<Num>,
        range: Range,
    },
    Array {
        items: Vec<Ast>,
        range: Range,
    },
    Is {
        name: Option<String>,
        name_range: Option<Range>,
        range: Range,
    },
}

impl Ast {
    pub fn range(&self) -> Range {
        match self {
            Ast::Node(n) => n.range,
            Ast::Use { range, .. }
            | Ast::Str { range, .. }
            | Ast::Bool { range, .. }
            | Ast::Null { range }
            | Ast::Numbers { range, .. }
            | Ast::Array { range, .. }
            | Ast::Is { range, .. } => *range,
            Ast::Number(n) => n.range,
            Ast::Route(r) => r.range,
            Ast::Proto(p) => p.range,
            Ast::ExternProto(e) => e.range,
            Ast::Field(f) => f.range,
        }
    }

    /// A stable lowercase name for the value kind (inspector display).
    pub fn kind_name(&self) -> &'static str {
        match self {
            Ast::Node(_) => "node",
            Ast::Use { .. } => "use",
            Ast::Route(_) => "route",
            Ast::Proto(_) => "proto",
            Ast::ExternProto(_) => "externproto",
            Ast::Field(_) => "field",
            Ast::Str { .. } => "string",
            Ast::Bool { .. } => "bool",
            Ast::Null { .. } => "null",
            Ast::Number(_) => "number",
            Ast::Numbers { .. } => "numbers",
            Ast::Array { .. } => "array",
            Ast::Is { .. } => "is",
        }
    }
}
