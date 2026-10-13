// SPDX-License-Identifier: GPL-3.0-or-later
//! B2: the pure parts of the Source panel's line numbers, Go to Line and
//! Find / Replace. No DOM, no IPC: text in, offsets out, so every rule is
//! unit-tested.
//!
//! All offsets are UTF-16 code units (the `<textarea>`'s and the edit
//! protocol's unit). A match never splits a surrogate pair. Lines are
//! 1-based; LF, CRLF and a lone CR each end ONE line (the editor view is
//! LF-only, but the rules hold for any of them).

/// A match: `[from, to)` in UTF-16 units.
pub type Span = (u32, u32);

const LF: u16 = b'\n' as u16;
const CR: u16 = b'\r' as u16;

/// Lines in `t`: line breaks + 1 (an empty text, or a text ending with a
/// break, has an empty last line the caret can stand on).
pub fn line_count(t: &[u16]) -> u32 {
    let mut n = 1u32;
    let mut i = 0;
    while i < t.len() {
        match t[i] {
            CR => {
                n += 1;
                if t.get(i + 1) == Some(&LF) {
                    i += 1;
                }
            }
            LF => n += 1,
            _ => {}
        }
        i += 1;
    }
    n
}

/// The offset where 1-based `line` starts, or `None` past the last line.
pub fn line_start(t: &[u16], line: u32) -> Option<u32> {
    if line == 0 {
        return None;
    }
    let mut cur = 1u32;
    if cur == line {
        return Some(0);
    }
    let mut i = 0;
    while i < t.len() {
        let brk = match t[i] {
            CR if t.get(i + 1) == Some(&LF) => {
                i += 1;
                true
            }
            CR | LF => true,
            _ => false,
        };
        i += 1;
        if brk {
            cur += 1;
            if cur == line {
                return Some(i as u32);
            }
        }
    }
    None
}

/// Go to Line input -> a line in `1..=total`, or the message to show.
pub fn parse_line(input: &str, total: u32) -> Result<u32, String> {
    let s = input.trim();
    let range = || format!("Enter a line number from 1 to {total}.");
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(range());
    }
    match s.parse::<u32>() {
        Ok(n) if (1..=total).contains(&n) => Ok(n),
        _ => Err(format!("Line {s} is out of range: {}", range())),
    }
}

/// The gutter text for lines `first..=last` (1-based), one per row.
pub fn gutter_text(first: u32, last: u32) -> String {
    let mut s = String::new();
    for n in first..=last {
        if n > first {
            s.push('\n');
        }
        s.push_str(&n.to_string());
    }
    s
}

/// Decimal digits of `n` (the gutter width).
pub fn digits(n: u32) -> u32 {
    n.max(1).ilog10() + 1
}

/// Case folding for "match case" off: a character whose lowercase form is
/// ONE character folds to it; any other (e.g. `İ`) only matches itself, so
/// a match never changes length or misaligns offsets.
fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(a), None) => a,
        _ => c,
    }
}

/// `t` as (UTF-16 offset, char) pairs; a lone surrogate is `None` and never
/// matches anything.
fn chars(t: &[u16]) -> Vec<(u32, Option<char>)> {
    let mut out = Vec::with_capacity(t.len());
    let mut at = 0u32;
    for r in char::decode_utf16(t.iter().copied()) {
        match r {
            Ok(c) => {
                out.push((at, Some(c)));
                at += c.len_utf16() as u32;
            }
            Err(_) => {
                out.push((at, None));
                at += 1;
            }
        }
    }
    out
}

