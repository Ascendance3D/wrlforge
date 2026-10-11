// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-2A: scope analysis through the real `Service` (the path the UI
//! uses), over the RUST-STAB-1 Flux fixtures. Fixture text is never changed.
//!
//! - Every proven reference target is a Scene Tree item of the SAME analysis
//!   whose view text is that DEF node, across LF, CRLF, BOM+CRLF and gzip.
//! - An uncertain reference never carries a target.
//! - Scope findings are advisory: never "error", and Save is unaffected.
//! - Analyses are bound to one session and one revision.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wrl_forge_desktop::files::{self, Format};
use wrl_forge_desktop::service::Service;
use wrlforge_desktop_protocol as p;

fn clock() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_500_000)
}

const FIXTURES: &[&str] = &[
    "01_anim_no_starttime.wrl",
    "02_anim_touch.wrl",
    "03_shape_no_appearance.wrl",
    "04_ifs_normals.wrl",
    "05_ifs_quads.wrl",
    "06_background_precision.wrl",
    "07_shared_material.wrl",
    "08_proto_script_inline.wrl",
    "09_duplicate_def.wrl",
    "10_cylinder_cone.wrl",
    "malformed/m1_cylinder_topradius.wrl",
    "malformed/m2_route_no_such_eventout.wrl",
    "malformed/m3_route_component_access.wrl",
    "malformed/m4_def_two_type_names.wrl",
    "malformed/m5_unbalanced_braces.wrl",
    "malformed/m6_route_before_def.wrl",
];

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/flux-regression")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "wrlforge-rust-stab-2a-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// LF, CRLF, BOM+CRLF and gzip bytes of one LF text.
fn variants(lf: &str) -> Vec<(&'static str, Vec<u8>)> {
    let crlf = lf.replace('\n', "\r\n");
    vec![
        ("lf", lf.as_bytes().to_vec()),
        ("crlf", crlf.clone().into_bytes()),
        ("bom-crlf", format!("\u{feff}{crlf}").into_bytes()),
        ("gzip", files::encode(lf, Format::Gzip).unwrap()),
    ]
}

fn opened(o: p::OpenOutcome) -> p::DocumentInfo {
    match o {
        p::OpenOutcome::Opened { doc } => doc,
        other => panic!("not opened: {other:?}"),
    }
}

/// UTF-16 view slice `[from, to)`.
fn view_slice(view: &str, from: u64, to: u64) -> String {
    let u: Vec<u16> = view.encode_utf16().collect();
    String::from_utf16(&u[from as usize..to as usize]).unwrap()
}

/// The fixed proof: proven targets are real DEF items; nothing else has one.
fn check_targets(name: &str, view: &str, a: &p::Analysis) {
    for it in &a.items {
        for r in &it.refs {
            if r.class == "proven" {
                let t = r.target.as_ref().unwrap_or_else(|| {
                    panic!("{name}: proven {r:?} lacks a target (DEF in a non-item context?)")
                });
                let def = a
                    .items
                    .iter()
                    .find(|i| &i.id == t)
                    .expect("target is an item");
                let text = view_slice(view, def.view_from, def.view_to);
                let want = format!("DEF {} ", r.name.as_deref().unwrap());
                assert!(text.starts_with(&want), "{name}: {t} is {text:?}");
                // The target precedes the reference (4.6.2 / 4.10.2), or
                // contains it (a Script self-reference).
                assert!(def.view_from < it.view_to, "{name}: target after reference");
            } else {
                assert!(r.target.is_none(), "{name}: uncertain {r:?} has a target");
            }
        }
    }
    for d in &a.scope_diagnostics {
        assert_ne!(d.severity, "error", "{name}: scope findings are advisory");
        assert!(d.code.starts_with("SCOPE"), "{name}: {}", d.code);
    }
}

/// Per-item references (target ids blanked) and (code, message) findings.
type Answers = (Vec<Vec<p::SceneRef>>, Vec<(String, String)>);

