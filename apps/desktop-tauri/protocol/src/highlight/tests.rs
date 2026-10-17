// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

fn perspective(fov: f64, near: f64, far: f64, w: f64, h: f64) -> [f64; 16] {
    let f = 1.0 / (fov / 2.0).tan();
    let mut m = [0.0; 16];
    m[0] = f / (w / h);
    m[5] = f;
    m[10] = (far + near) / (near - far);
    m[11] = -1.0;
    m[14] = 2.0 * far * near / (near - far);
    m
}

/// A camera at (0, 0, `z`) looking down -Z, 800 x 600, canvas at (10, 20).
fn cam_at(z: f64) -> Camera {
    Camera {
        view: translate([0.0, 0.0, -z]),
        proj: perspective(std::f64::consts::FRAC_PI_4, 0.1, 1000.0, 800.0, 600.0),
        viewport: [0.0, 0.0, 800.0, 600.0],
        size: [800.0, 600.0],
        rect: [10.0, 20.0, 800.0, 600.0],
    }
}

/// X_ITE's `Box3.set(size, center)` matrix.
fn box3(size: V3, center: V3) -> [f64; 16] {
    let mut m = [0.0; 16];
    m[0] = size[0] / 2.0;
    m[5] = size[1] / 2.0;
    m[10] = size[2] / 2.0;
    m[12] = center[0];
    m[13] = center[1];
    m[14] = center[2];
    m[15] = 1.0;
    m
}

fn frame(t: V3, r: [f64; 4], s: V3) -> Frame {
    Frame {
        translation: t,
        rotation: r,
        scale: s,
        scale_orientation: [0.0, 0.0, 1.0, 0.0],
        center: [0.0; 3],
    }
}

fn close(a: V3, b: V3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

fn unit_box(ancestors: Vec<RenderedAncestor>) -> Bounds {
    Bounds {
        r#box: box3([2.0, 2.0, 2.0], [0.0; 3]),
        ancestors,
    }
}

#[test]
fn eight_corners_of_a_box_in_its_parent_frame() {
    let b = Bounds {
        r#box: box3([2.0, 4.0, 6.0], [1.0, 0.0, -1.0]),
        ancestors: vec![],
    };
    let c = world_corners(&b).unwrap();
    assert!(close(c[0], [0.0, -2.0, -4.0]));
    assert!(close(c[7], [2.0, 2.0, 2.0]));
    assert!(close(c[1], [2.0, -2.0, -4.0]));
    assert!(close(c[6], [0.0, 2.0, 2.0]));
}

#[test]
fn every_edge_joins_corners_that_differ_in_exactly_one_axis() {
    let mut seen = std::collections::BTreeSet::new();
    for (a, b) in EDGES {
        assert_eq!((a ^ b).count_ones(), 1, "{a}-{b}");
        assert!(seen.insert((a.min(b), a.max(b))), "duplicate edge {a}-{b}");
    }
    assert_eq!(seen.len(), 12);
}

#[test]
fn transform_ancestors_compose_innermost_first() {
    // Outer: translate (10, 0, 0). Inner (direct parent): scale 2.
    let inner = RenderedAncestor {
        node_type: "Transform".into(),
        frame: Some(frame([0.0; 3], [0.0, 0.0, 1.0, 0.0], [2.0, 2.0, 2.0])),
        proto_instance: false,
    };
    let group = RenderedAncestor {
        node_type: "Group".into(),
        frame: None,
        proto_instance: false,
    };
    let outer = RenderedAncestor {
        node_type: "Transform".into(),
        frame: Some(frame([10.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0; 3])),
        proto_instance: false,
    };
    let c = world_corners(&unit_box(vec![inner, group, outer])).unwrap();
    assert!(close(c[0], [8.0, -2.0, -2.0]));
    assert!(close(c[7], [12.0, 2.0, 2.0]));
}

#[test]
fn transform_matrix_follows_iso_order_with_center_and_scale_orientation() {
    // A quarter turn about Z around center (1, 0, 0), then translate.
    let f = Frame {
        translation: [0.0, 5.0, 0.0],
        rotation: [0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_2],
        scale: [1.0; 3],
        scale_orientation: [0.0, 1.0, 0.0, 0.7],
        center: [1.0, 0.0, 0.0],
    };
    let m = transform_matrix(&f).unwrap();
    let p = apply(&m, [0.0, 0.0, 0.0, 1.0]);
    // origin -> (-1,0,0) about center -> rotated (0,-1,0) -> +center (1,-1,0) -> +T
    assert!(close([p[0], p[1], p[2]], [1.0, 4.0, 0.0]), "{p:?}");
    // A non-uniform scale under a scale orientation is a pure stretch.
    let g = Frame {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 1.0, 0.0],
        scale: [2.0, 1.0, 1.0],
        scale_orientation: [0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_2],
        center: [0.0; 3],
    };
    let m = transform_matrix(&g).unwrap();
    let p = apply(&m, [1.0, 0.0, 0.0, 1.0]);
    assert!(close([p[0], p[1], p[2]], [1.0, 0.0, 0.0]), "{p:?}");
    let p = apply(&m, [0.0, 1.0, 0.0, 1.0]);
    assert!(close([p[0], p[1], p[2]], [0.0, 2.0, 0.0]), "{p:?}");
}

