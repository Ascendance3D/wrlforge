// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-2 viewport selection: the Select mode's click gesture and the pick
//! round trip.
//!
//! The UI holds no identity authority. A click becomes the X_ITE adapter's
//! plain-data snapshot (`preview-adapter.js` -> `xite-pick-adapter.js`); Rust
//! (`doc_pick`) decides what it means. This module only binds the snapshot to
//! the preview generation it came from, refuses anything late or stale, and
//! hands a PROVEN item to the one selection authority (`ui::adopt_selection`).
//!
//! A preview generation is (`session`, `revision`, `hash`, `seq`): the exact
//! text Rust returned for one load, and that load's number. It is retired the
//! moment the document changes, a new preview starts, a load fails, a
//! document opens or closes, or the viewport is replaced.

use std::cell::RefCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;

use crate::editor::{now, CORE};
use crate::ipc::{self, call};
use crate::ui::{self, ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gen {
    pub session: u64,
    pub revision: u64,
    pub hash: u64,
    pub seq: u64,
}

/// A click must not travel further than this between press and release
/// (CSS px); anything longer is a camera drag, never a pick.
const CLICK_SLOP: f64 = 4.0;

#[derive(Default)]
pub struct PickState {
    /// The generation on screen, or `None` (retired / not a document).
    pub active: Option<Gen>,
    /// Bumped per pick request; only the newest reply may land.
    pub seq: u64,
    /// Primary-button press: (pointer id, client x, client y).
    down: Option<(i32, f64, f64)>,
    /// Test counters: replies refused because the preview or document moved
    /// while they were in flight, and replies superseded by a newer pick.
    pub late_refused: u64,
    pub superseded: u64,
    /// The last pick: adapter snapshot time and click-to-result time (ms).
    pub last_snapshot_ms: f64,
    pub last_total_ms: f64,
    pub last: Option<p::PickOutcome>,
    /// Picks finished (applied or superseded).
    pub completed: u64,
    /// Client coordinates of the last click that requested a pick.
    pub last_click: Option<(f64, f64)>,
    /// P0-A: the last applied pick, for "click again to reach the Shape
    /// behind" (see [`next_layer`]).
    pub cycle: Option<Cycle>,
}

/// P0-A hit cycling. The renderer reports only the NEAREST Shape under the
/// pointer, so a nearly transparent coat hides the paint under it. Each
/// viewport pick therefore also lists how many Shapes lie under the pointer
/// (`layers`, front to back, peeled by the X_ITE adapter); clicking AGAIN at
/// the same place selects the next one behind, wrapping to the front. Every
/// layer is resolved by Rust (`doc_pick`) through exactly the same proven
/// binding as a plain pick; a layer without one is reported as unavailable
/// and selects nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Cycle {
    /// The preview generation the pick was made on.
    pub seq: u64,
    pub x: f64,
    pub y: f64,
    /// The layer that pick returned, and how many were found.
    pub layer: u32,
    pub layers: u32,
    /// The selected Scene Tree item after that pick was applied.
    pub selected: Option<String>,
}

/// Which layer a click at (`x`, `y`) asks for: the one behind the previous
/// pick when this click REPEATS it -- same preview generation, within
/// [`CLICK_SLOP`], and the selection still exactly what that pick left
/// (anything selected since, any edit or reload, starts again at the front).
/// Otherwise 0, the nearest Shape: a single click behaves as it always did.
pub fn next_layer(prev: Option<&Cycle>, seq: u64, x: f64, y: f64, selected: Option<&str>) -> u32 {
    match prev {
        Some(c)
            if c.seq == seq
                && c.layers > 1
                && (c.x - x).hypot(c.y - y) <= CLICK_SLOP
                && c.selected.as_deref() == selected =>
        {
            (c.layer + 1) % c.layers
        }
        _ => 0,
    }
}

thread_local! {
    pub static PICK: RefCell<PickState> = RefCell::new(PickState::default());
}

/// What a pick status line describes: the selection, document revision
/// and viewport tool at the moment it was written. Any other state makes
/// the line stale ("Shape 2 of 3" about another object), so it is cleared
/// -- never rewritten to guess what it now means.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageContext {
    session: Option<u64>,
    revision: u64,
    selected: Option<(u64, u64, String)>,
    tools: [bool; 4],
}

