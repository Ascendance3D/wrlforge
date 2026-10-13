// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

/// OpenGL-style perspective (column-vector, shared element order), the
/// shape X_ITE's `Camera.perspective` builds.
fn perspective(fov: f64, near: f64, far: f64, w: f64, h: f64) -> [f64; 16] {
    let aspect = w / h;
    let f = 1.0 / (fov / 2.0).tan();
    let mut m = [0.0; 16];
    m[0] = f / aspect;
    m[5] = f;
    m[10] = (far + near) / (near - far);
    m[11] = -1.0;
    m[14] = 2.0 * far * near / (near - far);
    m
}

fn rot(axis: V3, angle: f64) -> [f64; 16] {
    let [x, y, z] = norm(axis).unwrap();
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    // Column-vector rotation, stored column-major.
    let r = [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ];
    let mut m = [0.0; 16];
    for row in 0..3 {
        for col in 0..3 {
            m[col * 4 + row] = r[row][col];
        }
    }
    m[15] = 1.0;
    m
}

fn translate(v: V3) -> [f64; 16] {
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m[12] = v[0];
    m[13] = v[1];
    m[14] = v[2];
    m
}

/// A VRML Viewpoint (position, orientation) → view matrix (world → eye).
fn camera(pos: V3, axis: V3, angle: f64, fov: f64, rect: [f64; 4], buf: [f64; 2]) -> Camera {
    let cam_to_world = mat_mul(&translate(pos), &rot(axis, angle));
    Camera {
        view: invert(&cam_to_world).unwrap(),
        proj: perspective(fov, 0.1, 1000.0, buf[0], buf[1]),
        viewport: [0.0, 0.0, buf[0], buf[1]],
        size: buf,
        rect,
    }
}

fn front() -> Camera {
    camera(
        [0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0],
        0.0,
        std::f64::consts::FRAC_PI_4,
        [100.0, 50.0, 640.0, 480.0],
        [640.0, 480.0],
    )
}

/// Orbited 40 degrees about Y and raised 25 degrees: every axis is oblique.
fn oblique() -> Camera {
    let yaw = 40f64.to_radians();
    let pitch = -25f64.to_radians();
    let orient = mat_mul(&rot([0.0, 1.0, 0.0], yaw), &rot([1.0, 0.0, 0.0], pitch));
    let back = [orient[8] * 12.0, orient[9] * 12.0, orient[10] * 12.0];
    let cam_to_world = mat_mul(&translate(back), &orient);
    Camera {
        view: invert(&cam_to_world).unwrap(),
        proj: perspective(0.9, 0.1, 1000.0, 800.0, 500.0),
        viewport: [0.0, 0.0, 800.0, 500.0],
        size: [800.0, 500.0],
        rect: [10.0, 20.0, 400.0, 250.0], // CSS px != drawing px (HiDPI-like)
    }
}

