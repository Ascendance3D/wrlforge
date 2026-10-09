// SPDX-License-Identifier: GPL-3.0-or-later
//! VRML97 tokenizer. Direct Rust translation of `src/vrml/tokenizer.js`.
//!
//! Char stream -> token stream. Every token carries an exact source span. The
//! span reports BOTH the UTF-16 code-unit offset (the public coordinate system
//! shared with `src/vrml/*.js` and `wrlforge-text`) and the UTF-8 byte offset
//! (so a lexeme slices from the Rust `&str` without a conversion).
//!
//! Line/column are 1-based and count exactly like the JS tokenizer: a CRLF pair
//! or a lone CR advances the line once; every other UTF-16 code unit advances
//! the column by one (so an astral character advances it by two).
//!
//! Trivia (whitespace, commas, comments) is skipped; comments are collected.
//! The JS `leadingTrivia` arrays are not translated: no consumer in this
//! application reads them, and the source text itself stays the authority.

use crate::diagnostics::{Code, Diagnostic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    /// UTF-16 code-unit offset (public coordinate).
    pub offset: u32,
    /// UTF-8 byte offset (internal coordinate).
    pub byte: u32,
    pub line: u32,
    pub column: u32,
}

/// Positions are stored as `u32` (16 bytes per `Pos`): a token stream for a
/// 100 MB generated world otherwise costs gigabytes. `parse` refuses text at
/// or above this size instead of wrapping.
pub const MAX_TEXT_BYTES: usize = u32::MAX as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

impl Range {
    pub fn merge(a: Range, b: Range) -> Range {
        Range {
            start: a.start,
            end: b.end,
        }
    }
    pub fn slice<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start.byte as usize..self.end.byte as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numeric {
    Int,
    Float,
    Hex,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    /// Boxed: one header per file must not widen every token.
    Header(Box<HeaderWords>),
    Id,
    Keyword,
    Bool(bool),
    Str {
        value: String,
        terminated: bool,
    },
    Number {
        value: f64,
        numeric: Numeric,
        valid: bool,
    },
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Period,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderWords {
    pub version: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub range: Range,
}

impl Token {
    pub fn lexeme<'a>(&self, src: &'a str) -> &'a str {
        self.range.slice(src)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub range: Range,
}

pub const KEYWORDS: &[&str] = &[
    "DEF",
    "USE",
    "PROTO",
    "EXTERNPROTO",
    "IS",
    "ROUTE",
    "TO",
    "NULL",
    "eventIn",
    "eventOut",
    "exposedField",
    "field",
];

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0C}' | '\u{0B}')
}
fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}
fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}
fn is_control(c: char) -> bool {
    let code = c as u32;
    code <= 0x20 || code == 0x7f
}
fn is_id_delim(c: char) -> bool {
    matches!(
        c,
        '{' | '}' | '[' | ']' | '"' | '#' | ',' | '.' | '\\' | '\''
    )
}
fn is_id_start(c: Option<char>) -> bool {
    match c {
        Some(c) => !is_control(c) && !is_id_delim(c) && !is_digit(c) && c != '+' && c != '-',
        None => false,
    }
}
fn is_id_part(c: Option<char>) -> bool {
    match c {
        Some(c) => !is_control(c) && !is_id_delim(c),
        None => false,
    }
}

pub struct Tokenized {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub diagnostics: Vec<Diagnostic>,
}

struct Lexer<'a> {
    src: &'a str,
    i: usize,
    u16: u32,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    fn pos(&self) -> Pos {
        Pos {
            offset: self.u16,
            byte: self.i as u32,
            line: self.line,
            column: self.col,
        }
    }
    fn at_end(&self) -> bool {
        self.i >= self.src.len()
    }
    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }
    fn peek_at(&self, k: usize) -> Option<char> {
        self.src[self.i..].chars().nth(k)
    }
    /// One source position; CRLF and lone CR count as ONE line break.
    fn advance(&mut self) {
        let Some(c) = self.peek() else { return };
        if c == '\r' {
            self.i += 1;
            self.u16 += 1;
            if self.peek() == Some('\n') {
                self.i += 1;
                self.u16 += 1;
            }
            self.line += 1;
            self.col = 1;
            return;
        }
        if c == '\n' {
            self.i += 1;
            self.u16 += 1;
            self.line += 1;
            self.col = 1;
            return;
        }
        self.i += c.len_utf8();
        let units = c.len_utf16() as u32;
        self.u16 += units;
        self.col += units;
    }
    fn span_from(&self, start: Pos) -> Range {
        Range {
            start,
            end: self.pos(),
        }
    }
}

