// SPDX-License-Identifier: GPL-3.0-or-later
//! B2: the selection highlight -- a cyan bounding-box outline of the
//! selected node's rendered geometry over the viewport (Spazz3D / Flux
//! parity).
//!
//! VIEW STATE ONLY. Nothing here writes the source, marks the document
//! dirty or adds an undo step; the scene is never mutated (the outline is an
//! SVG overlay, like the gizmo).
//!
//! The outlined runtime node is never guessed. It is reached exactly the way
//! the gizmo binds a Transform (VISUAL-3A1): Rust names the item's exact
//! preview span (`doc_highlight_target`), the preview locates the ONE runtime
//! node its parser recorded for that span (`snapshotSpan`), and Rust proves
//! that node's runtime chain link by link (`doc_highlight_prove`,
//! `pick::resolve_node`). Only then are its bounds read, through `nodeAt` on
//! the same span, with each proven ancestor's frame through `nodeAt` on the
//! ancestor's span. B3: a PROTO body node has one runtime copy per instance;
//! the outline is drawn on the PICKED instance's copy (`snapshotBodySpan` /
//! `bodyNodeAt`), its ancestors followed up the runtime chain of that copy
//! and checked span by span against Rust's proof. Anything unprovable --
//! several instances and no pick on one, a shared
//! DEF/USE node, a node under a Billboard, a stale preview -- shows NO
//! outline and a short status note.
//!
//! The box is re-read a few times a second (it moves with animation and
//! the gizmo's temporary frame) and re-projected through the renderer's
//! real camera every frame. It disappears the moment the selection, the
//! revision or the preview generation changes, until the new one is proven.

use std::cell::RefCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::highlight as hl;

use crate::editor::{now, CORE};
use crate::ipc::{self, call};
use crate::ui::ui;

/// How often the rendered box is re-read (ms); the camera is read per frame,
/// and the box too while a gizmo drag moves the node.
const BOUNDS_MS: f64 = 200.0;

/// (session, item, revision, preview generation): what an outline is for.
type Key = (u64, String, u64, u64);

/// A proven node, bound to the preview generation `seq`.
#[derive(Clone, Debug, PartialEq)]
pub struct Proven {
    pub seq: u64,
    pub span: (u64, u64),
    pub node_type: String,
    /// The direct parent first: preview span, source type, and whether it is
    /// the PROTO instance the chain was proven through (B3).
    pub ancestors: Vec<((u64, u64), String, bool)>,
    /// B3: a PROTO body node (located as the picked instance's copy).
    pub body: bool,
}

#[derive(Default)]
pub struct HighlightState {
    key: Option<Key>,
    req_seq: u64,
    pub target: Option<Proven>,
    bounds: Option<(f64, Result<hl::Bounds, String>)>,
    drawn: String,
    loop_on: bool,
    // ---- test / diagnostics counters --------------------------------------
    pub proofs: u64,
    pub refusals: u64,
    pub last_refusal: Option<String>,
    /// The SVG path data last drawn ("" when hidden).
    pub path: String,
}

type FrameCallback = Closure<dyn FnMut(f64)>;

thread_local! {
    pub static HIGHLIGHT: RefCell<HighlightState> = RefCell::new(HighlightState::default());
    static FRAME: RefCell<Option<FrameCallback>> = const { RefCell::new(None) };
}

fn note(text: Option<String>) {
    let u = ui();
    if u.highlight_message.get_untracked() != text {
        u.highlight_message.set(text);
    }
}

fn refused(reason: &str) {
    HIGHLIGHT.with_borrow_mut(|h| {
        h.refusals += 1;
        h.last_refusal = Some(reason.to_string());
    });
    note(Some(format!("No outline: {} [{reason}]", explain(reason))));
}

/// One short sentence per refusal family.
fn explain(reason: &str) -> &'static str {
    match reason {
        "runtime-node-for-span-not-unique" | "proto-instance" => {
            "the preview draws this node more than once or inside a PROTO definition"
        }
        "proto-instance-not-picked" => {
            "its PROTO has several instances on screen; click the instance in the viewport to outline it"
        }
        "runtime-node-referenced-by-several-source-occurrences"
        | "runtime-node-has-several-live-parents" => "it is drawn by a shared (DEF/USE) node",
        hl::refusal::ANCESTOR => {
            "it is inside a Billboard or another node the outline cannot follow"
        }
        hl::refusal::NO_BOUNDS | "bounds-unavailable" => "it has no rendered geometry",
        hl::refusal::BEHIND_CAMERA => "part of it is behind the camera",
        "document-has-syntax-errors" | "document-parse-capped" => {
            "the document must parse cleanly first"
        }
        "not-a-node" => "only a node instance can be outlined",
        _ => "the preview cannot prove which rendered object this is",
    }
}

