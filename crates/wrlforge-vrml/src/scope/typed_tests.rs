// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-2B typed reference tests. Every expected answer comes from
//! ISO/IEC 14772-1 (4.7, 4.8.1, 4.8.3, Table 4.4, 4.8.4, 4.9.2, 4.10.2), not
//! from a viewer. Every uncertain answer is proven to produce no finding.

use super::*;
use crate::node_schema::{self, Profile};
use crate::parse;

fn graph(src: &str) -> ScopeGraph {
    build(&parse(src))
}

const H: &str = "#VRML V2.0 utf8\n";

fn codes(g: &ScopeGraph) -> Vec<&'static str> {
    g.diagnostics().iter().map(|d| d.code).collect()
}

fn typed_codes(g: &ScopeGraph) -> Vec<&'static str> {
    codes(g)
        .into_iter()
        .filter(|c| c.starts_with("SCOPE1"))
        .collect()
}

/// The text a finding marks.
fn marked(src: &str, g: &ScopeGraph, code: &str) -> Vec<String> {
    let utf16: Vec<u16> = src.encode_utf16().collect();
    g.diagnostics()
        .iter()
        .filter(|d| d.code == code)
        .map(|d| {
            String::from_utf16(&utf16[d.range.start.offset as usize..d.range.end.offset as usize])
                .unwrap()
        })
        .collect()
}

fn type_of<'a>(g: &'a ScopeGraph, name: &str) -> Vec<&'a TypeRef> {
    g.instances.iter().filter(|t| t.type_name == name).collect()
}

/// Table 4.4, restated independently of the implementation.
fn table_4_4_oracle(definition: Access, declaration: Access) -> bool {
    use Access::*;
    matches!(
        (definition, declaration),
        (ExposedField, _) | (Field, Field) | (EventIn, EventIn) | (EventOut, EventOut)
    )
}

/// The hard gate for typed answers:
/// - an unproven node never has a resolved event;
/// - a resolved ROUTE has two resolved events of one type and direction;
/// - a resolved IS has a definition and a declaration of one type, legal
///   under Table 4.4;
/// - a resolved user type names a declaration of that name; a built-in
///   names none; an unresolved type names nothing;
/// - no finding is an error.
fn assert_gate(g: &ScopeGraph) {
    for r in &g.routes {
        for (n, e) in [
            (&g.references[r.source_node], &r.source),
            (&g.references[r.destination_node], &r.destination),
        ] {
            if n.status != Status::Resolved {
                assert_ne!(
                    e.status,
                    Status::Resolved,
                    "unproven node, proven event: {r:?}"
                );
                assert!(e.field_type.is_none() && e.access.is_none());
            }
            if e.status == Status::Resolved {
                assert!(e.field_type.is_some() && e.access.is_some() && e.bound_name.is_some());
            }
        }
        if r.status == Status::Resolved {
            assert_eq!(r.source.field_type, r.destination.field_type);
            assert!(matches!(
                r.source.access,
                Some(Access::EventOut | Access::ExposedField)
            ));
            assert!(matches!(
                r.destination.access,
                Some(Access::EventIn | Access::ExposedField)
            ));
        }
    }
    for b in &g.is_bindings {
        if b.status == Status::Resolved {
            let (d, i) = (
                b.definition.as_ref().unwrap(),
                b.declaration.as_ref().unwrap(),
            );
            assert_eq!(d.1, i.1);
            assert!(table_4_4_oracle(d.0, i.0));
        }
    }
    for t in &g.instances {
        match (t.status, t.target) {
            (Status::Resolved, Some(d)) => {
                assert!(!t.builtin);
                assert_eq!(g.types[d].name.as_deref(), Some(t.type_name.as_str()));
            }
            (Status::Resolved, None) => assert!(t.builtin),
            (_, target) => assert!(target.is_none() && !t.builtin, "{t:?}"),
        }
    }
    for d in g.diagnostics() {
        assert_ne!(d.severity, "error");
    }
}

const ANIM: &str = "DEF Clock TimeSensor { cycleInterval 2 loop TRUE }
DEF Path PositionInterpolator { key [ 0 1 ] keyValue [ 0 0 0  0 1 0 ] }
DEF Fade ScalarInterpolator { key [ 0 1 ] keyValue [ 0 1 ] }
DEF Mover Transform { children [ Shape { geometry Box { } } ] }
DEF Other Transform { }
";

