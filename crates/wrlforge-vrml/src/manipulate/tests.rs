// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A translation-handle planner. Every ready plan is checked byte for
//! byte: the bytes before and after the one moved token are identical.

use super::*;
use crate::create::{template, Primitive};

const H: &str = "#VRML V2.0 utf8\n";

/// Every node of `ty` in walk order: its exact UTF-16 span.
fn spans(src: &str, ty: &str) -> Vec<(u64, u64)> {
    let p = parse(src);
    let mut out = Vec::new();
    for s in &p.tree.statements {
        visit(s, &mut |a| {
            if let Ast::Node(n) = a {
                if n.node_type == ty {
                    out.push((n.range.start.offset as u64, n.range.end.offset as u64));
                }
            }
        });
    }
    out
}

fn utf16_to_byte(s: &str, u: u64) -> usize {
    let mut n = 0u64;
    for (i, c) in s.char_indices() {
        if n == u {
            return i;
        }
        n += c.len_utf16() as u64;
    }
    s.len()
}

/// Apply a ready plan and prove it changed exactly one token: returns the
/// new text and the (old, new) token.
fn applied(src: &str, plan: &Plan) -> (String, String, String) {
    let Plan::Ready {
        edits, new_text, ..
    } = plan
    else {
        panic!("not ready: {plan:?}");
    };
    assert_eq!(edits.len(), 1, "exactly one token edit");
    let e = &edits[0];
    let (a, b) = (utf16_to_byte(src, e.from), utf16_to_byte(src, e.to));
    // Unchanged regions, byte for byte.
    assert_eq!(&new_text.as_bytes()[..a], &src.as_bytes()[..a]);
    let tail = &src.as_bytes()[b..];
    assert_eq!(&new_text.as_bytes()[new_text.len() - tail.len()..], tail);
    (new_text.clone(), src[a..b].to_string(), e.insert.clone())
}

fn world(eol: &str) -> String {
    format!(
        "#VRML V2.0 utf8{eol}{}{eol}",
        template(Primitive::Box, "Box_1", eol)
    )
}

fn box_span(src: &str) -> (u64, u64) {
    spans(src, "Transform")[0]
}

fn translation_of(src: &str) -> [f64; 3] {
    let (f, t) = box_span(src);
    translate_target(src, f, t).unwrap().translation
}

#[test]
fn the_visual1_box_is_a_target_at_its_translation() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    let tg = translate_target(&src, f, t).unwrap();
    assert_eq!(tg.def.as_deref(), Some("Box_1"));
    assert_eq!(tg.root_index, Some(0));
    assert_eq!(tg.translation, [0.0, 0.0, 0.0]);
    assert_eq!(tg.origin, [0.0, 0.0, 0.0]);
    assert_eq!(tg.lexemes, ["0", "0", "0"].map(String::from));
}

#[test]
fn each_axis_moves_alone_in_both_directions() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    for (axis, i) in [(Axis::X, 0), (Axis::Y, 1), (Axis::Z, 2)] {
        for v in [2.5, -1.25] {
            let (plan, text) = plan_translate(&src, f, t, axis, v, 4);
            let (out, old, new) = applied(&src, &plan);
            assert_eq!(
                (old.as_str(), new.as_str()),
                ("0", text.as_deref().unwrap())
            );
            let mut want = [0.0; 3];
            want[i] = v;
            assert_eq!(translation_of(&out), want, "{axis:?} {v}");
        }
    }
}

#[test]
fn rounding_and_zero_distance() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    let (plan, text) = plan_translate(&src, f, t, Axis::X, 1.234_567_89, 3);
    applied(&src, &plan);
    assert_eq!(text.as_deref(), Some("1.235"));
    // Rounds to the current value: nothing changes.
    assert_eq!(
        plan_translate(&src, f, t, Axis::Y, 0.000_04, 4).0,
        Plan::Unchanged
    );
    assert_eq!(
        plan_translate(&src, f, t, Axis::Z, -0.0, 4).0,
        Plan::Unchanged
    );
    // Same value, different spelling: still unchanged.
    let spelled = src.replace("translation 0 0 0", "translation 1.50 0 0");
    let (f2, t2) = box_span(&spelled);
    assert_eq!(
        plan_translate(&spelled, f2, t2, Axis::X, 1.5, 4).0,
        Plan::Unchanged
    );
}

