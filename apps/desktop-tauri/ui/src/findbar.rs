// SPDX-License-Identifier: GPL-3.0-or-later
//! B2: the Source panel's Find / Replace / Go to Line bar.
//!
//! UI state only: nothing here is document state. Matches are computed by
//! `find.rs` over the text Rust last ACKNOWLEDGED (`editor::CORE.shown`) at
//! one revision, and a replacement is sent as ONE view edit against exactly
//! that revision through `editor::submit_edit` (revision-checked, view-hash
//! verified). The textarea is never changed before Rust applies the edit.
//! Replace All is one edit spanning the first to the last match, so it is
//! one undo step.

use std::cell::{Cell, OnceCell, RefCell};

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, KeyboardEvent};
use wrlforge_desktop_protocol::layout::PanelId;

use crate::editor::{self, CORE};
use crate::{dock, find, ipc, ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Find,
    Replace,
    GoTo,
}

#[derive(Clone, Copy)]
struct State {
    mode: RwSignal<Option<Mode>>,
    query: RwSignal<String>,
    replacement: RwSignal<String>,
    match_case: RwSignal<bool>,
    wrap: RwSignal<bool>,
    line: RwSignal<String>,
    /// The bar's own status line (count, wrap, refusals).
    status: RwSignal<String>,
    /// The status is a refusal / error (styled, announced).
    error: RwSignal<bool>,
}

/// Matches of one query over one acknowledged text.
struct Cache {
    session: Option<u64>,
    revision: u64,
    query: String,
    match_case: bool,
    matches: Vec<find::Span>,
}

thread_local! {
    static STATE: OnceCell<State> = const { OnceCell::new() };
    static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) };
    static COUNT_SEQ: Cell<u64> = const { Cell::new(0) };
}

fn st() -> State {
    STATE.with(|s| {
        *s.get_or_init(|| State {
            mode: RwSignal::new(None),
            query: RwSignal::new(String::new()),
            replacement: RwSignal::new(String::new()),
            match_case: RwSignal::new(false),
            wrap: RwSignal::new(true),
            line: RwSignal::new(String::new()),
            status: RwSignal::new(String::new()),
            error: RwSignal::new(false),
        })
    })
}

/// Created before mount, outside any owner (like the menu state).
pub fn init() {
    let _ = st();
}

fn say(msg: impl Into<String>, error: bool) {
    let s = st();
    s.status.set(msg.into());
    s.error.set(error);
}

fn input(id: &str) -> Option<HtmlInputElement> {
    crate::element_by_id(id)
}

/// The Source selection (UTF-16 offsets).
fn selection() -> (u32, u32) {
    editor::textarea()
        .map(|t| {
            (
                t.selection_start().ok().flatten().unwrap_or(0),
                t.selection_end().ok().flatten().unwrap_or(0),
            )
        })
        .unwrap_or((0, 0))
}

/// Open the bar (Edit → Find / Replace / Go to Line, Ctrl+F / H / G).
pub fn open(mode: Mode) {
    let s = st();
    // Prefill the query from a one-line Source selection.
    if mode != Mode::GoTo {
        if let Some(ta) = editor::textarea() {
            let (a, b) = selection();
            if b > a && b - a <= 200 {
                let v: Vec<u16> = ta.value().encode_utf16().collect();
                if let Some(sel) = v.get(a as usize..b as usize) {
                    if !sel.iter().any(|&u| u == b'\n' as u16 || u == b'\r' as u16) {
                        s.query.set(String::from_utf16_lossy(sel));
                    }
                }
            }
        }
    }
    if mode == Mode::GoTo {
        s.line.set(String::new());
    }
    s.mode.set(Some(mode));
    say(String::new(), false);
    // The Source panel must be on screen (a tab may hide it).
    dock::focus(PanelId::Source);
    spawn_local(async move {
        // After the dock gave the textarea the focus.
        ipc::sleep(30).await;
        let id = match mode {
            Mode::GoTo => "goto-line",
            _ => "find-query",
        };
        if let Some(i) = input(id) {
            let _ = i.focus();
            i.select();
        }
        if mode != Mode::GoTo {
            refresh_count();
        } else {
            say(format!("Line 1 to {}", crate::syntax::line_total()), false);
        }
    });
}

