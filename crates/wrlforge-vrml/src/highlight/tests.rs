// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::parse;

/// UTF-16 offsets -> the exact source text of each span.
fn slice16(src: &str, from: u32, to: u32) -> String {
    let u: Vec<u16> = src.encode_utf16().collect();
    String::from_utf16(&u[from as usize..to as usize]).unwrap()
}

fn spans(src: &str) -> Vec<(String, &'static str)> {
    let p = parse(src);
    let hs = highlight(&p, src);
    check_well_formed(src, &hs);
    hs.iter()
        .map(|s| (slice16(src, s.from, s.to), s.class.as_str()))
        .collect()
}

fn class_of(src: &str, text: &str) -> Vec<&'static str> {
    spans(src)
        .into_iter()
        .filter(|(t, _)| t == text)
        .map(|(_, c)| c)
        .collect()
}

/// Sorted, non-empty, non-overlapping, inside the text, never splitting a
/// surrogate pair.
fn check_well_formed(src: &str, hs: &[Span]) {
    let u: Vec<u16> = src.encode_utf16().collect();
    let mut prev = 0u32;
    for s in hs {
        assert!(s.from < s.to, "empty span {s:?}");
        assert!(s.from >= prev, "overlap or disorder at {s:?}");
        assert!(s.to as usize <= u.len(), "span past end {s:?}");
        for at in [s.from, s.to] {
            if (at as usize) < u.len() && at > 0 {
                assert!(
                    !(0xDC00..0xE000).contains(&u[at as usize]),
                    "span edge splits a surrogate pair {s:?}"
                );
            }
        }
        prev = s.to;
    }
}

#[test]
fn header_comments_keywords_and_roles() {
    let src = "#VRML V2.0 utf8\n# a comment\nDEF Box1 Transform {\n  translation 1 -2.5 3e2\n  children [ Shape { geometry Box {} } USE Box1 ]\n}\n";
    let s = spans(src);
    assert_eq!(s[0], ("#VRML V2.0 utf8".into(), "header"));
    assert_eq!(s[1], ("# a comment".into(), "comment"));
    assert_eq!(class_of(src, "DEF"), ["keyword"]);
    assert_eq!(class_of(src, "USE"), ["keyword"]);
    assert_eq!(class_of(src, "Box1"), ["def-name", "def-ref"]);
    assert_eq!(class_of(src, "Transform"), ["node-type"]);
    assert_eq!(class_of(src, "Shape"), ["node-type"]);
    assert_eq!(class_of(src, "Box"), ["node-type"]);
    assert_eq!(class_of(src, "translation"), ["field"]);
    assert_eq!(class_of(src, "children"), ["field"]);
    assert_eq!(class_of(src, "geometry"), ["field"]);
    assert_eq!(class_of(src, "1"), ["number"]);
    assert_eq!(class_of(src, "-2.5"), ["number"]);
    assert_eq!(class_of(src, "3e2"), ["number"]);
    assert_eq!(class_of(src, "{").len(), 3);
    assert!(class_of(src, "{").iter().all(|c| *c == "punctuation"));
    assert_eq!(class_of(src, "["), ["punctuation"]);
}

#[test]
fn proto_externproto_interfaces_and_is() {
    let src = "#VRML V2.0 utf8\nPROTO Ball [ field SFFloat r 1 exposedField MFNode kids [] eventIn SFBool set_on eventOut SFTime t ] {\n  Sphere { radius IS r }\n}\nEXTERNPROTO Far [ field SFColor c ] \"far.wrl#Far\"\nBall { r 2 }\n";
    assert_eq!(class_of(src, "PROTO"), ["keyword"]);
    assert_eq!(class_of(src, "EXTERNPROTO"), ["keyword"]);
    assert_eq!(class_of(src, "Ball"), ["node-type", "node-type"]);
    assert_eq!(class_of(src, "Far"), ["node-type"]);
    for kw in ["field", "exposedField", "eventIn", "eventOut", "IS"] {
        assert!(
            class_of(src, kw).iter().all(|c| *c == "keyword"),
            "{kw}: {:?}",
            class_of(src, kw)
        );
    }
    assert_eq!(class_of(src, "SFFloat"), ["field-type"]);
    assert_eq!(class_of(src, "MFNode"), ["field-type"]);
    assert_eq!(class_of(src, "SFColor"), ["field-type"]);
    // `r`: interface name, IS target, and the field name in the instance.
    assert_eq!(class_of(src, "r"), ["field", "field", "field"]);
    assert_eq!(class_of(src, "radius"), ["field"]);
    assert_eq!(class_of(src, "\"far.wrl#Far\""), ["string"]);
}

