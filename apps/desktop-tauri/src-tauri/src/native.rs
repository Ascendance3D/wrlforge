// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1, the document side of the native viewport. Tauri-free.
//!
//! * `show` projects the CURRENT text of a session (the canonical text minus
//!   a leading BOM, the same text X_ITE is given) into a `RenderScene` with a
//!   new generation number. A damaged parse keeps the last valid projection.
//! * `resolve` turns a render-thread pick into the same `PickOutcome` a
//!   `doc_pick` gives: the hit's frame must name the session, revision, text
//!   and generation on screen NOW, and the hit is proven by the existing
//!   `wrlforge_vrml::pick::resolve` against the canonical parse.
//! * `selected_ids` maps a Scene Tree item to the pick ids to highlight.
//!
//! Nothing here edits the document: the native viewport is selection only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use wrlforge_desktop_protocol as p;
use wrlforge_render::thread::Picked;
use wrlforge_scene::oracle::{self, Verdict};
use wrlforge_scene::RenderScene;
use wrlforge_vrml::pick::{self as pk, Status};

use crate::service::{node_span, pick_outcome, Service, ViewMap};

/// The native viewport's document-side state: one viewport per window.
#[derive(Default)]
pub struct Native {
    /// The native viewport host was installed for this run.
    pub active: std::sync::atomic::AtomicBool,
    next_generation: AtomicU64,
    /// The projection last handed to the viewport, and its session.
    shown: Mutex<Option<Arc<RenderScene>>>,
}

pub enum Shown {
    /// A new projection: hand it to the viewport.
    New(Arc<RenderScene>, p::NativeShown),
    /// The text is damaged: the previous projection stays.
    KeptLastValid(p::NativeShown),
}

/// Pick refusals of the native runtime, mapped onto the existing statuses.
fn refusal(why: &str) -> (Status, String) {
    let status = match why {
        oracle::reason::NEAR_EDGE | oracle::reason::DEPTH_TIE => Status::RefusedAmbiguous,
        "no-frame-shown" | "frame-not-shown" | "viewport-resizing" | "frame scene replaced" => Status::RefusedStale,
        _ => Status::Unsupported,
    };
    (status, why.to_string())
}

fn refusal_message(status: Status, reason: &str) -> String {
    match reason {
        oracle::reason::NEAR_EDGE => "The pointer is on the edge of an object; click inside it, or select it in the Scene Tree.".into(),
        oracle::reason::DEPTH_TIE => "Two objects overlap exactly here; select one in the Scene Tree.".into(),
        oracle::reason::DISAGREE => "The viewport could not confirm this pick; select the object in the Scene Tree.".into(),
        _ => pk::refusal_text(status, reason),
    }
}

impl Native {
    pub fn current(&self) -> Option<Arc<RenderScene>> {
        self.shown.lock().ok().and_then(|g| g.clone())
    }

    /// Forget the projection (document closed, viewport gone).
    pub fn clear(&self) {
        if let Ok(mut g) = self.shown.lock() {
            *g = None;
        }
    }

    pub fn show(&self, svc: &Service, session: p::SessionId) -> Result<Shown, String> {
        let (text, revision) = svc.text_at(session)?;
        let preview = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        let offset = u64::from(preview.len() != text.len());
        let hash = p::preview_hash(preview);
        let parsed = wrlforge_vrml::parse(preview);
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let scene = wrlforge_scene::project(&parsed, session, revision, hash, generation);
        let not_shown = scene
            .not_shown
            .iter()
            .map(|n| p::NativeNotShown { node_type: n.node_type.clone(), from: n.from + offset, to: n.to + offset, reason: n.reason.into() })
            .collect();
        let mut g = self.shown.lock().map_err(|_| "native state poisoned".to_string())?;
        if scene.damaged {
            let last = g.as_ref().filter(|s| s.session == session);
            return Ok(Shown::KeptLastValid(p::NativeShown {
                generation: last.map_or(0, |s| s.generation),
                revision: last.map_or(0, |s| s.revision),
                hash: last.map_or(0, |s| s.text_hash),
                objects: last.map_or(0, |s| s.objects.len() as u32),
                not_shown,
                kept_last_valid: true,
            }));
        }
        let info = p::NativeShown { generation, revision, hash, objects: scene.objects.len() as u32, not_shown, kept_last_valid: false };
        let scene = Arc::new(scene);
        *g = Some(scene.clone());
        Ok(Shown::New(scene, info))
    }