#[test]
fn invalid_values_are_refused_and_change_nothing() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let (plan, _) = plan_translate(&src, f, t, Axis::X, v, 4);
        assert!(
            matches!(
                plan,
                Plan::Refused {
                    reason: reason::VALUE_NOT_FINITE,
                    ..
                }
            ),
            "{v}"
        );
    }
    let (plan, _) = plan_translate(&src, f, t, Axis::X, 1e39, 0);
    assert!(matches!(
        plan,
        Plan::Refused {
            reason: reason::VALUE_OUT_OF_RANGE,
            ..
        }
    ));
}

#[test]
fn the_other_components_keep_their_exact_lexemes() {
    let src = world("\n").replace("translation 0 0 0", "translation +1.0e0  -2.50\t.5");
    let (f, t) = box_span(&src);
    let tg = translate_target(&src, f, t).unwrap();
    assert_eq!(tg.translation, [1.0, -2.5, 0.5]);
    let (plan, _) = plan_translate(&src, f, t, Axis::Y, 3.0, 2);
    let (out, old, new) = applied(&src, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("-2.50", "3"));
    assert!(out.contains("translation +1.0e0  3\t.5"));
}

#[test]
fn line_endings_bom_comments_and_unicode_are_preserved() {
    for eol in ["\n", "\r\n", "\r"] {
        for bom in ["", "\u{feff}"] {
            let body = template(Primitive::Box, "Box_ü", eol).replace(
                &format!("scale 1 1 1{eol}"),
                &format!("scale 1 1 1 # größe ✓ 😀{eol}"),
            );
            let src = format!(
                "{bom}#VRML V2.0 utf8{eol}# 世界 — comment{eol}WorldInfo {{ title \"héllo 😀\" }}{eol}{body}{eol}# end ✓{eol}"
            );
            let (f, t) = box_span(&src);
            let tg = translate_target(&src, f, t).unwrap();
            assert_eq!(tg.def.as_deref(), Some("Box_ü"));
            assert_eq!(tg.root_index, Some(1), "after WorldInfo");
            let (plan, _) = plan_translate(&src, f, t, Axis::Z, -0.75, 2);
            let (out, old, new) = applied(&src, &plan);
            assert_eq!((old.as_str(), new.as_str()), ("0", "-0.75"));
            assert_eq!(out.len(), src.len() + 4);
            assert!(out.starts_with(bom) && out.ends_with(&format!("# end ✓{eol}")));
            assert_eq!(translation_of(&out), [0.0, 0.0, -0.75]);
        }
    }
}

#[test]
fn wrong_nodes_are_refused() {
    let src = world("\n");
    // A Shape (not a Transform), and a span that is not a node at all.
    let shape = spans(&src, "Shape")[0];
    let r = translate_target(&src, shape.0, shape.1).unwrap_err();
    assert_eq!(r.reason, reason::NOT_TRANSFORM);
    let (f, t) = box_span(&src);
    let r = translate_target(&src, f, t - 1).unwrap_err();
    assert_eq!(r.reason, field_edit::reason::NOT_A_NODE);
    let (plan, _) = plan_translate(&src, f + 1, t, Axis::X, 1.0, 2);
    assert!(matches!(
        plan,
        Plan::Refused {
            reason: field_edit::reason::NOT_A_NODE,
            ..
        }
    ));
}

#[test]
fn nested_transforms_are_refused() {
    let src = format!(
        "{H}Transform {{ translation 1 0 0 children [ DEF Inner Transform {{ translation 0 0 0 }} ] }}\n"
    );
    let inner = spans(&src, "Transform")[1];
    let r = translate_target(&src, inner.0, inner.1).unwrap_err();
    assert_eq!(r.reason, reason::NOT_TOP_LEVEL);
}

