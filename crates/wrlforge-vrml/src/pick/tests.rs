// SPDX-License-Identifier: GPL-3.0-or-later
//! Resolver tests. Every expected span is known by AUTHORSHIP: each fixture is
//! composed by concatenation and records the UTF-16 span of every labelled
//! occurrence as it writes it (the WD2-C0 oracle rule). The snapshot graphs
//! are written by hand in the shape the X_ITE adapter produces; real X_ITE
//! snapshots are exercised by the in-window smoke (`--smoke-pick`).
use super::*;
use crate::parse;
use crate::scene::build_scene_tree;

const HEADER: &str = "#VRML V2.0 utf8\n";

/// The oracle: text by concatenation, spans by authorship (UTF-16).
struct Doc {
    s: String,
    spans: HashMap<&'static str, (u64, u64)>,
    stack: Vec<(&'static str, u64)>,
}

impl Doc {
    fn new() -> Doc {
        Doc::with(HEADER)
    }
    fn with(head: &str) -> Doc {
        Doc {
            s: head.to_string(),
            spans: HashMap::new(),
            stack: vec![],
        }
    }
    fn len16(&self) -> u64 {
        self.s.encode_utf16().count() as u64
    }
    fn put(&mut self, t: &str) -> &mut Doc {
        self.s.push_str(t);
        self
    }
    fn open(&mut self, label: &'static str, t: &str) -> &mut Doc {
        let at = self.len16();
        self.stack.push((label, at));
        self.put(t)
    }
    fn close(&mut self, t: &str) -> &mut Doc {
        self.put(t);
        let (label, start) = self.stack.pop().unwrap();
        let end = self.len16();
        assert!(self.spans.insert(label, (start, end)).is_none());
        self
    }
    fn leaf(&mut self, label: &'static str, t: &str) -> &mut Doc {
        let start = self.len16();
        self.put(t);
        let end = self.len16();
        assert!(self.spans.insert(label, (start, end)).is_none());
        self
    }
    fn span(&self, l: &str) -> (u64, u64) {
        self.spans[l]
    }
}

/// `Transform { translation t children [ Shape { geometry G } ] }`.
fn simple(d: &mut Doc, t: &'static str, s: &'static str, translation: &str, geometry: &str) {
    d.open(
        t,
        &format!("Transform {{ translation {translation} children [ "),
    );
    d.leaf(s, &format!("Shape {{ geometry {geometry} }}"));
    d.close(" ] }");
    d.put("\n");
}

/// A hand-written X_ITE snapshot graph.
#[derive(Default)]
struct G {
    nodes: HashMap<String, GraphNode>,
}

impl G {
    fn node(&mut self, l: &str, ty: &str, occ: &[(u64, u64)], parents: &[&str]) -> &mut G {
        self.node_in(l, ty, Ctx::Document, occ, parents)
    }
    fn node_in(
        &mut self,
        l: &str,
        ty: &str,
        ctx: Ctx,
        occ: &[(u64, u64)],
        parents: &[&str],
    ) -> &mut G {
        self.nodes.insert(
            l.into(),
            GraphNode {
                type_name: Some(ty.into()),
                ctx,
                occurrences: occ.to_vec(),
                parents: parents.iter().map(|p| Parent::parse(p)).collect(),
            },
        );
        self
    }
    fn hit(&self, shape: &str) -> Hit {
        Hit {
            shape: shape.into(),
            ctx: self.nodes[shape].ctx,
            sensors: vec![],
            graph: self.nodes.clone(),
        }
    }
}

fn run(text: &str, hit: &Hit, offset: u64) -> Resolution {
    let p = parse(text);
    let t = build_scene_tree(&p.tree, text);
    resolve(hit, &p, &t, offset)
}

fn slice16(s: &str, from: u64, to: u64) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16(&u[from as usize..to as usize]).unwrap()
}

#[track_caller]
fn proven(r: Resolution) -> Proven {
    match r {
        Resolution::Proven(p) => p,
        other => panic!("not proven: {other:?}"),
    }
}

#[track_caller]
fn refused(r: Resolution, status: Status, reason: &str) {
    assert_eq!(r.status(), status, "{r:?}");
    assert_eq!(r.reason(), reason, "{r:?}");
}

