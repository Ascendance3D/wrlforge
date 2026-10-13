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
//! B3 (LV2): the copy is the one of the instance Rust proved (after a gizmo
//! commit, the instance Rust carried through the edit, `pick::instance_hint`).
//! Because an edit of a shared definition changes EVERY instance, each OTHER
//! instance's copy is located by its own instance span, proven the same way,
//! and outlined DASHED in a separate overlay (`#shared-outline`), with a note
//! naming the PROTO and the count. An unprovable copy is counted, never drawn.
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
    /// B3 (LV2): the preview span of the PROTO instance the body copy was
    /// proven through (its bounds are read on exactly that copy).
    pub inst: Option<(u64, u64)>,
}

/// B3 (LV2): the most OTHER instances a secondary outline is proven for.
const MAX_OTHERS: usize = 64;

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
    /// B3 (LV2): the OTHER instances of a selected shared PROTO body node,
    /// each proven like the selected copy, with its cached bounds.
    pub others: Vec<(Proven, Option<(f64, Result<hl::Bounds, String>)>)>,
    /// Other copies that could not be proven (no secondary outline).
    pub others_unproven: u32,
    /// The secondary outline path data last drawn ("" when hidden), and
    /// how many instances it outlines.
    pub shared_path: String,
    pub shared_drawn: usize,
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
        // LV2: after a gizmo commit, the instance Rust carried through it.
        let hint = crate::pick::instance_hint(session, revision)
            .filter(|_| body)
            .map(|h| h.span);
        let r = prove_one(session, revision, gen_seq, gen.hash, &item, span, body, hint).await;
        if !current() {
            return;
        }
        let t = match r {
            Ok(Some(t)) => t,
            Ok(None) => return,
            Err(why) => return refused(&why),
        };
        let mine = t.inst;
        HIGHLIGHT.with_borrow_mut(|h| {
            h.proofs += 1;
            h.target = Some(t);
            h.bounds = None;
            h.drawn.clear();
            h.others.clear();
            h.others_unproven = 0;
            h.shared_drawn = 0;
        });
        note(None);
        // B3 (LV2): the OTHER instances this shared definition renders --
        // an edit changes them too. Each is proven exactly like the
        // selected copy (located by its instance span, chain proven by
        // Rust); an unprovable one is counted, never guessed.
        let Some(mine) = mine else { return };
        let (list, mut unproven) = ipc::body_instances(gen_seq, span.0, span.1);
        let mut others = Vec::new();
        for inst in list.into_iter().filter(|i| *i != mine).take(MAX_OTHERS) {
            match prove_one(session, revision, gen_seq, gen.hash, &item, span, true, Some(inst)).await {
                Ok(Some(o)) => others.push((o, None)),
                _ => unproven += 1,
            }
            if !current() {
                return;
            }
        }
        HIGHLIGHT.with_borrow_mut(|h| {
            h.others = others;
            h.others_unproven = unproven;
        });
    });
}

/// Locate `item` (a body node: the copy of instance `inst`, or of the
/// picked instance) and have Rust prove its runtime chain. `Ok(None)`: the
/// world moved on (stale); `Err` is the refusal reason.
#[allow(clippy::too_many_arguments)]
async fn prove_one(
    session: u64,
    revision: u64,
    gen_seq: u64,
    hash: u64,
    item: &str,
    span: (u64, u64),
    body: bool,
    inst: Option<(u64, u64)>,
) -> Result<Option<Proven>, String> {
    let snapshot = ipc::gizmo_locate(gen_seq, span.0, span.1, body, inst)?;
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
                preview_hash: hash,
                generation: gen_seq,
                item: item.to_string(),
                snapshot,
            },
        },
    )
    .await?;
    match r {
        p::HighlightProveOutcome::Proven {
            revision: rev,
            item: it,
            node_type,
            ancestors,
        } if rev == revision && it == item => {
            let ancestors: Vec<((u64, u64), String, bool)> = ancestors
                .into_iter()
                .map(|a| ((a.preview_from, a.preview_to), a.node_type, a.proto_instance))
                .collect();
            // The instance Rust proved the body chain through: exactly one,
            // and exactly the one asked for, if any was.
            let proven: Vec<(u64, u64)> = ancestors.iter().filter(|a| a.2).map(|a| a.0).collect();
            let found = match (body, proven.as_slice()) {
                (false, []) => None,
                (true, [one]) => Some(*one),
                _ => return Err("proto-instance-not-proven".into()),
            };
            if inst.is_some() && found != inst {
                return Err("proto-instance-changed".into());
            }
            Ok(Some(Proven {
                seq: gen_seq,
                span,
                node_type,
                ancestors,
                body,
                inst: found,
            }))
        }
        p::HighlightProveOutcome::Proven { .. } => Err("proof-for-another-node".into()),
        p::HighlightProveOutcome::Refused { reason, .. } => Err(reason),
        p::HighlightProveOutcome::Stale { .. } => Ok(None),
    }
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
    let b = read_bounds(t);
    HIGHLIGHT.with_borrow_mut(|h| h.bounds = Some((now(), b.clone())));
    b
}

