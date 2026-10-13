// SPDX-License-Identifier: GPL-3.0-or-later
//! VRML97 structural parser. Direct Rust translation of `src/vrml/parser.js`.
//!
//! Token-driven recursive descent. Recovery-oriented (one malformed construct
//! yields one diagnostic and a still-usable partial tree), bounded (depth and
//! node-count limits), and every loop is guaranteed to make progress. The
//! Cybertown/Blaxxun leniency for ROUTE/PROTO/EXTERNPROTO inside an MFNode
//! array is preserved exactly as the JS parser accepts it.

use crate::ast::*;
use crate::diagnostics::{Code, Diagnostic};
use crate::tokenizer::{tokenize, Comment, Range, TokKind, Token};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_depth: usize,
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_depth: 256,
            max_nodes: 100_000,
        }
    }
}

pub struct ParseResult {
    pub tree: Document,
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub diagnostics: Vec<Diagnostic>,
    pub truncated: bool,
    pub depth_capped: bool,
}

struct Abort;

struct Parser<'a> {
    src: &'a str,
    toks: &'a [Token],
    pos: usize,
    diags: Vec<Diagnostic>,
    limits: Limits,
    node_budget: isize,
    nodes_capped: bool,
    depth_capped: bool,
}

const ACCESS: &[&str] = &["field", "eventIn", "eventOut", "exposedField"];