pub fn tokenize(src: &str) -> Tokenized {
    let mut lx = Lexer {
        src,
        i: 0,
        u16: 0,
        line: 1,
        col: 1,
    };
    let mut tokens = Vec::new();
    let mut comments = Vec::new();
    let mut diagnostics = Vec::new();

    // A leading U+FEFF is the UTF-8 encoding signature, not content: it is
    // skipped as trivia so a BOM file parses like its BOM-less twin. Every
    // span still counts it (offset 1 / byte 3), so the text is never altered.
    // DELIBERATE DIFFERENCE from `src/vrml/tokenizer.js` (TAURI-RUST-
    // MIGRATION-2), which reads it as an identifier and reports VRML001/020.
    if lx.peek() == Some('\u{FEFF}') {
        lx.advance();
    }

    // --- header: optional leading whitespace, then `#VRML ...` ---
    {
        let save = (lx.i, lx.u16, lx.line, lx.col);
        while !lx.at_end() && lx.peek().is_some_and(is_whitespace) {
            lx.advance();
        }
        let head = &src[lx.i..];
        let is_header =
            head.len() >= 5 && head.is_char_boundary(5) && head[..5].eq_ignore_ascii_case("#VRML");
        if is_header {
            let start = lx.pos();
            while !lx.at_end() && !matches!(lx.peek(), Some('\n') | Some('\r')) {
                lx.advance();
            }
            let range = lx.span_from(start);
            let text = range.slice(src);
            let (version, encoding) = parse_header_words(text);
            tokens.push(Token {
                kind: TokKind::Header(Box::new(HeaderWords { version, encoding })),
                range,
            });
        } else {
            (lx.i, lx.u16, lx.line, lx.col) = save;
        }
    }

    loop {
        // trivia
        while !lx.at_end() {
            let c = lx.peek().unwrap();
            if is_whitespace(c) || c == ',' {
                lx.advance();
            } else if c == '#' {
                let start = lx.pos();
                while !lx.at_end() && !matches!(lx.peek(), Some('\n') | Some('\r')) {
                    lx.advance();
                }
                comments.push(Comment {
                    range: lx.span_from(start),
                });
            } else {
                break;
            }
        }
        if lx.at_end() {
            let p = lx.pos();
            tokens.push(Token {
                kind: TokKind::Eof,
                range: Range { start: p, end: p },
            });
            break;
        }
        let c = lx.peek().unwrap();
        let tok = match c {
            '{' => punct(&mut lx, TokKind::LBrace),
            '}' => punct(&mut lx, TokKind::RBrace),
            '[' => punct(&mut lx, TokKind::LBracket),
            ']' => punct(&mut lx, TokKind::RBracket),
            '"' => read_string(&mut lx, &mut diagnostics),
            '.' => {
                if lx.peek_at(1).is_some_and(is_digit) {
                    read_number(&mut lx, &mut diagnostics)
                } else {
                    punct(&mut lx, TokKind::Period)
                }
            }
            c if is_digit(c) => read_number(&mut lx, &mut diagnostics),
            '+' | '-'
                if lx.peek_at(1).is_some_and(is_digit)
                    || (lx.peek_at(1) == Some('.') && lx.peek_at(2).is_some_and(is_digit)) =>
            {
                read_number(&mut lx, &mut diagnostics)
            }
            c if is_id_start(Some(c)) => read_identifier(&mut lx),
            c => {
                let start = lx.pos();
                lx.advance();
                diagnostics.push(Diagnostic::error(
                    Code::UnexpectedChar,
                    format!("Unexpected character {:?}", c.to_string()),
                    lx.span_from(start),
                ));
                continue;
            }
        };
        tokens.push(tok);
    }

    Tokenized {
        tokens,
        comments,
        diagnostics,
    }
}

/// `#VRML V2.0 utf8` -> (Some("V2.0"), Some("utf8")); mirrors the JS regex
/// `/^#VRML\s+(\S+)\s+(\S+)/` (both words required, or neither is reported).
fn parse_header_words(text: &str) -> (Option<String>, Option<String>) {
    let rest = &text[5..];
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return (None, None);
    }
    let mut words = rest.split_whitespace();
    match (words.next(), words.next()) {
        (Some(v), Some(e)) => {
            // `\s+` between them is guaranteed by split_whitespace.
            (Some(v.to_string()), Some(e.to_string()))
        }
        _ => (None, None),
    }
}

fn punct(lx: &mut Lexer, kind: TokKind) -> Token {
    let start = lx.pos();
    lx.advance();
    Token {
        kind,
        range: lx.span_from(start),
    }
}

