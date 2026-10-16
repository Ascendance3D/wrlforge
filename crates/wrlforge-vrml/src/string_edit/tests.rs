// SPDX-License-Identifier: GPL-3.0-or-later
//! B2 MFString / texture-assignment contract. Every ready plan is checked
//! against the EXACT expected text: the diff is the intended change only.

use super::*;
use crate::field_edit::{inspect_node_fields, walk_doc};

const H: &str = "#VRML V2.0 utf8\n";

fn node_span(src: &str, ty: &str, nth: usize) -> (u64, u64) {
    let p = parse(src);
    let mut found = Vec::new();
    walk_doc(&p, &mut |a, _| {
        if let Ast::Node(n) = a {
            if n.node_type == ty {
                found.push((n.range.start.offset as u64, n.range.end.offset as u64));
            }
        }
    });
    found[nth]
}

fn field_index(src: &str, span: (u64, u64), name: &str) -> usize {
    let p = parse(src);
    let n = nodes_at(&p, span.0, span.1)[0].node;
    n.fields
        .iter()
        .position(|f| matches!(f, Ast::Field(f) if f.name == name))
        .expect("field is authored")
}

fn shown(src: &str, ty: &str, field: &str) -> StringList {
    let span = node_span(src, ty, 0);
    let p = parse(src);
    let d = inspect_node_fields(&p, src, span.0, span.1);
    let f = d.fields.iter().find(|f| f.name == field).unwrap();
    assert!(f.editable, "{field} not editable: {}", f.reason);
    f.strings.clone().expect("a string list")
}

fn req(src: &str, ty: &str, field: &str, ops: Vec<Op>) -> Request {
    let span = node_span(src, ty, 0);
    Request {
        confirm_shared: false,
        node_from: span.0,
        node_to: span.1,
        field_index: field_index(src, span, field),
        field_name: field.into(),
        expected: shown(src, ty, field).values(),
        ops,
    }
}

fn set(i: usize, v: &str) -> Op {
    Op::Set {
        index: i,
        value: v.into(),
    }
}
fn ins(i: usize, v: &str) -> Op {
    Op::Insert {
        index: i,
        value: v.into(),
    }
}
fn del(i: usize) -> Op {
    Op::Delete { index: i }
}

/// Plan on an ImageTexture url and return the new text (must be Ready).
fn url_edit(src: &str, ops: Vec<Op>) -> String {
    match plan_string_edit(src, &req(src, "ImageTexture", "url", ops)) {
        Plan::Ready { new_text, edit, .. } => {
            // The single edit reproduces the text.
            assert_eq!(splice(src, &[edit]).unwrap(), new_text);
            new_text
        }
        other => panic!("not ready: {other:?}"),
    }
}

