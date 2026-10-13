// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-editor syntax coloring (UI-SYNTAX-1).
//!
//! The `<textarea>` stays the ONLY editing control: it owns focus, caret,
//! selection, IME, clipboard and scrolling, and it still edits the Rust
//! document through `editor.rs`. Its glyphs are transparent. Behind it sits a
//! read-only `aria-hidden` `<pre>` that paints the SAME text — always the
//! widget's current value — in the same font metrics, with color spans from
//! the Rust analysis of one exact revision. The overlay takes no pointer
//! input, holds no document and never writes text anywhere.
//!
//! Safety rules:
//! * An analysis is applied only when its session, revision, view hash and
//!   view length all match the text on screen; anything else is discarded.
//! * Between a keystroke and the next analysis, spans that touch the edit are
//!   dropped and later spans are shifted over EXACTLY the unchanged text, so a
//!   color never lands on a different character. Their class may lag one
//!   analysis (the editor is then "pending").
//! * The layer is a stack of independent block CHUNKS (at most 64 lines or
//!   ~8K UTF-16 units each, always whole lines). An edit rebuilds only the
//!   chunk(s) it touches, so the browser relays out one small block instead
//!   of the whole document; later chunks only move. Only chunks near the
//!   viewport carry color spans; the rest are one plain text node each, so
//!   the DOM stays bounded.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::syntax::{self, SYNTAX_CLASSES};

use crate::editor::{self, CORE};

/// Lines colored above and below the visible lines.
const MARGIN_LINES: u32 = 80;
/// A chunk ends after this many lines, or at the first line end past
/// `CHUNK_UNITS` UTF-16 units (one long line is never split).
const CHUNK_LINES: u32 = 64;
const CHUNK_UNITS: u32 = 8_192;
/// Cap on colored elements per chunk (very long lines).
const MAX_ELEMENTS: usize = 4_000;
/// Cap on colored elements in the whole layer. Visible chunks come first,
/// then the nearest ones; beyond the budget a chunk stays plain text.
const BUDGET_ELEMENTS: usize = 16_000;
/// Cap on diagnostic underlines kept.
const MAX_MARKS: usize = 2_000;
/// Must match `.source` line-height in style.css. Used only to pick the
/// window; alignment never depends on it.
const LINE_PX: f64 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub from: u32,
    pub to: u32,
    pub class: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub from: u32,
    pub to: u32,
    /// 2 = error, 1 = warning.
    pub sev: u8,
}

/// One block of whole lines in the color layer.
struct Chunk {
    from: u32,
    to: u32,
    lines: u32,
    el: web_sys::Element,
    colored: bool,
    /// Colored elements currently in `el`.
    elements: usize,
}

#[derive(Default)]
struct Hl {
    /// The text the overlay shows: always the widget's value.
    text: Vec<u16>,
    spans: Vec<Span>,
    marks: Vec<Mark>,
    /// Spans/marks came from an analysis of exactly `text`.
    exact: bool,
    /// The (session, revision) the exact spans came from.
    from_analysis: Option<(u64, u64)>,
    chunks: Vec<Chunk>,
    /// Cached textarea (scrollTop, scrollLeft, clientHeight).
    vp: (i32, i32, i32),
    /// Test/diagnostic counters.
    stale_rejected: u64,
    renders: u64,
    last_render_ms: f64,
    last_chunks_rebuilt: usize,
    last_set_text_ms: f64,
}

thread_local! {
    static HL: RefCell<Hl> = RefCell::new(Hl::default());
}

// ---------------------------------------------------------------- pure parts

/// Split `text[from..to)` (which starts at a line start and ends at a line
/// start or the end) into chunks of whole lines: (from, to, lines).
pub fn split_chunks(text: &[u16], from: u32, to: u32) -> Vec<(u32, u32, u32)> {
    let mut out = vec![];
    let mut start = from;
    let mut lines = 0u32;
    let mut i = from;
    while i < to {
        if text[i as usize] == b'\n' as u16 {
            lines += 1;
            if lines >= CHUNK_LINES || i + 1 - start >= CHUNK_UNITS {
                out.push((start, i + 1, lines));
                start = i + 1;
                lines = 0;
            }
        }
        i += 1;
    }
    if start < to {
        // A final line without a line break is still one line.
        let last_open = text[to as usize - 1] != b'\n' as u16;
        out.push((start, to, lines + last_open as u32));
    }
    out
}