fn anim(routes: &str) -> String {
    format!("{H}{ANIM}{routes}")
}

// 1. TimeSensor -> interpolator.
#[test]
fn valid_timesensor_to_interpolator_route() {
    let g = graph(&anim("ROUTE Clock.fraction_changed TO Path.set_fraction\n"));
    let r = &g.routes[0];
    assert_eq!((r.status, r.reason), (Status::Resolved, reason::OK));
    assert_eq!(r.source.access, Some(Access::EventOut));
    assert_eq!(r.destination.access, Some(Access::EventIn));
    assert_eq!(r.source.field_type.as_deref(), Some("SFFloat"));
    assert_eq!(r.source.origin, Some(Origin::BuiltinSchema));
    assert!(typed_codes(&g).is_empty());
    assert_gate(&g);
}

// 2. Interpolator -> Transform.
#[test]
fn valid_interpolator_to_transform_route() {
    let g = graph(&anim("ROUTE Path.value_changed TO Mover.set_translation\n"));
    let r = &g.routes[0];
    assert_eq!(r.status, Status::Resolved);
    assert_eq!(r.source.field_type.as_deref(), Some("SFVec3f"));
    // set_translation is the 4.7 eventIn name of exposedField translation.
    assert_eq!(
        r.destination.detail,
        Some(detail::EVENT_VIA_EXPOSEDFIELD_ALIAS)
    );
    assert!(typed_codes(&g).is_empty());
    assert_gate(&g);
}

// 3. Missing event name.
#[test]
fn route_with_missing_event_name() {
    let src = anim("ROUTE Path.key_value TO Mover.set_translation\n");
    let g = graph(&src);
    let r = &g.routes[0];
    assert_eq!(
        (r.source.status, r.source.reason),
        (Status::Unresolved, reason::ROUTE_EVENT_UNKNOWN)
    );
    assert_eq!(
        r.destination.status,
        Status::Resolved,
        "the other side survives"
    );
    assert_eq!(
        (r.status, r.reason),
        (Status::Unresolved, reason::ROUTE_EVENT_UNKNOWN)
    );
    assert_eq!(typed_codes(&g), vec![code::ROUTE_EVENT_UNKNOWN]);
    assert_eq!(
        marked(&src, &g, code::ROUTE_EVENT_UNKNOWN),
        vec!["key_value"]
    );
    assert_gate(&g);
}

// 4. Wrong direction, both sides.
#[test]
fn route_with_wrong_event_direction() {
    let src = anim(
        "ROUTE Mover.set_translation TO Other.set_translation
ROUTE Path.value_changed TO Mover.translation_changed
ROUTE Clock.cycleInterval TO Fade.set_fraction
",
    );
    let g = graph(&src);
    assert_eq!(
        g.routes[0].source.reason,
        reason::ROUTE_SOURCE_NOT_AN_EVENT_OUT
    );
    assert_eq!(g.routes[0].status, Status::Invalid);
    assert_eq!(
        g.routes[1].destination.reason,
        reason::ROUTE_DESTINATION_NOT_AN_EVENT_IN
    );
    assert_eq!(g.routes[1].status, Status::Invalid);
    // cycleInterval is an exposedField: a legal eventOut (4.10.2).
    assert_eq!(g.routes[2].source.status, Status::Resolved);
    assert_eq!(
        typed_codes(&g),
        vec![
            code::ROUTE_SOURCE_DIRECTION,
            code::ROUTE_DESTINATION_DIRECTION,
            code::ROUTE_TYPE_MISMATCH, // SFTime -> SFFloat
        ]
    );
    assert_eq!(
        marked(&src, &g, code::ROUTE_SOURCE_DIRECTION),
        vec!["set_translation"]
    );
    assert_eq!(
        marked(&src, &g, code::ROUTE_DESTINATION_DIRECTION),
        vec!["translation_changed"]
    );
    assert_gate(&g);
    // A built-in `field` is never an event (4.10.2; the fallback applies).
    let g = graph(&format!(
        "{H}DEF B Box {{ }}\nDEF T Transform {{ }}\nROUTE B.size TO T.set_scale\n"
    ));
    assert_eq!(
        g.routes[0].source.reason,
        reason::ROUTE_SOURCE_NOT_AN_EVENT_OUT
    );
    assert_gate(&g);
}