fn refusal(p: Plan) -> &'static str {
    match p {
        Plan::Refused { reason, .. } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn tex(url: &str) -> String {
    format!("{H}Shape {{ appearance Appearance {{ texture ImageTexture {{ url {url} }} }} }}\n")
}

#[test]
fn reads_bare_bracketed_and_empty_forms() {
    let l = shown(&tex("\"a.png\""), "ImageTexture", "url");
    assert_eq!(
        (l.form, l.values()),
        (Form::Bare, vec!["a.png".to_string()])
    );
    let l = shown(&tex("[ \"a.png\", \"b.jpg\" ]"), "ImageTexture", "url");
    assert!(matches!(l.form, Form::Bracketed { .. }));
    assert_eq!(l.values(), vec!["a.png", "b.jpg"]);
    assert!(shown(&tex("[]"), "ImageTexture", "url").items.is_empty());
    // WorldInfo.info, Inline.url and Anchor.url are MFString too.
    let src = format!("{H}WorldInfo {{ info [ \"x\" ] title \"t\" }}\n");
    assert_eq!(shown(&src, "WorldInfo", "info").values(), vec!["x"]);
    let src = format!("{H}Inline {{ url \"w.wrl\" }}\nAnchor {{ url [ \"a\" \"b\" ] }}\n");
    assert_eq!(shown(&src, "Inline", "url").values(), vec!["w.wrl"]);
    assert_eq!(shown(&src, "Anchor", "url").values(), vec!["a", "b"]);
}

#[test]
fn set_single_bare_string_keeps_the_form() {
    let src = tex("\"a.png\"");
    assert_eq!(
        url_edit(&src, vec![set(0, "tex/b.png")]),
        tex("\"tex/b.png\"")
    );
}

#[test]
fn set_one_entry_of_a_list_touches_only_its_token() {
    let src = tex("[ \"a.png\",  \"b.png\" ]");
    assert_eq!(
        url_edit(&src, vec![set(1, "c.png")]),
        tex("[ \"a.png\",  \"c.png\" ]")
    );
}

#[test]
fn unchanged_value_is_unchanged() {
    let src = tex("[ \"a.png\" ]");
    assert_eq!(
        plan_string_edit(
            &src,
            &req(&src, "ImageTexture", "url", vec![set(0, "a.png")])
        ),
        Plan::Unchanged
    );
}

#[test]
fn insert_into_bare_brackets_the_value() {
    let src = tex("\"a.png\"");
    assert_eq!(
        url_edit(&src, vec![ins(1, "b.png")]),
        tex("[ \"a.png\" \"b.png\" ]")
    );
    assert_eq!(
        url_edit(&src, vec![ins(0, "b.png")]),
        tex("[ \"b.png\" \"a.png\" ]")
    );
}

#[test]
fn insert_copies_the_lists_own_separator() {
    let src = tex("[ \"a\", \"b\" ]");
    assert_eq!(
        url_edit(&src, vec![ins(2, "c")]),
        tex("[ \"a\", \"b\", \"c\" ]")
    );
    assert_eq!(
        url_edit(&src, vec![ins(1, "c")]),
        tex("[ \"a\", \"c\", \"b\" ]")
    );
    // A one-entry list has no gap to copy: one space.
    let src = tex("[ \"a\" ]");
    assert_eq!(url_edit(&src, vec![ins(1, "c")]), tex("[ \"a\" \"c\" ]"));
    // Empty lists.
    assert_eq!(url_edit(&tex("[]"), vec![ins(0, "c")]), tex("[ \"c\" ]"));
    assert_eq!(url_edit(&tex("[ ]"), vec![ins(0, "c")]), tex("[ \"c\" ]"));
}

#[test]
fn delete_entries_and_the_separator_next_to_them() {
    let src = tex("[ \"a\", \"b\", \"c\" ]");
    assert_eq!(url_edit(&src, vec![del(1)]), tex("[ \"a\", \"c\" ]"));
    assert_eq!(url_edit(&src, vec![del(2)]), tex("[ \"a\", \"b\" ]"));
    assert_eq!(url_edit(&src, vec![del(0)]), tex("[ \"b\", \"c\" ]"));
    // The last entry of a bracketed list keeps the brackets.
    assert_eq!(url_edit(&tex("[ \"a\" ]"), vec![del(0)]), tex("[ ]"));
    // The only value of a bare list becomes an empty list.
    assert_eq!(url_edit(&tex("\"a\""), vec![del(0)]), tex("[ ]"));
    // A comment next to the entry survives.
    let src = tex("[ \"a\" # keep\n \"b\" ]");
    assert_eq!(url_edit(&src, vec![del(1)]), tex("[ \"a\" # keep\n  ]"));
}

#[test]
fn several_operations_are_one_edit() {
    let src = tex("[ \"a\" \"b\" ]");
    let p = plan_string_edit(
        &src,
        &req(
            &src,
            "ImageTexture",
            "url",
            vec![set(0, "x"), del(1), ins(1, "y")],
        ),
    );
    let Plan::Ready {
        new_text, values, ..
    } = p
    else {
        panic!("{p:?}")
    };
    assert_eq!(new_text, tex("[ \"x\" \"y\" ]"));
    assert_eq!(values, vec!["x", "y"]);
}

#[test]
fn escaping_quotes_backslashes_and_unicode() {
    let src = tex("\"a\"");
    assert_eq!(
        url_edit(&src, vec![set(0, r#"say "hi" \ there"#)]),
        tex(r#""say \"hi\" \\ there""#)
    );
    // Unicode, including a non-BMP character (UTF-16 offsets).
    let src = tex("[ \"ä\" \"b\" ]");
    assert_eq!(
        url_edit(&src, vec![set(1, "tëx/🙂.png")]),
        tex("[ \"ä\" \"tëx/🙂.png\" ]")
    );
    assert_eq!(url_edit(&src, vec![del(0)]), tex("[ \"b\" ]"));
    let src = tex("[ \"🙂\" \"b\" ]");
    assert_eq!(url_edit(&src, vec![set(1, "c")]), tex("[ \"🙂\" \"c\" ]"));
    // An escaped authored value is read decoded and kept when unchanged.
    let src = tex(r#"[ "q\"\\x" "b" ]"#);
    assert_eq!(shown(&src, "ImageTexture", "url").values()[0], "q\"\\x");
    assert_eq!(url_edit(&src, vec![set(1, "c")]), tex(r#"[ "q\"\\x" "c" ]"#));
    // A line break cannot be entered.
    assert_eq!(
        refusal(plan_string_edit(
            &src,
            &req(&src, "ImageTexture", "url", vec![set(0, "a\nb")])
        )),
        fr::INPUT_STRING_UNENCODABLE
    );
}

#[test]
fn edit_spans_are_utf16_offsets_after_non_bmp_text() {
    // The astral character is two UTF-16 units; the span must follow them.
    let src = tex("[ \"🙂\" \"b\" ]");
    let Plan::Ready { edit, new_text, .. } = plan_string_edit(
        &src,
        &req(&src, "ImageTexture", "url", vec![set(1, "c")]),
    ) else {
        panic!("not ready");
    };
    let utf16 = |s: &str| s.encode_utf16().count() as u64;
    // Only the text between the quotes is replaced.
    let at = src.find("\"b\"").unwrap() + 1;
    assert_eq!(edit.from, utf16(&src[..at]));
    assert_eq!(edit.to, edit.from + utf16("b"));
    assert_eq!(edit.insert, "c");
    // The exact source: only the string body changed.
    assert_eq!(new_text, tex("[ \"🙂\" \"c\" ]"));
    assert_eq!(splice(&src, &[edit]).unwrap(), new_text);
}

#[test]
fn crlf_documents_keep_their_line_endings() {
    let src = "#VRML V2.0 utf8\r\nShape {\r\n  appearance Appearance {\r\n    texture ImageTexture {\r\n      url [\r\n        \"a.png\"\r\n        \"b.png\"\r\n      ]\r\n    }\r\n  }\r\n}\r\n";
    let out = url_edit(src, vec![ins(2, "c.png")]);
    assert_eq!(
        out,
        src.replace("\"b.png\"", "\"b.png\"\r\n        \"c.png\"")
    );
    let out = url_edit(src, vec![del(0)]);
    assert_eq!(out, src.replace("\"a.png\"\r\n        ", ""));

    // Comments, whitespace, and CRLF outside an edited string are unchanged.
    let src = "#VRML V2.0 utf8\r\nShape { appearance Appearance { texture ImageTexture { url [\r\n  \"a.png\" # comment\r\n  \"b.png\"\r\n] } } }\r\n";
    let out = url_edit(src, vec![set(1, "c.png")]);
    assert_eq!(out, src.replace("\"b.png\"", "\"c.png\""));
}

#[test]
fn stale_spans_and_lists_are_refused() {
    let src = tex("[ \"a\" ]");
    let mut r = req(&src, "ImageTexture", "url", vec![set(0, "b")]);
    r.node_to += 1;
    assert_eq!(refusal(plan_string_edit(&src, &r)), fr::NODE_NOT_FOUND);
    let mut r = req(&src, "ImageTexture", "url", vec![set(0, "b")]);
    r.expected = vec!["other".into()];
    assert_eq!(refusal(plan_string_edit(&src, &r)), reason::LIST_CHANGED);
    let r = req(&src, "ImageTexture", "url", vec![set(3, "b")]);
    assert_eq!(refusal(plan_string_edit(&src, &r)), reason::INDEX);
    let r = req(&src, "ImageTexture", "url", vec![]);
    assert_eq!(refusal(plan_string_edit(&src, &r)), reason::NO_OPS);
    // The wrong field name at that index.
    let mut r = req(&src, "ImageTexture", "url", vec![set(0, "b")]);
    r.field_name = "repeatS".into();
    assert_eq!(refusal(plan_string_edit(&src, &r)), fr::FIELD_NOT_PRESENT);
}

#[test]
fn non_string_forms_and_fields_are_read_only() {
    let src = tex("[ \"a\" 3 ]");
    let span = node_span(&src, "ImageTexture", 0);
    let d = inspect_node_fields(&parse(&src), &src, span.0, span.1);
    assert_eq!(d.fields[0].reason, reason::LIST_FORM);
    let src = format!("{H}PROTO P [ field MFString u \"a\" ] {{ ImageTexture {{ url IS u }} }}\n");
    let span = node_span(&src, "ImageTexture", 0);
    let d = inspect_node_fields(&parse(&src), &src, span.0, span.1);
    assert_eq!(d.fields[0].reason, fr::IS_BOUND);
}

#[test]
fn proto_body_needs_consent() {
    let src = format!(
        "{H}PROTO T [] {{ Shape {{ appearance Appearance {{ texture ImageTexture {{ url \"a.png\" }} }} }} }}\nT {{}}\n"
    );
    let r = req(&src, "ImageTexture", "url", vec![set(0, "b.png")]);
    assert_eq!(
        refusal(plan_string_edit(&src, &r)),
        fr::SHARED_DEFINITION_UNCONFIRMED
    );
    let r = Request {
        confirm_shared: true,
        ..r
    };
    let Plan::Ready { new_text, .. } = plan_string_edit(&src, &r) else {
        panic!()
    };
    assert_eq!(new_text, src.replace("\"a.png\"", "\"b.png\""));

    // Add texture: the same consent gate.
    let src = format!("{H}PROTO T [] {{ Shape {{ appearance Appearance {{ }} }} }}\nT {{}}\n");
    let span = node_span(&src, "Appearance", 0);
    let mut a = AddTextureRequest {
        node_from: span.0,
        node_to: span.1,
        url: "t.png".into(),
        confirm_shared: false,
    };
    assert_eq!(
        refusal(plan_add_texture(&src, &a)),
        fr::SHARED_DEFINITION_UNCONFIRMED
    );
    a.confirm_shared = true;
    assert!(matches!(plan_add_texture(&src, &a), Plan::Ready { .. }));
}

fn add(src: &str, url: &str) -> Plan {
    let span = node_span(src, "Appearance", 0);
    plan_add_texture(
        src,
        &AddTextureRequest {
            node_from: span.0,
            node_to: span.1,
            url: url.into(),
            confirm_shared: false,
        },
    )
}

#[test]
fn add_texture_to_an_appearance() {
    let src = format!("{H}Shape {{ appearance Appearance {{}} geometry Box {{}} }}\n");
    let Plan::Ready { new_text, edit, .. } = add(&src, "tex/wood grain.png") else {
        panic!()
    };
    assert_eq!(
        new_text,
        src.replace(
            "Appearance {}",
            "Appearance { texture ImageTexture { url \"tex/wood grain.png\" } }"
        )
    );
    assert_eq!(splice(&src, &[edit]).unwrap(), new_text);

    // After the material, on the node's own indentation and line endings.
    let src = "#VRML V2.0 utf8\r\nShape {\r\n  appearance Appearance {\r\n    material Material { }\r\n  }\r\n}\r\n";
    let Plan::Ready { new_text, .. } = add(src, "a.png") else {
        panic!()
    };
    assert_eq!(
        new_text,
        src.replace(
            "material Material { }",
            "material Material { }\r\n    texture ImageTexture { url \"a.png\" }"
        )
    );
    // Quotes in the name are escaped.
    let src = format!("{H}Appearance {{ }}\n");
    let Plan::Ready { new_text, .. } = add(&src, "a\"b.png") else {
        panic!()
    };
    assert!(new_text.contains(r#"url "a\"b.png""#));
}

#[test]
fn add_texture_refusals() {
    let src = format!("{H}Appearance {{ texture NULL }}\n");
    assert_eq!(refusal(add(&src, "a.png")), reason::TEXTURE_AUTHORED);
    let src = format!("{H}Appearance {{ }}\n");
    assert_eq!(refusal(add(&src, " ")), reason::URL_EMPTY);
    let src = format!("{H}Material {{ }}\n");
    let span = node_span(&src, "Material", 0);
    let r = AddTextureRequest {
        node_from: span.0,
        node_to: span.1,
        url: "a.png".into(),
        confirm_shared: false,
    };
    assert_eq!(refusal(plan_add_texture(&src, &r)), reason::NOT_APPEARANCE);
    let src = format!("{H}Appearance {{ }}\n");
    let span = node_span(&src, "Appearance", 0);
    let r = AddTextureRequest {
        node_from: span.0 + 1,
        node_to: span.1,
        url: "a.png".into(),
        confirm_shared: false,
    };
    assert_eq!(refusal(plan_add_texture(&src, &r)), fr::NODE_NOT_FOUND);
}
