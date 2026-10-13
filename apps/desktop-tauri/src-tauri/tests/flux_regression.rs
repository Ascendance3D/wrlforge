// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-1: source-preservation regression tests.
//!
//! A competing tool rewrote VRML97 on save (added `Material`, `startTime`,
//! `creaseAngle`; dropped normals; triangulated quads; reordered ROUTEs).
//! WRL Forge must keep the exact source text. These tests drive the real
//! `Service` (the same path the UI uses) over small original fixtures in
//! `tests/fixtures/flux-regression/`:
//!
//! - A: open -> save (no edit) -> save_as keeps the text, across LF, CRLF,
//!   BOM+CRLF and gzip variants.
//! - B: the positive fixtures analyze with no error-severity diagnostic.
//! - C: Inspector and gizmo edits change exactly the intended token, and undo
//!   restores the original text byte-for-byte.
//! - D: intentionally malformed fixtures open and save without crashing and
//!   without changing a byte.
//!
//! Fixtures are read from disk at run time; temp files live under
//! `std::env::temp_dir()` and are removed by a drop guard, even on panic.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wrl_forge_desktop::files::{self, Format};
use wrl_forge_desktop::service::Service;
use wrlforge_desktop_protocol as p;

/// Fixed clock so backup names are deterministic.
fn clock() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_500_000)
}

/// The positive fixtures (LF, as authored): parse-clean, used for source
/// preservation and edit tests. All are valid VRML97 EXCEPT
/// `08_proto_script_inline.wrl`, which binds `field SFFloat size` to
/// `Box.size` (SFVec3f) with IS, an ISO 4.8.3 type mismatch (RUST-STAB-2B,
/// SCOPE114). It stays here unchanged: source preservation must hold for
/// invalid VRML too. Its valid counterpart is
/// `corrected/08_proto_script_inline_valid.wrl`.
const POSITIVE: &[&str] = &[
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
];

/// The intentionally malformed fixtures. Never treated as valid.
const MALFORMED: &[&str] = &[
    "m1_cylinder_topradius.wrl",
    "m2_route_no_such_eventout.wrl",
    "m3_route_component_access.wrl",
    "m4_def_two_type_names.wrl",
    "m5_unbalanced_braces.wrl",
    "m6_route_before_def.wrl",
];

/// Line-ending and encoding variants applied to each fixture.
#[derive(Clone, Copy, Debug)]
enum Variant {
    /// Plain UTF-8, LF (the file as authored).
    Lf,
    /// Plain UTF-8, CRLF line endings.
    Crlf,
    /// UTF-8 byte-order mark plus CRLF line endings.
    BomCrlf,
    /// gzip-compressed LF text (a Mall-style artifact).
    Gzip,
}

const VARIANTS: [Variant; 4] = [Variant::Lf, Variant::Crlf, Variant::BomCrlf, Variant::Gzip];

/// Reads a fixture under `tests/fixtures/flux-regression/` (or `malformed/`).
fn read_fixture(name: &str) -> Vec<u8> {
    let rel = if MALFORMED.contains(&name) {
        format!("tests/fixtures/flux-regression/malformed/{name}")
    } else {
        format!("tests/fixtures/flux-regression/{name}")
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The source text a variant must present, BOM included.
fn expected_text(lf: &str, v: Variant) -> String {
    let crlf = lf.replace('\n', "\r\n");
    match v {
        Variant::Lf | Variant::Gzip => lf.to_string(),
        Variant::Crlf => crlf,
        Variant::BomCrlf => format!("\u{feff}{crlf}"),
    }
}

/// The bytes a variant is written to disk as.
fn variant_bytes(lf: &str, v: Variant) -> Vec<u8> {
    match v {
        Variant::Gzip => files::encode(lf, Format::Gzip).unwrap(),
        _ => expected_text(lf, v).into_bytes(),
    }
}

/// Decodes on-disk bytes (plain or gzip) to their text.
fn decoded(bytes: &[u8]) -> String {
    files::decode(bytes).unwrap().0
}

/// A temp directory removed on drop, so a failing test leaves nothing behind.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "wrlforge-rust-stab-1-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes one fixture variant into a fresh temp dir and returns its path.
fn write_variant(dir: &TempDir, name: &str, v: Variant) -> PathBuf {
    let lf = String::from_utf8(read_fixture(name)).unwrap();
    let path = dir.path().join(format!("case-{name}"));
    fs::write(&path, variant_bytes(&lf, v)).unwrap();
    path
}

fn opened(o: p::OpenOutcome) -> p::DocumentInfo {
    match o {
        p::OpenOutcome::Opened { doc } => doc,
        other => panic!("not opened: {other:?}"),
    }
}

/// The `ROUTE` statements of a text, in order.
fn route_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("ROUTE "))
        .collect()
}

