// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-2A scope graph tests. Every expected answer is checked against
//! ISO/IEC 14772-1 (4.4.4, 4.6.2, 4.8.4, 4.10.2), not against the flat JS
//! analyzer. Every `Resolved` answer is proven to point at the exact source
//! node; every uncertain answer is proven to point at no node.

use super::*;
use crate::parse;

fn graph(src: &str) -> ScopeGraph {
    build(&parse(src))
}

/// UTF-16 offset of the `nth` (0-based) occurrence of `needle`.
fn at(src: &str, needle: &str, nth: usize) -> u32 {
    let byte = src
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("no occurrence {nth} of {needle:?}"))
        .0;
    src[..byte].encode_utf16().count() as u32
}

fn uses<'a>(g: &'a ScopeGraph, name: &str) -> Vec<&'a Reference> {
    g.references
        .iter()
        .filter(|r| r.kind == RefKind::Use && r.name.as_deref() == Some(name))
        .collect()
}

fn routes(g: &ScopeGraph) -> Vec<&Reference> {
    g.references
        .iter()
        .filter(|r| r.kind != RefKind::Use)
        .collect()
}

/// The edit target's DEF keyword offset, or `None`.
fn target_at(g: &ScopeGraph, r: &Reference) -> Option<u32> {
    g.edit_target(r).map(|d| d.node_range.start.offset)
}

/// The hard gate: an answer that is not `Resolved` names no edit target.
fn assert_gate(g: &ScopeGraph) {
    for r in &g.references {
        if r.status == Status::Resolved {
            let d = g.edit_target(r).expect("resolved has a target");
            assert_eq!(Some(&d.name), r.name.as_ref(), "target has the same name");
            assert!(d.name_range.start.offset < r.statement_range.end.offset);
        } else {
            assert!(g.edit_target(r).is_none(), "{r:?} must not have a target");
            assert!(r.target.is_none());
        }
        if matches!(r.status, Status::Recovered | Status::Unsupported) {
            assert!(r.language_target.is_none(), "uncertain: no meaning claimed");
        }
    }
}

// 1. One DEF and one USE.
#[test]
fn one_def_one_use_resolves_to_that_node() {
    let src =
        "#VRML V2.0 utf8\nDEF M Material { }\nShape { appearance Appearance { material USE M } }";
    let g = graph(src);
    let u = uses(&g, "M");
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].status, Status::Resolved);
    assert_eq!(u[0].reason, reason::OK);
    assert_eq!(target_at(&g, u[0]), Some(at(src, "DEF M", 0)));
    assert_eq!(g.edit_target(u[0]).unwrap().node_type, "Material");
    assert!(g.diagnostics().is_empty());
    assert_gate(&g);
}

// 2. A USE before its DEF: 4.6.2 binds only a PRECEDING DEF.
#[test]
fn use_before_def_is_invalid_and_binds_nothing() {
    let src = "#VRML V2.0 utf8\nGroup { children [ USE B ] }\nDEF B Group { }";
    let g = graph(src);
    let u = uses(&g, "B")[0];
    assert_eq!(u.status, Status::Invalid);
    assert_eq!(u.reason, reason::USE_BEFORE_DEF);
    assert_eq!(u.candidates.len(), 1, "the later DEF is evidence only");
    assert!(u.language_target.is_none());
    let d = g.diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, code::BEFORE_DEF);
    assert_eq!(d[0].severity, "warning");
    assert_gate(&g);
}

