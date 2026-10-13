// SPDX-License-Identifier: GPL-3.0-or-later
//! B2 Delete / Duplicate contract: exact text results, exact refusals.

use super::*;

const H: &str = "#VRML V2.0 utf8\n";

/// The nth node of a type in walk order: its exact UTF-16 span.
fn span(src: &str, ty: &str, nth: usize) -> (u64, u64) {
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

fn slice16(s: &str, from: u64, to: u64) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16(&u[from as usize..to as usize]).unwrap()
}

fn delete(src: &str, ty: &str, nth: usize) -> String {
    let (f, t) = span(src, ty, nth);
    match plan_delete(src, f, t, false) {
        Plan::Ready {
            edit,
            new_text,
            node,
            ..
        } => {
            assert!(node.is_none());
            assert!(edit.insert.is_empty());
            // Exactly one removed range that contains the node.
            assert!(edit.from <= f && edit.to >= t);
            let mut back = new_text.clone();
            let at = byte_at(&new_text, edit.from);
            back.insert_str(at, &slice16(src, edit.from, edit.to));
            assert_eq!(back, src);
            new_text
        }
        other => panic!("not ready: {other:?}"),
    }
}

fn byte_at(s: &str, u16_off: u64) -> usize {
    crate::field_edit::byte_at(s, u16_off).unwrap()
}