/// Pairs of whitespace-separated tokens that differ. Panics when the token
/// counts differ, because a change that adds or removes a token is not a
/// one-token edit.
fn differing_tokens<'a>(before: &'a str, after: &'a str) -> Vec<(&'a str, &'a str)> {
    let a: Vec<&str> = before.split_whitespace().collect();
    let b: Vec<&str> = after.split_whitespace().collect();
    assert_eq!(a.len(), b.len(), "token count changed");
    a.into_iter().zip(b).filter(|(x, y)| x != y).collect()
}

// ---------------------------------------------------------------------------
// A. open -> save (no edit) -> save_as preserves the text
// ---------------------------------------------------------------------------

/// For every positive fixture and every variant: open, save without an edit,
/// then save_as. The disk bytes (plain) or decoded text (gzip) must match the
/// input, and no Material/startTime/creaseAngle may appear that the source
/// lacked. ROUTE lines must keep their original order.
#[test]
fn positive_fixtures_save_and_save_as_preserve_source_text() {
    for name in POSITIVE {
        let lf = String::from_utf8(read_fixture(name)).unwrap();
        for v in VARIANTS {
            let ctx = format!("{name} [{v:?}]");
            let dir = TempDir::new("a-preserve");
            let path = write_variant(&dir, name, v);
            let original_bytes = fs::read(&path).unwrap();
            let want = expected_text(&lf, v);

            let svc = Service::new(clock);
            let doc = opened(svc.open_path(&path));
            assert_eq!(svc.text(doc.session).unwrap(), want, "open text {ctx}");

            // Save with no edit.
            let saved = svc.save(doc.session);
            let p::SaveOutcome::Saved { preserved, .. } = saved else {
                panic!("save not Saved {ctx}: {saved:?}");
            };
            let disk = fs::read(&path).unwrap();
            match v {
                Variant::Gzip => {
                    // An unchanged gzip artifact is preserved (no write), per
                    // CLAUDE.md. The decoded text must be identical either way.
                    assert!(preserved, "unchanged gzip not preserved {ctx}");
                    assert_eq!(decoded(&disk), want, "gzip save text {ctx}");
                }
                _ => {
                    assert_eq!(disk, original_bytes, "save bytes {ctx}");
                }
            }

            // save_as to a new path, then compare decoded text.
            let copy = dir.path().join("copy-saved-as.wrl");
            let as_out = svc.save_as(doc.session, &copy);
            assert!(
                matches!(as_out, p::SaveOutcome::Saved { .. }),
                "save_as {ctx}: {as_out:?}"
            );
            let copy_bytes = fs::read(&copy).unwrap();
            let out = decoded(&copy_bytes);
            assert_eq!(out, want, "save_as text {ctx}");
            if !matches!(v, Variant::Gzip) {
                assert_eq!(copy_bytes, original_bytes, "save_as bytes {ctx}");
            }

            // No invented content: markers the source lacked must not appear.
            for marker in ["Material", "startTime", "creaseAngle"] {
                if !want.contains(marker) {
                    assert!(!out.contains(marker), "gained {marker} {ctx}");
                }
            }
            // ROUTE order is unchanged.
            assert_eq!(route_lines(&out), route_lines(&want), "ROUTE order {ctx}");
        }
    }
}

// ---------------------------------------------------------------------------
// B. the positive fixtures analyze with no error-severity diagnostic
// ---------------------------------------------------------------------------

/// Every positive fixture analyzes with zero diagnostics of severity
/// "error". Duplicate DEF names (09) must not be a syntax error. Warnings are
/// allowed and printed for the record.
#[test]
fn positive_fixtures_analyze_without_error_diagnostics() {
    for name in POSITIVE {
        let dir = TempDir::new("b-analyze");
        let path = write_variant(&dir, name, Variant::Lf);
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a = svc.analyze(doc.session).unwrap();
        let errors: Vec<_> = a
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .collect();
        assert!(errors.is_empty(), "{name}: error diagnostics {errors:?}");
        for d in a.diagnostics.iter().filter(|d| d.severity != "error") {
            eprintln!("note {name}: {} {} {}", d.severity, d.code, d.message);
        }
    }
}

// ---------------------------------------------------------------------------
// C. token-exact editor actions, with exact undo
// ---------------------------------------------------------------------------