#[test]
fn identity_preconditions_are_refused() {
    let cases = [
        ("DEF A Transform { translation 0 0 0 } DEF A Group {}", reason::DEF_NOT_UNIQUE),
        ("DEF A Transform { translation 0 0 0 } Group { children USE A }", reason::INSTANCED),
        (
            "DEF A Transform { translation 0 0 0 } DEF P PositionInterpolator {} ROUTE P.value_changed TO A.set_translation",
            reason::ROUTED,
        ),
        ("DEF A Transform { rotation 0 1 0 1 }", reason::NO_TRANSLATION),
        ("DEF A Transform { translation 0 0 0 translation 1 1 1 }", reason::TRANSLATION_READ_ONLY),
        ("DEF A Transform { translation 0 0 0 center 1 }", reason::FRAME_UNREADABLE),
        ("DEF A Transform { translation 0 0 0 ] }", field_edit::reason::SYNTAX_ERRORS),
    ];
    for (body, want) in cases {
        let src = format!("{H}{body}\n");
        let (f, t) = spans(&src, "Transform")[0];
        let got = translate_target(&src, f, t).map(|_| ()).unwrap_err();
        assert_eq!(got.reason, want, "{body}");
        // A refused target never plans an edit.
        let (plan, _) = plan_translate(&src, f, t, Axis::X, 3.0, 2);
        assert!(matches!(plan, Plan::Refused { .. }), "{body}");
    }
    // A ROUTE FROM the Transform is fine (it is not driven by it).
    let src = format!(
        "{H}DEF A Transform {{ translation 0 0 0 }} DEF B Transform {{ translation 0 0 0 }} ROUTE A.translation_changed TO B.set_translation\n"
    );
    let (f, t) = spans(&src, "Transform")[0];
    assert!(translate_target(&src, f, t).is_ok());
}

/// The root index counts top-level NODE statements only (node, USE, NULL);
/// PROTO, EXTERNPROTO and ROUTE add no root node. A Transform without a DEF
/// is movable: it is bound by that index, not by a name.
#[test]
fn root_index_counts_top_level_node_statements_only() {
    let src = format!(
        "{H}PROTO P [] {{ Group {{}} }}\nDEF G Group {{}}\nEXTERNPROTO E [] \"e.wrl\"\nUSE G\nP {{}}\nDEF S TimeSensor {{}}\nDEF T Transform {{ translation 0 0 0 }}\nROUTE S.time TO S.set_startTime\nTransform {{ translation 1 2 3 }}\n"
    );
    let ts = spans(&src, "Transform");
    let a = translate_target(&src, ts[0].0, ts[0].1).unwrap();
    let b = translate_target(&src, ts[1].0, ts[1].1).unwrap();
    // G, USE G, P {}, S, T -> T is index 4; the undefined one is 5.
    assert_eq!((a.root_index, a.def.as_deref()), (Some(4), Some("T")));
    assert_eq!((b.root_index, b.def), (Some(5), None));
    assert_eq!(b.translation, [1.0, 2.0, 3.0]);
    let (plan, _) = plan_translate(&src, ts[1].0, ts[1].1, Axis::Y, -1.5, 2);
    let (out, old, new) = applied(&src, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("2", "-1.5"));
    assert!(out.contains("Transform { translation 1 -1.5 3 }"));
}

/// B3: a PROTO body Transform at the instance's frame is a target of every
/// tool; a commit needs consent (the field-edit node gate), changes exactly
/// the tool's tokens in the BODY text, and the result re-parses to it.
const CHEVY_LIKE: &str = "PROTO Car [ field SFVec3f bboxSize 1 1 1 ] {\n\tGroup {\n\t\tbboxSize IS bboxSize\n\t\tchildren [\n\t\t\tDEF CarXForm Transform { translation 0 0 0 rotation 0 1 0 0 scale 1 1 1 children [ Shape { geometry Box {} } ] }\n\t\t]\n\t}\n}\nCar {}\nTransform { translation 5 0 0 children Car {} }\n";

