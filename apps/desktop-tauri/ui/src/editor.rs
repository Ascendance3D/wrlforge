// SPDX-License-Identifier: GPL-3.0-or-later
//! The source editor's synchronization with the canonical Rust document.
//!
//! The `<textarea>` DISPLAYS the editor view; it is not a text authority. Each
//! user change is turned into ONE minimal view edit against the text last
//! acknowledged by the backend, sent with that revision, and only adopted when
//! the backend applies it and returns a matching view hash. At most one edit is
//! in flight; keystrokes that arrive meanwhile are folded into the next diff.
//! Any refusal or hash mismatch resyncs the widget from the backend, so the
//! widget can never drift into a second document.

use std::cell::RefCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use web_sys::HtmlTextAreaElement;
use wrlforge_desktop_protocol as p;

use crate::ipc::{self, call, Session};
use crate::{syntax, ui};

#[derive(Default)]
pub struct Core {
    pub session: Option<u64>,
    /// The view text as last acknowledged by the backend, in UTF-16 units
    /// (the widget's own unit), so an edit is diffed without re-encoding.
    pub shown: Vec<u16>,
    pub revision: u64,
    pub busy: bool,
    /// Bumped on every acknowledged change; drives debounced analysis/preview.
    pub generation: u64,
    /// Last revisions the derived views were computed for (skip repeats).
    pub analyzed: Option<u64>,
    pub previewed: Option<u64>,
    /// Bumped per preview load; only the newest load reports its status.
    pub preview_seq: u64,
    /// Timings of the last keystroke path, for the smoke harness (ms).
    pub perf: Perf,
    /// Bumped per Inspector request; only the newest reply may land.
    pub inspect_seq: u64,
    /// Replies discarded as stale (test/diagnostic counters).
    pub stale_analyses: u64,
    pub stale_inspections: u64,
}

/// Per-stage cost of the last edit, measured where it happens.
#[derive(Default, Clone, Copy, Debug)]
pub struct Perf {
    /// `input` handler: widget read + color layer.
    pub input_ms: f64,
    /// Pump work on the main thread before and after the IPC call.
    pub pump_ms: f64,
    /// The `doc_edit` round trip (Rust apply + acknowledgment).
    pub ack_ms: f64,
    /// Caret line / column.
    pub cursor_ms: f64,
}

pub fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

thread_local! {
    pub static CORE: RefCell<Core> = RefCell::new(Core::default());
}

pub fn textarea() -> Option<HtmlTextAreaElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("source")?
        .dyn_into()
        .ok()
}

/// Minimal single edit turning `old` into `new`, in UTF-16 units, never
/// splitting a surrogate pair.
pub fn diff(old: &str, new: &str) -> (u64, u64, String) {
    let a: Vec<u16> = old.encode_utf16().collect();
    let b: Vec<u16> = new.encode_utf16().collect();
    diff_units(&a, &b)
}

/// `diff` over UTF-16 units: (from, old_to, inserted text).
pub fn diff_units(a: &[u16], b: &[u16]) -> (u64, u64, String) {
    let (from, old_to, new_to) = syntax::diff16(a, b);
    let insert = String::from_utf16(&b[from as usize..new_to as usize]).unwrap_or_default();
    (from as u64, old_to as u64, insert)
}

pub fn utf16_len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

/// Load a freshly opened / reloaded document into the widget.
pub fn adopt(doc: &p::DocumentInfo) {
    CORE.with_borrow_mut(|c| {
        c.session = Some(doc.session);
        c.shown = doc.view.encode_utf16().collect();
        c.revision = doc.revision;
        c.busy = false;
        c.generation += 1;
        c.analyzed = None;
        c.previewed = None;
    });
    if let Some(ta) = textarea() {
        ta.set_value(&doc.view);
        let _ = ta.set_selection_range(0, 0);
        ta.set_scroll_top(0);
        ta.set_scroll_left(0);
    }
    syntax::reset(&doc.view);
    ui::doc_loaded(doc);
}