/// Close the bar and give the Source editor the focus back.
pub fn close() {
    st().mode.set(None);
    if let Some(ta) = editor::textarea() {
        let _ = ta.focus();
    }
}

pub fn mode() -> Option<Mode> {
    st().mode.get_untracked()
}

/// The matches of the current query in the acknowledged text, and the
/// revision they belong to.
fn matches() -> (u64, Vec<find::Span>) {
    let s = st();
    let query = s.query.get_untracked();
    let match_case = s.match_case.get_untracked();
    let (session, revision) = CORE.with_borrow(|c| (c.session, c.revision));
    let hit = CACHE.with_borrow(|c| {
        c.as_ref()
            .filter(|c| {
                c.session == session
                    && c.revision == revision
                    && c.query == query
                    && c.match_case == match_case
            })
            .map(|c| c.matches.clone())
    });
    if let Some(m) = hit {
        return (revision, m);
    }
    let m = CORE.with_borrow(|c| find::find_all(&c.shown, &query, match_case));
    CACHE.with_borrow_mut(|c| {
        *c = Some(Cache {
            session,
            revision,
            query,
            match_case,
            matches: m.clone(),
        })
    });
    (revision, m)
}

/// The count line: "3 of 12" on a match, else "12 matches".
fn refresh_count() {
    if st().query.with_untracked(|q| q.is_empty()) {
        say("Type text to find", false);
        return;
    }
    let (_, m) = matches();
    let (a, b) = selection();
    let msg = match (m.len(), find::selected_match(&m, a, b)) {
        (0, _) => "No matches".to_string(),
        (n, Some(i)) => format!("{} of {n}", i + 1),
        (1, None) => "1 match".to_string(),
        (n, None) => format!("{n} matches"),
    };
    say(msg, false);
}

/// After a document change, re-count once typing pauses (a large file is
/// not searched per keystroke).
fn schedule_count() {
    if !matches!(mode(), Some(Mode::Find | Mode::Replace)) {
        return;
    }
    let seq = COUNT_SEQ.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    spawn_local(async move {
        ipc::sleep(250).await;
        if COUNT_SEQ.with(|c| c.get()) == seq && CORE.with_borrow(|c| !c.busy) {
            refresh_count();
        }
    });
}

/// Find Next / Previous: select the match, keep the focus in the bar.
pub fn step(forward: bool) {
    let s = st();
    if s.query.with_untracked(|q| q.is_empty()) {
        say("Type text to find", false);
        return;
    }
    let (_, m) = matches();
    if m.is_empty() {
        say("No matches", true);
        return;
    }
    let (a, b) = selection();
    let wrap = s.wrap.get_untracked();
    let r = if forward {
        find::next_match(&m, b, wrap)
    } else {
        find::prev_match(&m, a, wrap)
    };
    match r {
        Some((i, wrapped)) => {
            editor::select_quiet(m[i].0, m[i].1);
            let w = if !wrapped {
                ""
            } else if forward {
                " (wrapped to the top)"
            } else {
                " (wrapped to the bottom)"
            };
            say(format!("{} of {}{w}", i + 1, m.len()), false);
        }
        None => say(
            if forward {
                "No more matches below (wrap-around is off)"
            } else {
                "No more matches above (wrap-around is off)"
            },
            true,
        ),
    }
}

fn replacement_units() -> Vec<u16> {
    st().replacement
        .with_untracked(|r| r.encode_utf16().collect())
}