// 3. Repeated DEF names: valid VRML97, no error; the standard picks the
// closest preceding DEF, the edit answer refuses.
#[test]
fn repeated_def_language_meaning_vs_edit_identity() {
    let src = "#VRML V2.0 utf8\nDEF A Group { }\nGroup { children [ USE A ] }\nDEF A Transform { }\nGroup { children [ USE A ] }";
    let g = graph(src);
    let u = uses(&g, "A");
    // First USE sees only the first DEF: unique, resolved.
    assert_eq!(u[0].status, Status::Resolved);
    assert_eq!(target_at(&g, u[0]), Some(at(src, "DEF A", 0)));
    // Second USE sees two: ambiguous for editing, closest for the language.
    assert_eq!(u[1].status, Status::Ambiguous);
    assert_eq!(u[1].reason, reason::DUPLICATE_DEF_IN_SCOPE);
    assert_eq!(u[1].candidates.len(), 2);
    assert!(g.edit_target(u[1]).is_none());
    let lang = g.language_target(u[1]).unwrap();
    assert_eq!(lang.node_range.start.offset, at(src, "DEF A", 1));
    assert_eq!(lang.node_type, "Transform");
    // Advisory info only: never an error.
    let d = g.diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].code, d[0].severity), (code::AMBIGUOUS, "info"));
    assert!(parse(src).diagnostics.is_empty(), "no syntax error");
    assert_gate(&g);
}

// 4. A PROTO body is its own DEF/USE scope (4.8.4).
#[test]
fn proto_body_def_binds_inside_only() {
    let src = "#VRML V2.0 utf8\nPROTO P [ ] { DEF In Group { children [ USE In ] } }\nGroup { children [ USE In ] }";
    let g = graph(src);
    let u = uses(&g, "In");
    // Inside: a self-reference in the transformation hierarchy.
    assert_eq!(u[0].status, Status::Invalid);
    assert_eq!(u[0].reason, reason::SELF_REFERENTIAL_USE);
    // Outside: the body DEF is not visible.
    assert_eq!(u[1].status, Status::Unresolved);
    assert_eq!(u[1].reason, reason::DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY);
    assert_eq!(g.scopes[g.defs[0].scope].kind, ScopeKind::ProtoBody);
    assert_gate(&g);
}

// 5. Nested PROTOs: separate from the enclosing body too (4.8.4).
#[test]
fn nested_proto_scopes_are_disjoint_both_ways() {
    let src = "#VRML V2.0 utf8\nPROTO Outer [ ] {\n DEF X Group { }\n PROTO Inner [ ] { DEF Y Group { } Group { children [ USE X USE Y ] } }\n Group { children [ USE Y USE X ] }\n}";
    let g = graph(src);
    let x = uses(&g, "X");
    let y = uses(&g, "Y");
    // In Inner: X is Outer's, not visible; Y is Inner's own.
    assert_eq!(x[0].reason, reason::DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY);
    assert_eq!(y[0].status, Status::Resolved);
    assert_eq!(target_at(&g, y[0]), Some(at(src, "DEF Y", 0)));
    // In Outer: Y is Inner's, not visible; X is Outer's own.
    assert_eq!(y[1].reason, reason::DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY);
    assert_eq!(x[1].status, Status::Resolved);
    assert_eq!(target_at(&g, x[1]), Some(at(src, "DEF X", 0)));
    assert_eq!(g.scopes.len(), 3);
    assert_gate(&g);
}

// 6. An outer DEF with the same name as a PROTO-local DEF. Not a duplicate:
// the two live in disjoint scopes (corrected Flux fixture 09 shape).
#[test]
fn outer_and_proto_local_same_name_are_not_duplicates() {
    let src = "#VRML V2.0 utf8\nDEF Floor Transform { }\nPROTO Crate [ ] { DEF Floor Transform { } Group { children [ USE Floor ] } }\nCrate { }\nGroup { children [ USE Floor ] }";
    let g = graph(src);
    let u = uses(&g, "Floor");
    assert_eq!(u[0].status, Status::Resolved);
    assert_eq!(
        target_at(&g, u[0]),
        Some(at(src, "DEF Floor", 1)),
        "body USE -> body DEF"
    );
    assert_eq!(u[1].status, Status::Resolved);
    assert_eq!(
        target_at(&g, u[1]),
        Some(at(src, "DEF Floor", 0)),
        "outer USE -> outer DEF"
    );
    assert!(g.diagnostics().is_empty());
    assert_gate(&g);
}