/// 1-based line and UTF-16 column of `pos`, using whole-line chunks
/// `(from, to, lines)` that tile `text`: only the chunk holding `pos` is
/// scanned, never the text before it.
pub fn line_col_in(text: &[u16], chunks: &[(u32, u32, u32)], pos: u32) -> Option<(u32, u32)> {
    if pos as usize > text.len() || chunks.is_empty() {
        return None;
    }
    let i = chunks.partition_point(|c| c.1 <= pos).min(chunks.len() - 1);
    // Every chunk before `i` ends with a line break: `lines` counts them.
    let before: u32 = chunks[..i].iter().map(|c| c.2).sum();
    let mut line = before + 1;
    let mut start = chunks[i].0;
    for k in chunks[i].0..pos {
        if text[k as usize] == b'\n' as u16 {
            line += 1;
            start = k + 1;
        }
    }
    Some((line, pos - start + 1))
}

/// Line / column of `pos` from the layer's copy of the widget text, if that
/// copy has the widget's length `len` (else `None`: the caller scans).
pub fn line_col(pos: u32, len: u32) -> Option<(u32, u32)> {
    HL.with_borrow(|h| {
        if h.text.len() != len as usize {
            return None;
        }
        let c: Vec<(u32, u32, u32)> = h.chunks.iter().map(|c| (c.from, c.to, c.lines)).collect();
        line_col_in(&h.text, &c, pos)
    })
}

/// Minimal single edit (UTF-16, surrogate-safe) turning `a` into `b`:
/// (from, old_to, new_to).
pub fn diff16(a: &[u16], b: &[u16]) -> (u32, u32, u32) {
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
    (pre as u32, (a.len() - suf) as u32, (b.len() - suf) as u32)
}

/// Carry ranges across one edit. A range that overlaps or touches the edited
/// region is dropped (its token may have changed); a range wholly after it
/// moves by the length change, so it still covers the identical characters.
/// Also returns the OLD-coordinate extent of everything dropped, which must
/// be repainted.
pub fn shift<T: Copy>(
    items: &[T],
    (from, old_to, new_to): (u32, u32, u32),
    get: impl Fn(&T) -> (u32, u32),
    set: impl Fn(&T, u32, u32) -> T,
) -> (Vec<T>, Option<(u32, u32)>) {
    let delta = new_to as i64 - old_to as i64;
    let mut dropped: Option<(u32, u32)> = None;
    let kept = items
        .iter()
        .filter_map(|it| {
            let (a, b) = get(it);
            if b < from {
                Some(*it)
            } else if a > old_to {
                Some(set(
                    it,
                    (a as i64 + delta) as u32,
                    (b as i64 + delta) as u32,
                ))
            } else {
                dropped = Some(match dropped {
                    Some((x, y)) => (x.min(a), y.max(b)),
                    None => (a, b),
                });
                None
            }
        })
        .collect();
    (kept, dropped)
}

/// One painted run: `[from, to)` with an optional class and severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seg {
    pub from: u32,
    pub to: u32,
    pub class: Option<u8>,
    pub sev: u8,
}

/// Split `[ws, we)` into runs by syntax span and diagnostic mark. Spans are
/// sorted and disjoint; marks may overlap (the strongest severity wins).
pub fn segments(spans: &[Span], marks: &[Mark], ws: u32, we: u32) -> Vec<Seg> {
    let first = spans.partition_point(|s| s.to <= ws);
    let in_spans: Vec<&Span> = spans[first..].iter().take_while(|s| s.from < we).collect();
    let in_marks: Vec<&Mark> = marks.iter().filter(|m| m.to > ws && m.from < we).collect();
    let mut cuts: Vec<u32> = vec![ws, we];
    for s in &in_spans {
        cuts.push(s.from.clamp(ws, we));
        cuts.push(s.to.clamp(ws, we));
    }
    for m in &in_marks {
        cuts.push(m.from.clamp(ws, we));
        cuts.push(m.to.clamp(ws, we));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out: Vec<Seg> = Vec::with_capacity(cuts.len());
    let mut si = 0;
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        while si < in_spans.len() && in_spans[si].to <= a {
            si += 1;
        }
        let class = in_spans
            .get(si)
            .filter(|s| s.from <= a && a < s.to)
            .map(|s| s.class);
        let sev = in_marks
            .iter()
            .filter(|m| m.from <= a && a < m.to)
            .map(|m| m.sev)
            .max()
            .unwrap_or(0);
        match out.last_mut() {
            Some(l) if l.class == class && l.sev == sev && l.to == a => l.to = b,
            _ => out.push(Seg {
                from: a,
                to: b,
                class,
                sev,
            }),
        }
    }
    out
}

