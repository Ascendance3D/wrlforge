// SPDX-License-Identifier: GPL-3.0-or-later
//! Ports of the `test/vrml/field-edit.test.js` contract cases, plus the Rust
//! lane's span-identity refusals. Every ready plan is checked byte for byte:
//! everything outside the edit spans must be identical.

use super::*;
use crate::node_schema::get_field_schema;

const H: &str = "#VRML V2.0 utf8\n";

/// The nth node of a type in walk order: its exact UTF-16 span.
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

fn text(v: &[&str]) -> Vec<Input> {
    v.iter().map(|s| Input::Text(s.to_string())).collect()
}

fn plan_nth(src: &str, ty: &str, field: &str, components: Vec<Input>, nth: usize) -> Plan {
    let span = node_span(src, ty, nth);
    plan_field_edit(
        src,
        &Request {
            node_from: span.0,
            node_to: span.1,
            field_index: field_index(src, span, field),
            field_name: field.into(),
            components,
        },
    )
}

fn plan(src: &str, ty: &str, field: &str, components: Vec<Input>) -> Plan {
    plan_nth(src, ty, field, components, 0)
}

/// Walk old and new text in lockstep over the edits.
fn assert_only_spans_changed(old: &str, new: &str, edits: &[SpanEdit]) {
    let o: Vec<u16> = old.encode_utf16().collect();
    let n: Vec<u16> = new.encode_utf16().collect();
    let (mut oc, mut nc) = (0usize, 0usize);
    for e in edits {
        let gap = &o[oc..e.from as usize];
        assert_eq!(&n[nc..nc + gap.len()], gap, "bytes before a span untouched");
        nc += gap.len();
        let ins: Vec<u16> = e.insert.encode_utf16().collect();
        assert_eq!(&n[nc..nc + ins.len()], &ins[..], "span holds its insert");
        nc += ins.len();
        oc = e.to as usize;
    }
    assert_eq!(&n[nc..], &o[oc..], "bytes after the last span untouched");
}

fn ready(p: Plan, old: &str) -> (Vec<SpanEdit>, String) {
    match p {
        Plan::Ready {
            edits, new_text, ..
        } => {
            assert_only_spans_changed(old, &new_text, &edits);
            (edits, new_text)
        }
        other => panic!("expected ready, got {other:?}"),
    }
}

fn refused(p: Plan) -> &'static str {
    match p {
        Plan::Refused { reason, .. } => reason,
        other => panic!("expected refusal, got {other:?}"),
    }
}

#[test]
fn nine_type_matrix_matches_the_js_cases() {
    // (type, node, field, source, input, expected source)
    type Case = (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        Vec<Input>,
        &'static str,
    );
    let cases: &[Case] = &[
        (
            "SFBool",
            "DirectionalLight",
            "on",
            "DirectionalLight { on TRUE }",
            vec![Input::Bool(false)],
            "DirectionalLight { on FALSE }",
        ),
        (
            "SFInt32",
            "Switch",
            "whichChoice",
            "Switch { whichChoice -1 }",
            text(&["0"]),
            "Switch { whichChoice 0 }",
        ),
        (
            "SFFloat",
            "Sphere",
            "radius",
            "Sphere { radius 1 }",
            text(&["2.5"]),
            "Sphere { radius 2.5 }",
        ),
        (
            "SFTime",
            "TimeSensor",
            "cycleInterval",
            "TimeSensor { cycleInterval 1 }",
            text(&["4.25"]),
            "TimeSensor { cycleInterval 4.25 }",
        ),
        (
            "SFVec2f",
            "TextureTransform",
            "translation",
            "TextureTransform { translation 0 0 }",
            text(&["0.5", "0"]),
            "TextureTransform { translation 0.5 0 }",
        ),
        (
            "SFVec3f",
            "Transform",
            "translation",
            "Transform { translation 0 0 0 }",
            text(&["3", "0", "0"]),
            "Transform { translation 3 0 0 }",
        ),
        (
            "SFColor",
            "Material",
            "diffuseColor",
            "Material { diffuseColor 0.8 0.8 0.8 }",
            text(&["1", "0", "0.8"]),
            "Material { diffuseColor 1 0 0.8 }",
        ),
        (
            "SFRotation",
            "Transform",
            "rotation",
            "Transform { rotation 0 0 1 0 }",
            text(&["0", "1", "0", "1.5708"]),
            "Transform { rotation 0 1 0 1.5708 }",
        ),
        (
            "SFString",
            "WorldInfo",
            "title",
            "WorldInfo { title \"Old\" }",
            text(&["New \"quoted\" \\ title"]),
            "WorldInfo { title \"New \\\"quoted\\\" \\\\ title\" }",
        ),
    ];
    for (ty, node, field, src, input, expect) in cases {
        assert_eq!(get_field_schema(node, field).unwrap().field_type, *ty);
        let src = format!("{H}{src}\n");
        let span = node_span(&src, node, 0);
        let info = inspect_node_fields(&parse(&src), &src, span.0, span.1);
        let fd = info.fields.iter().find(|f| f.name == *field).unwrap();
        assert!(fd.editable, "{ty}: {}", fd.reason);
        assert_eq!(fd.field_type, Some(*ty));
        assert_eq!(fd.components.len(), type_spec(ty).unwrap().arity());
        let (_, new) = ready(plan(&src, node, field, input.clone()), &src);
        assert_eq!(new, format!("{H}{expect}\n"), "{ty}");
    }
    assert_eq!(EDITABLE_TYPES.len(), 9);
    assert!(EDITABLE_TYPES.iter().all(|t| type_spec(t).is_some()));
    assert!(type_spec("MFVec3f").is_none() && type_spec("SFNode").is_none());
}