/// One fresh read of `t`'s rendered bounds, checked against its proof.
fn read_bounds(t: &Proven) -> Result<hl::Bounds, String> {
    let spans: Vec<(u64, u64)> = t.ancestors.iter().map(|(s, _, _)| *s).collect();
    ipc::highlight_bounds(t.seq, t.span, &spans, t.body, t.inst).and_then(|mut b| {
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
    })
}

/// B3 (LV2): the secondary outline edges of every OTHER proven instance
/// (bounds re-read like the selected one's). An instance whose outline
/// cannot be drawn now is skipped, never approximated.
fn others_edges() -> Vec<[hl::Edge; 12]> {
    let Some(cam) = ipc::gizmo_camera() else {
        return Vec::new();
    };
    let n = HIGHLIGHT.with_borrow(|h| h.others.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let Some((t, cached)) = HIGHLIGHT.with_borrow(|h| h.others.get(i).cloned()) else {
            break;
        };
        let b = match cached.filter(|(at, _)| !crate::gizmo::dragging() && now() - at < BOUNDS_MS) {
            Some((_, b)) => b,
            None => {
                let b = read_bounds(&t);
                HIGHLIGHT.with_borrow_mut(|h| {
                    if let Some(o) = h.others.get_mut(i) {
                        o.1 = Some((now(), b.clone()));
                    }
                });
                b
            }
        };
        if let Ok(e) = b
            .and_then(|b| hl::world_corners(&b).map_err(String::from))
            .and_then(|c| hl::outline(&cam, &c).map_err(String::from))
        {
            out.push(e);
        }
    }
    out
}

/// Update the secondary (other shared instances) overlay; empty hides it.
fn draw_shared(edges: &[[hl::Edge; 12]]) {
    let Some(svg) = crate::element_by_id::<web_sys::Element>("shared-outline") else {
        return;
    };
    let d = if edges.is_empty() {
        String::new()
    } else {
        let r = svg.get_bounding_client_rect();
        edges
            .iter()
            .map(|e| hl::svg_path(e, r.left(), r.top()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    // The words beside it: which PROTO, how many other instances change.
    let note = HIGHLIGHT.with_borrow(|h| {
        let proto = h
            .target
            .as_ref()
            .and_then(|t| t.ancestors.iter().find(|a| a.2).map(|a| a.1.clone()))
            .unwrap_or_default();
        let n = h.others.len() as u32 + h.others_unproven;
        let hidden = n as usize - edges.len().min(n as usize);
        format!(
            "PROTO {proto}: editing it also changes {n} other instance{} (dashed){}",
            if n == 1 { "" } else { "s" },
            if hidden > 0 { format!("; {hidden} not outlined") } else { String::new() }
        )
    });
    let d = if d.is_empty() { d } else { format!("{d}\u{0}{note}") };
    let (same, count_same) =
        HIGHLIGHT.with_borrow(|h| (h.shared_path == d, h.shared_drawn == edges.len()));
    if same && count_same {
        return;
    }
    HIGHLIGHT.with_borrow_mut(|h| {
        h.shared_path = d.clone();
        h.shared_drawn = edges.len();
    });
    let _ = svg.set_attribute("data-count", &edges.len().to_string());
    let note_el = crate::element_by_id::<web_sys::Element>("shared-outline-note");
    if d.is_empty() {
        let _ = svg.set_attribute("data-state", "hidden");
        if let Some(e) = &note_el {
            let _ = e.set_attribute("hidden", "");
        }
        return;
    }
    let path = d.split('\u{0}').next().unwrap_or_default();
    for id in ["hl-shared-halo", "hl-shared-line"] {
        if let Some(e) = crate::element_by_id::<web_sys::Element>(id) {
            let _ = e.set_attribute("d", path);
        }
    }
    if let Some(e) = &note_el {
        e.set_text_content(Some(&note));
        let _ = e.remove_attribute("hidden");
    }
    let _ = svg.set_attribute("data-state", "shown");
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
            h.others.clear();
            h.others_unproven = 0;
        });
        note(None);
        draw(None);
        draw_shared(&[]);
        if let Some(k) = key.clone() {
            if CORE.with_borrow(|c| c.revision == k.2 && !c.busy) {
                request(k);
            }
        }
    }
    let Some(t) = HIGHLIGHT.with_borrow(|h| h.target.clone()) else {
        draw(None);
        draw_shared(&[]);
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
            draw_shared(&others_edges());
        }
        Err(why) => {
            draw(None);
            draw_shared(&[]);
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
                h.others.clear();
            });
            note(None);
            draw(None);
            draw_shared(&[]);
        }
    });
    FRAME.with_borrow_mut(|f| *f = Some(cb));
    if ui().selected.get_untracked().is_some() {
        start();
    }
}
