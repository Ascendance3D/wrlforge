// SPDX-License-Identifier: GPL-3.0-or-later
//! An immutable, derived text snapshot.
//!
//! A snapshot is a read-only copy of ONE exact source revision owned elsewhere
//! (CodeMirror / the JavaScript session). It is never edited in place, never
//! written anywhere, and is not a second canonical document: edits produce a
//! proposed `String`, and only the JavaScript owner decides whether that text
//! becomes the next revision.

use crate::edit::{apply_edits, Edit, EditError};
use crate::line_index::{LineCol, LineEndings, LineError, LineIndex};
use crate::offsets::OffsetError;

/// A validated half-open span in both offset systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub from: u64,
    pub to: u64,
    pub byte_from: usize,
    pub byte_to: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanError {
    Inverted,
    From(OffsetError),
    To(OffsetError),
}

#[derive(Debug)]
pub struct TextSnapshot {
    text: String,
    lines: LineIndex,
}

impl TextSnapshot {
    /// Takes ownership of already-validated UTF-8 (a Rust `String` cannot hold
    /// an unpaired surrogate; the boundary refuses those before this point).
    pub fn new(text: String) -> Self {
        let lines = LineIndex::new(&text);
        TextSnapshot { text, lines }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn utf16_len(&self) -> u64 {
        self.lines.utf16_len()
    }

    pub fn utf16_to_utf8(&self, offset: u64) -> Result<usize, OffsetError> {
        self.lines.utf16_to_byte(&self.text, offset)
    }

    pub fn utf8_to_utf16(&self, byte: usize) -> Result<u64, OffsetError> {
        self.lines.byte_to_utf16(&self.text, byte)
    }

    pub fn validate_span(&self, from: u64, to: u64) -> Result<Span, SpanError> {
        if from > to {
            return Err(SpanError::Inverted);
        }
        let byte_from = self.utf16_to_utf8(from).map_err(SpanError::From)?;
        let byte_to = self.utf16_to_utf8(to).map_err(SpanError::To)?;
        Ok(Span {
            from,
            to,
            byte_from,
            byte_to,
        })
    }

    pub fn line_col(&self, offset: u64) -> Result<LineCol, OffsetError> {
        self.lines.line_col(&self.text, offset)
    }

    pub fn offset_at(&self, line: u64, column: u64) -> Result<u64, LineError> {
        self.lines.offset_at(&self.text, line, column)
    }

    pub fn line_count(&self) -> u64 {
        self.lines.line_count()
    }

    pub fn line_endings(&self) -> LineEndings {
        self.lines.endings()
    }

    /// A proposed next text. The snapshot itself never changes.
    pub fn apply_edits(&self, edits: &[Edit]) -> Result<String, EditError> {
        apply_edits(&self.text, edits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_unchanged_by_a_proposal() {
        let s = TextSnapshot::new("a\r\nb".to_string());
        let next = s
            .apply_edits(&[Edit {
                from: 3,
                to: 4,
                insert: "B".into(),
            }])
            .unwrap();
        assert_eq!(next, "a\r\nB");
        assert_eq!(s.text(), "a\r\nb");
    }

    #[test]
    fn spans_validate_both_ends() {
        let s = TextSnapshot::new("a😀b".to_string());
        assert_eq!(
            s.validate_span(1, 3),
            Ok(Span {
                from: 1,
                to: 3,
                byte_from: 1,
                byte_to: 5
            })
        );
        assert_eq!(s.validate_span(3, 1), Err(SpanError::Inverted));
        assert_eq!(
            s.validate_span(2, 3),
            Err(SpanError::From(OffsetError::InsideSurrogatePair {
                offset: 2
            }))
        );
        assert!(matches!(
            s.validate_span(0, 9),
            Err(SpanError::To(OffsetError::OutOfBounds { .. }))
        ));
    }
}