#[test]
fn route_statement() {
    let src = "#VRML V2.0 utf8\nDEF T TimeSensor {}\nDEF S Script { eventIn SFTime go url \"javascript: function go(){ DEF x }\" }\nROUTE T.cycleTime TO S.go\n";
    assert_eq!(class_of(src, "ROUTE"), ["route"]);
    assert_eq!(class_of(src, "TO"), ["route"]);
    assert_eq!(class_of(src, "T"), ["def-name", "def-ref"]);
    assert_eq!(class_of(src, "S"), ["def-name", "def-ref"]);
    assert_eq!(class_of(src, "cycleTime"), ["field"]);
    // `go` is a Script interface name and a ROUTE event.
    assert_eq!(class_of(src, "go"), ["field", "field"]);
    assert_eq!(class_of(src, "."), ["punctuation", "punctuation"]);
    // Script text is a string: its keywords are never highlighted as VRML.
    let s = spans(src);
    let script = s
        .iter()
        .find(|(t, _)| t.starts_with("\"javascript:"))
        .unwrap();
    assert_eq!(script.1, "string");
    assert!(!s.iter().any(|(t, _)| t == "function" || t == "x"));
}

#[test]
fn strings_with_keywords_and_multiline_strings() {
    let src = "#VRML V2.0 utf8\nWorldInfo { info [ \"DEF USE ROUTE\" \"line one\nline two # not a comment\" ] title \"a \\\"q\\\" b\" }\n";
    let s = spans(src);
    assert!(s
        .iter()
        .any(|(t, c)| t == "\"DEF USE ROUTE\"" && *c == "string"));
    assert!(s
        .iter()
        .any(|(t, c)| t == "\"line one\nline two # not a comment\"" && *c == "string"));
    assert!(s
        .iter()
        .any(|(t, c)| t == "\"a \\\"q\\\" b\"" && *c == "string"));
    assert!(!s.iter().any(|(_, c)| *c == "keyword"));
    assert!(!s.iter().any(|(_, c)| *c == "comment"));
}

#[test]
fn literals() {
    let src = "#VRML V2.0 utf8\nDirectionalLight { on TRUE }\nShape { appearance NULL }\nTouchSensor { enabled FALSE }\n";
    assert_eq!(class_of(src, "TRUE"), ["literal"]);
    assert_eq!(class_of(src, "FALSE"), ["literal"]);
    assert_eq!(class_of(src, "NULL"), ["literal"]);
}

#[test]
fn invalid_numbers_and_unterminated_strings() {
    let src = "#VRML V2.0 utf8\nTransform { translation 1.2.3 4e 5 }\n";
    let s = spans(src);
    assert!(s.iter().any(|(_, c)| *c == "invalid"), "{s:?}");
    let src = "#VRML V2.0 utf8\nWorldInfo { title \"never closed\n}\n";
    let s = spans(src);
    let last = s.last().unwrap();
    assert_eq!(last.1, "invalid");
    assert!(last.0.starts_with("\"never closed"));
}

#[test]
fn incomplete_documents_keep_what_the_parser_placed() {
    // Recovery: unclosed node, missing value, stray brace.
    for src in [
        "#VRML V2.0 utf8\nTransform { translation 1 2",
        "#VRML V2.0 utf8\nTransform { children [ Shape {",
        "#VRML V2.0 utf8\n} Group { }",
        "#VRML V2.0 utf8\nDEF",
        "#VRML V2.0 utf8\nROUTE A.b TO",
        "#VRML V2.0 utf8\nPROTO P [ field SFFloat",
        "",
        "Group {}",
    ] {
        let _ = spans(src);
    }
    assert_eq!(
        class_of("#VRML V2.0 utf8\nTransform { translation 1 2", "Transform"),
        ["node-type"]
    );
}

#[test]
fn unplaced_identifiers_stay_plain() {
    // `foo` sits in a PROTO interface list where only an access keyword is
    // legal; the parser skips it, so no class is invented for it.
    let src = "#VRML V2.0 utf8\nPROTO P [ foo field SFFloat x 1 ] { Group {} }\n";
    let s = spans(src);
    assert!(!s.iter().any(|(t, _)| t == "foo"), "{s:?}");
    assert_eq!(class_of(src, "x"), ["field"]);
}

