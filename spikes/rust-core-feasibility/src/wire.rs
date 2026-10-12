// SPDX-License-Identifier: GPL-3.0-or-later
//! Dependency-free line protocol for the differential harness.
//!
//! Strings travel as UTF-16 code units, four lowercase hex digits each, or `-`
//! for the empty string. UTF-16 is used on the wire, not UTF-8, so the harness
//! can hand Rust exactly what a JavaScript string holds -- including a lone
//! surrogate -- and observe the refusal rather than a pre-sanitized input.
//!
//! Requests:
//!   A <id> <text> <n> (<from> <to> <insert>){n}        applyEdits
//!   M <id> <offset> <b|a> <n> (<from> <to> <insert>){n} mapOffset
//!   C <id> <text> <utf16-offset>                      offset round trip
//! Responses:
//!   <id> OK <string> | <id> OKN <number> | <id> OKC <byte> <utf16>
//!   <id> ERR <CODE> <index|-> <otherIndex|->
//! Codes beyond edit.js's: EEDITBOUNDARY, EENCODING (ill-formed UTF-16),
//! EOFFSETBOUNDS / EOFFSETSURROGATE (C requests), EWIRE (malformed request).

use crate::edit::{apply_edits, map_offset, Affinity, Edit, EditError};
use crate::offsets::{byte_to_utf16, utf16_to_byte, OffsetError};

enum Fail {
    Wire,
    Encoding,
}

pub fn decode(s: &str) -> Result<String, &'static str> {
    if s == "-" {
        return Ok(String::new());
    }
    if !s.len().is_multiple_of(4) {
        return Err("EWIRE");
    }
    let mut units = Vec::with_capacity(s.len() / 4);
    for i in (0..s.len()).step_by(4) {
        units.push(u16::from_str_radix(&s[i..i + 4], 16).map_err(|_| "EWIRE")?);
    }
    String::from_utf16(&units).map_err(|_| "EENCODING")
}

pub fn encode(s: &str) -> String {
    if s.is_empty() {
        return "-".to_string();
    }
    s.encode_utf16().map(|u| format!("{u:04x}")).collect()
}

fn text(tok: Option<&str>) -> Result<String, Fail> {
    match decode(tok.ok_or(Fail::Wire)?) {
        Ok(s) => Ok(s),
        Err("EENCODING") => Err(Fail::Encoding),
        Err(_) => Err(Fail::Wire),
    }
}

fn num(tok: Option<&str>) -> Result<usize, Fail> {
    tok.ok_or(Fail::Wire)?.parse().map_err(|_| Fail::Wire)
}

fn edits<'a>(it: &mut impl Iterator<Item = &'a str>) -> Result<Vec<Edit>, Fail> {
    let n = num(it.next())?;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let from = num(it.next())?;
        let to = num(it.next())?;
        let insert = text(it.next())?;
        v.push(Edit { from, to, insert });
    }
    Ok(v)
}

fn edit_err(id: &str, e: EditError) -> String {
    let f = |x: Option<usize>| x.map_or("-".to_string(), |v| v.to_string());
    format!(
        "{id} ERR {} {} {}",
        e.code.as_str(),
        f(e.index),
        f(e.other_index)
    )
}

pub fn run_case(line: &str) -> String {
    let mut it = line.split(' ');
    let op = it.next().unwrap_or("");
    let id = it.next().unwrap_or("?").to_string();
    let result = (|| -> Result<String, Fail> {
        match op {
            "A" => {
                let t = text(it.next())?;
                let ed = edits(&mut it)?;
                Ok(match apply_edits(&t, &ed) {
                    Ok(s) => format!("{id} OK {}", encode(&s)),
                    Err(e) => edit_err(&id, e),
                })
            }
            "M" => {
                let off = num(it.next())?;
                let aff = match it.next() {
                    Some("b") => Affinity::Before,
                    Some("a") => Affinity::After,
                    _ => return Err(Fail::Wire),
                };
                let ed = edits(&mut it)?;
                Ok(match map_offset(off, &ed, aff) {
                    Ok(n) => format!("{id} OKN {n}"),
                    Err(e) => edit_err(&id, e),
                })
            }
            "C" => {
                let t = text(it.next())?;
                let off = num(it.next())?;
                Ok(match utf16_to_byte(&t, off) {
                    Ok(b) => match byte_to_utf16(&t, b) {
                        Ok(u) => format!("{id} OKC {b} {u}"),
                        Err(_) => unreachable!("a converted byte offset is a char boundary"),
                    },
                    Err(OffsetError::InsideSurrogatePair { .. }) => {
                        format!("{id} ERR EOFFSETSURROGATE - -")
                    }
                    Err(_) => format!("{id} ERR EOFFSETBOUNDS - -"),
                })
            }
            _ => Err(Fail::Wire),
        }
    })();
    match result {
        Ok(s) => s,
        Err(Fail::Encoding) => format!("{id} ERR EENCODING - -"),
        Err(Fail::Wire) => format!("{id} ERR EWIRE - -"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_strings_including_astral() {
        for s in ["", "a", "\r\n", "é€", "😀", "\u{FEFF}#VRML"] {
            assert_eq!(decode(&encode(s)).unwrap(), s);
        }
        assert_eq!(encode("😀"), "d83dde00");
    }

    #[test]
    fn lone_surrogate_is_refused_not_replaced() {
        assert_eq!(decode("0061d800"), Err("EENCODING"));
        assert_eq!(run_case("A 7 0061d800 0"), "7 ERR EENCODING - -");
    }

    #[test]
    fn requests() {
        assert_eq!(run_case("A 1 00610062 1 1 1 0078"), "1 OK 006100780062");
        assert_eq!(run_case("M 2 1 a 1 1 1 0078"), "2 OKN 2");
        assert_eq!(run_case("C 3 0061d83dde000062 3"), "3 OKC 5 3");
        assert_eq!(
            run_case("C 4 0061d83dde000062 2"),
            "4 ERR EOFFSETSURROGATE - -"
        );
        assert_eq!(run_case("Z 5"), "5 ERR EWIRE - -");
    }
}
