// SPDX-License-Identifier: GPL-3.0-or-later
//! The WD1.2 span-patch algebra (`src/vrml/edit.js`), adopted from the RUST-0
//! spike. No production caller exists.
//!
//! Contract, kept identical to the JavaScript module:
//! * `from`/`to` are zero-based UTF-16 code-unit offsets, half-open `[from, to)`.
//! * Canonical order: `from` ascending, insertion first, then `to`, then the
//!   insert text compared by UTF-16 code units (JavaScript `<` on strings).
//!   Comparing Rust strings byte-wise would order U+E000..U+FFFF after astral
//!   characters and change which edit an error names.
//! * Same-offset insertions are refused (EEDITAMBIGUOUS), never ordered.
//! * Overlaps are refused (EEDITOVERLAP), never merged.
//! * Errors carry the offending edit's index in the CALLER'S array.
//! * Text outside the edited spans is copied verbatim.
//! * A mapped range that comes back inverted is refused (EEDITINVERTED).
//!
//! Offsets are `u64`, not `usize`: on wasm32 `usize` is 32 bits, and the
//! JavaScript contract accepts any non-negative integer. Text-bound offsets are
//! narrowed to `usize` only after the bounds check proves they fit.
//!
//! One deliberate, additive difference (registry id `RUST0-BOUNDARY`):
//! * EEDITBOUNDARY: an endpoint strictly inside a surrogate pair is refused.
//!   JavaScript applies it and can produce a lone surrogate, which the save path
//!   (`Buffer.from(text, 'utf8')`) silently turns into U+FFFD. This check runs
//!   only after every JavaScript check has passed, so it never changes an error
//!   JavaScript would have reported.

