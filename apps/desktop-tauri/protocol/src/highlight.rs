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
//!
//! LV2 precise outline: where the renderer can report the node's rendered
//! triangles and lines ([`ShapeReport`], X_ITE's own shape list of the SAME
//! proven node), [`build_mesh`] turns them into an edge mesh once and
//! [`display`] draws, every frame, its boundary, crease and silhouette
//! edges -- the outline follows the geometry. The bounding box is only the
//! fallback, and says why ([`Display::Box`]). Non-geometry nodes
//! (Appearance, Material, a texture, Coordinate, ...) are not rendered
//! objects of their own ([`node_role`]); the outline shows the Shape that
//! owns them, labeled as the owner, and never claims bounds for them.

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
    /// LV2: the node is never drawn (a Viewpoint, a sensor, a light, ...).
    pub const NOT_RENDERED: &str = "node-is-not-drawn";
    /// LV2: a non-geometry node (Material, Coordinate, ...) whose owning
    /// Shape is not in its proven chain.
    pub const NO_OWNER: &str = "property-node-has-no-owning-shape";
    // ---- LV2 precise-outline fallbacks (the bounding box is drawn) --------
    /// The renderer cannot report the node's shapes.
    pub const GEOMETRY_UNAVAILABLE: &str = "geometry-unavailable";
    /// More triangles / lines than the outline draws per frame.
    pub const TOO_DETAILED: &str = "geometry-too-detailed";
    /// Only points (or nothing) to draw edges of.
    pub const NO_EDGES: &str = "geometry-has-no-edges";
    /// The shape report is malformed (counts, non-finite values).
    pub const GEOMETRY_INVALID: &str = "geometry-report-invalid";
}

/// How a proven ancestor node changes the coordinate frame of its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AncestorKind {
    /// `Transform`: its rendered fields give the frame.
    Transform,
    /// A grouping node that does not change the frame.
    Identity,
}

/// What a selected node is, for the selection display (LV2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// Geometry, a Shape, a grouping node or a PROTO instance: it may have
    /// rendered geometry of its own; the renderer is asked.
    Rendered,
    /// A non-geometry node a Shape draws WITH (Appearance, Material, a
    /// texture, Coordinate, ...): no bounds of its own; its owner is shown.
    Property,
    /// Never drawn (a Viewpoint, a sensor, a light, a Script, ...).
    NotRendered,
}

/// The [`NodeRole`] of a node of type `node_type`. Unknown names (PROTO
/// instances, X3D nodes) are `Rendered`: the renderer decides.
pub fn node_role(node_type: &str) -> NodeRole {
    match node_type {
        "Appearance" | "Material" | "ImageTexture" | "PixelTexture" | "MovieTexture"
        | "TextureTransform" | "Coordinate" | "Normal" | "Color" | "TextureCoordinate"
        | "FontStyle" => NodeRole::Property,
        "Viewpoint" | "NavigationInfo" | "Background" | "Fog" | "WorldInfo"
        | "DirectionalLight" | "PointLight" | "SpotLight" | "Sound" | "AudioClip"
        | "Script" | "TimeSensor" | "TouchSensor" | "PlaneSensor" | "CylinderSensor"
        | "SphereSensor" | "ProximitySensor" | "VisibilitySensor" => NodeRole::NotRendered,
        t if t.ends_with("Interpolator") => NodeRole::NotRendered,
        _ => NodeRole::Rendered,
    }
}