    /// Resolve one native pick against the document AS IT IS NOW.
    pub fn resolve(&self, svc: &Service, session: p::SessionId, picked: &Picked) -> p::PickOutcome {
        let current_revision = svc.text_at(session).map(|t| t.1).unwrap_or(0);
        let generation = picked.frame.map_or(0, |f| f.generation);
        let refuse = |status: Status, why: &str, revision: u64| {
            let mut o = pick_outcome(pk::refuse(status, why), generation, revision, None);
            o.message = refusal_message(status, why);
            o
        };
        let id = match picked.verdict {
            Verdict::Refused(why) => {
                let (status, why) = refusal(why);
                return refuse(status, &why, current_revision);
            }
            Verdict::Background => return refuse(Status::NoHit, pk::reason::NO_GEOMETRY, current_revision),
            Verdict::Object(id) => id,
        };
        let (Some(frame), Some(scene)) = (picked.frame, picked.scene.as_ref()) else {
            return refuse(Status::Unsupported, pk::reason::GENERATION_UNPROVABLE, current_revision);
        };
        // The frame must show THIS session's projection that is current now.
        let shown = self.current();
        let on_screen = shown.as_ref().is_some_and(|s| Arc::ptr_eq(s, scene) && s.generation == frame.generation);
        if frame.session != session || scene.session != session || !on_screen {
            return refuse(Status::RefusedStale, pk::reason::OTHER_GENERATION, current_revision);
        }
        let Ok((text, revision)) = svc.text_at(session) else {
            return refuse(Status::RefusedStale, pk::reason::OTHER_GENERATION, current_revision);
        };
        let preview = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        if frame.revision != revision || p::preview_hash(preview) != frame.text_hash {
            return refuse(Status::RefusedStale, pk::reason::SOURCE_CHANGED, revision);
        }
        let Some(obj) = scene.object(id) else {
            return refuse(Status::Unsupported, pk::reason::NO_PROVENANCE, revision);
        };
        let res = match wrlforge_scene::identity::hit(obj) {
            Err(r) => r,
            Ok(hit) => {
                let parsed = wrlforge_vrml::parse(&text);
                let tree = wrlforge_vrml::scene::build_scene_tree(&parsed.tree, &text);
                let offset = u64::from(preview.len() != text.len());
                pk::resolve(&hit, &parsed, &tree, offset)
            }
        };
        let vm = ViewMap::new(&text);
        pick_outcome(res, frame.generation, revision, Some(&vm))
    }