fn read_string(lx: &mut Lexer, diags: &mut Vec<Diagnostic>) -> Token {
    let start = lx.pos();
    lx.advance(); // opening quote
    let mut value = String::new();
    let mut terminated = false;
    while !lx.at_end() {
        let c = lx.peek().unwrap();
        if c == '\\' {
            let next = lx.peek_at(1);
            if next == Some('"') || next == Some('\\') {
                value.push(next.unwrap());
                lx.advance();
                lx.advance();
                continue;
            }
            value.push(c);
            lx.advance();
            continue;
        }
        if c == '"' {
            lx.advance();
            terminated = true;
            break;
        }
        if c == '\r' || c == '\n' {
            value.push('\n');
            lx.advance();
            continue;
        }
        value.push(c);
        lx.advance();
    }
    let range = lx.span_from(start);
    if !terminated {
        diags.push(Diagnostic::error(
            Code::UnterminatedString,
            "Unterminated string literal".to_string(),
            range,
        ));
    }
    Token {
        kind: TokKind::Str { value, terminated },
        range,
    }
}

fn read_number(lx: &mut Lexer, diags: &mut Vec<Diagnostic>) -> Token {
    let start = lx.pos();
    let mut valid = true;
    let mut numeric = Numeric::Int;
    if matches!(lx.peek(), Some('+') | Some('-')) {
        lx.advance();
    }
    if lx.peek() == Some('0') && matches!(lx.peek_at(1), Some('x') | Some('X')) {
        numeric = Numeric::Hex;
        lx.advance();
        lx.advance();
        let mut any = false;
        while lx.peek().is_some_and(is_hex_digit) {
            lx.advance();
            any = true;
        }
        if !any {
            valid = false;
        }
    } else {
        let mut int_digits = 0;
        while lx.peek().is_some_and(is_digit) {
            lx.advance();
            int_digits += 1;
        }
        if lx.peek() == Some('.') {
            numeric = Numeric::Float;
            lx.advance();
            while lx.peek().is_some_and(is_digit) {
                lx.advance();
            }
        }
        if matches!(lx.peek(), Some('e') | Some('E')) {
            numeric = Numeric::Float;
            lx.advance();
            if matches!(lx.peek(), Some('+') | Some('-')) {
                lx.advance();
            }
            let mut exp_digits = 0;
            while lx.peek().is_some_and(is_digit) {
                lx.advance();
                exp_digits += 1;
            }
            if exp_digits == 0 {
                valid = false;
            }
        }
        if int_digits == 0 && numeric != Numeric::Float {
            valid = false;
        }
        if numeric == Numeric::Float
            && int_digits == 0
            && !src_slice(lx, start).bytes().any(|b| b.is_ascii_digit())
        {
            valid = false;
        }
    }
    let p = lx.peek();
    if is_id_part(p) && p != Some('.') && p != Some('-') && p != Some('+') {
        valid = false;
        while is_id_part(lx.peek()) && !matches!(lx.peek(), Some('-') | Some('+')) {
            lx.advance();
        }
    }
    let range = lx.span_from(start);
    let text = range.slice(lx.src);
    let value = number_value(text, numeric);
    if !value.is_finite() {
        valid = false;
    }
    if !valid {
        diags.push(Diagnostic::error(
            Code::InvalidNumber,
            format!("Invalid number literal '{text}'"),
            range,
        ));
    }
    Token {
        kind: TokKind::Number {
            value,
            numeric,
            valid,
        },
        range,
    }
}

fn src_slice<'a>(lx: &Lexer<'a>, start: Pos) -> &'a str {
    &lx.src[start.byte as usize..lx.i]
}

/// Best-effort numeric value, mirroring JS parseInt/parseFloat prefix rules
/// closely enough for display; validity is decided by the lexical checks above.
fn number_value(text: &str, numeric: Numeric) -> f64 {
    match numeric {
        Numeric::Hex => {
            let (neg, body) = match text.as_bytes().first() {
                Some(b'-') => (true, &text[1..]),
                Some(b'+') => (false, &text[1..]),
                _ => (false, text),
            };
            let digits: String = body[2..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            match u128::from_str_radix(&digits, 16) {
                Ok(v) => {
                    let f = v as f64;
                    if neg {
                        -f
                    } else {
                        f
                    }
                }
                Err(_) => f64::NAN,
            }
        }
        _ => {
            // Longest numeric prefix (JS parseFloat / parseInt semantics).
            let bytes = text.as_bytes();
            let mut end = 0;
            if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
                end = 1;
            }
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if numeric == Numeric::Float {
                if end < bytes.len() && bytes[end] == b'.' {
                    end += 1;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                }
                if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
                    let mut e = end + 1;
                    if e < bytes.len() && (bytes[e] == b'+' || bytes[e] == b'-') {
                        e += 1;
                    }
                    let ds = e;
                    while e < bytes.len() && bytes[e].is_ascii_digit() {
                        e += 1;
                    }
                    if e > ds {
                        end = e;
                    }
                }
            }
            text[..end].parse::<f64>().unwrap_or(f64::NAN)
        }
    }
}