// 7. A valid ROUTE between known nodes: both endpoint ranges are exact.
#[test]
fn valid_route_binds_both_endpoints_with_exact_ranges() {
    let src = "#VRML V2.0 utf8\nDEF T TimeSensor { }\nDEF I PositionInterpolator { }\nROUTE T.fraction_changed TO I.set_fraction";
    let g = graph(src);
    let r = routes(&g);
    assert_eq!(r.len(), 2);
    let (s, d) = (r[0], r[1]);
    assert_eq!(
        (s.kind, d.kind),
        (RefKind::RouteSource, RefKind::RouteDestination)
    );
    assert_eq!(s.status, Status::Resolved);
    assert_eq!(d.status, Status::Resolved);
    assert_eq!(target_at(&g, s), Some(at(src, "DEF T", 0)));
    assert_eq!(target_at(&g, d), Some(at(src, "DEF I", 0)));
    let route = at(src, "ROUTE", 0);
    assert_eq!(s.name_range.unwrap().start.offset, route + 6);
    assert_eq!(d.name_range.unwrap().start.offset, at(src, "I.set", 0));
    assert_eq!(
        s.name_range.unwrap().end.offset - s.name_range.unwrap().start.offset,
        1
    );
    assert_eq!(g.route_at(s.statement_range).unwrap().1, d);
    assert!(g.diagnostics().is_empty());
    assert_gate(&g);
}

// 8. ROUTE endpoints with missing names.
#[test]
fn route_missing_and_unknown_names() {
    // Unknown names: a defect, warned.
    let src =
        "#VRML V2.0 utf8\nDEF T TimeSensor { }\nROUTE T.fraction_changed TO Nope.set_fraction";
    let g = graph(src);
    let r = routes(&g);
    assert_eq!(r[0].status, Status::Resolved);
    assert_eq!(r[1].status, Status::Unresolved);
    assert_eq!(r[1].reason, reason::DEF_NOT_DECLARED_IN_SCOPE);
    let d = g.diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, code::NOT_DECLARED);
    assert_eq!(d[0].range, r[1].name_range.unwrap());
    // A ROUTE with no node token at all: the parse has damage, so nothing
    // is claimed, and the token fact is reported as missing-name.
    let g = graph("#VRML V2.0 utf8\nDEF T TimeSensor { }\nROUTE T.isActive TO .x");
    let r = routes(&g);
    assert_eq!(r[1].status, Status::Invalid);
    assert_eq!(r[1].reason, reason::MISSING_NAME);
    assert_eq!(
        r[0].status,
        Status::Recovered,
        "document scope has a syntax error"
    );
    assert!(g.diagnostics().is_empty());
    assert_gate(&g);
}

// 9. A ROUTE inside a PROTO body sees only the body's DEFs.
#[test]
fn route_inside_proto_body_uses_body_scope() {
    let src = "#VRML V2.0 utf8\nDEF Clk TimeSensor { }\nPROTO P [ ] {\n DEF Clk TimeSensor { }\n DEF S Script { eventIn SFTime go }\n ROUTE Clk.cycleTime TO S.go\n ROUTE Out.x TO S.go\n}\nDEF Out Group { }";
    let g = graph(src);
    let r = routes(&g);
    assert_eq!(r[0].status, Status::Resolved);
    assert_eq!(
        target_at(&g, r[0]),
        Some(at(src, "DEF Clk", 1)),
        "the body's Clk"
    );
    assert_eq!(target_at(&g, r[1]), Some(at(src, "DEF S", 0)));
    // `Out` is outer and later: across the boundary, not "defined later".
    assert_eq!(r[2].status, Status::Unresolved);
    assert_eq!(r[2].reason, reason::DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY);
    assert_gate(&g);
}

