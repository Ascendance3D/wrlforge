// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A / 4B: the Move, Rotate and Scale tools' gizmo.
//!
//! The UI holds no authority here either. Rust decides WHAT may move
//! (`doc_translate_target`: a top-level Transform with an explicit
//! translation; a DEF, if any, unique and unshared), WHICH rendered object it
//! is (`doc_translate_prove`, VISUAL-3A1: the runtime node X_ITE's parser
//! recorded for the Transform's exact source span, its runtime chain proven
//! link by link -- never a root index, name, value or position) and writes
//! the result (`doc_translate`: one token, one undo step). This module only:
//!
//! * draws three axis handles (SVG over the viewport) at the Transform's
//!   origin, from the renderer's REAL camera (`protocol::gizmo::layout`);
//! * turns a drag that STARTS on a handle into an axis-constrained value
//!   (`protocol::gizmo::axis_param`, camera frozen at the press), shown live
//!   by a temporary rendered translation that never touches the source;
//! * commits once on release, or cancels (restoring the rendered value) on
//!   Escape, lost capture, focus loss, or any change of document, revision,
//!   selection or preview generation.
//!
//! A drag is bound to the (session, revision, item, generation, tool) it
//! started in; it can never be redirected to another node.
//!
//! VISUAL-4B. Rotate draws one ring per PARENT (world) axis around the pivot
//! `T + C`; a drag on a ring turns the object about that axis (`doc_rotate`
//! composes `Rot(axis, delta) · rotation`). Scale draws the three scale axes
//! the object really stretches along (`R · SR · e_i`, from the pivot) plus a
//! uniform square at the pivot; a drag changes one component (or all three)
//! by a factor (`doc_scale`). Both need their field authored: a missing one
//! is refused, with an explicit "Write" action, never written silently.
//! Shift snaps a rotation to 15 degree steps.

use std::cell::RefCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::gizmo::{self as gz, Axis, Camera, Tool};

use crate::editor::{self, now, CORE};
use crate::ipc::{self, call};
use crate::ui::{self, ui};

/// What Rust proved movable, bound to the preview generation `seq`.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub session: u64,
    pub revision: u64,
    pub item: String,
    /// Display name: the DEF name, or "the Transform".
    pub def: String,
    pub translation: [f64; 3],
    pub origin: [f64; 3],
    pub seq: u64,
    /// VISUAL-4B: the tool this target was proven for, and the frame.
    pub tool: Tool,
    pub rotation: [f64; 4],
    pub scale: [f64; 3],
    pub scale_orientation: [f64; 4],
    /// The rotation / scale pivot (`T + C`).
    pub pivot: [f64; 3],
    /// B3: the Transform is part of this shared PROTO definition: the
    /// preview shows the picked instance; a commit asks consent first.
    pub shared: Option<p::SharedProto>,
}

/// Which handle a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// Move along a world axis.
    Axis(Axis),
    /// Rotate about a world axis (VISUAL-4B).
    Ring(Axis),
    /// Scale one component (VISUAL-4B).
    Stretch(Axis),
    /// Scale all three components (VISUAL-4B).
    Uniform,
}