/// Fixture 01: an Inspector edit of TimeSensor.cycleInterval (4 -> 6) changes
/// exactly one token, leaves every ROUTE line intact, and undo restores the
/// original text exactly. Runs on LF and BOM+CRLF.
#[test]
fn anim_cycle_interval_edit_changes_one_token_and_undo_restores() {
    for v in [Variant::Lf, Variant::BomCrlf] {
        let dir = TempDir::new("c-cycle");
        let path = write_variant(&dir, "01_anim_no_starttime.wrl", v);
        let lf = String::from_utf8(read_fixture("01_anim_no_starttime.wrl")).unwrap();
        let before = expected_text(&lf, v);
        let svc = Service::new(clock);
        let doc = opened(svc.open_path(&path));
        let a = svc.analyze(doc.session).unwrap();
        let ts = a
            .items
            .iter()
            .find(|i| i.label.starts_with("TimeSensor"))
            .unwrap_or_else(|| panic!("no TimeSensor item: {:?}", a.items));
        let insp = match svc.inspect(doc.session, &ts.id, a.revision).unwrap() {
            p::InspectOutcome::Found { inspection } => inspection,
            other => panic!("inspect not found: {other:?}"),
        };
        let node = insp.node.expect("TimeSensor is a node");
        let field = node
            .fields
            .iter()
            .find(|f| f.name == "cycleInterval")
            .expect("cycleInterval field");
        // ISO/IEC 14772-1: TimeSensor.cycleInterval is SFTime.
        assert_eq!(field.field_type.as_deref(), Some("SFTime"));

        let out = svc
            .edit_field(&p::FieldEditRequest {
                confirm_shared: false,
                session: doc.session,
                base_revision: a.revision,
                item: ts.id.clone(),
                field_index: field.index,
                field_name: "cycleInterval".into(),
                components: vec![p::FieldInput::Text { value: "6".into() }],
            })
            .unwrap();
        let p::FieldEditOutcome::Applied { .. } = out else {
            panic!("cycleInterval edit not applied ({v:?}): {out:?}");
        };

        let after = svc.text(doc.session).unwrap();
        assert_eq!(
            differing_tokens(&before, &after),
            vec![("4", "6")],
            "{v:?}: exactly one token must change"
        );
        assert_eq!(
            after,
            before.replacen("cycleInterval 4", "cycleInterval 6", 1)
        );
        assert_eq!(route_lines(&after), route_lines(&before), "{v:?}: ROUTEs");

        // Undo restores the exact original text.
        let undone = svc.undo(doc.session).unwrap();
        assert!(
            matches!(undone, p::HistoryOutcome::Applied { .. }),
            "{undone:?}"
        );
        assert_eq!(svc.text(doc.session).unwrap(), before, "{v:?}: undo");
    }
}

/// Fixture 07: the Inspector edit of the DEF Material's diffuseColor changes
/// only the green component token. The `material USE Mat` in the second
/// Shape stays, and undo restores the original text.
#[test]
fn shared_material_edit_changes_only_definition_token_and_keeps_use() {
    let dir = TempDir::new("c-material");
    let path = write_variant(&dir, "07_shared_material.wrl", Variant::Lf);
    let before = String::from_utf8(read_fixture("07_shared_material.wrl")).unwrap();
    let svc = Service::new(clock);
    let doc = opened(svc.open_path(&path));
    let a = svc.analyze(doc.session).unwrap();
    // The DEF site is the Material item that is not itself a USE.
    let mat = a
        .items
        .iter()
        .find(|i| i.label.starts_with("Material") && i.use_status.is_none())
        .unwrap_or_else(|| panic!("no DEF Material item: {:?}", a.items));
    let insp = match svc.inspect(doc.session, &mat.id, a.revision).unwrap() {
        p::InspectOutcome::Found { inspection } => inspection,
        other => panic!("inspect not found: {other:?}"),
    };
    let node = insp.node.expect("Material is a node");
    let field = node
        .fields
        .iter()
        .find(|f| f.name == "diffuseColor")
        .expect("diffuseColor field");

    let out = svc
        .edit_field(&p::FieldEditRequest {
            confirm_shared: false,
            session: doc.session,
            base_revision: a.revision,
            item: mat.id.clone(),
            field_index: field.index,
            field_name: "diffuseColor".into(),
            components: ["0.25", "0.9", "0.75"]
                .iter()
                .map(|v| p::FieldInput::Text {
                    value: v.to_string(),
                })
                .collect(),
        })
        .unwrap();
    let p::FieldEditOutcome::Applied { .. } = out else {
        panic!("diffuseColor edit not applied: {out:?}");
    };

    let after = svc.text(doc.session).unwrap();
    assert_eq!(
        differing_tokens(&before, &after),
        vec![("0.5", "0.9")],
        "exactly the green token changes"
    );
    assert!(after.contains("material USE Mat"), "USE must survive");
    assert_eq!(
        after,
        before.replacen(
            "diffuseColor 0.25 0.5 0.75",
            "diffuseColor 0.25 0.9 0.75",
            1
        )
    );

    let undone = svc.undo(doc.session).unwrap();
    assert!(
        matches!(undone, p::HistoryOutcome::Applied { .. }),
        "{undone:?}"
    );
    assert_eq!(svc.text(doc.session).unwrap(), before, "undo restores");
}