// 4.10.2: nodes in a ROUTE "shall be defined before the ROUTE statement".
// The corrected Flux fixture m6 is genuinely non-conforming.
#[test]
fn route_before_def_is_unresolved_with_its_own_reason() {
    let src = "#VRML V2.0 utf8\nROUTE TS.fraction_changed TO PI.set_fraction\nDEF PI PositionInterpolator { }\nDEF TS TimeSensor { }";
    let g = graph(src);
    for r in routes(&g) {
        assert_eq!(r.status, Status::Unresolved);
        assert_eq!(r.reason, reason::ROUTE_NODE_NOT_DEFINED_BEFORE_ROUTE);
    }
    assert_eq!(g.diagnostics().len(), 2);
    assert!(g.diagnostics().iter().all(|d| d.code == code::BEFORE_DEF));
    assert_gate(&g);
}

// 10. DEF, PROTO and interface names with the same spelling never collide.
#[test]
fn three_namespaces_with_one_spelling() {
    let src = "#VRML V2.0 utf8\nPROTO Ball [ field SFFloat Ball 1 ] { Sphere { radius IS Ball } }\nDEF Ball Ball { Ball 2 }\nDEF S Script { field SFNode Ball USE Ball }\nGroup { children [ USE Ball ] }";
    let g = graph(src);
    assert_eq!(g.defs.iter().filter(|d| d.name == "Ball").count(), 1);
    assert_eq!(g.types.len(), 1);
    assert_eq!(g.types[0].name.as_deref(), Some("Ball"));
    assert_eq!(g.members_of(MemberOwner::Type(0)).count(), 1);
    let script = g
        .members
        .iter()
        .filter(|m| matches!(m.owner, MemberOwner::Script(_)));
    assert_eq!(script.count(), 1);
    // Both USEs bind the one DEF; the PROTO and member names do not count.
    for u in uses(&g, "Ball") {
        assert_eq!(u.status, Status::Resolved, "{u:?}");
        assert_eq!(target_at(&g, u), Some(at(src, "DEF Ball", 0)));
    }
    // Built-in types (Sphere, Script, Group) are never declarations.
    assert!(g.types.iter().all(|t| t.name.as_deref() == Some("Ball")));
    assert_gate(&g);
}

// 4.4.4: a self-reference below a Script is a standard idiom, not a cycle.
#[test]
fn script_self_reference_is_resolved() {
    let src = "#VRML V2.0 utf8\nDEF S Script { field SFNode me USE S }";
    let g = graph(src);
    let u = uses(&g, "S")[0];
    assert_eq!(u.status, Status::Resolved);
    assert_eq!(
        u.reason,
        reason::SELF_REFERENCE_OUTSIDE_TRANSFORMATION_HIERARCHY
    );
    assert_eq!(target_at(&g, u), Some(at(src, "DEF S", 0)));
    assert!(g.diagnostics().is_empty());
}

// 11. Malformed and incomplete source: no panic, no false answer.
#[test]
fn malformed_source_withholds_answers() {
    // An unclosed PROTO absorbs the following statements: with the brace,
    // `USE Foo` would be ambiguous; without it, a single `Foo` in the body.
    let src = "#VRML V2.0 utf8\nDEF Foo Group { }\nPROTO P [ ] { Shape { }\nDEF Foo Transform { }\nGroup { children [ USE Foo ] }";
    let g = graph(src);
    assert_eq!(
        g.document_recovered,
        Some(reason::DOCUMENT_STRUCTURE_UNBALANCED)
    );
    let u = uses(&g, "Foo")[0];
    assert_eq!(u.status, Status::Recovered);
    assert!(
        g.diagnostics().is_empty(),
        "uncertain never becomes a finding"
    );
    assert_gate(&g);

    // A localized error in one PROTO body withholds only that body.
    let src = "#VRML V2.0 utf8\nDEF A Group { }\nPROTO P [ ] { Group { children [ USE Z ] 7 } }\nGroup { children [ USE A USE Q ] }";
    let g = graph(src);
    assert!(g.document_recovered.is_none());
    assert_eq!(uses(&g, "Z")[0].status, Status::Recovered);
    assert_eq!(uses(&g, "A")[0].status, Status::Resolved);
    assert_eq!(uses(&g, "Q")[0].status, Status::Unresolved);
    assert_gate(&g);

    // Garbage and every prefix of a real document: never panic, gate holds.
    let real = "#VRML V2.0 utf8\nDEF A Group { children [ USE A ] }\nPROTO P [ field SFNode n DEF Q Group { } ] { DEF B Group { } ROUTE B.x TO B.y }\nROUTE A.a TO A.b";
    let cuts: Vec<usize> = real.char_indices().map(|(i, _)| i).collect();
    for &cut in &cuts {
        assert_gate(&graph(&real[..cut]));
    }
    for junk in [
        "}}]]{{[[ DEF USE ROUTE . . 1e PROTO [ field ",
        "USE",
        "ROUTE",
        "DEF",
        "PROTO",
        "\u{feff}",
    ] {
        assert_gate(&graph(junk));
    }
}