fn close(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

#[test]
fn invert_round_trips() {
    let c = oblique();
    let vp = mat_mul(&c.proj, &c.view);
    let id = mat_mul(&vp, &invert(&vp).unwrap());
    for (i, v) in id.iter().enumerate() {
        let want = if i % 5 == 0 { 1.0 } else { 0.0 };
        assert!(close(*v, want, 1e-9), "{i}: {v}");
    }
    assert!(invert(&[0.0; 16]).is_none());
}

#[test]
fn project_and_ray_agree() {
    for cam in [front(), oblique()] {
        for p in [[0.0, 0.0, 0.0], [1.5, -2.0, 0.5], [-3.0, 1.0, -4.0]] {
            let (x, y) = project(&cam, p).unwrap();
            let (o, d) = ray(&cam, x, y).unwrap();
            // The point lies on the ray: |(p - o) x d| == 0.
            let w = sub(p, o);
            let along = dot(w, d);
            let perp = sub(w, [d[0] * along, d[1] * along, d[2] * along]);
            assert!(dot(perp, perp).sqrt() < 1e-6, "{p:?} off the ray");
            assert!(along > 0.0);
        }
    }
}

#[test]
fn the_default_view_puts_the_origin_at_the_viewport_center() {
    let c = front();
    let (x, y) = project(&c, [0.0, 0.0, 0.0]).unwrap();
    assert!(close(x, 100.0 + 320.0, 1e-9) && close(y, 50.0 + 240.0, 1e-9));
}

/// Drag from the origin's screen point to the screen point of origin + d
/// along `axis`: the parameter is exactly d, in both directions.
fn drag_to(cam: &Camera, origin: V3, axis: Axis, d: f64) -> f64 {
    let p0 = project(cam, origin).unwrap();
    let p1 = project(cam, add_scaled(origin, axis.unit(), d)).unwrap();
    let t0 = axis_param(cam, origin, axis.unit(), p0.0, p0.1).unwrap();
    let t1 = axis_param(cam, origin, axis.unit(), p1.0, p1.1).unwrap();
    t1 - t0
}

#[test]
fn x_and_y_drags_move_exactly_the_projected_distance_both_ways() {
    let c = front();
    let o = [0.5, -0.25, 1.0];
    for d in [2.0, -2.0, 0.37, -3.5] {
        assert!(close(drag_to(&c, o, Axis::X, d), d, 1e-9), "X {d}");
        assert!(close(drag_to(&c, o, Axis::Y, d), d, 1e-9), "Y {d}");
    }
}

#[test]
fn every_axis_drags_correctly_from_an_oblique_camera() {
    let c = oblique();
    let o = [1.0, 0.5, -0.5];
    for axis in Axis::ALL {
        for d in [1.5, -1.5, 0.2, -4.0] {
            assert!(close(drag_to(&c, o, axis, d), d, 1e-7), "{axis:?} {d}");
        }
    }
}

/// Under perspective the closest axis point moves slightly when the pointer
/// leaves the axis; the drag stays stable (bounded, symmetric), never jumps.
#[test]
fn pointer_motion_across_the_axis_is_stable() {
    let c = front();
    let o = [0.0, 0.0, 0.0];
    let p = project(&c, [2.0, 0.0, 0.0]).unwrap();
    for (dy, tol) in [(5.0, 1e-3), (40.0, 2e-2)] {
        let up = axis_param(&c, o, Axis::X.unit(), p.0, p.1 - dy).unwrap();
        let down = axis_param(&c, o, Axis::X.unit(), p.0, p.1 + dy).unwrap();
        assert!(
            close(up, 2.0, tol) && close(down, 2.0, tol),
            "dy {dy}: {up} {down}"
        );
        assert!(close(up, down, 1e-9), "symmetric about the axis");
    }
}

#[test]
fn zoom_and_canvas_size_do_not_change_world_distance() {
    // Same world drag at two fields of view and two canvas sizes.
    for (fov, rect) in [
        (0.3, [0.0, 0.0, 320.0, 240.0]),
        (1.2, [0.0, 0.0, 1280.0, 960.0]),
    ] {
        let c = camera(
            [0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0],
            0.0,
            fov,
            rect,
            [640.0, 480.0],
        );
        assert!(close(drag_to(&c, [0.0; 3], Axis::X, 1.25), 1.25, 1e-9));
    }
}

#[test]
fn unsafe_positions_are_refused_never_guessed() {
    let c = front();
    // Z points at the viewer: the ray through the origin is parallel.
    let (x, y) = project(&c, [0.0; 3]).unwrap();
    assert_eq!(
        axis_param(&c, [0.0; 3], Axis::Z.unit(), x, y),
        Err(refusal::AXIS_PARALLEL)
    );
    let mut bad = c;
    bad.view[3] = f64::NAN;
    assert_eq!(ray(&bad, x, y), Err(refusal::CAMERA_INVALID));
    assert!(project(&bad, [0.0; 3]).is_none());
    assert_eq!(
        axis_param(&c, [f64::INFINITY, 0.0, 0.0], Axis::X.unit(), x, y),
        Err(refusal::NOT_FINITE)
    );
    // A point behind the camera is not drawable.
    assert!(project(&c, [0.0, 0.0, 20.0]).is_none());
}

#[test]
fn layout_disables_an_axis_that_points_at_the_viewer() {
    let l = layout(&front(), [0.0; 3]).unwrap();
    assert!(l.handles[0].enabled && l.handles[1].enabled);
    assert!(!l.handles[2].enabled, "Z faces the default camera");
    // Constant screen length for the enabled ones.
    for h in &l.handles[..2] {
        let t = h.tip.unwrap();
        assert!(close(
            (t.0 - l.origin.0).hypot(t.1 - l.origin.1),
            HANDLE_PX,
            1e-6
        ));
    }
    // X points right, Y points up on screen.
    assert!(l.handles[0].tip.unwrap().0 > l.origin.0);
    assert!(l.handles[1].tip.unwrap().1 < l.origin.1);
    let o = layout(&oblique(), [0.0; 3]).unwrap();
    assert!(
        o.handles.iter().all(|h| h.enabled),
        "oblique: all three usable"
    );
    assert!(
        layout(&front(), [0.0, 0.0, 30.0]).is_none(),
        "behind the camera"
    );
}

#[test]
fn decimals_follow_the_screen_precision() {
    assert_eq!(decimals_for(0.01), 2);
    assert_eq!(decimals_for(0.0123), 2);
    assert_eq!(decimals_for(0.5), 1);
    assert_eq!(decimals_for(3.0), 0);
    assert_eq!(decimals_for(1e-9), 6);
    assert_eq!(decimals_for(f64::NAN), 6);
}

#[test]
fn segment_distance_is_euclidean_to_the_segment() {
    assert!(close(
        segment_distance((5.0, 3.0), (0.0, 0.0), (10.0, 0.0)),
        3.0,
        1e-12
    ));
    assert!(close(
        segment_distance((-3.0, 4.0), (0.0, 0.0), (10.0, 0.0)),
        5.0,
        1e-12
    ));
}

// ---- VISUAL-4B: rotation rings and scale handles ----------------------------

fn ring_point(pivot: V3, axis: Axis, r: f64, th: f64) -> V3 {
    let (u, v) = ring_basis(axis);
    add_scaled(add_scaled(pivot, u, r * th.cos()), v, r * th.sin())
}

#[test]
fn ring_angles_round_trip_from_front_and_oblique_cameras() {
    let pivot = [0.4, -0.3, 0.2];
    for cam in [front(), oblique()] {
        let lay = ring_layout(&cam, pivot).unwrap();
        for ring in &lay.rings {
            if !ring.enabled {
                continue;
            }
            for th in [-2.5, -0.7, 0.0, 0.9, 3.0] {
                let (x, y) = project(&cam, ring_point(pivot, ring.axis, lay.radius, th)).unwrap();
                let got = ring_angle(&cam, pivot, ring.axis, lay.radius, x, y).unwrap();
                assert!(
                    angle_step(th, got).abs() < 1e-6,
                    "{:?} {th} {got}",
                    ring.axis
                );
            }
        }
    }
    // The oblique camera sees every ring.
    assert!(ring_layout(&oblique(), pivot)
        .unwrap()
        .rings
        .iter()
        .all(|r| r.enabled));
}

#[test]
fn edge_on_rings_are_disabled_and_their_drags_refused() {
    let cam = front();
    let lay = ring_layout(&cam, [0.0; 3]).unwrap();
    let on: Vec<_> = lay.rings.iter().map(|r| r.enabled).collect();
    assert_eq!(on, [false, false, true]);
    // A pointer ray parallel to the X ring's plane (looking down -Z, plane
    // x = 0 contains the ray through the origin).
    let o = project(&cam, [0.0; 3]).unwrap();
    assert_eq!(
        ring_angle(&cam, [0.0; 3], Axis::X, lay.radius, o.0, o.1 - 30.0),
        Err(refusal::EDGE_ON)
    );
    // At the pivot itself the angle is undefined.
    assert_eq!(
        ring_angle(&cam, [0.0; 3], Axis::Z, lay.radius, o.0 + 1.0, o.1),
        Err(refusal::TOO_CLOSE)
    );
    // The ring's screen radius is RING_PX in this front view.
    let p = lay.rings[2].points[0].unwrap();
    assert!(((p.0 - o.0).hypot(p.1 - o.1) - RING_PX).abs() < 0.5);
}

#[test]
fn angle_steps_unwrap_across_pi() {
    use std::f64::consts::PI;
    assert!((angle_step(3.0, -3.0) - (2.0 * PI - 6.0)).abs() < 1e-12);
    assert!((angle_step(-3.0, 3.0) + (2.0 * PI - 6.0)).abs() < 1e-12);
    assert_eq!(angle_step(0.5, 0.75), 0.25);
}

#[test]
fn rotate_about_matches_matrix_composition() {
    let old = [1.0, 2.0, -0.5, 0.8];
    for axis in Axis::ALL {
        let d = 0.6;
        let got = rotate_about(old, axis, d).unwrap();
        let u = axis.unit();
        let a = rot_matrix([u[0], u[1], u[2], d]).unwrap();
        let b = rot_matrix(old).unwrap();
        let want: M3 =
            [0, 1, 2].map(|r| [0, 1, 2].map(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()));
        let g = rot_matrix(got).unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert!((g[r][c] - want[r][c]).abs() < 1e-12);
            }
        }
        assert!(got[3] >= 0.0 && got[3] <= std::f64::consts::PI + 1e-12);
    }
    // Undoing the turn gives the identity, keeping the old axis.
    let back = rotate_about([0.0, 0.0, 2.0, 0.4], Axis::Z, -0.4).unwrap();
    assert_eq!(back, [0.0, 0.0, 1.0, 0.0]);
    assert!(same_rotation(
        [0.0, 1.0, 0.0, 0.3],
        [0.0, -2.0, 0.0, -0.3],
        1e-12
    ));
    assert!(!same_rotation(
        [0.0, 1.0, 0.0, 0.3],
        [0.0, 1.0, 0.0, 0.31],
        1e-3
    ));
}