thread_local! {
    static MESSAGE_CONTEXT: RefCell<Option<MessageContext>> = const { RefCell::new(None) };
}

/// The context now; `track` subscribes the calling effect to it.
fn message_context(track: bool) -> MessageContext {
    let u = ui();
    let get = |s: RwSignal<bool>| if track { s.get() } else { s.get_untracked() };
    let selected =
        |s: &Option<ui::Selected>| s.as_ref().map(|s| (s.session, s.revision, s.id.clone()));
    let session = |d: &Option<p::DocumentInfo>| d.as_ref().map(|d| d.session);
    MessageContext {
        session: if track {
            u.doc.with(session)
        } else {
            u.doc.with_untracked(session)
        },
        revision: if track {
            u.revision.get()
        } else {
            u.revision.get_untracked()
        },
        selected: if track {
            u.selected.with(selected)
        } else {
            u.selected.with_untracked(selected)
        },
        tools: [u.pick_mode, u.move_mode, u.rotate_mode, u.scale_mode].map(get),
    }
}

/// Show a pick status line (`ok` | `none` | `refused`) bound to the
/// current selection, revision and tool.
pub fn message(kind: &str, text: String) {
    MESSAGE_CONTEXT.with_borrow_mut(|c| *c = Some(message_context(false)));
    ui().pick_message.set(Some((kind.into(), text)));
}

/// Clear the status line and the hit cycle the moment anything else changes
/// the selection, the document or the tool: the Scene Tree, a breadcrumb,
/// Parent, a source selection, Escape, Undo / Redo, a reload or a tool
/// switch. A repeated click at the same place changes none of these until
/// its own pick applies, so hit cycling is kept.
pub fn watch_message() {
    Effect::new(move |_| {
        let now = message_context(true);
        let stale = MESSAGE_CONTEXT.with_borrow(|c| c.as_ref().is_some_and(|then| *then != now));
        if stale {
            MESSAGE_CONTEXT.with_borrow_mut(|c| *c = None);
            PICK.with_borrow_mut(|p| p.cycle = None);
            ui().pick_message.set(None);
        }
    });
}

pub fn active() -> Option<Gen> {
    PICK.with_borrow(|p| p.active)
}

/// B3 (LV2): the PROTO instance a shared-body gizmo commit was carried to
/// (Rust mapped it through the edit's exact changes). In ITS revision only,
/// the gizmo and the outline bind that instance's copy instead of asking
/// for a new pick. A new viewport pick, another document or any other
/// revision ends it; it never names a node by itself -- Rust re-proves the
/// whole chain and refuses any other instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceHint {
    pub session: u64,
    pub revision: u64,
    /// The instance's node item in `revision`.
    pub item: String,
    /// Its preview span.
    pub span: (u64, u64),
}

thread_local! {
    static INSTANCE_HINT: RefCell<Option<InstanceHint>> = const { RefCell::new(None) };
}

pub fn set_instance_hint(h: Option<InstanceHint>) {
    INSTANCE_HINT.with_borrow_mut(|x| *x = h);
}

/// The hint for exactly (`session`, `revision`), if any.
pub fn instance_hint(session: u64, revision: u64) -> Option<InstanceHint> {
    INSTANCE_HINT
        .with_borrow(|x| x.clone())
        .filter(|h| h.session == session && h.revision == revision)
}

/// A selection made OUTSIDE the viewport (Scene Tree, breadcrumb, Parent,
/// an Inspector "Open" button) -- even of the item already selected: the
/// status line and the hit cycle belong to viewport picks only, so the next
/// click there starts at the front layer again.
pub fn non_pick_selection() {
    MESSAGE_CONTEXT.with_borrow_mut(|c| *c = None);
    PICK.with_borrow_mut(|p| p.cycle = None);
    ui().pick_message.set(None);
}

/// The selection was cleared: no hit cycle, no proven pick and no instance
/// hint outlive it (the next click starts at the front layer).
pub fn forget_selection() {
    PICK.with_borrow_mut(|p| {
        p.cycle = None;
        p.last = None;
    });
    set_instance_hint(None);
}

