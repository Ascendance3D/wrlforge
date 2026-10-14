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
