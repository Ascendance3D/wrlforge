// SPDX-License-Identifier: GPL-3.0-or-later
//! LV2: the app-drawn tooltip.
//!
//! WebKitGTK keeps a shown native `title` tooltip on screen until the
//! pointer moves -- over an opened menu, after a click, after the control
//! changed. Controls therefore carry their hover text in `data-tip`, and
//! this one tooltip element shows it: after a short hover delay, and hidden
//! on pointer leave, on any pointer press (a click, a menu opening), on any
//! key, on wheel / scroll, on window blur and after a timeout. A control
//! reached with the keyboard shows its tip the same way while it has focus.
//!
//! Accessibility: the text stays in the accessibility tree. An element with
//! no accessible name of its own (no `aria-label`, no text) gets the tip as
//! its `aria-label`; any other gets it as `aria-description`. Kept in sync
//! by one MutationObserver as the UI re-renders.
//!
//! View only: nothing here touches the document.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement};

/// Hover time before the tooltip shows (ms).
const SHOW_MS: i32 = 550;
/// A shown tooltip hides by itself after this long (ms).
const HIDE_MS: i32 = 6000;
pub const TIP_ID: &str = "app-tooltip";

#[derive(Default)]
struct Tip {
    /// The element hovered (shown or pending).
    target: Option<Element>,
    show_timer: Option<i32>,
    hide_timer: Option<i32>,
    /// Test counters.
    shown: u64,
    hidden: u64,
}

thread_local! {
    static TIP: RefCell<Tip> = RefCell::new(Tip::default());
    static KEEP: RefCell<Vec<Box<dyn std::any::Any>>> = const { RefCell::new(Vec::new()) };
}

fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

fn tip_el() -> Option<HtmlElement> {
    crate::element_by_id(TIP_ID)
}

fn clear_timers() {
    let Some(w) = window() else { return };
    TIP.with_borrow_mut(|t| {
        if let Some(id) = t.show_timer.take() {
            w.clear_timeout_with_handle(id);
        }
        if let Some(id) = t.hide_timer.take() {
            w.clear_timeout_with_handle(id);
        }
    });
}

/// Hide the tooltip and forget the hovered element.
pub fn hide() {
    clear_timers();
    TIP.with_borrow_mut(|t| t.target = None);
    if let Some(el) = tip_el() {
        if !el.hidden() {
            el.set_hidden(true);
            TIP.with_borrow_mut(|t| t.hidden += 1);
        }
    }
}

fn later(ms: i32, f: impl FnOnce() + 'static) -> Option<i32> {
    let cb = Closure::once_into_js(f);
    window()?
        .set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), ms)
        .ok()
}

/// (shown, hidden) counts -- for the smoke tests.
pub fn counts() -> (u64, u64) {
    TIP.with_borrow(|t| (t.shown, t.hidden))
}

/// Show the tip of `el` now (it must still be the hovered element).
fn show(el: &Element) {
    let same = TIP.with_borrow(|t| t.target.as_ref() == Some(el));
    let text = el.get_attribute("data-tip").unwrap_or_default();
    let (Some(tip), Some(w)) = (tip_el(), window()) else {
        return;
    };
    if !same || text.trim().is_empty() || !el.is_connected() {
        return;
    }
    tip.set_text_content(Some(&text));
    tip.set_hidden(false);
    // Below the control, inside the window; above it when there is no room.
    let r = el.get_bounding_client_rect();
    let t = tip.get_bounding_client_rect();
    let vw = w
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1024.0);
    let vh = w
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(768.0);
    let left = r.left().min(vw - t.width() - 4.0).max(4.0);
    let mut top = r.bottom() + 6.0;
    if top + t.height() > vh - 4.0 {
        top = (r.top() - t.height() - 6.0).max(4.0);
    }
    let s = tip.style();
    let _ = s.set_property("left", &format!("{left:.0}px"));
    let _ = s.set_property("top", &format!("{top:.0}px"));
    TIP.with_borrow_mut(|t| t.shown += 1);
    let id = later(HIDE_MS, hide);
    TIP.with_borrow_mut(|t| t.hide_timer = id);
}

/// The nearest element with a tip at or above the event target.
fn tipped(ev: &web_sys::Event) -> Option<Element> {
    let el: Element = ev.target()?.dyn_into().ok()?;
    el.closest("[data-tip]").ok().flatten()
}

fn hover(el: Option<Element>) {
    let current = TIP.with_borrow(|t| t.target.clone());
    if current == el {
        return;
    }
    hide();
    let Some(el) = el else { return };
    TIP.with_borrow_mut(|t| t.target = Some(el.clone()));
    let id = later(SHOW_MS, move || show(&el));
    TIP.with_borrow_mut(|t| t.show_timer = id);
}