#[test]
fn hex_int32_and_single_component_patch() {
    let src = format!("{H}Switch {{ whichChoice 0 }}\n");
    let (_, new) = ready(plan(&src, "Switch", "whichChoice", text(&["0x1F"])), &src);
    assert!(new.contains("whichChoice 0x1F"));
    let src = format!("{H}Transform {{ translation 1 2 3 }}\n");
    let (edits, new) = ready(
        plan(&src, "Transform", "translation", text(&["1", "9", "3"])),
        &src,
    );
    assert_eq!(edits.len(), 1);
    assert!(new.contains("translation 1 9 3"));
    let (edits, _) = ready(
        plan(&src, "Transform", "translation", text(&["7", "2", "8"])),
        &src,
    );
    assert_eq!(edits.len(), 2);
    assert_eq!(
        plan(&src, "Transform", "translation", text(&["1", " 2 ", "3"])),
        Plan::Unchanged
    );
}

#[test]
fn trivia_comments_commas_crlf_exponents_survive() {
    let src = "#VRML V2.0 utf8\r\n# head\r\nTransform {\r\n  translation 1e0 , # c1\r\n 2.50E+1\t\t3 # tail\r\n  scale 1 1 1\r\n}\r\n";
    let (_, new) = ready(
        plan(
            src,
            "Transform",
            "translation",
            text(&["1e0", "2.50E+1", "-4"]),
        ),
        src,
    );
    assert_eq!(new, src.replace("\t\t3 #", "\t\t-4 #"));
    assert_eq!(new.matches("\r\n").count(), src.matches("\r\n").count());
    assert!(!new.replace("\r\n", "").contains('\n'));
}

#[test]
fn bom_lone_cr_and_non_ascii_survive() {
    let src = "\u{FEFF}#VRML V2.0 utf8\r# é😀 ünïcödé\rDEF Ä Transform { translation 0 0 0 }\rWorldInfo { title \"日本😀\" }\r";
    let (edits, new) = ready(
        plan(src, "Transform", "translation", text(&["0", "2", "0"])),
        src,
    );
    assert_eq!(edits.len(), 1);
    assert_eq!(new, src.replace("translation 0 0 0", "translation 0 2 0"));
    assert!(new.starts_with('\u{FEFF}'));
    let (_, new) = ready(plan(src, "WorldInfo", "title", text(&["Ünï😀"])), src);
    assert_eq!(new, src.replace("\"日本😀\"", "\"Ünï😀\""));
    assert_eq!(new.matches('\r').count(), src.matches('\r').count());
}

#[test]
fn identical_siblings_nested_and_def_nodes_edit_only_the_target() {
    let src = format!("{H}Group {{ children [ Material {{ diffuseColor 1 1 1 }} Material {{ diffuseColor 1 1 1 }} ] }}\n");
    let (_, new) = ready(
        plan_nth(&src, "Material", "diffuseColor", text(&["0", "1", "1"]), 1),
        &src,
    );
    assert_eq!(
        new,
        src.replacen("diffuseColor 1 1 1 } ]", "diffuseColor 0 1 1 } ]", 1)
    );
    assert!(new.contains("Material { diffuseColor 1 1 1 } Material { diffuseColor 0 1 1 }"));
    let src = format!(
        "{H}Transform {{ translation 1 1 1 children Transform {{ translation 1 1 1 }} }}\n"
    );
    let (_, new) = ready(
        plan_nth(&src, "Transform", "translation", text(&["5", "1", "1"]), 1),
        &src,
    );
    assert!(
        new.contains("Transform { translation 1 1 1 children Transform { translation 5 1 1 } }")
    );
    let src =
        format!("{H}DEF T Transform {{ translation 0 0 0 }} Transform {{ children USE T }}\n");
    let (_, new) = ready(
        plan(&src, "Transform", "translation", text(&["1", "0", "0"])),
        &src,
    );
    assert!(new.contains("DEF T Transform { translation 1 0 0 }"));
}