#[test]
fn fixtures_scope_answers_across_variants_and_save_is_unaffected() {
    for name in FIXTURES {
        let lf = fixture(name);
        let mut baseline: Option<Answers> = None;
        for (tag, bytes) in variants(&lf) {
            let dir = TempDir::new("variants");
            let path = dir.0.join("case.wrl");
            fs::write(&path, &bytes).unwrap();
            let svc = Service::new(clock);
            let doc = opened(svc.open_path(&path));
            let a = svc.analyze(doc.session).unwrap();
            assert_eq!(a.resolution_scope, "lexical");
            assert_eq!((a.session, a.revision), (doc.session, doc.revision));
            check_targets(name, &doc.view, &a);
            // RUST-STAB-2B typed findings (SCOPE1xx) mark other ranges; they
            // are checked in `typed_regression.rs`.
            for d in a
                .scope_diagnostics
                .iter()
                .filter(|d| d.code.starts_with("SCOPE0"))
            {
                // Each finding marks exactly its reference name token.
                let token = view_slice(&doc.view, d.view_from, d.view_to);
                assert!(
                    d.message.contains(&format!("'{token}'")),
                    "{name}/{tag}: {d:?}"
                );
            }
            // The same answers in every encoding (ids differ only by offset).
            let refs: Vec<Vec<p::SceneRef>> = a
                .items
                .iter()
                .map(|i| {
                    i.refs
                        .iter()
                        .map(|r| p::SceneRef {
                            target: r.target.as_ref().map(|_| String::new()),
                            language_target: r.language_target.as_ref().map(|_| String::new()),
                            ..r.clone()
                        })
                        .collect()
                })
                .collect();
            let diags: Vec<(String, String)> = a
                .scope_diagnostics
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect();
            match &baseline {
                None => baseline = Some((refs, diags)),
                Some(b) => assert_eq!(b, &(refs, diags), "{name}/{tag}: answers differ"),
            }
            // Save never depends on scope analysis, and keeps every byte.
            let saved = svc.save(doc.session);
            assert!(
                matches!(saved, p::SaveOutcome::Saved { .. }),
                "{name}/{tag}: {saved:?}"
            );
            let after = fs::read(&path).unwrap();
            if tag == "gzip" {
                assert_eq!(files::decode(&after).unwrap().0, lf, "{name}/{tag}");
            } else {
                assert_eq!(after, bytes, "{name}/{tag}: bytes changed");
            }
        }
    }
}

#[test]
fn fixture_expectations_follow_the_standard() {
    let svc = Service::new(clock);
    let analyze = |name: &str| {
        let dir = TempDir::new("expect");
        let path = dir.0.join("x.wrl");
        fs::write(&path, fixture(name)).unwrap();
        let doc = opened(svc.open_path(&path));
        (svc.analyze(doc.session).unwrap(), doc.view)
    };
    let refs = |a: &p::Analysis| -> Vec<p::SceneRef> {
        a.items.iter().flat_map(|i| i.refs.clone()).collect()
    };

    // 09: valid VRML97 with a repeated DEF. Ambiguous for editing, but
    // neither an error nor a warning.
    let (a, view) = analyze("09_duplicate_def.wrl");
    let st: Vec<_> = refs(&a).iter().map(|r| r.status.clone()).collect();
    assert_eq!(st, ["resolved", "ambiguous", "resolved"]);
    let amb = refs(&a)
        .into_iter()
        .find(|r| r.status == "ambiguous")
        .unwrap();
    let lang = a
        .items
        .iter()
        .find(|i| Some(&i.id) == amb.language_target.as_ref())
        .unwrap();
    assert!(view_slice(&view, lang.view_from, lang.view_to).contains("translation -1 0 0"));
    assert!(a.scope_diagnostics.iter().all(|d| d.severity == "info"));
    // `USE Floor` binds the OUTER Floor, not the PROTO's (4.8.4).
    let floor = refs(&a)
        .into_iter()
        .find(|r| r.name.as_deref() == Some("Floor"))
        .unwrap();
    let t = a
        .items
        .iter()
        .find(|i| Some(&i.id) == floor.target.as_ref())
        .unwrap();
    assert_eq!(t.depth, 1, "the top-level DEF Floor");

    // 08: four ROUTE endpoints, all proven. No node-name finding; its one
    // typed finding (an IS type mismatch) is RUST-STAB-2B's.
    let (a, _) = analyze("08_proto_script_inline.wrl");
    assert_eq!(refs(&a).len(), 4);
    assert!(refs(&a).iter().all(|r| r.class == "proven"));
    let codes: Vec<_> = a
        .scope_diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["SCOPE114"]);

    // m6: ROUTE before its DEFs (4.10.2): two SCOPE003 warnings.
    let (a, _) = analyze("malformed/m6_route_before_def.wrl");
    let codes: Vec<_> = a
        .scope_diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["SCOPE003", "SCOPE003"]);

    // m5: unbalanced braces: no claim at all.
    let (a, _) = analyze("malformed/m5_unbalanced_braces.wrl");
    assert!(a.scope_diagnostics.is_empty());
}

