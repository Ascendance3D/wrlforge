// SPDX-License-Identifier: GPL-3.0-or-later
//! A native hit -> the plain-data `pick::Hit` the existing resolver proves.
//!
//! The native renderer is a second RUNTIME, never a second identity
//! authority: it reports what it drew (an instance path of exact spans) in
//! the same shape the X_ITE adapter reports, and `wrlforge_vrml::pick::resolve`
//! joins it to the canonical parse and the Scene Tree exactly as before.

use std::collections::HashMap;

use wrlforge_vrml::pick::{self, Ctx, GraphNode, Hit, Parent, Resolution, Status};

use crate::RenderObject;

/// Build the hit for one drawn object. Any node on the path whose DEF name
/// is also `USE`d is refused as ambiguous: X_ITE would show that node under
/// several parents, so a click on it cannot name one source occurrence.
/// Sensors are passed through for `resolve` to refuse first, as it does for
/// X_ITE hits.
pub fn hit(obj: &RenderObject) -> Result<Hit, Resolution> {
    let path = &obj.path.0;
    if path.is_empty() {
        return Err(pick::refuse(Status::Unsupported, pick::reason::NO_PROVENANCE));
    }
    if obj.sensors.is_empty() && path.iter().any(|n| !n.use_sites.is_empty()) {
        return Err(pick::refuse(Status::RefusedAmbiguous, pick::reason::SEVERAL_PARENTS));
    }
    let label = |i: usize| format!("n{i}");
    let graph: HashMap<String, GraphNode> = path
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let parent = if i == 0 { Parent::Scene } else { Parent::Node(label(i - 1)) };
            (
                label(i),
                GraphNode {
                    type_name: Some(n.node_type.clone()),
                    ctx: Ctx::Document,
                    occurrences: vec![(n.from, n.to)],
                    parents: vec![parent],
                },
            )
        })
        .collect();
    Ok(Hit {
        shape: label(path.len() - 1),
        ctx: Ctx::Document,
        sensors: obj.sensors.iter().cloned().map(Some).collect(),
        graph,
    })
}