use crate::offsets::{utf16_len, OffsetError, Utf16Cursor};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub from: u64,
    pub to: u64,
    pub insert: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Affinity {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCode {
    Shape,
    Bounds,
    Overlap,
    Ambiguous,
    Range,
    Inverted,
    Boundary,
    /// A mapped offset does not fit in `u64`. Unreachable from the JS boundary,
    /// which refuses anything above `Number.MAX_SAFE_INTEGER` first.
    Overflow,
}

impl EditCode {
    /// The stable `err.code` string. The first six are `src/vrml/edit.js`'s.
    pub fn as_str(self) -> &'static str {
        match self {
            EditCode::Shape => "EEDITSHAPE",
            EditCode::Bounds => "EEDITBOUNDS",
            EditCode::Overlap => "EEDITOVERLAP",
            EditCode::Ambiguous => "EEDITAMBIGUOUS",
            EditCode::Range => "EEDITRANGE",
            EditCode::Inverted => "EEDITINVERTED",
            EditCode::Boundary => "EEDITBOUNDARY",
            EditCode::Overflow => "EOFFSETOVERFLOW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditError {
    pub code: EditCode,
    pub index: Option<usize>,
    pub other_index: Option<usize>,
}

fn err(code: EditCode, index: usize, other: Option<usize>) -> EditError {
    EditError {
        code,
        index: Some(index),
        other_index: other,
    }
}

fn bare(code: EditCode) -> EditError {
    EditError {
        code,
        index: None,
        other_index: None,
    }
}

fn is_insertion(e: &Edit) -> bool {
    e.from == e.to
}

#[cfg(not(feature = "negative-control-utf8-tiebreak"))]
fn compare_inserts(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

// NEGATIVE CONTROL: the defect RUST-0 found while porting.
#[cfg(feature = "negative-control-utf8-tiebreak")]
fn compare_inserts(a: &str, b: &str) -> Ordering {
    a.cmp(b)
}

fn compare_edits(a: &Edit, b: &Edit) -> Ordering {
    a.from
        .cmp(&b.from)
        .then_with(|| (!is_insertion(a)).cmp(&!is_insertion(b)))
        .then_with(|| a.to.cmp(&b.to))
        .then_with(|| compare_inserts(&a.insert, &b.insert))
}

/// `normalizeEdits`: validate and return caller indexes in canonical order.
/// `text_len` is `None` for the text-free mapping functions.
fn normalize(edits: &[Edit], text_len: Option<u64>) -> Result<Vec<usize>, EditError> {
    for (i, e) in edits.iter().enumerate() {
        if e.from > e.to {
            return Err(err(EditCode::Shape, i, None));
        }
    }
    if let Some(len) = text_len {
        for (i, e) in edits.iter().enumerate() {
            if e.to > len {
                return Err(err(EditCode::Bounds, i, None));
            }
        }
    }
    let mut order: Vec<usize> = (0..edits.len()).collect();
    // `sort_by` is stable, like Array.prototype.sort.
    order.sort_by(|&a, &b| compare_edits(&edits[a], &edits[b]));

    let mut max_end: Option<(u64, usize)> = None; // (end, caller index)
    let mut insertions: Vec<(u64, usize)> = Vec::new(); // (offset, caller index)
    for &i in &order {
        let e = &edits[i];
        let inside = |m: Option<(u64, usize)>| m.filter(|&(end, _)| e.from < end);
        if is_insertion(e) {
            if cfg!(not(feature = "negative-control-no-ambiguity")) {
                if let Some(&(_, seen)) = insertions.iter().find(|&&(o, _)| o == e.from) {
                    return Err(err(EditCode::Ambiguous, i, Some(seen)));
                }
            }
            if let Some((_, other)) = inside(max_end) {
                return Err(err(EditCode::Overlap, i, Some(other)));
            }
            insertions.push((e.from, i));
            if max_end.is_none_or(|(end, _)| e.to > end) {
                max_end = Some((e.to, i));
            }
            continue;
        }
        if let Some((_, other)) = inside(max_end) {
            return Err(err(EditCode::Overlap, i, Some(other)));
        }
        max_end = Some((e.to, i));
    }
    Ok(order)
}

/// `validateEdits`: the canonical order as caller indexes, or the refusal.
pub fn validate_edits(text: &str, edits: &[Edit]) -> Result<Vec<usize>, EditError> {
    normalize(edits, Some(utf16_len(text) as u64))
}

/// `applyEdits`: atomic; any refusal produces no output at all.
pub fn apply_edits(text: &str, edits: &[Edit]) -> Result<String, EditError> {
    let order = validate_edits(text, edits)?;
    // Additive Rust-only check, after every JavaScript check has passed.
    let mut cursor = Utf16Cursor::new(text);
    let mut spans = Vec::with_capacity(order.len());
    for &i in &order {
        let e = &edits[i];
        // Bounds proved `to <= utf16_len(text)`, which fits in usize.
        let from = seek(&mut cursor, e.from as usize, i)?;
        let to = seek(&mut cursor, e.to as usize, i)?;
        spans.push((from, to, i));
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (from, to, i) in spans {
        out.push_str(&text[at..from]);
        out.push_str(&edits[i].insert);
        at = to;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

#[cfg(not(feature = "negative-control-no-boundary"))]
fn seek(cursor: &mut Utf16Cursor<'_>, offset: usize, index: usize) -> Result<usize, EditError> {
    cursor.seek(offset).map_err(|x| match x {
        OffsetError::InsideSurrogatePair { .. } => err(EditCode::Boundary, index, None),
        // Bounds were already proven by `normalize`; reaching this is a bug.
        _ => unreachable!("offset error after bounds validation: {x:?}"),
    })
}

// NEGATIVE CONTROL: silently round an interior offset down to the scalar start.
#[cfg(feature = "negative-control-no-boundary")]
fn seek(cursor: &mut Utf16Cursor<'_>, offset: usize, _index: usize) -> Result<usize, EditError> {
    match cursor.seek(offset) {
        Ok(b) => Ok(b),
        Err(OffsetError::InsideSurrogatePair { .. }) => Ok(cursor.seek(offset - 1).unwrap()),
        Err(x) => unreachable!("{x:?}"),
    }
}

/// `mapOffset`: pure UTF-16 arithmetic over a validated edit set.
pub fn map_offset(offset: u64, edits: &[Edit], affinity: Affinity) -> Result<u64, EditError> {
    let order = normalize(edits, None)?;
    let after = affinity == Affinity::After;
    // i128: every intermediate fits, so nothing wraps silently.
    let mut shift: i128 = 0;
    let done = |v: i128| u64::try_from(v).map_err(|_| bare(EditCode::Overflow));
    for &i in &order {
        let e = &edits[i];
        let n = utf16_len(&e.insert) as i128;
        let grow = if after { n } else { 0 };
        if is_insertion(e) {
            if offset < e.from {
                return done(offset as i128 + shift);
            }
            if offset == e.from {
                return done(offset as i128 + shift + grow);
            }
            shift += n;
            continue;
        }
        if offset < e.from {
            return done(offset as i128 + shift);
        }
        if offset < e.to {
            return done(e.from as i128 + shift + grow);
        }
        shift += n - (e.to - e.from) as i128;
    }
    done(offset as i128 + shift)
}

/// `mapRange`: map both endpoints, one affinity each; an inverted result is
/// refused, never swapped or clamped.
pub fn map_range(
    from: u64,
    to: u64,
    edits: &[Edit],
    start: Affinity,
    end: Affinity,
) -> Result<(u64, u64), EditError> {
    if from > to {
        return Err(bare(EditCode::Range));
    }
    let a = map_offset(from, edits, start)?;
    let b = map_offset(to, edits, end)?;
    if a > b {
        return Err(bare(EditCode::Inverted));
    }
    Ok((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(from: u64, to: u64, insert: &str) -> Edit {
        Edit {
            from,
            to,
            insert: insert.to_string(),
        }
    }

    // Hand-authored expectations, mirroring the WD1.2 contract text.
    #[test]
    fn empty_set_returns_text_unchanged() {
        let t = "#VRML V2.0 utf8\r\nShape {}\r\n";
        assert_eq!(apply_edits(t, &[]).unwrap(), t);
        assert_eq!(apply_edits("", &[]).unwrap(), "");
        assert_eq!(apply_edits("", &[e(0, 0, "")]).unwrap(), "");
    }

    #[test]
    fn outside_bytes_are_untouched_including_crlf_cr_bom_nul() {
        let t = "\u{FEFF}a\r\nb\rc\n\0";
        assert_eq!(
            apply_edits(t, &[e(4, 5, "B")]).unwrap(),
            "\u{FEFF}a\r\nB\rc\n\0"
        );
    }

    #[test]
    fn caller_order_does_not_matter() {
        let t = "0123456789";
        let a = apply_edits(t, &[e(8, 9, "x"), e(1, 2, "y")]).unwrap();
        let b = apply_edits(t, &[e(1, 2, "y"), e(8, 9, "x")]).unwrap();
        assert_eq!(a, "0y234567x9");
        assert_eq!(a, b);
    }

    #[test]
    fn same_offset_insertions_are_ambiguous_even_when_empty() {
        let r = apply_edits("abc", &[e(1, 1, "x"), e(1, 1, "y")]).unwrap_err();
        assert_eq!(r.code, EditCode::Ambiguous);
        assert_eq!((r.index, r.other_index), (Some(1), Some(0)));
        let r = apply_edits("abc", &[e(1, 1, ""), e(1, 1, "")]).unwrap_err();
        assert_eq!(r.code, EditCode::Ambiguous);
    }

    #[test]
    fn insertion_strictly_inside_a_span_overlaps_but_at_its_edges_does_not() {
        assert_eq!(
            apply_edits("abcd", &[e(1, 3, ""), e(2, 2, "x")])
                .unwrap_err()
                .code,
            EditCode::Overlap
        );
        assert_eq!(
            apply_edits("abcd", &[e(1, 3, ""), e(1, 1, "x")]).unwrap(),
            "axd"
        );
        assert_eq!(
            apply_edits("abcd", &[e(1, 3, ""), e(3, 3, "x")]).unwrap(),
            "axd"
        );
    }

    #[test]
    fn bounds_and_shape_including_huge_offsets() {
        assert_eq!(
            apply_edits("ab", &[e(1, 3, "")]).unwrap_err().code,
            EditCode::Bounds
        );
        assert_eq!(
            apply_edits("ab", &[e(2, 1, "")]).unwrap_err().code,
            EditCode::Shape
        );
        // Larger than any wasm32 usize: refused by bounds, never truncated.
        let huge = (1u64 << 53) - 1;
        assert_eq!(
            apply_edits("ab", &[e(0, huge, "")]).unwrap_err().code,
            EditCode::Bounds
        );
        assert_eq!(
            apply_edits("ab", &[e(1u64 << 32, 1u64 << 32, "")])
                .unwrap_err()
                .code,
            EditCode::Bounds
        );
    }

    #[test]
    fn astral_offsets_count_two_units() {
        let t = "a😀b";
        assert_eq!(apply_edits(t, &[e(1, 3, "é")]).unwrap(), "aéb");
        let r = apply_edits(t, &[e(2, 2, "x")]).unwrap_err();
        assert_eq!((r.code, r.index), (EditCode::Boundary, Some(0)));
        let r = apply_edits(t, &[e(0, 1, ""), e(3, 4, "")]).unwrap();
        assert_eq!(r, "😀");
    }

    #[test]
    fn canonical_tie_break_uses_utf16_order() {
        // U+FF5E is one unit 0xFF5E; U+1F600 starts with unit 0xD83D. In
        // UTF-16 order 😀 < ～; in UTF-8 byte order ～ (EF..) < 😀 (F0..).
        // The duplicate span is reported on the edit that sorts second.
        let r = apply_edits("abc", &[e(0, 1, "\u{FF5E}"), e(0, 1, "😀")]).unwrap_err();
        assert_eq!(r.code, EditCode::Overlap);
        assert_eq!((r.index, r.other_index), (Some(0), Some(1)));
    }

    #[test]
    fn map_offset_affinity() {
        let ed = [e(2, 2, "xyz")];
        assert_eq!(map_offset(2, &ed, Affinity::Before).unwrap(), 2);
        assert_eq!(map_offset(2, &ed, Affinity::After).unwrap(), 5);
        assert_eq!(map_offset(4, &[e(1, 3, "")], Affinity::Before).unwrap(), 2);
        assert_eq!(map_offset(2, &[e(1, 3, "Q")], Affinity::After).unwrap(), 2);
        // astral insert counts two units
        assert_eq!(
            map_offset(5, &[e(0, 0, "😀")], Affinity::Before).unwrap(),
            7
        );
    }

    #[test]
    fn map_offset_large_values_are_exact() {
        let big = (1u64 << 53) - 1;
        assert_eq!(map_offset(big, &[], Affinity::Before).unwrap(), big);
        assert_eq!(
            map_offset(big, &[e(0, 0, "ab")], Affinity::Before).unwrap(),
            big + 2
        );
        assert_eq!(
            map_offset(u64::MAX, &[], Affinity::Before).unwrap(),
            u64::MAX
        );
        assert_eq!(
            map_offset(u64::MAX, &[e(0, 0, "a")], Affinity::Before)
                .unwrap_err()
                .code,
            EditCode::Overflow
        );
    }

    #[test]
    fn map_range_defaults_and_refusals() {
        let ed = [e(2, 2, "xy")];
        assert_eq!(
            map_range(2, 2, &ed, Affinity::Before, Affinity::After).unwrap(),
            (2, 4)
        );
        assert_eq!(
            map_range(3, 2, &ed, Affinity::Before, Affinity::After)
                .unwrap_err()
                .code,
            EditCode::Range
        );
        // replacement [1,3) by "Q": start after -> 2, end before -> 1: inverted
        let r = map_range(1, 2, &[e(1, 3, "Q")], Affinity::After, Affinity::Before).unwrap_err();
        assert_eq!(r.code, EditCode::Inverted);
    }
}
