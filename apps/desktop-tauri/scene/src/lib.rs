// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1: the render projection of ONE document revision.
//!
//! The exact source text is the document (`WD.md` §2). This crate is a
//! derived, disposable READER of one parse of one text: it has no edit API,
//! no I/O, no clock and no GPU. `wrlforge-document` stays the only source
//! authority; the renderer (`wrlforge-render`) only draws what this crate
//! returns, and the desktop service turns a pick into a selection through the
//! existing `wrlforge_vrml::pick::resolve` proof.
//!
//! Scope (owner decision, NATIVE-RENDER-1): `Transform`, `Shape`,
//! `Appearance`, `Material`, `Box`, `Sphere`, `Cylinder`, `Cone`, with
//! ISO/IEC 14772-1 field defaults and transform order. Everything else is
//! listed in `RenderScene::not_shown` and never drawn. In particular `USE`
//! is NOT drawn: drawing it needs a proven DEF/USE scope resolver, which the
//! Rust core does not have yet (WD1.5 exists in JS only).
//!
//! The SAME tessellated triangles feed the GPU and the CPU oracle
//! (`oracle`), so the two checks look at identical geometry.

#![forbid(unsafe_code)]

pub mod camera;
pub mod identity;
pub mod mesh;
pub mod oracle;
pub mod project;
mod values;

use std::sync::Arc;

pub use project::project;

use glam::DMat4;

/// Where a projected node came from: its exact UTF-16 span in the text that
/// was projected (the preview text: the canonical text minus a leading BOM),
/// and its type. NATIVE-RENDER-1 never descends into a PROTO body, so every
/// occurrence is in the document's own scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeOccurrence {
    pub from: u64,
    pub to: u64,
    pub node_type: String,
    /// Spans of `USE` statements that name this node's DEF name anywhere in
    /// the file. Non-empty means "may have several live parents": the pick is
    /// refused as ambiguous (by name, conservatively: never a guess).
    pub use_sites: Vec<(u64, u64)>,
}

/// One instantiation path, root first, the Shape last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstancePath(pub Arc<[NodeOccurrence]>);

impl InstancePath {
    pub fn shape(&self) -> Option<&NodeOccurrence> {
        self.0.last()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geometry {
    Box { size: [f64; 3] },
    Sphere { radius: f64 },
    Cylinder { radius: f64, height: f64, bottom: bool, side: bool, top: bool },
    Cone { bottom_radius: f64, height: f64, bottom: bool, side: bool },
}

/// ISO 14772-1 6.27 Material (the fields NATIVE-RENDER-1 uses).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub ambient_intensity: f32,
    pub diffuse: [f32; 3],
    pub emissive: [f32; 3],
    pub shininess: f32,
    pub specular: [f32; 3],
    pub transparency: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            ambient_intensity: 0.2,
            diffuse: [0.8, 0.8, 0.8],
            emissive: [0.0; 3],
            shininess: 0.2,
            specular: [0.0; 3],
            transparency: 0.0,
        }
    }
}

/// ISO 4.14.4: no Appearance or no Material means lighting is off and the
/// object is white (no texture in NATIVE-RENDER-1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shading {
    Unlit([f32; 3]),
    Lit(Material),
}

impl Shading {
    pub fn alpha(&self) -> f32 {
        match self {
            Shading::Unlit(_) => 1.0,
            Shading::Lit(m) => 1.0 - m.transparency,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderObject {
    /// 1-based, valid only within its generation. 0 is the background.
    pub pick_id: u32,
    /// Object -> world, composed in the ISO 4.4.3 Transform order.
    pub world: DMat4,
    /// `det(world) < 0`: the front face winding is mirrored.
    pub mirrored: bool,
    pub geometry: Geometry,
    pub mesh: Arc<mesh::Mesh>,
    pub shading: Shading,
    pub path: InstancePath,
    /// Pointing-device sensor types that would be active at this geometry
    /// (siblings at any level of its path). Non-empty refuses the pick.
    pub sensors: Vec<String>,
}

/// A node the projection did not draw, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotShown {
    pub node_type: String,
    pub from: u64,
    pub to: u64,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct RenderScene {
    pub session: u64,
    pub revision: u64,
    /// `preview_hash` of the projected text.
    pub text_hash: u64,
    /// Assigned by the owner of the session; one per projection.
    pub generation: u64,
    pub objects: Vec<RenderObject>,
    pub not_shown: Vec<NotShown>,
    /// Syntax errors or a capped parse: nothing here may be selected.
    pub damaged: bool,
}

impl RenderScene {
    pub fn object(&self, pick_id: u32) -> Option<&RenderObject> {
        let i = pick_id.checked_sub(1)? as usize;
        self.objects.get(i).filter(|o| o.pick_id == pick_id)
    }
}

/// Reserved for future manipulation handles (never an object id).
pub const HANDLE_ID_BASE: u32 = 0xFFFF_0000;

#[cfg(test)]
mod tests;
