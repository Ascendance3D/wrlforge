// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-MENU-1: Edit → Cut / Copy / Paste / Select All, and the focus record
//! the menus act on.
//!
//! A menu takes the keyboard focus while it is open, so every Edit command
//! acts on the control that had the focus BEFORE (`LAST`), and gives the
//! focus back first.
//!
//! Cut and Paste change the Source text. They change it in the textarea
//! widget and then call `editor::on_input`, the same function a typed key
//! reaches: the editor diffs the widget against the text Rust confirmed and
//! sends ONE `doc_edit` (revision-checked, view-hash verified, resynced on
//! any refusal). No other path writes the document; a text field other than
//! Source is never edited from the menu (its own Ctrl+X / Ctrl+V stay).
//! `document.execCommand` is not used: its `input` event is not guaranteed.
//!
//! The clipboard answers asynchronously, and the user can move the Source
//! selection, type, or open another document while it does. Cut and Paste
//! therefore take a [`Ticket`] BEFORE the clipboard call (document session,
//! revision and generation, the target textarea, its selection and its
//! text) and edit only if the ticket still holds AFTER it. A stale answer
//! changes nothing: no widget text, no `doc_edit`, no undo entry.

use std::cell::{Cell, RefCell};

use js_sys::{JsString, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Element, HtmlElement, HtmlInputElement, HtmlTextAreaElement};
use wrlforge_desktop_protocol::command::FocusKind;

use crate::{editor, ipc, menu, ui};

thread_local! {
    /// The last focused control outside the menu bar (None: nothing).
    static LAST: RefCell<Option<HtmlElement>> = const { RefCell::new(None) };
    /// Test only: a controlled clipboard answer (None in production).
    static HOLD: Cell<Option<TestHold>> = const { Cell::new(None) };
    static STATS: Cell<Stats> = const { Cell::new(Stats { applied: 0, stale: 0 }) };
}

/// Test only (smoke): delay every clipboard answer by `delay_ms`, and fail
/// it (as a canceled or refused clipboard does) when `fail` is set. The real
/// clipboard and its permissions are unchanged.
#[derive(Clone, Copy, Debug)]
pub struct TestHold {
    pub delay_ms: i32,
    pub fail: bool,
}

pub fn set_test_hold(h: Option<TestHold>) {
    HOLD.with(|c| c.set(h));
}

/// Cut / Paste edits applied, and refused as stale (test counters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub applied: u64,
    pub stale: u64,
}

pub fn stats() -> Stats {
    STATS.with(|c| c.get())
}

fn count(applied: bool) {
    STATS.with(|c| {
        let mut s = c.get();
        if applied {
            s.applied += 1;
        } else {
            s.stale += 1;
        }
        c.set(s);
    });
}

/// The test hold, before the clipboard call: `Err` when it must fail.
async fn hold_before() -> Result<(), String> {
    match HOLD.with(|c| c.get()) {
        Some(h) if h.fail => {
            ipc::sleep(h.delay_ms).await;
            Err("the clipboard request was canceled (test)".into())
        }
        _ => Ok(()),
    }
}

/// The test hold, after the clipboard answered: the late response.
async fn hold_after() {
    if let Some(h) = HOLD.with(|c| c.get()) {
        ipc::sleep(h.delay_ms).await;
    }
}

/// What a control is, for the registry's focus requirements.
pub fn focus_kind(el: Option<Element>) -> FocusKind {
    let Some(el) = el else {
        return FocusKind::Other;
    };
    if el.id() == "source" {
        return FocusKind::Source;
    }
    if el.is_instance_of::<HtmlTextAreaElement>() {
        return FocusKind::TextField;
    }
    match el.dyn_ref::<HtmlInputElement>() {
        Some(i) if text_input(i) => FocusKind::TextField,
        _ => FocusKind::Other,
    }
}

