// SPDX-License-Identifier: GPL-3.0-or-later
//! Parser diagnostics. Codes and severities mirror `src/vrml/diagnostics.js`.

use crate::tokenizer::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    MissingHeader,
    InvalidHeader,
    UnterminatedString,
    InvalidNumber,
    UnexpectedChar,
    ExpectedToken,
    UnexpectedToken,
    ExpectedFieldValue,
    UnclosedBrace,
    UnclosedBracket,
    ExpectedIdentifier,
    ExpectedInterface,
    MaxDepth,
    MaxNodes,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::MissingHeader => "VRML001",
            Code::InvalidHeader => "VRML002",
            Code::UnterminatedString => "VRML010",
            Code::InvalidNumber => "VRML011",
            Code::UnexpectedChar => "VRML012",
            Code::ExpectedToken => "VRML020",
            Code::UnexpectedToken => "VRML021",
            Code::ExpectedFieldValue => "VRML022",
            Code::UnclosedBrace => "VRML023",
            Code::UnclosedBracket => "VRML024",
            Code::ExpectedIdentifier => "VRML026",
            Code::ExpectedInterface => "VRML027",
            Code::MaxDepth => "VRML030",
            Code::MaxNodes => "VRML032",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    pub range: Range,
}

impl Diagnostic {
    pub fn error(code: Code, message: String, range: Range) -> Self {
        Diagnostic {
            code,
            severity: Severity::Error,
            message,
            range,
        }
    }
    pub fn warning(code: Code, message: String, range: Range) -> Self {
        Diagnostic {
            code,
            severity: Severity::Warning,
            message,
            range,
        }
    }
}
