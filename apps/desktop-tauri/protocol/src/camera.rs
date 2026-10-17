// SPDX-License-Identifier: GPL-3.0-or-later
//! B2-CAMERA: viewport camera math for the standard views. Pure, std-only;
//! no renderer, DOM or document access.
//!
//! The renderer reports plain numbers (`FrameInput`): the scene bounds in
//! the bound viewpoint's own coordinate system, that viewpoint's authored
//! position / orientation / center of rotation and field of view, and the
//! viewport size. `standard_view` answers the viewpoint USER OFFSETS that
//! look at the bounds from one of six axis directions with the whole box in
//! view. The camera is editor state: nothing here is ever written to the
//! source.
//!
//! Conventions (ISO/IEC 14772-1 6.53): a viewpoint with no rotation looks
//! down -Z with +Y up; `fieldOfView` is the angle of the SMALLER viewport
//! dimension. Rotations are VRML axis-angle `[x, y, z, angle]` (right-handed).
//! Quaternions are Hamilton `[x, y, z, w]`; the renderer composes the user
//! orientation as `offset ⊗ orientation` (the authored orientation first),
//! which the adapter re-checks after every write.
//!
//! The framing distance mirrors the renderer's own View All rule so a
//! standard view and Fit frame alike: the box's extent across the view is
//! fitted to the smaller viewport dimension with a 10 % margin, and half the
//! box's depth is added so the near face is in front of the camera.

use serde::{Deserialize, Serialize};

/// The six axis views. Front looks along -Z (the VRML default view).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StandardView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl StandardView {
    pub const ALL: [StandardView; 6] = [
        StandardView::Front,
        StandardView::Back,
        StandardView::Left,
        StandardView::Right,
        StandardView::Top,
        StandardView::Bottom,
    ];

    /// (view direction, up) in the viewpoint's coordinate system. The camera
    /// sits on the named side of the scene and looks across it.
    pub fn look(self) -> ([f64; 3], [f64; 3]) {
        match self {
            StandardView::Front => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            StandardView::Back => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            StandardView::Left => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            StandardView::Right => ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            StandardView::Top => ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0]),
            StandardView::Bottom => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        }
    }

    /// The VRML orientation that turns the default view (-Z, up +Y) into
    /// this one, as `[x, y, z, angle]`.
    pub fn orientation(self) -> [f64; 4] {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            StandardView::Front => [0.0, 0.0, 1.0, 0.0],
            StandardView::Back => [0.0, 1.0, 0.0, PI],
            StandardView::Left => [0.0, 1.0, 0.0, -FRAC_PI_2],
            StandardView::Right => [0.0, 1.0, 0.0, FRAC_PI_2],
            StandardView::Top => [1.0, 0.0, 0.0, -FRAC_PI_2],
            StandardView::Bottom => [1.0, 0.0, 0.0, FRAC_PI_2],
        }
    }

    /// The box's extent (across, up, depth) seen from this view.
    pub fn extent(self, size: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = size;
        match self {
            StandardView::Front | StandardView::Back => [x, y, z],
            StandardView::Left | StandardView::Right => [z, y, x],
            StandardView::Top | StandardView::Bottom => [x, z, y],
        }
    }
}

/// What the adapter reports for a standard view (camelCase JSON).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameInput {
    /// Scene bounds in the viewpoint's coordinate system.
    pub center: [f64; 3],
    pub size: [f64; 3],
    /// The viewpoint's authored fields.
    pub position: [f64; 3],
    pub orientation: [f64; 4],
    pub center_of_rotation: [f64; 3],
    /// Radians; applies to the smaller viewport dimension.
    pub field_of_view: f64,
    /// Viewport width, height (px).
    pub viewport: [f64; 2],
    /// LV2-C Frame Selection: the viewpoint's model matrix (viewpoint →
    /// world, the shared element order of `gizmo::Camera`) and its current
    /// user orientation offset. Not needed by the standard views.
    #[serde(default)]
    pub model: Option<[f64; 16]>,
    #[serde(default)]
    pub orientation_offset: Option<[f64; 4]>,
}