    /// The pick ids of the shown projection drawn by Scene Tree `item` of
    /// `revision` (its own Shape, or every Shape under it). Empty when the
    /// projection is of another session or revision.
    pub fn selected_ids(&self, svc: &Service, session: p::SessionId, revision: u64, item: Option<&str>) -> (u64, Vec<u32>) {
        let Some(scene) = self.current() else { return (0, Vec::new()) };
        let ids = (|| {
            let (from, to) = node_span(item?)?;
            if scene.session != session || scene.revision != revision {
                return None;
            }
            let (text, now) = svc.text_at(session).ok()?;
            if now != revision {
                return None;
            }
            let offset = u64::from(text.starts_with('\u{FEFF}'));
            let (from, to) = (from.checked_sub(offset)?, to.checked_sub(offset)?);
            Some(
                scene
                    .objects
                    .iter()
                    .filter(|o| o.path.0.iter().any(|n| (n.from, n.to) == (from, to)))
                    .map(|o| o.pick_id)
                    .collect(),
            )
        })()
        .unwrap_or_default();
        (scene.generation, ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrlforge_render::thread::FrameStamp;

    fn svc_with(text: &str) -> (Service, p::SessionId) {
        let svc = Service::default();
        let id = svc.open_text_for_test(text);
        (svc, id)
    }

    fn picked(scene: &Arc<RenderScene>, verdict: Verdict) -> Picked {
        Picked {
            req: 1,
            verdict,
            oracle: None,
            gpu: None,
            frame: Some(FrameStamp {
                seq: 1,
                session: scene.session,
                revision: scene.revision,
                generation: scene.generation,
                text_hash: scene.text_hash,
                width: 10,
                height: 10,
                view: wrlforge_scene::camera::View::new(&Default::default(), 10, 10),
                scale: 1.0,
            }),
            scene: Some(scene.clone()),
            physical: Some((1, 1)),
            ms: 0.0,
        }
    }

    const W: &str = "#VRML V2.0 utf8\nTransform { translation 1 0 0 children Shape { geometry Box {} } }\nDEF S Shape { geometry Sphere {} }\nTransform { children USE S }\n";

    fn shown(n: &Native, svc: &Service, id: p::SessionId) -> Arc<RenderScene> {
        match n.show(svc, id).unwrap() {
            Shown::New(s, _) => s,
            Shown::KeptLastValid(_) => panic!("damaged"),
        }
    }

    #[test]
    fn a_proven_native_pick_selects_the_simple_object() {
        let (svc, id) = svc_with(W);
        let n = Native::default();
        let s = shown(&n, &svc, id);
        let o = n.resolve(&svc, id, &picked(&s, Verdict::Object(1)));
        assert!(o.is_proven(), "{o:?}");
        assert_eq!(o.role.as_deref(), Some("simple-object"));
        assert_eq!(o.generation, s.generation);
        // The Sphere is DEF'd and USEd: refused, never a guess.
        let o = n.resolve(&svc, id, &picked(&s, Verdict::Object(2)));
        assert_eq!(o.status, "REFUSED_AMBIGUOUS");
    }

    #[test]
    fn a_pick_of_an_old_generation_or_text_is_stale() {
        let (svc, id) = svc_with(W);
        let n = Native::default();
        let old = shown(&n, &svc, id);
        let _new = shown(&n, &svc, id);
        assert_eq!(n.resolve(&svc, id, &picked(&old, Verdict::Object(1))).status, "REFUSED_STALE");
        let cur = shown(&n, &svc, id);
        svc.edit_for_test(id, 0, 0, " ");
        let o = n.resolve(&svc, id, &picked(&cur, Verdict::Object(1)));
        assert_eq!((o.status.as_str(), o.reason.as_str()), ("REFUSED_STALE", pk::reason::SOURCE_CHANGED));
        // Another session's frame.
        let (svc2, id2) = svc_with(W);
        let _ = (svc2, id2);
        let mut p = picked(&cur, Verdict::Object(1));
        if let Some(f) = p.frame.as_mut() {
            f.session += 99;
        }
        assert_eq!(n.resolve(&svc, id, &p).status, "REFUSED_STALE");
    }

    #[test]
    fn refusals_and_background_select_nothing() {
        let (svc, id) = svc_with(W);
        let n = Native::default();
        let s = shown(&n, &svc, id);
        for (v, status) in [
            (Verdict::Background, "NO_HIT"),
            (Verdict::Refused(oracle::reason::NEAR_EDGE), "REFUSED_AMBIGUOUS"),
            (Verdict::Refused(oracle::reason::DEPTH_TIE), "REFUSED_AMBIGUOUS"),
            (Verdict::Refused(oracle::reason::DISAGREE), "UNSUPPORTED"),
            (Verdict::Refused(oracle::reason::GPU_FAILED), "UNSUPPORTED"),
            (Verdict::Refused("device-lost"), "UNSUPPORTED"),
            (Verdict::Refused("viewport-resizing"), "REFUSED_STALE"),
            (Verdict::Object(99), "UNSUPPORTED"),
        ] {
            let o = n.resolve(&svc, id, &picked(&s, v));
            assert_eq!(o.status, status, "{v:?}");
            assert!(o.item.is_none());
        }
    }

    #[test]
    fn damaged_text_keeps_the_last_valid_projection() {
        let (svc, id) = svc_with(W);
        let n = Native::default();
        let s = shown(&n, &svc, id);
        svc.edit_for_test(id, W.len() as u64 - 1, W.len() as u64 - 1, "Shape {");
        let Shown::KeptLastValid(info) = n.show(&svc, id).unwrap() else { panic!() };
        assert!(info.kept_last_valid);
        assert_eq!(info.generation, s.generation);
        assert!(Arc::ptr_eq(&n.current().unwrap(), &s));
        assert_eq!(n.resolve(&svc, id, &picked(&s, Verdict::Object(1))).status, "REFUSED_STALE");
    }

    #[test]
    fn selection_maps_items_to_pick_ids_of_the_same_revision() {
        let (svc, id) = svc_with(W);
        let n = Native::default();
        let s = shown(&n, &svc, id);
        let o = n.resolve(&svc, id, &picked(&s, Verdict::Object(1)));
        let (g, ids) = n.selected_ids(&svc, id, o.revision, o.item.as_deref());
        assert_eq!((g, ids), (s.generation, vec![1]));
        assert!(n.selected_ids(&svc, id, o.revision + 1, o.item.as_deref()).1.is_empty());
        assert!(n.selected_ids(&svc, id, o.revision, None).1.is_empty());
    }

    #[test]
    fn a_bom_shifts_spans_exactly_once() {
        let text = format!("\u{FEFF}{W}");
        let (svc, id) = svc_with(&text);
        let n = Native::default();
        let s = shown(&n, &svc, id);
        let o = n.resolve(&svc, id, &picked(&s, Verdict::Object(1)));
        assert!(o.is_proven(), "{o:?}");
        let logical = o.logical.unwrap();
        assert_eq!(logical.from, ("\u{FEFF}#VRML V2.0 utf8\n").encode_utf16().count() as u64);
        assert_eq!(n.selected_ids(&svc, id, o.revision, o.item.as_deref()).1, vec![1]);
    }
}