pub async fn resync(reason: &str) {
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    match call::<p::DocumentInfo>("doc_snapshot", Session { session }).await {
        Ok(doc) => {
            let caret = textarea()
                .and_then(|t| t.selection_start().ok().flatten())
                .unwrap_or(0);
            CORE.with_borrow_mut(|c| {
                c.shown = doc.view.encode_utf16().collect();
                c.revision = doc.revision;
                c.busy = false;
                c.generation += 1;
            });
            if let Some(ta) = textarea() {
                ta.set_value(&doc.view);
                let _ = ta.set_selection_range(caret, caret);
            }
            syntax::set_text(&doc.view);
            // The change that led here is unknown: no selection survives it.
            ui::ui().selected.set(None);
            ui::ui().inspection.set(None);
            ui::state_changed(doc.revision, doc.dirty, doc.can_undo, doc.can_redo);
            ui::flash(&format!("Editor resynced from the Rust document: {reason}"));
        }
        Err(e) => ui::flash(&format!("Resync failed: {e}")),
    }
}

/// `input` handler: start (or fold into) the single in-flight edit.
pub fn on_input() {
    let t0 = now();
    // Paint the widget's new text immediately (typing, paste, IME preedit).
    if let Some(ta) = textarea() {
        syntax::set_text(&ta.value());
    }
    CORE.with_borrow_mut(|c| c.perf.input_ms = now() - t0);
    let start = CORE.with_borrow_mut(|c| {
        if c.busy || c.session.is_none() {
            false
        } else {
            c.busy = true;
            true
        }
    });
    if start {
        spawn_local(pump());
    }
}

async fn pump() {
    loop {
        let Some(ta) = textarea() else { break };
        let t0 = now();
        // The widget is the truth for what the user typed: read it, once.
        let current: Vec<u16> = ta.value().encode_utf16().collect();
        let (session, revision) = CORE.with_borrow(|c| (c.session, c.revision));
        let Some(session) = session else { break };
        let Some((from, to, insert)) =
            CORE.with_borrow(|c| (c.shown != current).then(|| diff_units(&c.shown, &current)))
        else {
            CORE.with_borrow_mut(|c| c.busy = false);
            break;
        };
        // The selection travels with the edit only if it is of this base.
        let sel = ui::ui()
            .selected
            .get_untracked()
            .filter(|s| s.session == session && s.revision == revision);
        let req = p::EditRequest {
            session,
            base_revision: revision,
            from,
            to,
            insert,
            item: sel.as_ref().map(|s| s.id.clone()),
        };
        #[derive(serde::Serialize)]
        struct A {
            request: p::EditRequest,
        }
        let t1 = now();
        let reply = call::<p::EditOutcome>("doc_edit", A { request: req }).await;
        let t2 = now();
        match reply {
            Ok(p::EditOutcome::Applied { state, item }) => {
                let hash_ok = state.view_len == current.len() as u64
                    && state.view_hash == p::view_hash_units(current.iter().copied());
                CORE.with_borrow_mut(|c| {
                    c.perf.pump_ms = (t1 - t0) + (now() - t2);
                    c.perf.ack_ms = t2 - t1;
                });
                if !hash_ok {
                    resync("view hash mismatch after edit").await;
                    break;
                }
                CORE.with_borrow_mut(|c| {
                    c.shown = current;
                    c.revision = state.revision;
                    c.generation += 1;
                });
                carry_selection(sel, item, state.revision);
                ui::state_changed(state.revision, state.dirty, state.can_undo, state.can_redo);
            }
            Ok(p::EditOutcome::Refused { message }) => {
                resync(&format!("edit refused ({message})")).await;
                break;
            }
            Err(e) => {
                resync(&format!("IPC error ({e})")).await;
                break;
            }
        }
    }
}

/// Wait until no edit is in flight (bounded).
pub async fn idle() {
    for _ in 0..400 {
        if !CORE.with_borrow(|c| c.busy) {
            return;
        }
        ipc::sleep(10).await;
    }
}