/// Diagnostic ranges -> underline marks inside `len`. An empty range is
/// widened to one character (two for a surrogate pair) so it stays visible.
pub fn marks_from(diags: &[p::Diagnostic], text: &[u16]) -> Vec<Mark> {
    let len = text.len() as u32;
    let mut out = Vec::new();
    for d in diags.iter().take(MAX_MARKS) {
        let sev = match d.severity.as_str() {
            "error" => 2,
            "warning" => 1,
            _ => continue,
        };
        let from = (d.view_from.min(len as u64)) as u32;
        let mut to = (d.view_to.min(len as u64)) as u32;
        if to <= from {
            to = (from + 1).min(len);
            if to < len && (0xD800..0xDC00).contains(&text[from as usize]) {
                to += 1;
            }
        }
        if to > from {
            out.push(Mark { from, to, sev });
        }
    }
    out
}

// ---------------------------------------------------------------- DOM parts

fn doc() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn layer() -> Option<web_sys::HtmlElement> {
    doc()?.get_element_by_id("source-hl")?.dyn_into().ok()
}

fn pending_attr(exact: bool) {
    if let Some(l) = layer() {
        let _ = l.set_attribute("data-state", if exact { "exact" } else { "pending" });
    }
}

/// The textarea's scroll offsets and height, cached from scroll / resize
/// events so painting never forces a layout read in the input handler.
fn read_viewport(h: &mut Hl) {
    if let Some(ta) = editor::textarea() {
        h.vp = (ta.scroll_top(), ta.scroll_left(), ta.client_height());
    }
}

/// Keep the overlay at the textarea's scroll offset. A transform has no scroll
/// limits, so the bottom/right edges stay aligned with scrollbars present.
fn sync_scroll(h: &Hl) {
    if let Some(l) = layer() {
        let _ = l.style().set_property(
            "transform",
            &format!("translate({}px, {}px)", -h.vp.1, -h.vp.0),
        );
    }
}

/// Lines that should carry color: the visible ones plus a margin.
fn visible_lines(h: &Hl) -> (u32, u32) {
    let top = (h.vp.0 as f64 / LINE_PX).floor().max(0.0) as u32;
    let rows = (h.vp.2.max(0) as f64 / LINE_PX).ceil() as u32 + 1;
    (top, top + rows)
}

/// Which chunks carry color: the chunks touching the visible lines, then
/// the nearest ones within the margin, while the element budget lasts.
fn wanted(h: &Hl) -> Vec<bool> {
    let (vt, vb) = visible_lines(h);
    let (lo, hi) = (vt.saturating_sub(MARGIN_LINES), vb + MARGIN_LINES);
    let mut line = 0u32;
    let mut cand: Vec<(u32, usize, usize)> = vec![]; // (distance, index, cost)
    for (i, c) in h.chunks.iter().enumerate() {
        let (a, b) = (line, line + c.lines.max(1));
        if a < hi && b > lo {
            let dist = if b <= vt {
                vt - b + 1
            } else if a >= vb {
                a - vb + 1
            } else {
                0
            };
            let first = h.spans.partition_point(|s| s.to <= c.from);
            let n = h.spans[first..]
                .iter()
                .take_while(|s| s.from < c.to)
                .count();
            cand.push((dist, i, n.min(MAX_ELEMENTS)));
        }
        line += c.lines;
    }
    cand.sort_unstable();
    let mut want = vec![false; h.chunks.len()];
    let mut spent = 0usize;
    for (_, i, cost) in cand {
        // Nearest first; the first chunk is always colored.
        if spent > 0 && spent + cost > BUDGET_ELEMENTS {
            continue;
        }
        spent += cost;
        want[i] = true;
    }
    want
}

