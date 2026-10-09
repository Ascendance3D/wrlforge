// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A: the Move tool's translation gizmo.
//!
//! The UI holds no authority here either. Rust decides WHAT may move
//! (`doc_translate_target`: a top-level Transform with an explicit
//! translation and a unique DEF) and writes the result (`doc_translate`: one
//! token, one undo step). This module only:
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
//! A drag is bound to the (session, revision, item, generation) it started
//! in; it can never be redirected to another node.

use std::cell::RefCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::gizmo::{self as gz, Axis, Camera};

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
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub target: Target,
    pub axis: Axis,
    /// The camera at the press; the view cannot change during a drag.
    cam: Camera,
    pointer: i32,
    /// Axis parameter under the pointer at the press.
    t_start: f64,
    /// The current value of the dragged component.
    pub value: f64,
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

/// (session, item, revision, generation): what a target was asked for.
type Key = (u64, String, u64, u64);

#[derive(Default)]
pub struct GizmoState {
    pub target: Option<Target>,
    key: Option<Key>,
    req_seq: u64,
    pub drag: Option<Drag>,
    /// The newest pointer position not yet applied, and its event time.
    pending: Option<(f64, f64, f64)>,
    pub committing: bool,
    /// The last drawn layout (client px) and its camera.
    pub layout: Option<gz::Layout>,
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
        release_capture(d.axis, d.pointer);
        msg(
            "refused",
            format!("Move canceled ({reason}); the source is unchanged."),
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
        g.drawn.clear();
    });
    ui().gizmo_message.set(None);
}

fn handle_el(axis: Axis) -> Option<web_sys::Element> {
    crate::element_by_id(&format!("gz-{}", axis.label().to_lowercase()))
}

fn release_capture(axis: Axis, pointer: i32) {
    if let Some(el) = handle_el(axis) {
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
    (sel.session == session && gen.session == session).then_some((
        session,
        sel.id,
        sel.revision,
        gen.seq,
    ))
}

/// Ask Rust whether the selection may move; bind the preview on success.
fn request_target(key: Key) {
    let seq = GIZMO.with_borrow_mut(|g| {
        g.req_seq += 1;
        g.req_seq
    });
    spawn_local(async move {
        let (session, item, revision, gen_seq) = key.clone();
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
                translation,
                origin,
            }) if rev == revision && it == item => {
                let name = def_name.as_deref().unwrap_or("");
                let def_name = def_name.clone().unwrap_or_else(|| "the Transform".into());
                match ipc::gizmo_bind(gen_seq, root_index, name, translation) {
                    Ok(()) => {
                        let t = Target {
                            session,
                            revision,
                            item,
                            def: def_name.clone(),
                            translation,
                            origin,
                            seq: gen_seq,
                        };
                        let released = GIZMO.with_borrow_mut(|g| {
                            g.target = Some(t);
                            g.ghost = None;
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
                        msg(
                            "ok",
                            format!("Move {def_name}: drag the X, Y or Z handle. Esc cancels; the Inspector takes exact values."),
                        );
                    }
                    Err(why) => {
                        GIZMO.with_borrow_mut(|g| g.ghost = None);
                        msg(
                            "refused",
                            format!("The preview cannot show {def_name} for moving ({why}). Use the Inspector."),
                        );
                    }
                }
            }
            Ok(p::TranslateTargetOutcome::Ready { .. })
            | Ok(p::TranslateTargetOutcome::Stale { .. }) => {}
            Ok(p::TranslateTargetOutcome::Refused { reason, message }) => {
                GIZMO.with_borrow_mut(|g| {
                    g.ghost = None;
                    g.last_refusal = Some(reason.clone());
                });
                msg("refused", format!("{message} [{reason}]"));
            }
            Err(e) => msg("refused", format!("Move unavailable: {e}")),
        }
    });
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
    if !ui().move_mode.get_untracked() {
        return Some("the Move tool was turned off");
    }
    None
}

fn translation_with(t: [f64; 3], axis: Axis, v: f64) -> [f64; 3] {
    let mut o = t;
    o[axis.index()] = v;
    o
}

/// Apply the newest pointer position of the drag (camera frozen at press).
fn drag_to(x: f64, y: f64) {
    let t0 = now();
    let upd = GIZMO.with_borrow_mut(|g| {
        let d = g.drag.as_mut()?;
        let i = d.axis.index();
        match gz::axis_param(&d.cam, d.target.origin, d.axis.unit(), x, y) {
            Ok(t) => {
                let v = d.target.translation[i] + (t - d.t_start);
                if v.is_finite() {
                    d.value = v;
                    d.safe = true;
                    d.moves += 1;
                    return Some(Ok(translation_with(d.target.translation, d.axis, v)));
                }
                d.safe = false;
                Some(Err(gz::refusal::NOT_FINITE))
            }
            Err(why) => {
                d.safe = false;
                Some(Err(why))
            }
        }
    });
    match upd {
        Some(Ok(t)) => {
            ipc::gizmo_set(t);
            GIZMO.with_borrow_mut(|g| g.move_ms.push(now() - t0));
        }
        Some(Err(why)) => msg(
            "refused",
            format!("This pointer position cannot be calculated safely ({why}); releasing here cancels the move."),
        ),
        None => {}
    }
}

