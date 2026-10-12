// SPDX-License-Identifier: GPL-3.0-or-later
//! Syntax-highlight spans (UI-SYNTAX-1), derived from ONE parse result.
//!
//! Rust port of the classification in `src/editor/language.js`: lexical
//! classes come from the tokenizer, and identifier ROLES (node type, field,
//! DEF name, USE / ROUTE reference) come from the exact AST ranges the parser
//! recorded. There is no second grammar here and no regular expression: a
//! token the parse tree does not place keeps no class (normal text).
//!
//! Spans are half-open UTF-16 SOURCE offsets, sorted, and never overlap —
//! tokens and comments are disjoint by construction of the tokenizer.

use std::collections::HashMap;

use crate::ast::{Ast, Interface, Node, RouteEnd};
use crate::parser::ParseResult;
use crate::tokenizer::{Range, TokKind};

/// The presentation role of one source span. Stable numeric codes: the
/// desktop protocol sends `as u8` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Class {
    /// `#VRML V2.0 utf8`.
    Header = 0,
    Comment = 1,
    /// DEF USE PROTO EXTERNPROTO IS eventIn eventOut field exposedField.
    Keyword = 2,
    /// ROUTE and TO.
    Route = 3,
    /// TRUE FALSE NULL.
    Literal = 4,
    Str = 5,
    Number = 6,
    /// A node type in node position, or a PROTO / EXTERNPROTO name.
    NodeType = 7,
    /// A field type in an interface declaration (SFVec3f, MFNode, ...).
    FieldType = 8,
    /// A field name, an interface member name, an IS target, a ROUTE event.
    Field = 9,
    /// The name a DEF declares.
    DefName = 10,
    /// A name in USE or ROUTE node position. Syntactic position only: this
    /// makes NO claim that the name resolves (the scope model is flat).
    DefRef = 11,
    /// `{ } [ ] .`
    Punctuation = 12,
    /// An invalid number or an unterminated string.
    Invalid = 13,
}

impl Class {
    pub const ALL: [Class; 14] = [
        Class::Header,
        Class::Comment,
        Class::Keyword,
        Class::Route,
        Class::Literal,
        Class::Str,
        Class::Number,
        Class::NodeType,
        Class::FieldType,
        Class::Field,
        Class::DefName,
        Class::DefRef,
        Class::Punctuation,
        Class::Invalid,
    ];

    /// CSS class suffix (`tk-<name>`).
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Header => "header",
            Class::Comment => "comment",
            Class::Keyword => "keyword",
            Class::Route => "route",
            Class::Literal => "literal",
            Class::Str => "string",
            Class::Number => "number",
            Class::NodeType => "node-type",
            Class::FieldType => "field-type",
            Class::Field => "field",
            Class::DefName => "def-name",
            Class::DefRef => "def-ref",
            Class::Punctuation => "punctuation",
            Class::Invalid => "invalid",
        }
    }

    pub fn from_code(c: u8) -> Option<Class> {
        Class::ALL.get(c as usize).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// UTF-16 source offsets, half-open.
    pub from: u32,
    pub to: u32,
    pub class: Class,
}

/// Identifier roles keyed by the token's UTF-16 start offset. Applied ONLY to
/// `Id` tokens, so a recovery range that lands on a keyword or brace never
/// recolors it.
struct Roles(HashMap<u32, Class>);

impl Roles {
    fn mark(&mut self, r: Option<Range>, c: Class) {
        if let Some(r) = r {
            self.0.insert(r.start.offset, c);
        }
    }

    fn ast(&mut self, a: &Ast) {
        match a {
            Ast::Node(n) => self.node(n),
            Ast::Use { name_range, .. } => self.mark(*name_range, Class::DefRef),
            Ast::Route(r) => {
                self.route_end(&r.from);
                self.route_end(&r.to);
            }
            Ast::Proto(p) => {
                self.mark(p.name_range, Class::NodeType);
                self.interfaces(&p.interfaces);
                for s in &p.body {
                    self.ast(s);
                }
            }
            Ast::ExternProto(e) => {
                self.mark(e.name_range, Class::NodeType);
                self.interfaces(&e.interfaces);
                if let Some(u) = &e.url {
                    self.ast(u);
                }
            }
            Ast::Field(f) => {
                self.mark(Some(f.name_range), Class::Field);
                if let Some(v) = &f.value {
                    self.ast(v);
                }
            }
            Ast::Array { items, .. } => {
                for i in items {
                    self.ast(i);
                }
            }
            Ast::Is { name_range, .. } => self.mark(*name_range, Class::Field),
            Ast::Str { .. }
            | Ast::Bool { .. }
            | Ast::Null { .. }
            | Ast::Number(_)
            | Ast::Numbers { .. } => {}
        }
    }

    fn node(&mut self, n: &Node) {
        self.mark(Some(n.type_range), Class::NodeType);
        self.mark(n.def_range, Class::DefName);
        self.interfaces(&n.interfaces);
        for f in &n.fields {
            self.ast(f);
        }
    }

    fn interfaces(&mut self, list: &[Interface]) {
        for i in list {
            self.mark(i.field_type_range, Class::FieldType);
            self.mark(i.name_range, Class::Field);
            self.mark(i.is_range, Class::Field);
            if let Some(d) = &i.default {
                self.ast(d);
            }
        }
    }

    fn route_end(&mut self, e: &RouteEnd) {
        self.mark(e.node_range, Class::DefRef);
        self.mark(e.event_range, Class::Field);
    }
}

fn keyword_class(lexeme: &str) -> Class {
    match lexeme {
        "ROUTE" | "TO" => Class::Route,
        "NULL" => Class::Literal,
        _ => Class::Keyword,
    }
}

/// Highlight spans for `src`, from the parse result of exactly that text.
pub fn highlight(parsed: &ParseResult, src: &str) -> Vec<Span> {
    let mut roles = Roles(HashMap::new());
    for s in &parsed.tree.statements {
        roles.ast(s);
    }

    let mut out: Vec<Span> = Vec::with_capacity(parsed.tokens.len() + parsed.comments.len());
    for t in &parsed.tokens {
        let class = match &t.kind {
            TokKind::Eof => continue,
            TokKind::Header(_) => Some(Class::Header),
            TokKind::Keyword => Some(keyword_class(t.lexeme(src))),
            TokKind::Bool(_) => Some(Class::Literal),
            TokKind::Str { terminated, .. } => Some(if *terminated {
                Class::Str
            } else {
                Class::Invalid
            }),
            TokKind::Number { valid, .. } => Some(if *valid {
                Class::Number
            } else {
                Class::Invalid
            }),
            TokKind::LBrace
            | TokKind::RBrace
            | TokKind::LBracket
            | TokKind::RBracket
            | TokKind::Period => Some(Class::Punctuation),
            // Unplaced identifiers stay normal text: no invented meaning.
            TokKind::Id => roles.0.get(&t.range.start.offset).copied(),
        };
        if let Some(class) = class {
            if t.range.end.offset > t.range.start.offset {
                out.push(Span {
                    from: t.range.start.offset,
                    to: t.range.end.offset,
                    class,
                });
            }
        }
    }
    for c in &parsed.comments {
        if c.range.end.offset > c.range.start.offset {
            out.push(Span {
                from: c.range.start.offset,
                to: c.range.end.offset,
                class: Class::Comment,
            });
        }
    }
    out.sort_by_key(|s| s.from);
    out
}

#[cfg(test)]
mod tests;