/// 13 + 15: identical names in two sessions; a stale revision is refused.
#[test]
fn analyses_are_bound_to_session_and_revision() {
    let svc = Service::new(clock);
    let dir = TempDir::new("sessions");
    let (pa, pb) = (dir.0.join("a.wrl"), dir.0.join("b.wrl"));
    fs::write(
        &pa,
        "#VRML V2.0 utf8\nDEF X Group { }\nGroup { children [ USE X ] }\n",
    )
    .unwrap();
    fs::write(
        &pb,
        "#VRML V2.0 utf8\nPROTO P [ ] { DEF X Group { } }\nGroup { children [ USE X ] }\n",
    )
    .unwrap();
    let (da, db) = (opened(svc.open_path(&pa)), opened(svc.open_path(&pb)));
    assert_ne!(da.session, db.session);
    let (aa, ab) = (
        svc.analyze(da.session).unwrap(),
        svc.analyze(db.session).unwrap(),
    );
    let use_of =
        |a: &p::Analysis| a.items.iter().find(|i| i.kind == "Use").unwrap().refs[0].clone();
    assert_eq!(use_of(&aa).status, "resolved");
    assert_eq!(use_of(&ab).status, "unresolved");
    assert_eq!(use_of(&ab).reason, "def-not-visible-across-proto-boundary");
    assert!(use_of(&ab).target.is_none());
    assert_eq!(ab.scope_diagnostics.len(), 1);
    assert!(aa.scope_diagnostics.is_empty());

    // Rename the DEF in session A: revision 1 answers differently, and the
    // revision-0 target id is refused, never re-read against new text.
    let old_target = use_of(&aa).target.unwrap();
    let def_at = da.view.find("DEF X").unwrap() as u64 + 4;
    let r = svc
        .edit(&p::EditRequest {
            session: da.session,
            base_revision: 0,
            from: def_at,
            to: def_at + 1,
            insert: "Y".into(),
            item: None,
        })
        .unwrap();
    assert!(matches!(r, p::EditOutcome::Applied { .. }));
    let a1 = svc.analyze(da.session).unwrap();
    assert_eq!(a1.revision, 1);
    assert_eq!(use_of(&a1).status, "unresolved");
    assert_eq!(a1.scope_diagnostics[0].code, "SCOPE001");
    assert_eq!(
        svc.inspect(da.session, &old_target, 0).unwrap(),
        p::InspectOutcome::Stale { current: 1 }
    );
    // Session B is untouched by A's edit.
    assert_eq!(svc.analyze(db.session).unwrap(), ab);
}
