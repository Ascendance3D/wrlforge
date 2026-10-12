// SPDX-License-Identifier: GPL-3.0-or-later
//! Deterministic tessellation of the four VRML97 primitives (ISO 6.4 Box,
//! 6.42 Sphere, 6.17 Cylinder, 6.12 Cone): centered at the origin, axis +Y,
//! counter-clockwise front faces seen from outside.
//!
//! The GPU draws exactly these triangles and the CPU oracle intersects
//! exactly these triangles, so a facet edge is the same edge for both.

use crate::Geometry;
use std::f64::consts::{PI, TAU};

/// Segments around the axis for Sphere, Cylinder and Cone.
pub const SEGMENTS: u32 = 48;
/// Latitude rings for Sphere.
pub const RINGS: u32 = 24;

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Triangles, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
}

impl Mesh {
    fn new() -> Self {
        Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new() }
    }
    fn v(&mut self, p: [f64; 3], n: [f64; 3]) -> u32 {
        let i = self.positions.len() as u32;
        self.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        self.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
        i
    }
    pub fn triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        self.indices.chunks_exact(3).map(|t| [self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]])
    }
    /// Radius of the bounding sphere around the origin (oracle culling).
    pub fn radius(&self) -> f64 {
        self.positions.iter().map(|p| ((p[0] as f64).powi(2) + (p[1] as f64).powi(2) + (p[2] as f64).powi(2)).sqrt()).fold(0.0, f64::max)
    }
}

pub fn tessellate(g: &Geometry) -> Mesh {
    match *g {
        Geometry::Box { size } => box_mesh(size),
        Geometry::Sphere { radius } => sphere(radius),
        Geometry::Cylinder { radius, height, bottom, side, top } => cylinder(radius, height, bottom, side, top),
        Geometry::Cone { bottom_radius, height, bottom, side } => cone(bottom_radius, height, bottom, side),
    }
}

fn box_mesh(size: [f64; 3]) -> Mesh {
    let h = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
    // (normal axis, sign); u x w == n keeps the quad counter-clockwise.
    let faces: [([f64; 3], [f64; 3], [f64; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ];
    let mut m = Mesh::new();
    for (n, u, w) in faces {
        let base = m.positions.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = [(n[0] + u[0] * a + w[0] * b) * h[0], (n[1] + u[1] * a + w[1] * b) * h[1], (n[2] + u[2] * a + w[2] * b) * h[2]];
            m.v(p, n);
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

fn sphere(r: f64) -> Mesh {
    let mut m = Mesh::new();
    for y in 0..=RINGS {
        let t = PI * y as f64 / RINGS as f64;
        for x in 0..=SEGMENTS {
            let p = TAU * x as f64 / SEGMENTS as f64;
            // p runs from +Z towards -X; (a, a+1, b) is then CCW from outside.
            let n = [-t.sin() * p.sin(), t.cos(), t.sin() * p.cos()];
            m.v([n[0] * r, n[1] * r, n[2] * r], n);
        }
    }
    for y in 0..RINGS {
        for x in 0..SEGMENTS {
            let a = y * (SEGMENTS + 1) + x;
            let b = a + SEGMENTS + 1;
            // Pole rows: one triangle of the quad is degenerate; skip it.
            if y != 0 { m.indices.extend_from_slice(&[a, a + 1, b]); }
            if y != RINGS - 1 { m.indices.extend_from_slice(&[a + 1, b + 1, b]); }
        }
    }
    m
}

/// Point (x, z) on the unit circle at segment `i`: +Z towards +X, which is
/// counter-clockwise seen from +Y (a positive rotation about +Y).
fn ring(i: u32) -> (f64, f64) {
    let a = TAU * i as f64 / SEGMENTS as f64;
    (a.sin(), a.cos()) // (x, z)
}

fn cylinder(r: f64, height: f64, bottom: bool, side: bool, top: bool) -> Mesh {
    let mut m = Mesh::new();
    let h = height / 2.0;
    if side {
        for i in 0..SEGMENTS {
            let ((x0, z0), (x1, z1)) = (ring(i), ring(i + 1));
            let a = m.v([x0 * r, -h, z0 * r], [x0, 0.0, z0]);
            let b = m.v([x1 * r, -h, z1 * r], [x1, 0.0, z1]);
            let c = m.v([x1 * r, h, z1 * r], [x1, 0.0, z1]);
            let d = m.v([x0 * r, h, z0 * r], [x0, 0.0, z0]);
            m.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    cap(&mut m, r, h, top, true);
    cap(&mut m, r, -h, bottom, false);
    m
}

fn cap(m: &mut Mesh, r: f64, y: f64, on: bool, up: bool) {
    if !on { return; }
    let n = if up { [0.0, 1.0, 0.0] } else { [0.0, -1.0, 0.0] };
    let c = m.v([0.0, y, 0.0], n);
    for i in 0..SEGMENTS {
        let ((x0, z0), (x1, z1)) = (ring(i), ring(i + 1));
        let a = m.v([x0 * r, y, z0 * r], n);
        let b = m.v([x1 * r, y, z1 * r], n);
        // The ring runs counter-clockwise seen from +Y: (c, a, b) faces +Y,
        // (c, b, a) faces -Y.
        if up { m.indices.extend_from_slice(&[c, a, b]); } else { m.indices.extend_from_slice(&[c, b, a]); }
    }
}

fn cone(r: f64, height: f64, bottom: bool, side: bool) -> Mesh {
    let mut m = Mesh::new();
    let h = height / 2.0;
    if side {
        // Side normal: perpendicular to the slant, pointing out and up.
        let (ny, nr) = (r / (r * r + height * height).sqrt(), height / (r * r + height * height).sqrt());
        for i in 0..SEGMENTS {
            let ((x0, z0), (x1, z1)) = (ring(i), ring(i + 1));
            let a = m.v([x0 * r, -h, z0 * r], [x0 * nr, ny, z0 * nr]);
            let b = m.v([x1 * r, -h, z1 * r], [x1 * nr, ny, z1 * nr]);
            let (xa, za) = mid(i);
            let c = m.v([0.0, h, 0.0], [xa * nr, ny, za * nr]);
            m.indices.extend_from_slice(&[a, b, c]);
        }
    }
    cap(&mut m, r, -h, bottom, false);
    m
}

/// Direction halfway between segments i and i+1 (the apex normal of a facet).
fn mid(i: u32) -> (f64, f64) {
    let a = TAU * (i as f64 + 0.5) / SEGMENTS as f64;
    (a.sin(), a.cos())
}
