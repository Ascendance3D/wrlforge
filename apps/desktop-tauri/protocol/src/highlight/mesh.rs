// SPDX-License-Identifier: GPL-3.0-or-later
//! LV2 precise selection outline: the edges of the selected node's RENDERED
//! geometry, projected through the renderer's real camera. Pure, std-only.
//!
//! The renderer reports the shapes X_ITE itself draws for the proven node
//! (`getShapes` on that node: shape → node-parent matrix, geometry type and
//! the triangle / line vertex list). [`build_mesh`] welds each shape's
//! vertices by exact position, so triangles that share a source point share
//! an edge, and classifies every edge once:
//!
//! * a **boundary** edge (one triangle), a **crease** (the two triangles
//!   meet at more than [`CREASE_DEGREES`]) or a non-manifold edge is always
//!   drawn; a line segment is always drawn;
//! * a **smooth** edge is drawn only while it is on the silhouette: its two
//!   triangles face opposite ways on screen (decided per frame from their
//!   projected winding, so it is exact under any affine frame).
//!
//! Nothing is guessed: a vertex behind the camera, a non-finite value, a
//! malformed report or more geometry than [`MAX_VERTICES`] /
//! [`MAX_SEGMENTS`] refuses the precise outline, and [`display`] falls back
//! to the bounding box (or to nothing), saying why.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{ancestor_matrix, outline, refusal, world_corners, Bounds, Edge};
use crate::gizmo::{self as gz, Camera, V3};

/// Total reported vertices above which no precise outline is attempted.
pub const MAX_VERTICES: usize = 300_000;
/// Drawn segments above which the precise outline is refused for the frame.
pub const MAX_SEGMENTS: usize = 40_000;
/// Two triangles meeting at more than this are a crease (always drawn).
pub const CREASE_DEGREES: f64 = 40.0;

/// One shape the renderer draws for the outlined node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShapeReport {
    /// Shape → node-parent matrix (the shared element order, see [`Camera`]).
    pub m: [f64; 16],
    /// X_ITE geometry type: 0 points, 1 lines, 2 and 3 triangles.
    pub k: u8,
    /// Vertex count (the shape list's fingerprint).
    pub n: u32,
    /// `x y z` per vertex, in the shape's own coordinates; only when asked.
    #[serde(default)]
    pub v: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EdgeKind {
    /// Boundary, crease, non-manifold or a line segment.
    Always,
    /// Shared by triangles `a` and `b`; `same`: both run the edge in the
    /// same direction (inconsistent winding), so one facing is flipped.
    Smooth { a: u32, b: u32, same: bool },
}

#[derive(Debug, Clone, PartialEq)]
struct ShapeMesh {
    matrix: [f64; 16],
    points: Vec<V3>,
    tris: Vec<[u32; 3]>,
    edges: Vec<(u32, u32, EdgeKind)>,
}

/// The welded, classified edges of every reported shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    shapes: Vec<ShapeMesh>,
    /// (geometry type, vertex count) per shape: a report with another
    /// fingerprint needs a new mesh.
    pub fingerprint: Vec<(u8, u32)>,
}

impl Mesh {
    /// Take the newest shape matrices (they move with animation and the
    /// gizmo); `false` when `shapes` is not this mesh's shape list.
    pub fn update_matrices(&mut self, shapes: &[ShapeReport]) -> bool {
        if fingerprint(shapes) != self.fingerprint || !shapes.iter().all(|s| finite(&s.m)) {
            return false;
        }
        for (m, s) in self.shapes.iter_mut().zip(shapes) {
            m.matrix = s.m;
        }
        true
    }

    /// Edges that could be drawn (for tests and diagnostics).
    pub fn edge_count(&self) -> usize {
        self.shapes.iter().map(|s| s.edges.len()).sum()
    }
}