fn utf16_str(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// Fill one chunk element: plain text, or colored runs.
fn paint(h: &Hl, d: &web_sys::Document, i: usize, colored: bool) -> usize {
    let c = &h.chunks[i];
    let el = &c.el;
    let text = |a: u32, b: u32| utf16_str(&h.text[a as usize..b as usize]);
    if !colored {
        el.set_text_content(Some(&text(c.from, c.to)));
        return 0;
    }
    el.set_text_content(None);
    let mut elements = 0usize;
    let mut plain_from: Option<u32> = None;
    for s in segments(&h.spans, &h.marks, c.from, c.to) {
        let styled = s.class.is_some() || s.sev > 0;
        if !styled || elements >= MAX_ELEMENTS {
            plain_from.get_or_insert(s.from);
            continue;
        }
        if let Some(pf) = plain_from.take() {
            let _ = el.append_child(&d.create_text_node(&text(pf, s.from)));
        }
        let Ok(span) = d.create_element("span") else {
            continue;
        };
        let mut cls = String::new();
        if let Some(c) = s.class.and_then(|c| SYNTAX_CLASSES.get(c as usize)) {
            cls.push_str("tk-");
            cls.push_str(c.0);
        }
        match s.sev {
            2 => cls.push_str(" dg-error"),
            1 => cls.push_str(" dg-warning"),
            _ => {}
        }
        span.set_class_name(cls.trim_start());
        span.set_text_content(Some(&text(s.from, s.to)));
        let _ = el.append_child(&span);
        elements += 1;
    }
    if let Some(pf) = plain_from.take() {
        let _ = el.append_child(&d.create_text_node(&text(pf, c.to)));
    }
    elements
}

fn new_chunk_el(d: &web_sys::Document) -> Option<web_sys::Element> {
    let el = d.create_element("div").ok()?;
    el.set_class_name("hl-chunk");
    Some(el)
}

/// Which chunks to repaint even if their colored state is unchanged.
enum Force {
    None,
    Range(std::ops::Range<usize>),
    Colored,
    All,
}

/// (Re)paint chunks: those entering or leaving the colored range, plus
/// the `force`d ones.
fn update(h: &mut Hl, force: Force) {
    let Some(d) = doc() else { return };
    let t0 = now();
    let wanted = wanted(h);
    let mut rebuilt = 0;
    for (i, &want) in wanted.iter().enumerate() {
        let was = h.chunks[i].colored;
        let forced = match &force {
            Force::None => false,
            Force::Range(r) => r.contains(&i),
            Force::Colored => was,
            Force::All => true,
        };
        if want != was || forced {
            let n = paint(h, &d, i, want);
            h.chunks[i].colored = want;
            h.chunks[i].elements = n;
            rebuilt += 1;
        }
    }
    if rebuilt > 0 {
        h.renders += 1;
        h.last_chunks_rebuilt = rebuilt;
        h.last_render_ms = now() - t0;
    }
}

/// Rebuild every chunk from scratch (new document, or a full repaint).
fn rebuild_all(h: &mut Hl) {
    let (Some(layer), Some(d)) = (layer(), doc()) else {
        return;
    };
    layer.set_text_content(None);
    h.chunks.clear();
    for (from, to, lines) in split_chunks(&h.text, 0, h.text.len() as u32) {
        let Some(el) = new_chunk_el(&d) else { return };
        let _ = layer.append_child(&el);
        h.chunks.push(Chunk {
            from,
            to,
            lines,
            el,
            colored: false,
            elements: 0,
        });
    }
    update(h, Force::All);
    sync_scroll(h);
}

/// The widget's text changed (typing, paste, IME, undo, redo, Inspector,
/// resync). Show the new text NOW; keep only colors that provably still
/// cover the same characters.
pub fn set_text(new: &str) {
    let t0 = now();
    let exact = HL.with_borrow_mut(|h| {
        let b: Vec<u16> = new.encode_utf16().collect();
        if b == h.text {
            return h.exact;
        }
        let e = diff16(&h.text, &b);
        let (from, old_to, new_to) = e;
        let (spans, ds) = shift(
            &h.spans,
            e,
            |s| (s.from, s.to),
            |s, from, to| Span { from, to, ..*s },
        );
        let (marks, dm) = shift(
            &h.marks,
            e,
            |m| (m.from, m.to),
            |m, from, to| Mark { from, to, ..*m },
        );
        h.spans = spans;
        h.marks = marks;
        h.exact = false;
        let delta = new_to as i64 - old_to as i64;
        // OLD-coordinate range whose painting may be wrong now: the edit and
        // every dropped color.
        let mut lo = from;
        let mut hi = old_to;
        for (a, z) in [ds, dm].into_iter().flatten() {
            lo = lo.min(a);
            hi = hi.max(z);
        }
        let n = h.chunks.len();
        let (Some(layer), Some(d)) = (layer(), doc()) else {
            h.text = b;
            return false;
        };
        if n == 0 {
            h.text = b;
            rebuild_all(h);
            return false;
        }
        // Chunks [a, z] cover [lo, hi] (an edit at a chunk boundary belongs
        // to both neighbors: the line it joins may change).
        let a = h.chunks.partition_point(|c| c.to < lo).min(n - 1);
        let z = h
            .chunks
            .partition_point(|c| c.from <= hi)
            .saturating_sub(1)
            .max(a);
        let new_from = h.chunks[a].from;
        let new_to_c = (h.chunks[z].to as i64 + delta) as u32;
        h.text = b;
        let pieces = split_chunks(&h.text, new_from, new_to_c);
        let next = h.chunks.get(z + 1).map(|c| c.el.clone());
        for c in h.chunks.drain(a..=z) {
            c.el.remove();
        }
        let mut fresh = Vec::with_capacity(pieces.len());
        for (from, to, lines) in pieces {
            let Some(el) = new_chunk_el(&d) else { continue };
            let _ = layer.insert_before(&el, next.as_ref().map(|e| e.as_ref()));
            fresh.push(Chunk {
                from,
                to,
                lines,
                el,
                colored: false,
                elements: 0,
            });
        }
        let count = fresh.len();
        h.chunks.splice(a..a, fresh);
        for c in &mut h.chunks[a + count..] {
            c.from = (c.from as i64 + delta) as u32;
            c.to = (c.to as i64 + delta) as u32;
        }
        update(h, Force::Range(a..a + count));
        false
    });
    HL.with_borrow_mut(|h| h.last_set_text_ms = now() - t0);
    pending_attr(exact);
}

/// A different document: no color carries over.
pub fn reset(text: &str) {
    HL.with_borrow_mut(|h| {
        h.text = text.encode_utf16().collect();
        h.spans.clear();
        h.marks.clear();
        h.exact = false;
        h.from_analysis = None;
        read_viewport(h);
        rebuild_all(h);
    });
    pending_attr(false);
}

/// Adopt the colors of one analysis, if — and only if — it describes
/// exactly the text on screen. Returns whether it was applied.
pub fn apply(a: &p::Analysis) -> bool {
    let (session, revision) = CORE.with_borrow(|c| (c.session, c.revision));
    let ok = HL.with_borrow_mut(|h| {
        let matches = session == Some(a.session)
            && revision == a.revision
            && a.view_len == h.text.len() as u64
            && a.view_hash == p::view_hash_units(h.text.iter().copied());
        let decoded = matches
            .then(|| syntax::decode(&a.highlights, a.view_len))
            .flatten();
        match decoded {
            Some(spans) => {
                h.spans = spans
                    .into_iter()
                    .map(|s| Span {
                        from: s.from as u32,
                        to: s.to as u32,
                        class: s.class,
                    })
                    .collect();
                h.marks = marks_from(&a.diagnostics, &h.text);
                h.exact = true;
                h.from_analysis = Some((a.session, a.revision));
                // Only colored chunks show classes; plain ones are unchanged.
                update(h, Force::Colored);
                true
            }
            None => {
                h.stale_rejected += 1;
                false
            }
        }
    });
    if ok {
        pending_attr(true);
    }
    ok
}

pub fn on_scroll() {
    HL.with_borrow_mut(|h| {
        read_viewport(h);
        sync_scroll(h);
        update(h, Force::None);
    });
}

/// Window resize changes how many lines are visible.
pub fn install() {
    let cb = Closure::<dyn FnMut()>::new(on_scroll);
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
    }
    cb.forget();
}