fn text_input(i: &HtmlInputElement) -> bool {
    matches!(
        i.type_().as_str(),
        "text" | "search" | "number" | "url" | "email" | "tel" | "password" | ""
    )
}

/// The recorded control's kind, and whether it has a text selection.
pub fn focus_snapshot() -> (FocusKind, bool) {
    let el = last();
    let kind = focus_kind(el.clone().map(Into::into));
    let selected = el.as_ref().and_then(selection).is_some_and(|(a, b)| b > a);
    (kind, selected)
}

/// The recorded control, if it is still in the document.
pub fn last() -> Option<HtmlElement> {
    LAST.with_borrow(|l| l.clone()).filter(|e| e.is_connected())
}

/// `focusin` anywhere outside the menu bar: remember the control.
pub fn note_focus(el: Option<HtmlElement>) {
    LAST.with_borrow_mut(|l| *l = el);
    menu::bump_focus();
}

/// Give the focus back to the recorded control (after a menu closes).
pub fn restore_focus() {
    if let Some(el) = last() {
        let _ = el.focus();
    }
}

/// Selection offsets (UTF-16) of a textarea or a text input.
fn selection(el: &HtmlElement) -> Option<(u32, u32)> {
    if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
        return Some((t.selection_start().ok()??, t.selection_end().ok()??));
    }
    let i = el.dyn_ref::<HtmlInputElement>()?;
    if !text_input(i) {
        return None;
    }
    Some((i.selection_start().ok()??, i.selection_end().ok()??))
}

fn value(el: &HtmlElement) -> Option<String> {
    if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
        return Some(t.value());
    }
    el.dyn_ref::<HtmlInputElement>().map(|i| i.value())
}

/// The selected text (UTF-16 offsets, as the DOM reports them).
fn selected_text(el: &HtmlElement) -> Option<String> {
    let (a, b) = selection(el)?;
    let v = value(el)?;
    (b > a).then(|| String::from(JsString::from(v).substring(a, b)))
}

fn clipboard() -> Option<JsValue> {
    let nav = web_sys::window()?.navigator();
    let c = Reflect::get(&nav, &"clipboard".into()).ok()?;
    (!c.is_undefined() && !c.is_null()).then_some(c)
}

async fn call_promise(target: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let f = Reflect::get(target, &method.into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| format!("navigator.clipboard.{method} is not available"))?;
    let p = match args {
        [] => f.call0(target),
        [a] => f.call1(target, a),
        _ => return Err("bad call".into()),
    }
    .map_err(|e| format!("{e:?}"))?;
    let p: Promise = p.dyn_into().map_err(|_| "not a promise".to_string())?;
    JsFuture::from(p).await.map_err(|e| {
        Reflect::get(&e, &"message".into())
            .ok()
            .and_then(|m| m.as_string())
            .unwrap_or_else(|| format!("{e:?}"))
    })
}

/// Write `text` to the system clipboard.
pub async fn write_text(text: &str) -> Result<(), String> {
    hold_before().await?;
    let c = clipboard().ok_or("the clipboard API is not available")?;
    let r = call_promise(&c, "writeText", &[JsValue::from_str(text)])
        .await
        .map(|_| ());
    hold_after().await;
    r
}

/// Read text from the system clipboard.
pub async fn read_text() -> Result<String, String> {
    hold_before().await?;
    let c = clipboard().ok_or("the clipboard API is not available")?;
    let r = call_promise(&c, "readText", &[]).await;
    hold_after().await;
    r?.as_string()
        .ok_or_else(|| "the clipboard holds no text".into())
}

fn source() -> Option<HtmlTextAreaElement> {
    let el = last()?;
    (el.id() == "source").then(|| el.dyn_into().ok()).flatten()
}

/// What a Cut or Paste was asked for, taken before the clipboard call.
#[derive(Clone, PartialEq)]
struct Ticket {
    ta: HtmlTextAreaElement,
    session: u64,
    revision: u64,
    generation: u64,
    selection: (u32, u32),
    text: String,
}

