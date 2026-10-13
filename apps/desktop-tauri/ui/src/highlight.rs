// SPDX-License-Identifier: GPL-3.0-or-later
//! B2: the selection highlight -- an outline of the selected node's
//! rendered geometry over the viewport (Spazz3D / Flux parity).
//!
//! LV2: the outline follows the geometry. The renderer reports the shapes
//! X_ITE draws for the SAME proven node (its own shape list: matrices and
//! triangle / line vertices); `protocol::highlight::display` draws their
//! boundary, crease and silhouette edges every frame. The bounding box is
//! the fallback only, drawn dashed with the reason in the status note. A
//! non-geometry node (Material, Appearance, a texture, Coordinate, ...) has
//! no bounds of its own: Rust proves its owning Shape in the same chain and
//! that Shape is outlined, dotted and labeled as the owner.
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
    /// LV2: the selected node is not drawn by itself (Material, ...): this
    /// is its owning Shape's preview span, outlined instead (`span` stays
    /// the selected node's own).
    pub owner: Option<(u64, u64)>,
}

impl Proven {
    /// The span whose runtime node is outlined.
    fn drawn_span(&self) -> (u64, u64) {
        self.owner.unwrap_or(self.span)
    }
}

/// B3 (LV2): the most OTHER instances a secondary outline is proven for.
const MAX_OTHERS: usize = 64;

/// LV2: one outlined runtime node's cached reads.
#[derive(Clone, Debug, Default)]
pub struct Reads {
    /// The rendered bounds, re-read at most every `BOUNDS_MS`.
    bounds: Option<(f64, Result<hl::Bounds, String>)>,
    /// The welded edge mesh of the drawn node's shapes, or why there is
    /// none (the box is drawn then). Keyed by the shape list's
    /// fingerprint, so a failure is not retried until the list changes.
    mesh: Option<(Vec<(u8, u32)>, Result<hl::Mesh, String>)>,
}