#[test]
fn proto_body_transforms_are_targets_with_consent() {
    let src = format!("{H}{CHEVY_LIKE}");
    let (f, t) = spans(&src, "Transform")[0];
    for tool in [Tool::Move, Tool::Rotate, Tool::Scale] {
        let tg = manip_target(&src, f, t, tool).unwrap();
        assert_eq!(tg.root_index, None, "a body node is no root node");
        let sh = tg.shared.as_ref().expect("shared definition");
        assert_eq!(
            (sh.proto.as_str(), sh.instances, sh.name_ambiguous),
            ("Car", 2, false)
        );
        assert_eq!(tg.def.as_deref(), Some("CarXForm"));
    }
    // No consent: refused exactly as an Inspector edit, nothing planned.
    for plan in [
        plan_translate(&src, f, t, Axis::X, 1.5, 4).0,
        plan_rotate(&src, f, t, Axis::Y, 0.5).0,
        plan_scale(&src, f, t, None, 2.0).0,
    ] {
        assert!(
            matches!(
                plan,
                Plan::Refused {
                    reason: field_edit::reason::SHARED_DEFINITION_UNCONFIRMED,
                    ..
                }
            ),
            "{plan:?}"
        );
    }
    // With consent: one token of the body changes; the rest is byte-exact.
    let (plan, text) = plan_translate_with(&src, f, t, Axis::X, 1.5, 4, true);
    assert_eq!(text.as_deref(), Some("1.5"));
    let (out, old, new) = applied(&src, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("0", "1.5"));
    assert!(out.contains("DEF CarXForm Transform { translation 1.5 0 0 rotation"));
    let (f2, t2) = spans(&out, "Transform")[0];
    let after = manip_target(&out, f2, t2, Tool::Move).unwrap();
    assert_eq!(after.translation, [1.5, 0.0, 0.0]);
    assert!(after.shared.is_some());

    let (plan, _) = plan_rotate_with(&src, f, t, Axis::Y, 0.5, true);
    let out = applied_any(&src, &plan);
    assert!(out.contains("rotation 0 1 0 0.5 scale"), "{out}");
    let (plan, _) = plan_scale_with(&src, f, t, Some(Axis::Z), 2.0, true);
    let (out, old, new) = applied(&src, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("1", "2"));
    assert!(out.contains("scale 1 1 2 children"), "{out}");
}

#[test]
fn proto_body_transforms_outside_the_instance_frame_are_refused() {
    let cases: [(&str, usize, &str); 8] = [
        // Under a transforming node of the body.
        (
            "PROTO P [] { Transform { translation 1 0 0 children [ Transform { translation 0 0 0 } ] } } P {}",
            1,
            reason::BODY_FRAME,
        ),
        // A Collision proxy is not rendered.
        (
            "PROTO P [] { Collision { proxy Transform { translation 0 0 0 } } } P {}",
            0,
            reason::BODY_FRAME,
        ),
        // A nested PROTO's body.
        (
            "PROTO P [] { PROTO Q [] { Transform { translation 0 0 0 } } Q {} } P {}",
            0,
            reason::BODY_FRAME,
        ),
        // The frame driven through the interface.
        (
            "PROTO P [ eventIn SFRotation spin ] { Transform { translation 0 0 0 set_rotation IS spin } } P {}",
            0,
            reason::BODY_FRAME,
        ),
        // The field itself bound with IS: each instance has its own value.
        (
            "PROTO P [ field SFVec3f at 0 0 0 ] { Transform { translation IS at } } P {}",
            0,
            reason::BODY_FRAME,
        ),
        // An enclosing group USEd again in the body.
        (
            "PROTO P [] { Group { children [ DEF G Group { children [ Transform { translation 0 0 0 } ] } Transform { translation 2 0 0 children USE G } ] } } P {}",
            0,
            reason::BODY_FRAME,
        ),
        // The Transform itself instanced, or animated, inside the body.
        (
            "PROTO P [] { Group { children [ DEF A Transform { translation 0 0 0 } USE A ] } } P {}",
            0,
            reason::INSTANCED,
        ),
        (
            "PROTO P [] { Group { children [ DEF A Transform { translation 0 0 0 } DEF I PositionInterpolator {} ] } ROUTE I.value_changed TO A.set_translation } P {}",
            0,
            reason::ROUTED,
        ),
    ];
    for (body, k, want) in cases {
        let src = format!("{H}{body}\n");
        let (f, t) = spans(&src, "Transform")[k];
        let got = translate_target(&src, f, t).unwrap_err();
        assert_eq!(got.reason, want, "{body}");
        let (plan, _) = plan_translate_with(&src, f, t, Axis::X, 3.0, 2, true);
        assert!(matches!(plan, Plan::Refused { .. }), "{body}");
    }
}