/// (geometry type, vertex count) per shape.
pub fn fingerprint(shapes: &[ShapeReport]) -> Vec<(u8, u32)> {
    shapes.iter().map(|s| (s.k, s.n)).collect()
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit_normal(p: &[V3], t: [u32; 3]) -> Option<V3> {
    let n = cross(
        sub(p[t[1] as usize], p[t[0] as usize]),
        sub(p[t[2] as usize], p[t[0] as usize]),
    );
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (len > 1e-12 && len.is_finite()).then(|| [n[0] / len, n[1] / len, n[2] / len])
}

/// Weld, triangulate and classify every reported shape. Every shape must
/// carry its vertices (`v`), `3 * n` finite numbers.
pub fn build_mesh(shapes: &[ShapeReport]) -> Result<Mesh, &'static str> {
    let total: usize = shapes.iter().map(|s| s.n as usize).sum();
    if total > MAX_VERTICES {
        return Err(refusal::TOO_DETAILED);
    }
    let crease_cos = CREASE_DEGREES.to_radians().cos();
    let mut out = Vec::with_capacity(shapes.len());
    for s in shapes {
        let v = s.v.as_deref().ok_or(refusal::GEOMETRY_INVALID)?;
        if v.len() != 3 * s.n as usize || !finite(v) || !finite(&s.m) {
            return Err(refusal::GEOMETRY_INVALID);
        }
        // Weld by exact position (-0 and 0 are one point).
        let mut ids: HashMap<[u64; 3], u32> = HashMap::new();
        let mut points: Vec<V3> = Vec::new();
        let index: Vec<u32> = v
            .chunks_exact(3)
            .map(|c| {
                let p = [c[0] + 0.0, c[1] + 0.0, c[2] + 0.0];
                *ids.entry(p.map(f64::to_bits)).or_insert_with(|| {
                    points.push(p);
                    points.len() as u32 - 1
                })
            })
            .collect();
        let mut tris = Vec::new();
        let mut edges = Vec::new();
        match s.k {
            0 => {}
            1 => {
                let mut seen = std::collections::HashSet::new();
                for c in index.chunks_exact(2) {
                    let (a, b) = (c[0].min(c[1]), c[0].max(c[1]));
                    if a != b && seen.insert((a, b)) {
                        edges.push((a, b, EdgeKind::Always));
                    }
                }
            }
            2 | 3 => {
                // (low, high) -> [(triangle, runs low -> high)]
                let mut by_edge: HashMap<(u32, u32), Vec<(u32, bool)>> = HashMap::new();
                for c in index.chunks_exact(3) {
                    let t = [c[0], c[1], c[2]];
                    if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                        continue;
                    }
                    let ti = tris.len() as u32;
                    tris.push(t);
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        by_edge
                            .entry((a.min(b), a.max(b)))
                            .or_default()
                            .push((ti, a < b));
                    }
                }
                let mut keys: Vec<_> = by_edge.keys().copied().collect();
                keys.sort_unstable();
                for k in keys {
                    let f = &by_edge[&k];
                    let kind = match f.as_slice() {
                        [(a, da), (b, db)] => {
                            let same = da == db;
                            let crease = match (
                                unit_normal(&points, tris[*a as usize]),
                                unit_normal(&points, tris[*b as usize]),
                            ) {
                                (Some(na), Some(nb)) => {
                                    let d = na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2];
                                    // Consistent winding: the normals agree
                                    // on a flat surface; same direction: one
                                    // of them is flipped.
                                    let d = if same { -d } else { d };
                                    d < crease_cos
                                }
                                _ => false,
                            };
                            if crease {
                                EdgeKind::Always
                            } else {
                                EdgeKind::Smooth { a: *a, b: *b, same }
                            }
                        }
                        _ => EdgeKind::Always,
                    };
                    edges.push((k.0, k.1, kind));
                }
            }
            _ => return Err(refusal::GEOMETRY_INVALID),
        }
        out.push(ShapeMesh {
            matrix: s.m,
            points,
            tris,
            edges,
        });
    }
    let mesh = Mesh {
        shapes: out,
        fingerprint: fingerprint(shapes),
    };
    if mesh.edge_count() == 0 {
        return Err(refusal::NO_EDGES);
    }
    Ok(mesh)
}