/// The common runtime chain of a simple object: Shape -> Transform -> scene.
fn simple_graph(d: &Doc, t: &str, s: &str, shift: u64) -> G {
    let sh = |(a, b): (u64, u64)| (a - shift, b - shift);
    let mut g = G::default();
    g.node("n1", "Shape", &[sh(d.span(s))], &["n2"]).node(
        "n2",
        "Transform",
        &[sh(d.span(t))],
        &["SCENE"],
    );
    g
}

#[test]
fn one_root_box_shape_is_proven_exactly() {
    let mut d = Doc::new();
    d.leaf("s", "Shape {\n  geometry Box { size 4 4 4 }\n}");
    d.put("\n");
    let mut g = G::default();
    g.node("n1", "Shape", &[d.span("s")], &["SCENE"]);
    let p = proven(run(&d.s, &g.hit("n1"), 0));
    assert_eq!((p.shape.from, p.shape.to), d.span("s"));
    assert_eq!(p.logical, p.shape);
    assert_eq!(p.role, Role::Shape);
    assert_eq!(p.item, format!("node-{}-{}", d.span("s").0, d.span("s").1));
}

#[test]
fn two_primitives_promote_each_to_its_own_transform() {
    let mut d = Doc::new();
    simple(&mut d, "box.t", "box.s", "-3 0 0", "Box { }");
    simple(&mut d, "sph.t", "sph.s", "3 0 0", "Sphere { }");
    let b = proven(run(
        &d.s,
        &simple_graph(&d, "box.t", "box.s", 0).hit("n1"),
        0,
    ));
    let s = proven(run(
        &d.s,
        &simple_graph(&d, "sph.t", "sph.s", 0).hit("n1"),
        0,
    ));
    assert_eq!((b.logical.from, b.logical.to), d.span("box.t"));
    assert_eq!((s.logical.from, s.logical.to), d.span("sph.t"));
    assert_eq!(b.role, Role::SimpleObject);
    assert_eq!(b.logical.node_type, "Transform");
    assert_eq!(b.shape.node_type, "Shape");
    assert_ne!(b.item, s.item);
}

#[test]
fn every_visual1_primitive_promotes() {
    for prim in ["Box", "Sphere", "Cylinder", "Cone"] {
        let mut d = Doc::new();
        simple(&mut d, "t", "s", "0 0 0", &format!("{prim} {{ }}"));
        let p = proven(run(&d.s, &simple_graph(&d, "t", "s", 0).hit("n1"), 0));
        assert_eq!(p.role, Role::SimpleObject, "{prim}");
    }
    // Not a primitive: the Shape itself is selected.
    let mut d = Doc::new();
    simple(&mut d, "t", "s", "0 0 0", "IndexedFaceSet { }");
    let p = proven(run(&d.s, &simple_graph(&d, "t", "s", 0).hit("n1"), 0));
    assert_eq!(
        (p.role, (p.logical.from, p.logical.to)),
        (Role::Shape, d.span("s"))
    );
}

#[test]
fn identical_siblings_resolve_to_their_own_occurrence() {
    let mut d = Doc::new();
    let labels: Vec<(&'static str, &'static str)> = vec![
        ("t0", "s0"),
        ("t1", "s1"),
        ("t2", "s2"),
        ("t3", "s3"),
        ("t4", "s4"),
        ("t5", "s5"),
    ];
    for (t, s) in &labels {
        // Byte-identical text: only the provenance span tells them apart.
        simple(&mut d, t, s, "0 0 0", "Box { }");
    }
    let mut seen = std::collections::HashSet::new();
    for (t, s) in &labels {
        let p = proven(run(&d.s, &simple_graph(&d, t, s, 0).hit("n1"), 0));
        assert_eq!((p.logical.from, p.logical.to), d.span(t));
        assert!(seen.insert(p.item));
    }
}