#[derive(Default)]
pub struct HighlightState {
    key: Option<Key>,
    req_seq: u64,
    pub target: Option<Proven>,
    /// The selected node's cached reads (bounds and edge mesh).
    reads: Reads,
    drawn: String,
    loop_on: bool,
    // ---- test / diagnostics counters --------------------------------------
    pub proofs: u64,
    pub refusals: u64,
    pub last_refusal: Option<String>,
    /// The SVG path data last drawn ("" when hidden).
    pub path: String,
    /// B3 (LV2): the OTHER instances of a selected shared PROTO body node,
    /// each proven like the selected copy, with its own cached reads (LV2:
    /// bounds and edge mesh -- each copy is outlined precisely when it can
    /// be, else by its box).
    pub others: Vec<(Proven, Reads)>,
    /// Other copies that could not be proven (no secondary outline).
    pub others_unproven: u32,
    /// The secondary outline path data last drawn ("" when hidden), and
    /// how many instances it outlines.
    pub shared_path: String,
    pub shared_drawn: usize,
    /// LV2: what the overlay shows: "precise", "box", "owner-precise",
    /// "owner-box" or "" (hidden).
    pub mode: &'static str,
    /// LV2: why the precise outline was not drawn, while the box is.
    pub box_reason: Option<String>,
    /// LV2: the owner note shown while an owner Shape is outlined.
    owner_note: Option<String>,
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

fn info(text: Option<String>) {
    let u = ui();
    if u.highlight_info.get_untracked() != text {
        u.highlight_info.set(text);
    }
}

fn refused(reason: &str) {
    refused_because(reason, None);
}

/// Record a refusal and show its TRUE reason. `detail`: Rust's own
/// sentence, used where it names more than the code can (the ancestor's
/// node type, the never-drawn node's type).
fn refused_because(reason: &str, detail: Option<&str>) {
    HIGHLIGHT.with_borrow_mut(|h| {
        h.refusals += 1;
        h.last_refusal = Some(reason.to_string());
    });
    info(None);
    note(Some(refusal_text(reason, detail)));
}

/// "No outline: <why> [code]".
pub fn refusal_text(reason: &str, detail: Option<&str>) -> String {
    let why = match detail {
        Some(d) if reason == hl::refusal::ANCESTOR || reason == hl::refusal::NOT_RENDERED => {
            lower_first(d.trim_end_matches('.'))
        }
        _ => explain(reason).to_string(),
    };
    format!("No outline: {why} [{reason}]")
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// One short sentence per refusal code (`protocol::highlight`).
fn explain(reason: &str) -> &'static str {
    hl::refusal_message(reason)
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

/// P0-B: the selected node's largest rendered extent in its parent's
/// coordinates, read from the outline's PROVEN target for exactly the
/// current selection (session, item, revision, preview generation). `None`
/// when no outline is proven for it (Duplicate then uses the fixed step).
pub fn selection_extent() -> Option<f64> {
    let key = current_key()?;
    let t = HIGHLIGHT.with_borrow(|h| (h.key.as_ref() == Some(&key)).then(|| h.target.clone()))??;
    // LV2: an owner Shape's box is not the selected node's extent.
    if t.owner.is_some() {
        return None;
    }
    let mut reads = HIGHLIGHT.with_borrow_mut(|h| std::mem::take(&mut h.reads));
    let b = bounds_of(&t, &mut reads);
    HIGHLIGHT.with_borrow_mut(|h| h.reads = reads);
    b.ok().as_ref().and_then(hl::parent_extent)
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
            let ok = HIGHLIGHT.with_borrow(|h| h.req_seq == seq && h.key.as_ref() == Some(&key))
                && current_key().as_ref() == Some(&key)
                && CORE.with_borrow(|c| c.revision == revision && !c.busy);
            // LV2: only BUSY interrupted this (same request, same key):
            // let the next frame ask again rather than leave no outline.
            if !ok
                && CORE.with_borrow(|c| c.busy)
                && HIGHLIGHT.with_borrow(|h| h.req_seq == seq && h.key.as_ref() == Some(&key))
            {
                HIGHLIGHT.with_borrow_mut(|h| h.key = None);
            }
            ok
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
            Ok(p::HighlightTargetOutcome::Refused { reason, message }) => {
                return refused_because(&reason, Some(&message))
            }
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
            Err((why, message)) => return refused_because(&why, message.as_deref()),
        };
        let mine = t.inst;
        // LV2: a node not drawn by itself is shown by its owning Shape.
        let owner_note = t.owner.map(|_| {
            format!(
                "{} is not drawn by itself: the dotted outline is the Shape that uses it{}",
                t.node_type,
                crate::panels::owner_line_note()
            )
        });
        HIGHLIGHT.with_borrow_mut(|h| {
            h.proofs += 1;
            h.target = Some(t);
            h.reads = Reads::default();
            h.drawn.clear();
            h.owner_note = owner_note.clone();
            h.others.clear();
            h.others_unproven = 0;
            h.shared_drawn = 0;
        });
        note(None);
        info(owner_note);
        // B3 (LV2): the OTHER instances this shared definition renders --
        // an edit changes them too. Each is proven exactly like the
        // selected copy (located by its instance span, chain proven by
        // Rust); an unprovable one is counted, never guessed.
        let Some(mine) = mine else { return };
        let (list, mut unproven) = ipc::body_instances(gen_seq, span.0, span.1);
        let mut others = Vec::new();
        for inst in list.into_iter().filter(|i| *i != mine).take(MAX_OTHERS) {
            match prove_one(session, revision, gen_seq, gen.hash, &item, span, true, Some(inst)).await {
                Ok(Some(o)) => others.push((o, Reads::default())),
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
/// world moved on (stale); `Err` is the refusal reason and, when Rust gave
/// one, its own sentence (LV2: the TRUE refusal message).
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
) -> Result<Option<Proven>, (String, Option<String>)> {
    let snapshot = ipc::gizmo_locate(gen_seq, span.0, span.1, body, inst).map_err(|e| (e, None))?;
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
    .await
    .map_err(|e| (e, None))?;
    match r {
        p::HighlightProveOutcome::Proven {
            revision: rev,
            item: it,
            node_type,
            ancestors,
            owner,
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
                _ => return Err(("proto-instance-not-proven".into(), None)),
            };
            if inst.is_some() && found != inst {
                return Err(("proto-instance-changed".into(), None));
            }
            Ok(Some(Proven {
                seq: gen_seq,
                span,
                node_type,
                ancestors,
                body,
                inst: found,
                owner: owner.as_ref().map(|o| (o.preview_from, o.preview_to)),
            }))
        }
        p::HighlightProveOutcome::Proven { .. } => Err(("proof-for-another-node".into(), None)),
        p::HighlightProveOutcome::Refused { reason, message } => Err((reason, Some(message))),
        p::HighlightProveOutcome::Stale { .. } => Ok(None),
    }
}

/// The proven node's bounds, re-read at most every `BOUNDS_MS`. The
/// renderer's ancestor types must equal the proven source types. LV2: the
/// same read carries the node's shape list (counts and matrices); a new
/// list -- or none yet -- reads the vertices once and rebuilds the mesh.
/// `reads` is that node's own cache (the selected copy's, or one OTHER
/// instance's: every copy is read on its own proven runtime node).
fn bounds_of(t: &Proven, reads: &mut Reads) -> Result<hl::Bounds, String> {
    let fresh = reads
        .bounds
        .as_ref()
        // A gizmo drag moves the rendered node every frame: follow it.
        .filter(|(at, _)| !crate::gizmo::dragging() && now() - at < BOUNDS_MS)
        .map(|(_, b)| b.clone());
    if let Some(b) = fresh {
        return b;
    }
    let spans: Vec<(u64, u64)> = t.ancestors.iter().map(|(s, _, _)| *s).collect();
    let agree = |mut b: hl::Bounds| {
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
    };
    let read = |g| ipc::highlight_bounds(t.seq, t.drawn_span(), &spans, t.body, t.inst, g);
    let b = match read(ipc::Geometry::Counts) {
        Ok((b, shapes)) => {
            let fp = match &shapes {
                Some(Ok(s)) => hl::fingerprint(s),
                _ => Vec::new(),
            };
            let have_mesh = match (&mut reads.mesh, &shapes) {
                (Some((f, Ok(m))), Some(Ok(s))) if *f == fp => m.update_matrices(s),
                // A failure stays one until the shape list changes.
                (Some((f, Err(_))), Some(Ok(_))) => *f == fp,
                (Some((_, Err(_))), Some(Err(_))) => true,
                _ => false,
            };
            if !have_mesh {
                let mesh = match shapes {
                    Some(Err(why)) => Err(why),
                    None => Err(hl::refusal::GEOMETRY_UNAVAILABLE.to_string()),
                    Some(Ok(_)) => match read(ipc::Geometry::Vertices) {
                        Ok((_, Some(Ok(full)))) => hl::build_mesh(&full).map_err(String::from),
                        Ok((_, Some(Err(why)))) => Err(why),
                        Ok((_, None)) | Err(_) => Err(hl::refusal::GEOMETRY_UNAVAILABLE.to_string()),
                    },
                };
                reads.mesh = Some((fp, mesh));
            }
            agree(b)
        }
        Err(e) => Err(e),
    };
    reads.bounds = Some((now(), b.clone()));
    b
}

/// LV2: what one proven node's outline is this frame: its geometry's own
/// edges, else its bounding box saying why, else `Err` (nothing drawn).
fn display_of(t: &Proven, reads: &mut Reads) -> Result<hl::Display, String> {
    let b = bounds_of(t, reads)?;
    let cam = ipc::gizmo_camera().ok_or("camera-unavailable")?;
    let mesh = match &reads.mesh {
        Some((_, Ok(m))) => Ok(m),
        Some((_, Err(e))) => Err(e.as_str()),
        None => Err(hl::refusal::GEOMETRY_UNAVAILABLE),
    };
    hl::display(&cam, &b, mesh).map_err(String::from)
}

/// B3 (LV2): the secondary outline edges of every OTHER proven instance,
/// each read on its own runtime copy like the selected one (LV2: precise
/// when its own geometry can be meshed, else its box). An instance whose
/// outline cannot be drawn now is skipped, never approximated.
fn others_edges() -> Vec<Vec<hl::Edge>> {
    let mut list = HIGHLIGHT.with_borrow_mut(|h| std::mem::take(&mut h.others));
    let mut out = Vec::with_capacity(list.len());
    for (t, reads) in list.iter_mut() {
        match display_of(t, reads) {
            Ok(hl::Display::Precise(e)) => out.push(e),
            Ok(hl::Display::Box { edges, .. }) => out.push(edges.to_vec()),
            Err(_) => {}
        }
    }
    HIGHLIGHT.with_borrow_mut(|h| {
        // Put the reads back unless a new proof replaced the list meanwhile.
        if h.others.is_empty() {
            h.others = list;
        }
    });
    out
}

/// Update the secondary (other shared instances) overlay; empty hides it.
fn draw_shared(edges: &[Vec<hl::Edge>]) {
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
            h.owner_note = None;
            h.reads = Reads::default();
            h.others.clear();
            h.others_unproven = 0;
        });
        note(None);
        info(None);
        draw(None);
        draw_shared(&[]);
        if let Some(k) = key.clone() {
            if CORE.with_borrow(|c| c.revision == k.2 && !c.busy) {
                request(k);
            } else {
                // LV2: the document is busy (a Create, an edit in flight):
                // ask again next frame instead of never (the outline was
                // missing after Create).
                HIGHLIGHT.with_borrow_mut(|h| h.key = None);
            }
        }
    }
    let Some(t) = HIGHLIGHT.with_borrow(|h| h.target.clone()) else {
        draw(None);
        draw_shared(&[]);
        return;
    };
    let mut reads = HIGHLIGHT.with_borrow_mut(|h| std::mem::take(&mut h.reads));
    let shown = display_of(&t, &mut reads);
    HIGHLIGHT.with_borrow_mut(|h| h.reads = reads);
    let owned = t.owner.is_some();
    match shown {
        Ok(hl::Display::Precise(edges)) => {
            if ui().highlight_message.get_untracked().is_some() {
                note(None);
            }
            HIGHLIGHT.with_borrow_mut(|h| h.box_reason = None);
            info(HIGHLIGHT.with_borrow(|h| h.owner_note.clone()));
            draw(Some((&edges, if owned { "owner-precise" } else { "precise" })));
            draw_shared(&others_edges());
        }
        Ok(hl::Display::Box { edges, reason }) => {
            let msg = Some(format!(
                "Bounding box shown: {} [{reason}]",
                explain(&reason)
            ));
            if ui().highlight_message.get_untracked() != msg {
                note(msg);
            }
            HIGHLIGHT.with_borrow_mut(|h| h.box_reason = Some(reason));
            info(HIGHLIGHT.with_borrow(|h| h.owner_note.clone()));
            draw(Some((&edges, if owned { "owner-box" } else { "box" })));
            draw_shared(&others_edges());
        }
        Err(why) => {
            draw(None);
            draw_shared(&[]);
            let msg = Some(refusal_text(&why, None));
            if ui().highlight_message.get_untracked() != msg {
                refused(&why);
            }
        }
    }
}