#[test]
fn ambiguous_or_other_proto_scopes_are_refused_even_with_consent() {
    // Two declarations of the name: which one an instance uses is unproven.
    let src = format!(
        "{H}PROTO P [] {{ Transform {{ translation 0 0 0 }} }}\nPROTO P [] {{ Group {{}} }}\nP {{}}\n"
    );
    let (f, t) = spans(&src, "Transform")[0];
    assert_eq!(
        translate_target(&src, f, t).unwrap_err().reason,
        field_edit::reason::PROTO_NAME_AMBIGUOUS
    );
    let (plan, _) = plan_translate_with(&src, f, t, Axis::X, 1.0, 2, true);
    assert!(matches!(plan, Plan::Refused { .. }));
    // An interface default and an EXTERNPROTO-free interface scope: never.
    let src = format!(
        "{H}PROTO P [ field SFNode n Transform {{ translation 0 0 0 }} ] {{ Group {{}} }}\nP {{}}\n"
    );
    let (f, t) = spans(&src, "Transform")[0];
    assert_eq!(
        translate_target(&src, f, t).unwrap_err().reason,
        field_edit::reason::NODE_IN_PROTO_SCOPE
    );
    let (plan, _) = plan_translate_with(&src, f, t, Axis::X, 1.0, 2, true);
    assert!(matches!(
        plan,
        Plan::Refused {
            reason: field_edit::reason::NODE_IN_PROTO_SCOPE,
            ..
        }
    ));
}

#[test]
fn origin_follows_the_frame_fields() {
    // No center: the origin is the translation, whatever the rotation.
    assert_eq!(
        origin_of(
            [1.0, 2.0, 3.0],
            [0.0; 3],
            [0.0, 1.0, 0.0, 1.0],
            [2.0; 3],
            [0.0, 0.0, 1.0, 0.0]
        ),
        Some([1.0, 2.0, 3.0])
    );
    // center (1,0,0), rotation 90 deg about Z: origin = T + C + R(-C).
    let o = origin_of(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_2],
        [1.0; 3],
        [0.0, 0.0, 1.0, 0.0],
    )
    .unwrap();
    assert!((o[0] - 1.0).abs() < 1e-12 && (o[1] + 1.0).abs() < 1e-12 && o[2].abs() < 1e-12);
    let src = format!(
        "{H}DEF A Transform {{ translation 0 0 0 center 1 0 0 rotation 0 0 1 1.5707963267948966 }}\n"
    );
    let (f, t) = spans(&src, "Transform")[0];
    let tg = translate_target(&src, f, t).unwrap();
    assert!((tg.origin[0] - 1.0).abs() < 1e-9 && (tg.origin[1] + 1.0).abs() < 1e-9);
    // A zero rotation axis with a non-zero angle has no frame.
    assert!(origin_of(
        [0.0; 3],
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        [1.0; 3],
        [0.0, 0.0, 1.0, 0.0]
    )
    .is_none());
}

#[test]
fn format_component_writes_plain_vrml_floats() {
    assert_eq!(format_component(1.5, 4).as_deref(), Some("1.5"));
    assert_eq!(format_component(-0.00001, 3).as_deref(), Some("0"));
    assert_eq!(format_component(2.0, 2).as_deref(), Some("2"));
    assert_eq!(
        format_component(-12.345678, 9).as_deref(),
        Some("-12.345678")
    );
    assert_eq!(format_component(1e7, 2).as_deref(), Some("10000000"));
    assert_eq!(format_component(f64::NAN, 2), None);
}

// ---- VISUAL-4B: rotation and scale -----------------------------------------