/// Retire the generation on screen in both the UI and the adapter.
pub fn retire(reason: &str) {
    // A drag belongs to the generation on screen: it ends with it.
    crate::gizmo::cancel("the preview was reloaded");
    PICK.with_borrow_mut(|p| p.active = None);
    ipc::preview_retire(reason);
}

/// The newest load finished and shows exactly `g`.
pub fn loaded(g: Gen) {
    PICK.with_borrow_mut(|p| p.active = Some(g));
}

/// Every acknowledged change: a generation of another text is retired.
pub fn document_changed(session: Option<u64>, revision: u64) {
    if active().is_some_and(|g| Some(g.session) != session || g.revision != revision) {
        retire("source-changed-since-preview");
    }
}

#[derive(Deserialize)]
struct JsGen {
    session: Option<u64>,
    seq: Option<u64>,
}

/// `preview-adapter.js` `pick()`: the snapshot plus its generation binding.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsPick {
    #[serde(flatten)]
    snapshot: p::PickSnapshot,
    generation: Option<JsGen>,
    active: Option<Gen>,
    #[serde(default)]
    ms: f64,
    /// P0-A: the layer returned and how many Shapes are under the pointer.
    #[serde(default)]
    layer: u32,
    #[serde(default)]
    layers: u32,
}

const STALE_TEXT: &str = "The preview is out of date; select after it updates.";

fn local(
    status: &str,
    reason: &str,
    message: &str,
    generation: u64,
    revision: u64,
) -> p::PickOutcome {
    p::PickOutcome {
        status: status.into(),
        reason: reason.into(),
        message: message.into(),
        generation,
        revision,
        item: None,
        role: None,
        logical: None,
        shape: None,
    }
}

/// Resolve the click at (`client_x`, `client_y`) and apply the result.
/// Returns the outcome that was applied, or `None` when a newer pick
/// superseded this one (nothing applied).
pub async fn click(client_x: f64, client_y: f64) -> Option<p::PickOutcome> {
    let t0 = now();
    let session = CORE.with_borrow(|c| c.session)?;
    let seq = PICK.with_borrow_mut(|p| {
        p.seq += 1;
        p.seq
    });
    let selected = || ui().selected.get_untracked().map(|s| s.id);
    let gen_seq = active().map_or(0, |g| g.seq);
    let want = PICK.with_borrow(|p| {
        next_layer(
            p.cycle.as_ref(),
            gen_seq,
            client_x,
            client_y,
            selected().as_deref(),
        )
    });
    let mut layer = None;
    let out = resolve(session, client_x, client_y, want, &mut layer).await;
    if PICK.with_borrow(|p| p.seq) != seq {
        PICK.with_borrow_mut(|p| {
            p.superseded += 1;
            p.completed += 1;
        });
        return None;
    }
    let out = apply(session, out, layer).await;
    let after = selected();
    PICK.with_borrow_mut(|p| {
        p.last_total_ms = now() - t0;
        p.last = Some(out.clone());
        p.completed += 1;
        p.cycle = layer.map(|(layer, layers)| Cycle {
            seq: gen_seq,
            x: client_x,
            y: client_y,
            layer,
            layers,
            selected: after,
        });
    });
    Some(out)
}

