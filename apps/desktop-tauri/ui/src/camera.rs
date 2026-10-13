// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A1: keep the user's working camera across a preview reload.
//!
//! Every reload builds a whole new X_ITE scene, which binds its first
//! viewpoint (or the layer default) with zero user offsets: the view the user
//! orbited, panned or zoomed to would be lost after every Move. The carry
//! restores it only onto a viewpoint PROVEN to be the same one:
//!
//! * the layer's built-in default viewpoint, in both scenes; or
//! * the authored viewpoint whose provenance span the adapter captured when
//!   its generation retired, mapped by Rust (`doc_preview_carry`) through the
//!   exact logged changes to THIS revision, and found as the ONE runtime
//!   node for that span in the new generation.
//!
//! Nothing is matched by name, description, index or position; when the
//! proof fails, nothing is restored and the reason is reported. The camera
//! is editor state: no source change, no dirty flag, no undo step.

use std::cell::RefCell;

use wrlforge_desktop_protocol as p;

use crate::ipc::{self, call, CameraWant};

thread_local! {
    /// Per document preview load: X_ITE parse + scene replacement (ms).
    pub static LOAD_MS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

/// What the next load of `revision` (session `session`) may restore onto,
/// and why not when nothing.
pub async fn want(session: u64, revision: u64) -> (Option<CameraWant>, &'static str) {
    let Some(cap) = ipc::camera_capture() else {
        return (None, "no-capture");
    };
    if cap.session != session {
        return (None, "another-document");
    }
    match cap.kind.as_str() {
        "default" => (Some(CameraWant::Default), "default-viewpoint"),
        "authored" => {
            let (Some(from), Some(to)) = (cap.from, cap.to) else {
                return (None, "viewpoint-without-span");
            };
            #[derive(serde::Serialize)]
            struct A {
                request: p::PreviewCarryRequest,
            }
            let r = call::<p::PreviewCarryOutcome>(
                "doc_preview_carry",
                A {
                    request: p::PreviewCarryRequest {
                        session,
                        from_revision: cap.revision,
                        to_revision: revision,
                        from,
                        to,
                    },
                },
            )
            .await;
            match r {
                Ok(p::PreviewCarryOutcome::Mapped { from, to }) => (
                    Some(CameraWant::Authored { from, to }),
                    "authored-viewpoint",
                ),
                Ok(p::PreviewCarryOutcome::Lost { .. }) => (None, "viewpoint-span-lost"),
                Err(_) => (None, "carry-unavailable"),
            }
        }
        _ => (None, "viewpoint-unprovable"),
    }
}

/// A short status suffix when the user's view could NOT be kept (the reason
/// is reported, never hidden); empty when it was, or when there was nothing
/// to keep (first load, another document).
pub fn note(why: &str) -> String {
    let reason = match why {
        "no-capture" | "another-document" => return String::new(),
        "default-viewpoint" | "authored-viewpoint" => match ipc::camera_result() {
            Some(r) if r.status == "restored" => return String::new(),
            // The old scene stayed on screen (a failed load) or a newer load
            // reports: the view did not change here.
            Some(r)
                if matches!(
                    r.reason.as_deref(),
                    Some("world-not-replaced" | "superseded")
                ) =>
            {
                return String::new()
            }
            Some(r) => r.reason.unwrap_or_else(|| r.status.clone()),
            None => "camera-carry-unavailable".into(),
        },
        other => other.to_string(),
    };
    format!(" · view reset ({reason})")
}

// ---- B2-CAMERA: Fit, standard views, Reset, viewpoint list -----------------
//
// Every action changes only the renderer's camera (the bound viewpoint's
// user offsets, or which viewpoint is bound): never the source, never the
// dirty flag, never the undo history. Because the user offsets are exactly
// what the carry above keeps, a Fit or a standard view survives the next
// preview reload like any orbit.

use std::cell::Cell;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ui::{self, ui};

/// Transition of an explicit Fit (s). The automatic first Fit is immediate.
const FIT_SECONDS: f64 = 0.3;

thread_local! {
    /// The session whose first shown scene may still be framed once
    /// (File → Open, a startup file, a restored document).
    static AUTO_FIT: Cell<Option<u64>> = const { Cell::new(None) };
    /// File → Open / startup armed the next opened document.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// A GUI smoke run is active: no automatic Fit at all.
    static SUPPRESSED: Cell<bool> = const { Cell::new(false) };
}

/// The next document `apply_open` adopts gets one automatic Fit.
pub fn arm_auto_fit() {
    if !SUPPRESSED.with(|s| s.get()) {
        ARMED.with(|a| a.set(true));
    }
}

/// No automatic Fit for the rest of this run. The GUI smoke suites were
/// written for the authored (unframed) camera.
pub fn suppress_auto_fit() {
    SUPPRESSED.with(|s| s.set(true));
    ARMED.with(|a| a.set(false));
    AUTO_FIT.with(|f| f.set(None));
}

/// `apply_open` adopted `session` (or nothing, with `None`).
pub fn opened(session: Option<u64>) {
    if ARMED.with(|a| a.replace(false)) {
        AUTO_FIT.with(|f| f.set(session));
    }
}

/// Whether the load of `session` about to start may frame the scene.
pub fn auto_fit_wanted(session: u64) -> bool {
    AUTO_FIT.with(|f| f.get()) == Some(session)
}

/// After the newest load of `session` (generation `seq`) is on screen: the
/// automatic Fit is used up, a viewpoint chosen from the list is bound
/// again when the new scene bound another one, and the list is refreshed.
/// Returns the preview status suffix, replacing `note` when it changed.
pub fn after_load(
    session: u64,
    seq: u64,
    auto_fit: bool,
    want: Option<CameraWant>,
    note: String,
) -> String {
    let mut note = note;
    if auto_fit {
        AUTO_FIT.with(|f| f.set(None));
        if let Some(r) = ipc::camera_auto_fit().filter(|r| r.seq == Some(seq)) {
            match r.status.as_str() {
                "fitted" => note.push_str(" · view fitted to the scene"),
                "failed" => note.push_str(&format!(
                    " · could not fit the view ({})",
                    r.reason.unwrap_or_default()
                )),
                _ => {}
            }
        }
    }
    // The carry keeps the user's offsets only on the viewpoint the new scene
    // binds itself. A viewpoint the user chose from the list is the SAME
    // proven span (mapped by Rust); bind it again rather than drop it.
    if let Some(CameraWant::Authored { from, to }) = want {
        let rebinds = ipc::camera_result().is_some_and(|r| {
            r.session == Some(session)
                && r.reason.as_deref() == Some("new-scene-binds-another-viewpoint")
        });
        if rebinds && ipc::camera_bind(seq, from, to).ok {
            note = " · chosen viewpoint kept".into();
        }
    }
    refresh_viewpoints();
    note
}

/// Re-read the scene's authored viewpoints now and once more after X_ITE
/// has drawn (its list is filled while it draws).
pub fn refresh_viewpoints() {
    ui().viewpoints.set(ipc::camera_viewpoints());
    spawn_local(async {
        sleep_ms(250).await;
        let now = ipc::camera_viewpoints();
        let u = ui();
        if u.viewpoints.with_untracked(|v| *v != now) {
            u.viewpoints.set(now);
        }
    });
}

async fn sleep_ms(ms: i32) {
    let p = js_sys::Promise::new(&mut |res, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&res, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}

fn refused(what: &str, a: &ipc::CameraAck) {
    ui::flash(&format!(
        "{what} is not available: {}.",
        a.reason.as_deref().unwrap_or("the camera could not be changed")
    ));
}

/// View → View All (Fit): X_ITE's own View All on the bound viewpoint.
pub fn fit() {
    let a = ipc::camera_fit(FIT_SECONDS);
    if a.ok {
        ui::flash("View All: the whole scene is in view (camera only; the source is unchanged).");
    } else {
        refused("View All", &a);
    }
}

/// LV2-C: move the camera (only) so every `world` point is in view, looking
/// the same way as now. `Err` says why nothing changed.
pub fn frame_points(world: &[[f64; 3]]) -> Result<(), String> {
    let input = ipc::camera_frame()?;
    let o = p::camera::frame_points(&input, world).map_err(String::from)?;
    let a = ipc::camera_apply(&o);
    if a.ok {
        Ok(())
    } else {
        Err(a.reason.unwrap_or_else(|| "the view could not be changed".into()))
    }
}

/// View → Frame Selection: the selected object's proven outline box.
pub fn frame_selection() {
    let corners = crate::highlight::selection_bounds()
        .ok_or_else(|| "the selection has no proven outline in the viewport".to_string())
        .and_then(|b| p::highlight::world_corners(&b).map_err(String::from));
    match corners.and_then(|c| frame_points(&c)) {
        Ok(()) => ui::flash("Framed the selection (camera only; the source is unchanged)."),
        Err(why) => ui::flash(&format!("Frame Selection is not available: {why}.")),
    }
}

/// View → Front / Back / Left / Right / Top / Bottom.
pub fn standard(view: p::camera::StandardView) {
    let name = format!("{view:?}");
    let input = match ipc::camera_frame() {
        Ok(f) => f,
        Err(why) => {
            return ui::flash(&format!("{name} view is not available: {why}."));
        }
    };
    match p::camera::standard_view(&input, view) {
        Ok(o) => {
            let a = ipc::camera_apply(&o);
            if a.ok {
                ui::flash(&format!("{name} view (camera only; the source is unchanged)."));
            } else {
                refused(&format!("{name} view"), &a);
            }
        }
        Err(why) => ui::flash(&format!("{name} view is not available: {why}.")),
    }
}

/// View → Reset View: the bound viewpoint as authored. The layer's default
/// viewpoint has no authored view of the scene, so it is framed instead.
pub fn reset() {
    let a = ipc::camera_reset();
    if !a.ok {
        return refused("Reset View", &a);
    }
    if a.is_default == Some(true) {
        let f = ipc::camera_fit(0.0);
        if !f.ok {
            return refused("Reset View", &f);
        }
        ui::flash("Reset View: the default view, framed on the scene.");
    } else {
        ui::flash("Reset View: the bound viewpoint as authored.");
    }
}

/// The viewpoint list: bind item `index` of the list on screen.
pub fn choose_viewpoint(index: usize) {
    let list = ui().viewpoints.get_untracked();
    let (Some(seq), Some(item)) = (list.seq, list.items.get(index)) else {
        return ui::flash("That viewpoint is no longer in the preview.");
    };
    let a = ipc::camera_bind(seq, item.from, item.to);
    if a.ok {
        ui::flash(&format!("Viewpoint: {}.", viewpoint_label(item, index)));
        refresh_viewpoints();
    } else {
        refused("That viewpoint", &a);
    }
}

/// The list label: the description, else the DEF name, else its position.
pub fn viewpoint_label(item: &ipc::ViewpointItem, index: usize) -> String {
    if !item.description.trim().is_empty() {
        item.description.trim().to_string()
    } else if !item.name.is_empty() {
        item.name.clone()
    } else {
        format!("Viewpoint {}", index + 1)
    }
}
