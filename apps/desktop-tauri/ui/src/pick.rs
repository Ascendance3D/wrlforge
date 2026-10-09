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
}

thread_local! {
    pub static PICK: RefCell<PickState> = RefCell::new(PickState::default());
}

pub fn active() -> Option<Gen> {
    PICK.with_borrow(|p| p.active)
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
    let out = resolve(session, client_x, client_y).await;
    if PICK.with_borrow(|p| p.seq) != seq {
        PICK.with_borrow_mut(|p| {
            p.superseded += 1;
            p.completed += 1;
        });
        return None;
    }
    let out = apply(session, out).await;
    PICK.with_borrow_mut(|p| {
        p.last_total_ms = now() - t0;
        p.last = Some(out.clone());
        p.completed += 1;
    });
    Some(out)
}

async fn resolve(session: u64, x: f64, y: f64) -> p::PickOutcome {
    let current = CORE.with_borrow(|c| c.revision);
    let raw = match ipc::preview_pick(x, y) {
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
async fn apply(session: u64, out: p::PickOutcome) -> p::PickOutcome {
    let u = ui();
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
            Ok(label) => u.pick_message.set(Some((
                "ok".into(),
                format!("Selected {label} from the viewport."),
            ))),
            Err(why) => {
                u.pick_message.set(Some((
                    "refused".into(),
                    format!("Selection not changed: {why}."),
                )));
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
        u.pick_message.set(Some((
            "none".into(),
            "Nothing selectable under the pointer.".into(),
        )));
    } else {
        u.pick_message
            .set(Some(("refused".into(), out.message.clone())));
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
            || !(ui().pick_mode.get_untracked() || ui().move_mode.get_untracked())
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
    // The viewport lives as long as the page.
    down.forget();
    up.forget();
    cancel.forget();
}
