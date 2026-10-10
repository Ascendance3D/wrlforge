// SPDX-License-Identifier: GPL-3.0-or-later
//! TEXTURE-LOCAL-1 in-window texture run (`--smoke-texture`).
//!
//! Each fixture is opened by INDEX through Rust, previewed by X_ITE, and
//! judged on the RENDERED frame: the viewport pixels at the plan's sample
//! points must be the texture's known color (or the untextured surface),
//! and the Rust URL check must report exactly the expected number of
//! unloadable `ImageTexture` nodes. A capture of every frame is stored as
//! evidence. Two fixtures run a workflow: a document switch while a texture
//! request is held in flight, and save → close → reopen → reload.
//!
//! The source is never touched: every fixture must stay at revision 0, not
//! dirty, with the editor showing exactly the file's text; Rust checks the
//! bytes on disk at the end.

use leptos::prelude::*;
use wrlforge_desktop_protocol as p;

use super::create::wait_ms;
use super::pick::{ready, viewport};
use super::{snapshot, R};
use crate::editor::CORE;
use crate::ipc::{self, call};
use crate::ui::{self, ui};

/// Per-channel tolerance for a textured sample (JPEG at quality 95).
const TOL: i32 = 40;

fn close_to(px: &[u8], want: [u8; 3]) -> bool {
    px.len() >= 3 && (0..3).all(|i| (px[i] as i32 - want[i] as i32).abs() <= TOL)
}

/// The untextured surface: light and neutral (no image color on it).
fn untextured(px: &[u8]) -> bool {
    px.len() >= 3 && {
        let (mx, mn) = (
            *px[..3].iter().max().unwrap(),
            *px[..3].iter().min().unwrap(),
        );
        mn >= 150 && mx - mn <= 30
    }
}

async fn open(i: usize) -> Option<p::DocumentInfo> {
    #[derive(serde::Serialize)]
    struct A {
        index: usize,
    }
    let old = CORE.with_borrow(|c| c.session);
    let o = call::<p::OpenOutcome>("smoke_open_fixture", A { index: i })
        .await
        .ok()?;
    ui::apply_open(o, "Opened").await;
    wait_ms(5000, || {
        ui().doc
            .get_untracked()
            .filter(|d| d.revision == 0 && !d.untitled && Some(d.session) != old)
    })
    .await
}

/// Wait for the preview of the current revision, then sample.
async fn judge(fx: &p::TextureFixture, r: &mut R, step: &str) -> bool {
    let loaded = ready(20_000).await.is_some();
    // Textures finish before X_ITE resolves replaceWorld; one more frame.
    ipc::sleep(150).await;
    let Some(rect) = viewport().map(|v| v.get_bounding_client_rect()) else {
        return r.step(step, false, "no viewport");
    };
    let mut ok = loaded;
    let mut detail = vec![format!(
        "loaded {loaded} · status {}{}",
        ui().preview_status.get_untracked(),
        if loaded {
            String::new()
        } else {
            format!(" · {}", ipc::preview_load_state())
        }
    )];
    for s in &fx.samples {
        let x = rect.left() + rect.width() * s.x;
        let y = rect.top() + rect.height() * s.y;
        let px = ipc::preview_pixel(x, y).await.unwrap_or_default();
        let good = match s.color {
            Some(c) => close_to(&px, c),
            None => untextured(&px),
        };
        ok &= good;
        detail.push(format!(
            "({:.2},{:.2}) want {} got {:?}",
            s.x,
            s.y,
            s.color
                .map_or("untextured".to_string(), |c| format!("{c:?}")),
            px
        ));
    }
    let warnings = ui().texture_warnings.get_untracked();
    let warn_ok = warnings.len() == fx.warnings;
    ok &= warn_ok;
    detail.push(format!(
        "warnings {} (want {}){}",
        warnings.len(),
        fx.warnings,
        warnings
            .iter()
            .map(|w| format!(" | line {} {}: {}", w.line, w.node, w.detail))
            .collect::<String>()
    ));
    // The texture never edits the source.
    let doc = snapshot().await;
    let rev = CORE.with_borrow(|c| c.revision);
    let clean = doc
        .as_ref()
        .is_some_and(|d| d.revision == rev && !d.dirty && super::create::text() == d.view);
    ok &= clean;
    if !clean {
        detail.push("source changed by the preview".into());
    }
    let id = step
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    if let Some(png) = ipc::preview_capture().await {
        #[derive(serde::Serialize)]
        struct C {
            id: String,
            png: Vec<u8>,
        }
        if let Ok(path) = call::<String>("smoke_texture_capture", C { id, png }).await {
            detail.push(path);
        }
    }
    r.step(
        &format!("{step}: {} — rendered as expected", fx.label),
        ok,
        detail.join("; "),
    )
}