#[test]
fn unknown_or_unproven_ancestors_refuse() {
    for t in ["Billboard", "MyProto", "Inline", ""] {
        let a = RenderedAncestor {
            node_type: t.into(),
            frame: None,
            proto_instance: false,
        };
        assert_eq!(
            world_corners(&unit_box(vec![a])),
            Err(refusal::ANCESTOR),
            "{t}"
        );
    }
    let no_frame = RenderedAncestor {
        node_type: "Transform".into(),
        frame: None,
        proto_instance: false,
    };
    assert_eq!(
        world_corners(&unit_box(vec![no_frame])),
        Err(refusal::ANCESTOR_DISAGREES)
    );
    // B3: a proven PROTO instance link adds no frame; the same type
    // without Rust's proof still refuses.
    let inst = RenderedAncestor {
        node_type: "MyProto".into(),
        frame: None,
        proto_instance: true,
    };
    let c = world_corners(&unit_box(vec![inst])).unwrap();
    assert!(close(c[0], [-1.0, -1.0, -1.0]) && close(c[7], [1.0, 1.0, 1.0]));
    let bad = RenderedAncestor {
        node_type: "Transform".into(),
        frame: Some(frame([f64::NAN, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0; 3])),
        proto_instance: false,
    };
    assert_eq!(
        world_corners(&unit_box(vec![bad])),
        Err(refusal::NOT_FINITE)
    );
}

#[test]
fn empty_or_non_finite_boxes_refuse() {
    let empty = Bounds {
        r#box: [0.0; 16],
        ancestors: vec![],
    };
    assert_eq!(world_corners(&empty), Err(refusal::NO_BOUNDS));
    let mut nan = box3([1.0; 3], [0.0; 3]);
    nan[13] = f64::INFINITY;
    let b = Bounds {
        r#box: nan,
        ancestors: vec![],
    };
    assert_eq!(world_corners(&b), Err(refusal::NOT_FINITE));
    // A flat box (a plane) is still a box.
    let flat = Bounds {
        r#box: box3([2.0, 2.0, 0.0], [0.0; 3]),
        ancestors: vec![],
    };
    assert!(world_corners(&flat).is_ok());
}

