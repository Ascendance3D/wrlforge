// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure tests. The `reference` module is the independent check of the oracle:
//! analytic ray/box and ray/sphere math and a hand-written Transform order,
//! with no call into `oracle`, `mesh`, `project` or glam's quaternions.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::camera::{Orbit, View};
use crate::oracle::{self, Answer, Verdict};
use glam::{DVec3, DVec4};
use wrlforge_vrml::pick::{self, Resolution, Status};

const H: &str = "#VRML V2.0 utf8\n";

fn scene(body: &str) -> RenderScene {
    let text = format!("{H}{body}");
    project(&wrlforge_vrml::parse(&text), 1, 7, 0, 3)
}

fn only(s: &RenderScene) -> &RenderObject {
    assert_eq!(s.objects.len(), 1, "objects: {:?} not shown: {:?}", s.objects.len(), s.not_shown);
    &s.objects[0]
}

mod reference {
    //! Independent reference math. Do not import the code under test here.
    pub type V = [f64; 3];
    pub fn add(a: V, b: V) -> V { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
    pub fn sub(a: V, b: V) -> V { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
    pub fn mul(a: V, s: f64) -> V { [a[0] * s, a[1] * s, a[2] * s] }
    pub fn dot(a: V, b: V) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
    pub fn cross(a: V, b: V) -> V { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }

    /// Rodrigues' rotation of `p` about unit-normalized `axis` by `angle`.
    pub fn rotate(p: V, axis: V, angle: f64) -> V {
        let l = dot(axis, axis).sqrt();
        let k = mul(axis, 1.0 / l);
        let (s, c) = angle.sin_cos();
        add(add(mul(p, c), mul(cross(k, p), s)), mul(k, dot(k, p) * (1.0 - c)))
    }

    /// ISO 6.52 applied to a point, one step at a time, innermost first.
    pub fn transform(p: V, t: V, c: V, r: [f64; 4], s: V, sr: [f64; 4]) -> V {
        let p = sub(p, c);
        let p = rotate(p, [sr[0], sr[1], sr[2]], -sr[3]);
        let p = [p[0] * s[0], p[1] * s[1], p[2] * s[2]];
        let p = rotate(p, [sr[0], sr[1], sr[2]], sr[3]);
        let p = rotate(p, [r[0], r[1], r[2]], r[3]);
        add(add(p, c), t)
    }

    /// Slab test: entry parameter of ray o + t·d into the axis-aligned box
    /// of half-extents `h`, or None.
    pub fn ray_box(o: V, d: V, h: V) -> Option<f64> {
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for i in 0..3 {
            if d[i] == 0.0 {
                if o[i].abs() > h[i] { return None; }
                continue;
            }
            let (a, b) = ((-h[i] - o[i]) / d[i], (h[i] - o[i]) / d[i]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        (t0 <= t1 && t1 >= 0.0).then_some(t0)
    }

    /// Distance from the origin to the line o + t·d.
    pub fn line_distance(o: V, d: V) -> f64 {
        let c = cross(o, d);
        (dot(c, c) / dot(d, d)).sqrt()
    }
}

// ---- projection ------------------------------------------------------------

#[test]
fn iso_defaults_for_all_four_primitives() {
    let s = scene("Shape { geometry Box {} }\nShape { geometry Sphere {} }\nShape { geometry Cylinder {} }\nShape { geometry Cone {} }\n");
    let g: Vec<Geometry> = s.objects.iter().map(|o| o.geometry).collect();
    assert_eq!(
        g,
        vec![
            Geometry::Box { size: [2.0; 3] },
            Geometry::Sphere { radius: 1.0 },
            Geometry::Cylinder { radius: 1.0, height: 2.0, bottom: true, side: true, top: true },
            Geometry::Cone { bottom_radius: 1.0, height: 2.0, bottom: true, side: true },
        ]
    );
    assert!(s.objects.iter().all(|o| o.shading == Shading::Unlit([1.0; 3])));
    assert_eq!(s.objects.iter().map(|o| o.pick_id).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    assert!(!s.damaged);
    assert!(s.not_shown.is_empty());
}

#[test]
fn material_defaults_and_values() {
    let s = scene("Shape { appearance Appearance { material Material {} } geometry Box {} }");
    assert_eq!(only(&s).shading, Shading::Lit(Material::default()));
    let s = scene("Shape { appearance Appearance { material Material { diffuseColor 1 0 0 transparency 0.25 shininess 1 } } geometry Box {} }");
    let Shading::Lit(m) = only(&s).shading else { panic!() };
    assert_eq!((m.diffuse, m.transparency, m.shininess), ([1.0, 0.0, 0.0], 0.25, 1.0));
    assert_eq!(only(&s).shading.alpha(), 0.75);
    // Appearance without a material: unlit white (ISO 4.14.4).
    let s = scene("Shape { appearance Appearance {} geometry Box {} }");
    assert_eq!(only(&s).shading, Shading::Unlit([1.0; 3]));
}

#[test]
fn transform_order_matches_the_independent_reference() {
    let (t, c, r, sc, sr) = ([1.0, -2.0, 3.5], [0.5, 0.25, -1.0], [1.0, 2.0, 0.5, 0.9], [2.0, 0.5, 3.0], [0.0, 1.0, 1.0, -0.7]);
    let body = format!(
        "Transform {{ translation {} {} {} center {} {} {} rotation {} {} {} {} scale {} {} {} scaleOrientation {} {} {} {} children Shape {{ geometry Box {{}} }} }}",
        t[0], t[1], t[2], c[0], c[1], c[2], r[0], r[1], r[2], r[3], sc[0], sc[1], sc[2], sr[0], sr[1], sr[2], sr[3]
    );
    let s = scene(&body);
    let o = only(&s);
    for p in [[1.0, 1.0, 1.0], [-1.0, 0.5, 2.0], [0.0, 0.0, 0.0], [3.0, -4.0, 0.25]] {
        let want = reference::transform(p, t, c, r, sc, sr);
        let got = o.world.transform_point3(DVec3::from(p));
        for i in 0..3 {
            assert!((want[i] - got[i]).abs() < 1e-9, "{p:?}: {want:?} vs {got:?}");
        }
    }
    assert!(!o.mirrored);
}

#[test]
fn nested_transforms_compose_parent_first() {
    let s = scene("Transform { translation 10 0 0 rotation 0 0 1 1.5707963267948966 children Transform { translation 1 0 0 children Shape { geometry Sphere {} } } }");
    let p = only(&s).world.transform_point3(DVec3::ZERO);
    assert!((p - DVec3::new(10.0, 1.0, 0.0)).length() < 1e-12, "{p}");
    assert_eq!(only(&s).path.0.iter().map(|n| n.node_type.as_str()).collect::<Vec<_>>(), ["Transform", "Transform", "Shape"]);
}

#[test]
fn negative_scale_is_mirrored_zero_scale_is_refused() {
    let s = scene("Transform { scale -1 1 1 children Shape { geometry Box {} } }");
    assert!(only(&s).mirrored);
    let s = scene("Transform { scale 0 1 1 children Shape { geometry Box {} } }");
    assert!(s.objects.is_empty());
    assert_eq!(s.not_shown[0].reason, project::reason::SCALE_ZERO);
}

#[test]
fn values_are_never_guessed() {
    for (body, why) in [
        ("Shape { geometry Box { size 1 1 1 size 2 2 2 } }", "duplicate-field"),
        ("Shape { geometry Box { size 1 1 } }", "value-shape"),
        ("Shape { geometry Box { size 1 -1 1 } }", "value-out-of-range"),
        ("Shape { geometry Sphere { radius 0 } }", "value-out-of-range"),
        ("Shape { appearance Appearance { material Material { diffuseColor 2 0 0 } } geometry Box {} }", "value-out-of-range"),
        ("Transform { rotation 0 0 0 1 children Shape { geometry Box {} } }", project::reason::ROTATION_AXIS_ZERO),
        ("Shape { appearance Appearance { texture ImageTexture { url \"a.png\" } } geometry Box {} }", project::reason::TEXTURE),
        ("DEF M Material {}\nShape { appearance Appearance { material USE M } geometry Box {} }", project::reason::APPEARANCE_USE),
        ("DEF G Box {}\nShape { geometry USE G }", project::reason::GEOMETRY_USE),
    ] {
        let s = scene(body);
        assert!(s.objects.is_empty(), "{body}");
        assert!(s.not_shown.iter().any(|n| n.reason == why), "{body}: {:?}", s.not_shown);
    }
}

#[test]
fn out_of_scope_nodes_are_listed_and_not_entered() {
    let s = scene("Group { children Shape { geometry Box {} } }\nShape { geometry IndexedFaceSet {} }\nViewpoint {}\nDEF T Transform {}\nUSE T\n");
    assert!(s.objects.is_empty());
    let r: Vec<(&str, &str)> = s.not_shown.iter().map(|n| (n.node_type.as_str(), n.reason)).collect();
    assert_eq!(
        r,
        [
            ("Group", project::reason::NOT_IN_SCOPE),
            ("IndexedFaceSet", project::reason::NOT_IN_SCOPE),
            ("Viewpoint", project::reason::NOT_IN_SCOPE),
            ("USE", project::reason::USE_NOT_DRAWN),
        ]
    );
}

#[test]
fn proto_bodies_are_never_drawn() {
    let s = scene("PROTO P [] { Shape { geometry Box {} } }\nP {}\n");
    assert!(s.objects.is_empty());
    assert_eq!(s.not_shown[0].node_type, "P");
}

#[test]
fn damaged_parse_is_flagged() {
    assert!(scene("Shape { geometry Box { size 1 2 3 }").damaged);
}

#[test]
fn sensors_apply_to_siblings_and_descendants_only() {
    let s = scene("Transform { children [ TouchSensor {} Transform { children Shape { geometry Box {} } } ] }\nShape { geometry Sphere {} }");
    assert_eq!(s.objects[0].sensors, ["TouchSensor"]);
    assert!(s.objects[1].sensors.is_empty());
}

// ---- identity --------------------------------------------------------------

fn resolve(body: &str, id: u32) -> Resolution {
    let text = format!("{H}{body}");
    let parsed = wrlforge_vrml::parse(&text);
    let s = project(&parsed, 1, 1, 0, 1);
    let obj = s.object(id).expect("object");
    match identity::hit(obj) {
        Err(r) => r,
        Ok(h) => {
            let tree = wrlforge_vrml::scene::build_scene_tree(&parsed.tree, &text);
            pick::resolve(&h, &parsed, &tree, 0)
        }
    }
}

#[test]
fn a_native_hit_proves_through_the_existing_resolver() {
    let body = "Transform { translation 1 0 0 children Shape { geometry Box {} } }";
    let Resolution::Proven(p) = resolve(body, 1) else { panic!("{:?}", resolve(body, 1)) };
    assert_eq!(p.role, pick::Role::SimpleObject);
    assert_eq!(p.logical.node_type, "Transform");
    assert_eq!(p.logical.from as usize, H.len());
    let Resolution::Proven(p) = resolve("Shape { geometry Box {} }", 1) else { panic!() };
    assert_eq!(p.role, pick::Role::Shape);
}

#[test]
fn shared_and_interactive_nodes_are_refused() {
    let r = resolve("DEF T Transform { children Shape { geometry Box {} } }\nTransform { children USE T }", 1);
    assert_eq!((r.status(), r.reason()), (Status::RefusedAmbiguous, pick::reason::SEVERAL_PARENTS));
    let r = resolve("Transform { children [ TouchSensor {} Shape { geometry Box {} } ] }", 1);
    assert_eq!(r.status(), Status::RefusedSensorConflict);
    // Sensors win over sharing, as in `resolve`.
    let r = resolve("DEF T Transform { children [ TouchSensor {} Shape { geometry Box {} } ] }\nUSE T", 1);
    assert_eq!(r.status(), Status::RefusedSensorConflict);
}

#[test]
fn a_hit_against_another_text_fails_the_span_join() {
    let a = format!("{H}Shape {{ geometry Box {{}} }}");
    let b = format!("{H}  Shape {{ geometry Box {{}} }}");
    let s = project(&wrlforge_vrml::parse(&a), 1, 1, 0, 1);
    let h = identity::hit(&s.objects[0]).unwrap();
    let pb = wrlforge_vrml::parse(&b);
    let tree = wrlforge_vrml::scene::build_scene_tree(&pb.tree, &b);
    assert_eq!(pick::resolve(&h, &pb, &tree, 0).status(), Status::Unsupported);
}

// ---- mesh ------------------------------------------------------------------

fn meshes() -> Vec<mesh::Mesh> {
    [
        Geometry::Box { size: [1.0, 2.0, 3.0] },
        Geometry::Sphere { radius: 1.5 },
        Geometry::Cylinder { radius: 1.0, height: 3.0, bottom: true, side: true, top: true },
        Geometry::Cone { bottom_radius: 1.0, height: 2.0, bottom: true, side: true },
    ]
    .iter()
    .map(mesh::tessellate)
    .collect()
}

#[test]
fn every_triangle_faces_outward_and_matches_its_normals() {
    for m in meshes() {
        assert!(!m.indices.is_empty() && m.indices.len() % 3 == 0);
        for t in m.indices.chunks_exact(3) {
            let p = |i: u32| m.positions[i as usize].map(f64::from);
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let n = reference::cross(reference::sub(b, a), reference::sub(c, a));
            assert!(reference::dot(n, n) > 0.0, "degenerate triangle");
            let centroid = reference::mul(reference::add(reference::add(a, b), c), 1.0 / 3.0);
            // Convex, origin-centered: outward means away from the origin.
            assert!(reference::dot(n, centroid) > 0.0, "inward triangle {a:?} {b:?} {c:?}");
            for &i in t {
                assert!(reference::dot(n, m.normals[i as usize].map(f64::from)) > 0.0, "normal disagrees with winding");
            }
        }
    }
}

#[test]
fn caps_and_sides_can_be_switched_off() {
    let full = mesh::tessellate(&Geometry::Cylinder { radius: 1.0, height: 2.0, bottom: true, side: true, top: true });
    let none = mesh::tessellate(&Geometry::Cylinder { radius: 1.0, height: 2.0, bottom: false, side: false, top: true });
    assert_eq!(full.indices.len() / 3, mesh::SEGMENTS as usize * 4);
    assert_eq!(none.indices.len() / 3, mesh::SEGMENTS as usize);
}

// ---- camera ----------------------------------------------------------------

#[test]
fn pick_projection_centers_the_pixel() {
    let v = View::new(&Orbit::default(), 640, 480);
    for (px, py) in [(0u32, 0u32), (320, 240), (639, 479), (17, 401)] {
        let (o, d) = v.pixel_ray(px, py).unwrap();
        let p = o + d * 0.3;
        let c = v.pick_proj(px, py) * v.view * p.extend(1.0);
        assert!((c.x / c.w).abs() < 1e-6 && (c.y / c.w).abs() < 1e-6, "{px},{py}: {c}");
        // And a point half a pixel to the right lands on the target's edge.
        let (o2, d2) = v.ray(px as f64 + 1.0, py as f64 + 0.5).unwrap();
        let c2 = v.pick_proj(px, py) * v.view * (o2 + d2 * 0.3).extend(1.0);
        assert!((c2.x / c2.w - 1.0).abs() < 1e-6, "{c2}");
    }
}

#[test]
fn fit_frames_the_bounds() {
    let o = Orbit::fit(Some((DVec3::new(5.0, 0.0, 0.0), 2.0)));
    assert_eq!(o.target, DVec3::new(5.0, 0.0, 0.0));
    let v = View::new(&o, 100, 100);
    let c = v.view_proj() * DVec4::new(7.0, 0.0, 0.0, 1.0);
    assert!((c.x / c.w).abs() < 1.0);
    assert_eq!(Orbit::fit(None), Orbit::default());
}

// ---- oracle vs the independent reference ------------------------------------

fn sweep(s: &RenderScene, v: &View, step: u32, mut check: impl FnMut(u32, u32, Answer)) {
    for py in (0..v.height).step_by(step as usize) {
        for px in (0..v.width).step_by(step as usize) {
            check(px, py, oracle::pick(&s.objects, v, px, py));
        }
    }
}

#[test]
fn box_oracle_agrees_with_the_slab_reference() {
    let s = scene("Transform { rotation 1 1 0 0.6 children Shape { geometry Box { size 2 1 3 } } }");
    let o = only(&s);
    let inv = o.world.inverse();
    let v = View::new(&Orbit { yaw: 0.4, pitch: 0.3, ..Orbit::fit(Some((DVec3::ZERO, 2.0))) }, 160, 120);
    let (mut hits, mut misses, mut refused) = (0, 0, 0);
    sweep(&s, &v, 2, |px, py, a| {
        let (ro, rd) = v.pixel_ray(px, py).unwrap();
        let want = reference::ray_box(inv.transform_point3(ro).to_array(), inv.transform_vector3(rd).to_array(), [1.0, 0.5, 1.5]);
        match a {
            Answer::Object(1) => {
                hits += 1;
                assert!(want.is_some(), "{px},{py}: oracle hit, reference miss");
            }
            Answer::Background => {
                misses += 1;
                assert!(want.is_none(), "{px},{py}: oracle miss, reference hit");
            }
            Answer::Refused(r) => {
                refused += 1;
                assert_eq!(r, oracle::reason::NEAR_EDGE);
                // No incorrect refusal: the reference itself must show a
                // silhouette within 1.5 physical pixels of the sample.
                let at = |x: f64, y: f64| {
                    let (ro, rd) = v.ray(x, y).unwrap();
                    reference::ray_box(inv.transform_point3(ro).to_array(), inv.transform_vector3(rd).to_array(), [1.0, 0.5, 1.5]).is_some()
                };
                let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
                let ring = (0..16).map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / 16.0;
                    at(cx + 1.5 * a.cos(), cy + 1.5 * a.sin())
                });
                let mut seen = vec![at(cx, cy)];
                seen.extend(ring);
                assert!(seen.iter().any(|h| *h) && seen.iter().any(|h| !*h), "{px},{py}: refused with no edge within 1.5 px");
            }
            other => panic!("{other:?}"),
        }
    });
    assert!(hits > 200 && misses > 200 && refused > 0, "{hits} {misses} {refused}");
}

#[test]
fn sphere_oracle_agrees_with_the_analytic_reference_off_the_silhouette() {
    let s = scene("Shape { geometry Sphere { radius 1.5 } }");
    let v = View::new(&Orbit { yaw: 1.1, pitch: -0.4, ..Orbit::fit(Some((DVec3::ZERO, 1.5))) }, 120, 120);
    let inscribed = 1.5 * (std::f64::consts::PI / mesh::RINGS as f64).cos() * (std::f64::consts::PI / mesh::SEGMENTS as f64).cos();
    let mut checked = 0;
    sweep(&s, &v, 3, |px, py, a| {
        let (o, d) = v.pixel_ray(px, py).unwrap();
        let dist = reference::line_distance(o.to_array(), d.to_array());
        if dist < inscribed * 0.97 {
            assert_eq!(a, Answer::Object(1), "{px},{py}");
            checked += 1;
        } else if dist > 1.5 * 1.03 {
            assert_eq!(a, Answer::Background, "{px},{py}");
            checked += 1;
        }
    });
    assert!(checked > 1_000, "{checked}");
}

#[test]
fn internal_triangle_edges_are_not_boundaries() {
    // The +Z face's diagonal runs through the view center; the band must not
    // refuse there because both triangles belong to the same object.
    let s = scene("Shape { geometry Box { size 4 4 4 } }");
    let v = View::new(&Orbit::default(), 101, 101);
    assert_eq!(oracle::pick(&s.objects, &v, 50, 50), Answer::Object(1));
    assert_eq!(oracle::pick(&s.objects, &v, 40, 40), Answer::Object(1));
}

#[test]
fn nearest_object_wins_and_ties_are_refused() {
    let s = scene("Transform { translation 0 0 1 children Shape { geometry Box {} } }\nShape { geometry Box { size 3 3 0.5 } }");
    let v = View::new(&Orbit::default(), 101, 101);
    assert_eq!(oracle::pick(&s.objects, &v, 50, 50), Answer::Object(1));
    let s = scene("Shape { geometry Box {} }\nShape { geometry Box {} }");
    assert_eq!(oracle::pick(&s.objects, &v, 50, 50), Answer::Refused(oracle::reason::DEPTH_TIE));
}

#[test]
fn a_visible_edge_within_one_pixel_is_refused() {
    let s = scene("Shape { geometry Box {} }");
    let v = View::new(&Orbit::default(), 200, 200);
    // Find the box's right silhouette along the center row.
    let row: Vec<Answer> = (0..200).map(|x| oracle::pick(&s.objects, &v, x, 100)).collect();
    let refused: Vec<u32> = (0..200).filter(|&x| matches!(row[x as usize], Answer::Refused(_))).collect();
    assert!(!refused.is_empty() && refused.len() <= 8, "{refused:?}");
    for x in refused {
        assert_eq!(row[x as usize], Answer::Refused(oracle::reason::NEAR_EDGE));
    }
}

#[test]
fn mirrored_objects_show_their_outside() {
    let plain = scene("Shape { geometry Box {} }");
    let mirror = scene("Transform { scale -1 1 1 children Shape { geometry Box {} } }");
    let v = View::new(&Orbit::default(), 101, 101);
    let (o, d) = v.pixel_ray(50, 50).unwrap();
    let a = oracle::trace(&plain.objects, o, d);
    let b = oracle::trace(&mirror.objects, o, d);
    assert_eq!(a.len(), 1);
    assert!((a[0].t - b[0].t).abs() < 1e-12, "{a:?} {b:?}");
}

#[test]
fn outside_and_disagreement_rules() {
    let s = scene("Shape { geometry Box {} }");
    let v = View::new(&Orbit::default(), 10, 10);
    assert_eq!(oracle::pick(&s.objects, &v, 10, 0), Answer::Refused(oracle::reason::OUTSIDE));
    use oracle::agree;
    assert_eq!(agree(Answer::Object(3), Ok(3)), Verdict::Object(3));
    assert_eq!(agree(Answer::Background, Ok(0)), Verdict::Background);
    assert_eq!(agree(Answer::Object(3), Ok(4)), Verdict::Refused(oracle::reason::DISAGREE));
    assert_eq!(agree(Answer::Object(3), Ok(0)), Verdict::Refused(oracle::reason::DISAGREE));
    assert_eq!(agree(Answer::Background, Ok(2)), Verdict::Refused(oracle::reason::DISAGREE));
    assert_eq!(agree(Answer::Object(3), Err(())), Verdict::Refused(oracle::reason::GPU_FAILED));
    assert_eq!(agree(Answer::Refused("x"), Ok(3)), Verdict::Refused("x"));
}

#[test]
fn object_lookup_is_bounded() {
    let s = scene("Shape { geometry Box {} }");
    assert!(s.object(0).is_none() && s.object(2).is_none() && s.object(HANDLE_ID_BASE).is_none());
    assert_eq!(s.object(1).map(|o| o.pick_id), Some(1));
}