#[test]
fn nested_identical_structures() {
    let mut d = Doc::new();
    for (outer, up_t, up_s, dn_t, dn_s, x) in [
        ("L", "L.up.t", "L.up.s", "L.dn.t", "L.dn.s", "-3"),
        ("R", "R.up.t", "R.up.s", "R.dn.t", "R.dn.s", "3"),
    ] {
        d.open(
            outer,
            &format!("Transform {{ translation {x} 0 0 children [\n"),
        );
        d.put("  ");
        simple(&mut d, up_t, up_s, "0 2 0", "Box { }");
        d.put("  ");
        simple(&mut d, dn_t, dn_s, "0 -2 0", "Box { }");
        d.close("] }");
        d.put("\n");
    }
    for (outer, t, s) in [
        ("L", "L.up.t", "L.up.s"),
        ("L", "L.dn.t", "L.dn.s"),
        ("R", "R.up.t", "R.up.s"),
        ("R", "R.dn.t", "R.dn.s"),
    ] {
        let mut g = G::default();
        g.node("n1", "Shape", &[d.span(s)], &["n2"])
            .node("n2", "Transform", &[d.span(t)], &["n3"])
            .node("n3", "Transform", &[d.span(outer)], &["SCENE"]);
        let p = proven(run(&d.s, &g.hit("n1"), 0));
        assert_eq!((p.logical.from, p.logical.to), d.span(t), "{t}");
        // The wrong outer parent is a disagreement, never a re-match.
        let other = if outer == "L" { "R" } else { "L" };
        let mut bad = G::default();
        bad.node("n1", "Shape", &[d.span(s)], &["n2"])
            .node("n2", "Transform", &[d.span(t)], &["n3"])
            .node("n3", "Transform", &[d.span(other)], &["SCENE"]);
        refused(
            run(&d.s, &bad.hit("n1"), 0),
            Status::Unsupported,
            reason::CHAIN_DISAGREES,
        );
    }
}

#[test]
fn def_without_use_is_proven() {
    let mut d = Doc::new();
    d.open("t", "DEF Thing Transform {\n  children [ ");
    d.leaf("s", "Shape { geometry Box { size 3 3 3 } }");
    d.close(" ]\n}");
    d.put("\n");
    let p = proven(run(&d.s, &simple_graph(&d, "t", "s", 0).hit("n1"), 0));
    assert_eq!((p.logical.from, p.logical.to), d.span("t"));
    assert!(slice16(&d.s, p.logical.from, p.logical.to).starts_with("DEF Thing Transform"));
}

#[test]
fn def_with_use_and_repeated_uses_refuse_as_ambiguous() {
    // One DEF + USE of the Transform: X_ITE returns ONE runtime node for both
    // statements, so the provenance has two occurrences.
    let mut d = Doc::new();
    d.open("def.t", "DEF Thing Transform {\n  children [ ");
    d.leaf("def.s", "Shape { geometry Box { } }");
    d.close(" ]\n}\n");
    d.open("holder", "Transform { translation 6 0 0 children [ ");
    d.leaf("use1", "USE Thing");
    d.close(" ] }\n");
    let mut g = G::default();
    g.node("n1", "Shape", &[d.span("def.s")], &["n2"])
        .node(
            "n2",
            "Transform",
            &[d.span("def.t"), d.span("use1")],
            &["SCENE", "n3"],
        )
        .node("n3", "Transform", &[d.span("holder")], &["SCENE"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::RefusedAmbiguous,
        reason::SEVERAL_OCCURRENCES,
    );

    // Three USEs.
    let mut d = Doc::new();
    d.leaf("def.s", "DEF S Shape { geometry Sphere { } }");
    d.put("\n");
    let mut occ = vec![d.span("def.s")];
    for l in ["u1", "u2", "u3"] {
        d.put("Transform { children [ ");
        d.leaf(l, "USE S");
        d.put(" ] }\n");
        occ.push(d.span(l));
    }
    let mut g = G::default();
    g.node("n1", "Shape", &occ, &["SCENE", "n2", "n3", "n4"]);
    for t in ["n2", "n3", "n4"] {
        g.node(t, "Transform", &[(0, 1)], &["SCENE"]);
    }
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::RefusedAmbiguous,
        reason::SEVERAL_OCCURRENCES,
    );

    // One occurrence recorded but two live parents: still ambiguous.
    let mut d = Doc::new();
    simple(&mut d, "t", "s", "0 0 0", "Box { }");
    let mut g = simple_graph(&d, "t", "s", 0);
    g.node("n1", "Shape", &[d.span("s")], &["n2", "n2b"]).node(
        "n2b",
        "Transform",
        &[d.span("t")],
        &["SCENE"],
    );
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::RefusedAmbiguous,
        reason::SEVERAL_PARENTS,
    );
}