#[test]
fn eight_corners_project_to_a_screen_outline() {
    let cam = cam_at(10.0);
    let corners = world_corners(&unit_box(vec![])).unwrap();
    let edges = outline(&cam, &corners).unwrap();
    // Centered on the canvas center (410, 320), symmetric.
    let (mut minx, mut maxx, mut miny, mut maxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for ((ax, ay), (bx, by)) in edges {
        for (x, y) in [(ax, ay), (bx, by)] {
            minx = minx.min(x);
            maxx = maxx.max(x);
            miny = miny.min(y);
            maxy = maxy.max(y);
        }
    }
    assert!((minx + maxx - 820.0).abs() < 1e-6, "{minx} {maxx}");
    assert!((miny + maxy - 640.0).abs() < 1e-6, "{miny} {maxy}");
    assert!(maxx - minx > 20.0 && maxy - miny > 20.0);
    // The near face (z = +1, corners 4..7) is drawn larger than the far one.
    let w = |a: usize, b: usize| {
        let p = gz::project(&cam, corners[a]).unwrap();
        let q = gz::project(&cam, corners[b]).unwrap();
        (p.0 - q.0).abs()
    };
    assert!(w(4, 5) > w(0, 1));
    // Edge 0 joins corners 0 and 1, exactly as projected.
    assert_eq!(edges[0].0, gz::project(&cam, corners[0]).unwrap());
    assert_eq!(edges[0].1, gz::project(&cam, corners[1]).unwrap());
}

#[test]
fn a_corner_behind_the_camera_refuses_the_whole_outline() {
    let corners = world_corners(&unit_box(vec![])).unwrap();
    // The camera sits inside the box: the back corners are behind it.
    assert_eq!(outline(&cam_at(0.5), &corners), Err(refusal::BEHIND_CAMERA));
    // Entirely behind.
    assert_eq!(
        outline(&cam_at(-10.0), &corners),
        Err(refusal::BEHIND_CAMERA)
    );
    let mut bad = cam_at(10.0);
    bad.rect[2] = 0.0;
    assert_eq!(outline(&bad, &corners), Err(refusal::CAMERA_INVALID));
    let mut c = corners;
    c[3][1] = f64::NAN;
    assert_eq!(outline(&cam_at(10.0), &c), Err(refusal::NOT_FINITE));
}

#[test]
fn svg_path_is_relative_to_the_overlay() {
    let corners = world_corners(&unit_box(vec![])).unwrap();
    let edges = outline(&cam_at(10.0), &corners).unwrap();
    let d = svg_path(&edges, 10.0, 20.0);
    assert_eq!(d.matches('M').count(), 12);
    assert_eq!(d.matches('L').count(), 12);
    let first = format!("M{:.1} {:.1}L", edges[0].0 .0 - 10.0, edges[0].0 .1 - 20.0);
    assert!(d.starts_with(&first), "{d}");
}

#[test]
fn ancestor_kinds_are_an_explicit_list() {
    assert_eq!(ancestor_kind("Transform"), Some(AncestorKind::Transform));
    for t in ["Group", "Anchor", "Collision", "Switch", "LOD", "Shape"] {
        assert_eq!(ancestor_kind(t), Some(AncestorKind::Identity), "{t}");
    }
    for t in ["Billboard", "Inline", "transform", "Proto"] {
        assert_eq!(ancestor_kind(t), None, "{t}");
    }
}

// ---- LV2: node roles, refusal messages, the precise outline ---------------

#[test]
fn node_roles_separate_geometry_owners_and_never_drawn_nodes() {
    for t in [
        "Shape",
        "Box",
        "IndexedFaceSet",
        "Transform",
        "Group",
        "Billboard",
        "MyProto",
    ] {
        assert_eq!(node_role(t), NodeRole::Rendered, "{t}");
    }
    for t in [
        "Appearance",
        "Material",
        "ImageTexture",
        "PixelTexture",
        "MovieTexture",
        "TextureTransform",
        "Coordinate",
        "Normal",
        "Color",
        "TextureCoordinate",
        "FontStyle",
    ] {
        assert_eq!(node_role(t), NodeRole::Property, "{t}");
    }
    for t in [
        "Viewpoint",
        "NavigationInfo",
        "Background",
        "TimeSensor",
        "TouchSensor",
        "PointLight",
        "Script",
        "PositionInterpolator",
        "WorldInfo",
    ] {
        assert_eq!(node_role(t), NodeRole::NotRendered, "{t}");
    }
}

#[test]
fn every_refusal_code_has_its_own_true_message() {
    let generic = refusal_message("no-such-reason");
    let codes = [
        refusal::NO_BOUNDS,
        refusal::ANCESTOR,
        refusal::ANCESTOR_DISAGREES,
        refusal::BEHIND_CAMERA,
        refusal::CAMERA_INVALID,
        refusal::NOT_FINITE,
        refusal::NOT_RENDERED,
        refusal::NO_OWNER,
        refusal::GEOMETRY_UNAVAILABLE,
        refusal::TOO_DETAILED,
        refusal::NO_EDGES,
        refusal::GEOMETRY_INVALID,
        "proto-instance-not-picked",
        "proto-instance-not-rendered",
        "proto-instance",
        "runtime-node-for-span-not-unique",
        "runtime-node-referenced-by-several-source-occurrences",
        "runtime-node-has-several-live-parents",
        "document-has-syntax-errors",
        "not-a-node",
        "preview-scene-replaced",
        "camera-unavailable",
        "adapter-unavailable",
        "runtime-ancestor-not-proven",
        "bounds-unavailable",
    ];
    for c in codes {
        assert_ne!(
            refusal_message(c),
            generic,
            "{c} falls through to the generic text"
        );
    }
    // Only the ancestor refusal may name a Billboard; the non-geometry and
    // never-drawn cases say what they really are.
    for c in codes.iter().filter(|c| **c != refusal::ANCESTOR) {
        assert!(!refusal_message(c).contains("Billboard"), "{c}");
    }
    assert!(refusal_message(refusal::ANCESTOR).contains("Billboard"));
    assert!(refusal_message(refusal::NOT_RENDERED).contains("not drawn"));
    assert!(refusal_message(refusal::NO_OWNER).contains("Shape"));
    assert!(refusal_message(refusal::NO_BOUNDS).contains("no rendered geometry"));
    // The instance-count message is not the "drawn more than once" one.
    assert_ne!(
        refusal_message("proto-instance-not-picked"),
        refusal_message("runtime-node-for-span-not-unique")
    );
}

fn report(k: u8, v: Vec<f64>, m: [f64; 16]) -> ShapeReport {
    ShapeReport {
        m,
        k,
        n: (v.len() / 3) as u32,
        v: Some(v),
    }
}

fn ident() -> [f64; 16] {
    translate([0.0; 3])
}

/// A unit cube as 12 triangles, counter-clockwise from outside.
fn cube() -> Vec<f64> {
    let p = |i: usize| -> [f64; 3] {
        let s = |b: usize| if i & (1 << b) != 0 { 1.0 } else { -1.0 };
        [s(0), s(1), s(2)]
    };
    // Faces as corner quads (outward CCW), split into two triangles each.
    let quads = [
        [0, 2, 3, 1], // -z
        [4, 5, 7, 6], // +z
        [0, 1, 5, 4], // -y
        [2, 6, 7, 3], // +y
        [0, 4, 6, 2], // -x
        [1, 3, 7, 5], // +x
    ];
    let mut v = Vec::new();
    for q in quads {
        for i in [q[0], q[1], q[2], q[0], q[2], q[3]] {
            v.extend(p(i));
        }
    }
    v
}

#[test]
fn a_cube_outline_is_its_twelve_edges_not_the_face_diagonals() {
    let m = build_mesh(&[report(3, cube(), ident())]).unwrap();
    assert_eq!(m.edge_count(), 18, "12 edges + 6 diagonals welded");
    let cam = cam_at(6.0);
    let edges = mesh_edges(&cam, &ident(), &m).unwrap();
    assert_eq!(edges.len(), 12);
    // The same picture as the bounding box of the same cube.
    let corners = world_corners(&unit_box(vec![])).unwrap();
    let boxed = outline(&cam, &corners).unwrap();
    let key = |e: &Edge| {
        let r = |v: f64| (v * 1000.0).round() as i64;
        let (a, b) = (((r(e.0 .0)), r(e.0 .1)), (r(e.1 .0), r(e.1 .1)));
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    };
    let mut x: Vec<_> = edges.iter().map(key).collect();
    let mut y: Vec<_> = boxed.iter().map(key).collect();
    x.sort();
    y.sort();
    assert_eq!(x, y);
}

/// A shallow fold: A flat (y = 0, z <= 0), B rising 20 degrees (z >= 0).
fn fold(same_direction: bool) -> Vec<f64> {
    let (p0, p1, q, r) = (
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        [0.0, 20f64.to_radians().tan(), 1.0],
    );
    let b = if same_direction {
        [p0, p1, r]
    } else {
        [p1, p0, r]
    };
    [p0, p1, q]
        .iter()
        .chain(b.iter())
        .flatten()
        .copied()
        .collect()
}

fn cam_from(pos: V3) -> Camera {
    Camera {
        view: translate([-pos[0], -pos[1], -pos[2]]),
        ..cam_at(0.0)
    }
}

#[test]
fn a_smooth_edge_is_drawn_only_on_the_silhouette() {
    for same in [false, true] {
        let m = build_mesh(&[report(3, fold(same), ident())]).unwrap();
        assert_eq!(m.edge_count(), 5);
        // Above both faces: the ridge is interior.
        let above = mesh_edges(&cam_from([0.0, 3.0, 5.0]), &ident(), &m).unwrap();
        assert_eq!(above.len(), 4, "same={same}");
        // Between the two planes: one front, one back -- the ridge is the
        // silhouette, whichever way the triangles were wound.
        let grazing = mesh_edges(&cam_from([0.0, 0.1, 5.0]), &ident(), &m).unwrap();
        assert_eq!(grazing.len(), 5, "same={same}");
    }
}

#[test]
fn shape_and_ancestor_matrices_place_the_edges() {
    let m = build_mesh(&[report(
        1,
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        translate([2.0, 0.0, 0.0]),
    )])
    .unwrap();
    let cam = cam_at(10.0);
    let parent = translate([0.0, 1.0, 0.0]);
    let e = mesh_edges(&cam, &parent, &m).unwrap();
    assert_eq!(e.len(), 1);
    let want = (
        gz::project(&cam, [2.0, 1.0, 0.0]).unwrap(),
        gz::project(&cam, [3.0, 1.0, 0.0]).unwrap(),
    );
    let close2 =
        |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
    assert!(
        close2(e[0].0, want.0) && close2(e[0].1, want.1),
        "{e:?} vs {want:?}"
    );
    // New matrices from the renderer are taken only for the same shapes.
    let mut m2 = m.clone();
    assert!(m2.update_matrices(&[report(1, vec![0.0; 6], ident())]));
    assert!(!m2.update_matrices(&[report(3, vec![0.0; 9], ident())]));
}

#[test]
fn malformed_points_only_or_huge_geometry_refuses_the_precise_outline() {
    let mut no_v = report(3, cube(), ident());
    no_v.v = None;
    assert_eq!(build_mesh(&[no_v]), Err(refusal::GEOMETRY_INVALID));
    let mut short = report(3, cube(), ident());
    short.n += 1;
    assert_eq!(build_mesh(&[short]), Err(refusal::GEOMETRY_INVALID));
    let mut nan = cube();
    nan[4] = f64::NAN;
    assert_eq!(
        build_mesh(&[report(3, nan, ident())]),
        Err(refusal::GEOMETRY_INVALID)
    );
    assert_eq!(
        build_mesh(&[report(0, vec![0.0; 9], ident())]),
        Err(refusal::NO_EDGES)
    );
    assert_eq!(build_mesh(&[]), Err(refusal::NO_EDGES));
    let huge = ShapeReport {
        m: ident(),
        k: 3,
        n: (mesh::MAX_VERTICES + 3) as u32,
        v: None,
    };
    assert_eq!(build_mesh(&[huge]), Err(refusal::TOO_DETAILED));
    // A vertex behind the camera refuses the whole precise outline.
    let m = build_mesh(&[report(3, cube(), ident())]).unwrap();
    assert_eq!(
        mesh_edges(&cam_at(0.5), &ident(), &m),
        Err(refusal::BEHIND_CAMERA)
    );
}

#[test]
fn display_prefers_the_geometry_and_falls_back_to_the_box_saying_why() {
    let cam = cam_at(6.0);
    let b = unit_box(vec![]);
    let m = build_mesh(&[report(3, cube(), ident())]).unwrap();
    match display(&cam, &b, Ok(&m)).unwrap() {
        // Straight on: the front face's 4 edges; the back face and the
        // depth edges are hidden behind it (LV2-C).
        Display::Precise {
            edges,
            hidden,
            depth,
        } => {
            assert_eq!((edges.len(), hidden.len(), depth), (4, 8, None));
        }
        other => panic!("{other:?}"),
    }
    match display(&cam, &b, Err(refusal::TOO_DETAILED)).unwrap() {
        Display::Box { reason, .. } => assert_eq!(reason, refusal::TOO_DETAILED),
        other => panic!("{other:?}"),
    }
    // An ancestor the outline cannot follow refuses BOTH: no box in the
    // wrong place either.
    let bill = unit_box(vec![RenderedAncestor {
        node_type: "Billboard".into(),
        frame: None,
        proto_instance: false,
    }]);
    assert_eq!(display(&cam, &bill, Ok(&m)), Err(refusal::ANCESTOR));
    // The precise outline is behind the camera; so is the box: nothing.
    assert_eq!(
        display(&cam_at(0.5), &b, Ok(&m)),
        Err(refusal::BEHIND_CAMERA)
    );
}

#[test]
fn selection_texts_name_type_def_line_owner_and_proto() {
    assert_eq!(
        owner_badge("Material", "Shape", Some(37)),
        "Material of Shape at line 37"
    );
    assert_eq!(
        owner_badge("Coordinate", "Shape Wheel", None),
        "Coordinate of Shape Wheel"
    );
    assert_eq!(
        proto_context("Car", 2),
        "inside PROTO Car (shared, 2 instances)"
    );
    assert_eq!(
        proto_context("Car", 1),
        "inside PROTO Car (shared, 1 instance)"
    );
    assert_eq!(
        selection_summary(
            "Material",
            None,
            Some(40),
            Some(("Shape", Some(37))),
            Some(&proto_context("Car", 2))
        ),
        "Selected: Material · line 40 · of Shape at line 37 · inside PROTO Car (shared, 2 instances)"
    );
    assert_eq!(
        selection_summary("Transform", Some("Wheel"), Some(3), None, None),
        "Selected: Transform Wheel · line 3"
    );
    assert_eq!(
        selection_summary("Box", None, None, None, None),
        "Selected: Box"
    );
}

#[test]
fn parent_extent_is_the_largest_enclosing_edge() {
    // unit_box: the cube [-1, 1]^3 itself (edge 2).
    assert_eq!(parent_extent(&unit_box(vec![])), Some(2.0));
    let mut b = unit_box(vec![]);
    b.r#box[0] = 3.0; // x half-axis 3 -> edge 6
    b.r#box[12] = 100.0; // translation does not change the size
    assert_eq!(parent_extent(&b), Some(6.0));
    // A box rotated 45 degrees about Y: the enclosing x edge grows.
    let mut r = unit_box(vec![]);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    r.r#box[0] = h;
    r.r#box[2] = -h;
    r.r#box[8] = h;
    r.r#box[10] = h;
    let e = parent_extent(&r).unwrap();
    assert!((e - 2.0 * 2.0 * h).abs() < 1e-12, "{e}");
    let mut z = unit_box(vec![]);
    z.r#box = [0.0; 16];
    assert_eq!(parent_extent(&z), None);
    z.r#box[0] = f64::NAN;
    assert_eq!(parent_extent(&z), None);
}

// ---- LV2-C: hidden edges ----------------------------------------------------

#[test]
fn a_cube_seen_from_a_corner_hides_exactly_its_three_back_edges() {
    let m = build_mesh(&[report(3, cube(), ident())]).unwrap();
    let s = split_hidden(&cam_from([4.0, 3.0, 6.0]), &ident(), &m)
        .unwrap()
        .unwrap();
    assert_eq!((s.visible.len(), s.hidden.len()), (9, 3), "{s:?}");
    // Together they are the same twelve edges as before.
    assert_eq!(
        mesh_edges(&cam_from([4.0, 3.0, 6.0]), &ident(), &m)
            .unwrap()
            .len(),
        12
    );
}

#[test]
fn a_thin_open_surface_hides_nothing() {
    // One quad (two triangles): every boundary edge is in front.
    let q = vec![
        -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0,
        0.0,
    ];
    let m = build_mesh(&[report(3, q, ident())]).unwrap();
    let s = split_hidden(&cam_from([1.0, 2.0, 5.0]), &ident(), &m)
        .unwrap()
        .unwrap();
    assert_eq!((s.visible.len(), s.hidden.len()), (4, 0));
}

#[test]
fn an_edge_partly_behind_a_face_is_split_where_it_passes_under() {
    // A line from far left to far right passing behind a quad at z = 1.
    let quad = vec![
        -0.5, -0.5, 1.0, 0.5, -0.5, 1.0, 0.5, 0.5, 1.0, -0.5, -0.5, 1.0, 0.5, 0.5, 1.0, -0.5, 0.5,
        1.0,
    ];
    let line = vec![-2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
    let m = build_mesh(&[report(3, quad, ident()), report(1, line, ident())]).unwrap();
    let cam = cam_at(6.0);
    let s = split_hidden(&cam, &ident(), &m).unwrap().unwrap();
    // The quad's 4 edges and the line's two outer parts are visible; the
    // middle of the line is hidden.
    assert_eq!((s.visible.len(), s.hidden.len()), (6, 1), "{s:?}");
    let (a, b) = s.hidden[0];
    let inner = |x: f64| {
        let l = gz::project(&cam, [-0.5, 0.0, 1.0]).unwrap().0;
        let r = gz::project(&cam, [0.5, 0.0, 1.0]).unwrap().0;
        x > l - 3.5 && x < r + 3.5
    };
    assert!(inner(a.0) && inner(b.0), "{a:?} {b:?}");
}

#[test]
fn too_much_geometry_skips_the_hidden_edge_test_and_says_so() {
    let m = build_mesh(&[report(3, cube(), ident())]).unwrap();
    // Within the limits the test runs; the limits themselves are generous.
    assert!(split_hidden(&cam_at(6.0), &ident(), &m).unwrap().is_ok());
    assert!(mesh::MAX_OCCLUDERS >= 50_000 && mesh::MAX_SAMPLES >= 200_000);
}

#[test]
fn a_stray_primitive_vertex_refuses_the_mesh() {
    // 7 line vertices / 8 triangle vertices: never a partial outline.
    assert_eq!(
        build_mesh(&[report(1, vec![0.0; 21], ident())]),
        Err(refusal::GEOMETRY_INVALID)
    );
    let mut c = cube();
    c.extend([0.0; 6]);
    assert_eq!(
        build_mesh(&[report(3, c, ident())]),
        Err(refusal::GEOMETRY_INVALID)
    );
}

#[test]
fn a_non_uniform_rendered_scale_reclassifies_creases() {
    // The 20° fold is smooth in its own coordinates; stretched 3x in Y it
    // renders at ~47.5°, a crease, so its ridge is drawn even from above.
    let mut stretch = ident();
    stretch[5] = 3.0;
    for same in [false, true] {
        let flat = build_mesh(&[report(3, fold(same), ident())]).unwrap();
        let tall = build_mesh(&[report(3, fold(same), stretch)]).unwrap();
        let above = cam_from([0.0, 9.0, 5.0]);
        assert_eq!(
            mesh_edges(&above, &ident(), &flat).unwrap().len(),
            4,
            "same={same}"
        );
        assert_eq!(
            mesh_edges(&above, &ident(), &tall).unwrap().len(),
            5,
            "same={same}"
        );
        // The same stretch from an ancestor frame counts too.
        assert_eq!(
            mesh_edges(&above, &stretch, &flat).unwrap().len(),
            5,
            "same={same}"
        );
        // A uniform scale changes no angle.
        let mut twice = ident();
        twice[0] = 2.0;
        twice[5] = 2.0;
        twice[10] = 2.0;
        assert_eq!(
            mesh_edges(&above, &twice, &flat).unwrap().len(),
            4,
            "same={same}"
        );
    }
}
