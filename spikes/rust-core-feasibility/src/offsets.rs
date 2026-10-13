// SPDX-License-Identifier: GPL-3.0-or-later
//! UTF-16 code-unit offsets <-> UTF-8 byte offsets, with boundary validation.
//!
//! The JavaScript core (tokenizer, source map, edit algebra, CodeMirror) speaks
//! UTF-16 code-unit offsets. A Rust `str` stores UTF-8. This module is the ONLY
//! place the two offset systems meet. It never guesses: an offset that does not
//! fall on a Unicode scalar boundary is an error, never a rounded position.

/// Why an offset was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetError {
    /// The offset is past the end of the text.
    OutOfBounds { offset: usize, len: usize },
    /// A UTF-16 offset points between the two halves of a surrogate pair.
    InsideSurrogatePair { offset: usize },
    /// A UTF-8 byte offset points inside a multi-byte scalar.
    NotCharBoundary { offset: usize },
}

/// Length of `text` in UTF-16 code units (the JavaScript `string.length`).
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Convert one UTF-16 offset to a UTF-8 byte offset.
pub fn utf16_to_byte(text: &str, offset: usize) -> Result<usize, OffsetError> {
    let mut cursor = Utf16Cursor::new(text);
    cursor.seek(offset)
}

/// Convert one UTF-8 byte offset to a UTF-16 offset.
pub fn byte_to_utf16(text: &str, byte: usize) -> Result<usize, OffsetError> {
    if byte > text.len() {
        return Err(OffsetError::OutOfBounds {
            offset: byte,
            len: text.len(),
        });
    }
    if !text.is_char_boundary(byte) {
        return Err(OffsetError::NotCharBoundary { offset: byte });
    }
    Ok(utf16_len(&text[..byte]))
}

/// A forward-only converter. Seeking a non-decreasing sequence of UTF-16
/// offsets costs one pass over the text in total, which is what applying a
/// canonically ordered edit set needs.
pub struct Utf16Cursor<'a> {
    text: &'a str,
    byte: usize,
    unit: usize,
}

impl<'a> Utf16Cursor<'a> {
    pub fn new(text: &'a str) -> Self {
        Utf16Cursor {
            text,
            byte: 0,
            unit: 0,
        }
    }

    /// Advance to UTF-16 `offset` and return its byte offset. Seeking backwards
    /// restarts from the beginning (correct, only slower).
    pub fn seek(&mut self, offset: usize) -> Result<usize, OffsetError> {
        if offset < self.unit {
            self.byte = 0;
            self.unit = 0;
        }
        while self.unit < offset {
            let Some(c) = self.text[self.byte..].chars().next() else {
                return Err(OffsetError::OutOfBounds {
                    offset,
                    len: self.unit,
                });
            };
            let units = c.len_utf16();
            if self.unit + units > offset {
                // offset == self.unit + 1 and c is astral: between the halves.
                return Err(OffsetError::InsideSurrogatePair { offset });
            }
            self.unit += units;
            self.byte += c.len_utf8();
        }
        Ok(self.byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values below are hand-derived from the Unicode encodings, not
    // produced by either implementation.
    #[test]
    fn ascii_is_identity() {
        let t = "ab\r\ncd";
        for i in 0..=6 {
            assert_eq!(utf16_to_byte(t, i), Ok(i));
            assert_eq!(byte_to_utf16(t, i), Ok(i));
        }
        assert_eq!(utf16_len(t), 6);
    }

    #[test]
    fn bmp_multibyte() {
        // é = U+00E9 (2 bytes, 1 unit); € = U+20AC (3 bytes, 1 unit)
        let t = "é€x";
        assert_eq!(utf16_len(t), 3);
        assert_eq!(utf16_to_byte(t, 1), Ok(2));
        assert_eq!(utf16_to_byte(t, 2), Ok(5));
        assert_eq!(utf16_to_byte(t, 3), Ok(6));
        assert_eq!(
            byte_to_utf16(t, 1),
            Err(OffsetError::NotCharBoundary { offset: 1 })
        );
        assert_eq!(byte_to_utf16(t, 5), Ok(2));
    }

    #[test]
    fn astral_occupies_two_units_and_refuses_the_middle() {
        // 😀 = U+1F600: 4 bytes, 2 units
        let t = "a😀b";
        assert_eq!(utf16_len(t), 4);
        assert_eq!(utf16_to_byte(t, 1), Ok(1));
        assert_eq!(
            utf16_to_byte(t, 2),
            Err(OffsetError::InsideSurrogatePair { offset: 2 })
        );
        assert_eq!(utf16_to_byte(t, 3), Ok(5));
        assert_eq!(utf16_to_byte(t, 4), Ok(6));
        assert_eq!(
            utf16_to_byte(t, 5),
            Err(OffsetError::OutOfBounds { offset: 5, len: 4 })
        );
        for b in 2..=4 {
            assert_eq!(
                byte_to_utf16(t, b),
                Err(OffsetError::NotCharBoundary { offset: b })
            );
        }
        assert_eq!(
            byte_to_utf16(t, 7),
            Err(OffsetError::OutOfBounds { offset: 7, len: 6 })
        );
    }

    #[test]
    fn bom_is_an_ordinary_scalar() {
        // U+FEFF is kept, never stripped: 3 bytes, 1 unit.
        let t = "\u{FEFF}#V";
        assert_eq!(utf16_len(t), 3);
        assert_eq!(utf16_to_byte(t, 1), Ok(3));
    }

    #[test]
    fn cursor_matches_single_shot_and_restarts_backwards() {
        let t = "x😀é€\r\n😀";
        let mut c = Utf16Cursor::new(t);
        for off in [0, 1, 3, 4, 5, 6, 7, 9, 3, 0] {
            assert_eq!(c.seek(off), utf16_to_byte(t, off), "offset {off}");
        }
    }
}
