// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-editor syntax highlighting (UI-SYNTAX-1): the class registry both
//! sides agree on, its theme-token mapping, and the compact wire encoding of
//! highlight spans.
//!
//! The spans are a PRESENTATION of one parse of one revision. They never
//! carry text, never edit text, and are discarded when the revision moves.

/// Syntax classes in wire-code order (`code == index`), each with the theme
/// token that colours it. The order matches `wrlforge_vrml::highlight::Class`
/// (checked by a backend test). Add at the end, never reorder.
pub const SYNTAX_CLASSES: &[(&str, &str)] = &[
    ("header", "--wf-syntax-keyword"),
    ("comment", "--wf-syntax-comment"),
    ("keyword", "--wf-syntax-keyword"),
    ("route", "--wf-syntax-route"),
    // TRUE / FALSE / NULL: upstream Tokyo Night colours language constants
    // like numbers.
    ("literal", "--wf-syntax-number"),
    ("string", "--wf-syntax-string"),
    ("number", "--wf-syntax-number"),
    ("node-type", "--wf-syntax-node-type"),
    ("field-type", "--wf-syntax-node-type"),
    ("field", "--wf-syntax-field"),
    ("def-name", "--wf-syntax-def-name"),
    ("def-ref", "--wf-syntax-def-name"),
    ("punctuation", "--wf-syntax-punctuation"),
    ("invalid", "--wf-syntax-invalid"),
];

/// Spans beyond this count are not sent (`highlights_truncated`); the rest of
/// the document shows as normal text. Bounds the IPC payload.
pub const MAX_SPANS: usize = 400_000;

/// One decoded span: half-open UTF-16 VIEW offsets and a class code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub from: u64,
    pub to: u64,
    pub class: u8,
}

/// Flat `[gap, len, class]` triples, `gap` measured from the previous span's
/// end. Small numbers keep the JSON compact.
pub fn encode(spans: impl IntoIterator<Item = Span>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut prev = 0u64;
    for s in spans {
        debug_assert!(s.from >= prev && s.to > s.from);
        out.push((s.from - prev) as u32);
        out.push((s.to - s.from) as u32);
        out.push(s.class as u32);
        prev = s.to;
    }
    out
}

/// Decode and validate. Refuses (returns `None` for) anything malformed: a
/// partial triple, an unknown class, an empty span, or a span that ends past
/// `view_len`. Sorted and non-overlapping holds by construction.
pub fn decode(flat: &[u32], view_len: u64) -> Option<Vec<Span>> {
    if !flat.len().is_multiple_of(3) {
        return None;
    }
    let mut out = Vec::with_capacity(flat.len() / 3);
    let mut prev = 0u64;
    for t in flat.chunks_exact(3) {
        let from = prev + t[0] as u64;
        let to = from + t[1] as u64;
        if t[1] == 0 || to > view_len || t[2] as usize >= SYNTAX_CLASSES.len() {
            return None;
        }
        out.push(Span {
            from,
            to,
            class: t[2] as u8,
        });
        prev = to;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let s = vec![
            Span {
                from: 0,
                to: 15,
                class: 0,
            },
            Span {
                from: 16,
                to: 19,
                class: 2,
            },
            Span {
                from: 19,
                to: 20,
                class: 12,
            },
        ];
        let flat = encode(s.clone());
        assert_eq!(flat, vec![0, 15, 0, 1, 3, 2, 0, 1, 12]);
        assert_eq!(decode(&flat, 20), Some(s));
    }

    #[test]
    fn decode_refuses_malformed_input() {
        assert_eq!(decode(&[0, 1], 10), None);
        assert_eq!(decode(&[0, 0, 1], 10), None);
        assert_eq!(decode(&[0, 11, 1], 10), None);
        assert_eq!(decode(&[0, 1, SYNTAX_CLASSES.len() as u32], 10), None);
        assert_eq!(decode(&[], 0), Some(vec![]));
    }

    /// style.css colours every class with exactly its registered token, and
    /// changes nothing but colour (no weight / style / size that would move
    /// glyphs off the textarea's).
    #[test]
    fn stylesheet_paints_each_class_with_its_token_only() {
        let css = include_str!("../../ui/static/style.css");
        for (name, token) in SYNTAX_CLASSES {
            let sel = format!(".tk-{name}");
            let rule = css
                .split('}')
                .find(|r| {
                    r.split('{')
                        .next()
                        .is_some_and(|s| s.split(',').any(|x| x.trim() == sel))
                })
                .unwrap_or_else(|| panic!("no rule for {sel}"));
            let body = rule.split('{').nth(1).unwrap().trim();
            assert_eq!(body, format!("color: var({token});"), "{sel}");
        }
    }

    #[test]
    fn classes_are_unique_and_map_to_syntax_tokens() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, token) in SYNTAX_CLASSES {
            assert!(seen.insert(*name), "duplicate class {name}");
            assert!(token.starts_with("--wf-syntax-"), "{name} -> {token}");
            assert!(
                crate::theme::REQUIRED_TOKENS.contains(token),
                "{name} -> {token} is not a theme token"
            );
        }
    }
}