/// Apply a ready plan (any number of token edits) and prove the bytes
/// outside the field value are unchanged. Returns the new text.
fn applied_any(src: &str, plan: &Plan) -> String {
    let Plan::Ready {
        edits, new_text, ..
    } = plan
    else {
        panic!("not ready: {plan:?}");
    };
    let a = utf16_to_byte(src, edits.first().unwrap().from);
    let b = utf16_to_byte(src, edits.last().unwrap().to);
    assert_eq!(&new_text.as_bytes()[..a], &src.as_bytes()[..a]);
    let tail = &src.as_bytes()[b..];
    assert_eq!(&new_text.as_bytes()[new_text.len() - tail.len()..], tail);
    new_text.clone()
}

fn target_of(src: &str, tool: Tool) -> Target {
    let (f, t) = box_span(src);
    manip_target(src, f, t, tool).unwrap()
}

fn mat(r: [f64; 4]) -> M3 {
    rotation([r[0], r[1], r[2]], r[3]).unwrap()
}

fn mat_mul3(a: &M3, b: &M3) -> M3 {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn assert_mat_close(a: &M3, b: &M3, tol: f64) {
    for r in 0..3 {
        for c in 0..3 {
            assert!((a[r][c] - b[r][c]).abs() <= tol, "{a:?} != {b:?}");
        }
    }
}

#[test]
fn rotate_about_each_world_axis_composes_with_the_authored_rotation() {
    // The VISUAL-1 Box carries `rotation 0 1 0 0`.
    let src = world("\n");
    let (f, t) = box_span(&src);
    for (axis, d) in [(Axis::X, 0.5), (Axis::Y, -1.25), (Axis::Z, 2.0)] {
        let (plan, text) = plan_rotate(&src, f, t, axis, d);
        let out = applied_any(&src, &plan);
        let got = target_of(&out, Tool::Rotate).rotation;
        let u = axis.unit();
        assert_mat_close(&mat(got), &mat([u[0], u[1], u[2], d]), 2e-4);
        assert!(out.contains(&format!("rotation {}", text.unwrap())));
        // Translation, scale and the rest of the node are untouched.
        assert!(out.contains("translation 0 0 0") && out.contains("scale 1 1 1"));
    }
    // Y is parallel to the authored axis: only the angle token changes.
    let (plan, text) = plan_rotate(&src, f, t, Axis::Y, 0.75);
    let (_, old, new) = applied(&src, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("0", "0.75"));
    assert_eq!(text.as_deref(), Some("0 1 0 0.75"));
}

#[test]
fn an_existing_rotation_is_composed_never_euler_converted() {
    let src = world("\n").replace("rotation 0 1 0 0", "rotation 1 0 0 0.5");
    let (f, t) = box_span(&src);
    let (plan, _) = plan_rotate(&src, f, t, Axis::Y, 0.3);
    let out = applied_any(&src, &plan);
    let got = target_of(&out, Tool::Rotate).rotation;
    let want = mat_mul3(&mat([0.0, 1.0, 0.0, 0.3]), &mat([1.0, 0.0, 0.0, 0.5]));
    assert_mat_close(&mat(got), &want, 2e-4);
    // A negative parallel axis: the angle moves the other way, the axis
    // lexemes stay exactly as written (never normalized).
    let neg = world("\n").replace("rotation 0 1 0 0", "rotation 0 -2.0 0 0.3");
    let (f, t) = box_span(&neg);
    let (plan, text) = plan_rotate(&neg, f, t, Axis::Y, 0.2);
    let (_, old, new) = applied(&neg, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("0.3", "0.1"));
    assert_eq!(text.as_deref(), Some("0 -2.0 0 0.1"));
    // Past a full turn the parallel angle is not wrapped.
    let big = world("\n").replace("rotation 0 1 0 0", "rotation 1 0 0 6");
    let (f, t) = box_span(&big);
    let (plan, _) = plan_rotate(&big, f, t, Axis::X, 1.0);
    let (_, old, new) = applied(&big, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("6", "7"));
}

#[test]
fn unchanged_lexemes_survive_a_composed_rotation() {
    // Identity with a Z axis, rotated about X: Y stays "0" exactly.
    let src = world("\n").replace("rotation 0 1 0 0", "rotation 0.0 0.0 1 0");
    let (f, t) = box_span(&src);
    let (plan, text) = plan_rotate(&src, f, t, Axis::X, 1.0);
    applied_any(&src, &plan);
    assert_eq!(text.as_deref(), Some("1 0.0 0 1"));
    // A zero-axis identity (`0 0 0 0`) gets the ring's axis.
    let zero = world("\n").replace("rotation 0 1 0 0", "rotation 0 0 0 0");
    let (f, t) = box_span(&zero);
    let (plan, text) = plan_rotate(&zero, f, t, Axis::Z, 0.5);
    applied_any(&zero, &plan);
    assert_eq!(text.as_deref(), Some("0 0 1 0.5"));
}

#[test]
fn rotation_refusals_change_nothing() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    for d in [f64::NAN, f64::INFINITY] {
        assert!(matches!(
            plan_rotate(&src, f, t, Axis::X, d).0,
            Plan::Refused {
                reason: reason::VALUE_NOT_FINITE,
                ..
            }
        ));
    }
    assert_eq!(plan_rotate(&src, f, t, Axis::Y, 0.00001).0, Plan::Unchanged);
    let missing = src.replace("  rotation 0 1 0 0\n", "");
    let (f, t) = box_span(&missing);
    assert_eq!(
        manip_target(&missing, f, t, Tool::Rotate)
            .unwrap_err()
            .reason,
        reason::NO_ROTATION
    );
    assert!(matches!(
        plan_rotate(&missing, f, t, Axis::X, 1.0).0,
        Plan::Refused {
            reason: reason::NO_ROTATION,
            ..
        }
    ));
    // Move does not need a rotation field.
    assert!(manip_target(&missing, f, t, Tool::Move).is_ok());
    let used = format!("{src}Group {{ children USE Box_1 }}\n");
    let (f, t) = box_span(&used);
    assert_eq!(
        manip_target(&used, f, t, Tool::Rotate).unwrap_err().reason,
        reason::INSTANCED
    );
    let routed = format!("{src}DEF T TimeSensor {{}}\nDEF O OrientationInterpolator {{}}\nROUTE O.value_changed TO Box_1.set_rotation\n");
    let (f, t) = box_span(&routed);
    assert_eq!(
        manip_target(&routed, f, t, Tool::Scale).unwrap_err().reason,
        reason::ROUTED
    );
}