#[test]
fn proto_scope_use_and_unsupported_types_refuse() {
    // Stricter than JS: no WD1.5 scope graph in Rust yet.
    let src =
        format!("{H}PROTO P [ field SFFloat r 1 ] {{ Sphere {{ radius 2 }} }}\nP {{ r 3 }}\n");
    assert_eq!(
        refused(plan(&src, "Sphere", "radius", text(&["4"]))),
        reason::NODE_IN_PROTO_SCOPE
    );
    let span = node_span(&src, "P", 0);
    assert_eq!(
        inspect_node_fields(&parse(&src), &src, span.0, span.1).reason,
        reason::UNKNOWN_NODE_TYPE
    );
    let src = format!(
        "{H}PROTO Sphere [ field SFFloat radius 1 ] {{ Group {{}} }}\nSphere {{ radius 3 }}\n"
    );
    assert_eq!(
        refused(plan_nth(&src, "Sphere", "radius", text(&["4"]), 0)),
        reason::PROTO_INSTANCE
    );
    // MF / SFNode / IS / event / X3D-only / duplicated / unknown.
    let src = format!("{H}Shape {{ geometry IndexedFaceSet {{ coordIndex [ 0 1 2 ] }} appearance Appearance {{}} }}\n");
    let span = node_span(&src, "IndexedFaceSet", 0);
    let f = &inspect_node_fields(&parse(&src), &src, span.0, span.1).fields[0];
    assert_eq!((f.editable, f.reason), (false, reason::TYPE_UNSUPPORTED));
    let span = node_span(&src, "Shape", 0);
    let fs = inspect_node_fields(&parse(&src), &src, span.0, span.1).fields;
    assert!(fs.iter().all(|f| f.reason == reason::TYPE_UNSUPPORTED));
    let src = format!(
        "{H}Transform {{ translation 1 2 3 translation 4 5 6 bboxDisplay TRUE vendorThing 1 }}\n"
    );
    let span = node_span(&src, "Transform", 0);
    let reasons: Vec<_> = inspect_node_fields(&parse(&src), &src, span.0, span.1)
        .fields
        .iter()
        .map(|f| f.reason)
        .collect();
    assert_eq!(
        reasons,
        [
            reason::FIELD_DUPLICATED,
            reason::FIELD_DUPLICATED,
            reason::FIELD_X3D_ONLY,
            reason::FIELD_UNKNOWN
        ]
    );
}

#[test]
fn use_route_and_unknown_spans_are_not_node_instances() {
    let src =
        format!("{H}DEF T Transform {{}}\nTransform {{ children USE T }}\nROUTE T.a TO T.b\n");
    let p = parse(&src);
    let use_from = src.encode_utf16().count() as u64; // nothing starts at EOF
    assert_eq!(
        inspect_node_fields(&p, &src, use_from, use_from).reason,
        reason::NOT_A_NODE
    );
    let u = src.find("USE T").unwrap() as u64;
    assert_eq!(
        inspect_node_fields(&p, &src, u, u + 5).reason,
        reason::NOT_A_NODE
    );
    let r = plan_field_edit(
        &src,
        &Request {
            node_from: u,
            node_to: u + 5,
            field_index: 0,
            field_name: "children".into(),
            components: vec![],
        },
    );
    assert_eq!(refused(r), reason::NODE_NOT_FOUND);
}

#[test]
fn damaged_documents_are_read_only_but_a_missing_header_is_not() {
    let src = format!("{H}Transform {{ translation 1 2 3 }}\nGroup {{ children [ \n");
    assert_eq!(
        refused(plan(
            &src,
            "Transform",
            "translation",
            text(&["0", "0", "0"])
        )),
        reason::SYNTAX_ERRORS
    );
    let src = "Transform { translation 1 2 3 }\n";
    ready(
        plan(src, "Transform", "translation", text(&["0", "0", "0"])),
        src,
    );
}

#[test]
fn value_shape_must_match_the_schema_type() {
    for v in ["1 2", "[ 1 2 3 ]", "TRUE", "\"x\""] {
        let src = format!("{H}Transform {{ translation {v} }}\n");
        let span = node_span(&src, "Transform", 0);
        let f = &inspect_node_fields(&parse(&src), &src, span.0, span.1).fields[0];
        assert_eq!(f.reason, reason::VALUE_SHAPE, "{v}");
    }
    let src = format!("{H}Switch {{ whichChoice 1.5 }}\n");
    let span = node_span(&src, "Switch", 0);
    assert_eq!(
        inspect_node_fields(&parse(&src), &src, span.0, span.1).fields[0].reason,
        reason::VALUE_TOKEN_INVALID
    );
}