// 5. Different types: no implicit conversion.
#[test]
fn route_with_type_mismatch() {
    let src = anim(
        "ROUTE Clock.fraction_changed TO Mover.set_translation
ROUTE Fade.value_changed TO Path.set_fraction
",
    );
    let g = graph(&src);
    let r = &g.routes[0];
    assert_eq!(r.source.status, Status::Resolved);
    assert_eq!(r.destination.status, Status::Resolved);
    assert_eq!(
        (r.status, r.reason),
        (Status::Invalid, reason::ROUTE_TYPE_MISMATCH)
    );
    assert_eq!(g.routes[1].status, Status::Resolved, "SFFloat to SFFloat");
    assert_eq!(typed_codes(&g), vec![code::ROUTE_TYPE_MISMATCH]);
    assert_eq!(
        marked(&src, &g, code::ROUTE_TYPE_MISMATCH),
        vec!["ROUTE Clock.fraction_changed TO Mover.set_translation"]
    );
    assert_gate(&g);
}

// 6. exposedField event names: bare, explicit, and the 4.10.2 shorthand.
#[test]
fn exposed_field_event_names() {
    let g = graph(&anim(
        "ROUTE Mover.translation TO Other.translation
ROUTE Mover.translation_changed TO Other.set_translation
ROUTE Clock.fraction TO Path.fraction
",
    ));
    for r in &g.routes {
        assert_eq!(r.status, Status::Resolved, "{r:?}");
    }
    assert_eq!(g.routes[0].source.access, Some(Access::ExposedField));
    assert_eq!(g.routes[0].source.detail, None);
    assert_eq!(g.routes[1].source.access, Some(Access::EventOut));
    // fraction -> fraction_changed / set_fraction only by the fallback.
    assert_eq!(
        g.routes[2].source.bound_name.as_deref(),
        Some("fraction_changed")
    );
    assert_eq!(g.routes[2].source.detail, Some(detail::EVENT_VIA_SHORTHAND));
    assert_eq!(
        g.routes[2].destination.bound_name.as_deref(),
        Some("set_fraction")
    );
    assert!(typed_codes(&g).is_empty());
    assert_gate(&g);
}

// 7. Script eventIn and eventOut (the declared interface; the url is never read).
#[test]
fn script_event_in_and_event_out() {
    let src = format!(
        "{H}DEF Clk TimeSensor {{ }}
DEF Ctl Script {{
  eventIn SFBool go
  eventOut SFFloat grow
  field SFFloat rate 2
  url \"javascript: function go(v) {{ grow = 1; other_changed = 2; }}\"
}}
DEF F ScalarInterpolator {{ }}
ROUTE Clk.isActive TO Ctl.go
ROUTE Ctl.grow TO F.set_fraction
ROUTE Ctl.rate TO F.set_fraction
ROUTE Ctl.other_changed TO F.set_fraction
"
    );
    let g = graph(&src);
    let script = type_of(&g, "Script")[0];
    assert!(script.is_builtin("Script"));
    assert_eq!(g.routes[0].status, Status::Resolved);
    assert_eq!(
        g.routes[0].destination.origin,
        Some(Origin::ScriptInterface(script.node_range))
    );
    assert_eq!(g.routes[1].status, Status::Resolved);
    // A Script `field` is not an event.
    assert_eq!(
        g.routes[2].source.reason,
        reason::ROUTE_SOURCE_NOT_AN_EVENT_OUT
    );
    // Nothing is inferred from the script source.
    assert_eq!(g.routes[3].source.reason, reason::ROUTE_EVENT_UNKNOWN);
    assert_gate(&g);
}

const PULSE_OK: &str = "PROTO Pulse [
  field SFVec3f size 1 1 1
  field SFColor tint 1 0 0
  eventIn SFFloat set_level
  eventOut SFFloat level_changed
  exposedField SFBool enabled TRUE
]
{
  Group {
    children [
      Shape {
        appearance Appearance { material Material { diffuseColor IS tint } }
        geometry Box { size IS size }
      }
      DEF Relay ScalarInterpolator {
        key [ 0 1 ] keyValue [ 0 1 ]
        set_fraction IS set_level
        value_changed IS level_changed
      }
    ]
  }
}
";

