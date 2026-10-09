// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-SYNTAX-1 smoke steps: syntax colours from the Rust analysis, layer
//! registration, editing through the real textarea, stale-reply rejection,
//! diagnostics marks and theme recolouring.

use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement};
use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::syntax::SYNTAX_CLASSES;

use super::{computed, doc_el, hex_rgb, key, settle, snapshot, token, R};
use crate::editor::{textarea, CORE};
use crate::ipc;
use crate::syntax;
use crate::ui::{self, ui};

fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// Wait (bounded) until the colour layer holds an exact analysis of the
/// current session + revision. Uses the normal debounced scheduling.
async fn wait_exact() -> Option<f64> {
    let t0 = now();
    for _ in 0..250 {
        let (s, rev) = CORE.with_borrow(|c| (c.session, c.revision));
        let pr = syntax::probe();
        if pr.exact && s.is_some() && pr.from_analysis == Some((s.unwrap_or(0), rev)) {
            return Some(now() - t0);
        }
        ipc::sleep(20).await;
    }
    None
}

fn layer_spans() -> Vec<Element> {
    let mut out = vec![];
    if let Some(list) = doc_el().and_then(|d| d.query_selector_all("#source-hl span").ok()) {
        for i in 0..list.length() {
            if let Some(e) = list.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                out.push(e);
            }
        }
    }
    out
}

fn tk_class(e: &Element) -> Option<String> {
    e.class_name()
        .split_whitespace()
        .find_map(|c| c.strip_prefix("tk-").map(str::to_string))
}

fn class_of_text(text: &str) -> Option<String> {
    layer_spans()
        .into_iter()
        .find(|e| e.text_content().as_deref() == Some(text))
        .and_then(|e| tk_class(&e))
}

/// Every rendered class paints with its mapped theme token.
fn classes_match_tokens() -> (bool, usize, String) {
    let mut seen: Vec<String> = vec![];
    let mut bad = vec![];
    for e in layer_spans() {
        let Some(c) = tk_class(&e) else { continue };
        if seen.contains(&c) {
            continue;
        }
        let Some((_, tok)) = SYNTAX_CLASSES.iter().find(|(n, _)| *n == c) else {
            bad.push(format!("unknown class {c}"));
            continue;
        };
        let want = hex_rgb(&token(tok));
        let got = computed(&e, "color");
        if got != want {
            bad.push(format!("{c}: {got} != {tok} {want}"));
        }
        seen.push(c);
    }
    seen.sort();
    (
        bad.is_empty(),
        seen.len(),
        format!("{} | {}", seen.join(","), bad.join("; ")),
    )
}

fn type_at(at: u32, text: &str, paste: bool) -> Option<f64> {
    let ta = textarea()?;
    let _ = ta.set_selection_range(at, at);
    // Like real typing / pasting: the caret ends after the inserted text.
    ta.set_range_text_with_start_and_end_and_mode(text, at, at, "end")
        .ok()?;
    let init = web_sys::InputEventInit::new();
    init.set_input_type(if paste {
        "insertFromPaste"
    } else {
        "insertText"
    });
    init.set_bubbles(true);
    let ev = web_sys::InputEvent::new_with_event_init_dict("input", &init).ok()?;
    let t0 = now();
    ta.dispatch_event(&ev).ok()?;
    // Include the layout the browser must do for this keystroke (textarea
    // and colour layer), so the figure is the real synchronous cost.
    let _ = ta.scroll_height();
    let _ = crate::element_by_id::<HtmlElement>("source-hl").map(|l| l.offset_height());
    Some(now() - t0)
}