impl Grip {
    pub fn axis(self) -> Option<Axis> {
        match self {
            Grip::Axis(a) | Grip::Ring(a) | Grip::Stretch(a) => Some(a),
            Grip::Uniform => None,
        }
    }
    fn el_id(self) -> String {
        match self {
            Grip::Axis(a) | Grip::Stretch(a) => format!("gz-{}", a.label().to_lowercase()),
            Grip::Ring(a) => format!("gz-r{}", a.label().to_lowercase()),
            Grip::Uniform => "gz-u".into(),
        }
    }
    fn label(self) -> String {
        match self {
            Grip::Axis(a) => format!("along {}", a.label()),
            Grip::Ring(a) => format!("about {}", a.label()),
            Grip::Stretch(a) => format!("along {}", a.label()),
            Grip::Uniform => "uniformly".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub target: Target,
    pub grip: Grip,
    /// The camera at the press; the view cannot change during a drag.
    cam: Camera,
    pointer: i32,
    /// At the press: the axis parameter (Move, Stretch), the ring angle
    /// (Ring) or the pointer's client y (Uniform).
    t_start: f64,
    /// The current value: the dragged translation component (Move), the
    /// accumulated rotation in radians (Ring) or the scale factor (Stretch,
    /// Uniform).
    pub value: f64,
    /// Ring: the last pointer angle (the rotation unwraps past ±π).
    last_angle: f64,
    /// Ring: the world radius at the press. Stretch: the handle direction.
    radius: f64,
    dir: [f64; 3],
    /// Ring: the unsnapped accumulated angle.
    raw: f64,
    /// The last pointer position could be calculated safely.
    pub safe: bool,
    decimals: u8,
    pub moves: u32,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct CommitTiming {
    /// `doc_translate` round trip (ms).
    pub commit_ms: f64,
    /// Release → Inspector shows the new revision (ms), when measured.
    pub inspector_ms: Option<f64>,
    /// Release → a new preview generation shows the new revision and the
    /// gizmo is bound again (ms), when measured.
    pub preview_ms: Option<f64>,
}

/// (session, item, revision, generation, tool): what a target was asked for.
type Key = (u64, String, u64, u64, Tool);

#[derive(Default)]
pub struct GizmoState {
    pub target: Option<Target>,
    key: Option<Key>,
    req_seq: u64,
    pub drag: Option<Drag>,
    /// The newest pointer position not yet applied, its event time and
    /// whether Shift was held.
    pending: Option<(f64, f64, f64, bool)>,
    pub committing: bool,
    /// The last drawn handle layout (Move, Scale; client px).
    pub layout: Option<gz::Layout>,
    /// The last drawn rings (Rotate).
    pub rings: Option<gz::RingLayout>,
    /// The field the last refusal said is missing (`translation` /
    /// `rotation` / `scale`):
    /// the gizmo status offers an explicit "Write" for it.
    pub missing_field: Option<&'static str>,
    /// The origin last drawn for a target that is now being replaced: shown
    /// DISABLED until the new revision's target arrives.
    ghost: Option<[f64; 3]>,
    drawn: String,
    loop_on: bool,
    // ---- test / performance counters --------------------------------------
    pub commits: u64,
    pub cancels: u64,
    pub refused: u64,
    pub unchanged: u64,
    pub binds: u64,
    /// Targets Rust accepted whose runtime node could not be proven/bound.
    pub unproven: u64,
    pub last_unproven: Option<String>,
    /// Per bind: locate + Rust proof round trip (ms).
    pub prove_ms: Vec<f64>,
    /// Per bind: the adapter bind with its consistency assertions (ms).
    pub bind_ms: Vec<f64>,
    pub last_cancel: Option<String>,
    pub last_refusal: Option<String>,
    /// Per pointer move: math + temporary translation (ms).
    pub move_ms: Vec<f64>,
    /// Per applied pointer move: event time → temporary translation set (ms).
    pub latency_ms: Vec<f64>,
    /// Per drawn frame: camera read + layout + DOM update (ms).
    pub frame_ms: Vec<f64>,
    pub last_commit: Option<CommitTiming>,
    /// Release time of the newest commit, until its preview is bound again.
    released_at: Option<(f64, u64)>,
}

type FrameCallback = Closure<dyn FnMut(f64)>;

thread_local! {
    pub static GIZMO: RefCell<GizmoState> = RefCell::new(GizmoState::default());
    static FRAME: RefCell<Option<FrameCallback>> = const { RefCell::new(None) };
}

fn msg(kind: &str, text: impl Into<String>) {
    ui().gizmo_message.set(Some((kind.into(), text.into())));
}

/// Cancel an active drag: the rendered translation is restored and nothing
/// is committed. No-op without a drag.
pub fn cancel(reason: &str) {
    let had = GIZMO.with_borrow_mut(|g| {
        g.pending = None;
        let d = g.drag.take();
        if d.is_some() {
            g.cancels += 1;
            g.last_cancel = Some(reason.to_string());
        }
        d
    });
    if let Some(d) = had {
        ipc::gizmo_restore();
        release_capture(d.grip, d.pointer);
        msg(
            "refused",
            format!(
                "{} canceled ({reason}); the source is unchanged.",
                d.target.tool.label()
            ),
        );
        GIZMO.with_borrow_mut(|g| g.drawn.clear());
    }
}

/// The document was replaced or closed: cancel, unbind, forget the target.
pub fn reset(reason: &str) {
    cancel(reason);
    ipc::gizmo_unbind();
    GIZMO.with_borrow_mut(|g| {
        g.target = None;
        g.key = None;
        g.ghost = None;
        g.layout = None;
        g.rings = None;
        g.missing_field = None;
        g.drawn.clear();
    });
    ui().gizmo_message.set(None);
}

/// B3: a viewport pick landed (maybe on the same item, another PROTO
/// instance): re-ask and re-bind on the next idle frame. Never mid-drag.
pub fn repicked() {
    GIZMO.with_borrow_mut(|g| {
        if g.drag.is_none() && !g.committing {
            g.key = None;
        }
    });
}

/// UI-MENU-1: a handle drag is in progress (application shortcuts wait).
pub fn dragging() -> bool {
    GIZMO.with_borrow(|g| g.drag.is_some())
}

/// The active viewport manipulation tool, if any.
pub fn active_tool() -> Option<Tool> {
    let u = ui();
    if u.move_mode.get_untracked() {
        Some(Tool::Move)
    } else if u.rotate_mode.get_untracked() {
        Some(Tool::Rotate)
    } else if u.scale_mode.get_untracked() {
        Some(Tool::Scale)
    } else {
        None
    }
}

fn handle_el(grip: Grip) -> Option<web_sys::Element> {
    crate::element_by_id(&grip.el_id())
}

fn release_capture(grip: Grip, pointer: i32) {
    if let Some(el) = handle_el(grip) {
        if el.has_pointer_capture(pointer) {
            let _ = el.release_pointer_capture(pointer);
        }
    }
}

/// The current (session, selection, active generation) key, if a target
/// could be asked for at all.
fn current_key() -> Option<Key> {
    let session = CORE.with_borrow(|c| c.session)?;
    let sel = ui().selected.get_untracked()?;
    let gen = crate::pick::active()?;
    let tool = active_tool()?;
    (sel.session == session && gen.session == session).then_some((
        session,
        sel.id,
        sel.revision,
        gen.seq,
        tool,
    ))
}

/// Ask Rust whether the selection may move; bind the preview on success.
fn request_target(key: Key) {
    let seq = GIZMO.with_borrow_mut(|g| {
        g.req_seq += 1;
        g.req_seq
    });
    spawn_local(async move {
        let (session, item, revision, gen_seq, tool) = key.clone();
        let ready = || {
            CORE.with_borrow(|c| c.session == Some(session) && c.revision == revision && !c.busy)
                && crate::pick::active().is_some_and(|g| g.seq == gen_seq && g.revision == revision)
        };
        if !ready() {
            return;
        }
        #[derive(serde::Serialize)]
        struct A {
            request: p::TranslateTargetRequest,
        }
        let r = call::<p::TranslateTargetOutcome>(
            "doc_translate_target",
            A {
                request: p::TranslateTargetRequest {
                    session,
                    revision,
                    item: item.clone(),
                    tool,
                },
            },
        )
        .await;
        // Only the newest request, for the key still current, may land.
        let current = GIZMO.with_borrow(|g| g.req_seq == seq && g.key.as_ref() == Some(&key));
        if !current || !ready() || current_key().as_ref() != Some(&key) {
            return;
        }
        match r {
            Ok(p::TranslateTargetOutcome::Ready {
                revision: rev,
                item: it,
                def_name,
                root_index,
                preview_from,
                preview_to,
                translation,
                origin,
                rotation,
                scale,
                scale_orientation,
                pivot,
                shared,
                ..
            }) if rev == revision && it == item => {
                let body = shared.is_some();
                // A body node's DEF lives in the PROTO's own scope, not the
                // scene's: no name assertion then.
                let name = if body {
                    ""
                } else {
                    def_name.as_deref().unwrap_or("")
                };
                let def_name = def_name.clone().unwrap_or_else(|| "the Transform".into());
                let span = (preview_from, preview_to);
                let t0 = now();
                let proof = prove(session, revision, gen_seq, &item, span, tool, body).await;
                let prove_ms = now() - t0;
                // The world may have moved on while Rust proved it.
                let current =
                    GIZMO.with_borrow(|g| g.req_seq == seq && g.key.as_ref() == Some(&key));
                if !current || !ready() || current_key().as_ref() != Some(&key) {
                    return;
                }
                let t1 = now();
                let bound = match proof {
                    Ok(()) => ipc::gizmo_bind(gen_seq, span, root_index, name, translation)
                        .and_then(|()| frame_agrees(rotation, scale)),
                    Err(why) => Err(why),
                };
                let bind_ms = now() - t1;
                GIZMO.with_borrow_mut(|g| {
                    g.prove_ms.push(prove_ms);
                    g.bind_ms.push(bind_ms);
                });
                match bound {
                    Ok(()) => {
                        let t = Target {
                            session,
                            revision,
                            item,
                            def: def_name.clone(),
                            translation,
                            origin,
                            seq: gen_seq,
                            tool,
                            rotation,
                            scale,
                            scale_orientation,
                            pivot,
                            shared: shared.clone(),
                        };
                        let released = GIZMO.with_borrow_mut(|g| {
                            g.target = Some(t);
                            g.ghost = None;
                            g.missing_field = None;
                            g.binds += 1;
                            g.drawn.clear();
                            g.released_at.take()
                        });
                        if let Some((t0, rev_after)) = released {
                            if rev_after == revision {
                                GIZMO.with_borrow_mut(|g| {
                                    if let Some(c) = g.last_commit.as_mut() {
                                        c.preview_ms = Some(now() - t0);
                                    }
                                });
                            }
                        }
                        let in_proto = shared
                            .as_ref()
                            .map(|sh| format!(" [shared PROTO {}: the drag previews the picked instance; a change edits all {}]", sh.proto, sh.instances))
                            .unwrap_or_default();
                        msg(
                            "ok",
                            match tool {
                                Tool::Move => format!("Move {def_name}: drag the X, Y or Z handle. Esc cancels; the Inspector takes exact values."),
                                Tool::Rotate => format!("Rotate {def_name}: drag the X, Y or Z ring (world axes, about the center). Shift snaps to 15°. Esc cancels."),
                                Tool::Scale => format!("Scale {def_name}: drag an X, Y or Z handle (the object's scale axes), or the center square for uniform scale. Esc cancels."),
                            } + &in_proto,
                        );
                    }
                    Err(why) => {
                        ipc::gizmo_unbind();
                        GIZMO.with_borrow_mut(|g| {
                            g.ghost = None;
                            g.unproven += 1;
                            g.last_unproven = Some(why.clone());
                        });
                        msg(
                            "refused",
                            match (&shared, why.as_str()) {
                                (Some(sh), "proto-instance-not-picked") => format!(
                                    "PROTO {} has several instances on screen: click the instance to change in the viewport first (Select tool), then use the handles. Nothing was changed. [{why}]",
                                    sh.proto
                                ),
                                (Some(sh), _) => format!(
                                    "The preview cannot prove which rendered instance of PROTO {} shows {def_name}, so the handles are off ({why}). Use the Inspector.",
                                    sh.proto
                                ),
                                (None, _) => format!("The preview cannot show {def_name} for this tool ({why}). Use the Inspector."),
                            },
                        );
                    }
                }
            }
            Ok(p::TranslateTargetOutcome::Ready { .. })
            | Ok(p::TranslateTargetOutcome::Stale { .. }) => {}
            Ok(p::TranslateTargetOutcome::Refused { reason, message }) => {
                let missing = match reason.as_str() {
                    "translation-not-explicitly-authored" => Some("translation"),
                    "rotation-not-explicitly-authored" => Some("rotation"),
                    "scale-not-explicitly-authored" => Some("scale"),
                    _ => None,
                };
                GIZMO.with_borrow_mut(|g| {
                    g.ghost = None;
                    g.last_refusal = Some(reason.clone());
                    g.missing_field = missing;
                });
                msg("refused", format!("{message} [{reason}]"));
            }
            Err(e) => msg("refused", format!("{} unavailable: {e}", tool.label())),
        }
    });
}

/// VISUAL-4B secondary assertion: the bound node renders the source's
/// rotation and scale. Disagreement refuses; agreement proves nothing.
fn frame_agrees(rotation: [f64; 4], scale: [f64; 3]) -> Result<(), String> {
    let Some(f) = ipc::gizmo_frame() else {
        return Err("preview-frame-unreadable".into());
    };
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-5 + b.abs() * 1e-5;
    if !gz::same_rotation(f.rotation, rotation, 1e-5) {
        return Err("preview-rotation-differs-from-source".into());
    }
    if !(0..3).all(|i| close(f.scale[i], scale[i])) {
        return Err("preview-scale-differs-from-source".into());
    }
    Ok(())
}

/// VISUAL-3A1: locate the runtime node the generation on screen recorded for
/// `span` and have Rust prove it is the authored Transform `item`. `Err` is
/// the refusal reason; nothing binds then.
async fn prove(
    session: u64,
    revision: u64,
    gen_seq: u64,
    item: &str,
    span: (u64, u64),
    tool: Tool,
    body: bool,
) -> Result<(), String> {
    let Some(gen) = crate::pick::active().filter(|g| g.seq == gen_seq) else {
        return Err("preview-scene-replaced".into());
    };
    let snapshot = ipc::gizmo_locate(gen_seq, span.0, span.1, body)?;
    #[derive(serde::Serialize)]
    struct A {
        request: p::TranslateProveRequest,
    }
    let r = call::<p::TranslateProveOutcome>(
        "doc_translate_prove",
        A {
            request: p::TranslateProveRequest {
                session,
                revision,
                preview_hash: gen.hash,
                generation: gen_seq,
                item: item.to_string(),
                snapshot,
                tool,
            },
        },
    )
    .await?;
    match r {
        p::TranslateProveOutcome::Proven {
            revision: rev,
            item: it,
        } if rev == revision && it == item => Ok(()),
        p::TranslateProveOutcome::Proven { .. } => Err("proof-for-another-node".into()),
        p::TranslateProveOutcome::Refused { reason, .. } => Err(reason),
        p::TranslateProveOutcome::Stale { .. } => Err("source-changed-since-preview".into()),
    }
}

/// Whether the active drag still belongs to what is on screen. Returns the
/// reason it does not.
fn drag_invalid(d: &Drag) -> Option<&'static str> {
    let t = &d.target;
    let (s, r, busy) = CORE.with_borrow(|c| (c.session, c.revision, c.busy));
    if s != Some(t.session) {
        return Some("the document was replaced");
    }
    if r != t.revision || busy {
        return Some("the source changed");
    }
    let sel = ui().selected.get_untracked();
    if sel.as_ref().map(|s| (s.session, s.revision, s.id.as_str()))
        != Some((t.session, t.revision, t.item.as_str()))
    {
        return Some("the selection changed");
    }
    if crate::pick::active().map(|g| g.seq) != Some(t.seq) {
        return Some("the preview was reloaded");
    }
    if active_tool() != Some(t.tool) {
        return Some("the tool was turned off");
    }
    None
}

fn translation_with(t: [f64; 3], axis: Axis, v: f64) -> [f64; 3] {
    let mut o = t;
    o[axis.index()] = v;
    o
}

/// The scale after `factor` on `grip` (one component or all three).
fn scaled(s: [f64; 3], grip: Grip, factor: f64) -> [f64; 3] {
    match grip {
        Grip::Stretch(a) => {
            let mut o = s;
            o[a.index()] *= factor;
            o
        }
        _ => s.map(|v| v * factor),
    }
}

/// 15 degrees.
const SNAP: f64 = std::f64::consts::PI / 12.0;

/// Apply the newest pointer position of the drag (camera frozen at press).
fn drag_to(x: f64, y: f64, shift: bool) {
    let t0 = now();
    // (field, values) to show, or the reason the position is unsafe.
    let upd = GIZMO.with_borrow_mut(|g| {
        let d = g.drag.as_mut()?;
        let t = &d.target;
        let r: Result<(&'static str, Vec<f64>), &'static str> = match d.grip {
            Grip::Axis(axis) => {
                let i = axis.index();
                gz::axis_param(&d.cam, t.origin, axis.unit(), x, y).and_then(|p| {
                    let v = t.translation[i] + (p - d.t_start);
                    if !v.is_finite() {
                        return Err(gz::refusal::NOT_FINITE);
                    }
                    d.value = v;
                    Ok((
                        "translation",
                        translation_with(t.translation, axis, v).to_vec(),
                    ))
                })
            }
            Grip::Ring(axis) => {
                gz::ring_angle(&d.cam, t.pivot, axis, d.radius, x, y).and_then(|a| {
                    d.raw += gz::angle_step(d.last_angle, a);
                    d.last_angle = a;
                    let v = if shift {
                        (d.raw / SNAP).round() * SNAP
                    } else {
                        d.raw
                    };
                    d.value = v;
                    gz::rotate_about(t.rotation, axis, v)
                        .map(|r| ("rotation", r.to_vec()))
                        .ok_or(gz::refusal::NOT_FINITE)
                })
            }
            Grip::Stretch(_) => {
                gz::scale_factor(&d.cam, t.pivot, d.dir, d.t_start, x, y).map(|f| {
                    d.value = f;
                    ("scale", scaled(t.scale, d.grip, f).to_vec())
                })
            }
            Grip::Uniform => {
                let f = gz::uniform_factor(d.t_start, y);
                if f.is_finite() && f >= gz::MIN_FACTOR {
                    d.value = f;
                    Ok(("scale", scaled(t.scale, d.grip, f).to_vec()))
                } else {
                    Err(gz::refusal::COLLAPSE)
                }
            }
        };
        match r {
            Ok(v) => {
                d.safe = true;
                d.moves += 1;
                Some(Ok(v))
            }
            Err(why) => {
                d.safe = false;
                Some(Err(why))
            }
        }
    });
    match upd {
        Some(Ok((field, values))) => {
            ipc::gizmo_set_field(field, &values);
            GIZMO.with_borrow_mut(|g| g.move_ms.push(now() - t0));
        }
        Some(Err(why)) => msg(
            "refused",
            format!("This pointer position cannot be calculated safely ({why}); releasing here cancels."),
        ),
        None => {}
    }
}

/// One animation frame while a manipulation tool is on.
fn tick() {
    let u = ui();
    if active_tool().is_none() || CORE.with_borrow(|c| c.session.is_none()) {
        cancel("the tool was turned off");
        draw(None);
        return;
    }
    if let Some(why) = GIZMO.with_borrow(|g| g.drag.as_ref().and_then(drag_invalid)) {
        cancel(why);
    }
    if let Some((x, y, stamp, shift)) = GIZMO.with_borrow_mut(|g| g.pending.take()) {
        drag_to(x, y, shift);
        GIZMO.with_borrow_mut(|g| g.latency_ms.push(now() - stamp));
    }
    // Target lifecycle (never while dragging or committing).
    let idle = GIZMO.with_borrow(|g| g.drag.is_none() && !g.committing);
    if idle {
        let key = current_key();
        let changed = GIZMO.with_borrow(|g| g.key != key);
        if changed {
            GIZMO.with_borrow_mut(|g| {
                if let Some(t) = g.target.take() {
                    g.ghost = Some(if t.tool == Tool::Move {
                        t.origin
                    } else {
                        t.pivot
                    });
                }
                g.key = key.clone();
                g.missing_field = None;
                g.drawn.clear();
            });
            ipc::gizmo_unbind();
            match &key {
                Some(k) if k.2 == CORE.with_borrow(|c| c.revision) => request_target(k.clone()),
                Some(_) => {}
                None => {
                    if u.selected.get_untracked().is_none() {
                        GIZMO.with_borrow_mut(|g| g.ghost = None);
                        msg(
                            "none",
                            "Select an object (Select tool or Scene Tree) to show its handles.",
                        );
                    }
                }
            }
        }
    }
    let t0 = now();
    let (target, drag, ghost) = GIZMO.with_borrow(|g| (g.target.clone(), g.drag.clone(), g.ghost));
    let cam = match &drag {
        Some(d) => Some(d.cam),
        None => ipc::gizmo_camera(),
    };
    let tool = active_tool().unwrap_or(Tool::Move);
    let what = match (&target, &drag, cam) {
        (_, Some(d), Some(cam)) => view_of(&cam, &d.target, Some(d), false),
        (Some(t), None, Some(cam)) => {
            let busy = GIZMO.with_borrow(|g| g.committing);
            view_of(&cam, t, None, busy)
        }
        (None, None, Some(cam)) => ghost.and_then(|o| ghost_view(&cam, tool, o)),
        _ => None,
    };
    let drew = draw(what);
    if drew {
        GIZMO.with_borrow_mut(|g| g.frame_ms.push(now() - t0));
    }
}

/// What the overlay shows.
pub enum View {
    /// Move / Scale handles. `uniform` draws the Scale center square.
    Handles {
        layout: gz::Layout,
        tool: Tool,
        disabled: bool,
        active: Option<Grip>,
    },
    /// Rotate rings.
    Rings {
        rings: gz::RingLayout,
        disabled: bool,
        active: Option<Grip>,
    },
}

fn scale_dirs(t: &Target) -> Option<[[f64; 3]; 3]> {
    gz::scale_axes(t.rotation, t.scale_orientation)
}

fn view_of(cam: &Camera, t: &Target, d: Option<&Drag>, busy: bool) -> Option<View> {
    let active = d.map(|d| d.grip);
    match t.tool {
        Tool::Move => {
            let o = match d {
                Some(d) => {
                    let axis = d.grip.axis()?;
                    let off = d.value - t.translation[axis.index()];
                    gz::add_scaled(t.origin, axis.unit(), off)
                }
                None => t.origin,
            };
            gz::layout(cam, o).map(|layout| View::Handles {
                layout,
                tool: Tool::Move,
                disabled: busy,
                active,
            })
        }
        Tool::Rotate => gz::ring_layout(cam, t.pivot).map(|rings| View::Rings {
            rings,
            disabled: busy,
            active,
        }),
        Tool::Scale => {
            let dirs = scale_dirs(t)?;
            let mut layout = gz::layout_dirs(cam, t.pivot, dirs)?;
            // The dragged handle (or all three) shows its factor.
            if let Some(d) = d {
                for h in layout.handles.iter_mut() {
                    let stretched = match d.grip {
                        Grip::Stretch(a) => a == h.axis,
                        Grip::Uniform => true,
                        _ => false,
                    };
                    if stretched {
                        h.tip = gz::project(
                            cam,
                            gz::add_scaled(t.pivot, dirs[h.axis.index()], layout.length * d.value),
                        );
                    }
                }
            }
            Some(View::Handles {
                layout,
                tool: Tool::Scale,
                disabled: busy,
                active,
            })
        }
    }
}

/// The disabled overlay drawn while a replaced target is re-proven.
fn ghost_view(cam: &Camera, tool: Tool, o: [f64; 3]) -> Option<View> {
    match tool {
        Tool::Rotate => gz::ring_layout(cam, o).map(|rings| View::Rings {
            rings,
            disabled: true,
            active: None,
        }),
        _ => gz::layout(cam, o).map(|layout| View::Handles {
            layout,
            tool,
            disabled: true,
            active: None,
        }),
    }
}

fn tool_attr(t: Tool) -> &'static str {
    match t {
        Tool::Move => "move",
        Tool::Rotate => "rotate",
        Tool::Scale => "scale",
    }
}

/// Update the SVG overlay. `None` hides it. Returns whether the DOM changed.
fn draw(what: Option<View>) -> bool {
    let Some(svg) = crate::element_by_id::<web_sys::Element>("gizmo") else {
        return false;
    };
    let q = |v: f64| (v * 10.0).round();
    let sig = match &what {
        None => "hidden".to_string(),
        Some(View::Handles {
            layout: l,
            tool,
            disabled,
            active,
        }) => format!(
            "h{:?}|{:.1},{:.1}|{:?}|{}|{:?}",
            tool,
            l.origin.0,
            l.origin.1,
            l.handles
                .map(|h| (h.tip.map(|t| (q(t.0), q(t.1))), h.enabled)),
            disabled,
            active
        ),
        Some(View::Rings {
            rings,
            disabled,
            active,
        }) => format!(
            "r|{:.1},{:.1}|{:?}|{}|{:?}",
            rings.origin.0,
            rings.origin.1,
            rings
                .rings
                .iter()
                .map(|r| (
                    r.enabled,
                    r.points
                        .iter()
                        .step_by(8)
                        .map(|p| p.map(|p| (q(p.0), q(p.1))))
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>(),
            disabled,
            active
        ),
    };
    let same = GIZMO.with_borrow(|g| g.drawn == sig);
    if same {
        return false;
    }
    GIZMO.with_borrow_mut(|g| {
        g.drawn = sig;
        match &what {
            Some(View::Handles { layout, .. }) => {
                g.layout = Some(*layout);
                g.rings = None;
            }
            Some(View::Rings { rings, .. }) => {
                g.layout = None;
                g.rings = Some(rings.clone());
            }
            None => {
                g.layout = None;
                g.rings = None;
            }
        }
    });
    let Some(what) = what else {
        let _ = svg.set_attribute("data-state", "hidden");
        return true;
    };
    let r = svg.get_bounding_client_rect();
    let set = |id: &str, k: &str, v: f64| {
        if let Some(e) = crate::element_by_id::<web_sys::Element>(id) {
            let _ = e.set_attribute(k, &format!("{v:.2}"));
        }
    };
    let mark = |g: &web_sys::Element, usable: bool, active: bool| {
        let mut cls = g
            .get_attribute("class")
            .unwrap_or_default()
            .split_whitespace()
            .filter(|c| *c != "disabled" && *c != "active")
            .collect::<Vec<_>>()
            .join(" ");
        if !usable {
            cls.push_str(" disabled");
        }
        if active {
            cls.push_str(" active");
        }
        let _ = g.set_attribute("class", &cls);
        let _ = g.set_attribute("aria-disabled", if usable { "false" } else { "true" });
    };
    let (disabled, active, origin, tool) = match &what {
        View::Handles {
            layout,
            tool,
            disabled,
            active,
        } => (*disabled, *active, layout.origin, *tool),
        View::Rings {
            rings,
            disabled,
            active,
        } => (*disabled, *active, rings.origin, Tool::Rotate),
    };
    let (ox, oy) = (origin.0 - r.left(), origin.1 - r.top());
    let _ = svg.set_attribute("data-tool", tool_attr(tool));
    let _ = svg.set_attribute(
        "data-state",
        if disabled {
            "disabled"
        } else if active.is_some() {
            "dragging"
        } else {
            "ready"
        },
    );
    set("gz-origin", "cx", ox);
    set("gz-origin", "cy", oy);
    match &what {
        View::Handles { layout: l, .. } => {
            for h in l.handles {
                let grip = if tool == Tool::Scale {
                    Grip::Stretch(h.axis)
                } else {
                    Grip::Axis(h.axis)
                };
                let a = h.axis.label().to_lowercase();
                let Some(g) = handle_el(grip) else { continue };
                let usable = h.enabled && !disabled && (active.is_none() || active == Some(grip));
                mark(
                    &g,
                    usable,
                    active == Some(grip) || (active == Some(Grip::Uniform)),
                );
                let (tx, ty) = h.tip.map_or((ox, oy), |t| (t.0 - r.left(), t.1 - r.top()));
                for part in ["hit", "halo", "line"] {
                    let id = format!("gz-{a}-{part}");
                    set(&id, "x1", ox);
                    set(&id, "y1", oy);
                    set(&id, "x2", tx);
                    set(&id, "y2", ty);
                }
                set(&format!("gz-{a}-tip"), "cx", tx);
                set(&format!("gz-{a}-tip"), "cy", ty);
                set(&format!("gz-{a}-box"), "x", tx - 6.0);
                set(&format!("gz-{a}-box"), "y", ty - 6.0);
                let _ = g.set_attribute(
                    "data-tip",
                    &format!(
                        "{:.1},{:.1}",
                        h.tip.map_or(0.0, |t| t.0),
                        h.tip.map_or(0.0, |t| t.1)
                    ),
                );
            }
            if let Some(u) = handle_el(Grip::Uniform) {
                let usable = tool == Tool::Scale
                    && !disabled
                    && (active.is_none() || active == Some(Grip::Uniform));
                mark(&u, usable, active == Some(Grip::Uniform));
                for part in ["hit", "box"] {
                    let half = if part == "hit" { 12.0 } else { 7.0 };
                    set(&format!("gz-u-{part}"), "x", ox - half);
                    set(&format!("gz-u-{part}"), "y", oy - half);
                }
                let _ = u.set_attribute("data-tip", &format!("{:.1},{:.1}", origin.0, origin.1));
            }
        }
        View::Rings { rings, .. } => {
            for ring in &rings.rings {
                let grip = Grip::Ring(ring.axis);
                let a = ring.axis.label().to_lowercase();
                let Some(g) = handle_el(grip) else { continue };
                let usable =
                    ring.enabled && !disabled && (active.is_none() || active == Some(grip));
                mark(&g, usable, active == Some(grip));
                let pts = ring
                    .points
                    .iter()
                    .flatten()
                    .map(|p| format!("{:.1},{:.1}", p.0 - r.left(), p.1 - r.top()))
                    .collect::<Vec<_>>()
                    .join(" ");
                for part in ["hit", "halo", "line"] {
                    if let Some(e) =
                        crate::element_by_id::<web_sys::Element>(&format!("gz-r{a}-{part}"))
                    {
                        let _ = e.set_attribute("points", &pts);
                    }
                }
                // A grab point for tests and keyboard-free hints: the ring's
                // first drawn point that is farthest from the pivot.
                let grab = ring
                    .points
                    .iter()
                    .flatten()
                    .max_by(|p, q| {
                        let dp = (p.0 - origin.0).hypot(p.1 - origin.1);
                        let dq = (q.0 - origin.0).hypot(q.1 - origin.1);
                        dp.total_cmp(&dq)
                    })
                    .copied()
                    .unwrap_or(origin);
                let _ = g.set_attribute("data-tip", &format!("{:.1},{:.1}", grab.0, grab.1));
            }
        }
    }
    true
}

fn schedule() {
    FRAME.with_borrow(|f| {
        if let (Some(cb), Some(w)) = (f.as_ref(), web_sys::window()) {
            let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    });
}

/// Turn the frame loop on (Move tool active). Idempotent.
pub fn start() {
    let on = GIZMO.with_borrow_mut(|g| std::mem::replace(&mut g.loop_on, true));
    if !on {
        schedule();
    }
}

fn start_drag(grip: Grip, ev: &web_sys::PointerEvent) {
    if ev.button() != 0 || !ev.is_primary() {
        return;
    }
    let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
    let ok = GIZMO.with_borrow(|g| g.drag.is_none() && !g.committing);
    let Some(t) = GIZMO.with_borrow(|g| g.target.clone()).filter(|_| ok) else {
        return;
    };
    // The handle must belong to the tool the target was proven for.
    let fits = matches!(
        (t.tool, grip),
        (Tool::Move, Grip::Axis(_))
            | (Tool::Rotate, Grip::Ring(_))
            | (Tool::Scale, Grip::Stretch(_) | Grip::Uniform)
    );
    if !fits {
        return;
    }
    // A drag starts only on a handle of the target as it is on screen NOW.
    let Some(cam) = ipc::gizmo_camera() else {
        return msg(
            "refused",
            "The camera cannot be read; the drag did not start.",
        );
    };
    let mut probe = Drag {
        target: t.clone(),
        grip,
        cam,
        pointer: ev.pointer_id(),
        t_start: 0.0,
        value: 0.0,
        last_angle: 0.0,
        radius: 0.0,
        dir: [0.0; 3],
        raw: 0.0,
        safe: true,
        decimals: 6,
        moves: 0,
    };
    if let Some(why) = drag_invalid(&probe) {
        return msg("refused", format!("The drag did not start: {why}."));
    }
    let started: Result<Drag, String> = match grip {
        Grip::Axis(axis) => {
            let Some(lay) = gz::layout(&cam, t.origin) else {
                return;
            };
            let h = lay.handles[axis.index()];
            let tip = h.tip.unwrap_or(lay.origin);
            if gz::segment_distance((x, y), lay.origin, tip) > 12.0 || !h.enabled {
                return;
            }
            gz::axis_param(&cam, t.origin, axis.unit(), x, y)
                .map(|t_start| Drag {
                    t_start,
                    value: t.translation[axis.index()],
                    decimals: gz::decimals_for(lay.units_per_px),
                    ..probe.clone()
                })
                .map_err(String::from)
        }
        Grip::Ring(axis) => {
            let Some(rings) = gz::ring_layout(&cam, t.pivot) else {
                return;
            };
            let ring = &rings.rings[axis.index()];
            if !ring.enabled || gz::polyline_distance((x, y), &ring.points) > 12.0 {
                return;
            }
            gz::ring_angle(&cam, t.pivot, axis, rings.radius, x, y)
                .map(|a| Drag {
                    t_start: a,
                    last_angle: a,
                    radius: rings.radius,
                    ..probe.clone()
                })
                .map_err(String::from)
        }
        Grip::Stretch(axis) => {
            let Some(dirs) = scale_dirs(&t) else { return };
            let Some(lay) = gz::layout_dirs(&cam, t.pivot, dirs) else {
                return;
            };
            let h = lay.handles[axis.index()];
            let tip = h.tip.unwrap_or(lay.origin);
            if gz::segment_distance((x, y), lay.origin, tip) > 12.0 || !h.enabled {
                return;
            }
            let dir = dirs[axis.index()];
            match gz::axis_param(&cam, t.pivot, dir, x, y) {
                // The press must be clearly away from the pivot, or the
                // factor (distance / press distance) is unstable.
                Ok(p) if p >= gz::MIN_ARM * 2.0 * lay.length => Ok(Drag {
                    t_start: p,
                    value: 1.0,
                    dir,
                    ..probe.clone()
                }),
                Ok(_) => Err(gz::refusal::TOO_CLOSE.into()),
                Err(why) => Err(why.into()),
            }
        }
        Grip::Uniform => {
            let o = gz::project(&cam, t.pivot);
            match o {
                Some(o) if (x - o.0).hypot(y - o.1) <= 16.0 => {
                    probe.t_start = y;
                    probe.value = 1.0;
                    Ok(probe.clone())
                }
                _ => return,
            }
        }
    };
    let drag = match started {
        Ok(d) => d,
        Err(why) => return msg("refused", format!("The drag did not start ({why}).")),
    };
    ev.prevent_default();
    ev.stop_propagation();
    if let Some(el) = handle_el(grip) {
        let _ = el.set_pointer_capture(ev.pointer_id());
    }
    GIZMO.with_borrow_mut(|g| {
        g.drag = Some(drag);
        g.drawn.clear();
    });
    let verb = match t.tool {
        Tool::Move => "Moving",
        Tool::Rotate => "Rotating",
        Tool::Scale => "Scaling",
    };
    msg(
        "ok",
        format!(
            "{verb} {} {} — release to apply, Esc to cancel.",
            t.def,
            grip.label()
        ),
    );
}

fn finish_drag(ev: &web_sys::PointerEvent) {
    let mine = GIZMO.with_borrow(|g| g.drag.as_ref().map(|d| d.pointer == ev.pointer_id()));
    if mine != Some(true) {
        return;
    }
    if let Some(why) = GIZMO.with_borrow(|g| g.drag.as_ref().and_then(drag_invalid)) {
        return cancel(why);
    }
    // The release position is the final one.
    GIZMO.with_borrow_mut(|g| g.pending = None);
    drag_to(ev.client_x() as f64, ev.client_y() as f64, ev.shift_key());
    let Some(d) = GIZMO.with_borrow_mut(|g| g.drag.take()) else {
        return;
    };
    release_capture(d.grip, d.pointer);
    GIZMO.with_borrow_mut(|g| g.drawn.clear());
    if !d.safe {
        GIZMO.with_borrow_mut(|g| {
            g.cancels += 1;
            g.last_cancel = Some("unsafe position".into());
        });
        ipc::gizmo_restore();
        return msg(
            "refused",
            "Not changed: the release position could not be calculated safely. The source is unchanged.",
        );
    }
    let start = match d.grip {
        Grip::Axis(a) => d.target.translation[a.index()],
        Grip::Ring(_) => 0.0,
        Grip::Stretch(_) | Grip::Uniform => 1.0,
    };
    if d.moves == 0 || d.value == start {
        GIZMO.with_borrow_mut(|g| g.unchanged += 1);
        ipc::gizmo_restore();
        return msg("none", "No change; the source is unchanged.");
    }
    spawn_local(commit(d));
}

/// Commit one completed drag through Rust (`doc_translate`).
async fn commit(d: Drag) {
    let released = now();
    GIZMO.with_borrow_mut(|g| g.committing = true);
    let t = d.target.clone();
    // B3: a PROTO body Transform is the shared definition. Ask before EVERY
    // commit (the same consent the Inspector asks; nothing is remembered);
    // a refusal restores the view and changes nothing.
    let confirm_shared = match t.shared.clone() {
        Some(sh) => {
            if !ui::confirm_shared(t.session, sh.clone()).await {
                GIZMO.with_borrow_mut(|g| {
                    g.committing = false;
                    g.refused += 1;
                    g.last_refusal = Some("shared-proto-definition-not-confirmed".into());
                });
                ipc::gizmo_restore();
                return msg(
                    "refused",
                    format!(
                        "Not changed: editing the shared definition PROTO {} was not confirmed. The view was restored; the source is unchanged.",
                        sh.proto
                    ),
                );
            }
            true
        }
        None => false,
    };
    #[derive(serde::Serialize)]
    struct A<R> {
        request: R,
    }
    let r = match d.grip {
        Grip::Axis(axis) => {
            let request = p::TranslateRequest {
                confirm_shared,
                session: t.session,
                base_revision: t.revision,
                item: t.item.clone(),
                axis,
                value: d.value,
                decimals: d.decimals,
            };
            call::<p::TranslateOutcome>("doc_translate", A { request }).await
        }
        Grip::Ring(axis) => {
            let request = p::RotateRequest {
                confirm_shared,
                session: t.session,
                base_revision: t.revision,
                item: t.item.clone(),
                axis,
                delta: d.value,
            };
            call::<p::TranslateOutcome>("doc_rotate", A { request }).await
        }
        Grip::Stretch(_) | Grip::Uniform => {
            let request = p::ScaleRequest {
                confirm_shared,
                session: t.session,
                base_revision: t.revision,
                item: t.item.clone(),
                axis: d.grip.axis(),
                factor: d.value,
            };
            call::<p::TranslateOutcome>("doc_scale", A { request }).await
        }
    };
    let commit_ms = now() - released;
    GIZMO.with_borrow_mut(|g| g.committing = false);
    let u = ui();
    match r {
        Ok(p::TranslateOutcome::Applied {
            state,
            view,
            item,
            text,
        }) => {
            // A reply for a document that is no longer shown is not adopted
            // (Rust applied it to its own session; nothing here moves).
            if CORE.with_borrow(|c| c.session) != Some(t.session) {
                return;
            }
            // Keep the moved object drawn until the new revision's scene
            // replaces it (no flash back to the old position).
            ipc::gizmo_release();
            GIZMO.with_borrow_mut(|g| {
                g.commits += 1;
                g.last_commit = Some(CommitTiming {
                    commit_ms,
                    inspector_ms: None,
                    preview_ms: None,
                });
                g.released_at = Some((released, state.revision));
                g.target = None;
                g.ghost = Some(match d.grip {
                    Grip::Axis(a) => {
                        gz::add_scaled(t.origin, a.unit(), d.value - t.translation[a.index()])
                    }
                    _ => t.pivot,
                });
                g.drawn.clear();
            });
            u.field_error.set(None);
            editor::adopt_change(&state, view);
            let sel = ui::Selected {
                session: t.session,
                revision: state.revision,
                id: item,
            };
            u.selected.set(Some(sel.clone()));
            let (verb, field) = match d.grip {
                Grip::Axis(a) => ("Moved", format!("translation {}", a.label())),
                Grip::Ring(_) => ("Rotated", "rotation".into()),
                _ => ("Scaled", "scale".into()),
            };
            msg(
                "ok",
                format!(
                    "{verb} {} {} to {text} (revision {}).",
                    t.def,
                    d.grip.label(),
                    state.revision
                ),
            );
            ui::flash(&format!("{field} = {text} (revision {}).", state.revision));
            let rev = state.revision;
            spawn_local(async move {
                ui::inspect(sel).await;
                if ui()
                    .inspection
                    .with_untracked(|i| i.as_ref().is_some_and(|i| i.revision == rev))
                {
                    GIZMO.with_borrow_mut(|g| {
                        if let Some(c) = g.last_commit.as_mut() {
                            c.inspector_ms = Some(now() - released);
                        }
                    });
                }
            });
            // The final position comes from the new source revision, now.
            ui::preview(false).await;
        }
        Ok(p::TranslateOutcome::Unchanged) => {
            ipc::gizmo_restore();
            GIZMO.with_borrow_mut(|g| g.unchanged += 1);
            msg("none", "No change after rounding; the source is unchanged.");
        }
        Ok(p::TranslateOutcome::Refused { reason, message }) => {
            ipc::gizmo_restore();
            GIZMO.with_borrow_mut(|g| {
                g.refused += 1;
                g.last_refusal = Some(reason.clone());
            });
            msg("refused", format!("Not changed: {message} [{reason}]"));
        }
        Err(e) => {
            ipc::gizmo_restore();
            msg("refused", format!("Not changed: {e}"));
        }
    }
}

/// Install the frame loop and the handle / window listeners.
pub fn install() {
    let cb = Closure::<dyn FnMut(f64)>::new(|_t: f64| {
        tick();
        let on = active_tool().is_some() || GIZMO.with_borrow(|g| g.drag.is_some());
        if on {
            schedule();
        } else {
            GIZMO.with_borrow_mut(|g| g.loop_on = false);
            reset("the tool was turned off");
            draw(None);
        }
    });
    FRAME.with_borrow_mut(|f| *f = Some(cb));
    // The axis groups serve Move and Scale: the active tool decides.
    let mut grips: Vec<(Grip, bool)> = Axis::ALL.iter().map(|a| (Grip::Axis(*a), true)).collect();
    grips.extend(Axis::ALL.iter().map(|a| (Grip::Ring(*a), false)));
    grips.push((Grip::Uniform, false));
    for (grip, shared) in grips {
        let Some(el) = handle_el(grip) else { continue };
        let down =
            Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
                let g = match (shared, grip, active_tool()) {
                    (true, Grip::Axis(a), Some(Tool::Scale)) => Grip::Stretch(a),
                    _ => grip,
                };
                start_drag(g, &ev)
            });
        let mv = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
            GIZMO.with_borrow_mut(|g| {
                if g.drag
                    .as_ref()
                    .is_some_and(|d| d.pointer == ev.pointer_id())
                {
                    g.pending = Some((
                        ev.client_x() as f64,
                        ev.client_y() as f64,
                        ev.time_stamp(),
                        ev.shift_key(),
                    ));
                }
            });
        });
        let up = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
            finish_drag(&ev)
        });
        let pcancel =
            Closure::<dyn Fn(web_sys::PointerEvent)>::new(|_| cancel("the pointer was canceled"));
        let lost = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
            if GIZMO.with_borrow(|g| {
                g.drag
                    .as_ref()
                    .is_some_and(|d| d.pointer == ev.pointer_id())
            }) {
                cancel("pointer capture was lost");
            }
        });
        for (name, f) in [
            ("pointerdown", &down),
            ("pointermove", &mv),
            ("pointerup", &up),
            ("pointercancel", &pcancel),
            ("lostpointercapture", &lost),
        ] {
            let _ = el.add_event_listener_with_callback(name, f.as_ref().unchecked_ref());
        }
        down.forget();
        mv.forget();
        up.forget();
        pcancel.forget();
        lost.forget();
    }
    let Some(w) = web_sys::window() else { return };
    let esc = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(|ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" && GIZMO.with_borrow(|g| g.drag.is_some()) {
            ev.prevent_default();
            ev.stop_propagation();
            cancel("Escape");
        }
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
        "keydown",
        esc.as_ref().unchecked_ref(),
        &opts,
    );
    esc.forget();
    let blur = Closure::<dyn Fn(web_sys::Event)>::new(|_| cancel("the window lost focus"));
    let _ = w.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    blur.forget();
}

/// Smoke / diagnostics: the bound node's rendered translation.
pub fn rendered() -> Option<[f64; 3]> {
    ipc::gizmo_rendered()
}

/// Smoke / diagnostics: the bound node's rendered frame (VISUAL-4B).
pub fn rendered_frame() -> Option<ipc::RenderedFrame> {
    ipc::gizmo_frame()
}

/// The explicit authoring action for a field the tool needs (VISUAL-4B):
/// write the ISO default value through the Inspector's `doc_add_field` (one
/// undo step). Only on the user's request; never automatic.
pub async fn write_missing_field() {
    let Some(field) = GIZMO.with_borrow(|g| g.missing_field) else {
        return;
    };
    let values: Vec<String> = match field {
        "translation" => ["0", "0", "0"].map(String::from).to_vec(),
        "rotation" => ["0", "0", "1", "0"].map(String::from).to_vec(),
        "scale" => ["1", "1", "1"].map(String::from).to_vec(),
        _ => return,
    };
    if ui::add_field(field.to_string(), p::AddFieldValue::Tuple { values }).await {
        GIZMO.with_borrow_mut(|g| {
            g.missing_field = None;
            g.drawn.clear();
        });
    }
}