// 8. A PROTO instance as a ROUTE endpoint.
#[test]
fn valid_proto_instance_with_routes() {
    let src = format!(
        "{H}{PULSE_OK}DEF Clk TimeSensor {{ }}
DEF Inst Pulse {{ size 2 2 2 }}
DEF F ScalarInterpolator {{ }}
ROUTE Clk.fraction_changed TO Inst.set_level
ROUTE Inst.level_changed TO F.set_fraction
ROUTE Inst.enabled TO Inst.set_enabled
ROUTE Inst.level TO F.set_fraction
ROUTE Inst.sizes_changed TO F.set_fraction
"
    );
    let g = graph(&src);
    let pulse = g
        .types
        .iter()
        .position(|t| t.name.as_deref() == Some("Pulse"))
        .unwrap();
    let inst = type_of(&g, "Pulse")[0];
    assert_eq!((inst.status, inst.target), (Status::Resolved, Some(pulse)));
    for r in &g.routes[..4] {
        assert_eq!(r.status, Status::Resolved, "{r:?}");
    }
    assert_eq!(
        g.routes[0].destination.origin,
        Some(Origin::ProtoInterface(pulse))
    );
    // `level` falls back to the declared `level_changed` (4.10.2).
    assert_eq!(g.routes[3].source.detail, Some(detail::EVENT_VIA_SHORTHAND));
    // The PROTO interface is fully known: a missing name is a defect.
    assert_eq!(g.routes[4].source.reason, reason::ROUTE_EVENT_UNKNOWN);
    assert_eq!(typed_codes(&g), vec![code::ROUTE_EVENT_UNKNOWN]);
    assert!(g.is_bindings.iter().all(|b| b.status == Status::Resolved));
    assert_gate(&g);
}

// 9. EXTERNPROTO: the local declaration only; the URL is never fetched.
#[test]
fn externproto_interface_for_local_analysis() {
    let src = format!(
        "{H}EXTERNPROTO Lamp [
  eventIn SFFloat set_level
  eventOut SFFloat level_changed
  field SFColor tint
] [ \"http://example.invalid/lamp.wrl#Lamp\" \"lamp.wrl\" ]
DEF Clk TimeSensor {{ }}
DEF L Lamp {{ }}
ROUTE Clk.fraction_changed TO L.set_level
ROUTE L.level_changed TO Clk.set_startTime
ROUTE L.glow_changed TO Clk.set_stopTime
ROUTE Clk.fraction_changed TO L.tint
"
    );
    let g = graph(&src);
    let lamp = g
        .types
        .iter()
        .position(|t| t.kind == TypeKind::ExternProto)
        .unwrap();
    assert_eq!(g.routes[0].status, Status::Resolved);
    assert_eq!(
        g.routes[0].destination.origin,
        Some(Origin::ExternProtoInterface(lamp))
    );
    // Declared locally, so the type check is real: SFFloat -> SFTime.
    assert_eq!(g.routes[1].reason, reason::ROUTE_TYPE_MISMATCH);
    // Not declared locally: 4.9.2 subset, so unknowable, NOT missing.
    assert_eq!(
        (g.routes[2].source.status, g.routes[2].source.reason),
        (
            Status::Unsupported,
            reason::EXTERNPROTO_INTERFACE_NOT_LOCALLY_VERIFIABLE
        )
    );
    // `tint` is a declared field; set_tint may exist in the implementation,
    // so the fallback cannot prove a direction defect.
    assert_eq!(g.routes[3].destination.status, Status::Unsupported);
    assert_eq!(typed_codes(&g), vec![code::ROUTE_TYPE_MISMATCH]);
    assert_gate(&g);
}

// 10. Valid IS connections, incl. exposedField definitions (Table 4.4 row 1).
#[test]
fn valid_is_connections() {
    let src = format!(
        "{H}{PULSE_OK}PROTO Spin [ eventIn SFRotation turn exposedField SFVec3f at 0 0 0 ] {{
  Transform {{ rotation IS turn translation IS at }}
}}
Pulse {{ }}
Spin {{ }}
"
    );
    let g = graph(&src);
    assert_eq!(g.is_bindings.len(), 6);
    for b in &g.is_bindings {
        assert_eq!(
            (b.status, b.reason),
            (Status::Resolved, reason::OK),
            "{b:?}"
        );
    }
    let turn = g
        .is_bindings
        .iter()
        .find(|b| b.is_name.as_deref() == Some("turn"))
        .unwrap();
    assert_eq!(
        turn.definition,
        Some((Access::ExposedField, "SFRotation".into()))
    );
    assert_eq!(
        turn.declaration,
        Some((Access::EventIn, "SFRotation".into()))
    );
    assert!(typed_codes(&g).is_empty());
    assert_gate(&g);
}