/// Press Ctrl+Z (or Ctrl+Shift+Z) and wait until Rust's answer is adopted
/// (the revision moves), so a slow document never gets a second press.
async fn history_key(redo: bool) {
    let before = CORE.with_borrow(|c| c.revision);
    key("z", redo);
    for _ in 0..500 {
        ipc::sleep(10).await;
        if CORE.with_borrow(|c| c.revision) != before {
            break;
        }
    }
    settle().await;
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// The rendered position of a VIEW offset in the colour layer (client px).
fn layer_rect_at(offset: u32) -> Option<web_sys::DomRect> {
    let layer = doc_el()?.get_element_by_id("source-hl")?;
    // Walk text nodes in order, counting UTF-16 units.
    let walker = doc_el()?
        .create_tree_walker_with_what_to_show(&layer, 4)
        .ok()?;
    let mut acc = 0u32;
    while let Ok(Some(n)) = walker.next_node() {
        let len = n.text_content().map(|t| utf16_len(&t)).unwrap_or(0);
        if offset < acc + len {
            let range = doc_el()?.create_range().ok()?;
            range.set_start(&n, offset - acc).ok()?;
            range.set_end(&n, offset - acc + 1).ok()?;
            // WebKit adds a zero-width rect at the end of the previous line
            // for a range that starts right after a line break; the glyph's
            // own box is the last client rect.
            let rects = range.get_client_rects()?;
            return rects.item(rects.length().checked_sub(1)?);
        }
        acc += len;
    }
    None
}

fn px(s: &str) -> f64 {
    s.trim_end_matches("px").parse().unwrap_or(f64::NAN)
}

/// A character of a non-empty line at or after `k` sits in the layer exactly
/// where the textarea's box model puts it: padding + line * line-height -
/// scrollTop vertically, and padding + column * advance - scrollLeft
/// horizontally. The column is `col` when the text before it is plain
/// single-width ASCII (so its x is computable), else 0.
fn registration(view: &str, k: usize, col: u32) -> Option<(bool, String)> {
    let ta = textarea()?;
    let wrap = doc_el()?.query_selector(".source-wrap").ok()??;
    let w = wrap.get_bounding_client_rect();
    let units: Vec<u16> = view.encode_utf16().collect();
    let mut starts = vec![0u32];
    starts.extend(
        units
            .iter()
            .enumerate()
            .filter(|(_, u)| **u == b'\n' as u16)
            .map(|(i, _)| i as u32 + 1),
    );
    let line = (k..starts.len().min(k + 60)).find(|&i| {
        let at = starts[i] as usize;
        at < units.len() && units[at] != b'\n' as u16
    })?;
    let at = starts[line];
    let end = units[at as usize..]
        .iter()
        .position(|u| *u == b'\n' as u16)
        .map_or(units.len() as u32, |p| at + p as u32);
    let plain = |c: u32| {
        units[at as usize..(at + c) as usize]
            .iter()
            .all(|u| (0x21..0x7f).contains(u) || *u == 0x20)
    };
    let col = if at + col < end && plain(col) { col } else { 0 };
    let first = layer_rect_at(at)?;
    let rect = layer_rect_at(at + col)?;
    let pad_top = px(&computed(ta.as_ref(), "padding-top"));
    let pad_left = px(&computed(ta.as_ref(), "padding-left"));
    let lh = px(&computed(ta.as_ref(), "line-height"));
    let want_top = w.top() + pad_top + line as f64 * lh - ta.scroll_top() as f64;
    let want_left = w.left() + pad_left + col as f64 * first.width() - ta.scroll_left() as f64;
    let dy = rect.top() - want_top;
    let dx = rect.left() - want_left;
    // The glyph box sits inside the line box (half-leading), never outside.
    let ok = dy >= -0.5 && dy + rect.height() <= lh + 0.5 && dx.abs() <= 0.75;
    Some((
        ok,
        format!(
            "line {line} col {col}: dx {dx:.2}px dy {dy:.2}px h {:.1}",
            rect.height()
        ),
    ))
}

pub async fn syntax_steps(plan: &p::SmokePlan, r: &mut R) -> Option<()> {
    let ta = textarea()?;
    // ---- 1. initial colours from the Rust parse of this revision --------
    let waited = wait_exact().await;
    let pr = syntax::probe();
    r.step(
        "syntax: colours come from the Rust analysis of the current session + revision",
        waited.is_some(),
        format!(
            "{} spans, {} marks, analysis {:?}, waited {:.0} ms",
            pr.spans,
            pr.marks,
            pr.from_analysis,
            waited.unwrap_or(-1.0)
        ),
    );
    r.step(
        "syntax: colour layer shows exactly the widget text, in whole-line chunks",
        pr.text_matches_widget && pr.chunks_consistent,
        format!("{} chunks", pr.chunks),
    );
    let (ok, n, detail) = classes_match_tokens();
    r.step(
        "syntax: several syntax classes are visible, each painted by its theme token",
        ok && n >= 4,
        format!("{n} classes: {detail}"),
    );

    // ---- 2. one control, identical metrics, no input interception -------
    let layer: HtmlElement = crate::element_by_id("source-hl")?;
    let props = [
        "font-family",
        "font-size",
        "line-height",
        "tab-size",
        "padding-top",
        "padding-left",
        "letter-spacing",
        "word-spacing",
        "white-space",
        "font-weight",
        "font-style",
        "font-variant-ligatures",
        "font-kerning",
        "text-transform",
    ];
    let diff: Vec<String> = props
        .iter()
        .filter(|p| computed(ta.as_ref(), p) != computed(layer.as_ref(), p))
        .map(|p| {
            format!(
                "{p}: {} vs {}",
                computed(ta.as_ref(), p),
                computed(layer.as_ref(), p)
            )
        })
        .collect();
    r.step(
        "syntax: textarea and colour layer share every text-metric property",
        diff.is_empty(),
        diff.join("; "),
    );
    let wrap = doc_el()?.query_selector(".source-wrap").ok()??;
    let wr = wrap.get_bounding_client_rect();
    let hit = doc_el()?.element_from_point(
        (wr.left() + wr.width() / 2.0) as f32,
        (wr.top() + wr.height() / 2.0) as f32,
    );
    let focusable = doc_el()?
        .query_selector_all(".source-wrap textarea, .source-wrap [tabindex], .source-wrap input")
        .ok()?
        .length();
    r.step(
        "syntax: layer is aria-hidden, takes no pointer input; the textarea is the one control",
        layer.get_attribute("aria-hidden").as_deref() == Some("true")
            && computed(layer.as_ref(), "pointer-events") == "none"
            && hit.as_ref().map(|e| e.id()) == Some("source".into())
            && focusable == 1
            && ta.get_attribute("aria-label").is_some(),
        format!("hit {:?}, controls {focusable}", hit.map(|e| e.id())),
    );

    // ---- 3. scroll registration -----------------------------------------
    let view = ta.value();
    let lines = view.split('\n').count();
    let pad =
        px(&computed(ta.as_ref(), "padding-top")) + px(&computed(ta.as_ref(), "padding-bottom"));
    let lh = px(&computed(ta.as_ref(), "line-height"));
    let want_h = lines as f64 * lh + pad;
    let got_h = ta.scroll_height() as f64;
    r.step(
        "syntax: every textarea line box is exactly one line-height (uniform rows)",
        got_h < ta.client_height() as f64 + 1.0 || (got_h - want_h).abs() <= 2.0,
        format!("scrollHeight {got_h}, {lines} lines x {lh} + {pad} = {want_h}"),
    );
    let mut reg = vec![];
    let mut reg_ok = true;
    let max_top = (ta.scroll_height() - ta.client_height()).max(0);
    let max_left = (ta.scroll_width() - ta.client_width()).max(0);
    // The longest line, for a far-right check where text really is visible.
    let (long_k, long_len) = view
        .split('\n')
        .enumerate()
        .map(|(i, l)| (i, l.encode_utf16().count()))
        .max_by_key(|(_, n)| *n)
        .unwrap_or((0, 0));
    let char_w = layer_rect_at(0).map(|r| r.width()).unwrap_or(7.8).max(1.0);
    let states = [
        (0, 0, None),
        (max_top / 2, 200.min(max_left), None),
        (max_top, max_left / 3, None),
        (
            (long_k as i32 * lh as i32 - 60).clamp(0, max_top),
            max_left,
            Some(long_k),
        ),
    ];
    for (top, left, at_line) in states {
        ta.set_scroll_top(top);
        ta.set_scroll_left(left);
        ipc::sleep(60).await;
        let k = at_line.unwrap_or_else(|| {
            ((ta.scroll_top() as f64 / lh).ceil() as usize + 2).min(lines.saturating_sub(1))
        });
        // A column inside the visible part of the line.
        let col = ((ta.scroll_left() as f64 + 40.0) / char_w) as u32;
        let col = col.min(long_len as u32);
        let (ok, d) = registration(&view, k, col)?;
        let t = computed(layer.as_ref(), "transform");
        let want_t = format!(
            "matrix(1, 0, 0, 1, {}, {})",
            -ta.scroll_left(),
            -ta.scroll_top()
        );
        let t_ok = t == want_t || (ta.scroll_left() == 0 && ta.scroll_top() == 0 && t == "none");
        reg_ok &= ok && t_ok;
        reg.push(format!(
            "[scroll {},{}: {d}; {t}]",
            ta.scroll_top(),
            ta.scroll_left()
        ));
        if plan.syntax_align_hold_ms > 0 {
            let _ = wrap.set_attribute("data-align-check", "");
            ipc::sleep(plan.syntax_align_hold_ms as _).await;
            let _ = wrap.remove_attribute("data-align-check");
        }
    }
    r.step(
        "syntax: layer stays registered with the textarea at top, middle, bottom and far-right scroll",
        reg_ok,
        reg.join(" "),
    );
    ta.set_scroll_top(0);
    ta.set_scroll_left(0);
    ipc::sleep(40).await;

    // ---- 4. typing: painted at once, recoloured from the new revision ---
    let base = ta.value();
    let base_rev = CORE.with_borrow(|c| c.revision);
    let at = base
        .encode_utf16()
        .position(|u| u == b'\n' as u16)
        .unwrap_or(base.encode_utf16().count()) as u32;
    let long = format!("# long {}", "x".repeat(900));
    let probe_text = format!(
        "\n# syntax-probe é😀\nDEF SyntaxProbe Transform {{ translation 1 -2.5 3e1 }}\n{long}\n"
    );
    let ms = type_at(at, &probe_text, false)?;
    let pr = syntax::probe();
    r.step(
        "syntax: typed text is painted immediately, marked pending until re-analysed",
        pr.text_matches_widget && pr.chunks_consistent && !pr.exact,
        format!(
            "input + layout {ms:.1} ms, colour-layer Rust work {:.1} ms ({} chunk(s) rebuilt)",
            pr.last_set_text_ms, pr.last_chunks_rebuilt
        ),
    );
    settle().await;
    let waited = wait_exact().await;
    let rev = CORE.with_borrow(|c| c.revision);
    let classes = [
        ("# syntax-probe é😀", "comment"),
        ("DEF", "keyword"),
        ("SyntaxProbe", "def-name"),
        ("Transform", "node-type"),
        ("translation", "field"),
        ("-2.5", "number"),
        ("3e1", "number"),
    ];
    let wrong: Vec<String> = classes
        .iter()
        .filter(|(t, c)| class_of_text(t).as_deref() != Some(*c))
        .map(|(t, c)| format!("{t}: {:?} != {c}", class_of_text(t)))
        .collect();
    r.step(
        "syntax: after the edit, Rust re-analysed the new revision and the new tokens are coloured correctly",
        waited.is_some() && rev > base_rev && wrong.is_empty() && (syntax::probe().text_matches_widget && syntax::probe().chunks_consistent),
        format!(
            "rev {base_rev} -> {rev}, exact after {:.0} ms; {}",
            waited.unwrap_or(-1.0),
            wrong.join("; ")
        ),
    );
    let caret = ta.selection_start().ok().flatten();
    r.step(
        "syntax: caret sits right after the typed text (native textarea caret)",
        caret == Some(at + utf16_len(&probe_text)),
        format!("{caret:?}"),
    );

    // ---- 5. rapid typing + paste ----------------------------------------
    let mut times = vec![];
    let mut pos = at + utf16_len(&probe_text);
    for ch in "Group{children[Shape{}]}".chars() {
        let s = ch.to_string();
        times.push(type_at(pos, &s, false)?);
        pos += utf16_len(&s);
    }
    times.push(type_at(pos, " # pasted 😀 text\n", true)?);
    settle().await;
    let waited = wait_exact().await;
    let snap = snapshot().await?;
    let max = times.iter().cloned().fold(0.0, f64::max);
    let avg = times.iter().sum::<f64>() / times.len() as f64;
    r.step(
        "syntax: 25 rapid inputs + a paste reach Rust exactly; colours catch up to the final revision",
        snap.view == ta.value() && waited.is_some() && (syntax::probe().text_matches_widget && syntax::probe().chunks_consistent),
        format!(
            "input handler avg {avg:.1} ms, max {max:.1} ms; exact {:.0} ms after settle",
            waited.unwrap_or(-1.0)
        ),
    );

    // ---- 6. stale replies are refused -----------------------------------
    let a = ui().analysis.get_untracked()?;
    let before = syntax::probe();
    let mut old = a.clone();
    old.revision = a.revision.saturating_sub(1);
    let mut other = a.clone();
    other.session = a.session + 1000;
    let mut wrong_text = a.clone();
    wrong_text.view_hash ^= 1;
    let refused = !syntax::apply(&old) && !syntax::apply(&other) && !syntax::apply(&wrong_text);
    let after = syntax::probe();
    r.step(
        "syntax: replies for an older revision, another session or other text are rejected",
        refused
            && after.stale_rejected == before.stale_rejected + 3
            && after.spans == before.spans
            && after.from_analysis == before.from_analysis,
        format!(
            "rejected {} -> {}",
            before.stale_rejected, after.stale_rejected
        ),
    );

    // ---- 7. malformed source: still editable, diagnostics marked --------
    let end = utf16_len(&ta.value());
    type_at(end, "\nTransform { translation 1 2 \"open", false)?;
    settle().await;
    let waited = wait_exact().await;
    // The error is at the end: scroll there so its line is in the window.
    ta.set_scroll_top(ta.scroll_height());
    ipc::sleep(80).await;
    let marks = doc_el()?
        .query_selector_all("#source-hl .dg-error, #source-hl .dg-warning")
        .ok()?
        .length();
    let invalid = layer_spans()
        .iter()
        .any(|e| tk_class(e).as_deref() == Some("invalid"));
    ta.set_scroll_top(0);
    ipc::sleep(40).await;
    let head = doc_el()?
        .get_element_by_id("diag-head")
        .and_then(|e| e.get_attribute("data-revision"));
    let rev = CORE.with_borrow(|c| c.revision);
    let diags = ui()
        .analysis
        .get_untracked()
        .map(|a| a.diagnostics.len())
        .unwrap_or(0);
    r.step(
        "syntax: malformed source stays editable; diagnostics and underlines use the same revision",
        waited.is_some() && diags > 0 && marks > 0 && head == Some(rev.to_string()),
        format!("{diags} diagnostics, {marks} underline runs, invalid token shown: {invalid}, panel rev {head:?}, doc rev {rev}"),
    );
    let more = utf16_len(&ta.value());
    type_at(more, "x", false)?;
    settle().await;
    let snap = snapshot().await?;
    r.step(
        "syntax: an edit after the syntax error still reaches Rust",
        snap.view == ta.value(),
        "",
    );

    // ---- 8. undo back to the starting text; redo; undo ------------------
    for _ in 0..80 {
        if ta.value() == base || !ui().can_undo.get_untracked() {
            break;
        }
        history_key(false).await;
    }
    let waited = wait_exact().await;
    r.step(
        "syntax: Undo restores the exact text and its colours",
        ta.value() == base
            && waited.is_some()
            && (syntax::probe().text_matches_widget && syntax::probe().chunks_consistent),
        format!("exact {:.0} ms", waited.unwrap_or(-1.0)),
    );
    history_key(true).await;
    let redo_ok = wait_exact().await.is_some() && ta.value() != base;
    history_key(false).await;
    let undo_ok = wait_exact().await.is_some() && ta.value() == base;
    r.step(
        "syntax: Redo then Undo recolour each revision exactly",
        redo_ok
            && undo_ok
            && (syntax::probe().text_matches_widget && syntax::probe().chunks_consistent),
        "",
    );

    // ---- 9. theme change recolours without touching the document --------
    let pre_rev = CORE.with_borrow(|c| c.revision);
    let pre_doc = snapshot().await?;
    let pre = syntax::probe();
    let sel = (
        ta.selection_start().ok().flatten(),
        ta.selection_end().ok().flatten(),
    );
    let original = crate::theme::theme().id.get_untracked();
    let mut theme_rows = vec![];
    let mut theme_ok = true;
    use leptos::prelude::*;
    for t in wrlforge_desktop_protocol::theme::THEMES
        .iter()
        .chain(wrlforge_desktop_protocol::theme::find(&original))
    {
        crate::theme::select(t.id.to_string());
        let (ok, n, d) = classes_match_tokens();
        let kw = hex_rgb(&token("--wf-syntax-keyword"));
        theme_ok &= ok && n >= 4;
        theme_rows.push(format!("{}: {n} classes ok={ok} keyword {kw} {d}", t.id));
    }
    ipc::sleep(400).await;
    let post = syntax::probe();
    let post_doc = snapshot().await?;
    r.step(
        "syntax: each Tokyo Night theme recolours the same tokens with its own tokens",
        theme_ok,
        theme_rows.join(" | "),
    );
    r.step(
        "syntax: a theme change re-renders nothing, reparses nothing, edits nothing",
        post.renders == pre.renders
            && post.from_analysis == pre.from_analysis
            && CORE.with_borrow(|c| c.revision) == pre_rev
            && post_doc.view == pre_doc.view
            && post_doc.dirty == pre_doc.dirty
            && post_doc.can_undo == pre_doc.can_undo
            && (
                ta.selection_start().ok().flatten(),
                ta.selection_end().ok().flatten(),
            ) == sel,
        format!("renders {} -> {}", pre.renders, post.renders),
    );

    // ---- 10. size and timing of the real document -----------------------
    ta.set_scroll_top(ta.scroll_height() / 2);
    ipc::sleep(80).await;
    let t0 = now();
    ui::analyze().await;
    let rt = now() - t0;
    let pr = syntax::probe();
    let a = ui().analysis.get_untracked()?;
    r.step(
        "syntax: performance on this document",
        pr.total_elements <= 16_000,
        format!(
            "{} UTF-16 units, {} lines, {} spans ({} wire numbers), analyze round trip {rt:.0} ms, last render {:.1} ms; {} of {} chunks coloured, {} coloured elements in the DOM",
            utf16_len(&ta.value()),
            lines,
            pr.spans,
            a.highlights.len(),
            pr.last_render_ms,
            pr.coloured_chunks,
            pr.chunks,
            pr.total_elements
        ),
    );
    ta.set_scroll_top(0);
    ipc::sleep(80).await;

    // Per-keystroke synchronous cost (handler + layout), with the colour
    // layer shown vs. not laid out, so the layer's own share is visible.
    let base = ta.value();
    let at = base
        .encode_utf16()
        .position(|u| u == b'\n' as u16)
        .map(|i| i as u32 + 1)
        .unwrap_or(0);
    let mut shown = vec![];
    for _ in 0..5 {
        shown.push(type_at(at, "a", false)?);
        settle().await;
    }
    let _ = layer.style().set_property("display", "none");
    let mut hidden = vec![];
    for _ in 0..5 {
        hidden.push(type_at(at, "a", false)?);
        settle().await;
    }
    let _ = layer.style().remove_property("display");
    for _ in 0..40 {
        if ta.value() == base || !ui().can_undo.get_untracked() {
            break;
        }
        history_key(false).await;
    }
    let med = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let back = wait_exact().await.is_some() && ta.value() == base;
    r.step(
        "syntax: keystroke cost with the colour layer vs. without it (text restored after)",
        back,
        format!(
            "median {:.0} ms with layer, {:.0} ms with layer not laid out",
            med(&mut shown),
            med(&mut hidden)
        ),
    );
    Some(())
}

/// Reload from disk (the saved file): a fresh document view, colours from
/// nothing carried over, then a fresh exact analysis.
pub async fn reload_steps(r: &mut R) -> Option<()> {
    let ta = textarea()?;
    let before = CORE.with_borrow(|c| c.revision);
    ui::reload();
    for _ in 0..200 {
        ipc::sleep(20).await;
        if CORE.with_borrow(|c| c.revision) != before {
            break;
        }
    }
    let waited = wait_exact().await;
    let snap = snapshot().await?;
    let pr = syntax::probe();
    let (ok, n, _) = classes_match_tokens();
    r.step(
        "syntax: Reload from disk repaints the reloaded document from a fresh analysis",
        waited.is_some()
            && snap.view == ta.value()
            && !snap.dirty
            && pr.text_matches_widget
            && pr.chunks_consistent
            && ok
            && n >= 4,
        format!("rev {before} -> {}, {n} classes", snap.revision),
    );
    Some(())
}