async fn resource_log() -> Vec<p::ResourceLogEntry> {
    call::<Vec<p::ResourceLogEntry>>("smoke_resource_log", ipc::NoArgs {})
        .await
        .unwrap_or_default()
}

async fn delay(ms: u64) {
    #[derive(serde::Serialize)]
    struct D {
        ms: u64,
    }
    let _ = call::<()>("smoke_texture_delay", D { ms }).await;
}

pub(super) async fn run(t: &p::TextureSmoke, r: &mut R) -> Option<()> {
    for _ in 0..200 {
        if crate::editor::textarea().is_some() {
            break;
        }
        ipc::sleep(20).await;
    }
    let index = |id: &str| t.fixtures.iter().position(|f| f.id.starts_with(id));
    for (i, fx) in t.fixtures.iter().enumerate() {
        if fx.id.contains("slow-switch") {
            slow_switch(t, i, index("t01")?, r).await?;
        } else if fx.id.contains("save-reopen") {
            save_reopen(fx, i, r).await?;
        } else {
            open(i).await?;
            judge(fx, r, &fx.id).await;
        }
    }
    Some(())
}

/// Hold every texture read; open `i`, then switch to `other` while its
/// request is in flight. The held reply must be refused (its session is
/// closed), and `other` must render its own texture.
async fn slow_switch(t: &p::TextureSmoke, i: usize, other: usize, r: &mut R) -> Option<()> {
    let fx = &t.fixtures[i];
    delay(1500).await;
    let first = open(i).await?;
    // Wait until X_ITE's request for it is held in Rust.
    let mut held = false;
    for _ in 0..100 {
        if resource_log()
            .await
            .iter()
            .any(|e| e.session == Some(first.session) && e.status == 0)
        {
            held = true;
            break;
        }
        ipc::sleep(50).await;
    }
    r.step(
        &format!("{}: a texture request is in flight", fx.id),
        held,
        "",
    );
    open(other).await?;
    let ok_other = judge(&t.fixtures[other], r, &format!("{}-switched-to", fx.id)).await;
    // The held read finishes after the switch.
    ipc::sleep(1700).await;
    delay(0).await;
    let log = resource_log().await;
    let refused: Vec<&p::ResourceLogEntry> = log
        .iter()
        .filter(|e| e.session == Some(first.session))
        .collect();
    let stale = refused
        .iter()
        .any(|e| e.status == 403 && e.outcome.contains("replaced or document closed"));
    let refused: Vec<&p::ResourceLogEntry> =
        refused.into_iter().filter(|e| e.status != 0).collect();
    let served = refused.iter().any(|e| e.status == 200);
    r.step(
        &format!(
            "{}: {} — the in-flight reply for the closed document was refused",
            fx.id, fx.label
        ),
        ok_other && stale && !served,
        format!("{refused:?}"),
    );
    // Without the hold, the same document renders normally.
    open(i).await?;
    judge(fx, r, &format!("{}-reopened", fx.id)).await;
    Some(())
}

/// Save (unchanged), close, reopen, reload: the texture renders each time.
async fn save_reopen(fx: &p::TextureFixture, i: usize, r: &mut R) -> Option<()> {
    open(i).await?;
    judge(fx, r, &format!("{}-opened", fx.id)).await;
    let saved = ui::save_now(false).await;
    r.step(
        &format!("{}: Save succeeded", fx.id),
        matches!(saved, Some(p::SaveOutcome::Saved { .. })),
        format!("{saved:?}"),
    );
    judge(fx, r, &format!("{}-saved", fx.id)).await;
    ui::close();
    let closed = wait_ms(5000, || ui().doc.get_untracked().is_none().then_some(())).await;
    let warn_cleared = ui().texture_warnings.get_untracked().is_empty();
    r.step(
        &format!("{}: Close removes the document", fx.id),
        closed.is_some() && warn_cleared,
        "",
    );
    open(i).await?;
    judge(fx, r, &format!("{}-reopened", fx.id)).await;
    let before = CORE.with_borrow(|c| c.session);
    let loads = ui().preview_loads.get_untracked();
    ui::reload();
    wait_ms(20_000, || {
        (ui().preview_loads.get_untracked() > loads).then_some(())
    })
    .await;
    let same = CORE.with_borrow(|c| c.session) == before;
    r.step(&format!("{}: Reload keeps the session", fx.id), same, "");
    judge(fx, r, &format!("{}-reloaded", fx.id)).await;
    Some(())
}