// 11. IS with different field types (4.8.3), incl. the Flux 08 shape.
#[test]
fn is_with_type_mismatch() {
    let src = format!(
        "{H}PROTO P [ field SFFloat size 1 field MFColor c [ ] ] {{
  Shape {{
    appearance Appearance {{ material Material {{ diffuseColor IS c }} }}
    geometry Box {{ size IS size }}
  }}
}}
P {{ }}
"
    );
    let g = graph(&src);
    for b in &g.is_bindings {
        assert_eq!(
            (b.status, b.reason),
            (Status::Invalid, reason::IS_TYPE_MISMATCH)
        );
    }
    assert_eq!(
        g.is_bindings[1].definition,
        Some((Access::Field, "SFVec3f".into()))
    );
    assert_eq!(
        g.is_bindings[1].declaration,
        Some((Access::Field, "SFFloat".into()))
    );
    assert_eq!(
        marked(&src, &g, code::IS_TYPE_MISMATCH),
        vec!["diffuseColor IS c", "size IS size"],
        "SFColor vs MFColor is a mismatch too (4.8.3)"
    );
    assert_gate(&g);
}

// 12. IS with incompatible access kinds (Table 4.4).
#[test]
fn is_with_access_mismatch() {
    let src = format!(
        "{H}PROTO P [
  field SFVec3f f 0 0 0
  exposedField SFVec3f x 1 1 1
  eventOut SFVec3f out
  eventIn SFVec3f in
] {{
  Group {{ children [
    Transform {{ set_translation IS f }}
    Box {{ size IS x }}
    Transform {{ set_scale IS out }}
    Transform {{ translation_changed IS in }}
    Transform {{ scale IS x }}
  ] }}
}}
P {{ }}
"
    );
    let g = graph(&src);
    let st: Vec<_> = g.is_bindings.iter().map(|b| (b.status, b.reason)).collect();
    let bad = (Status::Invalid, reason::IS_ACCESS_MISMATCH);
    assert_eq!(st, vec![bad, bad, bad, bad, (Status::Resolved, reason::OK)]);
    // eventIn/field, field/exposedField, eventIn/eventOut, eventOut/eventIn.
    assert_eq!(typed_codes(&g), vec![code::IS_ACCESS_MISMATCH; 4]);
    assert_gate(&g);
}

// 13. Nested PROTO interfaces with the same member name (4.8.4 innermost).
#[test]
fn nested_proto_interfaces_with_same_member_name() {
    let src = format!(
        "{H}PROTO Outer [ field SFColor c 1 0 0 field SFFloat only 1 ] {{
  PROTO Inner [ field SFFloat c 0.5 ] {{
    Group {{ children [
      Shape {{ appearance Appearance {{ material Material {{ transparency IS c }} }} }}
      Shape {{ appearance Appearance {{ material Material {{ shininess IS only }} }} }}
    ] }}
  }}
  Group {{ children [
    Shape {{ appearance Appearance {{ material Material {{ diffuseColor IS c }} }} }}
    Inner {{ }}
  ] }}
}}
Outer {{ }}
"
    );
    let g = graph(&src);
    let owner = |i: usize| {
        g.types[g.is_bindings[i].owner.unwrap()]
            .name
            .clone()
            .unwrap()
    };
    // Inner's `c` is SFFloat; Outer's `c` is SFColor. Each binds its own.
    assert_eq!(g.is_bindings[0].status, Status::Resolved);
    assert_eq!(owner(0), "Inner");
    // `only` exists on Outer only: NOT searched from Inner's body.
    assert_eq!(
        (g.is_bindings[1].status, g.is_bindings[1].reason),
        (Status::Unresolved, reason::IS_INTERFACE_MEMBER_NOT_DECLARED)
    );
    assert_eq!(owner(1), "Inner");
    assert_eq!(g.is_bindings[2].status, Status::Resolved);
    assert_eq!(owner(2), "Outer");
    assert_eq!(marked(&src, &g, code::IS_MEMBER_NOT_DECLARED), vec!["only"]);
    // Inner is visible inside Outer's body after its declaration.
    assert_eq!(type_of(&g, "Inner")[0].status, Status::Resolved);
    assert_gate(&g);
}