/// Accessible text for every `[data-tip]` element under `root` (and root).
fn sync_aria(root: &Element) {
    let mut list = vec![];
    if root.has_attribute("data-tip") {
        list.push(root.clone());
    }
    if let Ok(all) = root.query_selector_all("[data-tip]") {
        for i in 0..all.length() {
            if let Some(e) = all.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                list.push(e);
            }
        }
    }
    for e in list {
        let tip = e.get_attribute("data-tip").unwrap_or_default();
        let tip = tip.trim();
        let named = e
            .get_attribute("aria-label")
            .is_some_and(|l| !l.trim().is_empty())
            && e.get_attribute("data-tip-label").is_none();
        let text = e.text_content().unwrap_or_default();
        if tip.is_empty() {
            let _ = e.remove_attribute("aria-description");
            continue;
        }
        if !named && text.trim().is_empty() {
            // No name of its own: the tip names it (marked, so a later tip
            // change updates it).
            if e.get_attribute("aria-label").as_deref() != Some(tip) {
                let _ = e.set_attribute("aria-label", tip);
                let _ = e.set_attribute("data-tip-label", "");
            }
        } else if e.get_attribute("aria-description").as_deref() != Some(tip)
            && e.get_attribute("aria-label").as_deref() != Some(tip)
        {
            let _ = e.set_attribute("aria-description", tip);
        }
    }
}

/// Install the tooltip element and its listeners. Idempotent.
pub fn install() {
    let Some(doc) = window().and_then(|w| w.document()) else {
        return;
    };
    if doc.get_element_by_id(TIP_ID).is_some() {
        return;
    }
    let Some(body) = doc.body() else { return };
    let Ok(tip) = doc.create_element("div") else {
        return;
    };
    tip.set_id(TIP_ID);
    tip.set_class_name("app-tooltip");
    let _ = tip.set_attribute("role", "tooltip");
    let _ = tip.set_attribute("hidden", "");
    let _ = tip.set_attribute("aria-hidden", "true");
    let _ = body.append_child(&tip);

    let on = |name: &str, f: Box<dyn FnMut(web_sys::Event)>, capture: bool| {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(f);
        let opts = web_sys::AddEventListenerOptions::new();
        opts.set_capture(capture);
        opts.set_passive(true);
        let _ = doc.add_event_listener_with_callback_and_add_event_listener_options(
            name,
            cb.as_ref().unchecked_ref(),
            &opts,
        );
        KEEP.with_borrow_mut(|k| k.push(Box::new(cb)));
    };
    on("pointerover", Box::new(|ev| hover(tipped(&ev))), true);
    on(
        "pointerout",
        Box::new(|ev| {
            let ev: web_sys::PointerEvent = match ev.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            // Left the hovered element (not just moved onto its child).
            let to = ev
                .related_target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .and_then(|e| e.closest("[data-tip]").ok().flatten());
            let cur = TIP.with_borrow(|t| t.target.clone());
            if cur.is_some() && to != cur {
                hide();
            }
        }),
        true,
    );
    // Keyboard users: a control reached by the keyboard (`:focus-visible`)
    // shows its tip like a hovered one; leaving it hides the tip.
    on(
        "focusin",
        Box::new(|ev| {
            let keyboard = ev
                .target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .is_some_and(|e| e.matches(":focus-visible").unwrap_or(false));
            if keyboard {
                hover(tipped(&ev));
            }
        }),
        true,
    );
    on("focusout", Box::new(|_| hide()), true);
    for name in ["pointerdown", "keydown", "wheel", "scroll", "dragstart"] {
        on(name, Box::new(|_| hide()), true);
    }
    // Leaving the window (or the page) hides it too.
    if let Some(w) = window() {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(|_| hide());
        let _ = w.add_event_listener_with_callback("blur", cb.as_ref().unchecked_ref());
        let _ = doc.add_event_listener_with_callback("pointerleave", cb.as_ref().unchecked_ref());
        KEEP.with_borrow_mut(|k| k.push(Box::new(cb)));
    }

    // Accessible text, now and as the UI re-renders.
    sync_aria(&body);
    let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
        |records: js_sys::Array, _o: web_sys::MutationObserver| {
            for r in records.iter() {
                let Ok(r) = r.dyn_into::<web_sys::MutationRecord>() else {
                    continue;
                };
                if r.type_() == "attributes" {
                    if let Some(e) = r.target().and_then(|t| t.dyn_into::<Element>().ok()) {
                        // The hovered control changed its tip: re-show.
                        let hovered = TIP.with_borrow(|t| t.target.as_ref() == Some(&e));
                        sync_aria(&e);
                        if hovered {
                            hide();
                        }
                    }
                    continue;
                }
                let added = r.added_nodes();
                for i in 0..added.length() {
                    if let Some(e) = added.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                        sync_aria(&e);
                    }
                }
                // The hovered control was removed (a menu closed, a row
                // re-rendered): its tooltip goes with it.
                let gone =
                    TIP.with_borrow(|t| t.target.as_ref().is_some_and(|e| !e.is_connected()));
                if gone {
                    hide();
                }
            }
        },
    );
    if let Ok(obs) = web_sys::MutationObserver::new(cb.as_ref().unchecked_ref()) {
        let init = web_sys::MutationObserverInit::new();
        init.set_child_list(true);
        init.set_subtree(true);
        init.set_attributes(true);
        init.set_attribute_filter(&js_sys::Array::of1(&JsValue::from_str("data-tip")));
        let _ = obs.observe_with_options(&body, &init);
        KEEP.with_borrow_mut(|k| k.push(Box::new(obs)));
    }
    KEEP.with_borrow_mut(|k| k.push(Box::new(cb)));
}
