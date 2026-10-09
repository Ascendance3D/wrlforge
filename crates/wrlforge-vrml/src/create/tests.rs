// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::scene::{build_scene_tree, item_id, Kind};

fn ready(src: &str, p: Primitive) -> (u64, String, String, u64, u64, String) {
    match plan_create(src, p) {
        Plan::Ready {
            at,
            insert,
            new_text,
            node_from,
            node_to,
            def_name,
        } => (at, insert, new_text, node_from, node_to, def_name),
        other => panic!("not ready: {other:?}"),
    }
}

fn reason_of(src: &str) -> &'static str {
    match plan_create(src, Primitive::Box) {
        Plan::Refused { reason, .. } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn slice16(s: &str, from: u64, to: u64) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16(&u[from as usize..to as usize]).unwrap()
}

/// The exact-source contract: the old text is an exact prefix of the new
/// text, and only the planned insert follows it.
fn assert_pure_append(src: &str, at: u64, insert: &str, new_text: &str) {
    assert_eq!(at, src.encode_utf16().count() as u64);
    assert!(new_text.starts_with(src));
    assert_eq!(&new_text[src.len()..], insert);
}

#[test]
fn every_primitive_in_a_new_world_is_exact_and_valid() {
    for p in Primitive::ALL {
        let (at, insert, new_text, from, to, name) = ready(NEW_WORLD, p);
        assert_pure_append(NEW_WORLD, at, &insert, &new_text);
        assert_eq!(name, format!("{}_1", p.node_type()));
        let parsed = parse(&new_text);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(parsed.tree.statements.len(), 1);
        let body = slice16(&new_text, from, to);
        assert!(body.starts_with(&format!("DEF {name} Transform {{")));
        assert!(body.ends_with('}'));
        assert!(body.contains(&format!("geometry {} {{", p.node_type())));
        // The Scene Tree id of the new Transform is exactly the planned span.
        let tree = build_scene_tree(&parsed.tree, &new_text);
        let it = &tree.items[&format!("node-{from}-{to}")];
        assert_eq!(it.label, format!("Transform {name}"));
        assert!(tree
            .items
            .values()
            .any(|i| i.label == "Material" && i.depth == it.depth + 3));
    }
}

#[test]
fn box_template_is_the_documented_structure() {
    let want = "DEF Box_1 Transform {\n  translation 0 0 0\n  rotation 0 1 0 0\n  scale 1 1 1\n  children [\n    Shape {\n      appearance Appearance {\n        material Material {\n          diffuseColor 0.8 0.3 0.2\n        }\n      }\n      geometry Box {\n        size 2 2 2\n      }\n    }\n  ]\n}";
    assert_eq!(template(Primitive::Box, "Box_1", "\n"), want);
    let (_, insert, ..) = ready(NEW_WORLD, Primitive::Box);
    assert_eq!(insert, format!("\n{want}\n"));
    assert!(template(Primitive::Cone, "C", "\n").contains("bottomRadius 1\n        height 2"));
    assert!(template(Primitive::Cylinder, "C", "\n").contains("radius 1\n        height 2"));
    assert!(template(Primitive::Sphere, "C", "\n").contains("radius 1\n      }"));
}

#[test]
fn appends_after_comments_and_existing_def_nodes_crlf_bom_unicode() {
    let src = "\u{FEFF}#VRML V2.0 utf8\r\n# Tëst 😀\r\nDEF Box_1 Group { children [ USE Box_1x ] }\r\nDEF Ä Transform { translation 1 2 3 }\r\n# trailing comment ✓";
    let (at, insert, new_text, from, to, name) = ready(src, Primitive::Box);
    assert_pure_append(src, at, &insert, &new_text);
    // Box_1 is taken (DEF) and Box_1x is a different token: Box_2 is free.
    assert_eq!(name, "Box_2");
    // CRLF document: every inserted line break is CRLF, and the trailing
    // comment is closed by a new line, never extended.
    assert!(!insert.replace("\r\n", "").contains(['\r', '\n']));
    assert!(insert.starts_with("\r\n\r\nDEF Box_2 Transform {\r\n"));
    let parsed = parse(&new_text);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert_eq!(parsed.tree.statements.len(), 3);
    let tree = build_scene_tree(&parsed.tree, &new_text);
    assert!(tree.items.contains_key(&format!("node-{from}-{to}")));
}