/// The current (session, selection, active generation) key, if an outline
/// could be asked for at all.
fn current_key() -> Option<Key> {
    let session = CORE.with_borrow(|c| c.session)?;
    let sel = ui().selected.get_untracked()?;
    let gen = crate::pick::active()?;
    (sel.session == session && gen.session == session && gen.revision == sel.revision).then_some((
        session,
        sel.id,
        sel.revision,
        gen.seq,
    ))
}

/// Locate and prove the selected node; on success the frame loop draws it.
fn request(key: Key) {
    let seq = HIGHLIGHT.with_borrow_mut(|h| {
        h.req_seq += 1;
        h.req_seq
    });
    spawn_local(async move {
        let (session, item, revision, gen_seq) = key.clone();
        let current = || {
            HIGHLIGHT.with_borrow(|h| h.req_seq == seq && h.key.as_ref() == Some(&key))
                && current_key().as_ref() == Some(&key)
                && CORE.with_borrow(|c| c.revision == revision && !c.busy)
        };
        if !current() {
            return;
        }
        #[derive(serde::Serialize)]
        struct T {
            request: p::HighlightTargetRequest,
        }
        let t = call::<p::HighlightTargetOutcome>(
            "doc_highlight_target",
            T {
                request: p::HighlightTargetRequest {
                    session,
                    revision,
                    item: item.clone(),
                },
            },
        )
        .await;
        if !current() {
            return;
        }
        let (span, body) = match t {
            Ok(p::HighlightTargetOutcome::Ready {
                revision: r,
                item: i,
                preview_from,
                preview_to,
                body,
            }) if r == revision && i == item => ((preview_from, preview_to), body),
            Ok(p::HighlightTargetOutcome::Refused { reason, .. }) => return refused(&reason),
            Ok(_) => return,
            Err(e) => return refused(&e),
        };
        let Some(gen) = crate::pick::active().filter(|g| g.seq == gen_seq) else {
            return;
        };
        let snapshot = match ipc::gizmo_locate(gen_seq, span.0, span.1, body) {
            Ok(s) => s,
            Err(e) => return refused(&e),
        };
        #[derive(serde::Serialize)]
        struct A {
            request: p::HighlightProveRequest,
        }
        let r = call::<p::HighlightProveOutcome>(
            "doc_highlight_prove",
            A {
                request: p::HighlightProveRequest {
                    session,
                    revision,
                    preview_hash: gen.hash,
                    generation: gen_seq,
                    item: item.clone(),
                    snapshot,
                },
            },
        )
        .await;
        if !current() {
            return;
        }
        match r {
            Ok(p::HighlightProveOutcome::Proven {
                revision: rev,
                item: it,
                node_type,
                ancestors,
            }) if rev == revision && it == item => {
                let t = Proven {
                    seq: gen_seq,
                    span,
                    node_type,
                    ancestors: ancestors
                        .into_iter()
                        .map(|a| ((a.preview_from, a.preview_to), a.node_type, a.proto_instance))
                        .collect(),
                    body,
                };
                HIGHLIGHT.with_borrow_mut(|h| {
                    h.proofs += 1;
                    h.target = Some(t);
                    h.bounds = None;
                    h.drawn.clear();
                });
                note(None);
            }
            Ok(p::HighlightProveOutcome::Proven { .. }) => refused("proof-for-another-node"),
            Ok(p::HighlightProveOutcome::Refused { reason, .. }) => refused(&reason),
            Ok(p::HighlightProveOutcome::Stale { .. }) => {}
            Err(e) => refused(&e),
        }
    });
}

