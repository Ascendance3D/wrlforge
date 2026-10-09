// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-3A1: keep the user's working camera across a preview reload.
//!
//! Every reload builds a whole new X_ITE scene, which binds its first
//! viewpoint (or the layer default) with zero user offsets: the view the user
//! orbited, panned or zoomed to would be lost after every Move. The carry
//! restores it only onto a viewpoint PROVEN to be the same one:
//!
//! * the layer's built-in default viewpoint, in both scenes; or
//! * the authored viewpoint whose provenance span the adapter captured when
//!   its generation retired, mapped by Rust (`doc_preview_carry`) through the
//!   exact logged changes to THIS revision, and found as the ONE runtime
//!   node for that span in the new generation.
//!
//! Nothing is matched by name, description, index or position; when the
//! proof fails, nothing is restored and the reason is reported. The camera
//! is editor state: no source change, no dirty flag, no undo step.

use std::cell::RefCell;

use wrlforge_desktop_protocol as p;

use crate::ipc::{self, call, CameraWant};

thread_local! {
    /// Per document preview load: X_ITE parse + scene replacement (ms).
    pub static LOAD_MS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

/// What the next load of `revision` (session `session`) may restore onto,
/// and why not when nothing.
pub async fn want(session: u64, revision: u64) -> (Option<CameraWant>, &'static str) {
    let Some(cap) = ipc::camera_capture() else {
        return (None, "no-capture");
    };
    if cap.session != session {
        return (None, "another-document");
    }
    match cap.kind.as_str() {
        "default" => (Some(CameraWant::Default), "default-viewpoint"),
        "authored" => {
            let (Some(from), Some(to)) = (cap.from, cap.to) else {
                return (None, "viewpoint-without-span");
            };
            #[derive(serde::Serialize)]
            struct A {
                request: p::PreviewCarryRequest,
            }
            let r = call::<p::PreviewCarryOutcome>(
                "doc_preview_carry",
                A {
                    request: p::PreviewCarryRequest {
                        session,
                        from_revision: cap.revision,
                        to_revision: revision,
                        from,
                        to,
                    },
                },
            )
            .await;
            match r {
                Ok(p::PreviewCarryOutcome::Mapped { from, to }) => (
                    Some(CameraWant::Authored { from, to }),
                    "authored-viewpoint",
                ),
                Ok(p::PreviewCarryOutcome::Lost { .. }) => (None, "viewpoint-span-lost"),
                Err(_) => (None, "carry-unavailable"),
            }
        }
        _ => (None, "viewpoint-unprovable"),
    }
}

/// A short status suffix when the user's view could NOT be kept (the reason
/// is reported, never hidden); empty when it was, or when there was nothing
/// to keep (first load, another document).
pub fn note(why: &str) -> String {
    let reason = match why {
        "no-capture" | "another-document" => return String::new(),
        "default-viewpoint" | "authored-viewpoint" => match ipc::camera_result() {
            Some(r) if r.status == "restored" => return String::new(),
            // The old scene stayed on screen (a failed load) or a newer load
            // reports: the view did not change here.
            Some(r)
                if matches!(
                    r.reason.as_deref(),
                    Some("world-not-replaced" | "superseded")
                ) =>
            {
                return String::new()
            }
            Some(r) => r.reason.unwrap_or_else(|| r.status.clone()),
            None => "camera-carry-unavailable".into(),
        },
        other => other.to_string(),
    };
    format!(" · view reset ({reason})")
}