/// The user offsets to write onto the bound viewpoint, plus the user pose
/// they must produce (the adapter re-checks it after the write).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewOffsets {
    pub position_offset: [f64; 3],
    pub orientation_offset: [f64; 4],
    pub center_of_rotation_offset: [f64; 3],
    pub user_position: [f64; 3],
    pub user_orientation: [f64; 4],
    pub distance: f64,
    /// The renderer's own near-plane rule after a View All (1.25 % of the
    /// distance).
    pub near_distance: f64,
}

/// Why a standard view cannot be computed.
pub const EMPTY_SCENE: &str = "the scene has no visible geometry to frame";
pub const BAD_INPUT: &str = "the renderer reported an unusable camera";

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// The camera distance from the box center that shows the whole box: the
/// renderer's View All rule (see the module docs). `extent` is (across, up,
/// depth); `fov` radians; `viewport` (width, height). `None` for an empty or
/// unusable input.
pub fn framing_distance(extent: [f64; 3], fov: f64, viewport: [f64; 2]) -> Option<f64> {
    let [x, y, z] = extent;
    let [w, h] = viewport;
    if !finite(&[x, y, z, fov, w, h])
        || x < 0.0
        || y < 0.0
        || z < 0.0
        || !(fov > 0.0 && fov < std::f64::consts::PI)
        || !(w > 0.0 && h > 0.0)
    {
        return None;
    }
    if x == 0.0 && y == 0.0 && z == 0.0 {
        return None;
    }
    // The box extent measured along the SMALLER viewport dimension, the one
    // the field of view spans.
    let aspect = w / h;
    let across = if w < h {
        if y == 0.0 || aspect < x / y {
            x
        } else {
            y * aspect
        }
    } else if y == 0.0 || aspect < x / y {
        x / aspect
    } else {
        y
    };
    let s = across * 1.1;
    Some(s / 2.0 / (fov / 2.0).tan() + z / 2.0)
}

