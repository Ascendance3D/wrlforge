// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-1 recovery smoke, phases 2 and 3 (see
//! `src-tauri/src/smoke_recovery.rs`). Real clicks on the startup dialog;
//! every byte comparison is made by Rust (`smoke_recovery_check`).

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;
use wrlforge_desktop_protocol as p;

use super::{key, settle, snapshot, R};
use crate::editor::{textarea, CORE};
use crate::ipc::{self, call, NoArgs};
use crate::ui::ui;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub phase: String,
}

fn doc() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn dialog_text() -> Option<String> {
    doc()
        .get_element_by_id("recovery-dialog")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
        .map(|e| e.inner_text())
}

/// `(names of offered entries, number of damaged entries)` on screen.
fn on_screen() -> (Vec<String>, usize) {
    let mut names = vec![];
    let mut damaged = 0;
    if let Ok(list) = doc().query_selector_all("#recovery-dialog .recovery-entry") {
        for i in 0..list.length() {
            let Some(el) = list
                .item(i)
                .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
            else {
                continue;
            };
            if el.class_list().contains("recovery-damaged") {
                damaged += 1;
            } else if let Some(n) = el.get_attribute("data-name") {
                names.push(n);
            }
        }
    }
    names.sort();
    (names, damaged)
}

fn click_in(selector: &str, button: &str) -> bool {
    let q = format!("#recovery-dialog {selector} {button}");
    match doc()
        .query_selector(&q)
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        Some(b) => {
            b.click();
            true
        }
        None => false,
    }
}

async fn wait(mut cond: impl FnMut() -> bool, ms: u32) -> bool {
    for _ in 0..(ms / 20) {
        if cond() {
            return true;
        }
        ipc::sleep(20).await;
    }
    cond()
}

async fn check(r: &mut R, what: &str, name: &str) {
    #[derive(serde::Serialize)]
    struct A<'a> {
        session: Option<u64>,
        what: &'a str,
        name: &'a str,
    }
    let session = CORE.with_borrow(|c| c.session);
    match call::<p::SmokeStep>(
        "smoke_recovery_check",
        A {
            session,
            what,
            name,
        },
    )
    .await
    {
        Ok(s) => {
            r.step(&s.name, s.ok, s.detail);
        }
        Err(e) => {
            r.step(&format!("{what} {name}"), false, e);
        }
    }
}

/// Type one character at the end of the text through a real input event.
async fn type_at_end(s: &str) -> Option<()> {
    let ta = textarea()?;
    let end = ta.value().encode_utf16().count() as u32;
    let _ = ta.focus();
    ta.set_range_text_with_start_and_end(s, end, end).ok()?;
    let ev = web_sys::InputEvent::new("input").ok()?;
    ta.dispatch_event(&ev).ok()?;
    settle().await;
    Some(())
}

async fn restore(r: &mut R, name: &str) -> Option<p::DocumentInfo> {
    let sel = format!(".recovery-entry[data-name=\"{name}\"]");
    if !r.step(
        &format!("Restore clicked for {name}"),
        click_in(&sel, ".recovery-restore"),
        "",
    ) {
        return None;
    }
    let ok = wait(
        || {
            ui().doc
                .with_untracked(|d| d.as_ref().is_some_and(|d| d.recovered && d.name == name))
        },
        5000,
    )
    .await;
    settle().await;
    let d = snapshot().await?;
    r.step(
        &format!("{name} restored as a new untitled, unsaved document"),
        ok && d.recovered && d.untitled && d.dirty && d.name == name,
        format!(
            "recovered {} untitled {} dirty {} format {}",
            d.recovered, d.untitled, d.dirty, d.format
        ),
    );
    Some(d)
}

async fn save(r: &mut R, name: &str) {
    if let Some(b) = crate::element_by_id::<HtmlElement>("btn-save") {
        b.click();
    }
    let ok = wait(|| !ui().dirty.get_untracked(), 5000).await;
    settle().await;
    let d = snapshot().await;
    r.step(
        &format!("Save of restored {name} went through Save As"),
        ok && d.is_some_and(|d| !d.untitled && !d.dirty),
        "",
    );
}

async fn discard(r: &mut R, sel: &str, label: &str) {
    let before = on_screen();
    r.step(
        &format!("Discard clicked for {label}"),
        click_in(sel, ".recovery-discard"),
        "",
    );
    let ok = wait(|| on_screen() != before || dialog_text().is_none(), 5000).await;
    r.step(
        &format!("{label} left the dialog after confirmation"),
        ok,
        "",
    );
}