impl<'a> Parser<'a> {
    fn peek(&self) -> &'a Token {
        let i = self.pos.min(self.toks.len() - 1);
        &self.toks[i]
    }
    fn next(&mut self) -> &'a Token {
        let t = self.peek();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokKind::Eof)
    }
    fn lex(&self, t: &Token) -> &'a str {
        t.range.slice(self.src)
    }
    fn at_keyword(&self, kw: &str) -> bool {
        let t = self.peek();
        matches!(t.kind, TokKind::Keyword) && self.lex(t) == kw
    }
    fn at_id(&self) -> bool {
        matches!(self.peek().kind, TokKind::Id)
    }
    fn span_to(&self, start: Range) -> Range {
        let prev = &self.toks[self.pos.saturating_sub(1)];
        Range::merge(start, prev.range)
    }
    fn describe(&self, t: &Token) -> String {
        match &t.kind {
            TokKind::Eof => "end of input".into(),
            TokKind::Id => format!("identifier '{}'", self.lex(t)),
            TokKind::Keyword => format!("'{}'", self.lex(t)),
            TokKind::Str { .. } => "string".into(),
            TokKind::Number { .. } => format!("number '{}'", self.lex(t)),
            _ => format!("'{}'", self.lex(t)),
        }
    }
    fn err(&mut self, code: Code, msg: String, range: Range) {
        self.diags.push(Diagnostic::error(code, msg, range));
    }
    fn expect(&mut self, pred: fn(&TokKind) -> bool, what: &str) -> Option<&'a Token> {
        if pred(&self.peek().kind) {
            return Some(self.next());
        }
        let t = self.peek();
        let m = format!("Expected {what} but found {}", self.describe(t));
        self.err(Code::ExpectedToken, m, t.range);
        None
    }
    fn expect_ident(&mut self, message: &str) -> Option<&'a Token> {
        if self.at_id() {
            return Some(self.next());
        }
        let r = self.peek().range;
        self.err(Code::ExpectedIdentifier, message.to_string(), r);
        None
    }
    fn name_of(&self, t: Option<&Token>) -> (Option<String>, Option<Range>) {
        match t {
            Some(t) => (Some(self.lex(t).to_string()), Some(t.range)),
            None => (None, None),
        }
    }

    fn parse_document(&mut self) -> Result<Document, Abort> {
        let mut header = None;
        if let TokKind::Header(h) = &self.peek().kind {
            let (version, encoding) = (&h.version, &h.encoding);
            let t = self.next();
            if version.as_deref() != Some("V2.0") || encoding.as_deref() != Some("utf8") {
                let m = format!(
                    "Non-canonical VRML header '{}' (expected '#VRML V2.0 utf8')",
                    self.lex(t).trim()
                );
                self.diags
                    .push(Diagnostic::warning(Code::InvalidHeader, m, t.range));
            }
            header = Some(Header {
                version: version.clone(),
                encoding: encoding.clone(),
                range: t.range,
            });
        } else {
            let r = self.peek().range;
            self.err(
                Code::MissingHeader,
                "Missing '#VRML V2.0 utf8' header".into(),
                r,
            );
        }
        let mut statements = Vec::new();
        while !self.at_eof() {
            let before = self.pos;
            if let Some(s) = self.parse_top_statement(0)? {
                statements.push(s);
            }
            if self.pos == before {
                self.next();
            }
        }
        let start = match (&header, statements.first()) {
            (Some(h), _) => h.range.start,
            (None, Some(s)) => s.range().start,
            _ => self.peek().range.start,
        };
        Ok(Document {
            header,
            statements,
            range: Range {
                start,
                end: self.peek().range.end,
            },
        })
    }

    fn parse_top_statement(&mut self, depth: usize) -> Result<Option<Ast>, Abort> {
        let t = self.peek();
        if matches!(t.kind, TokKind::Keyword) {
            match self.lex(t) {
                "ROUTE" => return Ok(Some(self.parse_route())),
                "PROTO" => return self.parse_proto(depth).map(Some),
                "EXTERNPROTO" => return self.parse_extern_proto(depth).map(Some),
                "DEF" => return Ok(self.parse_node_statement(depth)?.map(Ast::Node)),
                "USE" => return Ok(Some(self.parse_use())),
                _ => {}
            }
        }
        if matches!(t.kind, TokKind::Id) {
            return Ok(self.parse_node(depth)?.map(Ast::Node));
        }
        let m = format!("Unexpected {} at top level", self.describe(t));
        self.err(Code::UnexpectedToken, m, t.range);
        self.next();
        Ok(None)
    }

    fn parse_node_statement(&mut self, depth: usize) -> Result<Option<Node>, Abort> {
        if self.at_keyword("DEF") {
            let def_tok = self.next();
            let name_tok = self.expect_ident("Expected a name after DEF");
            let node = self.parse_node(depth)?;
            return Ok(node.map(|mut n| {
                let (name, range) = self.name_of(name_tok);
                n.def = name;
                n.def_range = range;
                n.range = Range::merge(def_tok.range, n.range);
                n
            }));
        }
        self.parse_node(depth)
    }

    fn parse_node(&mut self, depth: usize) -> Result<Option<Node>, Abort> {
        self.node_budget -= 1;
        if self.node_budget < 0 {
            if !self.nodes_capped {
                self.nodes_capped = true;
                let r = self.peek().range;
                let m = format!(
                    "Node limit ({}) exceeded; parsing stopped",
                    self.limits.max_nodes
                );
                self.err(Code::MaxNodes, m, r);
            }
            return Err(Abort);
        }
        let Some(type_tok) = self.expect(|k| matches!(k, TokKind::Id), "a node type name") else {
            return Ok(None);
        };
        let node_type = self.lex(type_tok).to_string();

        if depth > self.limits.max_depth {
            if !self.depth_capped {
                self.depth_capped = true;
                let m = format!("Maximum nesting depth ({}) exceeded", self.limits.max_depth);
                self.err(Code::MaxDepth, m, type_tok.range);
            }
            self.skip_braced_block();
            return Ok(Some(Node {
                node_type,
                type_range: type_tok.range,
                def: None,
                def_range: None,
                fields: vec![],
                interfaces: vec![],
                incomplete: true,
                range: self.span_to(type_tok.range),
            }));
        }

        let mut node = Node {
            node_type,
            type_range: type_tok.range,
            def: None,
            def_range: None,
            fields: vec![],
            interfaces: vec![],
            incomplete: false,
            range: type_tok.range,
        };
        if !matches!(self.peek().kind, TokKind::LBrace) {
            let t = self.peek();
            let m = format!(
                "Expected '{{' to open {} body but found {}",
                node.node_type,
                self.describe(t)
            );
            self.err(Code::ExpectedToken, m, t.range);
            node.incomplete = true;
            return Ok(Some(node));
        }
        self.next();
        while !matches!(self.peek().kind, TokKind::RBrace) && !self.at_eof() {
            let before = self.pos;
            match self.parse_node_body_element(depth)? {
                Some(BodyEl::Interface(i)) => node.interfaces.push(i),
                Some(BodyEl::Stmt(s)) => node.fields.push(s),
                None => {}
            }
            if self.pos == before {
                let t = self.peek();
                let m = format!("Unexpected {} in {} body", self.describe(t), node.node_type);
                self.err(Code::UnexpectedToken, m, t.range);
                self.sync_in_body();
            }
        }
        if matches!(self.peek().kind, TokKind::RBrace) {
            self.next();
        } else {
            let m = format!("Unclosed '{{' for {}", node.node_type);
            self.err(Code::UnclosedBrace, m, type_tok.range);
        }
        node.range = self.span_to(type_tok.range);
        Ok(Some(node))
    }

    fn parse_node_body_element(&mut self, depth: usize) -> Result<Option<BodyEl>, Abort> {
        let t = self.peek();
        if matches!(t.kind, TokKind::Keyword) {
            match self.lex(t) {
                "ROUTE" => return Ok(Some(BodyEl::Stmt(self.parse_route()))),
                "PROTO" => return Ok(Some(BodyEl::Stmt(self.parse_proto(depth)?))),
                "EXTERNPROTO" => return Ok(Some(BodyEl::Stmt(self.parse_extern_proto(depth)?))),
                kw if ACCESS.contains(&kw) => {
                    return Ok(self.parse_interface_decl(false)?.map(BodyEl::Interface))
                }
                _ => {}
            }
        }
        if matches!(t.kind, TokKind::Id) {
            return Ok(Some(BodyEl::Stmt(Ast::Field(self.parse_field(depth)?))));
        }
        Ok(None)
    }

    fn parse_field(&mut self, depth: usize) -> Result<Field, Abort> {
        let name_tok = self.next();
        let name = self.lex(name_tok).to_string();
        if self.at_keyword("IS") {
            self.next();
            let id = self.expect_ident("Expected an interface name after IS");
            let (n, r) = self.name_of(id);
            let range = self.span_to(name_tok.range);
            return Ok(Field {
                name,
                name_range: name_tok.range,
                value: Some(Box::new(Ast::Is {
                    name: n,
                    name_range: r,
                    range,
                })),
                is_binding: true,
                range,
            });
        }
        let value = self.parse_value(depth)?;
        if value.is_none() {
            let t = self.peek();
            let m = format!(
                "Expected a value for field '{name}' but found {}",
                self.describe(t)
            );
            self.err(Code::ExpectedFieldValue, m, t.range);
        }
        Ok(Field {
            name,
            name_range: name_tok.range,
            value: value.map(Box::new),
            is_binding: false,
            range: self.span_to(name_tok.range),
        })
    }

    fn parse_value(&mut self, depth: usize) -> Result<Option<Ast>, Abort> {
        let t = self.peek();
        Ok(match &t.kind {
            TokKind::LBracket => Some(self.parse_array(depth)?),
            TokKind::Str { value, .. } => {
                self.next();
                Some(Ast::Str {
                    value: value.clone(),
                    range: t.range,
                })
            }
            TokKind::Bool(b) => {
                self.next();
                Some(Ast::Bool {
                    value: *b,
                    range: t.range,
                })
            }
            TokKind::Number { .. } => Some(self.parse_number_run()),
            TokKind::Id => self.parse_node(depth + 1)?.map(Ast::Node),
            TokKind::Keyword => match self.lex(t) {
                "NULL" => {
                    self.next();
                    Some(Ast::Null { range: t.range })
                }
                "USE" => Some(self.parse_use()),
                "DEF" => self.parse_node_statement(depth + 1)?.map(Ast::Node),
                _ => None,
            },
            _ => None,
        })
    }

    fn num(t: &Token) -> Num {
        match t.kind {
            TokKind::Number {
                value,
                numeric,
                valid,
            } => Num {
                value,
                numeric,
                valid,
                range: t.range,
            },
            _ => unreachable!("num() on a non-number token"),
        }
    }

    fn parse_number_run(&mut self) -> Ast {
        let first = self.peek().range;
        let mut values = Vec::new();
        while matches!(self.peek().kind, TokKind::Number { .. }) {
            let t = self.next();
            values.push(Self::num(t));
        }
        let last = values.last().map(|v| v.range).unwrap_or(first);
        Ast::Numbers {
            values,
            range: Range::merge(first, last),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<Ast, Abort> {
        let open = self.next();
        let mut items = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBracket) && !self.at_eof() {
            let before = self.pos;
            let t = self.peek();
            match &t.kind {
                TokKind::Number { .. } => {
                    self.next();
                    items.push(Ast::Number(Self::num(t)));
                }
                TokKind::Str { value, .. } => {
                    self.next();
                    items.push(Ast::Str {
                        value: value.clone(),
                        range: t.range,
                    });
                }
                TokKind::Bool(b) => {
                    self.next();
                    items.push(Ast::Bool {
                        value: *b,
                        range: t.range,
                    });
                }
                TokKind::Id => {
                    if let Some(n) = self.parse_node(depth + 1)? {
                        items.push(Ast::Node(n));
                    }
                }
                TokKind::LBracket => items.push(self.parse_array(depth + 1)?),
                TokKind::Keyword => match self.lex(t) {
                    "NULL" => {
                        self.next();
                        items.push(Ast::Null { range: t.range });
                    }
                    "USE" => items.push(self.parse_use()),
                    "DEF" => {
                        if let Some(n) = self.parse_node_statement(depth + 1)? {
                            items.push(Ast::Node(n));
                        }
                    }
                    // Lenient Cybertown/Blaxxun compatibility (see parser.js).
                    "ROUTE" => items.push(self.parse_route()),
                    "PROTO" => items.push(self.parse_proto(depth + 1)?),
                    "EXTERNPROTO" => items.push(self.parse_extern_proto(depth + 1)?),
                    _ => {
                        let m = format!("Unexpected {} in array", self.describe(t));
                        self.err(Code::UnexpectedToken, m, t.range);
                        self.next();
                    }
                },
                _ => {
                    let m = format!("Unexpected {} in array", self.describe(t));
                    self.err(Code::UnexpectedToken, m, t.range);
                    self.next();
                }
            }
            if self.pos == before {
                self.next();
            }
        }
        if matches!(self.peek().kind, TokKind::RBracket) {
            self.next();
        } else {
            self.err(Code::UnclosedBracket, "Unclosed '['".into(), open.range);
        }
        Ok(Ast::Array {
            items,
            range: self.span_to(open.range),
        })
    }

    fn parse_use(&mut self) -> Ast {
        let kw = self.next();
        let name_tok = self.expect_ident("Expected a name after USE");
        let (name, name_range) = self.name_of(name_tok);
        Ast::Use {
            name,
            name_range,
            range: self.span_to(kw.range),
        }
    }

    fn parse_route(&mut self) -> Ast {
        let kw = self.next();
        let from = self.parse_route_endpoint();
        if self.at_keyword("TO") {
            self.next();
        } else {
            let t = self.peek();
            let m = format!("Expected 'TO' in ROUTE but found {}", self.describe(t));
            self.err(Code::ExpectedToken, m, t.range);
        }
        let to = self.parse_route_endpoint();
        Ast::Route(Route {
            from,
            to,
            range: self.span_to(kw.range),
        })
    }

    fn parse_route_endpoint(&mut self) -> RouteEnd {
        let Some(node_tok) = self.expect_ident("Expected a node name in ROUTE endpoint") else {
            return RouteEnd {
                node: None,
                node_range: None,
                event: None,
                event_range: None,
                range: self.peek().range,
            };
        };
        let mut event = None;
        let mut event_range = None;
        if matches!(self.peek().kind, TokKind::Period) {
            self.next();
            if let Some(ev) = self.expect_ident("Expected an event name after \".\"") {
                event = Some(self.lex(ev).to_string());
                event_range = Some(ev.range);
            }
        } else {
            let r = self.peek().range;
            self.err(
                Code::ExpectedToken,
                "Expected '.' in ROUTE endpoint".into(),
                r,
            );
        }
        RouteEnd {
            node: Some(self.lex(node_tok).to_string()),
            node_range: Some(node_tok.range),
            event,
            event_range,
            range: Range::merge(node_tok.range, event_range.unwrap_or(node_tok.range)),
        }
    }

    fn parse_proto(&mut self, depth: usize) -> Result<Ast, Abort> {
        let kw = self.next();
        let name_tok = self.expect(|k| matches!(k, TokKind::Id), "a PROTO name");
        let interfaces = self.parse_interface_list(false)?;
        let mut body = Vec::new();
        if self
            .expect(|k| matches!(k, TokKind::LBrace), "'{' to open PROTO body")
            .is_some()
        {
            while !matches!(self.peek().kind, TokKind::RBrace) && !self.at_eof() {
                let before = self.pos;
                if let Some(s) = self.parse_top_statement(depth + 1)? {
                    body.push(s);
                }
                if self.pos == before {
                    self.next();
                }
            }
            if matches!(self.peek().kind, TokKind::RBrace) {
                self.next();
            } else {
                self.err(
                    Code::UnclosedBrace,
                    "Unclosed '{' for PROTO body".into(),
                    kw.range,
                );
            }
        }
        let (name, name_range) = self.name_of(name_tok);
        Ok(Ast::Proto(Proto {
            name,
            name_range,
            interfaces,
            body,
            range: self.span_to(kw.range),
        }))
    }

    fn parse_extern_proto(&mut self, depth: usize) -> Result<Ast, Abort> {
        let kw = self.next();
        let name_tok = self.expect(|k| matches!(k, TokKind::Id), "an EXTERNPROTO name");
        let interfaces = self.parse_interface_list(true)?;
        let mut url = None;
        if matches!(self.peek().kind, TokKind::LBracket | TokKind::Str { .. }) {
            url = self.parse_value(depth)?.map(Box::new);
        }
        let (name, name_range) = self.name_of(name_tok);
        Ok(Ast::ExternProto(ExternProto {
            name,
            name_range,
            interfaces,
            url,
            range: self.span_to(kw.range),
        }))
    }

    fn parse_interface_list(&mut self, extern_no_defaults: bool) -> Result<Vec<Interface>, Abort> {
        let mut decls = Vec::new();
        if !matches!(self.peek().kind, TokKind::LBracket) {
            let t = self.peek();
            let m = format!(
                "Expected '[' to open interface declarations but found {}",
                self.describe(t)
            );
            self.err(Code::ExpectedToken, m, t.range);
            return Ok(decls);
        }
        let open = self.next();
        while !matches!(self.peek().kind, TokKind::RBracket) && !self.at_eof() {
            let before = self.pos;
            if let Some(d) = self.parse_interface_decl(extern_no_defaults)? {
                decls.push(d);
            }
            if self.pos == before {
                let t = self.peek();
                let m = format!(
                    "Expected an interface declaration but found {}",
                    self.describe(t)
                );
                self.err(Code::ExpectedInterface, m, t.range);
                self.next();
            }
        }
        if matches!(self.peek().kind, TokKind::RBracket) {
            self.next();
        } else {
            self.err(
                Code::UnclosedBracket,
                "Unclosed '[' in interface declarations".into(),
                open.range,
            );
        }
        Ok(decls)
    }

    fn parse_interface_decl(
        &mut self,
        extern_no_defaults: bool,
    ) -> Result<Option<Interface>, Abort> {
        let t = self.peek();
        if !(matches!(t.kind, TokKind::Keyword) && ACCESS.contains(&self.lex(t))) {
            return Ok(None);
        }
        let access_tok = self.next();
        let access = self.lex(access_tok).to_string();
        let type_tok = self.expect(|k| matches!(k, TokKind::Id), "a field type (e.g. SFFloat)");
        let name_tok = self.expect(|k| matches!(k, TokKind::Id), "an interface field name");
        let mut default = None;
        let mut is = None;
        let mut is_range = None;
        if self.at_keyword("IS") {
            self.next();
            if let Some(id) = self.expect_ident("Expected an interface name after IS") {
                is = Some(self.lex(id).to_string());
                is_range = Some(id.range);
            }
        } else if (access == "field" || access == "exposedField") && !extern_no_defaults {
            default = self.parse_value(1)?.map(Box::new);
        }
        let (field_type, field_type_range) = self.name_of(type_tok);
        let (name, name_range) = self.name_of(name_tok);
        Ok(Some(Interface {
            access,
            field_type,
            field_type_range,
            name,
            name_range,
            default,
            is,
            is_range,
            range: self.span_to(t.range),
        }))
    }

    fn skip_braced_block(&mut self) {
        if !matches!(self.peek().kind, TokKind::LBrace) {
            return;
        }
        let mut depth = 0i64;
        while !self.at_eof() {
            let t = self.next();
            match t.kind {
                TokKind::LBrace => depth += 1,
                TokKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn sync_in_body(&mut self) {
        while !self.at_eof() {
            let t = self.peek();
            match t.kind {
                TokKind::RBrace | TokKind::Id => return,
                TokKind::Keyword
                    if matches!(self.lex(t), "ROUTE" | "PROTO" | "EXTERNPROTO")
                        || ACCESS.contains(&self.lex(t)) =>
                {
                    return
                }
                _ => {
                    self.next();
                }
            }
        }
    }
}

enum BodyEl {
    Interface(Interface),
    Stmt(Ast),
}

pub fn parse(text: &str) -> ParseResult {
    parse_with(text, Limits::default())
}

pub fn parse_with(text: &str, limits: Limits) -> ParseResult {
    if text.len() >= crate::tokenizer::MAX_TEXT_BYTES {
        let r = Range::default();
        return ParseResult {
            tree: Document {
                header: None,
                statements: vec![],
                range: r,
            },
            tokens: vec![],
            comments: vec![],
            diagnostics: vec![Diagnostic::error(
                Code::MaxNodes,
                "Document is too large to analyze (4 GiB limit)".into(),
                r,
            )],
            truncated: true,
            depth_capped: false,
        };
    }
    let lexed = tokenize(text);
    let tokens = lexed.tokens;
    let (tree, diags, truncated, depth_capped) = {
        let mut p = Parser {
            src: text,
            toks: &tokens,
            pos: 0,
            diags: lexed.diagnostics,
            limits,
            node_budget: limits.max_nodes as isize,
            nodes_capped: false,
            depth_capped: false,
        };
        let tree = match p.parse_document() {
            Ok(t) => t,
            Err(Abort) => Document {
                header: None,
                statements: vec![],
                range: Range {
                    start: tokens[0].range.start,
                    end: p.peek().range.end,
                },
            },
        };
        (tree, p.diags, p.nodes_capped, p.depth_capped)
    };
    ParseResult {
        tree,
        tokens,
        comments: lexed.comments,
        diagnostics: diags,
        truncated,
        depth_capped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(src: &str) -> Vec<&'static str> {
        parse(src)
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect()
    }

    #[test]
    fn parses_a_clean_document() {
        let src = "#VRML V2.0 utf8\nDEF T Transform { translation 1 2 3 children [ Shape { geometry Box {} } USE T ] }\nROUTE A.x TO B.y";
        let r = parse(src);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        assert_eq!(r.tree.statements.len(), 2);
        let Ast::Node(t) = &r.tree.statements[0] else {
            panic!()
        };
        assert_eq!(t.def.as_deref(), Some("T"));
        assert_eq!(t.node_type, "Transform");
        assert_eq!(t.range.slice(src), &src[16..src.find("\nROUTE").unwrap()]);
        assert_eq!(t.fields.len(), 2);
    }

    #[test]
    fn header_warnings_and_missing_header() {
        assert_eq!(codes("#VRML V2.0 UTF8\nGroup {}"), ["VRML002"]);
        assert_eq!(codes("Group {}"), ["VRML001"]);
    }

    #[test]
    fn recovers_from_unclosed_brace_and_bad_field() {
        let r = parse("#VRML V2.0 utf8\nGroup { children [ Shape { } ] ");
        let c: Vec<_> = r.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(c, ["VRML023"]);
        assert_eq!(r.tree.statements.len(), 1);
    }

    #[test]
    fn accepts_cybertown_route_in_mfnode_array() {
        let r = parse(
            "#VRML V2.0 utf8\nGroup { children [ ROUTE A.b TO C.d PROTO P [] { Group {} } ] }",
        );
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    }

    #[test]
    fn proto_with_interfaces_and_is() {
        let src = "#VRML V2.0 utf8\nPROTO Ball [ field SFFloat r 1 eventIn SFBool go ] { Sphere { radius IS r } }\nBall { r 2 }";
        let r = parse(src);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        let Ast::Proto(p) = &r.tree.statements[0] else {
            panic!()
        };
        assert_eq!(p.interfaces.len(), 2);
        assert_eq!(p.body.len(), 1);
    }

    #[test]
    fn node_limit_aborts_with_partial_shell() {
        let r = parse_with(
            "#VRML V2.0 utf8\nGroup {} Group {} Group {}",
            Limits {
                max_depth: 256,
                max_nodes: 2,
            },
        );
        assert!(r.truncated);
        assert!(r.diagnostics.iter().any(|d| d.code.as_str() == "VRML032"));
    }

    #[test]
    fn garbage_always_terminates() {
        let r = parse("}}]]{{[[ DEF USE ROUTE . . 1e PROTO [ field ");
        assert!(!r.diagnostics.is_empty());
    }
}
