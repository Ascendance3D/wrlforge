// SPDX-License-Identifier: GPL-3.0-or-later
//! The VRML97/X3D node and field schema (TAURI-RUST-MIGRATION-2). Rust
//! translation of the query API of `src/vrml/node-schema.js`; the DATA is a
//! generated, field-for-field projection of that committed file
//! (`node_schema_data.rs`, `scripts/build-rust-node-schema.js`).
//!
//! Lookups return `None` for an unknown name: an unknown node is an ordinary
//! fact about a document (a PROTO, a vendor extension), not an error.
//!
//! READ BEFORE USING `constraints`: `None` means NO MACHINE-REPRESENTED
//! CONSTRAINT IS AVAILABLE. It does not mean the field is unrestricted, and
//! nothing may be clamped or rejected on the strength of it. A present record
//! is not exhaustive either -- a `note` marks a restriction this shape cannot
//! carry.

// The data is the JS schema's literals verbatim (`1.570796` is the ISO
// default text, not an approximation to fix), and is generated, not styled.
#[rustfmt::skip]
#[allow(clippy::approx_constant)]
#[path = "node_schema_data.rs"]
mod data;

/// A schema default value, exactly as `defaultValue` in the JS schema.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DefaultValue {
    Null,
    Bool(bool),
    Num(f64),
    Str(&'static str),
    List(&'static [DefaultValue]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstraintNote {
    pub category: &'static str,
    pub source: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraints {
    pub min: Option<f64>,
    pub min_symbolic: Option<&'static str>,
    pub min_inclusive: Option<bool>,
    pub max: Option<f64>,
    pub max_symbolic: Option<&'static str>,
    pub max_inclusive: Option<bool>,
    pub note: Option<ConstraintNote>,
    pub accepted_node_classes: Option<&'static [&'static str]>,
    pub accepted_node_types: Option<&'static [&'static str]>,
    pub rules: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldSchema {
    pub name: &'static str,
    /// The field type token (`SFVec3f`, `MFNode`, ...).
    pub field_type: &'static str,
    pub access_type: &'static str,
    pub x3d_access_type: Option<&'static str>,
    /// `field` | `exposedField` | `eventIn` | `eventOut`; `None` for X3D-only.
    pub vrml97_declaration: Option<&'static str>,
    pub profiles: &'static [&'static str],
    pub order: Option<u32>,
    pub default_text: Option<&'static str>,
    /// `None` when the JS record has no `defaultValue` key at all.
    pub default_value: Option<DefaultValue>,
    pub constraints: Option<&'static Constraints>,
}

impl FieldSchema {
    pub fn in_profile(&self, profile: Profile) -> bool {
        self.profiles.contains(&profile.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeSchema {
    pub name: &'static str,
    pub section: &'static str,
    pub profiles: &'static [&'static str],
    pub classes: &'static [&'static str],
    /// ASCII-sorted by name.
    pub fields: &'static [FieldSchema],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeClass {
    pub id: &'static str,
    pub label: &'static str,
    pub form: &'static str,
    pub section: &'static str,
    pub members: &'static [&'static str],
}

/// The two schema profiles. A typed enum, so the JS `ESCHEMAPROFILE` caller
/// error cannot be expressed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Vrml97,
    X3d,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Vrml97 => "vrml97",
            Profile::X3d => "x3d",
        }
    }
}

/// Every node record, ASCII-sorted by name.
pub fn nodes() -> &'static [NodeSchema] {
    data::NODES
}

/// Clause 4 node classes, in the JS schema's order.
pub fn node_classes() -> &'static [NodeClass] {
    data::NODE_CLASSES
}

/// The JS schema's `COUNTS`, key-sorted.
pub fn counts() -> &'static [(&'static str, u32)] {
    data::COUNTS
}

pub fn get_node_schema(name: &str) -> Option<&'static NodeSchema> {
    let nodes = data::NODES;
    nodes
        .binary_search_by(|n| n.name.cmp(name))
        .ok()
        .map(|i| &nodes[i])
}

pub fn get_field_schema(node: &str, field: &str) -> Option<&'static FieldSchema> {
    let n = get_node_schema(node)?;
    n.fields
        .binary_search_by(|f| f.name.cmp(field))
        .ok()
        .map(|i| &n.fields[i])
}

/// Every node name, ASCII-sorted; `Some(profile)` restricts to that profile.
pub fn list_node_names(profile: Option<Profile>) -> Vec<&'static str> {
    data::NODES
        .iter()
        .filter(|n| profile.is_none_or(|p| n.profiles.contains(&p.as_str())))
        .map(|n| n.name)
        .collect()
}

/// Every field name of a node, ASCII-sorted; empty for an unknown node.
pub fn list_fields(node: &str, profile: Option<Profile>) -> Vec<&'static str> {
    get_node_schema(node)
        .map(|n| {
            n.fields
                .iter()
                .filter(|f| profile.is_none_or(|p| f.in_profile(p)))
                .map(|f| f.name)
                .collect()
        })
        .unwrap_or_default()
}