/// Read-only facts for the smoke harness.
pub struct Probe {
    pub exact: bool,
    pub from_analysis: Option<(u64, u64)>,
    pub spans: usize,
    pub marks: usize,
    pub stale_rejected: u64,
    pub renders: u64,
    pub last_render_ms: f64,
    pub last_chunks_rebuilt: usize,
    pub last_set_text_ms: f64,
    pub chunks: usize,
    pub colored_chunks: usize,
    pub total_elements: usize,
    pub chunks_consistent: bool,
    pub text_matches_widget: bool,
}

pub fn probe() -> Probe {
    let widget: Vec<u16> = editor::textarea()
        .map(|t| t.value().encode_utf16().collect())
        .unwrap_or_default();
    let layer_text: Vec<u16> = layer()
        .and_then(|l| l.text_content())
        .map(|t| t.encode_utf16().collect())
        .unwrap_or_default();
    HL.with_borrow(|h| Probe {
        exact: h.exact,
        from_analysis: h.from_analysis,
        spans: h.spans.len(),
        marks: h.marks.len(),
        stale_rejected: h.stale_rejected,
        renders: h.renders,
        last_render_ms: h.last_render_ms,
        last_chunks_rebuilt: h.last_chunks_rebuilt,
        last_set_text_ms: h.last_set_text_ms,
        chunks: h.chunks.len(),
        colored_chunks: h.chunks.iter().filter(|c| c.colored).count(),
        total_elements: h.chunks.iter().map(|c| c.elements).sum(),
        // Chunks tile the text exactly, each ends at a line end (or EOF),
        // and each element shows exactly its slice.
        chunks_consistent: h.chunks.windows(2).all(|w| w[0].to == w[1].from)
            && h.chunks.first().map_or(h.text.is_empty(), |c| c.from == 0)
            && h.chunks
                .last()
                .is_none_or(|c| c.to as usize == h.text.len())
            && h.chunks.iter().all(|c| {
                (c.to as usize == h.text.len() || h.text[c.to as usize - 1] == b'\n' as u16)
                    && c.el
                        .text_content()
                        .map(|t| {
                            t.encode_utf16()
                                .eq(h.text[c.from as usize..c.to as usize].iter().copied())
                        })
                        .unwrap_or(false)
            }),
        text_matches_widget: widget == h.text && layer_text == h.text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn diff16_is_minimal_and_surrogate_safe() {
        assert_eq!(diff16(&u("abc"), &u("abXc")), (2, 2, 3));
        assert_eq!(diff16(&u("abc"), &u("ac")), (1, 2, 1));
        assert_eq!(diff16(&u("a😀b"), &u("a😁b")), (1, 3, 3));
        assert_eq!(diff16(&u("same"), &u("same")), (4, 4, 4));
    }

    #[test]
    fn shift_keeps_only_untouched_ranges_on_identical_characters() {
        let old = u("DEF A Box {} USE A");
        let spans = vec![
            Span {
                from: 0,
                to: 3,
                class: 2,
            },
            Span {
                from: 4,
                to: 5,
                class: 10,
            },
            Span {
                from: 6,
                to: 9,
                class: 7,
            },
            Span {
                from: 10,
                to: 11,
                class: 12,
            },
            Span {
                from: 13,
                to: 16,
                class: 2,
            },
        ];
        // Insert "xx" at offset 6 (touches `Box`).
        let new = u("DEF A xxBox {} USE A");
        let e = diff16(&old, &new);
        let (got, dropped) = shift(
            &spans,
            e,
            |s| (s.from, s.to),
            |s, from, to| Span { from, to, ..*s },
        );
        assert_eq!(dropped, Some((6, 9)));
        assert_eq!(got.len(), 4);
        let kept: Vec<&Span> = spans.iter().filter(|s| s.class != 7).collect();
        for (s, o) in got.iter().zip(kept) {
            assert_eq!(s.class, o.class);
            assert_eq!(
                &new[s.from as usize..s.to as usize],
                &old[o.from as usize..o.to as usize]
            );
        }
    }

    #[test]
    fn line_col_from_chunks_matches_a_full_scan() {
        let long = "y".repeat(9000);
        for src in ["", "a", "a\n", "\n\n", "ab\ncd😀e\n\nfg", long.as_str()] {
            let mut text = String::new();
            for i in 0..150 {
                text.push_str(src);
                text.push_str(&format!("line {i}\n"));
            }
            let t = u(&text);
            let chunks = split_chunks(&t, 0, t.len() as u32);
            assert!(chunks.len() > 1);
            // One running scan supplies the expected value at every offset.
            let (mut l, mut c) = (1, 1);
            for pos in 0..=t.len() {
                if pos % 7 == 0 || pos == t.len() {
                    assert_eq!(line_col_in(&t, &chunks, pos as u32), Some((l, c)), "{pos}");
                }
                if pos < t.len() {
                    if t[pos] == b'\n' as u16 {
                        l += 1;
                        c = 1;
                    } else {
                        c += 1;
                    }
                }
            }
            assert_eq!(line_col_in(&t, &chunks, t.len() as u32 + 1), None);
        }
    }

    #[test]
    fn chunks_are_whole_lines_and_tile_the_text() {
        let text = u(&"ab\n".repeat(200));
        let c = split_chunks(&text, 0, text.len() as u32);
        assert_eq!(c.len(), 4);
        assert_eq!(c[0], (0, 192, 64));
        assert_eq!(c[3], (576, 600, 8));
        let t2 = u("a\nb");
        assert_eq!(split_chunks(&t2, 0, 3), vec![(0, 3, 2)]);
        // One long line is never split; it closes its chunk.
        let long = u(&format!("{}\nx\n", "y".repeat(9000)));
        assert_eq!(
            split_chunks(&long, 0, long.len() as u32),
            vec![(0, 9001, 1), (9001, 9003, 1)]
        );
        assert!(split_chunks(&t2, 0, 0).is_empty());
    }

    #[test]
    fn segments_split_by_span_and_mark() {
        let spans = vec![
            Span {
                from: 0,
                to: 3,
                class: 2,
            },
            Span {
                from: 6,
                to: 9,
                class: 7,
            },
        ];
        let marks = vec![
            Mark {
                from: 2,
                to: 7,
                sev: 2,
            },
            Mark {
                from: 4,
                to: 5,
                sev: 1,
            },
        ];
        let s = segments(&spans, &marks, 0, 10);
        let want = vec![
            Seg {
                from: 0,
                to: 2,
                class: Some(2),
                sev: 0,
            },
            Seg {
                from: 2,
                to: 3,
                class: Some(2),
                sev: 2,
            },
            Seg {
                from: 3,
                to: 6,
                class: None,
                sev: 2,
            },
            Seg {
                from: 6,
                to: 7,
                class: Some(7),
                sev: 2,
            },
            Seg {
                from: 7,
                to: 9,
                class: Some(7),
                sev: 0,
            },
            Seg {
                from: 9,
                to: 10,
                class: None,
                sev: 0,
            },
        ];
        assert_eq!(s, want);
        // Covers the window exactly, contiguous.
        let s = segments(&spans, &marks, 1, 8);
        assert_eq!(s.first().unwrap().from, 1);
        assert_eq!(s.last().unwrap().to, 8);
        assert!(s.windows(2).all(|w| w[0].to == w[1].from));
    }

    #[test]
    fn empty_diagnostics_widen_and_clamp() {
        let text = u("a😀");
        let d = |f: u64, t: u64, sev: &str| p::Diagnostic {
            code: "X".into(),
            severity: sev.into(),
            message: String::new(),
            line: 1,
            column: 1,
            view_from: f,
            view_to: t,
        };
        let m = marks_from(
            &[d(1, 1, "error"), d(3, 3, "warning"), d(0, 99, "warning")],
            &text,
        );
        assert_eq!(
            m[0],
            Mark {
                from: 1,
                to: 3,
                sev: 2
            }
        );
        assert_eq!(
            m[1],
            Mark {
                from: 0,
                to: 3,
                sev: 1
            }
        );
        assert_eq!(m.len(), 2);
    }
}