/// Replace: the selected match (exactly one) is replaced by ONE doc_edit,
/// then the next match is selected. With no match selected, it only finds.
pub async fn replace_one() {
    let s = st();
    if s.query.with_untracked(|q| q.is_empty()) {
        say("Type text to find", false);
        return;
    }
    editor::idle().await;
    let (rev, m) = matches();
    let (a, b) = selection();
    let Some(i) = find::selected_match(&m, a, b) else {
        step(true);
        return;
    };
    let rep = replacement_units();
    let len = rep.len() as u32;
    let (from, to) = m[i];
    match editor::submit_edit(rev, from, to, rep).await {
        Ok(_) => {
            let caret = from + len;
            let (_, m) = matches();
            match find::next_match(&m, caret, s.wrap.get_untracked()) {
                Some((j, _)) => {
                    editor::select_quiet(m[j].0, m[j].1);
                    say(format!("Replaced 1. {} of {}", j + 1, m.len()), false);
                }
                None => {
                    editor::select_quiet(from, caret);
                    let left = m.len();
                    say(
                        if left == 0 {
                            "Replaced 1. No more matches".to_string()
                        } else {
                            format!("Replaced 1. {left} left (wrap-around is off)")
                        },
                        false,
                    );
                }
            }
        }
        Err(e) => say(format!("Not replaced: {e}"), true),
    }
}

/// Replace All: ONE edit from the first to the last match, against the
/// revision the matches came from: one undo step.
pub async fn replace_all() {
    let s = st();
    if s.query.with_untracked(|q| q.is_empty()) {
        say("Type text to find", false);
        return;
    }
    editor::idle().await;
    let (rev, m) = matches();
    let rep = s.replacement.get_untracked();
    let Some(edit) = CORE.with_borrow(|c| find::replace_all(&c.shown, &m, &rep)) else {
        say("No matches", true);
        return;
    };
    // The backend writes inserted line breaks in the document's dominant
    // EOL: in a mixed-EOL document an edit across lines would rewrite the
    // other endings in between. Refuse rather than change bytes nobody
    // asked to change.
    let mixed = ui::ui()
        .doc
        .with_untracked(|d| d.as_ref().is_some_and(|d| d.eol_mixed));
    if mixed && CORE.with_borrow(|c| find::has_break(&c.shown, edit.0, edit.1)) {
        say(
            "Replace All is refused across lines in a mixed line-ending document (it would rewrite line endings); use Replace",
            true,
        );
        return;
    }
    let n = m.len();
    let from = edit.0;
    match editor::submit_edit(rev, edit.0, edit.1, edit.2).await {
        Ok(rev) => {
            editor::select_quiet(from, from);
            let msg = format!(
                "Replaced {n} occurrence{} (one undo step, revision {rev})",
                if n == 1 { "" } else { "s" }
            );
            ui::flash(&msg);
            say(msg, false);
        }
        Err(e) => say(format!("Nothing replaced: {e}"), true),
    }
}

/// Go to Line: move the caret to the start of the line and scroll to it.
pub fn go() {
    let s = st();
    let Some(ta) = editor::textarea() else { return };
    let text: Vec<u16> = ta.value().encode_utf16().collect();
    let total = find::line_count(&text);
    match find::parse_line(&s.line.get_untracked(), total) {
        Ok(n) => {
            let at = find::line_start(&text, n).unwrap_or(0);
            s.mode.set(None);
            editor::select(at as u64, at as u64);
            ui::flash(&format!("Line {n} of {total}"));
        }
        Err(e) => say(e, true),
    }
}

/// Escape anywhere in the bar (an input or one of its buttons) closes it.
fn on_bar_key(ev: KeyboardEvent) {
    if ev.key() == "Escape" {
        ev.prevent_default();
        ev.stop_propagation();
        close();
    }
}