/// Every match of `needle` in `hay`, left to right and NON-overlapping
/// (`aa` in `aaaa` is two matches, at 0 and 2). An empty needle matches
/// nothing.
pub fn find_all(hay: &[u16], needle: &str, match_case: bool) -> Vec<Span> {
    let key = |c: char| if match_case { c } else { fold(c) };
    let pat: Vec<char> = needle.chars().map(key).collect();
    if pat.is_empty() {
        return Vec::new();
    }
    let hc = chars(hay);
    let mut out = Vec::new();
    let mut i = 0;
    while i + pat.len() <= hc.len() {
        let hit = pat
            .iter()
            .zip(&hc[i..])
            .all(|(p, (_, h))| h.is_some_and(|h| key(h) == *p));
        if hit {
            let from = hc[i].0;
            let end = i + pat.len();
            let to = hc.get(end).map_or(hay.len() as u32, |(o, _)| *o);
            out.push((from, to));
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// The match Find Next selects from caret / selection end `pos`: the first
/// starting at or after it, else (with `wrap`) the first. `(index, wrapped)`.
pub fn next_match(m: &[Span], pos: u32, wrap: bool) -> Option<(usize, bool)> {
    let i = m.partition_point(|s| s.0 < pos);
    if i < m.len() {
        Some((i, false))
    } else if wrap && !m.is_empty() {
        Some((0, true))
    } else {
        None
    }
}

/// The match Find Previous selects from selection start `pos`: the last
/// starting before it, else (with `wrap`) the last.
pub fn prev_match(m: &[Span], pos: u32, wrap: bool) -> Option<(usize, bool)> {
    let i = m.partition_point(|s| s.0 < pos);
    if i > 0 {
        Some((i - 1, false))
    } else if wrap && !m.is_empty() {
        Some((m.len() - 1, true))
    } else {
        None
    }
}

/// The match the selection is exactly on, if any (Replace replaces it).
pub fn selected_match(m: &[Span], from: u32, to: u32) -> Option<usize> {
    m.binary_search_by_key(&from, |s| s.0)
        .ok()
        .filter(|&i| m[i].1 == to)
}

/// One view edit `(from, to, insert)`.
pub type Edit = (u32, u32, Vec<u16>);

/// Replace All as ONE edit: the span from the first match's start to the
/// last match's end, whose new text is the old text with every match
/// replaced and everything between matches kept unit-for-unit. `None` when
/// there is nothing to replace.
pub fn replace_all(hay: &[u16], m: &[Span], replacement: &str) -> Option<Edit> {
    let (first, last) = (m.first()?.0, m.last()?.1);
    let rep: Vec<u16> = replacement.encode_utf16().collect();
    let mut out = Vec::with_capacity((last - first) as usize);
    let mut at = first;
    for &(a, z) in m {
        out.extend_from_slice(&hay[at as usize..a as usize]);
        out.extend_from_slice(&rep);
        at = z;
    }
    Some((first, last, out))
}

/// `hay` with the edit applied (the text Rust must acknowledge).
pub fn apply(hay: &[u16], e: &Edit) -> Vec<u16> {
    let mut out = Vec::with_capacity(hay.len() + e.2.len());
    out.extend_from_slice(&hay[..e.0 as usize]);
    out.extend_from_slice(&e.2);
    out.extend_from_slice(&hay[e.1 as usize..]);
    out
}

/// Does `[from, to)` contain a line break (Replace All in a mixed-EOL
/// document is refused across one: the backend writes the dominant EOL).
pub fn has_break(t: &[u16], from: u32, to: u32) -> bool {
    t[from as usize..to as usize]
        .iter()
        .any(|&u| u == LF || u == CR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn line_count_and_start_for_lf_crlf_and_cr() {
        for (text, n) in [
            ("", 1),
            ("a", 1),
            ("a\n", 2),
            ("a\nb", 2),
            ("a\r\nb\r\n", 3),
            ("a\rb\rc", 3),
            ("a\r\n\r\nb", 3),
            ("a\n\rb", 3),
        ] {
            assert_eq!(line_count(&u(text)), n, "{text:?}");
        }
        for text in ["l1\nl2\nl3", "l1\r\nl2\r\nl3", "l1\rl2\rl3"] {
            let t = u(text);
            let w = if text.contains("\r\n") { 4 } else { 3 };
            assert_eq!(line_start(&t, 1), Some(0));
            assert_eq!(line_start(&t, 2), Some(w));
            assert_eq!(line_start(&t, 3), Some(2 * w));
            assert_eq!(line_start(&t, 4), None);
            assert_eq!(line_start(&t, 0), None);
        }
        // The empty last line after a final break is a line.
        assert_eq!(line_start(&u("x\n"), 2), Some(2));
        // UTF-16: an astral character is two units.
        assert_eq!(line_start(&u("😀\nb"), 2), Some(3));
    }

    #[test]
    fn go_to_line_input_is_checked() {
        assert_eq!(parse_line(" 7 ", 10), Ok(7));
        assert_eq!(parse_line("1", 1), Ok(1));
        for bad in ["0", "11", "", "-3", "3.5", "x", "99999999999999"] {
            let e = parse_line(bad, 10).unwrap_err();
            assert!(e.contains("from 1 to 10"), "{bad:?}: {e}");
        }
    }

    #[test]
    fn gutter_text_and_width() {
        assert_eq!(gutter_text(1, 3), "1\n2\n3");
        assert_eq!(gutter_text(9, 9), "9");
        assert_eq!(
            (digits(0), digits(9), digits(10), digits(12345)),
            (1, 1, 2, 5)
        );
    }

    #[test]
    fn find_is_non_overlapping_case_aware_and_rejects_empty() {
        let t = u("aaaa");
        assert_eq!(find_all(&t, "aa", true), [(0, 2), (2, 4)]);
        assert_eq!(find_all(&t, "aaa", true), [(0, 3)]);
        assert!(find_all(&t, "", true).is_empty());
        assert!(find_all(&t, "aaaaa", true).is_empty());
        let t = u("DEF Box def BOX");
        assert_eq!(find_all(&t, "def", true), [(8, 11)]);
        assert_eq!(find_all(&t, "def", false), [(0, 3), (8, 11)]);
        assert_eq!(find_all(&t, "box", false), [(4, 7), (12, 15)]);
        assert!(find_all(&t, "box", true).is_empty());
    }

    #[test]
    fn find_uses_utf16_offsets_and_never_splits_a_pair() {
        let t = u("é😀x😀é");
        assert_eq!(find_all(&t, "😀", true), [(1, 3), (4, 6)]);
        assert_eq!(find_all(&t, "x😀é", true), [(3, 7)]);
        assert_eq!(find_all(&t, "É", false), [(0, 1), (6, 7)]);
        assert!(find_all(&t, "É", true).is_empty());
        // A lone surrogate never matches (and never shifts later offsets).
        let mut t = u("a");
        t.push(0xD800);
        t.extend(u("bab"));
        assert_eq!(find_all(&t, "b", true), [(2, 3), (4, 5)]);
        // A multi-char lowercase (İ) only matches itself.
        assert_eq!(find_all(&u("İi"), "i", false), [(1, 2)]);
        assert_eq!(find_all(&u("İi"), "İ", false), [(0, 1)]);
    }

    #[test]
    fn find_spans_line_breaks_as_text() {
        // The view is LF-only; a CRLF source text is searched unit by unit.
        let t = u("a\r\nb\na");
        assert_eq!(find_all(&t, "a", true), [(0, 1), (5, 6)]);
        assert_eq!(find_all(&t, "b", true), [(3, 4)]);
    }

    #[test]
    fn next_and_previous_wrap_or_stop() {
        let m = [(2, 4), (6, 8), (10, 12)];
        assert_eq!(next_match(&m, 0, true), Some((0, false)));
        assert_eq!(next_match(&m, 2, true), Some((0, false)));
        assert_eq!(next_match(&m, 4, true), Some((1, false)));
        assert_eq!(next_match(&m, 11, true), Some((0, true)));
        assert_eq!(next_match(&m, 11, false), None);
        assert_eq!(prev_match(&m, 6, true), Some((0, false)));
        assert_eq!(prev_match(&m, 12, true), Some((2, false)));
        assert_eq!(prev_match(&m, 2, true), Some((2, true)));
        assert_eq!(prev_match(&m, 2, false), None);
        assert_eq!(next_match(&[], 0, true), None);
        assert_eq!(prev_match(&[], 0, true), None);
        assert_eq!(selected_match(&m, 6, 8), Some(1));
        assert_eq!(selected_match(&m, 6, 7), None);
        assert_eq!(selected_match(&m, 5, 8), None);
    }

    #[test]
    fn replace_all_is_one_edit_keeping_the_text_between_matches() {
        let t = u("x DEF a\nDEF b DEF");
        let m = find_all(&t, "DEF", true);
        assert_eq!(m, [(2, 5), (8, 11), (14, 17)]);
        let e = replace_all(&t, &m, "USE😀").unwrap();
        assert_eq!((e.0, e.1), (2, 17));
        assert_eq!(String::from_utf16(&e.2).unwrap(), "USE😀 a\nUSE😀 b USE😀");
        assert_eq!(
            String::from_utf16(&apply(&t, &e)).unwrap(),
            "x USE😀 a\nUSE😀 b USE😀"
        );
        assert!(has_break(&t, e.0, e.1));
        // One match: the edit is exactly that match.
        let m1 = &m[..1];
        assert_eq!(replace_all(&t, m1, ""), Some((2, 5, vec![])));
        assert!(!has_break(&t, 2, 5));
        assert_eq!(replace_all(&t, &[], "z"), None);
        // Overlapping candidates were never produced, so replacing is exact.
        let t = u("aaaa");
        let e = replace_all(&t, &find_all(&t, "aa", true), "b").unwrap();
        assert_eq!(String::from_utf16(&apply(&t, &e)).unwrap(), "bb");
        // A replacement containing the query does not recurse.
        let t = u("ab ab");
        let e = replace_all(&t, &find_all(&t, "ab", true), "abab").unwrap();
        assert_eq!(String::from_utf16(&apply(&t, &e)).unwrap(), "abab abab");
    }
}
