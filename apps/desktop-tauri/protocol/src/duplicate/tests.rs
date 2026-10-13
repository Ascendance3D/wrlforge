// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::highlight::{Frame, RenderedAncestor};

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

fn translate(t: V3) -> [f64; 16] {
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m[12] = t[0];
    m[13] = t[1];
    m[14] = t[2];
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

fn bounds(size: V3, center: V3, ancestors: Vec<RenderedAncestor>) -> Bounds {
    Bounds {
        r#box: box3(size, center),
        ancestors,
    }
}

fn transform(t: V3, r: [f64; 4], s: V3) -> RenderedAncestor {
    RenderedAncestor {
        node_type: "Transform".into(),
        frame: Some(Frame {
            translation: t,
            rotation: r,
            scale: s,
            scale_orientation: [0.0, 0.0, 1.0, 0.0],
            center: [0.0; 3],
        }),
        proto_instance: false,
    }
}

fn moved(c: &Choice) -> V3 {
    let d: Vec<f64> = (0..3).map(|i| c.copy[0][i] - c.original[0][i]).collect();
    [d[0], d[1], d[2]]
}

fn close(a: V3, b: V3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

#[test]
fn step_is_about_the_object_size_and_never_guessed() {
    assert_eq!(step(0.0), None);
    assert_eq!(step(f64::NAN), None);
    assert_eq!(step(-1.0), None);
    assert_eq!(step(2.0), Some(2.5));
    assert_eq!(step(1.0), Some(2.0));
    assert_eq!(step(0.1), Some(0.2));
    assert_eq!(step(0.02), Some(0.025));
    assert_eq!(step(40.0), Some(50.0));
    assert_eq!(step(4.0), Some(5.0));
}

#[test]
fn a_small_object_in_view_goes_to_plus_x() {
    let c = choose(&cam_at(10.0), &bounds([2.0; 3], [0.0; 3], vec![])).unwrap();
    assert_eq!(c.offset, Offset { axis: 0, step: 2.5 });
    assert!(c.in_view);
    assert!(close(moved(&c), [2.5, 0.0, 0.0]));
}

#[test]
fn an_object_at_the_right_edge_goes_left() {
    // +X would leave the canvas (half-width ~5.5 at this distance).
    let c = choose(&cam_at(10.0), &bounds([2.0; 3], [3.5, 0.0, 0.0], vec![])).unwrap();
    assert_eq!(
        c.offset,
        Offset {
            axis: 0,
            step: -2.5
        }
    );
    assert!(c.in_view);
}

#[test]
fn an_axis_along_the_view_is_skipped() {
    // The parent is turned 90° about Y: its X runs along the line of
    // sight, so +X / -X do not move the copy apart on screen.
    let parent = transform(
        [0.0; 3],
        [0.0, 1.0, 0.0, std::f64::consts::FRAC_PI_2],
        [1.0; 3],
    );
    let c = choose(&cam_at(10.0), &bounds([2.0; 3], [0.0; 3], vec![parent])).unwrap();
    assert_eq!(c.offset, Offset { axis: 1, step: 2.5 });
    assert!(c.in_view);
    assert!(close(moved(&c), [0.0, 2.5, 0.0]));
}

#[test]
fn a_scaled_nested_parent_scales_the_world_move() {
    // The step is in PARENT units; the parent chain doubles it on screen.
    let inner = transform([1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0; 3]);
    let outer = transform([0.0; 3], [0.0, 0.0, 1.0, 0.0], [2.0; 3]);
    let c = choose(
        &cam_at(30.0),
        &bounds([1.0; 3], [0.0; 3], vec![inner, outer]),
    )
    .unwrap();
    assert_eq!(c.offset, Offset { axis: 0, step: 2.0 });
    assert!(c.in_view);
    assert!(close(moved(&c), [4.0, 0.0, 0.0]));
}

#[test]
fn an_object_filling_the_view_is_framed() {
    // No copy can fit: +X (apart) is taken and the caller frames the view.
    let c = choose(&cam_at(10.0), &bounds([7.0, 7.0, 1.0], [0.0; 3], vec![])).unwrap();
    assert!(!c.in_view);
    assert_eq!(c.offset.axis, 0);
    assert!(c.offset.step > 0.0);
}

#[test]
fn an_object_behind_the_camera_is_framed() {
    let c = choose(&cam_at(10.0), &bounds([1.0; 3], [0.0, 0.0, 20.0], vec![])).unwrap();
    assert!(!c.in_view);
    assert_eq!(c.offset, Offset { axis: 0, step: 2.0 });
}

#[test]
fn no_proven_size_is_no_choice() {
    let empty = Bounds {
        r#box: [0.0; 16],
        ancestors: vec![],
    };
    assert_eq!(choose(&cam_at(10.0), &empty), Err(NO_SIZE));
    let unknown = RenderedAncestor {
        node_type: "Mystery".into(),
        frame: None,
        proto_instance: false,
    };
    assert!(choose(&cam_at(10.0), &bounds([1.0; 3], [0.0; 3], vec![unknown])).is_err());
}

#[test]
fn offsets_are_validated() {
    assert!(Offset {
        axis: 2,
        step: -0.5
    }
    .valid());
    assert!(!Offset { axis: 3, step: 1.0 }.valid());
    assert!(!Offset { axis: 0, step: 0.0 }.valid());
    assert!(!Offset {
        axis: 0,
        step: f64::INFINITY
    }
    .valid());
    assert_eq!(Offset { axis: 1, step: 1.0 }.axis_name(), 'Y');
}