fn read_identifier(lx: &mut Lexer) -> Token {
    let start = lx.pos();
    lx.advance();
    while is_id_part(lx.peek()) {
        lx.advance();
    }
    let range = lx.span_from(start);
    let text = range.slice(lx.src);
    let kind = if text == "TRUE" || text == "FALSE" {
        TokKind::Bool(text == "TRUE")
    } else if KEYWORDS.contains(&text) {
        TokKind::Keyword
    } else {
        TokKind::Id
    };
    Token { kind, range }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_bom_is_signature_trivia_with_exact_spans() {
        let src = "\u{FEFF}#VRML V2.0 utf8\r\nGroup {}";
        let t = tokenize(src);
        assert!(t.diagnostics.is_empty());
        assert!(matches!(t.tokens[0].kind, TokKind::Header(_)));
        assert_eq!(t.tokens[0].range.start.offset, 1);
        assert_eq!(t.tokens[0].range.start.byte, 3);
        assert_eq!(t.tokens[1].lexeme(src), "Group");
        assert_eq!(
            t.tokens[1].range.start.offset as usize,
            src.encode_utf16().count() - "Group {}".len()
        );
        // Only a LEADING U+FEFF is a signature; elsewhere it stays content.
        assert_eq!(tokenize("Group {} \u{FEFF}").tokens.len(), 5);
    }

    fn kinds(src: &str) -> Vec<(String, String)> {
        let t = tokenize(src);
        t.tokens
            .iter()
            .map(|t| {
                let k = match &t.kind {
                    TokKind::Header(_) => "header",
                    TokKind::Id => "id",
                    TokKind::Keyword => "keyword",
                    TokKind::Bool(_) => "bool",
                    TokKind::Str { .. } => "string",
                    TokKind::Number { .. } => "number",
                    TokKind::LBrace => "{",
                    TokKind::RBrace => "}",
                    TokKind::LBracket => "[",
                    TokKind::RBracket => "]",
                    TokKind::Period => ".",
                    TokKind::Eof => "eof",
                };
                (k.to_string(), t.lexeme(src).to_string())
            })
            .collect()
    }

    #[test]
    fn header_and_basic_tokens() {
        let k = kinds("#VRML V2.0 utf8\nDEF A-b Transform { translation 1 -2 .5e1 }");
        let names: Vec<&str> = k.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(
            names,
            [
                "header", "keyword", "id", "id", "{", "id", "number", "number", "number", "}",
                "eof"
            ]
        );
        assert_eq!(k[2].1, "A-b");
        assert_eq!(k[8].1, ".5e1");
    }

    #[test]
    fn crlf_and_lone_cr_count_one_line_and_keep_exact_lexemes() {
        let src = "#VRML V2.0 utf8\r\nA {\r}\n\"x\r\ny\"";
        let t = tokenize(src);
        let a = &t.tokens[1];
        assert_eq!(a.range.start.line, 2);
        let rb = &t.tokens[3];
        assert_eq!(rb.range.start.line, 3);
        let s = &t.tokens[4];
        assert_eq!(s.range.start.line, 4);
        assert_eq!(s.lexeme(src), "\"x\r\ny\"");
        match &s.kind {
            TokKind::Str { value, terminated } => {
                assert_eq!(value, "x\ny");
                assert!(terminated);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn utf16_offsets_and_columns_count_code_units() {
        let src = "😀 é A";
        let t = tokenize(src);
        // "😀" is an identifier (non-ASCII allowed), 2 UTF-16 units, 4 bytes.
        assert_eq!(t.tokens[0].range.end.offset, 2);
        assert_eq!(t.tokens[0].range.end.byte, 4);
        assert_eq!(t.tokens[1].range.start.offset, 3);
        assert_eq!(t.tokens[1].range.start.column, 4);
        assert_eq!(t.tokens[2].range.start.offset, 5);
    }

    #[test]
    fn invalid_numbers_and_unknown_chars_diagnose_without_hanging() {
        let t = tokenize("12abc 0x \\ \"open");
        let codes: Vec<&str> = t.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["VRML011", "VRML011", "VRML012", "VRML010"]);
    }

    #[test]
    fn keywords_and_bools() {
        let k = kinds("TRUE FALSE NULL IS eventIn foo");
        let names: Vec<&str> = k.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(
            names,
            ["bool", "bool", "keyword", "keyword", "keyword", "id", "eof"]
        );
    }
}