pub async fn history(undo: bool) {
    idle().await;
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    let cmd = if undo { "doc_undo" } else { "doc_redo" };
    #[derive(serde::Serialize)]
    struct A {
        session: u64,
        item: Option<String>,
    }
    let revision = CORE.with_borrow(|c| c.revision);
    let sel = ui::ui()
        .selected
        .get_untracked()
        .filter(|s| s.session == session && s.revision == revision);
    let item = sel.as_ref().map(|s| s.id.clone());
    match call::<p::HistoryOutcome>(cmd, A { session, item }).await {
        Ok(p::HistoryOutcome::Applied { state, view, item }) => {
            adopt_change(&state, view);
            if let Some(sel) = carry_selection(sel, item, state.revision) {
                ui::inspect(sel).await;
            }
        }
        Ok(p::HistoryOutcome::Nothing) => ui::flash(if undo {
            "Nothing to undo"
        } else {
            "Nothing to redo"
        }),
        Err(e) => ui::flash(&format!("{cmd} failed: {e}")),
    }
}

/// After an acknowledged change: the selection becomes the item Rust mapped
/// through the exact change, at the new revision, or is cleared when Rust
/// could not prove it -- never guessed. The Inspector keeps its (older)
/// data, marked as updating, until the analysis of the new revision.
fn carry_selection(
    sent: Option<ui::Selected>,
    item: Option<String>,
    revision: u64,
) -> Option<ui::Selected> {
    let u = ui::ui();
    let next = sent.zip(item).map(|(s, id)| ui::Selected {
        session: s.session,
        revision,
        id,
    });
    if next.is_none() && u.selected.get_untracked().is_some() {
        u.inspection.set(None);
    }
    u.selected.set(next.clone());
    next
}

/// Adopt a change Rust made and acknowledged (undo, redo, an Inspector field
/// edit): the widget shows exactly the returned view, never a local guess.
pub fn adopt_change(state: &p::DocState, view: String) {
    CORE.with_borrow_mut(|c| {
        c.shown = view.encode_utf16().collect();
        c.revision = state.revision;
        c.generation += 1;
    });
    if let Some(ta) = textarea() {
        let top = ta.scroll_top();
        ta.set_value(&view);
        let caret = state.caret as u32;
        let _ = ta.set_selection_range(caret, caret);
        ta.set_scroll_top(top);
    }
    syntax::set_text(&view);
    ui::state_changed(state.revision, state.dirty, state.can_undo, state.can_redo);
}

/// Select a view span in the editor and scroll it into view.
pub fn select(from: u64, to: u64) {
    if let Some(ta) = textarea() {
        let _ = ta.focus();
        let _ = ta.set_selection_range(from as u32, to as u32);
        // Scroll: place the selection's line near the top third.
        let line = line_col(&ta, from as u32).0 as usize - 1;
        let lh = 18.0_f64; // matches .source line-height in style.css (px)
        ta.set_scroll_top(((line as f64 * lh) - ta.client_height() as f64 / 3.0).max(0.0) as i32);
        update_cursor();
    }
}

pub fn update_cursor() {
    let t0 = now();
    let Some(ta) = textarea() else { return };
    let pos = ta.selection_start().ok().flatten().unwrap_or(0);
    let end = ta.selection_end().ok().flatten().unwrap_or(0);
    let (line, col) = line_col(&ta, pos);
    ui::cursor(line, col, end.saturating_sub(pos));
    CORE.with_borrow_mut(|c| c.perf.cursor_ms = now() - t0);
}

/// 1-based line / UTF-16 column of `pos`: from the color layer's line index
/// when it holds the widget's text, else by scanning the widget value.
fn line_col(ta: &HtmlTextAreaElement, pos: u32) -> (u32, u32) {
    if let Some(lc) = syntax::line_col(pos, ta.text_length()) {
        return lc;
    }
    let mut line = 1u32;
    let mut col = 1u32;
    for u in ta.value().encode_utf16().take(pos as usize) {
        if u == b'\n' as u16 {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::diff;

    #[test]
    fn diff_is_minimal_and_surrogate_safe() {
        assert_eq!(diff("abc", "abXc"), (2, 2, "X".into()));
        assert_eq!(diff("abc", "ac"), (1, 2, "".into()));
        assert_eq!(diff("", "x"), (0, 0, "x".into()));
        assert_eq!(diff("a😀b", "a😁b"), (1, 3, "😁".into()));
        assert_eq!(diff("aaa", "aa"), (2, 3, "".into()));
        assert_eq!(diff("same", "same"), (4, 4, "".into()));
    }
}
