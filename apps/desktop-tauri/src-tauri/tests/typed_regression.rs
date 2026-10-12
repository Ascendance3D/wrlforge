// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-2B: typed ROUTE, IS and PROTO reference analysis through the
//! real `Service` (the path the UI uses). Fixture text is never changed.
//!
//! - The original Flux 08 fixture is invalid VRML97 (an IS type mismatch)
//!   and gets exactly one SCOPE114; its corrected copy gets none.
//! - Findings mark the same text and say the same thing in LF, CRLF,
//!   BOM+CRLF and gzip form, and Save keeps every byte.
//! - Typed answers reach the Scene Tree items of the same analysis.
//! - Analyses are bound to one session and one revision, and analysis
//!   never changes the source.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wrl_forge_desktop::files::{self, Format};
use wrl_forge_desktop::service::Service;
use wrlforge_desktop_protocol as p;

fn clock() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_500_000)
}

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
            "wrlforge-rust-stab-2b-{tag}-{}-{nanos}",
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

fn view_slice(view: &str, from: u64, to: u64) -> String {
    let u: Vec<u16> = view.encode_utf16().collect();
    String::from_utf16(&u[from as usize..to as usize]).unwrap()
}

/// (code, severity, marked text) per scope finding.
fn findings(view: &str, a: &p::Analysis) -> Vec<(String, String, String)> {
    a.scope_diagnostics
        .iter()
        .map(|d| {
            (
                d.code.clone(),
                d.severity.clone(),
                view_slice(view, d.view_from, d.view_to),
            )
        })
        .collect()
}

fn f(code: &str, sev: &str, text: &str) -> (String, String, String) {
    (code.into(), sev.into(), text.into())
}