/// One animation frame while the Move tool is on.
fn tick() {
    let u = ui();
    if !u.move_mode.get_untracked() || CORE.with_borrow(|c| c.session.is_none()) {
        cancel("the Move tool was turned off");
        draw(None);
        return;
    }
    if let Some(why) = GIZMO.with_borrow(|g| g.drag.as_ref().and_then(drag_invalid)) {
        cancel(why);
    }
    if let Some((x, y, stamp)) = GIZMO.with_borrow_mut(|g| g.pending.take()) {
        drag_to(x, y);
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
                    g.ghost = Some(t.origin);
                }
                g.key = key.clone();
                g.drawn.clear();
            });
            ipc::gizmo_unbind();
            match &key {
                Some(k) if k.2 == CORE.with_borrow(|c| c.revision) => request_target(k.clone()),
                Some(_) => {}
                None => {
                    if u.selected.get_untracked().is_none() {
                        GIZMO.with_borrow_mut(|g| g.ghost = None);
                        msg("none", "Move: select an object (Select tool or Scene Tree) to show its handles.");
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
    let what = match (&target, &drag, cam) {
        (_, Some(d), Some(cam)) => {
            let off = d.value - d.target.translation[d.axis.index()];
            let o = gz::add_scaled(d.target.origin, d.axis.unit(), off);
            gz::layout(&cam, o).map(|l| (l, false, Some(d.axis)))
        }
        (Some(t), None, Some(cam)) => {
            let busy = GIZMO.with_borrow(|g| g.committing);
            gz::layout(&cam, t.origin).map(|l| (l, busy, None))
        }
        (None, None, Some(cam)) => ghost
            .and_then(|o| gz::layout(&cam, o))
            .map(|l| (l, true, None)),
        _ => None,
    };
    let drew = draw(what);
    if drew {
        GIZMO.with_borrow_mut(|g| g.frame_ms.push(now() - t0));
    }
}

/// Update the SVG overlay. `None` hides it. Returns whether the DOM changed.
fn draw(what: Option<(gz::Layout, bool, Option<Axis>)>) -> bool {
    let Some(svg) = crate::element_by_id::<web_sys::Element>("gizmo") else {
        return false;
    };
    let sig = match &what {
        None => "hidden".to_string(),
        Some((l, disabled, active)) => format!(
            "{:.1},{:.1}|{:?}|{}|{:?}",
            l.origin.0,
            l.origin.1,
            l.handles.map(|h| (
                h.tip.map(|t| ((t.0 * 10.0).round(), (t.1 * 10.0).round())),
                h.enabled
            )),
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
        g.layout = what.as_ref().map(|w| w.0);
    });
    let Some((l, disabled, active)) = what else {
        let _ = svg.set_attribute("data-state", "hidden");
        return true;
    };
    let r = svg.get_bounding_client_rect();
    let (ox, oy) = (l.origin.0 - r.left(), l.origin.1 - r.top());
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
    let set = |id: &str, k: &str, v: f64| {
        if let Some(e) = crate::element_by_id::<web_sys::Element>(id) {
            let _ = e.set_attribute(k, &format!("{v:.2}"));
        }
    };
    set("gz-origin", "cx", ox);
    set("gz-origin", "cy", oy);
    for h in l.handles {
        let a = h.axis.label().to_lowercase();
        let Some(g) = handle_el(h.axis) else { continue };
        let usable = h.enabled && !disabled && (active.is_none() || active == Some(h.axis));
        let mut cls = String::from("gz-handle");
        if !usable {
            cls.push_str(" disabled");
        }
        if active == Some(h.axis) {
            cls.push_str(" active");
        }
        let _ = g.set_attribute("class", &cls);
        let _ = g.set_attribute("aria-disabled", if usable { "false" } else { "true" });
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
        let _ = g.set_attribute(
            "data-tip",
            &format!(
                "{:.1},{:.1}",
                h.tip.map_or(0.0, |t| t.0),
                h.tip.map_or(0.0, |t| t.1)
            ),
        );
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

fn start_drag(axis: Axis, ev: &web_sys::PointerEvent) {
    if ev.button() != 0 || !ev.is_primary() {
        return;
    }
    let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
    let ok = GIZMO.with_borrow(|g| {
        g.drag.is_none()
            && !g.committing
            && g.target.is_some()
            && g.layout.is_some_and(|l| l.handles[axis.index()].enabled)
    });
    let Some(t) = GIZMO.with_borrow(|g| g.target.clone()).filter(|_| ok) else {
        return;
    };
    // A drag starts only on a handle of the target as it is on screen NOW.
    let probe = Drag {
        target: t.clone(),
        axis,
        cam: match ipc::gizmo_camera() {
            Some(c) => c,
            None => {
                return msg(
                    "refused",
                    "The camera cannot be read; the move did not start.",
                )
            }
        },
        pointer: ev.pointer_id(),
        t_start: 0.0,
        value: t.translation[axis.index()],
        safe: true,
        decimals: 6,
        moves: 0,
    };
    if let Some(why) = drag_invalid(&probe) {
        return msg("refused", format!("The move did not start: {why}."));
    }
    let Some(lay) = gz::layout(&probe.cam, t.origin) else {
        return;
    };
    // The press must be on this handle's drawn segment.
    let tip = lay.handles[axis.index()].tip.unwrap_or(lay.origin);
    if gz::segment_distance((x, y), lay.origin, tip) > 12.0 || !lay.handles[axis.index()].enabled {
        return;
    }
    let t_start = match gz::axis_param(&probe.cam, t.origin, axis.unit(), x, y) {
        Ok(v) => v,
        Err(why) => return msg("refused", format!("The move did not start ({why}).")),
    };
    ev.prevent_default();
    ev.stop_propagation();
    if let Some(el) = handle_el(axis) {
        let _ = el.set_pointer_capture(ev.pointer_id());
    }
    GIZMO.with_borrow_mut(|g| {
        g.drag = Some(Drag {
            t_start,
            decimals: gz::decimals_for(lay.units_per_px),
            ..probe
        });
        g.drawn.clear();
    });
    msg(
        "ok",
        format!(
            "Moving {} along {} — release to apply, Esc to cancel.",
            t.def,
            axis.label()
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
    drag_to(ev.client_x() as f64, ev.client_y() as f64);
    let Some(d) = GIZMO.with_borrow_mut(|g| g.drag.take()) else {
        return;
    };
    release_capture(d.axis, d.pointer);
    GIZMO.with_borrow_mut(|g| g.drawn.clear());
    if !d.safe {
        GIZMO.with_borrow_mut(|g| {
            g.cancels += 1;
            g.last_cancel = Some("unsafe position".into());
        });
        ipc::gizmo_restore();
        return msg(
            "refused",
            "Not moved: the release position could not be calculated safely. The source is unchanged.",
        );
    }
    let start = d.target.translation[d.axis.index()];
    if d.moves == 0 || d.value == start {
        GIZMO.with_borrow_mut(|g| g.unchanged += 1);
        ipc::gizmo_restore();
        return msg("none", "No movement; the source is unchanged.");
    }
    spawn_local(commit(d));
}

/// Commit one completed drag through Rust (`doc_translate`).
async fn commit(d: Drag) {
    let released = now();
    GIZMO.with_borrow_mut(|g| g.committing = true);
    let t = d.target.clone();
    #[derive(serde::Serialize)]
    struct A {
        request: p::TranslateRequest,
    }
    let request = p::TranslateRequest {
        session: t.session,
        base_revision: t.revision,
        item: t.item.clone(),
        axis: d.axis,
        value: d.value,
        decimals: d.decimals,
    };
    let r = call::<p::TranslateOutcome>("doc_translate", A { request }).await;
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
                g.ghost = Some(gz::add_scaled(
                    t.origin,
                    d.axis.unit(),
                    d.value - t.translation[d.axis.index()],
                ));
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
            msg(
                "ok",
                format!(
                    "Moved {} along {} to {text} (revision {}).",
                    t.def,
                    d.axis.label(),
                    state.revision
                ),
            );
            ui::flash(&format!(
                "translation {} = {text} (revision {}).",
                d.axis.label(),
                state.revision
            ));
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
            msg(
                "none",
                "No movement after rounding; the source is unchanged.",
            );
        }
        Ok(p::TranslateOutcome::Refused { reason, message }) => {
            ipc::gizmo_restore();
            GIZMO.with_borrow_mut(|g| {
                g.refused += 1;
                g.last_refusal = Some(reason.clone());
            });
            msg("refused", format!("Not moved: {message} [{reason}]"));
        }
        Err(e) => {
            ipc::gizmo_restore();
            msg("refused", format!("Not moved: {e}"));
        }
    }
}

/// Install the frame loop and the handle / window listeners.
pub fn install() {
    let cb = Closure::<dyn FnMut(f64)>::new(|_t: f64| {
        tick();
        let on = ui().move_mode.get_untracked() || GIZMO.with_borrow(|g| g.drag.is_some());
        if on {
            schedule();
        } else {
            GIZMO.with_borrow_mut(|g| g.loop_on = false);
            reset("the Move tool was turned off");
            draw(None);
        }
    });
    FRAME.with_borrow_mut(|f| *f = Some(cb));
    for axis in Axis::ALL {
        let Some(el) = handle_el(axis) else { continue };
        let down =
            Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
                start_drag(axis, &ev)
            });
        let mv = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
            GIZMO.with_borrow_mut(|g| {
                if g.drag
                    .as_ref()
                    .is_some_and(|d| d.pointer == ev.pointer_id())
                {
                    g.pending = Some((ev.client_x() as f64, ev.client_y() as f64, ev.time_stamp()));
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
