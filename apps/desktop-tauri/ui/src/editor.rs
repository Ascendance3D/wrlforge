// SPDX-License-Identifier: GPL-3.0-or-later
//! The source editor's synchronisation with the canonical Rust document.
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
use crate::ui;

#[derive(Default)]
pub struct Core {
    pub session: Option<u64>,
    /// The view text as last acknowledged by the backend.
    pub shown: String,
    pub revision: u64,
    pub busy: bool,
    /// Bumped on every acknowledged change; drives debounced analysis/preview.
    pub generation: u64,
    /// Last revisions the derived views were computed for (skip repeats).
    pub analyzed: Option<u64>,
    pub previewed: Option<u64>,
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
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    if pre > 0 && (0xD800..0xDC00).contains(&a[pre - 1]) {
        pre -= 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }
    if suf > 0 && (0xDC00..0xE000).contains(&a[a.len() - suf]) {
        suf -= 1;
    }
    let insert = String::from_utf16(&b[pre..b.len() - suf]).unwrap_or_default();
    (pre as u64, (a.len() - suf) as u64, insert)
}

pub fn utf16_len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

/// Load a freshly opened / reloaded document into the widget.
pub fn adopt(doc: &p::DocumentInfo) {
    CORE.with_borrow_mut(|c| {
        c.session = Some(doc.session);
        c.shown = doc.view.clone();
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
                c.shown = doc.view.clone();
                c.revision = doc.revision;
                c.busy = false;
                c.generation += 1;
            });
            if let Some(ta) = textarea() {
                ta.set_value(&doc.view);
                let _ = ta.set_selection_range(caret, caret);
            }
            ui::state_changed(doc.revision, doc.dirty, doc.can_undo, doc.can_redo);
            ui::flash(&format!("Editor resynced from the Rust document: {reason}"));
        }
        Err(e) => ui::flash(&format!("Resync failed: {e}")),
    }
}

/// `input` handler: start (or fold into) the single in-flight edit.
pub fn on_input() {
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
        let current = ta.value();
        let (session, shown, revision) =
            CORE.with_borrow(|c| (c.session, c.shown.clone(), c.revision));
        let Some(session) = session else { break };
        if current == shown {
            CORE.with_borrow_mut(|c| c.busy = false);
            break;
        }
        let (from, to, insert) = diff(&shown, &current);
        let req = p::EditRequest {
            session,
            base_revision: revision,
            from,
            to,
            insert,
        };
        #[derive(serde::Serialize)]
        struct A {
            request: p::EditRequest,
        }
        match call::<p::EditOutcome>("doc_edit", A { request: req }).await {
            Ok(p::EditOutcome::Applied { state }) => {
                if state.view_hash != p::view_hash(&current)
                    || state.view_len != utf16_len(&current)
                {
                    resync("view hash mismatch after edit").await;
                    break;
                }
                CORE.with_borrow_mut(|c| {
                    c.shown = current;
                    c.revision = state.revision;
                    c.generation += 1;
                });
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
    let item = ui::ui().selected.get_untracked();
    let had = item.is_some();
    match call::<p::HistoryOutcome>(cmd, A { session, item }).await {
        Ok(p::HistoryOutcome::Applied { state, view, item }) => {
            adopt_change(&state, view);
            // Rust mapped the selection through the exact change; a lost one
            // is cleared, never guessed.
            match item {
                Some(id) => ui::inspect(id).await,
                None if had => {
                    ui::ui().selected.set(None);
                    ui::ui().inspection.set(None);
                }
                None => {}
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

/// Adopt a change Rust made and acknowledged (undo, redo, an Inspector field
/// edit): the widget shows exactly the returned view, never a local guess.
pub fn adopt_change(state: &p::DocState, view: String) {
    CORE.with_borrow_mut(|c| {
        c.shown = view.clone();
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
    ui::state_changed(state.revision, state.dirty, state.can_undo, state.can_redo);
}

/// Select a view span in the editor and scroll it into view.
pub fn select(from: u64, to: u64) {
    if let Some(ta) = textarea() {
        let _ = ta.focus();
        let _ = ta.set_selection_range(from as u32, to as u32);
        // Scroll: place the selection's line near the top third.
        let v = ta.value();
        let line = v
            .encode_utf16()
            .take(from as usize)
            .filter(|&u| u == b'\n' as u16)
            .count();
        let lh = 18.0_f64; // matches .source line-height in style.css (px)
        ta.set_scroll_top(((line as f64 * lh) - ta.client_height() as f64 / 3.0).max(0.0) as i32);
        update_cursor();
    }
}

pub fn update_cursor() {
    let Some(ta) = textarea() else { return };
    let pos = ta.selection_start().ok().flatten().unwrap_or(0) as usize;
    let end = ta.selection_end().ok().flatten().unwrap_or(0) as usize;
    let v = ta.value();
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, u) in v.encode_utf16().enumerate() {
        if i >= pos {
            break;
        }
        if u == b'\n' as u16 {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    ui::cursor(line, col, end.saturating_sub(pos) as u32);
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