/// Resolve one click. `layer` asks for that Shape under the pointer (P0-A);
/// `got` receives the (layer, layers) the adapter reported for a hit.
async fn resolve(
    session: u64,
    x: f64,
    y: f64,
    layer: u32,
    got: &mut Option<(u32, u32)>,
) -> p::PickOutcome {
    let current = CORE.with_borrow(|c| c.revision);
    let raw = match ipc::preview_pick(x, y, layer) {
        Ok(r) => r,
        Err(e) => {
            return local(
                "COMPATIBILITY_DISABLED",
                "adapter-unavailable",
                &format!("Preview picking unavailable ({e}). Select objects in the Scene Tree."),
                0,
                current,
            )
        }
    };
    let Ok(js) = serde_json::from_str::<JsPick>(&raw) else {
        return local(
            "UNSUPPORTED",
            "generation-unprovable",
            "This object cannot be selected from the preview; select it in the Scene Tree.",
            0,
            current,
        );
    };
    PICK.with_borrow_mut(|p| p.last_snapshot_ms = js.ms);
    if js.snapshot.outcome == "hit" && js.layers > 0 {
        *got = Some((js.layer, js.layers));
    }
    let active = active();
    let outcome = js.snapshot.outcome.as_str();
    // The hit must belong to the generation on screen NOW, of THIS session,
    // as both the adapter and the UI recorded it.
    if outcome != "disabled" {
        let bound = match (&js.generation, active) {
            (Some(g), Some(a)) => {
                g.session == Some(a.session) && g.seq == Some(a.seq) && js.active == Some(a)
            }
            (None, _) => !matches!(outcome, "hit" | "no-hit"),
            _ => false,
        };
        if !bound || active.is_some_and(|a| a.session != session) {
            return local(
                "REFUSED_STALE",
                "hit-from-another-preview-generation",
                STALE_TEXT,
                active.map_or(0, |a| a.seq),
                current,
            );
        }
    }
    let (revision, hash, generation) =
        active.map_or((current, 0, 0), |a| (a.revision, a.hash, a.seq));
    if outcome != "disabled" {
        if let Some(_why) = ui::stale_reason(session, revision) {
            return local(
                "REFUSED_STALE",
                "source-changed-since-preview",
                STALE_TEXT,
                generation,
                current,
            );
        }
    }
    #[derive(Serialize)]
    struct A {
        request: p::PickRequest,
    }
    let request = p::PickRequest {
        session,
        revision,
        preview_hash: hash,
        generation,
        snapshot: js.snapshot,
    };
    match call::<p::PickOutcome>("doc_pick", A { request }).await {
        Ok(o) => o,
        Err(e) => local(
            "UNSUPPORTED",
            "pick-failed",
            &format!("Pick failed: {e}"),
            generation,
            current,
        ),
    }
}

/// Apply one reply. Late replies (the preview, the document or the session
/// moved while it was in flight) are refused, never applied. A refusal never
/// clears an existing selection.
async fn apply(session: u64, out: p::PickOutcome, layer: Option<(u32, u32)>) -> p::PickOutcome {
    // P0-A: where this hit sits among the Shapes under the pointer.
    let (nth, behind) = match layer {
        Some((l, n)) if n > 1 => (
            format!(" Shape {} of {n} under the pointer", l + 1),
            if l + 1 < n {
                " — click again to select the one behind it"
            } else {
                " — click again to return to the front"
            },
        ),
        _ => (String::new(), ""),
    };
    let moved = CORE.with_borrow(|c| c.session) != Some(session)
        || (out.generation != 0 && active().map(|a| a.seq) != Some(out.generation))
        || (out.is_proven() && ui::stale_reason(session, out.revision).is_some());
    let out = if moved && out.status != "COMPATIBILITY_DISABLED" {
        PICK.with_borrow_mut(|p| p.late_refused += 1);
        local(
            "REFUSED_STALE",
            "preview-scene-replaced",
            STALE_TEXT,
            out.generation,
            out.revision,
        )
    } else {
        out
    };
    if out.is_proven() {
        let item = out.item.clone().unwrap_or_default();
        match ui::pick_select(session, out.revision, item).await {
            Ok(label) => {
                // B3: a PROTO body node binds the PICKED instance, so a new
                // pick of the same item may mean another instance. LV2: the
                // pick replaces any carried instance.
                set_instance_hint(None);
                crate::highlight::repicked();
                crate::gizmo::repicked();
                message(
                    "ok",
                    if nth.is_empty() {
                        format!("Selected {label} from the viewport.")
                    } else {
                        format!("Selected {label} ({}){behind}.", nth.trim_start())
                    },
                )
            }
            Err(why) => {
                message("refused", format!("Selection not changed: {why}."));
                return local(
                    "REFUSED_STALE",
                    "scene-tree-moved",
                    STALE_TEXT,
                    out.generation,
                    out.revision,
                );
            }
        }
    } else if out.status == "NO_HIT" {
        message("none", "Nothing selectable under the pointer.".into());
    } else {
        // The exact reason id is shown too: a refusal is never generic. A
        // layer behind the front one is named, so the user knows which
        // Shape is unavailable and that the next click moves on.
        let what = if nth.is_empty() {
            String::new()
        } else {
            format!("{} is unavailable{behind}: ", nth.trim_start())
        };
        message(
            "refused",
            format!("{what}{} [{}: {}]", out.message, out.status, out.reason),
        );
    }
    out
}