#[test]
fn typed_findings_across_variants_and_save_keeps_bytes() {
    let cases: Vec<(&str, Vec<(String, String, String)>)> = vec![
        (
            "08_proto_script_inline.wrl",
            vec![f("SCOPE114", "warning", "size IS size")],
        ),
        ("corrected/08_proto_script_inline_valid.wrl", vec![]),
        (
            "malformed/m2_route_no_such_eventout.wrl",
            vec![f("SCOPE101", "warning", "key_value")],
        ),
        // Non-conforming ROUTE order (4.10.2): the 2A finding only. No
        // typed claim is made through the unbound node names.
        (
            "malformed/m6_route_before_def.wrl",
            vec![
                f("SCOPE003", "warning", "TS"),
                f("SCOPE003", "warning", "PI"),
            ],
        ),
        ("malformed/m5_unbalanced_braces.wrl", vec![]),
    ];
    for (name, want) in cases {
        let lf = fixture(name);
        let mut baseline: Option<Vec<(String, String)>> = None;
        for (tag, bytes) in variants(&lf) {
            let dir = TempDir::new("variants");
            let path = dir.0.join("case.wrl");
            fs::write(&path, &bytes).unwrap();
            let svc = Service::new(clock);
            let doc = opened(svc.open_path(&path));
            let a = svc.analyze(doc.session).unwrap();
            assert_eq!((a.session, a.revision), (doc.session, doc.revision));
            assert_eq!(findings(&doc.view, &a), want, "{name}/{tag}");
            // Parser diagnostics stay a separate list.
            assert!(a.diagnostics.iter().all(|d| !d.code.starts_with("SCOPE")));
            let msgs: Vec<_> = a
                .scope_diagnostics
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect();
            match &baseline {
                None => baseline = Some(msgs),
                Some(b) => assert_eq!(b, &msgs, "{name}/{tag}: findings differ"),
            }
            // Analysis changed nothing; Save keeps every byte.
            assert_eq!(
                svc.text(doc.session).unwrap(),
                files::decode(&bytes).unwrap().0
            );
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
    // The m6 X_ITE preview failure is recorded separately (see the 2B doc):
    // this test needs no renderer.
}

#[test]
fn scene_tree_carries_typed_answers() {
    let svc = Service::new(clock);
    let dir = TempDir::new("tree");
    let path = dir.0.join("x.wrl");
    fs::write(&path, fixture("corrected/08_proto_script_inline_valid.wrl")).unwrap();
    let doc = opened(svc.open_path(&path));
    let a = svc.analyze(doc.session).unwrap();
    let routes: Vec<_> = a.items.iter().filter(|i| i.kind == "Route").collect();
    assert_eq!(routes.len(), 3);
    let summaries: Vec<_> = routes
        .iter()
        .map(|i| {
            let t = i.typed.as_ref().expect("ROUTE has a typed answer");
            assert_eq!((t.subject.as_str(), t.class.as_str()), ("route", "proven"));
            t.summary.clone().unwrap()
        })
        .collect();
    assert_eq!(
        summaries,
        [
            "SFBool -> SFBool",
            "SFFloat -> SFFloat",
            "SFFloat -> SFFloat"
        ]
    );
    // The PROTO instance names its declaration; built-ins carry nothing.
    let inst = a
        .items
        .iter()
        .find(|i| view_slice(&doc.view, i.view_from, i.view_to).starts_with("DEF Inst Pulse"))
        .unwrap();
    let t = inst.typed.as_ref().unwrap();
    assert_eq!(
        (t.subject.as_str(), t.status.as_str(), t.summary.as_deref()),
        ("type", "resolved", Some("PROTO Pulse"))
    );
    let clk = a
        .items
        .iter()
        .find(|i| view_slice(&doc.view, i.view_from, i.view_to).starts_with("DEF Clk "))
        .unwrap();
    assert!(clk.typed.is_none());

    // An uncertain node gives an uncertain ROUTE verdict, never "proven".
    fs::write(
        &path,
        "#VRML V2.0 utf8\nDEF A TimeSensor { }\nDEF A TimeSensor { }\nDEF F ScalarInterpolator { }\nROUTE A.fraction_changed TO F.set_fraction\n",
    )
    .unwrap();
    let doc = opened(svc.open_path(&path));
    let a = svc.analyze(doc.session).unwrap();
    let r = a.items.iter().find(|i| i.kind == "Route").unwrap();
    let t = r.typed.as_ref().unwrap();
    assert_eq!(
        (t.status.as_str(), t.class.as_str()),
        ("ambiguous", "uncertain")
    );
    assert!(t.summary.is_none());
    assert!(a
        .scope_diagnostics
        .iter()
        .all(|d| !d.code.starts_with("SCOPE1")));
}

/// 18 + 19: identical names in two sessions; an older revision's answer is
/// never the current one.
#[test]
fn typed_analyses_are_bound_to_session_and_revision() {
    let svc = Service::new(clock);
    let dir = TempDir::new("sessions");
    let (pa, pb) = (dir.0.join("a.wrl"), dir.0.join("b.wrl"));
    let a_text =
        "#VRML V2.0 utf8\nPROTO P [ field SFVec3f s 1 1 1 ] { Box { size IS s } }\nP { }\n";
    fs::write(&pa, a_text).unwrap();
    fs::write(
        &pb,
        "#VRML V2.0 utf8\nPROTO P [ field SFFloat s 1 ] { Box { size IS s } }\nP { }\n",
    )
    .unwrap();
    let (da, db) = (opened(svc.open_path(&pa)), opened(svc.open_path(&pb)));
    assert_ne!(da.session, db.session);
    let (aa, ab) = (
        svc.analyze(da.session).unwrap(),
        svc.analyze(db.session).unwrap(),
    );
    assert!(aa.scope_diagnostics.is_empty());
    assert_eq!(
        findings(&db.view, &ab),
        vec![f("SCOPE114", "warning", "size IS s")]
    );

    // Change A's interface type: revision 1 has the finding, revision 0
    // did not, and each analysis names its own revision.
    let at = da.view.find("SFVec3f").unwrap() as u64;
    let r = svc
        .edit(&p::EditRequest {
            session: da.session,
            base_revision: 0,
            from: at,
            to: at + "SFVec3f".len() as u64,
            insert: "SFFloat".into(),
            item: None,
        })
        .unwrap();
    assert!(matches!(r, p::EditOutcome::Applied { .. }));
    let a1 = svc.analyze(da.session).unwrap();
    assert_eq!((aa.revision, a1.revision), (0, 1));
    assert_ne!(aa.view_hash, a1.view_hash);
    let codes: Vec<_> = a1
        .scope_diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["SCOPE114"]);
    // An edit based on the old revision is refused; the buffer is unchanged.
    let stale = svc
        .edit(&p::EditRequest {
            session: da.session,
            base_revision: 0,
            from: 0,
            to: 0,
            insert: "x".into(),
            item: None,
        })
        .unwrap();
    assert!(matches!(stale, p::EditOutcome::Refused { .. }));
    assert_eq!(svc.analyze(da.session).unwrap(), a1);
    // Session B is untouched by A's edit.
    assert_eq!(svc.analyze(db.session).unwrap(), ab);
    // Analysis never edits: the file on disk is still the original.
    assert_eq!(fs::read_to_string(&pa).unwrap(), a_text);
}