/// The proven node's bounds, re-read at most every `BOUNDS_MS`. The
/// renderer's ancestor types must equal the proven source types.
fn bounds_of(t: &Proven) -> Result<hl::Bounds, String> {
    let fresh = HIGHLIGHT.with_borrow(|h| {
        h.bounds
            .as_ref()
            // A gizmo drag moves the rendered node every frame: follow it.
            .filter(|(at, _)| !crate::gizmo::dragging() && now() - at < BOUNDS_MS)
            .map(|(_, b)| b.clone())
    });
    if let Some(b) = fresh {
        return b;
    }
    let spans: Vec<(u64, u64)> = t.ancestors.iter().map(|(s, _, _)| *s).collect();
    let b = ipc::highlight_bounds(t.seq, t.span, &spans, t.body).and_then(|mut b| {
        let agrees = b.ancestors.len() == t.ancestors.len()
            && b.ancestors
                .iter()
                .zip(&t.ancestors)
                .all(|(r, (_, ty, _))| r.node_type == *ty);
        if agrees {
            // The instance flag comes from Rust's proof, never the renderer.
            for (r, (_, _, inst)) in b.ancestors.iter_mut().zip(&t.ancestors) {
                r.proto_instance = *inst;
            }
            Ok(b)
        } else {
            Err(hl::refusal::ANCESTOR_DISAGREES.to_string())
        }
    });
    HIGHLIGHT.with_borrow_mut(|h| h.bounds = Some((now(), b.clone())));
    b
}

/// One animation frame while something is selected.
fn tick() {
    let key = current_key();
    let changed = HIGHLIGHT.with_borrow(|h| h.key != key);
    if changed {
        HIGHLIGHT.with_borrow_mut(|h| {
            h.key = key.clone();
            h.target = None;
            h.bounds = None;
        });
        note(None);
        draw(None);
        if let Some(k) = key.clone() {
            if CORE.with_borrow(|c| c.revision == k.2 && !c.busy) {
                request(k);
            }
        }
    }
    let Some(t) = HIGHLIGHT.with_borrow(|h| h.target.clone()) else {
        draw(None);
        return;
    };
    let outline = bounds_of(&t)
        .and_then(|b| hl::world_corners(&b).map_err(String::from))
        .and_then(|c| {
            let cam = ipc::gizmo_camera().ok_or("camera-unavailable")?;
            hl::outline(&cam, &c).map_err(String::from)
        });
    match outline {
        Ok(edges) => {
            if ui().highlight_message.get_untracked().is_some() {
                note(None);
            }
            draw(Some(&edges));
        }
        Err(why) => {
            draw(None);
            let msg = Some(format!("No outline: {} [{why}]", explain(&why)));
            if ui().highlight_message.get_untracked() != msg {
                refused(&why);
            }
        }
    }
}

/// Update the overlay; `None` hides it.
fn draw(edges: Option<&[hl::Edge; 12]>) {
    let Some(svg) = crate::element_by_id::<web_sys::Element>("selection-outline") else {
        return;
    };
    let d = match edges {
        Some(e) => {
            let r = svg.get_bounding_client_rect();
            hl::svg_path(e, r.left(), r.top())
        }
        None => String::new(),
    };
    if HIGHLIGHT.with_borrow(|h| h.drawn == d) {
        return;
    }
    HIGHLIGHT.with_borrow_mut(|h| {
        h.drawn = d.clone();
        h.path = d.clone();
    });
    if d.is_empty() {
        let _ = svg.set_attribute("data-state", "hidden");
        return;
    }
    for id in ["hl-halo", "hl-line"] {
        if let Some(e) = crate::element_by_id::<web_sys::Element>(id) {
            let _ = e.set_attribute("d", &d);
        }
    }
    let _ = svg.set_attribute("data-state", "shown");
}

fn schedule() {
    FRAME.with_borrow(|f| {
        if let (Some(cb), Some(w)) = (f.as_ref(), web_sys::window()) {
            let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    });
}

/// B3: a viewport pick landed (maybe on the same item, another instance):
/// prove the outline again on the next frame.
pub fn repicked() {
    HIGHLIGHT.with_borrow_mut(|h| h.key = None);
}

/// Turn the frame loop on (a selection exists). Idempotent.
pub fn start() {
    let on = HIGHLIGHT.with_borrow_mut(|h| std::mem::replace(&mut h.loop_on, true));
    if !on {
        schedule();
    }
}

/// Install the frame loop; it runs while something is selected.
pub fn install() {
    let cb = Closure::<dyn FnMut(f64)>::new(|_t: f64| {
        tick();
        if ui().selected.get_untracked().is_some() {
            schedule();
        } else {
            HIGHLIGHT.with_borrow_mut(|h| {
                h.loop_on = false;
                h.key = None;
                h.target = None;
                h.bounds = None;
            });
            note(None);
            draw(None);
        }
    });
    FRAME.with_borrow_mut(|f| *f = Some(cb));
    if ui().selected.get_untracked().is_some() {
        start();
    }
}