/// Hamilton quaternion `[x, y, z, w]` of a VRML rotation. A zero axis is
/// the identity.
pub fn quat(r: [f64; 4]) -> [f64; 4] {
    let n = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    if !(n > 1e-12) {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let (s, c) = (r[3] / 2.0).sin_cos();
    [r[0] / n * s, r[1] / n * s, r[2] / n * s, c]
}

/// The VRML rotation of a (normalized here) quaternion; angle in [0, π].
pub fn rotation(q: [f64; 4]) -> [f64; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let mut q = if n > 0.0 {
        q.map(|v| v / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    if q[3] < 0.0 {
        q = q.map(|v| -v);
    }
    let s = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    if s < 1e-12 {
        return [0.0, 0.0, 1.0, 0.0];
    }
    [q[0] / s, q[1] / s, q[2] / s, 2.0 * s.atan2(q[3])]
}

/// Hamilton product `a ⊗ b` (b first, then a).
pub fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn conj(q: [f64; 4]) -> [f64; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// `v` rotated by the VRML rotation `r`.
pub fn rotate(r: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let q = quat(r);
    let p = quat_mul(quat_mul(q, [v[0], v[1], v[2], 0.0]), conj(q));
    [p[0], p[1], p[2]]
}

/// The user offsets that show the bounds from `view` (see the module docs).
pub fn standard_view(input: &FrameInput, view: StandardView) -> Result<ViewOffsets, &'static str> {
    let all: Vec<f64> = input
        .center
        .iter()
        .chain(&input.size)
        .chain(&input.position)
        .chain(&input.orientation)
        .chain(&input.center_of_rotation)
        .chain(&input.viewport)
        .chain(std::iter::once(&input.field_of_view))
        .copied()
        .collect();
    if !finite(&all) {
        return Err(BAD_INPUT);
    }
    if input.size.iter().any(|s| *s < 0.0) || input.size.iter().all(|s| *s == 0.0) {
        return Err(EMPTY_SCENE);
    }
    let distance = framing_distance(view.extent(input.size), input.field_of_view, input.viewport)
        .ok_or(BAD_INPUT)?;
    let (dir, _) = view.look();
    let c = input.center;
    let user_position = [
        c[0] - dir[0] * distance,
        c[1] - dir[1] * distance,
        c[2] - dir[2] * distance,
    ];
    Ok(offsets(
        input,
        c,
        user_position,
        view.orientation(),
        distance,
    ))
}

/// The user offsets that put the camera at `user_position` with
/// `user_orientation`, rotating about `c`.
fn offsets(
    input: &FrameInput,
    c: [f64; 3],
    user_position: [f64; 3],
    user_orientation: [f64; 4],
    distance: f64,
) -> ViewOffsets {
    // user = offset ⊗ authored  =>  offset = user ⊗ authored⁻¹
    let offset = quat_mul(quat(user_orientation), conj(quat(input.orientation)));
    let p = input.position;
    let r = input.center_of_rotation;
    ViewOffsets {
        position_offset: [
            user_position[0] - p[0],
            user_position[1] - p[1],
            user_position[2] - p[2],
        ],
        orientation_offset: rotation(offset),
        center_of_rotation_offset: [c[0] - r[0], c[1] - r[1], c[2] - r[2]],
        user_position,
        user_orientation,
        distance,
        near_distance: 0.0125 * distance,
    }
}

/// LV2-C Frame Selection: the user offsets that show every `world` point,
/// looking the SAME way as now (only the camera position and its center of
/// rotation move), framed by the View All rule. Camera only: the authored
/// viewpoint fields are never changed.
pub fn frame_points(input: &FrameInput, world: &[[f64; 3]]) -> Result<ViewOffsets, &'static str> {
    let (Some(model), Some(oo)) = (input.model, input.orientation_offset) else {
        return Err(BAD_INPUT);
    };
    let scalars = [input.field_of_view, input.viewport[0], input.viewport[1]];
    if world.is_empty()
        || !finite(&model)
        || !finite(&oo)
        || !finite(&input.orientation)
        || !finite(&scalars)
        || !finite(&input.position)
        || !finite(&input.center_of_rotation)
    {
        return Err(BAD_INPUT);
    }
    let inv = crate::gizmo::invert(&model).ok_or(BAD_INPUT)?;
    let user_orientation = rotation(quat_mul(quat(oo), quat(input.orientation)));
    let axes = [
        rotate(user_orientation, [1.0, 0.0, 0.0]),
        rotate(user_orientation, [0.0, 1.0, 0.0]),
        rotate(user_orientation, [0.0, 0.0, 1.0]),
    ];
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for w in world {
        let h: Vec<f64> = (0..4)
            .map(|r| inv[r] * w[0] + inv[4 + r] * w[1] + inv[8 + r] * w[2] + inv[12 + r])
            .collect();
        if !finite(&h) || h[3].abs() < 1e-12 {
            return Err(BAD_INPUT);
        }
        let l = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
        for (i, a) in axes.iter().enumerate() {
            let d = a[0] * l[0] + a[1] * l[1] + a[2] * l[2];
            lo[i] = lo[i].min(d);
            hi[i] = hi[i].max(d);
        }
    }
    let extent = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let distance =
        framing_distance(extent, input.field_of_view, input.viewport).ok_or(EMPTY_SCENE)?;
    let mid = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    let c = [0, 1, 2].map(|k| (0..3).map(|i| axes[i][k] * mid[i]).sum::<f64>());
    // The camera looks along -Z of its orientation: it stands on +Z.
    let user_position = [0, 1, 2].map(|k| c[k] + axes[2][k] * distance);
    if !finite(&user_position) {
        return Err(BAD_INPUT);
    }
    Ok(offsets(input, c, user_position, user_orientation, distance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn standard_orientations_turn_the_default_view_into_the_named_one() {
        for v in StandardView::ALL {
            let (dir, up) = v.look();
            let r = v.orientation();
            assert!(close(&rotate(r, [0.0, 0.0, -1.0]), &dir), "{v:?} direction");
            assert!(close(&rotate(r, [0.0, 1.0, 0.0]), &up), "{v:?} up");
        }
        // Front is the VRML default: no rotation at all.
        assert_eq!(StandardView::Front.orientation()[3], 0.0);
    }

    #[test]
    fn the_six_views_are_distinct_and_opposite_in_pairs() {
        let dirs: Vec<[f64; 3]> = StandardView::ALL.iter().map(|v| v.look().0).collect();
        for pair in dirs.chunks(2) {
            assert!(close(&pair[0], &pair[1].map(|x| -x)));
        }
        for i in 0..6 {
            for j in i + 1..6 {
                assert!(!close(&dirs[i], &dirs[j]));
            }
        }
    }

    #[test]
    fn all_six_standard_views_have_consistent_direction_and_up_conventions() {
        for v in StandardView::ALL {
            let (dir, up) = v.look();
            // Direction and up must be normalized.
            let dir_len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
            let up_len = (up[0] * up[0] + up[1] * up[1] + up[2] * up[2]).sqrt();
            assert!(
                (dir_len - 1.0).abs() < 1e-9,
                "{v:?} direction not normalized"
            );
            assert!((up_len - 1.0).abs() < 1e-9, "{v:?} up not normalized");

            // Direction and up must be orthogonal.
            let dot = dir[0] * up[0] + dir[1] * up[1] + dir[2] * up[2];
            assert!(dot.abs() < 1e-9, "{v:?} direction and up not orthogonal");

            // The cross product of the look direction and the up vector must be a normalized
            // right vector, establishing a consistent right-handed coordinate system.
            let right = [
                dir[1] * up[2] - dir[2] * up[1],
                dir[2] * up[0] - dir[0] * up[2],
                dir[0] * up[1] - dir[1] * up[0],
            ];
            let right_len =
                (right[0] * right[0] + right[1] * right[1] + right[2] * right[2]).sqrt();
            assert!((right_len - 1.0).abs() < 1e-9, "{v:?} right not normalized");
        }
    }

    #[test]
    fn framing_distance_fits_the_smaller_viewport_dimension() {
        // A 2 x 2 x 2 box, 90° field of view, square viewport:
        // 2 * 1.1 / 2 / tan(45°) + 1 = 2.1.
        let d = framing_distance([2.0, 2.0, 2.0], FRAC_PI_2, [400.0, 400.0]).unwrap();
        assert!((d - 2.1).abs() < 1e-12, "{d}");
        // A wide viewport: the height limits; a tall box stays height-bound.
        let tall = framing_distance([1.0, 4.0, 0.0], FRAC_PI_2, [800.0, 400.0]).unwrap();
        assert!((tall - 2.2).abs() < 1e-12, "{tall}");
        // A wide box in a wide viewport: its width is scaled to the height.
        let wide = framing_distance([8.0, 1.0, 0.0], FRAC_PI_2, [800.0, 400.0]).unwrap();
        assert!((wide - 2.2).abs() < 1e-12, "{wide}");
        // A narrow viewport: the width limits.
        let narrow = framing_distance([4.0, 1.0, 0.0], FRAC_PI_2, [400.0, 800.0]).unwrap();
        assert!((narrow - 2.2).abs() < 1e-12, "{narrow}");
    }

    #[test]
    fn framing_distance_handles_extreme_aspect_ratios() {
        let size = [2.0, 2.0, 2.0];
        // The field of view limits framing along the smaller viewport dimension.
        // For a 2x2x2 box at 90° FOV, the bounding dimension is 2.0.
        // Expected distance: 2.0 * 1.1 / 2.0 / tan(45°) + 2.0 / 2.0 = 2.1
        let very_wide = framing_distance(size, FRAC_PI_2, [100000.0, 1.0]).unwrap();
        assert!((very_wide - 2.1).abs() < 1e-12, "{very_wide}");

        let very_tall = framing_distance(size, FRAC_PI_2, [1.0, 100000.0]).unwrap();
        assert!((very_tall - 2.1).abs() < 1e-12, "{very_tall}");
    }

    #[test]
    fn framing_distance_grows_with_size_and_shrinks_with_field_of_view() {
        let vp = [640.0, 480.0];
        let small = framing_distance([1.0, 1.0, 1.0], FRAC_PI_4, vp).unwrap();
        let big = framing_distance([100.0, 100.0, 100.0], FRAC_PI_4, vp).unwrap();
        assert!((big / small - 100.0).abs() < 1e-9);
        let wide_lens = framing_distance([1.0, 1.0, 1.0], FRAC_PI_2, vp).unwrap();
        assert!(wide_lens < small);
        // The whole box is inside the frustum along the limiting dimension.
        let half = (FRAC_PI_4 / 2.0).tan() * (small - 0.5);
        assert!(half >= 0.5);
    }

    #[test]
    fn framing_distance_refuses_unusable_input() {
        let vp = [640.0, 480.0];
        assert_eq!(framing_distance([0.0, 0.0, 0.0], FRAC_PI_4, vp), None);
        assert_eq!(framing_distance([1.0, 1.0, 1.0], 0.0, vp), None);
        assert_eq!(framing_distance([1.0, 1.0, 1.0], PI, vp), None);
        assert_eq!(
            framing_distance([1.0, 1.0, 1.0], FRAC_PI_4, [0.0, 480.0]),
            None
        );
        assert_eq!(framing_distance([f64::NAN, 1.0, 1.0], FRAC_PI_4, vp), None);
        assert_eq!(framing_distance([-1.0, 1.0, 1.0], FRAC_PI_4, vp), None);
        // A flat box (a plane seen edge-on) still frames.
        assert!(framing_distance([2.0, 0.0, 2.0], FRAC_PI_4, vp).is_some());
    }

    #[test]
    fn framing_distance_handles_near_degenerate_scene_bounds() {
        let vp = [640.0, 480.0];
        // Near-degenerate scenes are tiny but > 0. Expected distances mathematically hold.
        let tiny = framing_distance([1e-15, 1e-15, 1e-15], FRAC_PI_4, vp).unwrap();
        let expected_tiny = 1.1e-15 / 2.0 / (FRAC_PI_4 / 2.0).tan() + 0.5e-15;
        assert!((tiny - expected_tiny).abs() < 1e-25, "{tiny}");

        let tiny_flat = framing_distance([1e-15, 0.0, 0.0], FRAC_PI_4, vp).unwrap();
        // For [x, 0, 0], aspect ratio limits scaling.
        // across = x / (640/480) = 1e-15 / (4/3) = 0.75e-15
        // expected: 0.75e-15 * 1.1 / 2.0 / tan(22.5°) + 0.0
        let expected_flat = (0.75e-15 * 1.1) / 2.0 / (FRAC_PI_4 / 2.0).tan();
        assert!((tiny_flat - expected_flat).abs() < 1e-25, "{tiny_flat}");
    }

    fn input() -> FrameInput {
        FrameInput {
            center: [10.0, 2.0, -3.0],
            size: [4.0, 2.0, 6.0],
            position: [0.0, 0.0, 10.0],
            orientation: [0.0, 0.0, 1.0, 0.0],
            center_of_rotation: [0.0, 0.0, 0.0],
            field_of_view: FRAC_PI_4,
            viewport: [800.0, 600.0],
            model: None,
            orientation_offset: None,
        }
    }

    #[test]
    fn a_standard_view_looks_at_the_center_from_the_framing_distance() {
        let inp = input();
        for v in StandardView::ALL {
            let o = standard_view(&inp, v).unwrap();
            let want =
                framing_distance(v.extent(inp.size), inp.field_of_view, inp.viewport).unwrap();
            assert!((o.distance - want).abs() < 1e-12);
            // The user position is the authored position plus the offset.
            let up: Vec<f64> = (0..3)
                .map(|i| inp.position[i] + o.position_offset[i])
                .collect();
            assert!(close(&up, &o.user_position));
            // Looking along the view direction from there reaches the center.
            let dir = rotate(o.user_orientation, [0.0, 0.0, -1.0]);
            let hit: Vec<f64> = (0..3)
                .map(|i| o.user_position[i] + dir[i] * o.distance)
                .collect();
            assert!(close(&hit, &inp.center), "{v:?}");
            // Orbiting turns about the scene center.
            let cor: Vec<f64> = (0..3)
                .map(|i| inp.center_of_rotation[i] + o.center_of_rotation_offset[i])
                .collect();
            assert!(close(&cor, &inp.center));
            assert!((o.near_distance - 0.0125 * o.distance).abs() < 1e-12);
        }
    }

    #[test]
    fn the_offset_composes_with_an_authored_orientation() {
        let mut inp = input();
        inp.orientation = [0.3, -1.0, 0.2, 1.1];
        for v in StandardView::ALL {
            let o = standard_view(&inp, v).unwrap();
            // user = offset ⊗ authored: the authored rotation first.
            let user = quat_mul(quat(o.orientation_offset), quat(inp.orientation));
            let want = quat(v.orientation());
            let same = close(&user, &want) || close(&user, &want.map(|x| -x));
            assert!(same, "{v:?}: {user:?} vs {want:?}");
        }
        // With the default orientation the offset IS the view orientation.
        let o = standard_view(&input(), StandardView::Right).unwrap();
        assert!(close(
            &rotate(o.orientation_offset, [0.0, 0.0, -1.0]),
            &[-1.0, 0.0, 0.0]
        ));
    }

    #[test]
    fn a_standard_view_refuses_an_empty_or_broken_scene() {
        let mut inp = input();
        inp.size = [0.0, 0.0, 0.0];
        assert_eq!(standard_view(&inp, StandardView::Top), Err(EMPTY_SCENE));
        let mut inp = input();
        inp.center[1] = f64::INFINITY;
        assert_eq!(standard_view(&inp, StandardView::Top), Err(BAD_INPUT));
        let mut inp = input();
        inp.viewport = [0.0, 0.0];
        assert_eq!(standard_view(&inp, StandardView::Top), Err(BAD_INPUT));
    }

    #[test]
    fn rotation_round_trips_through_quaternions() {
        for r in [
            [0.0, 1.0, 0.0, FRAC_PI_2],
            [1.0, 0.0, 0.0, -FRAC_PI_2],
            [0.0, 1.0, 0.0, PI],
            [0.6, 0.0, 0.8, 0.5],
        ] {
            let back = rotation(quat(r));
            let v = [0.2, -0.7, 0.4];
            assert!(close(&rotate(r, v), &rotate(back, v)), "{r:?} -> {back:?}");
        }
        assert_eq!(rotation([0.0, 0.0, 0.0, 1.0]), [0.0, 0.0, 1.0, 0.0]);
    }

    fn box_points(center: [f64; 3], size: [f64; 3], shift: [f64; 3]) -> Vec<[f64; 3]> {
        (0..8)
            .map(|i| {
                [0, 1, 2].map(|k| {
                    let s = if i & (1 << k) != 0 { 0.5 } else { -0.5 };
                    center[k] + s * size[k] + shift[k]
                })
            })
            .collect()
    }

    fn translated(t: [f64; 3]) -> [f64; 16] {
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

    fn same_view(a: &ViewOffsets, b: &ViewOffsets) -> bool {
        close(&a.user_position, &b.user_position)
            && close(
                &rotate(a.user_orientation, [0.0, 0.0, -1.0]),
                &rotate(b.user_orientation, [0.0, 0.0, -1.0]),
            )
            && close(&a.center_of_rotation_offset, &b.center_of_rotation_offset)
            && (a.distance - b.distance).abs() < 1e-9
    }

    #[test]
    fn frame_points_keeps_the_view_direction_and_frames_like_view_all() {
        let mut i = input();
        i.model = Some(translated([0.0; 3]));
        i.orientation_offset = Some([0.0, 0.0, 1.0, 0.0]);
        let pts = box_points(i.center, i.size, [0.0; 3]);
        // Looking along -Z now: the same as the Front view of that box.
        let f = frame_points(&i, &pts).unwrap();
        assert!(same_view(
            &f,
            &standard_view(&i, StandardView::Front).unwrap()
        ));
        // Turned to look along -X now: the same as the Right view.
        i.orientation_offset = Some([0.0, 1.0, 0.0, FRAC_PI_2]);
        let f = frame_points(&i, &pts).unwrap();
        assert!(same_view(
            &f,
            &standard_view(&i, StandardView::Right).unwrap()
        ));
        // A viewpoint under a Transform: world points are taken into its
        // own coordinates first.
        i.model = Some(translated([5.0, 0.0, 0.0]));
        let moved = box_points(i.center, i.size, [5.0, 0.0, 0.0]);
        assert!(same_view(&frame_points(&i, &moved).unwrap(), &f));
    }

    #[test]
    fn frame_points_refuses_without_a_usable_camera() {
        let mut i = input();
        let pts = box_points(i.center, i.size, [0.0; 3]);
        assert_eq!(frame_points(&i, &pts), Err(BAD_INPUT));
        i.model = Some(translated([0.0; 3]));
        i.orientation_offset = Some([0.0, 0.0, 1.0, 0.0]);
        assert_eq!(frame_points(&i, &[]), Err(BAD_INPUT));
        assert_eq!(frame_points(&i, &[[1.0, 1.0, 1.0]]), Err(EMPTY_SCENE));
        i.model = Some([0.0; 16]);
        assert_eq!(frame_points(&i, &pts), Err(BAD_INPUT));
    }
}
