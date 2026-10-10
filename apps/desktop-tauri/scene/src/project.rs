// SPDX-License-Identifier: GPL-3.0-or-later
//! One parse -> one `RenderScene`. Read only.
//!
//! The walk follows only what NATIVE-RENDER-1 draws: top-level statements and
//! `Transform.children`. A node of any other type is listed in `not_shown`
//! and its subtree is not entered. A node whose authored values cannot be
//! read exactly (duplicate field, `IS`, wrong shape, out of range) is not
//! drawn and is listed with the reason: the projection never guesses.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{DMat4, DQuat, DVec3};
use wrlforge_vrml::ast::{Ast, Node};
use wrlforge_vrml::diagnostics::Severity;
use wrlforge_vrml::ParseResult;

use crate::mesh::{self, Mesh};
use crate::values::{self as v, Child};
use crate::{Geometry, InstancePath, Material, NodeOccurrence, NotShown, RenderObject, RenderScene, Shading};

pub mod reason {
    pub const NOT_IN_SCOPE: &str = "node-type-not-in-native-scope";
    pub const USE_NOT_DRAWN: &str = "use-not-drawn";
    pub const GEOMETRY_USE: &str = "geometry-is-use";
    pub const APPEARANCE_USE: &str = "appearance-or-material-is-use";
    pub const TEXTURE: &str = "texture-not-in-native-scope";
    pub const ROTATION_AXIS_ZERO: &str = "rotation-axis-zero";
    pub const SCALE_ZERO: &str = "scale-zero";
    pub const SINGULAR: &str = "transform-singular";
    pub const CHILD_SHAPE: &str = "value-shape";
}

/// Pointing-device sensors (ISO 4.6.7.3): they activate on sibling geometry.
pub const SENSORS: [&str; 4] = ["TouchSensor", "PlaneSensor", "CylinderSensor", "SphereSensor"];

/// Project the parse of the PREVIEW text (the canonical text minus a leading
/// BOM, exactly what X_ITE would be given).
pub fn project(parse: &ParseResult, session: u64, revision: u64, text_hash: u64, generation: u64) -> RenderScene {
    let damaged = parse.truncated
        || parse.depth_capped
        || parse.diagnostics.iter().any(|d| d.severity == Severity::Error);
    let mut uses: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
    for s in &parse.tree.statements {
        collect_uses(s, &mut uses);
    }
    let mut p = Projector { uses, objects: Vec::new(), not_shown: Vec::new(), meshes: Vec::new() };
    let top: Vec<Child> = parse
        .tree
        .statements
        .iter()
        .filter_map(|s| match s {
            Ast::Node(n) => Some(Child::Node(n)),
            Ast::Use { range, .. } => Some(Child::Use {
                span: (range.start.offset as u64, range.end.offset as u64),
            }),
            _ => None,
        })
        .collect();
    p.level(&top, DMat4::IDENTITY, &[], &[]);
    RenderScene { session, revision, text_hash, generation, objects: p.objects, not_shown: p.not_shown, damaged }
}

