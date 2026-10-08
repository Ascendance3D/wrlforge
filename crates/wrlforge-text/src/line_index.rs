// SPDX-License-Identifier: GPL-3.0-or-later
//! Line index with JavaScript-compatible positions.
//!
//! Matches `src/vrml/tokenizer.js`: lines are 1-based; LF, CRLF and a lone CR
//! each end exactly one line; columns are 1-based and counted in UTF-16 code
//! units (an astral character advances the column by 2). Nothing is
//! normalized: the index only records where lines start.
//!
//! An offset between the CR and the LF of a CRLF belongs to the CR's line (the
//! tokenizer never reports such a position; it is defined here so the index is
//! total over every valid UTF-16 boundary).

use crate::offsets::{OffsetError, Utf16Cursor};

/// A 1-based line and a 1-based UTF-16 column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u64,
    pub column: u64,
}

/// How many of each line terminator the text contains. `mixed` is true when
/// more than one kind occurs (relevant to TEXT-1 / owner decision D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineEndings {
    pub lf: u64,
    pub crlf: u64,
    pub cr: u64,
}

impl LineEndings {
    pub fn mixed(&self) -> bool {
        [self.lf, self.crlf, self.cr]
            .iter()
            .filter(|&&n| n > 0)
            .count()
            > 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineError {
    /// `line` is 0 or past the last line.
    Line { line: u64, lines: u64 },
    /// `column` is 0 or past the end of the line's content.
    Column { column: u64, max: u64 },
    /// The column points between the halves of a surrogate pair.
    InsideSurrogatePair { column: u64 },
}

#[derive(Debug, Clone)]
struct LineStart {
    unit: u64,
    byte: usize,
    /// UTF-16 length of the line's content, terminator excluded.
    content_units: u64,
}

#[derive(Debug, Clone)]
pub struct LineIndex {
    lines: Vec<LineStart>,
    endings: LineEndings,
    len16: u64,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut lines = Vec::new();
        let mut endings = LineEndings::default();
        let (mut start_unit, mut start_byte) = (0u64, 0usize);
        let mut unit = 0u64;
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            // CR and LF are ASCII, so a byte scan never splits a scalar.
            if b == b'\r' || b == b'\n' {
                let content_units = unit - start_unit;
                let crlf = b == b'\r' && bytes.get(i + 1) == Some(&b'\n');
                let width = if crlf { 2 } else { 1 };
                match (b, crlf) {
                    (_, true) => endings.crlf += 1,
                    (b'\r', false) => endings.cr += 1,
                    _ => endings.lf += 1,
                }
                lines.push(LineStart {
                    unit: start_unit,
                    byte: start_byte,
                    content_units,
                });
                i += width;
                unit += width as u64;
                start_unit = unit;
                start_byte = i;
                continue;
            }
            // Advance one scalar.
            let c = text[i..].chars().next().expect("in bounds");
            unit += c.len_utf16() as u64;
            i += c.len_utf8();
        }
        lines.push(LineStart {
            unit: start_unit,
            byte: start_byte,
            content_units: unit - start_unit,
        });
        LineIndex {
            lines,
            endings,
            len16: unit,
        }
    }

    pub fn line_count(&self) -> u64 {
        self.lines.len() as u64
    }

    pub fn endings(&self) -> LineEndings {
        self.endings
    }

    pub fn utf16_len(&self) -> u64 {
        self.len16
    }

    fn line_of(&self, offset: u64) -> usize {
        // Last line whose start is <= offset.
        self.lines.partition_point(|l| l.unit <= offset) - 1
    }

    /// UTF-16 offset -> UTF-8 byte offset, via the line table. `text` must be
    /// the text this index was built from.
    pub fn utf16_to_byte(&self, text: &str, offset: u64) -> Result<usize, OffsetError> {
        if offset > self.len16 {
            // Saturate the payload, never truncate it: on wasm32 usize is 32 bits.
            return Err(OffsetError::OutOfBounds {
                offset: usize::try_from(offset).unwrap_or(usize::MAX),
                len: self.len16 as usize,
            });
        }
        let l = &self.lines[self.line_of(offset)];
        let within = (offset - l.unit) as usize;
        let mut cursor = Utf16Cursor::new(&text[l.byte..]);
        match cursor.seek(within) {
            Ok(b) => Ok(l.byte + b),
            Err(OffsetError::InsideSurrogatePair { .. }) => Err(OffsetError::InsideSurrogatePair {
                offset: offset as usize,
            }),
            Err(e) => unreachable!("in-bounds seek failed: {e:?}"),
        }
    }

    /// UTF-8 byte offset -> UTF-16 offset, via the line table.
    pub fn byte_to_utf16(&self, text: &str, byte: usize) -> Result<u64, OffsetError> {
        if byte > text.len() {
            return Err(OffsetError::OutOfBounds {
                offset: byte,
                len: text.len(),
            });
        }
        if !text.is_char_boundary(byte) {
            return Err(OffsetError::NotCharBoundary { offset: byte });
        }
        let li = self.lines.partition_point(|l| l.byte <= byte) - 1;
        let l = &self.lines[li];
        Ok(l.unit + crate::offsets::utf16_len(&text[l.byte..byte]) as u64)
    }