impl Ticket {
    /// The Source state now. None: no document, an edit is still in flight,
    /// or the widget holds text Rust has not acknowledged.
    fn take() -> Option<Ticket> {
        let ta = source()?;
        let text = ta.value();
        let (session, revision, generation, settled) = editor::CORE.with_borrow(|c| {
            let settled = !c.busy && c.shown.iter().copied().eq(text.encode_utf16());
            (c.session, c.revision, c.generation, settled)
        });
        let el: &HtmlElement = ta.unchecked_ref();
        let selection = selection(el)?;
        Some(Ticket {
            session: session?,
            revision,
            generation,
            selection,
            text,
            ta,
        })
        .filter(|_| settled)
    }

    /// Wait for a pending source edit, then take the ticket.
    async fn take_settled() -> Option<Ticket> {
        editor::idle().await;
        Ticket::take()
    }

    /// The ticket still describes the Source the user sees: same control,
    /// same document and revision, same selection, same text.
    fn holds(&self) -> bool {
        Ticket::take().is_some_and(|now| now == *self)
    }
}

/// Replace the ticket's selection with `text` in the widget, then hand the
/// change to the editor's edit path exactly as a typed key does. Refused
/// (nothing changes) when the ticket no longer holds.
fn replace_source_selection(t: &Ticket, text: &str) -> Result<(), String> {
    if t.ta.read_only() {
        return Err("the Source editor is read-only".into());
    }
    if !t.holds() {
        count(false);
        return Err("the Source selection or document changed while the clipboard answered".into());
    }
    let (a, b) = t.selection;
    t.ta.set_range_text_with_start_and_end_and_mode(text, a, b, "end")
        .map_err(|e| format!("{e:?}"))?;
    count(true);
    editor::on_input();
    editor::update_cursor();
    Ok(())
}

pub async fn copy() {
    restore_focus();
    let Some(el) = last() else { return };
    let Some(text) = selected_text(&el) else {
        ui::flash("Copy: nothing is selected.");
        return;
    };
    match write_text(&text).await {
        Ok(()) => ui::flash(&format!("Copied {} characters.", text.chars().count())),
        Err(e) => ui::flash(&format!("Copy is not available: {e}. Use Ctrl+C.")),
    }
}

pub async fn cut() {
    restore_focus();
    if source().is_none() {
        return;
    }
    let Some(t) = Ticket::take_settled().await else {
        ui::flash("Cut refused: a Source edit is still being applied.");
        return;
    };
    let (a, b) = t.selection;
    if b <= a {
        ui::flash("Cut: nothing is selected.");
        return;
    }
    let text = String::from(JsString::from(t.text.as_str()).substring(a, b));
    // The text is removed only after the clipboard holds it.
    if let Err(e) = write_text(&text).await {
        ui::flash(&format!("Cut is not available: {e}. Use Ctrl+X."));
        return;
    }
    if let Err(e) = replace_source_selection(&t, "") {
        ui::flash(&format!("Cut refused: {e}."));
    }
}

pub async fn paste() {
    restore_focus();
    if source().is_none() {
        return;
    }
    let Some(t) = Ticket::take_settled().await else {
        ui::flash("Paste refused: a Source edit is still being applied.");
        return;
    };
    match read_text().await {
        Ok(text) => {
            if let Err(e) = replace_source_selection(&t, &text) {
                ui::flash(&format!("Paste refused: {e}."));
            }
        }
        Err(e) => ui::flash(&format!(
            "Paste is not available from the menu: {e}. Use Ctrl+V."
        )),
    }
}

pub fn select_all() {
    restore_focus();
    let Some(el) = last() else { return };
    if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
        t.select();
        if t.id() == "source" {
            editor::update_cursor();
        }
    } else if let Some(i) = el.dyn_ref::<HtmlInputElement>() {
        i.select();
    }
    menu::bump_focus();
}