// The flux m4 shape: "DEF O Target Transform { }" is a syntax error.
#[test]
fn def_with_two_type_names_is_recovered() {
    let src = "#VRML V2.0 utf8\nDEF O Target Transform { }\nGroup { children [ USE O ] }";
    let g = graph(src);
    assert_eq!(uses(&g, "O")[0].status, Status::Recovered);
    assert_gate(&g);
}

// 12. BOM, CRLF, lone CR and Unicode names: UTF-16 offsets stay exact.
#[test]
fn bom_crlf_cr_and_unicode_names() {
    for eol in ["\n", "\r\n", "\r"] {
        let src = format!(
            "\u{feff}#VRML V2.0 utf8{eol}DEF Bäll😀 Group {{ }}{eol}# é{eol}Group {{ children [ USE Bäll😀 ] }}{eol}ROUTE Bäll😀.a TO Bäll😀.b{eol}"
        );
        let g = graph(&src);
        let u = uses(&g, "Bäll😀");
        assert_eq!(u.len(), 1, "eol {eol:?}");
        assert_eq!(u[0].status, Status::Resolved);
        assert_eq!(target_at(&g, u[0]), Some(at(&src, "DEF Bäll😀", 0)));
        let nr = u[0].name_range.unwrap();
        assert_eq!(nr.start.offset, at(&src, "Bäll😀 ]", 0));
        // 4 BMP chars + one surrogate pair = 6 UTF-16 units.
        assert_eq!(nr.end.offset - nr.start.offset, 6);
        assert_eq!(&src[nr.start.byte as usize..nr.end.byte as usize], "Bäll😀");
        assert!(routes(&g).iter().all(|r| r.status == Status::Resolved));
        assert_gate(&g);
    }
}

// 14. Unsupported scope: a PROTO interface default value.
#[test]
fn proto_interface_default_is_unsupported_not_missing() {
    let src = "#VRML V2.0 utf8\nDEF Out Group { }\nPROTO P [ field SFNode n DEF Q Group { children [ USE Out ] } ] { Group { } }\nGroup { children [ USE Q ] }";
    let g = graph(src);
    let out = uses(&g, "Out")[0];
    assert_eq!(out.status, Status::Unsupported);
    assert_eq!(out.reason, reason::REFERENCE_IN_PROTO_INTERFACE_DEFAULT);
    let q = uses(&g, "Q")[0];
    assert_eq!(q.status, Status::Unsupported);
    assert_eq!(q.reason, reason::NAME_DECLARED_IN_PROTO_INTERFACE_DEFAULT);
    assert_eq!(g.unscoped_defs.len(), 1);
    assert!(g.diagnostics().is_empty(), "no false missing-node finding");
    assert_gate(&g);
}

// A PROTO body with no name or no node: the scope cannot be proven.
#[test]
fn unprovable_proto_bodies_are_recovered() {
    let g = graph("#VRML V2.0 utf8\nPROTO P [ ] { ROUTE A.b TO C.d }");
    assert_eq!(g.scopes[1].recovered, Some(reason::PROTO_BODY_NOT_PROVABLE));
    assert!(routes(&g).iter().all(|r| r.status == Status::Recovered));
}

