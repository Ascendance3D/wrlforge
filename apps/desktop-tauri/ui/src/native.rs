// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1: the UI side of the hidden native viewport.
//!
//! The native viewport is a sibling of this WebView (a native split, D6); it
//! is not drawn here. This module only:
//! * routes the preview to `native_show` instead of X_ITE while the native
//!   viewport is starting or ready, and back to X_ITE when it fails (D8/D9);
//! * applies native pick replies through the ONE selection authority
//!   (`ui::pick_select`), refusing late or stale replies exactly like X_ITE
//!   picks (`pick.rs`);
//! * clears the selection on the viewport's Escape;
//! * tells Rust which item to highlight (display only).
//!
//! The UI holds no identity authority: Rust proved every pick it applies.

use std::cell::RefCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wrlforge_desktop_protocol as p;

use crate::editor::CORE;
use crate::ipc::{call, NoArgs, Session};
use crate::ui::{self, ui};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn tauri_listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> Result<JsValue, JsValue>;
}

#[derive(Default)]
struct NativeUi {
    /// The projection on screen: (session, revision, generation).
    shown: Option<(u64, u64, u64)>,
    handler: Option<Closure<dyn FnMut(JsValue)>>,
    /// Replies refused as late (test counter).
    late_refused: u64,
}

thread_local! {
    static NATIVE: RefCell<NativeUi> = RefCell::new(NativeUi::default());
}

/// The native viewport is on screen (starting or ready): X_ITE is not used.
pub fn on() -> bool {
    ui().native.with_untracked(|s| s.native_shown())
}

/// Read the state and subscribe to the native events. Before the first
/// document is applied, so the first preview already goes to the right
/// renderer.
pub async fn install() {
    let u = ui();
    if let Ok(s) = call::<p::NativeState>("native_state", NoArgs {}).await {
        u.native.set(s);
    }
    if !u.native.with_untracked(|s| s.requested) {
        return;
    }
    let handler = Closure::<dyn FnMut(JsValue)>::new(|ev: JsValue| {
        let Ok(payload) = js_sys::Reflect::get(&ev, &JsValue::from_str("payload")) else { return };
        match serde_wasm_bindgen::from_value::<p::NativeEvent>(payload) {
            Ok(e) => spawn_local(on_event(e)),
            Err(e) => web_sys::console::warn_1(&format!("native event: {e}").into()),
        }
    });
    let _ = tauri_listen(p::NATIVE_EVENT, &handler).await;
    NATIVE.with_borrow_mut(|n| n.handler = Some(handler));
    // The state may have moved while subscribing.
    if let Ok(s) = call::<p::NativeState>("native_state", NoArgs {}).await {
        u.native.set(s);
    }
    // Highlight follows the selection (display only).
    Effect::new(move |_| {
        let sel = u.selected.get();
        if !u.native.with(|s| s.native_shown()) {
            return;
        }
        let Some(session) = CORE.with_borrow(|c| c.session) else { return };
        let (revision, item) = match sel {
            Some(s) if s.session == session => (s.revision, Some(s.id)),
            _ => (CORE.with_borrow(|c| c.revision), None),
        };
        #[derive(Serialize)]
        struct A {
            session: u64,
            revision: u64,
            item: Option<String>,
        }
        spawn_local(async move {
            let _ = call::<u32>("native_select", A { session, revision, item }).await;
        });
    });
}

async fn on_event(e: p::NativeEvent) {
    let u = ui();
    match e {
        p::NativeEvent::State { state } => {
            let was = u.native.with_untracked(|s| s.native_shown());
            let now = state.native_shown();
            u.native.set(state);
            if was && !now {
                // D8/D9: X_ITE takes over with the current text.
                NATIVE.with_borrow_mut(|n| n.shown = None);
                ui::preview(true).await;
            }
        }
        p::NativeEvent::Cleared { session } => {
            if session.is_some() && session == CORE.with_borrow(|c| c.session) {
                u.selected.set(None);
                u.inspection.set(None);
                u.pick_message.set(Some(("none".into(), "Selection cleared.".into())));
            }
        }
        p::NativeEvent::Pick { session, pick } => apply(session, pick).await,
    }
}

/// Apply one native pick reply. A reply for another session, generation or
/// revision than the one on screen now is refused, never applied; a refusal
/// never clears an existing selection.
async fn apply(session: u64, out: p::PickOutcome) {
    let u = ui();
    let shown = NATIVE.with_borrow(|n| n.shown);
    let moved = CORE.with_borrow(|c| c.session) != Some(session)
        || shown.is_none_or(|(s, _, g)| s != session || g != out.generation)
        || (out.is_proven() && ui::stale_reason(session, out.revision).is_some());
    if moved && out.status != "COMPATIBILITY_DISABLED" {
        NATIVE.with_borrow_mut(|n| n.late_refused += 1);
        u.pick_message.set(Some(("refused".into(), "The viewport is out of date; select after it updates.".into())));
        return;
    }
    if out.is_proven() {
        let item = out.item.clone().unwrap_or_default();
        match ui::pick_select(session, out.revision, item).await {
            Ok(label) => u.pick_message.set(Some(("ok".into(), format!("Selected {label} from the viewport.")))),
            Err(why) => u.pick_message.set(Some(("refused".into(), format!("Selection not changed: {why}.")))),
        }
    } else if out.status == "NO_HIT" {
        u.pick_message.set(Some(("none".into(), "Nothing selectable under the pointer.".into())));
    } else {
        u.pick_message.set(Some(("refused".into(), out.message.clone())));
    }
}

/// The native counterpart of `ui::preview`.
pub async fn preview(force: bool) {
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    let (rev, done) = CORE.with_borrow(|c| (c.revision, c.previewed));
    if !force && done == Some(rev) {
        return;
    }
    let u = ui();
    u.preview_status.set("updating…".into());
    match call::<p::NativeShown>("native_show", Session { session }).await {
        Ok(s) => {
            CORE.with_borrow_mut(|c| c.previewed = Some(rev));
            u.preview_loads.update(|n| *n += 1);
            let hidden = if s.not_shown.is_empty() {
                String::new()
            } else {
                format!(" · {} node(s) not shown natively", s.not_shown.len())
            };
            if s.kept_last_valid {
                u.preview_status.set(format!("rev {rev} has syntax errors · showing the last valid scene (rev {}){hidden}", s.revision));
            } else {
                NATIVE.with_borrow_mut(|n| n.shown = Some((session, s.revision, s.generation)));
                u.preview_status.set(format!("rev {} · native · {} object(s){hidden}", s.revision, s.objects));
            }
        }
        Err(e) => u.preview_status.set(format!("native error: {e}")),
    }
}

pub fn reset_camera() {
    spawn_local(async {
        let _ = call::<()>("native_reset_camera", NoArgs {}).await;
    });
}
