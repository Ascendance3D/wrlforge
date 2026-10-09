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