/// Every `USE <name>` anywhere in the file, PROTO bodies included: a name
/// match is enough to refuse (never a scope guess in the permissive
/// direction).
fn collect_uses(a: &Ast, out: &mut HashMap<String, Vec<(u64, u64)>>) {
    match a {
        Ast::Use { name: Some(n), range, .. } => {
            out.entry(n.clone()).or_default().push((range.start.offset as u64, range.end.offset as u64))
        }
        Ast::Node(n) => {
            n.fields.iter().for_each(|f| collect_uses(f, out));
            n.interfaces.iter().filter_map(|i| i.default.as_ref()).for_each(|d| collect_uses(d, out));
        }
        Ast::Field(f) => {
            if let Some(v) = &f.value {
                collect_uses(v, out)
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| collect_uses(i, out)),
        Ast::Proto(p) => {
            p.interfaces.iter().filter_map(|i| i.default.as_ref()).for_each(|d| collect_uses(d, out));
            p.body.iter().for_each(|b| collect_uses(b, out));
        }
        _ => {}
    }
}

struct Projector {
    uses: HashMap<String, Vec<(u64, u64)>>,
    objects: Vec<RenderObject>,
    not_shown: Vec<NotShown>,
    meshes: Vec<(Geometry, Arc<Mesh>)>,
}

impl Projector {
    fn skip(&mut self, node_type: &str, span: (u64, u64), reason: &'static str) {
        self.not_shown.push(NotShown { node_type: node_type.into(), from: span.0, to: span.1, reason });
    }

    fn occurrence(&self, n: &Node) -> NodeOccurrence {
        let (from, to) = v::span(n);
        NodeOccurrence {
            from,
            to,
            node_type: n.node_type.clone(),
            use_sites: n.def.as_ref().and_then(|d| self.uses.get(d)).cloned().unwrap_or_default(),
        }
    }

    fn mesh(&mut self, g: Geometry) -> Arc<Mesh> {
        if let Some((_, m)) = self.meshes.iter().find(|(k, _)| *k == g) {
            return m.clone();
        }
        let m = Arc::new(mesh::tessellate(&g));
        self.meshes.push((g, m.clone()));
        m
    }

    /// One grouping level: the children of a Transform, or the scene root.
    fn level(&mut self, items: &[Child], world: DMat4, path: &[NodeOccurrence], above: &[String]) {
        let mut sensors = above.to_vec();
        for c in items {
            if let Child::Node(n) = c {
                if SENSORS.contains(&n.node_type.as_str()) {
                    sensors.push(n.node_type.clone());
                }
            }
        }
        for c in items {
            match c {
                Child::Use { span, .. } => self.skip("USE", *span, reason::USE_NOT_DRAWN),
                Child::Node(n) => match n.node_type.as_str() {
                    "Transform" => self.transform(n, world, path, &sensors),
                    "Shape" => self.shape(n, world, path, &sensors),
                    t if SENSORS.contains(&t) => {}
                    t => self.skip(t, v::span(n), reason::NOT_IN_SCOPE),
                },
            }
        }
    }

    fn transform(&mut self, n: &Node, world: DMat4, path: &[NodeOccurrence], sensors: &[String]) {
        let read = || -> v::R<(DMat4, Vec<Child>)> { Ok((local_matrix(n)?, v::children(n, "children")?)) };
        match read() {
            Err(r) => self.skip(&n.node_type, v::span(n), r),
            Ok((m, kids)) => {
                let mut p = path.to_vec();
                p.push(self.occurrence(n));
                self.level(&kids, world * m, &p, sensors);
            }
        }
    }

    fn shape(&mut self, n: &Node, world: DMat4, path: &[NodeOccurrence], sensors: &[String]) {
        let span = v::span(n);
        let geometry = match v::field(n, "geometry") {
            Err(r) => return self.skip("Shape", span, r),
            Ok(None | Some(Ast::Null { .. })) => return,
            Ok(Some(Ast::Use { .. })) => return self.skip("Shape", span, reason::GEOMETRY_USE),
            Ok(Some(Ast::Node(g))) => match geometry(g) {
                Ok(Some(g)) => g,
                Ok(None) => return self.skip(&g.node_type, v::span(g), reason::NOT_IN_SCOPE),
                Err(r) => return self.skip(&g.node_type, v::span(g), r),
            },
            Ok(Some(_)) => return self.skip("Shape", span, reason::CHILD_SHAPE),
        };
        let shading = match shading(n) {
            Ok(s) => s,
            Err(r) => return self.skip("Shape", span, r),
        };
        let det = world.determinant();
        if !det.is_finite() || det.abs() < 1e-300 || !world.is_finite() {
            return self.skip("Shape", span, reason::SINGULAR);
        }
        let mut p = path.to_vec();
        p.push(self.occurrence(n));
        let mesh = self.mesh(geometry);
        let pick_id = self.objects.len() as u32 + 1;
        self.objects.push(RenderObject {
            pick_id,
            world,
            mirrored: det < 0.0,
            geometry,
            mesh,
            shading,
            path: InstancePath(p.into()),
            sensors: sensors.to_vec(),
        });
    }
}

/// ISO 6.52: P' = T · C · R · SR · S · -SR · -C · P.
fn local_matrix(n: &Node) -> v::R<DMat4> {
    let t = DVec3::from(v::vec3(n, "translation", [0.0; 3])?);
    let c = DVec3::from(v::vec3(n, "center", [0.0; 3])?);
    let r = rotation(v::rotation(n, "rotation")?)?;
    let s = DVec3::from(v::vec3(n, "scale", [1.0; 3])?);
    let sr = rotation(v::rotation(n, "scaleOrientation")?)?;
    if s.x == 0.0 || s.y == 0.0 || s.z == 0.0 {
        return Err(reason::SCALE_ZERO);
    }
    Ok(DMat4::from_translation(t)
        * DMat4::from_translation(c)
        * DMat4::from_quat(r)
        * DMat4::from_quat(sr)
        * DMat4::from_scale(s)
        * DMat4::from_quat(sr.inverse())
        * DMat4::from_translation(-c))
}

/// SFRotation -> quaternion. A non-unit axis is normalized (ISO 5.8 asks for
/// a normalized axis; every browser normalizes). A zero axis with a non-zero
/// angle has no meaning and is refused.
fn rotation(r: [f64; 4]) -> v::R<DQuat> {
    let axis = DVec3::new(r[0], r[1], r[2]);
    let len = axis.length();
    if len < 1e-12 {
        return if r[3] == 0.0 { Ok(DQuat::IDENTITY) } else { Err(reason::ROTATION_AXIS_ZERO) };
    }
    Ok(DQuat::from_axis_angle(axis / len, r[3]))
}

/// `Ok(None)`: a geometry type outside the native scope.
fn geometry(g: &Node) -> v::R<Option<Geometry>> {
    Ok(Some(match g.node_type.as_str() {
        "Box" => {
            let size = v::vec3(g, "size", [2.0; 3])?;
            if size.iter().any(|s| *s <= 0.0) {
                return Err("value-out-of-range");
            }
            Geometry::Box { size }
        }
        "Sphere" => Geometry::Sphere { radius: v::positive(g, "radius", 1.0)? },
        "Cylinder" => Geometry::Cylinder {
            radius: v::positive(g, "radius", 1.0)?,
            height: v::positive(g, "height", 2.0)?,
            bottom: v::boolean(g, "bottom", true)?,
            side: v::boolean(g, "side", true)?,
            top: v::boolean(g, "top", true)?,
        },
        "Cone" => Geometry::Cone {
            bottom_radius: v::positive(g, "bottomRadius", 1.0)?,
            height: v::positive(g, "height", 2.0)?,
            bottom: v::boolean(g, "bottom", true)?,
            side: v::boolean(g, "side", true)?,
        },
        _ => return Ok(None),
    }))
}

const WHITE: Shading = Shading::Unlit([1.0, 1.0, 1.0]);

/// ISO 6.43 / 4.14.4: a NULL appearance or material means unlit white.
fn shading(shape: &Node) -> v::R<Shading> {
    let app = match v::field(shape, "appearance")? {
        None | Some(Ast::Null { .. }) => return Ok(WHITE),
        Some(Ast::Use { .. }) => return Err(reason::APPEARANCE_USE),
        Some(Ast::Node(a)) if a.node_type == "Appearance" => a,
        Some(Ast::Node(_)) => return Err(reason::NOT_IN_SCOPE),
        Some(_) => return Err(reason::CHILD_SHAPE),
    };
    for f in ["texture", "textureTransform"] {
        if !matches!(v::field(app, f)?, None | Some(Ast::Null { .. })) {
            return Err(reason::TEXTURE);
        }
    }
    let m = match v::field(app, "material")? {
        None | Some(Ast::Null { .. }) => return Ok(WHITE),
        Some(Ast::Use { .. }) => return Err(reason::APPEARANCE_USE),
        Some(Ast::Node(m)) if m.node_type == "Material" => m,
        Some(Ast::Node(_)) => return Err(reason::NOT_IN_SCOPE),
        Some(_) => return Err(reason::CHILD_SHAPE),
    };
    let d = Material::default();
    Ok(Shading::Lit(Material {
        ambient_intensity: v::unit(m, "ambientIntensity", d.ambient_intensity)?,
        diffuse: v::color(m, "diffuseColor", d.diffuse)?,
        emissive: v::color(m, "emissiveColor", d.emissive)?,
        shininess: v::unit(m, "shininess", d.shininess)?,
        specular: v::color(m, "specularColor", d.specular)?,
        transparency: v::unit(m, "transparency", d.transparency)?,
    }))
}
