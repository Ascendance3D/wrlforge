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
    assert_eq!(tg.root_index, 0);
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
            assert_eq!(tg.root_index, 1, "after WorldInfo");
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
    assert_eq!((a.root_index, a.def.as_deref()), (4, Some("T")));
    assert_eq!((b.root_index, b.def), (5, None));
    assert_eq!(b.translation, [1.0, 2.0, 3.0]);
    let (plan, _) = plan_translate(&src, ts[1].0, ts[1].1, Axis::Y, -1.5, 2);
    let (out, old, new) = applied(&src, &plan);
    assert_eq!((old.as_str(), new.as_str()), ("2", "-1.5"));
    assert!(out.contains("Transform { translation 1 -1.5 3 }"));
}

#[test]
fn proto_bodies_are_refused() {
    let src = format!("{H}PROTO P [] {{ DEF A Transform {{ translation 0 0 0 }} }}\n");
    let (f, t) = spans(&src, "Transform")[0];
    let r = translate_target(&src, f, t).unwrap_err();
    assert_eq!(r.reason, field_edit::reason::NODE_IN_PROTO_SCOPE);
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