// Cybertown/Blaxxun ROUTE inside an MFNode array: bound in the array's
// scope and classified, never normalized away.
#[test]
fn route_in_mfnode_array_is_classified() {
    let src = "#VRML V2.0 utf8\nDEF T TimeSensor { }\nGroup { children [ ROUTE T.isActive TO T.set_enabled ] }";
    let g = graph(src);
    for r in routes(&g) {
        assert_eq!(r.status, Status::Resolved);
        assert_eq!(r.detail, Some(detail::ROUTE_IN_MFNODE_ARRAY));
    }
}

// The corrected Flux fixtures, by content (not by file name).
#[test]
fn flux_fixture_shapes() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop-tauri/src-tauri/tests/fixtures/flux-regression/"
    );
    let read = |n: &str| std::fs::read_to_string(format!("{dir}{n}")).unwrap();

    // 09: repeated DEF + PROTO-local DEF. Valid VRML97.
    let src = read("09_duplicate_def.wrl");
    let g = graph(&src);
    let b = uses(&g, "Box1");
    assert_eq!(b[0].status, Status::Resolved);
    assert_eq!(target_at(&g, b[0]), Some(at(&src, "DEF Box1", 0)));
    assert_eq!(b[1].status, Status::Ambiguous);
    assert_eq!(
        g.language_target(b[1]).unwrap().node_range.start.offset,
        at(&src, "DEF Box1", 1)
    );
    let f = uses(&g, "Floor")[0];
    assert_eq!(f.status, Status::Resolved);
    assert_eq!(
        target_at(&g, f),
        Some(at(&src, "DEF Floor", 0)),
        "not the PROTO's Floor"
    );
    assert!(g.diagnostics().iter().all(|d| d.severity == "info"));

    // 08: PROTO + Script + ROUTEs. All endpoints proven.
    let src = read("08_proto_script_inline.wrl");
    let g = graph(&src);
    assert_eq!(routes(&g).len(), 4);
    assert!(routes(&g).iter().all(|r| r.status == Status::Resolved));
    assert!(g.diagnostics().is_empty());

    // m2: node names are fine; the bad event name is RUST-STAB-2B's.
    let g = graph(&read("malformed/m2_route_no_such_eventout.wrl"));
    assert!(routes(&g).iter().all(|r| r.status == Status::Resolved));
    assert!(g.diagnostics().is_empty());

    // m6: really non-conforming (4.10.2).
    let g = graph(&read("malformed/m6_route_before_def.wrl"));
    assert!(routes(&g)
        .iter()
        .all(|r| r.reason == reason::ROUTE_NODE_NOT_DEFINED_BEFORE_ROUTE));

    // m5: unbalanced: everything withheld.
    let g = graph(&read("malformed/m5_unbalanced_braces.wrl"));
    assert!(g.document_recovered.is_some());

    for n in [
        "01_anim_no_starttime.wrl",
        "02_anim_touch.wrl",
        "03_shape_no_appearance.wrl",
        "04_ifs_normals.wrl",
        "05_ifs_quads.wrl",
        "06_background_precision.wrl",
        "07_shared_material.wrl",
        "10_cylinder_cone.wrl",
    ] {
        let g = graph(&read(n));
        assert!(
            g.references.iter().all(|r| r.status == Status::Resolved),
            "{n}: {:?}",
            g.references
        );
        assert!(g.diagnostics().is_empty(), "{n}");
        assert_gate(&g);
    }
}

// Disposable and pure: the same text gives the same graph, and building it
// changes nothing in the parse.
#[test]
fn graph_is_deterministic_and_read_only() {
    let src = "#VRML V2.0 utf8\nDEF A Group { }\nGroup { children [ USE A ] }";
    let p = parse(src);
    let before = format!("{:?}", p.tree);
    assert_eq!(build(&p), build(&p));
    assert_eq!(format!("{:?}", p.tree), before);
}
