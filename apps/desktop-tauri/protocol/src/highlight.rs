// SPDX-License-Identifier: GPL-3.0-or-later
//! B2 selection highlight math: pure, std-only. No renderer, DOM or document
//! access.
//!
//! The highlight is VIEW state. It outlines the rendered bounds of a node
//! whose runtime object Rust PROVED (`doc_highlight_prove`, the same
//! provenance proof the gizmo binds through), and nothing else:
//!
//! * the renderer reports the node's bounding box as X_ITE's `Box3` frame
//!   (a matrix mapping the cube `[-1, 1]^3` onto the box, in the node's
//!   PARENT coordinates) and, for every proven ancestor, its type and -- for
//!   a `Transform` -- its rendered fields (public SAI values);
//! * [`world_corners`] composes those into the box's eight world corners
//!   (the ISO 14772-1 6.52 Transform rule, innermost parent first);
//! * [`outline`] projects them through the renderer's REAL camera into the
//!   twelve box edges in client CSS px. A corner behind the camera (or any
//!   non-finite value) refuses the whole outline: nothing is clipped,
//!   guessed or partially drawn.
//!
//! Only ancestors whose effect on the frame is known are accepted
//! ([`ancestor_kind`]); a Billboard, a PROTO instance or anything else
//! refuses rather than drawing a box in the wrong place.

use serde::{Deserialize, Serialize};

use crate::gizmo::{self as gz, Camera, V3};

/// Why no outline is drawn.
pub mod refusal {
    pub const NO_BOUNDS: &str = "node-has-no-rendered-bounds";
    pub const ANCESTOR: &str = "ancestor-transform-unsupported";
    pub const ANCESTOR_DISAGREES: &str = "runtime-ancestor-disagrees-with-source";
    pub const BEHIND_CAMERA: &str = super::gz::refusal::BEHIND_CAMERA;
    pub const CAMERA_INVALID: &str = super::gz::refusal::CAMERA_INVALID;
    pub const NOT_FINITE: &str = super::gz::refusal::NOT_FINITE;
}

/// How a proven ancestor node changes the coordinate frame of its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AncestorKind {
    /// `Transform`: its rendered fields give the frame.
    Transform,
    /// A grouping node that does not change the frame.
    Identity,
}

/// The frame effect of an ancestor of node type `node_type`, or `None` when
/// it is not known (Billboard, a PROTO instance, an unknown node).
pub fn ancestor_kind(node_type: &str) -> Option<AncestorKind> {
    match node_type {
        "Transform" => Some(AncestorKind::Transform),
        "Group" | "Anchor" | "Collision" | "Switch" | "LOD" | "Shape" => {
            Some(AncestorKind::Identity)
        }
        _ => None,
    }
}

/// A Transform's rendered fields, as the renderer reports them.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub translation: V3,
    pub rotation: [f64; 4],
    pub scale: V3,
    pub scale_orientation: [f64; 4],
    pub center: V3,
}

/// One proven ancestor as the renderer reports it: its runtime node type
/// and, for a Transform, its frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderedAncestor {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub frame: Option<Frame>,
}

