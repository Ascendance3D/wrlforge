// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A translation-gizmo math: pure, std-only, shared by the UI (live
//! drag) and the tests. No renderer, DOM or document access.
//!
//! The renderer reports its camera as plain numbers (`Camera`); everything
//! here is computed from those numbers, never from a guessed pixel scale:
//!
//! * `project` maps a world point to client CSS px through the exact view and
//!   projection matrices, the layer viewport and the canvas rectangle;
//! * `ray` is the inverse: the world-space pick ray under a client point;
//! * `axis_param` is the axis-constrained drag: the parameter of the point on
//!   the axis line closest to the pointer ray. It refuses when the ray is
//!   nearly parallel to the axis, the point is behind the camera, or any value
//!   is not finite -- the caller keeps the last safe value and never commits
//!   an unsafe one;
//! * `layout` places the three handles at a constant screen length and
//!   disables a handle whose axis points (nearly) at the viewer.
//!
//! VISUAL-4B adds the rotation rings and the scale handles:
//!
//! * `ring_layout` draws one ring per PARENT (world) axis around the pivot
//!   `T + C` at a constant screen radius; a ring seen (nearly) edge-on is
//!   disabled;
//! * `ring_angle` is the rotation drag: the pointer ray meets the ring's
//!   plane, and the angle of that point about the axis is measured in the
//!   ring's own right-handed basis. A ray (nearly) in the plane, a point
//!   behind the camera or a point too close to the pivot is refused;
//! * `scale_axes` gives the directions the three scale components stretch
//!   in the world (`R · SR · e_i`), so the handles follow the object's own
//!   scale frame, never the screen;
//! * `rotate_about` composes `Rot(axis, delta) · R` as quaternions (the same
//!   rule `wrlforge_vrml::manipulate::rotate_about` commits; a test in the
//!   desktop crate holds the two equal).
//!
//! Design reference (conceptually informed only, no code taken): White Dune's
//! `Node::getHandle()` / `Node::setHandle()` split -- a handle reports a
//! position, a drag produces a constrained value for one field. See
//! `OPEN_SOURCE_PROVENANCE.md` section 4.1.

use serde::{Deserialize, Serialize};

pub type V3 = [f64; 3];

/// One camera state, as the renderer drew it.
///
/// Matrices use X_ITE's `Matrix4` element order (`m00 m01 … m33`, row-vector
/// convention, translation in elements 12–14), which is the same memory
/// order as an OpenGL column-major matrix: element `(row r, col c)` of the
/// column-vector matrix is `m[c * 4 + r]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    /// World → eye.
    pub view: [f64; 16],
    /// Eye → clip.
    pub proj: [f64; 16],
    /// The layer viewport in drawing-buffer px, origin bottom-left: x, y, w, h.
    pub viewport: [f64; 4],
    /// The drawing-buffer size the viewport lives in: w, h.
    pub size: [f64; 2],
    /// The canvas element's client rectangle (CSS px): left, top, w, h.
    pub rect: [f64; 4],
}

/// VISUAL-4B: the viewport manipulation tool.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Move,
    Rotate,
    Scale,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Move => "Move",
            Tool::Rotate => "Rotate",
            Tool::Scale => "Scale",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
    pub fn unit(self) -> V3 {
        let mut v = [0.0; 3];
        v[self.index()] = 1.0;
        v
    }
    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }
}

/// Why a position cannot be computed safely.
pub mod refusal {
    pub const CAMERA_INVALID: &str = "camera-not-invertible";
    pub const AXIS_PARALLEL: &str = "axis-parallel-to-view";
    pub const BEHIND_CAMERA: &str = "point-behind-camera";
    pub const NOT_FINITE: &str = "value-not-finite";
    pub const EDGE_ON: &str = "ring-edge-on-to-view";
    pub const TOO_CLOSE: &str = "pointer-too-close-to-pivot";
    pub const COLLAPSE: &str = "scale-would-collapse-or-flip";
}

/// The handle length on screen (CSS px) and the shortest drawn axis that
/// is still usable: shorter means the axis points (nearly) at the viewer.
pub const HANDLE_PX: f64 = 90.0;
pub const MIN_HANDLE_PX: f64 = 14.0;
/// sin² of the smallest angle between the pointer ray and the axis that a
/// drag accepts (about 3 degrees).
pub const MIN_SIN2: f64 = 0.0027;

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn mul4(m: &[f64; 16], v: [f64; 4]) -> [f64; 4] {
    let mut o = [0.0; 4];
    for (r, out) in o.iter_mut().enumerate() {
        *out = (0..4).map(|c| m[c * 4 + r] * v[c]).sum();
    }
    o
}

