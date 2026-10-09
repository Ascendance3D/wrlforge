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
    fn view_proj(&self) -> [f64; 16] {
        mat_mul(&self.proj, &self.view)
    }
    /// Normalized device coordinates → client CSS px.
    fn ndc_to_client(&self, x: f64, y: f64) -> (f64, f64) {
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

/// Place the gizmo at world `origin`; `None` when the origin cannot be drawn
/// (behind the camera, invalid camera).
pub fn layout(cam: &Camera, origin: V3) -> Option<Layout> {
    let o = project(cam, origin)?;
    // Camera right in world space: the inverse view's first column.
    let iv = invert(&cam.view)?;
    let right = norm([iv[0], iv[1], iv[2]])?;
    let r = project(cam, add_scaled(origin, right, 1.0))?;
    let px_per_unit = (r.0 - o.0).hypot(r.1 - o.1);
    if !px_per_unit.is_finite() || px_per_unit <= 1e-9 {
        return None;
    }
    let length = HANDLE_PX / px_per_unit;
    let handles = Axis::ALL.map(|axis| {
        let tip = project(cam, add_scaled(origin, axis.unit(), length));
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
