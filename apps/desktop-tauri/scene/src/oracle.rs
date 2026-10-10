// SPDX-License-Identifier: GPL-3.0-or-later
//! The CPU pick oracle, and the GPU/CPU agreement rule (owner decision D14).
//!
//! The oracle casts rays through the SAME triangles the GPU draws, in object
//! space, with a NON-normalized ray (so one ray parameter orders hits across
//! every object), and culls back faces exactly as the GPU pipeline does:
//! a triangle faces the viewer when the eye is on its outward side in object
//! space; the renderer flips its front-face winding for a mirrored world
//! matrix (`RenderObject::mirrored`), which makes the two tests the same.
//!
//! Refusal band: a pick is refused when any point within one physical pixel
//! of the sample shows a different object (a VISIBLE object edge), or when
//! the nearest two objects at the sample are within `DEPTH_TIE` of each
//! other in depth. Edges between triangles of ONE object are not boundaries:
//! the band compares object ids, never triangle ids.

use glam::DVec3;

use crate::camera::View;
use crate::RenderObject;

/// Half-width of the refusal band, physical pixels.
pub const EDGE_BAND_PX: f64 = 1.0;
/// Two objects closer than this in clip depth at the sample are a tie.
pub const DEPTH_TIE: f64 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Background,
    Object(u32),
    Refused(&'static str),
}

pub mod reason {
    pub const NEAR_EDGE: &str = "pick-near-object-edge";
    pub const DEPTH_TIE: &str = "pick-depth-tie";
    pub const NO_RAY: &str = "pick-ray-undefined";
    pub const DISAGREE: &str = "gpu-cpu-pick-disagree";
    pub const GPU_FAILED: &str = "gpu-pick-failed";
    pub const OUTSIDE: &str = "pick-outside-viewport";
}

/// The nearest front-facing hit of each object along one ray, nearest first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub id: u32,
    pub t: f64,
}

/// All object hits along the world ray `o + t·d`, `t` in [0, 1] (near plane
/// to far plane), nearest first; at most one entry per object.
pub fn trace(objects: &[RenderObject], o: DVec3, d: DVec3) -> Vec<RayHit> {
    let mut hits = Vec::new();
    for obj in objects {
        let inv = obj.world.inverse();
        let (oo, od) = (inv.transform_point3(o), inv.transform_vector3(d));
        if !oo.is_finite() || !od.is_finite() || !sphere_overlaps(oo, od, obj.mesh.radius() * (1.0 + 1e-6) + 1e-9) {
            continue;
        }
        let mut best: Option<f64> = None;
        for [a, b, c] in obj.mesh.triangles() {
            let (a, b, c) = (DVec3::from(a.map(f64::from)), DVec3::from(b.map(f64::from)), DVec3::from(c.map(f64::from)));
            if let Some(t) = front_hit(oo, od, a, b, c) {
                if (0.0..=1.0).contains(&t) && best.is_none_or(|x| t < x) {
                    best = Some(t);
                }
            }
        }
        if let Some(t) = best {
            hits.push(RayHit { id: obj.pick_id, t });
        }
    }
    hits.sort_by(|x, y| x.t.total_cmp(&y.t).then(x.id.cmp(&y.id)));
    hits
}

/// Does the ray segment t in [0, 1] touch the sphere |p| <= r?
fn sphere_overlaps(o: DVec3, d: DVec3, r: f64) -> bool {
    let a = d.length_squared();
    if a == 0.0 {
        return false;
    }
    let b = o.dot(d);
    let c = o.length_squared() - r * r;
    let disc = b * b - a * c;
    if disc < 0.0 {
        return false;
    }
    let s = disc.sqrt();
    let (t0, t1) = ((-b - s) / a, (-b + s) / a);
    t1 >= 0.0 && t0 <= 1.0
}

/// Möller–Trumbore, front faces only (counter-clockwise seen from the ray
/// origin side). Barycentric bounds are inclusive with a tiny slack so a ray
/// exactly on a shared internal edge hits one of the two triangles.
fn front_hit(o: DVec3, d: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    const SLACK: f64 = 1e-9;
    let (e1, e2) = (b - a, c - a);
    if e1.cross(e2).dot(d) >= 0.0 {
        return None; // back face or edge-on
    }
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-300 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(-SLACK..=1.0 + SLACK).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let w = d.dot(q) * inv;
    if w < -SLACK || u + w > 1.0 + SLACK {
        return None;
    }
    let t = e2.dot(q) * inv;
    t.is_finite().then_some(t)
}

/// The visible object at physical-pixel point (`x`, `y`): `Ok(0)` for the
/// background.
fn visible(objects: &[RenderObject], view: &View, x: f64, y: f64) -> Option<(u32, Vec<RayHit>, DVec3, DVec3)> {
    let (o, d) = view.ray(x, y)?;
    let hits = trace(objects, o, d);
    Some((hits.first().map_or(0, |h| h.id), hits, o, d))
}

/// The oracle's answer for physical pixel (`px`, `py`) of `view`.
pub fn pick(objects: &[RenderObject], view: &View, px: u32, py: u32) -> Answer {
    if px >= view.width || py >= view.height {
        return Answer::Refused(reason::OUTSIDE);
    }
    let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
    let Some((id, hits, o, d)) = visible(objects, view, cx, cy) else {
        return Answer::Refused(reason::NO_RAY);
    };
    // Depth tie: the nearest object and the nearest OTHER object.
    if let (Some(a), Some(b)) = (hits.first(), hits.get(1)) {
        let (da, db) = (view.depth(o + d * a.t), view.depth(o + d * b.t));
        if (da - db).abs() <= DEPTH_TIE {
            return Answer::Refused(reason::DEPTH_TIE);
        }
    }
    let b = EDGE_BAND_PX;
    let ring = [
        (-b, -b), (0.0, -b), (b, -b),
        (-b, 0.0), (b, 0.0),
        (-b, b), (0.0, b), (b, b),
        (-b / 2.0, 0.0), (b / 2.0, 0.0), (0.0, -b / 2.0), (0.0, b / 2.0),
    ];
    for (dx, dy) in ring {
        match visible(objects, view, cx + dx, cy + dy) {
            Some((other, ..)) if other == id => {}
            Some(_) => return Answer::Refused(reason::NEAR_EDGE),
            None => return Answer::Refused(reason::NO_RAY),
        }
    }
    if id == 0 { Answer::Background } else { Answer::Object(id) }
}

/// The final pick verdict for one click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Object(u32),
    Background,
    Refused(&'static str),
}

/// D14: the oracle refuses first; a GPU failure or ANY GPU/CPU disagreement
/// is a refusal. Never a vote, never a nearest match.
pub fn agree(oracle: Answer, gpu: Result<u32, ()>) -> Verdict {
    match (oracle, gpu) {
        (Answer::Refused(r), _) => Verdict::Refused(r),
        (_, Err(())) => Verdict::Refused(reason::GPU_FAILED),
        (Answer::Background, Ok(0)) => Verdict::Background,
        (Answer::Object(a), Ok(g)) if a == g => Verdict::Object(a),
        _ => Verdict::Refused(reason::DISAGREE),
    }
}