// 14. A PROTO with a built-in node name (4.8.1).
#[test]
fn proto_with_builtin_name() {
    let src = format!(
        "{H}Shape {{ geometry Box {{ size 1 1 1 }} }}
PROTO Box [ field SFFloat w 1 ] {{ Sphere {{ radius IS w }} }}
Shape {{ geometry Box {{ w 2 }} }}
PROTO Script [ eventIn SFFloat x ] {{ Group {{ }} }}
DEF S Script {{ field SFNode me USE S }}
DEF T TimeSensor {{ }}
ROUTE T.fraction_changed TO S.x
"
    );
    let g = graph(&src);
    let boxes = type_of(&g, "Box");
    // Before the PROTO: the file's own declaration exists, so NOT built-in.
    assert_eq!(boxes[0].reason, reason::PROTO_INSTANCE_BEFORE_DECLARATION);
    assert_eq!(boxes[1].status, Status::Resolved);
    assert!(!boxes[1].builtin);
    assert_eq!(boxes[1].detail, Some(detail::PROTO_SHADOWS_BUILTIN));
    // Script-only rules do not apply to a PROTO named Script: the
    // self-reference is a cycle (4.4.4), not the Script exemption.
    let s = type_of(&g, "Script")[0];
    assert!(!s.is_builtin("Script"));
    let me = g
        .references
        .iter()
        .find(|r| r.kind == RefKind::Use)
        .unwrap();
    assert_eq!(me.reason, reason::SELF_REFERENTIAL_USE);
    // The ROUTE uses the PROTO interface, not a Script interface.
    assert_eq!(g.routes[0].status, Status::Resolved);
    assert!(matches!(
        g.routes[0].destination.origin,
        Some(Origin::ProtoInterface(_))
    ));
    assert_eq!(
        typed_codes(&g),
        vec![
            code::PROTO_BEFORE_DECLARATION,
            code::PROTO_SHADOWS_BUILTIN,
            code::PROTO_SHADOWS_BUILTIN
        ]
    );
    assert!(g
        .diagnostics()
        .iter()
        .filter(|d| d.code == code::PROTO_SHADOWS_BUILTIN)
        .all(|d| d.severity == "info"));
    assert_gate(&g);
}

// 15. Repeated DEF names: no typed answer through an ambiguous node.
#[test]
fn repeated_def_names_give_no_typed_answer() {
    let src = format!(
        "{H}DEF A TimeSensor {{ }}
DEF A Transform {{ }}
DEF F ScalarInterpolator {{ }}
ROUTE A.fraction_changed TO F.set_fraction
ROUTE A.nonsense TO F.set_fraction
"
    );
    let g = graph(&src);
    for r in &g.routes {
        let n = &g.references[r.source_node];
        assert_eq!(n.status, Status::Ambiguous);
        assert!(g.edit_target(n).is_none());
        assert_eq!(r.source.status, Status::Ambiguous);
        assert_eq!(r.status, Status::Ambiguous);
        assert!(r.source.field_type.is_none());
    }
    // Only the 2A info finding: no event or type finding, no type narrowing.
    assert_eq!(codes(&g), vec![code::AMBIGUOUS, code::AMBIGUOUS]);
    assert_gate(&g);
}