pub async fn run(plan: &Plan, r: &mut R) -> Option<()> {
    let shown = wait(|| dialog_text().is_some(), 10_000).await;
    let listed = call::<p::RecoveryList>("recovery_list", NoArgs {}).await;
    let detail = match &listed {
        Ok(l) => format!(
            "Rust lists {} offered, {} damaged; signal set {}; element {}; body has {}",
            l.entries.len(),
            l.damaged.len(),
            crate::recovery::list().get_untracked().is_some(),
            doc().get_element_by_id("recovery-dialog").is_some(),
            doc().body().map(|b| b.inner_html().len()).unwrap_or(0)
        ),
        Err(e) => format!("recovery_list failed: {e}"),
    };
    if !r.step("recovery dialog shown at startup", shown, detail) {
        return None;
    }
    let text = dialog_text().unwrap_or_default();
    r.step(
        "dialog shows names and times, never a path",
        !text.contains('/') && text.contains("recovered"),
        text.lines().take(3).collect::<Vec<_>>().join(" | "),
    );
    r.step(
        "nothing restored without a click",
        ui().doc.with_untracked(|d| d.is_none()),
        "",
    );
    match plan.phase.as_str() {
        "restore" => restore_phase(r).await,
        "later" => later_phase(r).await,
        other => {
            r.step("known phase", false, other.to_string());
            None
        }
    }
}

async fn restore_phase(r: &mut R) -> Option<()> {
    let (names, damaged) = on_screen();
    r.step(
        "four records offered, one damaged record reported (no clean-file prompt)",
        names == ["packed.wrl", "plain.wrl", "tricky.wrl", "untitled.wrl"] && damaged == 1,
        format!("{names:?} damaged {damaged}"),
    );
    check(r, "records", "5").await;

    // BOM + CRLF + lone CR + Unicode + malformed tail.
    restore(r, "tricky.wrl").await?;
    check(r, "session-exact", "tricky.wrl").await;
    let rev0 = snapshot().await?.revision;
    type_at_end("Z").await?;
    let typed = snapshot().await?;
    r.step(
        "recovered text accepts normal typing",
        typed.revision > rev0 && typed.dirty,
        "",
    );
    key("z", false);
    settle().await;
    check(r, "session-exact", "tricky.wrl").await;
    check(r, "record-is-session", "tricky.wrl").await;
    check(r, "originals", "docs").await;
    save(r, "tricky.wrl").await;
    check(r, "saved-exact", "tricky.wrl").await;
    check(r, "records", "4").await;

    // Gzip keeps its format through Restore and Save As.
    let d = restore(r, "packed.wrl").await?;
    r.step(
        "gzip record restores as gzip",
        d.format == "gzip",
        d.format.clone(),
    );
    check(r, "session-exact", "packed.wrl").await;
    save(r, "packed.wrl").await;
    check(r, "saved-exact", "packed.wrl").await;
    check(r, "records", "3").await;

    discard(r, ".recovery-entry[data-name=\"plain.wrl\"]", "plain.wrl").await;
    check(r, "records", "2").await;
    check(r, "originals", "docs").await;

    // A canceled window close keeps the unsaved work and its record.
    type_at_end("# after save\n").await?;
    check(r, "record-is-session", "restored-2.wrl").await;
    check(r, "records", "3").await;
    let _ = call::<()>("smoke_request_close", NoArgs {}).await;
    let asked = {
        let mut got = false;
        for _ in 0..100 {
            #[derive(serde::Serialize)]
            struct A<'a> {
                session: Option<u64>,
                what: &'a str,
                name: &'a str,
            }
            if let Ok(s) = call::<p::SmokeStep>(
                "smoke_recovery_check",
                A {
                    session: None,
                    what: "close-asked",
                    name: "[false]",
                },
            )
            .await
            {
                if s.ok {
                    got = true;
                    break;
                }
            }
            ipc::sleep(50).await;
        }
        got
    };
    r.step("window close asked once and was canceled", asked, "");
    ipc::sleep(300).await;
    r.step(
        "window still open with the unsaved work after Cancel",
        ui().dirty.get_untracked() && snapshot().await.is_some_and(|d| d.dirty),
        "",
    );
    check(r, "records", "3").await;

    // Later keeps the rest for the next start.
    r.step("Later clicked", click_in("", "#recovery-later"), "");
    let gone = wait(|| dialog_text().is_none(), 2000).await;
    r.step("Later closes the dialog", gone, "");
    check(r, "records", "3").await;
    Some(())
}

async fn later_phase(r: &mut R) -> Option<()> {
    let (names, damaged) = on_screen();
    r.step(
        "Later and the canceled close kept their records",
        names == ["restored-2.wrl", "untitled.wrl"] && damaged == 1,
        format!("{names:?} damaged {damaged}"),
    );
    restore(r, "untitled.wrl").await?;
    check(r, "session-exact", "untitled.wrl").await;
    discard(r, ".recovery-damaged", "the damaged record").await;
    check(r, "records", "2").await;
    discard(
        r,
        ".recovery-entry[data-name=\"restored-2.wrl\"]",
        "restored-2.wrl",
    )
    .await;
    check(r, "records", "1").await;
    let gone = wait(|| dialog_text().is_none(), 2000).await;
    r.step("dialog closes when nothing is left to decide", gone, "");
    check(r, "originals", "docs").await;
    Some(())
}