/// Is this field legal on this node in this profile (`None`: known at all)?
pub fn is_field_allowed(node: &str, field: &str, profile: Option<Profile>) -> bool {
    get_field_schema(node, field).is_some_and(|f| profile.is_none_or(|p| f.in_profile(p)))
}

pub fn is_vrml97_node(name: &str) -> bool {
    get_node_schema(name).is_some_and(|n| n.profiles.contains(&"vrml97"))
}

pub fn is_vrml97_field(node: &str, field: &str) -> bool {
    is_field_allowed(node, field, Some(Profile::Vrml97))
}

/// The clause 4 classes of a node (empty for an unknown name).
pub fn get_node_classes(name: &str) -> &'static [&'static str] {
    get_node_schema(name).map(|n| n.classes).unwrap_or(&[])
}

/// The members of one clause 4 class (empty for an unknown class id).
pub fn list_nodes_in_class(class_id: &str) -> &'static [&'static str] {
    data::NODE_CLASSES
        .iter()
        .find(|c| c.id == class_id)
        .map(|c| c.members)
        .unwrap_or(&[])
}

/// See the module header: `None` is NOT "unrestricted".
pub fn get_field_constraints(node: &str, field: &str) -> Option<&'static Constraints> {
    get_field_schema(node, field).and_then(|f| f.constraints)
}

// --- canonical JSON, for the JS parity check (examples/schema_dump.rs) -------

fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_num(n: f64, out: &mut String) {
    // `{}` never prints an exponent; JSON.parse on both sides compares values.
    out.push_str(&format!("{n}"));
}

fn json_list(a: &[&str], out: &mut String) {
    out.push('[');
    for (i, s) in a.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(s, out);
    }
    out.push(']');
}

fn json_value(v: &DefaultValue, out: &mut String) {
    match v {
        DefaultValue::Null => out.push_str("null"),
        DefaultValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        DefaultValue::Num(n) => json_num(*n, out),
        DefaultValue::Str(s) => json_str(s, out),
        DefaultValue::List(items) => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_value(it, out);
            }
            out.push(']');
        }
    }
}

struct Obj<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> Obj<'a> {
    fn new(out: &'a mut String) -> Self {
        out.push('{');
        Obj { out, first: true }
    }
    fn key(&mut self, k: &str) -> &mut String {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        json_str(k, self.out);
        self.out.push(':');
        self.out
    }
    fn end(self) {
        self.out.push('}');
    }
}

fn json_constraints(c: &Constraints, out: &mut String) {
    let mut o = Obj::new(out);
    if let Some(v) = c.min {
        json_num(v, o.key("min"));
    }
    if let Some(v) = c.min_symbolic {
        json_str(v, o.key("minSymbolic"));
    }
    if let Some(v) = c.min_inclusive {
        o.key("minInclusive")
            .push_str(if v { "true" } else { "false" });
    }
    if let Some(v) = c.max {
        json_num(v, o.key("max"));
    }
    if let Some(v) = c.max_symbolic {
        json_str(v, o.key("maxSymbolic"));
    }
    if let Some(v) = c.max_inclusive {
        o.key("maxInclusive")
            .push_str(if v { "true" } else { "false" });
    }
    if let Some(n) = c.note {
        let w = o.key("note");
        let mut no = Obj::new(w);
        json_str(n.category, no.key("category"));
        json_str(n.source, no.key("source"));
        no.end();
    }
    if let Some(v) = c.accepted_node_classes {
        json_list(v, o.key("acceptedNodeClasses"));
    }
    if let Some(v) = c.accepted_node_types {
        json_list(v, o.key("acceptedNodeTypes"));
    }
    json_list(c.rules, o.key("rules"));
    o.end();
}