/// Update the overlay; `None` hides it. The mode ("precise", "box",
/// "owner-precise", "owner-box") styles it.
fn draw(edges: Option<(&[hl::Edge], &'static str)>) {
    let Some(svg) = crate::element_by_id::<web_sys::Element>("selection-outline") else {
        return;
    };
    let (d, mode) = match edges {
        Some((e, mode)) => {
            let r = svg.get_bounding_client_rect();
            (hl::svg_path(e, r.left(), r.top()), mode)
        }
        None => (String::new(), ""),
    };
    if HIGHLIGHT.with_borrow(|h| h.drawn == d && h.mode == mode) {
        return;
    }
    HIGHLIGHT.with_borrow_mut(|h| {
        h.drawn = d.clone();
        h.path = d.clone();
        h.mode = mode;
    });
    let _ = svg.set_attribute("data-mode", mode);
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
                h.owner_note = None;
                h.reads = Reads::default();
                h.others.clear();
                h.others_unproven = 0;
            });
            note(None);
            info(None);
            draw(None);
            draw_shared(&[]);
        }
    });
    FRAME.with_borrow_mut(|f| *f = Some(cb));
    // LV2: EVERY way a selection appears starts the loop -- not only
    // `adopt_selection`. A Create (or an Undo / field edit) carries its
    // selection through `editor` / `ui` directly; after New World stopped
    // the loop, a created object was left with no outline.
    Effect::new(move |_| {
        if ui().selected.with(|s| s.is_some()) {
            start();
        }
    });
}