/// Arm the viewport's click gesture. Capture-phase listeners observe the
/// pointer without consuming it: camera navigation, authored sensors and
/// Anchors receive every event as before. Only a primary-button press and
/// release within `CLICK_SLOP`, while Select is active, requests a pick;
/// pointer movement never does.
pub fn install() {
    let Some(el) = crate::element_by_id::<web_sys::HtmlElement>("viewport") else {
        return;
    };
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    opts.set_passive(true);
    let down = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
        let d = (ev.button() == 0 && ev.is_primary())
            .then(|| (ev.pointer_id(), ev.client_x() as f64, ev.client_y() as f64));
        PICK.with_borrow_mut(|p| p.down = d);
    });
    let up = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
        let Some((id, x, y)) = PICK.with_borrow_mut(|p| p.down.take()) else {
            return;
        };
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        if id != ev.pointer_id()
            || ev.button() != 0
            || (cx - x).hypot(cy - y) > CLICK_SLOP
            || !(ui().pick_mode.get_untracked() || crate::gizmo::active_tool().is_some())
        {
            return;
        }
        PICK.with_borrow_mut(|p| p.last_click = Some((cx, cy)));
        // This capture listener runs BEFORE X_ITE's own release handlers, while
        // its viewer still counts the button as down (`touch()` would answer
        // false). Pick one task later, after the whole release is dispatched.
        spawn_local(async move {
            ipc::sleep(0).await;
            let _ = click(cx, cy).await;
        });
    });
    let cancel = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|_| {
        PICK.with_borrow_mut(|p| p.down = None);
    });
    for (name, f) in [
        ("pointerdown", &down),
        ("pointerup", &up),
        ("pointercancel", &cancel),
    ] {
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            name,
            f.as_ref().unchecked_ref(),
            &opts,
        );
    }
    watch_message();
    // The viewport lives as long as the page.
    down.forget();
    up.forget();
    cancel.forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prev(layer: u32, layers: u32, selected: Option<&str>) -> Cycle {
        Cycle {
            seq: 7,
            x: 100.0,
            y: 50.0,
            layer,
            layers,
            selected: selected.map(String::from),
        }
    }

    #[test]
    fn a_first_click_is_the_nearest_shape() {
        assert_eq!(next_layer(None, 7, 100.0, 50.0, None), 0);
        // One Shape under the pointer: nothing to cycle.
        let c = prev(0, 1, Some("node-1-2"));
        assert_eq!(next_layer(Some(&c), 7, 100.0, 50.0, Some("node-1-2")), 0);
    }

    #[test]
    fn clicking_again_in_place_steps_back_and_wraps() {
        let c = prev(0, 3, Some("coat"));
        assert_eq!(next_layer(Some(&c), 7, 101.0, 51.0, Some("coat")), 1);
        let c = prev(1, 3, Some("paint"));
        assert_eq!(next_layer(Some(&c), 7, 100.0, 50.0, Some("paint")), 2);
        let c = prev(2, 3, Some("body"));
        assert_eq!(next_layer(Some(&c), 7, 100.0, 50.0, Some("body")), 0);
        // A refused layer left the selection unchanged (here: nothing).
        let c = prev(1, 3, None);
        assert_eq!(next_layer(Some(&c), 7, 100.0, 50.0, None), 2);
    }

    #[test]
    fn anything_else_starts_again_at_the_front() {
        let c = prev(0, 2, Some("coat"));
        // Moved beyond the click slop.
        assert_eq!(
            next_layer(Some(&c), 7, 100.0 + CLICK_SLOP + 0.5, 50.0, Some("coat")),
            0
        );
        // Another preview generation (edit, reload).
        assert_eq!(next_layer(Some(&c), 8, 100.0, 50.0, Some("coat")), 0);
        // The selection changed since (Scene Tree, Undo, ...).
        assert_eq!(next_layer(Some(&c), 7, 100.0, 50.0, Some("wheel")), 0);
        assert_eq!(next_layer(Some(&c), 7, 100.0, 50.0, None), 0);
    }
}