    pub fn line_col(&self, text: &str, offset: u64) -> Result<LineCol, OffsetError> {
        // Validates bounds and the surrogate boundary.
        self.utf16_to_byte(text, offset)?;
        let li = self.line_of(offset);
        Ok(LineCol {
            line: li as u64 + 1,
            column: offset - self.lines[li].unit + 1,
        })
    }

    /// Inverse of `line_col` over line content: `column` may be 1 through the
    /// content length + 1 (the position of the terminator, or end of text).
    pub fn offset_at(&self, text: &str, line: u64, column: u64) -> Result<u64, LineError> {
        let lines = self.line_count();
        if line == 0 || line > lines {
            return Err(LineError::Line { line, lines });
        }
        let l = &self.lines[(line - 1) as usize];
        let max = l.content_units + 1;
        if column == 0 || column > max {
            return Err(LineError::Column { column, max });
        }
        let offset = l.unit + column - 1;
        match self.utf16_to_byte(text, offset) {
            Ok(_) => Ok(offset),
            Err(OffsetError::InsideSurrogatePair { .. }) => {
                Err(LineError::InsideSurrogatePair { column })
            }
            Err(e) => unreachable!("in-line offset failed: {e:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lc(line: u64, column: u64) -> LineCol {
        LineCol { line, column }
    }

    #[test]
    fn empty_text_has_one_line() {
        let ix = LineIndex::new("");
        assert_eq!(ix.line_count(), 1);
        assert_eq!(ix.line_col("", 0), Ok(lc(1, 1)));
        assert_eq!(ix.offset_at("", 1, 1), Ok(0));
        assert_eq!(
            ix.offset_at("", 1, 2),
            Err(LineError::Column { column: 2, max: 1 })
        );
    }

    #[test]
    fn lf_crlf_and_lone_cr_each_end_one_line() {
        let t = "a\nb\r\nc\rd";
        let ix = LineIndex::new(t);
        assert_eq!(ix.line_count(), 4);
        assert_eq!(
            ix.endings(),
            LineEndings {
                lf: 1,
                crlf: 1,
                cr: 1
            }
        );
        assert!(ix.endings().mixed());
        assert_eq!(ix.line_col(t, 0), Ok(lc(1, 1)));
        assert_eq!(ix.line_col(t, 1), Ok(lc(1, 2))); // at the LF
        assert_eq!(ix.line_col(t, 2), Ok(lc(2, 1)));
        assert_eq!(ix.line_col(t, 3), Ok(lc(2, 2))); // at the CR
        assert_eq!(ix.line_col(t, 4), Ok(lc(2, 3))); // between CR and LF
        assert_eq!(ix.line_col(t, 5), Ok(lc(3, 1)));
        assert_eq!(ix.line_col(t, 7), Ok(lc(4, 1)));
        assert_eq!(ix.line_col(t, 8), Ok(lc(4, 2))); // end of text
        assert_eq!(ix.offset_at(t, 3, 1), Ok(5));
        assert_eq!(
            ix.offset_at(t, 2, 3),
            Err(LineError::Column { column: 3, max: 2 })
        );
        assert_eq!(
            ix.offset_at(t, 5, 1),
            Err(LineError::Line { line: 5, lines: 4 })
        );
    }

    #[test]
    fn trailing_terminator_opens_an_empty_last_line() {
        let t = "x\r\n";
        let ix = LineIndex::new(t);
        assert_eq!(ix.line_count(), 2);
        assert_eq!(ix.line_col(t, 3), Ok(lc(2, 1)));
        assert!(!ix.endings().mixed());
    }

    #[test]
    fn columns_count_utf16_units_and_refuse_pair_interiors() {
        // tokenizer measurement from RUST-0: `DEF 😀x Group` -> Group at column 9
        let t = "DEF 😀x Group";
        let ix = LineIndex::new(t);
        assert_eq!(ix.line_col(t, 8), Ok(lc(1, 9)));
        assert_eq!(
            ix.line_col(t, 5),
            Err(OffsetError::InsideSurrogatePair { offset: 5 })
        );
        assert_eq!(
            ix.offset_at(t, 1, 6),
            Err(LineError::InsideSurrogatePair { column: 6 })
        );
        assert_eq!(ix.utf16_to_byte(t, 6), Ok(8));
        assert_eq!(ix.byte_to_utf16(t, 8), Ok(6));
        assert_eq!(
            ix.byte_to_utf16(t, 5),
            Err(OffsetError::NotCharBoundary { offset: 5 })
        );
    }

    #[test]
    fn indexed_conversion_matches_linear_conversion() {
        let t = "é\r\n😀\r€x\n\u{FEFF}\0";
        let ix = LineIndex::new(t);
        for off in 0..=crate::offsets::utf16_len(t) + 1 {
            assert_eq!(
                ix.utf16_to_byte(t, off as u64),
                crate::offsets::utf16_to_byte(t, off),
                "offset {off}"
            );
        }
        for b in 0..=t.len() + 1 {
            assert_eq!(
                ix.byte_to_utf16(t, b).map(|u| u as usize),
                crate::offsets::byte_to_utf16(t, b),
                "byte {b}"
            );
        }
    }
}