#[test]
fn crlf_lone_cr_bom_and_unicode_offsets() {
    let src = "\u{FEFF}#VRML V2.0 utf8\r\n# é😀 comment\rDEF Ünï Group {}\r\nWorldInfo { title \"😀😀\" }\n";
    let s = spans(src);
    assert_eq!(s[0], ("#VRML V2.0 utf8".into(), "header"));
    assert_eq!(s[1], ("# é😀 comment".into(), "comment"));
    assert_eq!(class_of(src, "Ünï"), ["def-name"]);
    assert_eq!(class_of(src, "Group"), ["node-type"]);
    assert_eq!(class_of(src, "\"😀😀\""), ["string"]);
    // The BOM is never covered by a span.
    let p = parse(src);
    assert!(highlight(&p, src)[0].from >= 1);
}

#[test]
fn every_committed_vrml_fixture_is_well_formed() {
    let fixtures: &[(&str, &str)] = &[
        (
            "comments",
            include_str!("../../../../test/fixtures/vrml/comments.wrl"),
        ),
        (
            "crlf",
            include_str!("../../../../test/fixtures/vrml/crlf.wrl"),
        ),
        (
            "def-use",
            include_str!("../../../../test/fixtures/vrml/def-use.wrl"),
        ),
        (
            "escapes",
            include_str!("../../../../test/fixtures/vrml/escapes.wrl"),
        ),
        (
            "externproto",
            include_str!("../../../../test/fixtures/vrml/externproto.wrl"),
        ),
        (
            "invalid-number",
            include_str!("../../../../test/fixtures/vrml/invalid-number.wrl"),
        ),
        (
            "invalid-route",
            include_str!("../../../../test/fixtures/vrml/invalid-route.wrl"),
        ),
        (
            "malformed-brace",
            include_str!("../../../../test/fixtures/vrml/malformed-brace.wrl"),
        ),
        (
            "malformed-bracket",
            include_str!("../../../../test/fixtures/vrml/malformed-bracket.wrl"),
        ),
        (
            "mall-item",
            include_str!("../../../../test/fixtures/vrml/mall-item.wrl"),
        ),
        (
            "multiline-script-crlf",
            include_str!("../../../../test/fixtures/vrml/multiline-script-crlf.wrl"),
        ),
        (
            "multiline-script",
            include_str!("../../../../test/fixtures/vrml/multiline-script.wrl"),
        ),
        (
            "nested",
            include_str!("../../../../test/fixtures/vrml/nested.wrl"),
        ),
        (
            "numbers",
            include_str!("../../../../test/fixtures/vrml/numbers.wrl"),
        ),
        (
            "proto",
            include_str!("../../../../test/fixtures/vrml/proto.wrl"),
        ),
        (
            "recovery",
            include_str!("../../../../test/fixtures/vrml/recovery.wrl"),
        ),
        (
            "route",
            include_str!("../../../../test/fixtures/vrml/route.wrl"),
        ),
        (
            "script",
            include_str!("../../../../test/fixtures/vrml/script.wrl"),
        ),
        (
            "unterminated-string",
            include_str!("../../../../test/fixtures/vrml/unterminated-string.wrl"),
        ),
        (
            "real-smartcar-lite",
            include_str!("../../../../test/fixtures/preview/real-smartcar-lite.wrl"),
        ),
    ];
    for (name, src) in fixtures {
        let p = parse(src);
        let hs = highlight(&p, src);
        check_well_formed(src, &hs);
        assert!(!hs.is_empty(), "{name}: no spans");
        // Every span is exactly a token or a comment of this parse.
        for s in &hs {
            let exact = p
                .tokens
                .iter()
                .map(|t| t.range)
                .chain(p.comments.iter().map(|c| c.range))
                .any(|r| r.start.offset == s.from && r.end.offset == s.to);
            assert!(exact, "{name}: span {s:?} is not a source token");
        }
    }
}

#[test]
fn class_codes_round_trip() {
    for (i, c) in Class::ALL.iter().enumerate() {
        assert_eq!(*c as u8 as usize, i);
        assert_eq!(Class::from_code(i as u8), Some(*c));
    }
    assert_eq!(Class::from_code(Class::ALL.len() as u8), None);
}