/// The renderer's report for one highlighted node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    /// X_ITE `Box3` matrix (shared element order, see [`Camera`]), in the
    /// node's parent coordinates.
    pub r#box: [f64; 16],
    /// Innermost (the direct parent) first.
    pub ancestors: Vec<RenderedAncestor>,
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn identity() -> [f64; 16] {
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

fn translate(t: V3) -> [f64; 16] {
    let mut m = identity();
    m[12] = t[0];
    m[13] = t[1];
    m[14] = t[2];
    m
}

fn rotate(r: [f64; 4]) -> Option<[f64; 16]> {
    let m3 = gz::rot_matrix(r)?;
    let mut m = identity();
    for (row, cols) in m3.iter().enumerate() {
        for (col, v) in cols.iter().enumerate() {
            m[col * 4 + row] = *v;
        }
    }
    Some(m)
}

fn scale(s: V3) -> [f64; 16] {
    let mut m = identity();
    m[0] = s[0];
    m[5] = s[1];
    m[10] = s[2];
    m
}

/// The Transform matrix `T · C · R · SR · S · -SR · -C` (ISO 14772-1 6.52);
/// `None` for a non-finite value or an unusable rotation axis.
pub fn transform_matrix(f: &Frame) -> Option<[f64; 16]> {
    let all = [
        &f.translation[..],
        &f.rotation[..],
        &f.scale[..],
        &f.scale_orientation[..],
        &f.center[..],
    ];
    if !all.iter().all(|v| finite(v)) {
        return None;
    }
    let so = f.scale_orientation;
    let parts = [
        translate(f.translation),
        translate(f.center),
        rotate(f.rotation)?,
        rotate(so)?,
        scale(f.scale),
        rotate([so[0], so[1], so[2], -so[3]])?,
        translate([-f.center[0], -f.center[1], -f.center[2]]),
    ];
    let m = parts
        .iter()
        .skip(1)
        .fold(parts[0], |acc, p| gz::mat_mul(&acc, p));
    finite(&m).then_some(m)
}

fn apply(m: &[f64; 16], v: [f64; 4]) -> [f64; 4] {
    let mut o = [0.0; 4];
    for (r, out) in o.iter_mut().enumerate() {
        *out = (0..4).map(|c| m[c * 4 + r] * v[c]).sum();
    }
    o
}

/// Corner `i` of the cube `[-1, 1]^3`: bit 0 is x, bit 1 y, bit 2 z.
fn cube(i: usize) -> [f64; 4] {
    let s = |b: usize| if i & (1 << b) != 0 { 1.0 } else { -1.0 };
    [s(0), s(1), s(2), 1.0]
}

/// The twelve box edges as corner index pairs (corners differ in one bit).
pub const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// The eight world corners of `bounds`. Refuses an empty box (X_ITE's
/// zero matrix), an unknown ancestor, a Transform ancestor without a
/// usable frame, or any non-finite value.
pub fn world_corners(bounds: &Bounds) -> Result<[V3; 8], &'static str> {
    let b = &bounds.r#box;
    if !finite(b) {
        return Err(refusal::NOT_FINITE);
    }
    if [0, 1, 2, 4, 5, 6, 8, 9, 10].iter().all(|&i| b[i] == 0.0) {
        return Err(refusal::NO_BOUNDS);
    }
    let mut m = *b;
    for a in &bounds.ancestors {
        match (ancestor_kind(&a.node_type), &a.frame) {
            (Some(AncestorKind::Identity), _) => {}
            (Some(AncestorKind::Transform), Some(f)) => {
                let t = transform_matrix(f).ok_or(refusal::NOT_FINITE)?;
                m = gz::mat_mul(&t, &m);
            }
            (Some(AncestorKind::Transform), None) => return Err(refusal::ANCESTOR_DISAGREES),
            (None, _) => return Err(refusal::ANCESTOR),
        }
    }
    let mut out = [[0.0; 3]; 8];
    for (i, c) in out.iter_mut().enumerate() {
        let p = apply(&m, cube(i));
        if !finite(&p) || (p[3] - 1.0).abs() > 1e-9 {
            return Err(refusal::NOT_FINITE);
        }
        *c = [p[0], p[1], p[2]];
    }
    Ok(out)
}

/// One projected box edge in client CSS px.
pub type Edge = ((f64, f64), (f64, f64));

/// The twelve edges of the box with world `corners`, projected through
/// `cam`. Any corner behind the camera refuses the whole outline.
pub fn outline(cam: &Camera, corners: &[V3; 8]) -> Result<[Edge; 12], &'static str> {
    if !cam.valid() {
        return Err(refusal::CAMERA_INVALID);
    }
    let mut px = [(0.0, 0.0); 8];
    for (o, c) in px.iter_mut().zip(corners) {
        if !finite(c) {
            return Err(refusal::NOT_FINITE);
        }
        *o = gz::project(cam, *c).ok_or(refusal::BEHIND_CAMERA)?;
    }
    Ok(EDGES.map(|(a, b)| (px[a], px[b])))
}

/// The SVG path data (`M x y L x y` per edge) of `edges`, relative to the
/// overlay origin (`ox`, `oy`) in client px.
pub fn svg_path(edges: &[Edge; 12], ox: f64, oy: f64) -> String {
    edges
        .iter()
        .map(|((ax, ay), (bx, by))| {
            format!(
                "M{:.1} {:.1}L{:.1} {:.1}",
                ax - ox,
                ay - oy,
                bx - ox,
                by - oy
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests;