#[test]
fn scale_handles_follow_the_object_frame() {
    use std::f64::consts::FRAC_PI_2;
    let dirs = scale_axes([0.0, 1.0, 0.0, FRAC_PI_2], [0.0, 0.0, 1.0, 0.0]).unwrap();
    let want = [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
    for (d, w) in dirs.iter().zip(want) {
        assert!(d.iter().zip(w).all(|(a, b)| (a - b).abs() < 1e-12), "{d:?}");
    }
    // scaleOrientation turns the scale axes inside the rotation.
    let dirs = scale_axes([0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 1.0, FRAC_PI_2]).unwrap();
    assert!((dirs[0][1] - 1.0).abs() < 1e-12);
    // A drag to twice the press distance doubles; past the pivot refuses.
    for cam in [front(), oblique()] {
        let pivot = [0.5, 0.25, -0.5];
        let lay = layout_dirs(&cam, pivot, dirs).unwrap();
        let d = dirs[0];
        let press = project(&cam, add_scaled(pivot, d, lay.length)).unwrap();
        let t0 = axis_param(&cam, pivot, d, press.0, press.1).unwrap();
        let p2 = project(&cam, add_scaled(pivot, d, 2.0 * lay.length)).unwrap();
        let f = scale_factor(&cam, pivot, d, t0, p2.0, p2.1).unwrap();
        assert!((f - 2.0).abs() < 1e-6, "{f}");
        let back = project(&cam, add_scaled(pivot, d, -0.5 * lay.length)).unwrap();
        assert_eq!(
            scale_factor(&cam, pivot, d, t0, back.0, back.1),
            Err(refusal::COLLAPSE)
        );
    }
    assert_eq!(uniform_factor(500.0, 500.0 - HANDLE_PX), 2.0);
    assert_eq!(uniform_factor(500.0, 500.0 + HANDLE_PX), 0.5);
}

#[test]
fn resizing_the_canvas_keeps_the_ring_angle() {
    let pivot = [0.0, 0.0, 0.0];
    let th = 0.8;
    for (rect, buf) in [
        ([0.0, 0.0, 640.0, 480.0], [640.0, 480.0]),
        ([30.0, 70.0, 1200.0, 500.0], [2400.0, 1000.0]),
    ] {
        let cam = camera([0.0, 1.0, 8.0], [1.0, 0.0, 0.0], -0.2, 0.9, rect, buf);
        let lay = ring_layout(&cam, pivot).unwrap();
        let (x, y) = project(&cam, ring_point(pivot, Axis::Z, lay.radius, th)).unwrap();
        let got = ring_angle(&cam, pivot, Axis::Z, lay.radius, x, y).unwrap();
        assert!((got - th).abs() < 1e-6);
    }
}

// ---- B3 (LV2): handles in a PROTO instance's parent frame ------------------

/// A conformal parent frame: rotate, scale 2, translate.
fn parent() -> [f64; 16] {
    let mut s = [0.0; 16];
    s[0] = 2.0;
    s[5] = 2.0;
    s[10] = 2.0;
    s[15] = 1.0;
    mat_mul(
        &translate([1.0, -3.0, 0.5]),
        &mat_mul(&rot([0.3, 1.0, 0.2], 0.8), &s),
    )
}

#[test]
fn the_world_frame_is_the_unchanged_identity() {
    let p = [0.4, -0.3, 0.2];
    assert_eq!(frame_point(None, p), p);
    assert_eq!(frame_dir(None, p), p);
    for cam in [front(), oblique()] {
        assert_eq!(ring_layout(&cam, p), ring_layout_in(&cam, p, None));
    }
    let m = parent();
    assert!(close(frame_point(Some(&m), [0.0; 3])[1], -3.0, 1e-12));
    let (u, l) = unit_len(frame_dir(Some(&m), [1.0, 0.0, 0.0])).unwrap();
    assert!(close(l, 2.0, 1e-12) && close(dot(u, u), 1.0, 1e-12));
}

#[test]
fn parent_frame_rings_measure_the_parent_angle() {
    // A point at parent-frame angle th on the ring about the PARENT axis
    // reads back as th: the committed Rot(axis, th) is the one drawn.
    let m = parent();
    let pivot_local = [0.2, 0.1, -0.4];
    let pivot = frame_point(Some(&m), pivot_local);
    let cam = oblique();
    let lay = ring_layout_in(&cam, pivot, Some(&m)).unwrap();
    let mut checked = 0;
    for ring in &lay.rings {
        if !ring.enabled {
            continue;
        }
        for th in [-2.5, -0.7, 0.0, 0.9, 3.0] {
            // The ring point in PARENT coordinates, radius in parent units.
            let local = ring_point(pivot_local, ring.axis, lay.radius / 2.0, th);
            let (x, y) = project(&cam, frame_point(Some(&m), local)).unwrap();
            let got = ring_angle_in(&cam, pivot, Some(&m), ring.axis, lay.radius, x, y).unwrap();
            assert!(angle_step(th, got).abs() < 1e-6, "{:?} {th} {got}", ring.axis);
            checked += 1;
        }
    }
    assert!(checked >= 10);
}

#[test]
fn parent_frame_axis_drags_return_parent_units() {
    // The Move handle runs along the image of the parent axis; the world
    // parameter divided by the frame's stretch is the parent-unit change.
    let m = parent();
    let origin_local = [0.5, 0.25, -1.0];
    let origin = frame_point(Some(&m), origin_local);
    let cam = oblique();
    for axis in Axis::ALL {
        let (u, len) = unit_len(frame_dir(Some(&m), axis.unit())).unwrap();
        let target_local = add_scaled(origin_local, axis.unit(), 0.75);
        let (x, y) = project(&cam, frame_point(Some(&m), target_local)).unwrap();
        let t = axis_param(&cam, origin, u, x, y).unwrap();
        assert!(close(t / len, 0.75, 1e-6), "{axis:?} {}", t / len);
    }
}