#[test]
fn lone_cr_and_lf_documents_keep_their_line_ending() {
    let src = "#VRML V2.0 utf8\rGroup {}\r";
    let (at, insert, new_text, ..) = ready(src, Primitive::Sphere);
    assert_pure_append(src, at, &insert, &new_text);
    assert!(!insert.contains('\n'));
    assert!(insert.starts_with("\rDEF Sphere_1 Transform {\r"));
    let src = "#VRML V2.0 utf8\nGroup {}";
    let (_, insert, ..) = ready(src, Primitive::Cone);
    assert!(insert.starts_with("\n\nDEF Cone_1 Transform {\n"));
}

#[test]
fn repeated_creation_takes_fresh_names_and_preserves_prior_objects() {
    let mut text = NEW_WORLD.to_string();
    let mut spans = vec![];
    for i in 1..=3 {
        let (at, insert, new_text, from, to, name) = ready(&text, Primitive::Box);
        assert_pure_append(&text, at, &insert, &new_text);
        assert_eq!(name, format!("Box_{i}"));
        text = new_text;
        spans.push((from, to));
    }
    let parsed = parse(&text);
    assert_eq!(parsed.tree.statements.len(), 3);
    for (s, (from, to)) in parsed.tree.statements.iter().zip(spans) {
        let r = s.range();
        assert_eq!((r.start.offset as u64, r.end.offset as u64), (from, to));
    }
}

#[test]
fn identifier_uses_anywhere_block_a_name() {
    // A USE, a ROUTE end, a PROTO name, a field name: all block the name,
    // so the new DEF can never capture an existing reference.
    let src = "#VRML V2.0 utf8\nPROTO Box_1 [ field SFInt32 Box_2 0 ] { Group {} }\nROUTE Box_3.a TO Box_4.b\nGroup { children [ USE Box_5 ] }\n";
    let (.., name) = ready(src, Primitive::Box);
    assert_eq!(name, "Box_6");
}

#[test]
fn unprovable_documents_are_refused() {
    assert_eq!(reason_of(""), reason::NOT_VRML97);
    assert_eq!(reason_of("Group {}\n"), reason::NOT_VRML97);
    assert_eq!(
        reason_of("#VRML V1.0 ascii\nSeparator {}\n"),
        reason::NOT_VRML97
    );
    assert_eq!(reason_of("#X3D V3.0 utf8\nGroup {}\n"), reason::NOT_VRML97);
    // Unclosed node / PROTO: the end of the text is INSIDE a body.
    assert_eq!(
        reason_of("#VRML V2.0 utf8\nTransform { children [\n"),
        reason::SYNTAX_ERROR
    );
    assert_eq!(
        reason_of("#VRML V2.0 utf8\nPROTO P [] { Group {\n"),
        reason::SYNTAX_ERROR
    );
    assert_eq!(
        reason_of("#VRML V2.0 utf8\nWorldInfo { title \"open\n"),
        reason::SYNTAX_ERROR
    );
}

#[test]
fn ids_match_the_scene_tree_format() {
    let (_, _, new_text, from, to, _) = ready(NEW_WORLD, Primitive::Cylinder);
    let parsed = parse(&new_text);
    let r = parsed.tree.statements[0].range();
    assert_eq!(item_id(Kind::Node, r), format!("node-{from}-{to}"));
}

#[test]
fn primitive_ids_are_a_closed_set() {
    for p in Primitive::ALL {
        assert_eq!(Primitive::from_id(p.node_type()), Some(p));
    }
    for bad in ["box", "IndexedFaceSet", "Box {}", "", "Script"] {
        assert_eq!(Primitive::from_id(bad), None);
    }
}