// 16. Missing, unsupported and damaged declarations.
#[test]
fn missing_unsupported_and_damaged_declarations() {
    // Unknown type, IS outside a PROTO, duplicate PROTO, recursion.
    let src = format!(
        "{H}Gadget {{ }}
Transform {{ translation IS t }}
PROTO Dup [ ] {{ Group {{ }} }}
PROTO Dup [ ] {{ Group {{ }} }}
PROTO Self [ ] {{ Group {{ children [ Self {{ }} ] }} }}
"
    );
    let g = graph(&src);
    assert_eq!(marked(&src, &g, code::NODE_TYPE_UNKNOWN), vec!["Gadget"]);
    assert_eq!(
        marked(&src, &g, code::IS_OUTSIDE_PROTO),
        vec!["translation IS t"]
    );
    assert_eq!(marked(&src, &g, code::DUPLICATE_PROTO), vec!["Dup"]);
    assert_eq!(marked(&src, &g, code::RECURSIVE_PROTO), vec!["Self"]);
    assert_gate(&g);

    // IS inside an interface default: unsupported, no finding.
    let g = graph(&format!(
        "{H}PROTO P [ field SFNode n Transform {{ translation IS t }} field SFVec3f t 0 0 0 ] {{ Group {{ }} }}\n"
    ));
    assert_eq!(
        g.is_bindings[0].reason,
        reason::IS_IN_PROTO_INTERFACE_DEFAULT
    );
    assert!(typed_codes(&g).is_empty());

    // Damaged source withholds every typed answer, the built-in one too.
    let damaged = format!(
        "{H}PROTO P [ field SFFloat size 1 ] {{ Box {{ size IS size }}
DEF Clk TimeSensor {{ }}
DEF F ScalarInterpolator {{ }}
ROUTE Clk.nope TO F.set_fraction
Gadget {{ }}
"
    );
    let g = graph(&damaged);
    assert!(g.document_recovered.is_some());
    assert!(g.instances.iter().all(|t| t.status == Status::Recovered));
    assert!(g.is_bindings.iter().all(|b| b.status == Status::Recovered));
    assert!(g.routes.iter().all(|r| r.status == Status::Recovered));
    assert!(typed_codes(&g).is_empty(), "no false positive from damage");
    assert_gate(&g);

    // A damaged PROTO interface (a member without a name) is not provable.
    let g = graph(&format!(
        "{H}PROTO Q [ field SFFloat ] {{ Group {{ }} }}\nDEF I Q {{ }}\nDEF T TimeSensor {{ }}\nROUTE T.fraction_changed TO I.x\n"
    ));
    assert_ne!(g.routes[0].destination.status, Status::Resolved);
    assert!(typed_codes(&g).is_empty());

    // Every prefix of a valid document: no panic, the gate holds, and no
    // typed mismatch or missing-declaration warning appears.
    let full = format!(
        "{H}{PULSE_OK}DEF Clk TimeSensor {{ }}\nDEF Inst Pulse {{ }}\nROUTE Clk.fraction_changed TO Inst.set_level\n"
    );
    for (c, _) in full.char_indices() {
        let g = graph(&full[..c]);
        assert_gate(&g);
        for d in g.diagnostics() {
            assert!(
                !matches!(
                    d.code,
                    code::ROUTE_TYPE_MISMATCH
                        | code::IS_TYPE_MISMATCH
                        | code::IS_ACCESS_MISMATCH
                        | code::IS_MEMBER_NOT_DECLARED
                ),
                "prefix {c}: {d:?}"
            );
        }
    }
}

// 17. BOM, CRLF, lone CR and Unicode give the same answers.
#[test]
fn bom_crlf_cr_and_unicode() {
    let lf = format!(
        "{H}PROTO Pülse [ eventIn SFFloat set_lévél field SFFloat size 1 ] {{
  Group {{ children [ DEF R ScalarInterpolator {{ set_fraction IS set_lévél }} Box {{ size IS size }} ] }}
}}
DEF 𝒞lk TimeSensor {{ }}
DEF Ïnst Pülse {{ }}
ROUTE 𝒞lk.fraction_changed TO Ïnst.set_lévél
"
    );
    let summary = |g: &ScopeGraph| {
        (
            g.routes
                .iter()
                .map(|r| (r.status, r.reason))
                .collect::<Vec<_>>(),
            g.is_bindings
                .iter()
                .map(|b| (b.status, b.reason))
                .collect::<Vec<_>>(),
            g.instances
                .iter()
                .map(|t| (t.status, t.reason))
                .collect::<Vec<_>>(),
            codes(g),
        )
    };
    let base = summary(&graph(&lf));
    assert_eq!(base.0, vec![(Status::Resolved, reason::OK)]);
    assert_eq!(base.1[0], (Status::Resolved, reason::OK));
    assert_eq!(base.1[1], (Status::Invalid, reason::IS_TYPE_MISMATCH));
    for text in [
        lf.replace('\n', "\r\n"),
        lf.replace('\n', "\r"),
        format!("\u{feff}{}", lf.replace('\n', "\r\n")),
    ] {
        let g = graph(&text);
        assert_eq!(summary(&g), base);
        // The finding still marks the exact connection text.
        assert_eq!(
            marked(&text, &g, code::IS_TYPE_MISMATCH),
            vec!["size IS size"]
        );
        assert_gate(&g);
    }
}

