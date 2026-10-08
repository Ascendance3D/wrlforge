// SPDX-License-Identifier: GPL-3.0-or-later
//! A Rust port of the WD1.2 span-patch algebra (`src/vrml/edit.js`), for the
//! RUST-0 feasibility question only. No production caller exists.
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
//!
//! One deliberate, additive difference (see README "Approved differences"):
//! * EEDITBOUNDARY: an endpoint strictly inside a surrogate pair is refused.
//!   JavaScript applies it and can produce a lone surrogate, which the save path
//!   (`Buffer.from(text, 'utf8')`) silently turns into U+FFFD. This check runs
//!   only after every JavaScript check has passed, so it never changes an error
//!   JavaScript would have reported.

use crate::offsets::{utf16_len, OffsetError, Utf16Cursor};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub from: usize,
    pub to: usize,
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
    Boundary,
}

impl EditCode {
    /// The stable `err.code` string used by `src/vrml/edit.js`.
    pub fn as_str(self) -> &'static str {
        match self {
            EditCode::Shape => "EEDITSHAPE",
            EditCode::Bounds => "EEDITBOUNDS",
            EditCode::Overlap => "EEDITOVERLAP",
            EditCode::Ambiguous => "EEDITAMBIGUOUS",
            EditCode::Boundary => "EEDITBOUNDARY",
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

fn is_insertion(e: &Edit) -> bool {
    e.from == e.to
}

fn compare_edits(a: &Edit, b: &Edit) -> Ordering {
    a.from
        .cmp(&b.from)
        .then_with(|| (!is_insertion(a)).cmp(&!is_insertion(b)))
        .then_with(|| a.to.cmp(&b.to))
        .then_with(|| a.insert.encode_utf16().cmp(b.insert.encode_utf16()))
}

/// `normalizeEdits`: validate and return caller indexes in canonical order.
/// `text_len` is `None` for the text-free mapping functions.
fn normalize(edits: &[Edit], text_len: Option<usize>) -> Result<Vec<usize>, EditError> {
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

    let mut max_end: Option<(usize, usize)> = None; // (end, caller index)
    let mut insertions: Vec<(usize, usize)> = Vec::new(); // (offset, caller index)
    for &i in &order {
        let e = &edits[i];
        let inside = |m: Option<(usize, usize)>| m.filter(|&(end, _)| e.from < end);
        if is_insertion(e) {
            if let Some(&(_, seen)) = insertions.iter().find(|&&(o, _)| o == e.from) {
                return Err(err(EditCode::Ambiguous, i, Some(seen)));
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

/// `applyEdits`: atomic; any refusal produces no output at all.
pub fn apply_edits(text: &str, edits: &[Edit]) -> Result<String, EditError> {
    let order = normalize(edits, Some(utf16_len(text)))?;
    // Additive Rust-only check, after every JavaScript check has passed.
    let mut cursor = Utf16Cursor::new(text);
    let mut spans = Vec::with_capacity(order.len());
    for &i in &order {
        let e = &edits[i];
        let from = cursor.seek(e.from).map_err(|x| boundary(x, i))?;
        let to = cursor.seek(e.to).map_err(|x| boundary(x, i))?;
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

fn boundary(e: OffsetError, index: usize) -> EditError {
    match e {
        OffsetError::InsideSurrogatePair { .. } => err(EditCode::Boundary, index, None),
        // Bounds were already proven by `normalize`; reaching this is a bug.
        _ => unreachable!("offset error after bounds validation: {e:?}"),
    }
}

/// `mapOffset`: pure UTF-16 arithmetic over a validated edit set.
pub fn map_offset(offset: usize, edits: &[Edit], affinity: Affinity) -> Result<usize, EditError> {
    let order = normalize(edits, None)?;
    let after = affinity == Affinity::After;
    // Signed shift: deletions move later offsets backwards.
    let mut shift: isize = 0;
    let at = |base: usize, shift: isize| (base as isize + shift) as usize;
    for &i in &order {
        let e = &edits[i];
        let n = utf16_len(&e.insert);
        if is_insertion(e) {
            if offset < e.from {
                return Ok(at(offset, shift));
            }
            if offset == e.from {
                return Ok(at(offset, shift) + if after { n } else { 0 });
            }
            shift += n as isize;
            continue;
        }
        if offset < e.from {
            return Ok(at(offset, shift));
        }
        if offset < e.to {
            return Ok(at(e.from, shift) + if after { n } else { 0 });
        }
        shift += n as isize - (e.to - e.from) as isize;
    }
    Ok(at(offset, shift))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(from: usize, to: usize, insert: &str) -> Edit {
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
    }

    #[test]
    fn outside_bytes_are_untouched_including_crlf() {
        let t = "a\r\nb\rc\n";
        assert_eq!(apply_edits(t, &[e(3, 4, "B")]).unwrap(), "a\r\nB\rc\n");
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
    fn same_offset_insertions_are_ambiguous() {
        let r = apply_edits("abc", &[e(1, 1, "x"), e(1, 1, "y")]).unwrap_err();
        assert_eq!(r.code, EditCode::Ambiguous);
        assert_eq!((r.index, r.other_index), (Some(1), Some(0)));
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
    fn bounds_and_shape() {
        assert_eq!(
            apply_edits("ab", &[e(1, 3, "")]).unwrap_err().code,
            EditCode::Bounds
        );
        assert_eq!(
            apply_edits("ab", &[e(2, 1, "")]).unwrap_err().code,
            EditCode::Shape
        );
    }

    #[test]
    fn astral_offsets_count_two_units() {
        let t = "a😀b";
        assert_eq!(apply_edits(t, &[e(1, 3, "é")]).unwrap(), "aéb");
        let r = apply_edits(t, &[e(2, 2, "x")]).unwrap_err();
        assert_eq!((r.code, r.index), (EditCode::Boundary, Some(0)));
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
    }
}