#[test]
fn the_pivot_is_translation_plus_center() {
    let src = world("\n").replace(
        "translation 0 0 0",
        "translation 1 2 3 center 0.5 0 -1 scaleOrientation 0 0 1 0.4",
    );
    let tg = target_of(&src, Tool::Rotate);
    assert_eq!(tg.pivot, [1.5, 2.0, 2.0]);
    assert_eq!(tg.center, [0.5, 0.0, -1.0]);
    assert_eq!(tg.scale_orientation, [0.0, 0.0, 1.0, 0.4]);
    // The center point is fixed under any rotation and scale.
    for r in [[0.0, 1.0, 0.0, 1.0], [1.0, 1.0, 0.0, 2.5]] {
        let o = origin_of(
            tg.translation,
            tg.center,
            r,
            [2.0, 0.5, 3.0],
            tg.scale_orientation,
        )
        .unwrap();
        // origin is the image of local (0,0,0); the image of C is T + C.
        let c_img = origin_of(
            [
                tg.translation[0] + tg.center[0],
                tg.translation[1] + tg.center[1],
                tg.translation[2] + tg.center[2],
            ],
            [0.0; 3],
            r,
            [2.0, 0.5, 3.0],
            tg.scale_orientation,
        )
        .unwrap();
        assert_eq!(c_img, tg.pivot);
        assert!(o.iter().all(|v| v.is_finite()));
    }
}