/// The whole schema as JSON shaped like the JS module's `nodes`,
/// `nodeClasses` and `counts` exports. Parity tooling only.
pub fn to_canonical_json() -> String {
    let mut out = String::new();
    let mut root = Obj::new(&mut out);
    {
        let w = root.key("nodes");
        let mut nodes = Obj::new(w);
        for n in data::NODES {
            let w = nodes.key(n.name);
            let mut no = Obj::new(w);
            json_str(n.name, no.key("name"));
            json_str(n.section, no.key("section"));
            json_list(n.profiles, no.key("profiles"));
            json_list(n.classes, no.key("classes"));
            let w = no.key("fields");
            let mut fo = Obj::new(w);
            for f in n.fields {
                let w = fo.key(f.name);
                let mut o = Obj::new(w);
                json_str(f.field_type, o.key("type"));
                json_str(f.access_type, o.key("accessType"));
                match f.vrml97_declaration {
                    Some(d) => json_str(d, o.key("vrml97Declaration")),
                    None => o.key("vrml97Declaration").push_str("null"),
                }
                if let Some(x) = f.x3d_access_type {
                    json_str(x, o.key("x3dAccessType"));
                }
                json_list(f.profiles, o.key("profiles"));
                match f.order {
                    Some(v) => o.key("order").push_str(&v.to_string()),
                    None => o.key("order").push_str("null"),
                }
                if let Some(t) = f.default_text {
                    json_str(t, o.key("defaultText"));
                }
                if let Some(v) = &f.default_value {
                    json_value(v, o.key("defaultValue"));
                }
                match f.constraints {
                    Some(c) => json_constraints(c, o.key("constraints")),
                    None => o.key("constraints").push_str("null"),
                }
                o.end();
            }
            fo.end();
            no.end();
        }
        nodes.end();
    }
    {
        let w = root.key("nodeClasses");
        let mut co = Obj::new(w);
        for c in data::NODE_CLASSES {
            let w = co.key(c.id);
            let mut o = Obj::new(w);
            json_str(c.form, o.key("form"));
            json_str(c.id, o.key("id"));
            json_str(c.label, o.key("label"));
            json_list(c.members, o.key("members"));
            json_str(c.section, o.key("section"));
            o.end();
        }
        co.end();
    }
    {
        let w = root.key("counts");
        let mut o = Obj::new(w);
        for (k, v) in data::COUNTS {
            o.key(k).push_str(&v.to_string());
        }
        o.end();
    }
    root.end();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(key: &str) -> usize {
        counts().iter().find(|(k, _)| *k == key).unwrap().1 as usize
    }

    #[test]
    fn counts_match_the_generated_js_counts() {
        assert_eq!(nodes().len(), count("nodes"));
        assert_eq!(node_classes().len(), count("nodeClasses"));
        let vrml97: usize = nodes()
            .iter()
            .map(|n| {
                n.fields
                    .iter()
                    .filter(|f| f.in_profile(Profile::Vrml97))
                    .count()
            })
            .sum();
        assert_eq!(vrml97, count("isoDeclarations"));
        let x3d_only: usize = nodes()
            .iter()
            .map(|n| n.fields.iter().filter(|f| f.profiles == ["x3d"]).count())
            .sum();
        assert_eq!(x3d_only, count("x3dOnly"));
        assert_eq!(list_node_names(Some(Profile::Vrml97)).len(), 54);
    }

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(nodes().windows(2).all(|w| w[0].name < w[1].name));
        for n in nodes() {
            assert!(
                n.fields.windows(2).all(|w| w[0].name < w[1].name),
                "{}",
                n.name
            );
        }
    }

    #[test]
    fn transform_and_material_records_are_exact() {
        let t = get_field_schema("Transform", "translation").unwrap();
        assert_eq!(t.field_type, "SFVec3f");
        assert_eq!(t.vrml97_declaration, Some("exposedField"));
        assert_eq!(t.default_text, Some("0 0 0"));
        assert_eq!(
            t.default_value,
            Some(DefaultValue::List(&[
                DefaultValue::Num(0.0),
                DefaultValue::Num(0.0),
                DefaultValue::Num(0.0)
            ]))
        );
        let r = get_field_schema("Transform", "rotation").unwrap();
        assert_eq!(r.field_type, "SFRotation");
        assert_eq!(
            r.constraints.unwrap().note.unwrap().category,
            "PER_COMPONENT_RANGE"
        );
        let d = get_field_schema("Material", "diffuseColor").unwrap();
        assert_eq!(d.field_type, "SFColor");
        assert_eq!(d.default_text, Some("0.8 0.8 0.8"));
        let c = d.constraints.unwrap();
        assert_eq!(
            (c.min, c.min_inclusive, c.max, c.max_inclusive),
            (Some(0.0), Some(true), Some(1.0), Some(true))
        );
        let shin = get_field_constraints("Material", "shininess").unwrap();
        assert_eq!((shin.min, shin.max), (Some(0.0), Some(1.0)));
        assert_eq!(
            get_field_constraints("Appearance", "material")
                .unwrap()
                .accepted_node_types,
            Some(&["Material"][..])
        );
    }

    #[test]
    fn profile_tagging_stops_x3d_leaks() {
        assert!(is_field_allowed("Transform", "bboxDisplay", None));
        assert!(is_field_allowed(
            "Transform",
            "bboxDisplay",
            Some(Profile::X3d)
        ));
        assert!(!is_field_allowed(
            "Transform",
            "bboxDisplay",
            Some(Profile::Vrml97)
        ));
        assert!(!is_vrml97_field("Transform", "metadata"));
        assert!(is_vrml97_field("Transform", "scale"));
        assert!(is_vrml97_node("Transform"));
        assert!(!is_vrml97_node("MyProto"));
        assert!(get_node_schema("transform").is_none(), "case-sensitive");
        assert!(get_field_constraints("Transform", "bboxDisplay").is_none());
        assert!(list_fields("Nope", None).is_empty());
        assert!(!list_fields("Transform", Some(Profile::Vrml97)).contains(&"metadata"));
    }

    #[test]
    fn node_classes_answer_both_directions() {
        assert_eq!(get_node_classes("Transform"), &["children", "grouping"]);
        assert!(list_nodes_in_class("geometry").contains(&"Box"));
        assert!(list_nodes_in_class("nope").is_empty());
        assert!(get_node_classes("Nope").is_empty());
    }
}