/// The drawn edges of `mesh` (node-parent coordinates, carried to the world
/// by `parent`), projected through `cam`. Any vertex behind the camera
/// refuses the whole outline; so does more than [`MAX_SEGMENTS`].
pub fn mesh_edges(
    cam: &Camera,
    parent: &[f64; 16],
    mesh: &Mesh,
) -> Result<Vec<Edge>, &'static str> {
    if !cam.valid() {
        return Err(refusal::CAMERA_INVALID);
    }
    let vp = cam.view_proj();
    let mut out = Vec::new();
    for s in &mesh.shapes {
        let m = gz::mat_mul(&vp, &gz::mat_mul(parent, &s.matrix));
        if !finite(&m) {
            return Err(refusal::NOT_FINITE);
        }
        let mut px = Vec::with_capacity(s.points.len());
        for p in &s.points {
            let c: Vec<f64> = (0..4)
                .map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
                .collect();
            if !finite(&c) || c[3] <= 1e-12 {
                return Err(refusal::BEHIND_CAMERA);
            }
            let xy = cam.ndc_to_client(c[0] / c[3], c[1] / c[3]);
            if !(xy.0.is_finite() && xy.1.is_finite()) {
                return Err(refusal::NOT_FINITE);
            }
            px.push(xy);
        }
        // Screen-space winding of each triangle (sign only).
        let facing: Vec<f64> = s
            .tris
            .iter()
            .map(|t| {
                let (a, b, c) = (px[t[0] as usize], px[t[1] as usize], px[t[2] as usize]);
                ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).signum()
            })
            .collect();
        for (a, b, kind) in &s.edges {
            let draw = match kind {
                EdgeKind::Always => true,
                EdgeKind::Smooth { a: fa, b: fb, same } => {
                    let sa = facing[*fa as usize];
                    let sb = facing[*fb as usize] * if *same { -1.0 } else { 1.0 };
                    sa * sb < 0.0
                }
            };
            if draw {
                if out.len() == MAX_SEGMENTS {
                    return Err(refusal::TOO_DETAILED);
                }
                out.push((px[*a as usize], px[*b as usize]));
            }
        }
    }
    Ok(out)
}

/// What the selection overlay shows for one frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Display {
    /// The geometry's own edges.
    Precise(Vec<Edge>),
    /// The bounding box, because the precise outline was refused for
    /// `reason` (a [`refusal`] code).
    Box { edges: [Edge; 12], reason: String },
}

/// The highlight decision for one frame: the precise outline when `mesh` is
/// a usable mesh of the SAME proven node, else the bounding box saying why,
/// else `Err` (nothing is drawn). `mesh`: `Ok` mesh, or `Err` why the
/// renderer's shapes could not be meshed.
pub fn display(
    cam: &Camera,
    bounds: &Bounds,
    mesh: Result<&Mesh, &str>,
) -> Result<Display, &'static str> {
    if !cam.valid() {
        return Err(refusal::CAMERA_INVALID);
    }
    // The frame chain is shared: an unknown ancestor refuses both outlines.
    let parent = ancestor_matrix(&bounds.ancestors)?;
    let why = match mesh {
        Ok(m) => match mesh_edges(cam, &parent, m) {
            Ok(e) if !e.is_empty() => return Ok(Display::Precise(e)),
            Ok(_) => refusal::NO_EDGES.to_string(),
            Err(e) => e.to_string(),
        },
        Err(e) => e.to_string(),
    };
    let corners = world_corners(bounds)?;
    let edges = outline(cam, &corners)?;
    Ok(Display::Box { edges, reason: why })
}