#[test]
fn scale_one_axis_or_uniformly() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    for (axis, i) in [(Axis::X, 0), (Axis::Y, 1), (Axis::Z, 2)] {
        let (plan, text) = plan_scale(&src, f, t, Some(axis), 2.5);
        let (out, old, new) = applied(&src, &plan);
        assert_eq!((old.as_str(), new.as_str()), ("1", "2.5"));
        let mut want = [1.0; 3];
        want[i] = 2.5;
        assert_eq!(target_of(&out, Tool::Scale).scale, want);
        assert_eq!(text.as_deref().map(|s| s.split(' ').count()), Some(3));
    }
    let (plan, text) = plan_scale(&src, f, t, None, 0.5);
    let out = applied_any(&src, &plan);
    assert_eq!(text.as_deref(), Some("0.5 0.5 0.5"));
    assert!(out.contains("scale 0.5 0.5 0.5"));
    // Four significant digits.
    let (_, text) = plan_scale(&src, f, t, Some(Axis::X), 1.234_567);
    assert_eq!(text.as_deref(), Some("1.235 1 1"));
    let (_, text) = plan_scale(&src, f, t, Some(Axis::X), 0.012_345_6);
    assert_eq!(text.as_deref(), Some("0.01235 1 1"));
}

#[test]
fn scale_refusals_change_nothing() {
    let src = world("\n");
    let (f, t) = box_span(&src);
    for k in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            plan_scale(&src, f, t, None, k).0,
            Plan::Refused {
                reason: reason::FACTOR_INVALID,
                ..
            }
        ));
    }
    assert!(matches!(
        plan_scale(&src, f, t, Some(Axis::Y), 1e-9).0,
        Plan::Refused {
            reason: reason::RESULT_SINGULAR,
            ..
        }
    ));
    assert_eq!(plan_scale(&src, f, t, None, 1.00001).0, Plan::Unchanged);
    let spelled = src.replace("scale 1 1 1", "scale 1.0 1 1");
    let (f2, t2) = box_span(&spelled);
    assert_eq!(
        plan_scale(&spelled, f2, t2, Some(Axis::X), 1.0).0,
        Plan::Unchanged
    );
    let neg = src.replace("scale 1 1 1", "scale 1 -1 1");
    let (f3, t3) = box_span(&neg);
    assert_eq!(
        manip_target(&neg, f3, t3, Tool::Scale).unwrap_err().reason,
        reason::SCALE_NOT_POSITIVE
    );
    // Rotate and Move still accept a mirrored frame (they do not change it).
    assert!(manip_target(&neg, f3, t3, Tool::Rotate).is_ok());
    let missing = src.replace("  scale 1 1 1\n", "");
    let (f4, t4) = box_span(&missing);
    assert_eq!(
        manip_target(&missing, f4, t4, Tool::Scale)
            .unwrap_err()
            .reason,
        reason::NO_SCALE
    );
}

#[test]
fn rotate_and_scale_keep_line_endings_bom_and_comments() {
    for eol in ["\n", "\r\n", "\r"] {
        let src = format!(
            "\u{FEFF}#VRML V2.0 utf8{eol}# héllo ✓{eol}{}{eol}# tail{eol}",
            template(Primitive::Cone, "Cône_1", eol)
        );
        let (f, t) = box_span(&src);
        let (plan, _) = plan_rotate(&src, f, t, Axis::X, 0.25);
        let out = applied_any(&src, &plan);
        assert!(
            out.starts_with('\u{FEFF}')
                && out.contains("# héllo ✓")
                && out.ends_with(&format!("# tail{eol}"))
        );
        assert_eq!(out.matches(eol).count(), src.matches(eol).count());
        let (f, t) = box_span(&out);
        let (plan, _) = plan_scale(&out, f, t, Some(Axis::Z), 3.0);
        let out2 = applied_any(&out, &plan);
        assert_eq!(out2.len(), out.len());
        assert!(out2.contains("scale 1 1 3"));
    }
}

#[test]
fn nested_transforms_are_refused_for_rotate_and_scale() {
    let src = format!(
        "{H}Transform {{ children [ DEF In Transform {{ rotation 0 1 0 0 scale 1 1 1 }} ] }}\n"
    );
    let inner = spans(&src, "Transform")[1];
    for tool in [Tool::Rotate, Tool::Scale] {
        assert_eq!(
            manip_target(&src, inner.0, inner.1, tool)
                .unwrap_err()
                .reason,
            reason::NOT_TOP_LEVEL
        );
    }
}