#[test]
fn proto_inline_sensor_and_detached_refuse() {
    let mut d = Doc::new();
    d.put("PROTO MyBox [ ] { Shape { geometry Box { } } }\n");
    d.leaf("inst", "MyBox { }");
    d.put("\n");
    let mut g = G::default();
    g.node_in("n1", "Shape", Ctx::ProtoBody, &[], &["n2"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::PROTO_INSTANCE,
    );
    // A document Shape whose parent is a PROTO body node.
    let mut d2 = Doc::new();
    simple(&mut d2, "t", "s", "0 0 0", "Box { }");
    let mut g = G::default();
    g.node("n1", "Shape", &[d2.span("s")], &["n2"]).node_in(
        "n2",
        "Transform",
        Ctx::ProtoBody,
        &[],
        &["SCENE"],
    );
    refused(
        run(&d2.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::PROTO_INSTANCE,
    );

    // Inline content.
    let mut g = G::default();
    g.node_in(
        "n1",
        "Shape",
        Ctx::ExternalScene,
        &[(0, 5)],
        &["OTHER_CONTEXT"],
    );
    refused(
        run(&d2.s, &g.hit("n1"), 0),
        Status::RefusedExternal,
        reason::OTHER_DOCUMENT,
    );
    let mut g = G::default();
    g.node("n1", "Shape", &[d2.span("s")], &["n2"]).node_in(
        "n2",
        "Transform",
        Ctx::ExternalScene,
        &[],
        &[],
    );
    // As in WD2-D: a non-document PARENT is outside the document.
    refused(
        run(&d2.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::PARENT_OUTSIDE_DOCUMENT,
    );

    // An authored sensor wins over a provable Shape.
    let mut h = simple_graph(&d2, "t", "s", 0).hit("n1");
    h.sensors = vec![Some("TouchSensor".into())];
    refused(
        run(&d2.s, &h, 0),
        Status::RefusedSensorConflict,
        reason::SENSOR,
    );
    let mut h = simple_graph(&d2, "t", "s", 0).hit("n1");
    h.sensors = vec![Some("Anchor".into())];
    refused(
        run(&d2.s, &h, 0),
        Status::RefusedSensorConflict,
        reason::SENSOR,
    );

    // Detached / outside the document.
    let mut g = G::default();
    g.node("n1", "Shape", &[d2.span("s")], &[]);
    refused(
        run(&d2.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::DETACHED,
    );
    let mut g = G::default();
    g.node("n1", "Shape", &[d2.span("s")], &["OTHER_CONTEXT"]);
    refused(
        run(&d2.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::PARENT_OUTSIDE_DOCUMENT,
    );
    let mut g = G::default();
    g.node_in("n1", "Shape", Ctx::None, &[d2.span("s")], &["SCENE"]);
    refused(
        run(&d2.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::DETACHED,
    );
}

#[test]
fn world_infrastructure_parents_are_skipped_by_identity_only() {
    let mut d = Doc::new();
    simple(&mut d, "t", "s", "0 0 0", "Box { }");
    let mut g = G::default();
    g.node("n1", "Shape", &[d.span("s")], &["n2"])
        .node("n2", "Transform", &[d.span("t")], &["SCENE", "n3"])
        .node_in("n3", "Group", Ctx::WorldInfrastructure, &[], &[]);
    let p = proven(run(&d.s, &g.hit("n1"), 0));
    assert_eq!((p.logical.from, p.logical.to), d.span("t"));
}

#[test]
fn provenance_must_join_exactly_and_agree_on_type() {
    let mut d = Doc::new();
    simple(&mut d, "t", "s", "0 0 0", "Box { }");
    let (a, b) = d.span("s");
    for occ in [(a + 1, b), (a, b - 1), (a - 1, b), (a, b + 1), (0, 0)] {
        let mut g = simple_graph(&d, "t", "s", 0);
        g.node("n1", "Shape", &[occ], &["n2"]);
        refused(
            run(&d.s, &g.hit("n1"), 0),
            Status::Unsupported,
            "exact-span-join-found-0",
        );
    }
    let mut g = simple_graph(&d, "t", "s", 0);
    g.node("n1", "Group", &[(a, b)], &["n2"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::JOIN_TYPE,
    );
    let mut g = simple_graph(&d, "t", "s", 0);
    g.nodes.get_mut("n1").unwrap().type_name = None;
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::JOIN_TYPE,
    );
    // No provenance at all.
    let mut g = simple_graph(&d, "t", "s", 0);
    g.node("n1", "Shape", &[], &["n2"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::NO_PROVENANCE,
    );
    // Missing graph link / cycle.
    let mut g = G::default();
    g.node("n1", "Shape", &[(a, b)], &["nX"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::CHAIN_UNBOUNDED,
    );
    let mut g = G::default();
    g.node("n1", "Shape", &[(a, b)], &["n2"])
        .node("n2", "Transform", &[d.span("t")], &["n1"]);
    let r = run(&d.s, &g.hit("n1"), 0);
    assert_eq!(r.status(), Status::Unsupported, "{r:?}");
    // The Shape claimed as a ROOT while the source nests it: disagreement.
    let mut g = G::default();
    g.node("n1", "Shape", &[(a, b)], &["SCENE"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::CHAIN_DISAGREES,
    );
}

#[test]
fn damaged_documents_prove_nothing() {
    let mut d = Doc::new();
    simple(&mut d, "t", "s", "0 0 0", "Box { }");
    d.put("Transform { children [ \n");
    let g = simple_graph(&d, "t", "s", 0);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::SYNTAX_ERRORS,
    );
}

/// LF, CRLF, lone CR, BOM, Unicode, comments and repeated structures: the
/// join is exact in every form, and the span slices to the authored node.
#[test]
fn source_forms_join_exactly() {
    let forms: [(&str, &str, &str, u64); 6] = [
        ("lf", "", "\n", 0),
        ("crlf", "", "\r\n", 0),
        ("cr", "", "\r", 0),
        ("bom", "\u{FEFF}", "\n", 1),
        ("bom-crlf", "\u{FEFF}", "\r\n", 1),
        ("unicode", "", "\n", 0),
    ];
    for (name, bom, nl, shift) in forms {
        let mut d = Doc::with(&format!("{bom}#VRML V2.0 utf8{nl}"));
        d.put(&format!("# comment é ✓ 😀 Shape {{ }} Transform{nl}"));
        if name == "unicode" {
            d.put(&format!("WorldInfo {{ title \"Grüße 😀 世界\" }}{nl}"));
        }
        for (t, s) in [("t0", "s0"), ("t1", "s1"), ("t2", "s2")] {
            d.open(t, &format!("Transform {{{nl}  # é{nl}  children [{nl}    "));
            d.leaf(s, "Shape { geometry Box { } }");
            d.put(&format!(" # 😀{nl}"));
            d.close(&format!("  ]{nl}}}"));
            d.put(nl);
        }
        for (t, s) in [("t0", "s0"), ("t1", "s1"), ("t2", "s2")] {
            // The preview text omits the BOM: provenance is `shift` lower.
            let g = simple_graph(&d, t, s, shift);
            let r = run(&d.s, &g.hit("n1"), shift);
            let p = match r {
                Resolution::Proven(p) => p,
                other => panic!("{name} {t}: {other:?}"),
            };
            assert_eq!((p.logical.from, p.logical.to), d.span(t), "{name} {t}");
            assert_eq!((p.shape.from, p.shape.to), d.span(s), "{name} {s}");
            assert!(slice16(&d.s, p.logical.from, p.logical.to).starts_with("Transform {"));
            assert!(slice16(&d.s, p.logical.from, p.logical.to).ends_with('}'));
            // Unshifted BOM provenance never joins.
            if shift > 0 {
                let g = simple_graph(&d, t, s, 0);
                assert_eq!(run(&d.s, &g.hit("n1"), shift).status(), Status::Unsupported);
            }
        }
    }
}

#[test]
fn script_interface_nodes_and_proto_bodies_are_indexed_but_never_document_roots() {
    // A Shape inside a PROTO body has a Proto encloser: a runtime claim that
    // it is a scene root disagrees.
    let mut d = Doc::new();
    d.put("PROTO P [ ] { ");
    d.leaf("ps", "Shape { geometry Box { } }");
    d.put(" }\nP { }\n");
    let mut g = G::default();
    g.node("n1", "Shape", &[d.span("ps")], &["SCENE"]);
    refused(
        run(&d.s, &g.hit("n1"), 0),
        Status::Unsupported,
        reason::CHAIN_DISAGREES,
    );
}

#[test]
fn refusal_texts_are_short_and_present() {
    for s in [
        Status::RefusedAmbiguous,
        Status::RefusedExternal,
        Status::RefusedSensorConflict,
        Status::RefusedStale,
        Status::Unsupported,
        Status::CompatibilityDisabled,
    ] {
        let t = refusal_text(s, "x");
        assert!(!t.is_empty() && t.len() < 160, "{s:?}");
    }
    assert!(refusal_text(Status::NoHit, "").is_empty());
}