#[test]
fn numeric_input_is_validated_never_clamped() {
    let src = format!(
        "{H}Sphere {{ radius 1 }}\nSwitch {{ whichChoice 0 }}\nMaterial {{ shininess 0.2 }}\n"
    );
    for bad in ["", "abc", "NaN", "Infinity", "1e999", "--1", "1 2", "0x10"] {
        let r = refused(plan(&src, "Sphere", "radius", text(&[bad])));
        assert!(
            [reason::INPUT_NOT_NUMBER, reason::INPUT_NOT_FINITE].contains(&r),
            "{bad}: {r}"
        );
    }
    assert_eq!(
        refused(plan(&src, "Switch", "whichChoice", text(&["1.5"]))),
        reason::INPUT_NOT_INTEGER
    );
    assert_eq!(
        refused(plan(&src, "Switch", "whichChoice", text(&["2147483648"]))),
        reason::INPUT_OUT_OF_RANGE
    );
    // Switch.whichChoice carries the schema bound [-1, infinity).
    assert_eq!(
        refused(plan(&src, "Switch", "whichChoice", text(&["-2"]))),
        reason::INPUT_OUT_OF_RANGE
    );
    ready(plan(&src, "Switch", "whichChoice", text(&["-1"])), &src);
    ready(
        plan(&src, "Switch", "whichChoice", text(&["2147483647"])),
        &src,
    );
    // Material.shininess is [0,1] in the schema.
    assert_eq!(
        refused(plan(&src, "Material", "shininess", text(&["1.5"]))),
        reason::INPUT_OUT_OF_RANGE
    );
    ready(plan(&src, "Material", "shininess", text(&["1"])), &src);
    assert_eq!(
        refused(plan(&src, "Sphere", "radius", vec![Input::Bool(true)])),
        reason::INPUT_NOT_NUMBER
    );
    assert_eq!(
        refused(plan(&src, "Sphere", "radius", text(&["1", "2"]))),
        reason::INPUT_SHAPE
    );
}

#[test]
fn trailing_dot_float_is_annex_a_valid() {
    assert!(is_float_lexeme("1.") && is_float_lexeme(".5") && is_float_lexeme("-1.5e-3"));
    assert!(!is_float_lexeme(".") && !is_float_lexeme("1e") && !is_float_lexeme("e1"));
    assert!(is_int32_lexeme("0xFF") && is_int32_lexeme("-12") && !is_int32_lexeme("0x"));
}

#[test]
fn bool_and_string_input_shapes() {
    let src =
        format!("{H}DirectionalLight {{ on TRUE }}\nWorldInfo {{ title \"a\" info [ \"x\" ] }}\n");
    assert_eq!(
        refused(plan(&src, "DirectionalLight", "on", text(&["FALSE"]))),
        reason::INPUT_NOT_BOOLEAN
    );
    assert_eq!(
        plan(&src, "DirectionalLight", "on", vec![Input::Bool(true)]),
        Plan::Unchanged
    );
    assert_eq!(
        refused(plan(&src, "WorldInfo", "title", vec![Input::Bool(true)])),
        reason::INPUT_NOT_STRING
    );
    for bad in ["a\rb", "a\nb"] {
        assert_eq!(
            refused(plan(&src, "WorldInfo", "title", text(&[bad]))),
            reason::INPUT_STRING_UNENCODABLE
        );
    }
    assert_eq!(encode_string("q\"\\").unwrap(), "\"q\\\"\\\\\"");
}

#[test]
fn identity_reasons_are_span_exact() {
    let src = format!("{H}Transform {{ translation 0 0 0 }}\n");
    let (f, t) = node_span(&src, "Transform", 0);
    let req = |from, to| Request {
        node_from: from,
        node_to: to,
        field_index: 0,
        field_name: "translation".into(),
        components: text(&["1", "0", "0"]),
    };
    assert_eq!(
        refused(plan_field_edit(&src, &req(f, t - 1))),
        reason::NODE_NOT_FOUND
    );
    assert_eq!(
        refused(plan_field_edit(&src, &req(f + 1, t))),
        reason::NODE_NOT_FOUND
    );
    let mut wrong_name = req(f, t);
    wrong_name.field_name = "scale".into();
    assert_eq!(
        refused(plan_field_edit(&src, &wrong_name)),
        reason::FIELD_NOT_PRESENT
    );
    let mut wrong_index = req(f, t);
    wrong_index.field_index = 3;
    assert_eq!(
        refused(plan_field_edit(&src, &wrong_index)),
        reason::FIELD_NOT_PRESENT
    );
}
