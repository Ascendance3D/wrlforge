// SPDX-License-Identifier: GPL-3.0-or-later
//! UTF-16 code-unit offsets <-> UTF-8 byte offsets, with boundary validation,
//! and strict (never lossy) UTF-16 decoding. Adopted from the RUST-0 spike.
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

/// A UTF-16 sequence that is not well formed: an unpaired surrogate.
///
/// `index` is the UTF-16 code-unit index of the FIRST unpaired surrogate, so a
/// caller can point at it. Nothing is ever substituted: there is no U+FFFD path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingError {
    pub index: usize,
}

/// Index of the first unpaired surrogate in `units`, or `None` when well formed.
/// Independent of `char::decode_utf16` so the two can check each other in tests.
pub fn first_unpaired_surrogate(units: &[u16]) -> Option<usize> {
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if (0xD800..=0xDBFF).contains(&u) {
            if i + 1 < units.len() && (0xDC00..=0xDFFF).contains(&units[i + 1]) {
                i += 2;
                continue;
            }
            return Some(i);
        }
        if (0xDC00..=0xDFFF).contains(&u) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Strict, lossless UTF-16 -> UTF-8. Refuses an unpaired surrogate; never
/// replaces it with U+FFFD. A legitimate U+FFFD in the input is kept as is.
pub fn decode_utf16_strict(units: &[u16]) -> Result<String, EncodingError> {
    if let Some(index) = first_unpaired_surrogate(units) {
        return Err(EncodingError { index });
    }
    let mut out = String::with_capacity(units.len());
    for c in char::decode_utf16(units.iter().copied()) {
        match c {
            Ok(c) => out.push(c),
            // Unreachable: the scan above proved well-formedness. Fail closed anyway.
            Err(_) => return Err(EncodingError { index: 0 }),
        }
    }
    Ok(out)
}

/// RUST-1A UTF-16 gate helper. Calls `genuine(unit)` with the UTF-16 index of
/// every U+FFFD in `text`, in order, and returns the first index for which it
/// answers `false`. `None` means every U+FFFD was confirmed (or there is none).
///
/// Why positions are enough: the WHATWG UTF-8 encoder replaces ONE unpaired
/// surrogate (one code unit) with ONE U+FFFD (one code unit) and copies every
/// other scalar unchanged. So UTF-16 indexes are preserved, and a converted
/// text holds a substitution exactly where its U+FFFD sits over a source unit
/// that is not 0xFFFD.
pub fn first_unconfirmed_replacement(
    text: &str,
    mut genuine: impl FnMut(usize) -> bool,
) -> Option<usize> {
    let mut unit = 0;
    let mut last = 0;
    for (byte, _) in text.match_indices('\u{FFFD}') {
        unit += utf16_len(&text[last..byte]);
        if !genuine(unit) {
            return Some(unit);
        }
        unit += 1;
        last = byte + '\u{FFFD}'.len_utf8();
    }
    None
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

    #[test]
    fn replacement_positions_are_utf16_indexes() {
        // "a" U+1F600 (2 units) U+FFFD "é" U+FFFD: FFFDs at units 3 and 5.
        let text = "a\u{1F600}\u{FFFD}\u{E9}\u{FFFD}";
        let mut seen = Vec::new();
        assert_eq!(
            first_unconfirmed_replacement(text, |u| {
                seen.push(u);
                true
            }),
            None
        );
        assert_eq!(seen, vec![3, 5]);
        // The first unconfirmed one is reported; later ones are not visited.
        let mut seen = Vec::new();
        assert_eq!(
            first_unconfirmed_replacement(text, |u| {
                seen.push(u);
                u != 3
            }),
            Some(3)
        );
        assert_eq!(seen, vec![3]);
        assert_eq!(
            first_unconfirmed_replacement("plain ascii", |_| false),
            None
        );
        assert_eq!(first_unconfirmed_replacement("", |_| false), None);
        assert_eq!(
            first_unconfirmed_replacement("\u{FFFD}", |u| u != 0),
            Some(0)
        );
    }

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
    fn strict_decode_refuses_lone_surrogates_and_keeps_real_fffd() {
        assert_eq!(
            decode_utf16_strict(&[0x61, 0xD800, 0x62]),
            Err(EncodingError { index: 1 })
        );
        assert_eq!(
            decode_utf16_strict(&[0xDC00]),
            Err(EncodingError { index: 0 })
        );
        assert_eq!(
            decode_utf16_strict(&[0x61, 0xD83D]),
            Err(EncodingError { index: 1 })
        );
        // reversed pair: low then high -> the low is unpaired at 0
        assert_eq!(
            decode_utf16_strict(&[0xDE00, 0xD83D]),
            Err(EncodingError { index: 0 })
        );
        assert_eq!(decode_utf16_strict(&[0xD83D, 0xDE00]).unwrap(), "\u{1F600}");
        // A real U+FFFD is ordinary text, not evidence of corruption.
        assert_eq!(decode_utf16_strict(&[0xFFFD]).unwrap(), "\u{FFFD}");
        assert_eq!(
            decode_utf16_strict(&[0x0000, 0xFEFF]).unwrap(),
            "\u{0}\u{FEFF}"
        );
        assert_eq!(decode_utf16_strict(&[]).unwrap(), "");
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