// 18. Two documents with the same identifiers are independent.
#[test]
fn two_documents_with_same_identifiers() {
    let a = format!("{H}PROTO P [ field SFVec3f s 1 1 1 ] {{ Box {{ size IS s }} }}\nP {{ }}\n");
    let b = format!("{H}PROTO P [ field SFFloat s 1 ] {{ Box {{ size IS s }} }}\nP {{ }}\n");
    let (ga, gb) = (graph(&a), graph(&b));
    assert_eq!(ga.is_bindings[0].status, Status::Resolved);
    assert_eq!(gb.is_bindings[0].status, Status::Invalid);
    assert_eq!(graph(&a), ga, "building b did not change a");
}

// 20. Analysis changes no source and no parse; it is deterministic.
#[test]
fn analysis_is_read_only_and_deterministic() {
    let src = format!(
        "{H}{PULSE_OK}DEF Clk TimeSensor {{ }}\nDEF Inst Pulse {{ }}\nROUTE Clk.fraction_changed TO Inst.set_level\n"
    );
    let before = src.clone();
    let p = parse(&src);
    let p_before = format!("{:?}", p.tree);
    let (g1, g2) = (build(&p), build(&p));
    assert_eq!(g1, g2);
    assert_eq!(format!("{:?}", p.tree), p_before);
    assert_eq!(src, before);
}

// The original Flux 08 fixture is invalid; the corrected copy is valid.
#[test]
fn flux_fixtures_original_and_corrected() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop-tauri/src-tauri/tests/fixtures/flux-regression/"
    );
    let read = |n: &str| std::fs::read_to_string(format!("{dir}{n}")).unwrap();
    let orig = read("08_proto_script_inline.wrl");
    let g = graph(&orig);
    assert_eq!(typed_codes(&g), vec![code::IS_TYPE_MISMATCH]);
    assert_eq!(
        marked(&orig, &g, code::IS_TYPE_MISMATCH),
        vec!["size IS size"]
    );
    assert!(g.routes.iter().all(|r| r.status == Status::Resolved));
    assert_gate(&g);

    let fixed = read("corrected/08_proto_script_inline_valid.wrl");
    let g = graph(&fixed);
    assert!(codes(&g).is_empty(), "{:?}", g.diagnostics());
    assert!(!g.is_bindings.is_empty());
    assert!(g.is_bindings.iter().all(|b| b.status == Status::Resolved));
    assert!(!g.routes.is_empty());
    assert!(g.routes.iter().all(|r| r.status == Status::Resolved));
    assert_gate(&g);

    // m2: the missing eventOut is now proven.
    let src = read("malformed/m2_route_no_such_eventout.wrl");
    let g = graph(&src);
    assert_eq!(
        marked(&src, &g, code::ROUTE_EVENT_UNKNOWN),
        vec!["key_value"]
    );
    // m6: 4.10.2 node order only; no typed claim through unbound nodes.
    let g = graph(&read("malformed/m6_route_before_def.wrl"));
    assert!(typed_codes(&g).is_empty());
    assert!(g.routes.iter().all(|r| r.status == Status::Unresolved
        && r.reason == reason::ROUTE_NODE_NOT_DEFINED_BEFORE_ROUTE));
    assert_eq!(codes(&g), vec![code::BEFORE_DEF, code::BEFORE_DEF]);

    for n in [
        "01_anim_no_starttime.wrl",
        "02_anim_touch.wrl",
        "03_shape_no_appearance.wrl",
        "04_ifs_normals.wrl",
        "05_ifs_quads.wrl",
        "06_background_precision.wrl",
        "07_shared_material.wrl",
        "09_duplicate_def.wrl",
        "10_cylinder_cone.wrl",
    ] {
        let g = graph(&read(n));
        assert!(typed_codes(&g).is_empty(), "{n}: {:?}", g.diagnostics());
        assert_gate(&g);
    }
}

// No VRML97 built-in has a 4.7 alias collision, so schema lookups are unique.
#[test]
fn builtin_schema_has_no_alias_collisions() {
    for n in node_schema::list_node_names(Some(Profile::Vrml97)) {
        let s = node_schema::get_node_schema(n).unwrap();
        let mut list = Vec::new();
        for f in s.fields.iter().filter(|f| f.in_profile(Profile::Vrml97)) {
            let a = Access::parse(f.vrml97_declaration.unwrap()).unwrap();
            list.push(f.name.to_string());
            if a == Access::ExposedField {
                list.push(format!("set_{}", f.name));
                list.push(format!("{}_changed", f.name));
            }
        }
        let mut sorted = list.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len(), "{n}");
    }
}
