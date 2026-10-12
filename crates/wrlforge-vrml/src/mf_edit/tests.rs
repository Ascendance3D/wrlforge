// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-4A list-edit contract. Every ready plan is checked against the
//! EXACT expected text: the diff must be the intended change and nothing else.

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

fn list_of(src: &str, field: &str) -> ListValue {
    let span = node_span(src, "Extrusion", 0);
    let p = parse(src);
    let d = inspect_node_fields(&p, src, span.0, span.1);
    let f = d.fields.iter().find(|f| f.name == field).unwrap();
    assert!(f.editable, "{} not editable: {}", field, f.reason);
    f.list.clone().expect("a list")
}

fn req(src: &str, field: &str, ops: Vec<Op>) -> Request {
    let span = node_span(src, "Extrusion", 0);
    let count = list_of(src, field).tuples.len();
    Request {
        node_from: span.0,
        node_to: span.1,
        field_index: field_index(src, span, field),
        field_name: field.into(),
        expected_count: count,
        ops,
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn set(i: usize, v: &[&str]) -> Op {
    Op::Set {
        index: i,
        components: s(v),
    }
}
fn ins(i: usize, v: &[&str]) -> Op {
    Op::Insert {
        index: i,
        components: s(v),
    }
}
fn del(i: usize) -> Op {
    Op::Delete { index: i }
}

/// Plan, require Ready, check the result equals `want` exactly, the single
/// edit reproduces it, and the node span moved by the length change only.
fn expect(src: &str, field: &str, ops: Vec<Op>, want: &str) -> Vec<Vec<String>> {
    let r = req(src, field, ops);
    match plan_list_edit(src, &r) {
        Plan::Ready {
            edit,
            new_text,
            node_from,
            node_to,
            lexemes,
        } => {
            assert_eq!(new_text, want);
            assert_eq!(
                splice(src, std::slice::from_ref(&edit)).as_deref(),
                Some(want)
            );
            assert_eq!(node_from, r.node_from);
            let delta = want.encode_utf16().count() as i64 - src.encode_utf16().count() as i64;
            assert_eq!(node_to as i64, r.node_to as i64 + delta);
            assert!(edit.from > r.node_from && edit.to < r.node_to);
            assert_eq!(list_of(want, field).lexemes(), lexemes);
            lexemes
        }
        other => panic!("not ready: {other:?}"),
    }
}

fn refusal(src: &str, field: &str, ops: Vec<Op>) -> &'static str {
    match plan_list_edit(src, &req(src, field, ops)) {
        Plan::Refused { reason, .. } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

const EX: &str = "#VRML V2.0 utf8\n# outline\nShape { geometry Extrusion {\n  crossSection [ 1 0, 0.5 -0.866,\n    -0.5 -0.866, -1 0 ] # open\n  spine [ 0 -1 0, 0 1 0 ]\n  ccw FALSE\n} }\n";

#[test]
fn reads_bracketed_bare_and_empty_lists_exactly() {
    let l = list_of(EX, "crossSection");
    assert_eq!(
        l.lexemes(),
        vec![
            s(&["1", "0"]),
            s(&["0.5", "-0.866"]),
            s(&["-0.5", "-0.866"]),
            s(&["-1", "0"])
        ]
    );
    assert!(matches!(l.form, Form::Bracketed { .. }));
    let bare = format!("{H}Extrusion {{ scale 2 .5 orientation 0 1 0 1.57 spine [] }}\n");
    let sc = list_of(&bare, "scale");
    assert_eq!((sc.form, sc.lexemes()), (Form::Bare, vec![s(&["2", ".5"])]));
    assert_eq!(
        list_of(&bare, "orientation").lexemes(),
        vec![s(&["0", "1", "0", "1.57"])]
    );
    assert!(list_of(&bare, "spine").tuples.is_empty());
}

#[test]
fn unsupported_forms_are_read_only_with_a_reason() {
    for (value, why) in [
        ("[ 1 2 3 ]", fr::VALUE_SHAPE),          // not a multiple of 2
        ("[ 1 2, [ 3 4 ] ]", reason::LIST_FORM), // nested
        ("[ \"a\" 1 ]", reason::LIST_FORM),      // string
        ("1 2 3 4", reason::LIST_FORM),          // bare run of two tuples
        ("[ 0x10 1 ]", fr::VALUE_TOKEN_INVALID), // hex is not a float
    ] {
        let src = format!("{H}Extrusion {{ crossSection {value} }}\n");
        let span = node_span(&src, "Extrusion", 0);
        let d = inspect_node_fields(&parse(&src), &src, span.0, span.1);
        assert_eq!(
            (d.fields[0].editable, d.fields[0].reason),
            (false, why),
            "{value}"
        );
        assert!(d.fields[0].list.is_none());
    }
}

#[test]
fn set_one_coordinate_changes_only_that_token() {
    let want = EX.replace("0.5 -0.866,\n", "0.5 -0.75,\n");
    expect(EX, "crossSection", vec![set(1, &["0.5", "-0.75"])], &want);
    let want = EX.replace("0 1 0 ]", "0 2.5 0 ]");
    expect(EX, "spine", vec![set(1, &["0", "2.5", "0"])], &want);
    // The exact typed lexeme is written: no rounding, no normalization.
    let want = EX.replace("[ 1 0,", "[ 1.000000000001 0,");
    expect(
        EX,
        "crossSection",
        vec![set(0, &["1.000000000001", " 0 "])],
        &want,
    );
    // Same text is Unchanged (no revision).
    assert_eq!(
        plan_list_edit(EX, &req(EX, "spine", vec![set(0, &["0", "-1", "0"])])),
        Plan::Unchanged
    );
}

#[test]
fn insert_and_delete_touch_only_one_tuple_and_its_separator() {
    // Middle insert copies the neighboring clean gap.
    let want = EX.replace("[ 1 0, 0.5 -0.866,", "[ 1 0, 0.75 -0.5, 0.5 -0.866,");
    expect(EX, "crossSection", vec![ins(1, &["0.75", "-0.5"])], &want);
    // Insert after the line break reuses the line-break gap.
    let want = EX.replace("0.5 -0.866,\n    -0.5", "0.5 -0.866,\n    0 -1,\n    -0.5");
    expect(EX, "crossSection", vec![ins(2, &["0", "-1"])], &want);
    // Append and prepend.
    let want = EX.replace("-1 0 ] # open", "-1 0, -0.5 0.866 ] # open");
    expect(EX, "crossSection", vec![ins(4, &["-0.5", "0.866"])], &want);
    let want = EX.replace("[ 1 0,", "[ 2 2, 1 0,");
    expect(EX, "crossSection", vec![ins(0, &["2", "2"])], &want);
    // Delete middle / first / last.
    let want = EX.replace("[ 1 0, 0.5 -0.866,\n    ", "[ 1 0, ");
    expect(EX, "crossSection", vec![del(1)], &want);
    let want = EX.replace("[ 1 0, 0.5", "[ 0.5");
    expect(EX, "crossSection", vec![del(0)], &want);
    let want = EX.replace(", -1 0 ] # open", " ] # open");
    expect(EX, "crossSection", vec![del(3)], &want);
}

#[test]
fn empty_and_bare_lists_grow_without_rewriting_existing_tokens() {
    let src = format!("{H}Extrusion {{ spine [] scale 2 .5 }}\n");
    let want = src.replace("spine []", "spine [ 0 0 0 ]");
    expect(&src, "spine", vec![ins(0, &["0", "0", "0"])], &want);
    let src2 = src.replace("spine []", "spine [  ]");
    let want = src2.replace("spine [  ]", "spine [ 0 0 0  ]");
    expect(&src2, "spine", vec![ins(0, &["0", "0", "0"])], &want);
    // Bare value: brackets are inserted around the untouched tokens.
    let want = src.replace("scale 2 .5", "scale [ 2 .5, 1 1 ]");
    expect(&src, "scale", vec![ins(1, &["1", "1"])], &want);
    let want = src.replace("scale 2 .5", "scale [ 1 1, 2 .5 ]");
    expect(&src, "scale", vec![ins(0, &["1", "1"])], &want);
}

#[test]
fn comments_between_points_survive_and_a_comment_inside_a_point_refuses() {
    let src = format!("{H}Extrusion {{ spine [ 0 0 0, # a\n 0 1 0, 0 2 0 ] }}\n");
    // The gap after point 0 holds a comment: delete takes the gap before 1? none;
    // point 0 has no clean gap after and no gap before -> tokens only.
    let want = src.replace("[ 0 0 0, # a", "[ , # a");
    expect(&src, "spine", vec![del(0)], &want);
    // Deleting point 1 removes it and its clean following gap; the comment stays.
    let want = src.replace(" 0 1 0, 0 2 0", " 0 2 0");
    expect(&src, "spine", vec![del(1)], &want);
    let src = format!("{H}Extrusion {{ spine [ 0 0 0, 0 # inside\n 1 0, 0 2 0 ] }}\n");
    assert_eq!(
        refusal(&src, "spine", vec![del(1)]),
        reason::POINT_HAS_COMMENT
    );
    // A Set inside it is fine: only the token changes.
    let want = src.replace("0 # inside\n 1 0", "0 # inside\n 3 0");
    expect(&src, "spine", vec![set(1, &["0", "3", "0"])], &want);
}

#[test]
fn an_outline_session_is_one_edit_and_one_proof() {
    let ops = vec![
        set(1, &["0.6", "-0.9"]),
        ins(2, &["0", "-1"]),
        set(2, &["0.1", "-1.1"]),
        del(4),
    ];
    let want = EX.replace(
        "0.5 -0.866,\n    -0.5 -0.866, -1 0 ]",
        "0.6 -0.9,\n    0.1 -1.1,\n    -0.5 -0.866 ]",
    );
    let lex = expect(EX, "crossSection", ops, &want);
    assert_eq!(lex.len(), 4);
    // A failing later op refuses the whole session; nothing changes.
    let ops = vec![set(0, &["2", "2"]), set(1, &["x", "0"])];
    match plan_list_edit(EX, &req(EX, "crossSection", ops)) {
        Plan::Refused {
            reason,
            op_index,
            component_index,
            ..
        } => {
            assert_eq!(
                (reason, op_index, component_index),
                (fr::INPUT_NOT_NUMBER, Some(1), Some(0))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn closed_outline_and_count_rules() {
    let closed = format!("{H}Extrusion {{ crossSection [ 1 1, 1 -1, -1 -1, -1 1, 1 1 ] spine [ 0 0 0, 0 1 0 ] scale 1 1 }}\n");
    assert!(is_closed(&list_of(&closed, "crossSection").values()));
    assert_eq!(
        refusal(&closed, "crossSection", vec![del(0)]),
        reason::WOULD_OPEN
    );
    assert_eq!(
        refusal(&closed, "crossSection", vec![del(4)]),
        reason::WOULD_OPEN
    );
    assert_eq!(
        refusal(&closed, "crossSection", vec![ins(0, &["0", "0"])]),
        reason::WOULD_OPEN
    );
    assert_eq!(
        refusal(&closed, "crossSection", vec![ins(5, &["0", "0"])]),
        reason::WOULD_OPEN
    );
    // A closed square may lose one corner (a triangle, still closed) ...
    let want = closed.replace("1 1, 1 -1, -1 -1,", "1 1, -1 -1,");
    expect(&closed, "crossSection", vec![del(1)], &want);
    // ... but not two.
    assert_eq!(
        refusal(&closed, "crossSection", vec![del(1), del(1)]),
        reason::TOO_FEW
    );
    assert_eq!(refusal(&closed, "spine", vec![del(0)]), reason::TOO_FEW);
    assert_eq!(refusal(&closed, "scale", vec![del(0)]), reason::TOO_FEW);
    // Open outline: two points minimum.
    let open = format!("{H}Extrusion {{ crossSection [ 0 0, 1 0 ] }}\n");
    assert_eq!(
        refusal(&open, "crossSection", vec![del(0)]),
        reason::TOO_FEW
    );
    // Moving the closing point is an explicit Set: allowed, never repaired.
    let want = closed.replace("-1 1, 1 1 ]", "-1 1, 2 2 ]");
    expect(&closed, "crossSection", vec![set(4, &["2", "2"])], &want);
}

#[test]
fn invalid_values_refuse_with_a_reason() {
    let src = format!("{H}Extrusion {{ scale [ 1 1, 0 2 ] orientation [ 0 0 1 0 ] }}\n");
    for (field, op, why) in [
        ("scale", set(0, &["0", "1"]), fr::INPUT_OUT_OF_RANGE),
        ("scale", set(0, &["-1", "1"]), fr::INPUT_OUT_OF_RANGE),
        ("scale", set(0, &["1e", "1"]), fr::INPUT_NOT_NUMBER),
        ("scale", set(0, &["1e999", "1"]), fr::INPUT_NOT_FINITE),
        ("scale", set(0, &["1"]), fr::INPUT_SHAPE),
        ("scale", set(0, &["NaN", "1"]), fr::INPUT_NOT_NUMBER),
        ("scale", set(9, &["1", "1"]), reason::INDEX),
        (
            "orientation",
            set(0, &["0", "0", "0", "1"]),
            reason::ZERO_AXIS,
        ),
        (
            "orientation",
            ins(1, &["0", "0", "0", "0"]),
            reason::ZERO_AXIS,
        ),
    ] {
        assert_eq!(
            refusal(&src, field, vec![op.clone()]),
            why,
            "{field} {op:?}"
        );
    }
    // An unusual authored value is preserved when its other component changes.
    let want = src.replace("0 2 ]", "0 3 ]");
    expect(&src, "scale", vec![set(1, &["0", "3"])], &want);
    // Angles are unbounded; the axis is never normalized.
    let want = src.replace("[ 0 0 1 0 ]", "[ 0 0 2 7.5 ]");
    expect(
        &src,
        "orientation",
        vec![set(0, &["0", "0", "2", "7.5"])],
        &want,
    );
}

#[test]
fn identity_and_document_gates_refuse() {
    // Stale count.
    let mut r = req(EX, "spine", vec![set(0, &["1", "1", "1"])]);
    r.expected_count = 3;
    assert!(matches!(
        plan_list_edit(EX, &r),
        Plan::Refused {
            reason: reason::COUNT_CHANGED,
            ..
        }
    ));
    // Off-by-one span.
    let mut r = req(EX, "spine", vec![set(0, &["1", "1", "1"])]);
    r.node_to -= 1;
    assert!(matches!(
        plan_list_edit(EX, &r),
        Plan::Refused {
            reason: fr::NODE_NOT_FOUND,
            ..
        }
    ));
    // Wrong field name for the index.
    let mut r = req(EX, "spine", vec![set(0, &["1", "1", "1"])]);
    r.field_name = "crossSection".into();
    assert!(matches!(
        plan_list_edit(EX, &r),
        Plan::Refused {
            reason: fr::FIELD_NOT_PRESENT,
            ..
        }
    ));
    // Syntax error elsewhere.
    let bad = format!("{EX}Group {{ children [ }}\n");
    let r = req(EX, "spine", vec![set(0, &["1", "1", "1"])]);
    assert!(matches!(
        plan_list_edit(&bad, &r),
        Plan::Refused {
            reason: fr::SYNTAX_ERRORS,
            ..
        }
    ));
    // Inside a PROTO body.
    let src = format!(
        "{H}PROTO P [] {{ Shape {{ geometry Extrusion {{ spine [ 0 0 0, 0 1 0 ] }} }} }}\n"
    );
    let span = node_span(&src, "Extrusion", 0);
    let r = Request {
        node_from: span.0,
        node_to: span.1,
        field_index: 0,
        field_name: "spine".into(),
        expected_count: 2,
        ops: vec![set(0, &["1", "1", "1"])],
    };
    assert!(matches!(
        plan_list_edit(&src, &r),
        Plan::Refused {
            reason: fr::NODE_IN_PROTO_SCOPE,
            ..
        }
    ));
    // IS-bound.
    let src =
        format!("{H}PROTO P [ field MFVec3f s [] ] {{ Extrusion {{ spine IS s }} }}\nP {{}}\n");
    let span = node_span(&src, "Extrusion", 0);
    let r = Request {
        node_from: span.0,
        node_to: span.1,
        field_index: 0,
        field_name: "spine".into(),
        expected_count: 0,
        ops: vec![del(0)],
    };
    assert!(matches!(plan_list_edit(&src, &r), Plan::Refused { .. }));
    // Duplicated field.
    let src = format!("{H}Extrusion {{ spine [ 0 0 0, 0 1 0 ] spine [ 0 0 0, 0 2 0 ] }}\n");
    let span = node_span(&src, "Extrusion", 0);
    let r = Request {
        node_from: span.0,
        node_to: span.1,
        field_index: 0,
        field_name: "spine".into(),
        expected_count: 2,
        ops: vec![set(0, &["1", "1", "1"])],
    };
    assert!(matches!(
        plan_list_edit(&src, &r),
        Plan::Refused {
            reason: fr::FIELD_DUPLICATED,
            ..
        }
    ));
    // No operations.
    assert_eq!(refusal(EX, "spine", vec![]), reason::NO_OPS);
}

#[test]
fn crlf_bom_unicode_and_def_use_proto_content_are_preserved() {
    let src = "\u{FEFF}#VRML V2.0 utf8\r\n# Tëst 😀\r\nPROTO Keep [ field SFFloat k 1 ] { Group {} }\r\nDEF Ä Shape { geometry DEF Ex Extrusion {\r\n  crossSection [ 0 0,\r\n    1 0, 1 1 ] # ✓\r\n  spine [ 0 0 0, 0 1 0 ]\r\n} }\r\nTransform { children [ USE Ä ] }\r\nKeep { k 2 }\r\n";
    let want = src.replace("1 0, 1 1 ]", "1 0, 2 1 ]");
    expect(src, "crossSection", vec![set(2, &["2", "1"])], &want);
    // Inserting after a CRLF gap copies that gap (CRLF + indentation).
    let want = src.replace("[ 0 0,\r\n    1 0,", "[ 0 0,\r\n    0.5 0,\r\n    1 0,");
    expect(src, "crossSection", vec![ins(1, &["0.5", "0"])], &want);
    let want = src.replace("[ 0 0,\r\n    1 0,", "[ 1 0,");
    expect(src, "crossSection", vec![del(0)], &want);
}

#[test]
fn add_field_authors_one_absent_field_on_request() {
    let src = format!("{H}Shape {{ geometry Extrusion {{\n  crossSection [ 0 0, 1 0 ]\n  spine [ 0 0 0, 0 1 0 ]\n}} }}\n");
    let span = node_span(&src, "Extrusion", 0);
    let add = |name: &str, value: AddValue| {
        plan_add_field(
            &src,
            &AddRequest {
                node_from: span.0,
                node_to: span.1,
                field_name: name.into(),
                value,
            },
        )
    };
    match add("beginCap", AddValue::Bool(false)) {
        Plan::Ready {
            new_text, node_to, ..
        } => {
            assert_eq!(
                new_text,
                src.replace("0 1 0 ]\n}", "0 1 0 ]\n  beginCap FALSE\n}")
            );
            assert_eq!(
                node_to,
                span.1 + "\n  beginCap FALSE".encode_utf16().count() as u64
            );
        }
        other => panic!("{other:?}"),
    }
    match add(
        "scale",
        AddValue::List(vec![s(&["1", "1"]), s(&["0.5", "0.5"])]),
    ) {
        Plan::Ready { new_text, .. } => assert_eq!(
            new_text,
            src.replace("0 1 0 ]\n}", "0 1 0 ]\n  scale [ 1 1, 0.5 0.5 ]\n}")
        ),
        other => panic!("{other:?}"),
    }
    match add("creaseAngle", AddValue::Number("0.5".into())) {
        Plan::Ready { new_text, .. } => assert!(new_text.contains("  creaseAngle 0.5\n}")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        add("spine", AddValue::List(vec![])),
        Plan::Refused {
            reason: reason::FIELD_ALREADY_AUTHORED,
            ..
        }
    ));
    assert!(matches!(
        add("set_spine", AddValue::List(vec![])),
        Plan::Refused {
            reason: reason::FIELD_NOT_ADDABLE,
            ..
        }
    ));
    assert!(matches!(
        add("metadata", AddValue::Bool(true)),
        Plan::Refused {
            reason: reason::FIELD_NOT_ADDABLE,
            ..
        }
    ));
    assert!(matches!(
        add("solid", AddValue::Number("1".into())),
        Plan::Refused {
            reason: reason::FIELD_NOT_ADDABLE,
            ..
        }
    ));
    assert!(matches!(
        add("creaseAngle", AddValue::Number("-1".into())),
        Plan::Refused {
            reason: fr::INPUT_OUT_OF_RANGE,
            ..
        }
    ));
    // An empty node body.
    let src = format!("{H}Extrusion {{}}\n");
    let span = node_span(&src, "Extrusion", 0);
    match plan_add_field(
        &src,
        &AddRequest {
            node_from: span.0,
            node_to: span.1,
            field_name: "ccw".into(),
            value: AddValue::Bool(false),
        },
    ) {
        Plan::Ready { new_text, .. } => assert_eq!(new_text, src.replace("{}", "{ ccw FALSE }")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn defaults_come_from_the_schema() {
    assert_eq!(
        default_list("Extrusion", "spine"),
        Some(vec![s(&["0", "0", "0"]), s(&["0", "1", "0"])])
    );
    assert_eq!(
        default_list("Extrusion", "scale"),
        Some(vec![s(&["1", "1"])])
    );
    assert_eq!(
        default_list("Extrusion", "orientation"),
        Some(vec![s(&["0", "0", "1", "0"])])
    );
    assert_eq!(
        default_list("Extrusion", "crossSection").map(|l| l.len()),
        Some(5)
    );
}

#[test]
fn single_tuple_fields_are_added_only_on_request() {
    // VISUAL-4B: a Transform without rotation / scale gets exactly the value
    // the user asked for, as the last statement.
    let src = "#VRML V2.0 utf8\nDEF A Transform {\n  translation 1 2 3\n  children [ ]\n}\n";
    let (f, t) = (16u64, src.encode_utf16().count() as u64 - 1);
    let req = |name: &str, v: &[&str]| AddRequest {
        node_from: f,
        node_to: t,
        field_name: name.into(),
        value: AddValue::Tuple(v.iter().map(|s| s.to_string()).collect()),
    };
    let Plan::Ready { new_text, .. } = plan_add_field(src, &req("rotation", &["0", "0", "1", "0"]))
    else {
        panic!("rotation not added");
    };
    assert_eq!(
        new_text,
        "#VRML V2.0 utf8\nDEF A Transform {\n  translation 1 2 3\n  children [ ]\n  rotation 0 0 1 0\n}\n"
    );
    assert!(matches!(
        plan_add_field(src, &req("scale", &["1", "1", "1"])),
        Plan::Ready { .. }
    ));
    for (name, v) in [
        ("rotation", &["0", "0", "0", "1"][..]),
        ("scale", &["1", "0", "1"][..]),
        ("scale", &["1", "1"][..]),
        ("translation", &["1", "1", "1"][..]),
    ] {
        assert!(
            matches!(plan_add_field(src, &req(name, v)), Plan::Refused { .. }),
            "{name} {v:?}"
        );
    }
}