/// One sentence (no leading capital, no full stop) saying truthfully why no
/// outline -- or only the bounding box -- is shown for refusal `reason`.
/// Every reason code the highlight path can produce has its own entry;
/// an unknown code says only that the preview cannot prove the object.
pub fn refusal_message(reason: &str) -> &'static str {
    use crate::highlight::refusal as r;
    match reason {
        r::NOT_RENDERED => "this node is not drawn, so it has no shape to outline",
        r::NO_OWNER => "this node is not drawn by itself and no Shape that uses it was proven",
        r::ANCESTOR => {
            "it is inside a node whose placement the outline cannot follow (a Billboard or a node the preview does not model)"
        }
        r::ANCESTOR_DISAGREES | "runtime-ancestor-not-proven" | "proof-for-another-node" => {
            "the preview's parent nodes no longer match the source"
        }
        r::NO_BOUNDS | "bounds-unavailable" | "node-has-no-rendered-geometry" => {
            "it has no rendered geometry"
        }
        r::BEHIND_CAMERA => "part of it is behind the camera",
        r::CAMERA_INVALID | "camera-unavailable" => "the viewport camera cannot be read",
        r::NOT_FINITE => "its rendered size or placement is not a finite number",
        r::GEOMETRY_UNAVAILABLE => "the preview cannot report its triangles",
        r::TOO_DETAILED => "it has too many triangles to outline edge by edge",
        r::NO_EDGES => "it has no edges to draw (points only)",
        r::GEOMETRY_INVALID => "the preview reported its triangles inconsistently",
        "proto-instance-not-picked" => {
            "its PROTO has several instances on screen; click the instance in the viewport to outline it"
        }
        "proto-instance-not-rendered" => "no instance of its PROTO is drawn",
        "proto-instance" | "proto-provenance-unavailable" => {
            "it is part of a PROTO definition the preview cannot trace"
        }
        "runtime-node-for-span-not-unique" => {
            "the preview has no single drawn object for this node (it is not drawn, or drawn more than once)"
        }
        "runtime-node-referenced-by-several-source-occurrences"
        | "runtime-node-has-several-live-parents" => "it is shared (DEF/USE) by several places",
        "document-has-syntax-errors" | "document-parse-capped" => {
            "the document must parse cleanly first"
        }
        "not-a-node" => "only a node instance can be outlined",
        "preview-scene-replaced" | "preview-shows-last-valid-scene" | "source-changed-since-preview" => {
            "the preview is not showing the current text yet"
        }
        "adapter-unavailable" | "world-infrastructure-missing" => "the 3D preview is not available",
        _ => "the preview cannot prove which rendered object this is",
    }
}

/// LV2: the Inspector badge of a non-geometry node: what owns it, e.g.
/// `Material of Shape at line 12` (or `of Shape Wheel` when the line is not
/// known). `owner_label` is the owner's Scene Tree label (`Shape [DEF]`).
pub fn owner_badge(node_type: &str, owner_label: &str, owner_line: Option<u32>) -> String {
    match owner_line {
        Some(l) => format!("{node_type} of {owner_label} at line {l}"),
        None => format!("{node_type} of {owner_label}"),
    }
}

/// LV2: the status-bar line for the selection: type, DEF, line, the owner
/// of a non-geometry node and the PROTO context, e.g.
/// `Selected: Material · line 40 · of Shape at line 37 · inside PROTO Car (shared, 2 instances)`.
pub fn selection_summary(
    node_type: &str,
    def: Option<&str>,
    line: Option<u32>,
    owner: Option<(&str, Option<u32>)>,
    context: Option<&str>,
) -> String {
    let mut parts = vec![match def {
        Some(d) => format!("Selected: {node_type} {d}"),
        None => format!("Selected: {node_type}"),
    }];
    if let Some(l) = line {
        parts.push(format!("line {l}"));
    }
    if let Some((label, l)) = owner {
        parts.push(match l {
            Some(l) => format!("of {label} at line {l}"),
            None => format!("of {label}"),
        });
    }
    if let Some(c) = context {
        parts.push(c.to_string());
    }
    parts.join(" · ")
}

/// LV2: `inside PROTO X (shared, N instance[s])`.
pub fn proto_context(proto: &str, instances: u32) -> String {
    format!(
        "inside PROTO {proto} (shared, {instances} instance{})",
        if instances == 1 { "" } else { "s" }
    )
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
    /// B3: this ancestor is the PROTO instance whose BODY the inner chain
    /// was proven through (set from Rust's proof, never by the renderer).
    /// An instance renders as its body root node, which is the next inner
    /// ancestor already: it adds no frame of its own.
    #[serde(default)]
    pub proto_instance: bool,
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

/// The node-parent → world matrix of the proven `ancestors` (innermost
/// first). Refuses an unknown ancestor or a Transform without a usable frame.
pub fn ancestor_matrix(ancestors: &[RenderedAncestor]) -> Result<[f64; 16], &'static str> {
    let mut m = identity();
    for a in ancestors {
        if a.proto_instance {
            continue;
        }
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
    Ok(m)
}

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
    let m = gz::mat_mul(&ancestor_matrix(&bounds.ancestors)?, b);
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
pub fn svg_path(edges: &[Edge], ox: f64, oy: f64) -> String {
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

pub use mesh::{build_mesh, display, fingerprint, mesh_edges, Display, Mesh, ShapeReport};

mod mesh;

#[cfg(test)]
mod tests;