fn delete_reason(src: &str, ty: &str, nth: usize, confirm: bool) -> &'static str {
    let (f, t) = span(src, ty, nth);
    match plan_delete(src, f, t, confirm) {
        Plan::Refused { reason, .. } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn duplicate(
    src: &str,
    ty: &str,
    nth: usize,
    confirm: bool,
) -> (String, (u64, u64), Vec<(String, String)>) {
    let (f, t) = span(src, ty, nth);
    match plan_duplicate(src, f, t, confirm) {
        Plan::Ready {
            edit,
            new_text,
            node,
            renamed,
        } => {
            assert_eq!((edit.from, edit.to), (t, t));
            // A pure insert after the node.
            let at = byte_at(src, t);
            assert_eq!(&new_text[..at], &src[..at]);
            assert_eq!(&new_text[at + edit.insert.len()..], &src[at..]);
            (new_text, node.unwrap(), renamed)
        }
        other => panic!("not ready: {other:?}"),
    }
}

fn duplicate_reason(src: &str, ty: &str, nth: usize, confirm: bool) -> &'static str {
    let (f, t) = span(src, ty, nth);
    match plan_duplicate(src, f, t, confirm) {
        Plan::Refused { reason, .. } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn deletes_a_top_level_statement_with_its_line() {
    let src = format!("{H}Shape {{}}\nGroup {{}}\nTransform {{}}\n");
    assert_eq!(
        delete(&src, "Group", 0),
        format!("{H}Shape {{}}\nTransform {{}}\n")
    );
    assert_eq!(
        delete(&src, "Shape", 0),
        format!("{H}Group {{}}\nTransform {{}}\n")
    );
    // The last line without a terminator takes the preceding one.
    let src = format!("{H}Shape {{}}\nGroup {{}}");
    assert_eq!(delete(&src, "Group", 0), format!("{H}Shape {{}}"));
}

#[test]
fn deletes_children_items_first_middle_last() {
    let src = format!(
        "{H}Group {{\n  children [\n    Shape {{}}\n    Group {{}}\n    Transform {{}}\n  ]\n}}\n"
    );
    let gone = |line: &str| src.replacen(line, "", 1);
    assert_eq!(delete(&src, "Shape", 0), gone("    Shape {}\n"));
    assert_eq!(delete(&src, "Group", 1), gone("    Group {}\n"));
    assert_eq!(delete(&src, "Transform", 0), gone("    Transform {}\n"));
}

#[test]
fn deletes_inline_array_items_cleanly() {
    let src = format!("{H}Group {{ children [ Shape {{}} Group {{}} Transform {{}} ] }}\n");
    assert_eq!(
        delete(&src, "Shape", 0),
        format!("{H}Group {{ children [ Group {{}} Transform {{}} ] }}\n")
    );
    assert_eq!(
        delete(&src, "Transform", 0),
        format!("{H}Group {{ children [ Shape {{}} Group {{}} ] }}\n")
    );
    let src = format!("{H}Group {{ children [Shape {{}} Group {{}}] }}\n");
    assert_eq!(
        delete(&src, "Group", 1),
        format!("{H}Group {{ children [Shape {{}}] }}\n")
    );
    let src = format!("{H}Group {{ children [ Shape {{}}, Group {{}} ] }}\n");
    assert_eq!(
        delete(&src, "Shape", 0),
        format!("{H}Group {{ children [ Group {{}} ] }}\n")
    );
    // The only item: an empty list stays.
    let src = format!("{H}Group {{ children [Shape {{}}] }}\n");
    assert_eq!(
        delete(&src, "Shape", 0),
        format!("{H}Group {{ children [] }}\n")
    );
}

#[test]
fn delete_preserves_crlf_and_comments() {
    let src = "#VRML V2.0 utf8\r\n# keep me\r\nGroup {\r\n  children [\r\n    # first\r\n    Shape {} # trailing\r\n    Transform {}\r\n  ]\r\n}\r\n";
    // A trailing comment is not the node's: the line stays.
    assert_eq!(
        delete(src, "Shape", 0),
        src.replacen("    Shape {} # trailing", "    # trailing", 1)
    );
    assert_eq!(
        delete(src, "Transform", 0),
        src.replacen("    Transform {}\r\n", "", 1)
    );
    assert!(!delete(src, "Transform", 0).contains("\n\n"));
}

#[test]
fn delete_refuses_a_referenced_def() {
    let src = format!("{H}DEF A Shape {{}}\nUSE A\n");
    assert_eq!(
        delete_reason(&src, "Shape", 0, false),
        reason::DEF_REFERENCED
    );
    // A nested DEF counts too, and so does a ROUTE.
    let src = format!(
        "{H}Group {{ children [ DEF T TimeSensor {{}} ] }}\nDEF S Script {{ eventIn SFTime t }}\nROUTE T.time TO S.t\n"
    );
    assert_eq!(
        delete_reason(&src, "Group", 0, false),
        reason::DEF_REFERENCED
    );
    // A USE inside the deleted node goes with it.
    let src = format!("{H}Group {{ children [ DEF A Shape {{}} USE A ] }}\nShape {{}}\n");
    assert_eq!(delete(&src, "Group", 0), format!("{H}Shape {{}}\n"));
}

#[test]
fn proto_body_needs_consent_and_keeps_its_type_node() {
    let src = format!(
        "{H}PROTO P [] {{\n  Group {{\n    children [\n      Shape {{}}\n    ]\n  }}\n  Transform {{}}\n}}\nP {{}}\n"
    );
    assert_eq!(
        delete_reason(&src, "Shape", 0, false),
        reason::SHARED_DEFINITION_UNCONFIRMED
    );
    assert_eq!(
        duplicate_reason(&src, "Shape", 0, false),
        reason::SHARED_DEFINITION_UNCONFIRMED
    );
    let (f, t) = span(&src, "Shape", 0);
    assert!(matches!(plan_delete(&src, f, t, true), Plan::Ready { .. }));
    assert_eq!(
        delete_reason(&src, "Group", 0, true),
        reason::PROTO_TYPE_NODE
    );
    let (f, t) = span(&src, "Transform", 0);
    assert!(matches!(plan_delete(&src, f, t, true), Plan::Ready { .. }));
    // A PROTO instance itself is a plain statement.
    let (f, t) = span(&src, "P", 0);
    assert!(matches!(plan_delete(&src, f, t, false), Plan::Ready { .. }));
}

#[test]
fn interface_defaults_and_field_values_are_refused() {
    let src = format!("{H}PROTO P [ field SFNode n Group {{}} ] {{ Shape {{}} }}\n");
    assert_eq!(
        delete_reason(&src, "Group", 0, true),
        reason::NODE_IN_PROTO_SCOPE
    );
    let src = format!("{H}Shape {{ geometry Box {{}} }}\n");
    assert_eq!(delete_reason(&src, "Box", 0, false), reason::FIELD_VALUE);
    assert_eq!(duplicate_reason(&src, "Box", 0, false), reason::FIELD_VALUE);
}

#[test]
fn stale_or_damaged_spans_are_refused() {
    let src = format!("{H}Shape {{}}\nGroup {{}}\n");
    let (f, t) = span(&src, "Group", 0);
    for (a, b) in [(f + 1, t), (f, t - 1), (f, t + 1), (0, 3)] {
        assert!(matches!(
            plan_delete(&src, a, b, false),
            Plan::Refused {
                reason: fr::NODE_NOT_FOUND,
                ..
            }
        ));
        assert!(matches!(
            plan_duplicate(&src, a, b, false),
            Plan::Refused {
                reason: fr::NODE_NOT_FOUND,
                ..
            }
        ));
    }
    let bad = format!("{H}Shape {{}}\nGroup {{ children [ }}\n");
    assert_eq!(delete_reason(&bad, "Shape", 0, false), fr::SYNTAX_ERRORS);
}

#[test]
fn duplicates_a_top_level_statement() {
    let src = format!("{H}Shape {{ geometry Box {{}} }}\nGroup {{}}\n");
    let (out, (f, t), renamed) = duplicate(&src, "Shape", 0, false);
    assert_eq!(
        out,
        format!("{H}Shape {{ geometry Box {{}} }}\nShape {{ geometry Box {{}} }}\nGroup {{}}\n")
    );
    assert_eq!(slice16(&out, f, t), "Shape { geometry Box {} }");
    assert!(renamed.is_empty());
}

#[test]
fn duplicates_children_items_with_indent_and_crlf() {
    let src = "#VRML V2.0 utf8\r\nGroup {\r\n  children [\r\n    Shape {\r\n      geometry Box {}\r\n    }\r\n    Group {}\r\n  ]\r\n}\r\n";
    let (out, (f, t), _) = duplicate(src, "Shape", 0, false);
    assert_eq!(
        out,
        src.replacen(
            "    }\r\n    Group",
            "    }\r\n    Shape {\r\n      geometry Box {}\r\n    }\r\n    Group",
            1
        )
    );
    assert_eq!(
        slice16(&out, f, t),
        "Shape {\r\n      geometry Box {}\r\n    }"
    );
    let inline = format!("{H}Group {{ children [ Shape {{}} Group {{}} ] }}\n");
    let (out, _, _) = duplicate(&inline, "Shape", 0, false);
    assert_eq!(
        out,
        format!("{H}Group {{ children [ Shape {{}} Shape {{}} Group {{}} ] }}\n")
    );
}

#[test]
fn duplicate_renames_defs_in_the_copy() {
    let src = format!(
        "{H}DEF Ball Transform {{ children [ DEF Ball_1 Shape {{}} USE Ball_1 ] }}\nUSE Ball\nDEF Ball_2 Group {{}}\n"
    );
    let (out, (f, t), renamed) = duplicate(&src, "Transform", 0, false);
    assert_eq!(
        renamed,
        vec![
            ("Ball".to_string(), "Ball_3".to_string()),
            ("Ball_1".to_string(), "Ball_4".to_string()),
        ]
    );
    assert_eq!(
        slice16(&out, f, t),
        "DEF Ball_3 Transform { children [ DEF Ball_4 Shape {} USE Ball_4 ] }"
    );
    // The original and the outside USE are untouched.
    assert!(out.starts_with(&format!(
        "{H}DEF Ball Transform {{ children [ DEF Ball_1 Shape {{}} USE Ball_1 ] }}\n"
    )));
    assert!(out.contains("\nUSE Ball\n"));
}

#[test]
fn duplicate_refuses_unprovable_renames() {
    // A ROUTE inside the node naming one of its DEFs.
    let src = format!(
        "{H}Group {{ children [ DEF T TimeSensor {{}} DEF S Script {{ eventIn SFTime t }} ] ROUTE T.time TO S.t }}\n"
    );
    assert_eq!(
        duplicate_reason(&src, "Group", 0, false),
        reason::RENAME_NOT_PROVABLE
    );
    // A USE of a name the node also DEFs, that binds to an outside DEF.
    let src = format!("{H}DEF A Shape {{}}\nGroup {{ children [ USE A DEF A Shape {{}} ] }}\n");
    assert_eq!(
        duplicate_reason(&src, "Group", 0, false),
        reason::RENAME_NOT_PROVABLE
    );
    // A PROTO declared inside a node body.
    let src = format!("{H}Group {{ PROTO Q [] {{ Shape {{}} }} }}\n");
    assert_eq!(
        duplicate_reason(&src, "Group", 0, false),
        reason::CONTAINS_DECLARATION
    );
}

#[test]
fn duplicate_keeps_outside_references_and_routes() {
    // A USE of an outside DEF stays as written; a ROUTE between outside
    // nodes is copied verbatim.
    let src =
        format!("{H}DEF M Material {{}}\nShape {{ appearance Appearance {{ material USE M }} }}\n");
    let (out, (f, t), renamed) = duplicate(&src, "Shape", 0, false);
    assert!(renamed.is_empty());
    assert_eq!(
        slice16(&out, f, t),
        "Shape { appearance Appearance { material USE M } }"
    );
}

#[test]
fn duplicate_in_a_proto_body_with_consent() {
    let src = format!("{H}PROTO P [] {{\n  Group {{ children [ Shape {{}} ] }}\n}}\nP {{}}\n");
    let (out, _, _) = duplicate(&src, "Shape", 0, true);
    assert_eq!(
        out,
        format!("{H}PROTO P [] {{\n  Group {{ children [ Shape {{}} Shape {{}} ] }}\n}}\nP {{}}\n")
    );
    // The PROTO's type node may be duplicated (the copy follows it).
    let (out, _, _) = duplicate(&src, "Group", 0, true);
    assert_eq!(out.matches("Group {").count(), 2);
}

#[test]
fn unicode_offsets_are_utf16() {
    let src = format!("{H}WorldInfo {{ title \"π 😀\" }}\nShape {{}}\n");
    assert_eq!(
        delete(&src, "Shape", 0),
        format!("{H}WorldInfo {{ title \"π 😀\" }}\n")
    );
    let (out, (f, t), _) = duplicate(&src, "WorldInfo", 0, false);
    assert_eq!(slice16(&out, f, t), "WorldInfo { title \"π 😀\" }");
}