fn on_key(ev: KeyboardEvent, which: Mode) {
    if ev.key() == "Enter" {
        ev.prevent_default();
        match which {
            Mode::GoTo => go(),
            Mode::Replace => spawn_local(replace_one()),
            Mode::Find => step(!ev.shift_key()),
        }
    }
}

fn value_of(ev: &web_sys::Event) -> String {
    ev.target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}

/// The bar, above the Source text. Hidden while closed.
#[component]
pub fn FindBar() -> impl IntoView {
    let s = st();
    // Re-count after each acknowledged change while the bar is open.
    Effect::new(move |_| {
        let _ = ui::ui().revision.get();
        schedule_count();
    });
    let finding = move || matches!(s.mode.get(), Some(Mode::Find | Mode::Replace));
    let replacing = move || s.mode.get() == Some(Mode::Replace);
    let going = move || s.mode.get() == Some(Mode::GoTo);
    view! {
        <div class="findbar" id="findbar" role="search" aria-label="Find in Source"
            hidden=move || s.mode.get().is_none()
            on:keydown=on_bar_key>
            <div class="findbar-row" hidden=move || !finding()>
                <input id="find-query" type="text" placeholder="Find" aria-label="Find"
                    spellcheck="false" autocomplete="off"
                    prop:value=move || s.query.get()
                    on:input=move |ev| { s.query.set(value_of(&ev)); refresh_count(); }
                    on:keydown=move |ev| on_key(ev, Mode::Find)/>
                <label title="Match case"><input id="find-case" type="checkbox"
                    prop:checked=move || s.match_case.get()
                    on:change=move |_| { s.match_case.update(|v| *v = !*v); refresh_count(); }/>"Aa"</label>
                <label title="Wrap around at the end of the document"><input id="find-wrap" type="checkbox"
                    prop:checked=move || s.wrap.get()
                    on:change=move |_| s.wrap.update(|v| *v = !*v)/>"Wrap"</label>
                <button id="find-prev" class="secondary" title="Previous match (Shift+Enter)"
                    on:click=move |_| step(false)>"↑"</button>
                <button id="find-next" class="secondary" title="Next match (Enter)"
                    on:click=move |_| step(true)>"↓"</button>
                <span class="spacer"></span>
                <button id="find-close" class="secondary" title="Close (Esc)" aria-label="Close"
                    on:click=move |_| close()>"×"</button>
            </div>
            <div class="findbar-row" hidden=move || !replacing()>
                <input id="find-replacement" type="text" placeholder="Replace with" aria-label="Replace with"
                    spellcheck="false" autocomplete="off"
                    prop:value=move || s.replacement.get()
                    on:input=move |ev| s.replacement.set(value_of(&ev))
                    on:keydown=move |ev| on_key(ev, Mode::Replace)/>
                <button id="find-replace" class="secondary" title="Replace the selected match, then find the next (Enter)"
                    on:click=move |_| spawn_local(replace_one())>"Replace"</button>
                <button id="find-replace-all" class="secondary" title="Replace every match as one undo step"
                    on:click=move |_| spawn_local(replace_all())>"Replace All"</button>
            </div>
            <div class="findbar-row" hidden=move || !going()>
                <label for="goto-line">"Go to line"</label>
                <input id="goto-line" type="text" inputmode="numeric" aria-label="Line number"
                    spellcheck="false" autocomplete="off"
                    prop:value=move || s.line.get()
                    on:input=move |ev| s.line.set(value_of(&ev))
                    on:keydown=move |ev| on_key(ev, Mode::GoTo)/>
                <button id="goto-go" class="secondary" on:click=move |_| go()>"Go"</button>
                <span class="spacer"></span>
                <button id="goto-close" class="secondary" title="Close (Esc)" aria-label="Close"
                    on:click=move |_| close()>"×"</button>
            </div>
            <div class="findbar-status" id="find-status" role="status" aria-live="polite"
                class:error=move || s.error.get()>{move || s.status.get()}</div>
        </div>
    }
}