/// `a * b` (column-vector convention, both in the shared element order).
pub fn mat_mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

/// General 4x4 inverse (cofactors); `None` when singular or not finite.
pub fn invert(m: &[f64; 16]) -> Option<[f64; 16]> {
    let mut inv = [0.0; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if !det.is_finite() || det.abs() < 1e-300 {
        return None;
    }
    for v in inv.iter_mut() {
        *v /= det;
    }
    finite(&inv).then_some(inv)
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add_scaled(a: V3, d: V3, s: f64) -> V3 {
    [a[0] + d[0] * s, a[1] + d[1] * s, a[2] + d[2] * s]
}
fn norm(a: V3) -> Option<V3> {
    let l = dot(a, a).sqrt();
    (l.is_finite() && l > 1e-300).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

impl Camera {
    pub fn valid(&self) -> bool {
        finite(&self.view)
            && finite(&self.proj)
            && finite(&self.viewport)
            && finite(&self.size)
            && finite(&self.rect)
            && self.viewport[2] > 0.0
            && self.viewport[3] > 0.0
            && self.size[0] > 0.0
            && self.size[1] > 0.0
            && self.rect[2] > 0.0
            && self.rect[3] > 0.0
    }
    pub(crate) fn view_proj(&self) -> [f64; 16] {
        mat_mul(&self.proj, &self.view)
    }
    /// Normalized device coordinates → client CSS px.
    pub(crate) fn ndc_to_client(&self, x: f64, y: f64) -> (f64, f64) {
        let [vx, vy, vw, vh] = self.viewport;
        let px = vx + (x + 1.0) / 2.0 * vw;
        let py = vy + (y + 1.0) / 2.0 * vh;
        let [l, t, w, h] = self.rect;
        (l + px / self.size[0] * w, t + (1.0 - py / self.size[1]) * h)
    }
    fn client_to_ndc(&self, cx: f64, cy: f64) -> (f64, f64) {
        let [l, t, w, h] = self.rect;
        let px = (cx - l) / w * self.size[0];
        let py = (1.0 - (cy - t) / h) * self.size[1];
        let [vx, vy, vw, vh] = self.viewport;
        ((px - vx) / vw * 2.0 - 1.0, (py - vy) / vh * 2.0 - 1.0)
    }
}

/// World point → client CSS px; `None` when behind the camera, not finite,
/// or the camera is invalid.
pub fn project(cam: &Camera, p: V3) -> Option<(f64, f64)> {
    if !cam.valid() || !finite(&p) {
        return None;
    }
    let c = mul4(&cam.view_proj(), [p[0], p[1], p[2], 1.0]);
    if !finite(&c) || c[3] <= 1e-12 {
        return None;
    }
    let (x, y) = cam.ndc_to_client(c[0] / c[3], c[1] / c[3]);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The world-space ray under client point (`cx`, `cy`): (origin on the near
/// plane, unit direction away from the viewer).
pub fn ray(cam: &Camera, cx: f64, cy: f64) -> Result<(V3, V3), &'static str> {
    if !cam.valid() || !cx.is_finite() || !cy.is_finite() {
        return Err(refusal::CAMERA_INVALID);
    }
    let inv = invert(&cam.view_proj()).ok_or(refusal::CAMERA_INVALID)?;
    let (x, y) = cam.client_to_ndc(cx, cy);
    let un = |z: f64| -> Option<V3> {
        let h = mul4(&inv, [x, y, z, 1.0]);
        (h[3].abs() > 1e-300 && finite(&h)).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])
    };
    let near = un(-1.0).ok_or(refusal::NOT_FINITE)?;
    let far = un(1.0).ok_or(refusal::NOT_FINITE)?;
    let dir = norm(sub(far, near)).ok_or(refusal::NOT_FINITE)?;
    Ok((near, dir))
}

/// The parameter `t` of the point `origin + t * axis` closest to the pointer
/// ray under (`cx`, `cy`). `axis` must be a unit vector.
pub fn axis_param(
    cam: &Camera,
    origin: V3,
    axis: V3,
    cx: f64,
    cy: f64,
) -> Result<f64, &'static str> {
    if !finite(&origin) || !finite(&axis) {
        return Err(refusal::NOT_FINITE);
    }
    let (o, r) = ray(cam, cx, cy)?;
    let b = dot(axis, r);
    let denom = 1.0 - b * b;
    if denom.is_nan() || denom < MIN_SIN2 {
        return Err(refusal::AXIS_PARALLEL);
    }
    let w0 = sub(origin, o);
    let d = dot(axis, w0);
    let e = dot(r, w0);
    let t = (b * e - d) / denom;
    let s = (e - b * d) / denom;
    if !t.is_finite() || !s.is_finite() {
        return Err(refusal::NOT_FINITE);
    }
    if s <= 0.0 {
        return Err(refusal::BEHIND_CAMERA);
    }
    Ok(t)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handle {
    pub axis: Axis,
    /// The handle tip in client px (drawn even when disabled, if in front).
    pub tip: Option<(f64, f64)>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The gizmo origin in client px.
    pub origin: (f64, f64),
    /// World length of a handle (constant on-screen length `HANDLE_PX`).
    pub length: f64,
    /// World units per client px at the origin's depth (value precision).
    pub units_per_px: f64,
    pub handles: [Handle; 3],
}

/// Screen px per world unit at `origin`'s depth.
fn px_per_unit(cam: &Camera, origin: V3) -> Option<((f64, f64), f64)> {
    let o = project(cam, origin)?;
    // Camera right in world space: the inverse view's first column.
    let iv = invert(&cam.view)?;
    let right = norm([iv[0], iv[1], iv[2]])?;
    let r = project(cam, add_scaled(origin, right, 1.0))?;
    let px = (r.0 - o.0).hypot(r.1 - o.1);
    (px.is_finite() && px > 1e-9).then_some((o, px))
}

/// Place the gizmo at world `origin`; `None` when the origin cannot be drawn
/// (behind the camera, invalid camera).
pub fn layout(cam: &Camera, origin: V3) -> Option<Layout> {
    layout_dirs(cam, origin, Axis::ALL.map(Axis::unit))
}

/// `layout` along three arbitrary world directions (unit vectors), in axis
/// order: the scale handles follow the object's scale frame.
pub fn layout_dirs(cam: &Camera, origin: V3, dirs: [V3; 3]) -> Option<Layout> {
    let (o, px_per_unit) = px_per_unit(cam, origin)?;
    let length = HANDLE_PX / px_per_unit;
    let handles = Axis::ALL.map(|axis| {
        let tip = project(cam, add_scaled(origin, dirs[axis.index()], length));
        let enabled = tip.is_some_and(|t| (t.0 - o.0).hypot(t.1 - o.1) >= MIN_HANDLE_PX);
        Handle { axis, tip, enabled }
    });
    Some(Layout {
        origin: o,
        length,
        units_per_px: 1.0 / px_per_unit,
        handles,
    })
}

pub type M3 = [[f64; 3]; 3];

/// The rotation matrix of an SFRotation (axis need not be unit length);
/// `None` for a zero axis with a non-zero angle or a non-finite value.
pub fn rot_matrix(r: [f64; 4]) -> Option<M3> {
    let [x, y, z, a] = r;
    if a == 0.0 {
        return Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }
    let [x, y, z] = norm([x, y, z])?;
    if !a.is_finite() {
        return None;
    }
    let (s, c) = a.sin_cos();
    let t = 1.0 - c;
    Some([
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ])
}

pub fn mat3_vec(m: &M3, v: V3) -> V3 {
    [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

/// Two SFRotation values describe the same orientation (matrix elements
/// within `tol`).
pub fn same_rotation(a: [f64; 4], b: [f64; 4], tol: f64) -> bool {
    match (rot_matrix(a), rot_matrix(b)) {
        (Some(x), Some(y)) => (0..3).all(|r| (0..3).all(|c| (x[r][c] - y[r][c]).abs() <= tol)),
        _ => false,
    }
}

type Quat = [f64; 4];

fn quat_of(r: [f64; 4]) -> Option<Quat> {
    if r[3] == 0.0 {
        return Some([1.0, 0.0, 0.0, 0.0]);
    }
    let [x, y, z] = norm([r[0], r[1], r[2]])?;
    let (s, c) = (r[3] / 2.0).sin_cos();
    finite(&[s, c]).then_some([c, s * x, s * y, s * z])
}

fn quat_mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// `Rot(axis, delta) · old` (parent-frame rotation) as axis-angle: unit
/// axis, angle in [0, π]. The identity keeps `old`'s axis.
pub fn rotate_about(old: [f64; 4], axis: Axis, delta: f64) -> Option<[f64; 4]> {
    let u = axis.unit();
    let q = quat_mul(quat_of([u[0], u[1], u[2], delta])?, quat_of(old)?);
    let q = if q[0] < 0.0 { q.map(|v| -v) } else { q };
    let s = (q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let out = if s <= 1e-12 {
        match norm([old[0], old[1], old[2]]) {
            Some(a) => [a[0], a[1], a[2], 0.0],
            None => [u[0], u[1], u[2], 0.0],
        }
    } else {
        [q[1] / s, q[2] / s, q[3] / s, 2.0 * s.atan2(q[0])]
    };
    finite(&out).then_some(out)
}

/// The world directions the three scale components stretch: `R · SR · e_i`.
pub fn scale_axes(rotation: [f64; 4], scale_orientation: [f64; 4]) -> Option<[V3; 3]> {
    let r = rot_matrix(rotation)?;
    let sr = rot_matrix(scale_orientation)?;
    let dirs = Axis::ALL.map(|a| norm(mat3_vec(&r, mat3_vec(&sr, a.unit()))));
    Some([dirs[0]?, dirs[1]?, dirs[2]?])
}

/// The ring radius on screen (CSS px), the number of drawn segments, the
/// smallest |cos| between a ring's axis and the line of sight for a usable
/// ring (about 7 degrees off edge-on), and for a usable drag ray (about 3
/// degrees), and the smallest pointer arm (fraction of the radius).
pub const RING_PX: f64 = 70.0;
pub const RING_SEGMENTS: usize = 64;
pub const MIN_RING_COS: f64 = 0.12;
pub const MIN_PLANE_COS: f64 = 0.05;
pub const MIN_ARM: f64 = 0.15;

/// The ring's right-handed in-plane basis (u × v = axis).
fn ring_basis(axis: Axis) -> (V3, V3) {
    match axis {
        Axis::X => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        Axis::Y => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        Axis::Z => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub axis: Axis,
    /// The projected ring, `RING_SEGMENTS + 1` points (closed); `None` for a
    /// point behind the camera.
    pub points: Vec<Option<(f64, f64)>>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RingLayout {
    /// The pivot in client px.
    pub origin: (f64, f64),
    /// World radius of every ring (constant on-screen radius `RING_PX`).
    pub radius: f64,
    pub rings: [Ring; 3],
}

/// The camera's line of sight towards world point `p` (unit).
fn sight(cam: &Camera, p: V3) -> Option<V3> {
    let iv = invert(&cam.view)?;
    // An orthographic projection (w row 0 0 0 1) looks along -Z of the eye.
    if cam.proj[11] == 0.0 && cam.proj[15] == 1.0 {
        return norm([-iv[8], -iv[9], -iv[10]]);
    }
    norm(sub(p, [iv[12], iv[13], iv[14]]))
}

/// Place the three rotation rings around world `pivot`.
pub fn ring_layout(cam: &Camera, pivot: V3) -> Option<RingLayout> {
    let (o, px_per_unit) = px_per_unit(cam, pivot)?;
    let radius = RING_PX / px_per_unit;
    let look = sight(cam, pivot)?;
    let rings = Axis::ALL.map(|axis| {
        let (u, v) = ring_basis(axis);
        let points: Vec<_> = (0..=RING_SEGMENTS)
            .map(|k| {
                let th = k as f64 / RING_SEGMENTS as f64 * std::f64::consts::TAU;
                let p = add_scaled(
                    add_scaled(pivot, u, radius * th.cos()),
                    v,
                    radius * th.sin(),
                );
                project(cam, p)
            })
            .collect();
        let enabled =
            dot(axis.unit(), look).abs() >= MIN_RING_COS && points.iter().all(Option::is_some);
        Ring {
            axis,
            points,
            enabled,
        }
    });
    Some(RingLayout {
        origin: o,
        radius,
        rings,
    })
}

/// The point where the pointer ray under (`cx`, `cy`) meets the plane
/// through `pivot` with unit normal `n`.
pub fn plane_point(cam: &Camera, pivot: V3, n: V3, cx: f64, cy: f64) -> Result<V3, &'static str> {
    if !finite(&pivot) || !finite(&n) {
        return Err(refusal::NOT_FINITE);
    }
    let (o, r) = ray(cam, cx, cy)?;
    let denom = dot(n, r);
    if denom.is_nan() || denom.abs() < MIN_PLANE_COS {
        return Err(refusal::EDGE_ON);
    }
    let s = dot(n, sub(pivot, o)) / denom;
    if !s.is_finite() {
        return Err(refusal::NOT_FINITE);
    }
    if s <= 0.0 {
        return Err(refusal::BEHIND_CAMERA);
    }
    let p = add_scaled(o, r, s);
    if finite(&p) {
        Ok(p)
    } else {
        Err(refusal::NOT_FINITE)
    }
}

/// The rotation drag: the angle (radians, right-handed about `axis`) of the
/// pointer's point on the ring plane around `pivot`. `radius` is the ring's
/// world radius (a pointer closer than `MIN_ARM` of it is refused).
pub fn ring_angle(
    cam: &Camera,
    pivot: V3,
    axis: Axis,
    radius: f64,
    cx: f64,
    cy: f64,
) -> Result<f64, &'static str> {
    let p = plane_point(cam, pivot, axis.unit(), cx, cy)?;
    let a = sub(p, pivot);
    let l = dot(a, a).sqrt();
    if !l.is_finite() || !radius.is_finite() {
        return Err(refusal::NOT_FINITE);
    }
    if l < MIN_ARM * radius {
        return Err(refusal::TOO_CLOSE);
    }
    let (u, v) = ring_basis(axis);
    let th = dot(a, v).atan2(dot(a, u));
    if th.is_finite() {
        Ok(th)
    } else {
        Err(refusal::NOT_FINITE)
    }
}

/// `b - a` wrapped into (-π, π]: the step between two ring angles.
pub fn angle_step(a: f64, b: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    let mut d = (b - a) % TAU;
    if d <= -PI {
        d += TAU;
    } else if d > PI {
        d -= TAU;
    }
    d
}

/// The smallest scale factor one drag may produce (no collapse, no flip).
pub const MIN_FACTOR: f64 = 1e-3;

/// The axis scale drag: the pointer's parameter along the handle line,
/// relative to the parameter at the press (`t_start`, > 0).
pub fn scale_factor(
    cam: &Camera,
    pivot: V3,
    dir: V3,
    t_start: f64,
    cx: f64,
    cy: f64,
) -> Result<f64, &'static str> {
    if !t_start.is_finite() || t_start <= 0.0 {
        return Err(refusal::TOO_CLOSE);
    }
    let t = axis_param(cam, pivot, dir, cx, cy)?;
    let f = t / t_start;
    if !f.is_finite() {
        return Err(refusal::NOT_FINITE);
    }
    if f < MIN_FACTOR {
        return Err(refusal::COLLAPSE);
    }
    Ok(f)
}

/// The uniform scale drag: screen-vertical only (up enlarges), doubling per
/// `HANDLE_PX` px; independent of the camera because a uniform scale has no
/// direction.
pub fn uniform_factor(y_start: f64, y: f64) -> f64 {
    2f64.powf((y_start - y) / HANDLE_PX)
}

/// Distance from client point `p` to a projected polyline (px).
pub fn polyline_distance(p: (f64, f64), pts: &[Option<(f64, f64)>]) -> f64 {
    pts.windows(2)
        .filter_map(|w| Some(segment_distance(p, w[0]?, w[1]?)))
        .fold(f64::INFINITY, f64::min)
}

/// Decimal places worth keeping for a drag at `units_per_px` (one screen
/// pixel of movement is never rounded away; never more than 6).
pub fn decimals_for(units_per_px: f64) -> u8 {
    if !units_per_px.is_finite() || units_per_px <= 0.0 {
        return 6;
    }
    (-units_per_px.log10()).ceil().clamp(0.0, 6.0) as u8
}

/// Distance from client point `p` to the segment `a`–`b` (px).
pub fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

#[cfg(test)]
mod tests;