/// Fixture 05: a gizmo Move of the top-level Transform on the Y axis changes
/// only the translation token. All six quad `coordIndex` lines (four indices
/// then -1) stay exactly as authored; there is no triangulation. Undo restores
/// the original text.
#[test]
fn cube_translate_changes_one_token_keeps_quads_and_undo_restores() {
    let dir = TempDir::new("c-translate");
    let path = write_variant(&dir, "05_ifs_quads.wrl", Variant::Lf);
    let before = String::from_utf8(read_fixture("05_ifs_quads.wrl")).unwrap();
    let svc = Service::new(clock);
    let doc = opened(svc.open_path(&path));
    let a = svc.analyze(doc.session).unwrap();
    let tr = a
        .items
        .iter()
        .find(|i| i.label == "Transform Cube")
        .unwrap_or_else(|| panic!("no Transform Cube item: {:?}", a.items));

    let out = svc
        .translate(&p::TranslateRequest {
            session: doc.session,
            base_revision: a.revision,
            item: tr.id.clone(),
            axis: p::gizmo::Axis::Y,
            value: 1.5,
            decimals: 3,
        })
        .unwrap();
    // Refusals leave the text unchanged; this fixture is expected to apply.
    let p::TranslateOutcome::Applied { text, .. } = out else {
        panic!("translate not applied: {out:?}");
    };
    assert_eq!(text, "1.5");

    let after = svc.text(doc.session).unwrap();
    assert_eq!(
        after,
        before.replacen("translation 0 0 0", "translation 0 1.5 0", 1)
    );
    assert_eq!(
        differing_tokens(&before, &after),
        vec![("0", "1.5")],
        "exactly the Y translation token changes"
    );
    // Quad faces survive untouched.
    let quads = |t: &str| -> Vec<String> {
        t.lines()
            .map(str::trim)
            .filter(|l| l.ends_with(" -1") && l.split_whitespace().count() == 5)
            .map(str::to_string)
            .collect()
    };
    assert_eq!(quads(&after).len(), 6, "six quad faces");
    assert_eq!(quads(&after), quads(&before), "coordIndex unchanged");

    let undone = svc.undo(doc.session).unwrap();
    assert!(
        matches!(undone, p::HistoryOutcome::Applied { .. }),
        "{undone:?}"
    );
    assert_eq!(svc.text(doc.session).unwrap(), before, "undo restores");
}

// ---------------------------------------------------------------------------
// D. malformed fixtures: no crash, no byte change on save
// ---------------------------------------------------------------------------

/// Every malformed fixture opens (or fails cleanly) without panicking. When it
/// opens, analyze runs and a save without an edit leaves the bytes identical.
/// Unbalanced braces (m5) and a DEF with two type names (m4) must produce at
/// least one diagnostic. The ROUTE semantic errors (m2, m3, m6) are not
/// required to be diagnosed in this lane.
#[test]
fn malformed_fixtures_open_save_without_crash_or_byte_change() {
    for name in MALFORMED {
        let dir = TempDir::new("d-malformed");
        let path = write_variant(&dir, name, Variant::Lf);
        let original = fs::read(&path).unwrap();
        let svc = Service::new(clock);
        match svc.open_path(&path) {
            p::OpenOutcome::Opened { doc } => {
                let a = svc
                    .analyze(doc.session)
                    .unwrap_or_else(|e| panic!("{name}: analyze error {e}"));
                if matches!(
                    *name,
                    "m4_def_two_type_names.wrl" | "m5_unbalanced_braces.wrl"
                ) {
                    assert!(
                        !a.diagnostics.is_empty(),
                        "{name}: expected at least one diagnostic"
                    );
                }
                for d in &a.diagnostics {
                    eprintln!("note {name}: {} {} {}", d.severity, d.code, d.message);
                }
                let saved = svc.save(doc.session);
                assert!(
                    matches!(saved, p::SaveOutcome::Saved { .. }),
                    "{name}: save {saved:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), original, "{name}: bytes changed");
            }
            p::OpenOutcome::Failed { message } => {
                // A clean refusal is acceptable; the file must be untouched.
                eprintln!("note {name}: open failed cleanly: {message}");
                assert_eq!(fs::read(&path).unwrap(), original, "{name}: bytes changed");
            }
            p::OpenOutcome::Canceled => panic!("{name}: unexpected cancel"),
        }
    }
}
