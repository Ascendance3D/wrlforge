// SPDX-License-Identifier: GPL-3.0-or-later
//! The viewport camera, in f64. One `View` is everything a frame used to put
//! the scene on screen: the GPU draws with it, and a pick of that frame repeats
//! it exactly (the 1×1 id pass and the CPU oracle both take the same `View`).
//!
//! Coordinates: physical pixels, origin top-left, y down; a pixel's sample
//! point is its center. Clip space is wgpu's (z in [0, 1], y up).

use glam::{DMat4, DVec3, DVec4};

/// An orbit camera around `target`. Navigation only: it never edits the
/// document (NATIVE-RENDER-1 is selection only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub target: DVec3,
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    /// Vertical field of view, radians. ISO 6.53 default: 0.785398.
    pub fov_y: f64,
    /// Radius of the scene bounds the camera was fitted to (clip planes).
    pub radius: f64,
}

impl Default for Orbit {
    /// The ISO 6.53 default viewpoint: at (0, 0, 10), looking along -Z, with
    /// the standard's literal default `fieldOfView` 0.785398 (not π/4).
    #[allow(clippy::approx_constant)]
    fn default() -> Self {
        Orbit { target: DVec3::ZERO, yaw: 0.0, pitch: 0.0, distance: 10.0, fov_y: 0.785_398, radius: 1.0 }
    }
}

pub const PITCH_LIMIT: f64 = 1.5;
const MIN_DISTANCE: f64 = 1e-3;
const MAX_DISTANCE: f64 = 1e6;

impl Orbit {
    /// Frame a bounding sphere (center, radius) from the default direction.
    pub fn fit(bounds: Option<(DVec3, f64)>) -> Orbit {
        let Some((c, r)) = bounds.filter(|(c, r)| c.is_finite() && r.is_finite() && *r > 0.0) else {
            return Orbit::default();
        };
        let mut o = Orbit { target: c, radius: r, ..Orbit::default() };
        o.distance = (r / (o.fov_y / 2.0).sin() * 1.1).clamp(MIN_DISTANCE, MAX_DISTANCE);
        o
    }

    pub fn eye(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + DVec3::new(cp * sy, sp, cp * cy) * self.distance
    }

    /// Drag by (`dx`, `dy`) logical pixels.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        if dx.is_finite() && dy.is_finite() {
            self.yaw -= dx * 0.01;
            self.pitch = (self.pitch + dy * 0.01).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        }
    }

    /// Wheel steps; positive moves closer.
    pub fn zoom(&mut self, steps: f64) {
        if steps.is_finite() {
            self.distance = (self.distance * 0.9f64.powf(steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
    }
}

/// One frame's camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub view: DMat4,
    pub proj: DMat4,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn new(o: &Orbit, width: u32, height: u32) -> View {
        let (w, h) = (width.max(1), height.max(1));
        let near = (o.distance * 0.01).max(1e-4);
        let far = o.distance + o.radius * 4.0 + 10.0;
        View {
            view: DMat4::look_at_rh(o.eye(), o.target, DVec3::Y),
            proj: DMat4::perspective_rh(o.fov_y, w as f64 / h as f64, near, far),
            width: w,
            height: h,
        }
    }

    pub fn view_proj(&self) -> DMat4 {
        self.proj * self.view
    }

    /// The world-space ray through the physical-pixel point (`x`, `y`):
    /// origin on the near plane, NON-normalized direction to the far plane,
    /// so the ray parameter of a hit orders hits the same way depth does.
    pub fn ray(&self, x: f64, y: f64) -> Option<(DVec3, DVec3)> {
        let inv = self.view_proj().inverse();
        let nx = 2.0 * x / self.width as f64 - 1.0;
        let ny = 1.0 - 2.0 * y / self.height as f64;
        let unproject = |z: f64| {
            let p = inv * DVec4::new(nx, ny, z, 1.0);
            (p.w.abs() > 1e-300).then(|| p.truncate() / p.w)
        };
        let (a, b) = (unproject(0.0)?, unproject(1.0)?);
        (a.is_finite() && b.is_finite()).then_some((a, b - a))
    }

    /// The ray through the CENTER of physical pixel (`px`, `py`).
    pub fn pixel_ray(&self, px: u32, py: u32) -> Option<(DVec3, DVec3)> {
        self.ray(px as f64 + 0.5, py as f64 + 0.5)
    }

    /// Clip-space depth (0 near .. 1 far) of a world point.
    pub fn depth(&self, p: DVec3) -> f64 {
        let c = self.view_proj() * p.extend(1.0);
        c.z / c.w
    }

    /// The projection for a 1×1 target that shows exactly physical pixel
    /// (`px`, `py`) of this view: that pixel's center maps to the target's
    /// center, so the target's one sample is the full view's sample.
    pub fn pick_proj(&self, px: u32, py: u32) -> DMat4 {
        let (w, h) = (self.width as f64, self.height as f64);
        let cx = 2.0 * (px as f64 + 0.5) / w - 1.0;
        let cy = 1.0 - 2.0 * (py as f64 + 0.5) / h;
        let pick = DMat4::from_cols(
            DVec4::new(w, 0.0, 0.0, 0.0),
            DVec4::new(0.0, h, 0.0, 0.0),
            DVec4::new(0.0, 0.0, 1.0, 0.0),
            DVec4::new(-cx * w, -cy * h, 0.0, 1.0),
        );
        pick * self.proj
    }
}
