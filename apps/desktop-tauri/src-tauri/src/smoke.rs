// SPDX-License-Identifier: GPL-3.0-or-later
//! `--smoke <file> [--smoke-report <json>]`: an end-to-end self-test through
//! the REAL window, WebView, Wasm UI and IPC.
//!
//! Rust opens `<file>` (always a disposable copy -- the harness never points
//! this at a user file). The UI drives its own widget: it checks the editor
//! shows the document, types a line through a genuine `input` event, undoes,
//! redoes, saves, selects a Scene Tree item, and tries the preview. It reports
//! each step back; Rust then independently verifies the bytes on disk and the
//! backup, prints one JSON report, and exits 0 (all pass) or 1.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::AppHandle;
use wrlforge_desktop_protocol as p;

use crate::files::{self, Format};
use wrlforge_document::EolCounts;

pub const INSERT: &str = "# smoke: añadido ✓ 😀";
/// The value the `--smoke-inspector` run writes through the Inspector.
pub const INSPECTOR_VALUE: &str = "0.25";

struct Plan {
    path: PathBuf,
    original_bytes: Vec<u8>,
    original_text: String,
    format: Format,
    report_path: Option<PathBuf>,
    expect_preview: bool,
    inspector: bool,
    theme: Option<p::ThemeSmoke>,
    settings_path: Option<PathBuf>,
    /// The settings file as it was before the run (None = absent).
    settings_before: Option<Vec<u8>>,
}

/// `--smoke-create <dir>` (VISUAL-1). The directory is a disposable one
/// under the system temp dir; Rust names the single file the workflow
/// saves, so the native Save As / Open dialogs and the discard confirmation
/// are answered by Rust from this plan, never by the WebView.
struct CreateArm {
    save_path: PathBuf,
    report_path: Option<PathBuf>,
    /// New World confirmations, in order: Cancel first, then Discard.
    confirms: VecDeque<bool>,
    /// Confirmations asked so far (Rust checks the count at the end).
    asked: usize,
}

pub const CREATE_FILE: &str = "visual1-world.wrl";
pub const CREATE_TRANSLATION: [&str; 3] = ["1.5", "0.5", "-1"];
pub const CREATE_COLOR: [&str; 3] = ["0.1", "0.6", "0.9"];

/// `--smoke-pick <dir>` (VISUAL-2). `dir` holds `plan.json` and the
/// fixture files `smoke-pick-plan.cjs` wrote. The UI opens a fixture by
/// INDEX; the path never leaves Rust.
struct PickArm {
    report_path: Option<PathBuf>,
    save_path: PathBuf,
    /// (path, the exact bytes at arm time): picks must never change them.
    files: Vec<(PathBuf, Vec<u8>)>,
    fixtures: Vec<p::PickFixture>,
}

pub const PICK_FILE: &str = "visual2-world.wrl";

/// `--smoke-move <dir>` (VISUAL-3A). Rust writes the source-format fixtures
/// into the disposable `dir` and keeps their original bytes; the UI opens
/// them by INDEX and moves one object in each, then saves.
struct MoveArm {
    report_path: Option<PathBuf>,
    save_path: PathBuf,
    /// (path, original bytes, label).
    fixtures: Vec<(PathBuf, Vec<u8>, String)>,
}

pub const MOVE_FILE: &str = "visual3a-world.wrl";

/// `--smoke-texture <dir>` (TEXTURE-LOCAL-1). `dir` holds `plan.json` and
/// the fixtures `smoke-texture-plan.py` wrote. The UI opens a fixture by
/// INDEX; the path never leaves Rust.
struct TextureArm {
    dir: PathBuf,
    report_path: Option<PathBuf>,
    /// The fixture documents, in plan order.
    docs: Vec<PathBuf>,
    fixtures: Vec<p::TextureFixture>,
    /// Every regular file under `dir` at arm time, with its bytes.
    files: Vec<(PathBuf, Vec<u8>)>,
}

/// `--smoke-extrusion <dir>` (VISUAL-4A). Rust writes the source-format
/// fixtures into the disposable `dir`; `smoke.sh --research` may add copies
/// of research files named `research-*.wrl` there first. The UI opens them
/// by INDEX; paths never leave Rust.
struct ExtrusionArm {
    dir: PathBuf,
    report_path: Option<PathBuf>,
    save_path: PathBuf,
    /// (path, original bytes, label, the exact text the UI edit must leave).
    fixtures: Vec<(PathBuf, Vec<u8>, String, String)>,
    /// (path, original bytes, name): opened and rendered, never edited.
    research: Vec<(PathBuf, Vec<u8>, String)>,
}

pub const EXTRUSION_FILE: &str = "visual4a-world.wrl";

/// The outline the in-window outline-editing session must leave (see
/// `ui/src/smoke/extrusion.rs`): Set point 3, drag point 4 (snapped), Add
/// point, Insert after point 2, Delete point 6 -- one Apply.
pub const EXTRUSION_FINAL_OUTLINE: &str =
    "crossSection [ 1 0, 0.5 -0.9, -0.05 -0.95, -0.6 -1, -1.5 0, 0.5 0.866, 0.75 0.433, 1 0 ]";

/// The exact text the New World → Create Extrusion workflow must save.
pub fn extrusion_expected() -> String {
    use wrlforge_vrml::create::{plan_create, Plan, Primitive, NEW_WORLD};
    let Plan::Ready { new_text, .. } = plan_create(NEW_WORLD, Primitive::Extrusion) else {
        panic!("create refused")
    };
    new_text
        .replacen(
            "crossSection [ 1 0, 0.5 -0.866, -0.5 -0.866, -1 0, -0.5 0.866, 0.5 0.866, 1 0 ]",
            EXTRUSION_FINAL_OUTLINE,
            1,
        )
        .replacen(
            "spine [ 0 -1 0, 0 1 0 ]",
            "spine [ 0 -1 0, 0 1.5 0 ]\n        scale [ 1.25 1.25 ]\n        orientation [ 0 1 0 0.3 ]\n        beginCap FALSE",
            1,
        )
}

/// (file, label, text): one Extrusion inside a DEF'd Shape that is USEd,
/// beside a PROTO, with comments and Unicode, in LF, BOM + CRLF and lone-CR
/// forms. The UI changes ONE crossSection coordinate (`0 -1` -> `0 -1.5`).
pub fn extrusion_fixtures() -> Vec<(&'static str, &'static str, String)> {
    let doc = |bom: &str, eol: &str| {
        [
            "#VRML V2.0 utf8",
            "# VISUAL-4A fixture ✓ — größe 😀",
            "PROTO Keep [ field SFFloat k 1 ] { Group {} }",
            "DEF Prism Transform {",
            "  translation 0 0 0",
            "  children [",
            "    DEF Body Shape {",
            "      appearance Appearance { material Material { diffuseColor 0.3 0.6 0.9 } }",
            "      geometry Extrusion {",
            "        crossSection [ 1 0, 0 -1, -1 0, 0 1, 1 0 ] # diamond ✓",
            "        spine [ 0 -1 0, 0 1 0 ]",
            "        ccw TRUE",
            "      }",
            "    }",
            "  ]",
            "}",
            "Transform { translation 3 0 0 children [ USE Body ] }",
            "Keep { k 2 }",
            "# fin ✓",
            "",
        ]
        .join(eol)
        .replacen('#', &format!("{bom}#"), 1)
    };
    vec![
        (
            "fx-ext-lf.wrl",
            "LF + comments + DEF/USE + PROTO",
            doc("", "\n"),
        ),
        (
            "fx-ext-crlf-bom.wrl",
            "BOM + CRLF + Unicode",
            doc("\u{feff}", "\r\n"),
        ),
        ("fx-ext-cr.wrl", "lone CR", doc("", "\r")),
    ]
}

/// The text a fixture must hold after the in-window edit.
pub fn extrusion_fixture_expected(original: &str) -> String {
    original.replacen("[ 1 0, 0 -1, -1 0,", "[ 1 0, 0 -1.5, -1 0,", 1)
}

/// `--smoke-rotscale <dir>` (VISUAL-4B). Rust writes the fixtures into the
/// disposable `dir`; the UI opens them by INDEX.
struct RotScaleArm {
    report_path: Option<PathBuf>,
    save_path: PathBuf,
    /// (path, original bytes, label, line indexes the run may change).
    fixtures: Vec<(PathBuf, Vec<u8>, String, Vec<usize>)>,
}

/// B3 (LV2), smoke runs only: every shared-definition consent the run was
/// asked, as (PROTO name, instance count) -- the words the dialog states.
static CONSENTS: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());

pub fn note_consent(proto: &str, instances: u32) {
    if let Ok(mut v) = CONSENTS.lock() {
        v.push((proto.to_string(), instances));
    }
}

pub const ROTSCALE_FILE: &str = "visual4b-world.wrl";

/// (file, label, text, changeable line indexes): a translated, rotated and
/// centered `Turn`, an instanced `Shared` (refused), a `Bare` Transform
/// without rotation (written explicitly in the first fixture only), a PROTO,
/// comments and Unicode, in LF, BOM + CRLF and lone-CR forms.
pub fn rotscale_fixtures() -> Vec<(&'static str, &'static str, String, Vec<usize>)> {
    let lines = [
        "#VRML V2.0 utf8",
        "# VISUAL-4B fixture ✓ — größe 😀",
        "PROTO Keep [ field SFFloat k 1 ] { Group {} }",
        "DEF Turn Transform {",
        "  translation 1 0.5 0 # placed ✓",
        "  rotation 0 0 1 0.5",
        "  scale 1 1.5 1",
        "  center 0.25 0 0",
        "  children [",
        "    Shape { appearance Appearance { material Material { diffuseColor 0.9 0.5 0.2 } } geometry Box { size 1 1 1 } }",
        "  ]",
        "}",
        "DEF Shared Transform { translation -2.5 0 0 rotation 0 1 0 0 scale 1 1 1 children [ Shape { geometry Sphere { radius 0.5 } } ] }",
        "Transform { translation -2.5 2 0 children [ USE Shared ] }",
        "DEF Bare Transform { translation 2.5 -1.5 0 children [ Shape { appearance Appearance { material Material { diffuseColor 0.2 0.8 0.3 } } geometry Cone { bottomRadius 0.5 height 1 } } ] }",
        "Keep { k 2 }",
        "# fin ✓",
        "",
    ];
    let doc = |bom: &str, eol: &str| lines.join(eol).replacen('#', &format!("{bom}#"), 1);
    vec![
        (
            "fx-rs-lf.wrl",
            "LF + comments + DEF/USE + PROTO",
            doc("", "\n"),
            vec![5, 6, 14],
        ),
        (
            PROTO_TWO_FILE,
            PROTO_TWO_LABEL,
            proto_rotscale_fixture(PROTO_TWO_TAIL),
            PROTO_BODY_LINES.to_vec(),
        ),
        (
            PROTO_NESTED_FILE,
            PROTO_NESTED_LABEL,
            proto_rotscale_fixture(PROTO_NESTED_TAIL),
            PROTO_BODY_LINES.to_vec(),
        ),
        (
            "fx-rs-crlf-bom.wrl",
            "BOM + CRLF + Unicode",
            doc("\u{feff}", "\r\n"),
            vec![5, 6],
        ),
        ("fx-rs-cr.wrl", "lone CR", doc("", "\r"), vec![5, 6]),
    ]
}

/// B3 (LV2): the shared-PROTO Rotate / Scale fixtures. Every label starts
/// with "shared PROTO" (the UI run and `verify_rotscale` branch on it).
pub const PROTO_TWO_FILE: &str = "fx-rs-proto-two.wrl";
pub const PROTO_TWO_LABEL: &str = "shared PROTO: two instances + DEF/USE in the body";
pub const PROTO_NESTED_FILE: &str = "fx-rs-proto-nested.wrl";
pub const PROTO_NESTED_LABEL: &str = "shared PROTO: an instance under nested Transforms";
/// The body Transform's rotation and scale lines (0-based).
pub const PROTO_BODY_LINES: [usize; 2] = [7, 8];
/// One top-level instance at the origin, one inside a Transform at y = -2.5.
pub const PROTO_TWO_TAIL: &str = "Wheel { }\nTransform { translation 0 -2.5 0 children [ Wheel { tint 0.2 0.6 0.9 } ] }\n";
/// One top-level instance, one under Transform (rotated, uniformly scaled)
/// > Group > Transform: its parent frame is composed from all three.
pub const PROTO_NESTED_TAIL: &str = "Wheel { }\nTransform {\n  translation 0 -2 0 rotation 0 0 1 0.4 scale 1.5 1.5 1.5\n  children [\n    Group { children [\n      Transform { translation 1.5 0 0 children [ Wheel { tint 0.2 0.6 0.9 } ] }\n    ] }\n  ]\n}\n";

/// The shared `Wheel` PROTO: a body Transform `Spin` (explicit rotation and
/// scale, one Shape) under a Group, beside a second body Transform whose
/// Shape USEs Spin's Appearance (DEF/USE inside the body); then `tail` (the
/// instances).
pub fn proto_rotscale_fixture(tail: &str) -> String {
    [
        "#VRML V2.0 utf8",
        "# LV2 shared PROTO ✓ — größe 😀",
        "PROTO Wheel [ field SFColor tint 0.9 0.4 0.2 ] {",
        "  Group {",
        "    children [",
        "      DEF Spin Transform {",
        "        translation 0 0 0",
        "        rotation 0 0 1 0.25",
        "        scale 1 1 1",
        "        children [ Shape { appearance DEF Paint Appearance { material Material { diffuseColor IS tint } } geometry Box { size 1.2 0.6 0.6 } } ]",
        "      }",
        "      Transform { translation 0 0.75 0 children [ Shape { appearance USE Paint geometry Sphere { radius 0.2 } } ] }",
        "    ]",
        "  }",
        "}",
    ]
    .join("\n")
        + "\n"
        + tail
        + "# fin ✓\n"
}

/// Whether `after` differs from `before` only on the lines `allowed`
/// (same line endings, same BOM, same line count), each changed line keeping
/// its leading field name.
pub fn only_lines_changed(before: &str, after: &str, eol: &str, allowed: &[usize]) -> bool {
    let (a, b): (Vec<_>, Vec<_>) = (before.split(eol).collect(), after.split(eol).collect());
    if a.len() != b.len() || before.starts_with('\u{feff}') != after.starts_with('\u{feff}') {
        return false;
    }
    a.iter().zip(&b).enumerate().all(|(i, (x, y))| {
        x == y
            || (allowed.contains(&i)
                && x.split_whitespace().next() == y.split_whitespace().next()
                && wrlforge_vrml::parse(after).diagnostics.is_empty())
    }) && a != b
}

/// Every regular file below `dir` (symlinks are not followed).
fn regular_files(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let t = e.file_type().map_err(|e| e.to_string())?;
        if t.is_dir() {
            regular_files(&e.path(), out)?;
        } else if t.is_file() {
            out.push((e.path(), std::fs::read(e.path()).map_err(|e| e.to_string())?));
        }
    }
    Ok(())
}

/// (file, label, text): LF / CRLF + BOM / lone CR, comments, Unicode, and a
/// nested Transform the gizmo must refuse.
pub fn move_fixtures() -> Vec<(&'static str, &'static str, String)> {
    use wrlforge_vrml::create::{template, Primitive};
    let doc = |bom: &str, eol: &str, def: &str, extra: &str| {
        let body = template(Primitive::Box, def, eol).replace(
            &format!("scale 1 1 1{eol}"),
            &format!("scale 1 1 1 # größe ✓ 😀{eol}"),
        );
        format!(
            "{bom}#VRML V2.0 utf8{eol}# 世界 — VISUAL-3A fixture ✓{eol}WorldInfo {{ title \"héllo 😀\" }}{eol}{body}{eol}{extra}# fin ✓{eol}"
        )
    };
    let nested = |eol: &str| {
        format!(
            "Transform {{ translation 0 -2.5 0 children [{eol}  DEF Inner Transform {{ translation 0 0 0 children [ Shape {{ appearance Appearance {{ material Material {{ diffuseColor 0.9 0.8 0.1 }} }} geometry Sphere {{ radius 0.4 }} }} ] }}{eol}] }}{eol}"
        )
    };
    vec![
        (
            "fx-crlf-bom.wrl",
            "BOM + CRLF + comments + Unicode",
            doc("\u{feff}", "\r\n", "Box_ü", &nested("\r\n")),
        ),
        ("fx-cr.wrl", "lone CR + Unicode", doc("", "\r", "Box_ü", "")),
        (
            "fx-lf.wrl",
            "LF + comments",
            doc("", "\n", "Box_1", "").replace("translation 0 0 0", "translation 0.0 0.0 0.0"),
        ),
        (TWINS_FILE, TWINS_LABEL, twins_fixture()),
        (PROTO_FILE, PROTO_LABEL, proto_fixture()),
    ]
}

/// B3: the shared-PROTO fixture's file name and label.
pub const PROTO_FILE: &str = "fx-proto-body.wrl";
pub const PROTO_LABEL: &str = "shared PROTO body Transform";

/// B3: a Transform in a PROTO body (under a Group whose bboxSize is an IS,
/// like the Chevy's), and TWO instances: one top-level, one inside a moved
/// Transform. A scene-tree selection cannot say which instance is meant;
/// a real click on the top-level one binds that one.
pub fn proto_fixture() -> String {
    "#VRML V2.0 utf8\n# B3 — shared PROTO body ✓\nPROTO Car [ field SFVec3f bboxSize 2 2 2 ] {\n  Group {\n    bboxSize IS bboxSize\n    children [\n      DEF CarXForm Transform {\n        translation 0 0 0\n        children [ Shape { appearance Appearance { material Material { diffuseColor 0.2 0.5 0.9 } } geometry Box { } } ]\n      }\n    ]\n  }\n}\nCar { }\nTransform { translation 0 -3 0 children [ Car { } ] }\n# fin ✓\n".into()
}

/// VISUAL-3A1: the twin fixture's file name and label.
pub const TWINS_FILE: &str = "fx-twins-viewpoint.wrl";
pub const TWINS_LABEL: &str = "twin Transforms + authored Viewpoint";

/// Two ANONYMOUS top-level Transforms with the SAME translation, geometry
/// and material (name, type, value and appearance cannot tell them apart),
/// then an authored Viewpoint whose span shifts when the first twin's
/// translation token changes length.
pub fn twins_fixture() -> String {
    let twin = "Transform {\n  translation 1.5 0 0\n  children [\n    Shape { appearance Appearance { material Material { diffuseColor 0.8 0.3 0.2 } } geometry Box { } }\n  ]\n}\n";
    format!(
        "#VRML V2.0 utf8\n# twins ✓ — VISUAL-3A1 fixture\n{twin}{twin}Viewpoint {{ position 1.5 0.5 9 description \"twins ✓\" }}\n# fin ✓\n"
    )
}

#[derive(Default)]
pub struct SmokeState(
    Mutex<Option<Plan>>,
    Mutex<Option<CreateArm>>,
    Mutex<Option<PickArm>>,
    Mutex<Option<MoveArm>>,
    Mutex<Option<TextureArm>>,
    Mutex<Option<ExtrusionArm>>,
    Mutex<Option<RotScaleArm>>,
);

impl SmokeState {
    pub fn arm(
        &self,
        path: PathBuf,
        report_path: Option<PathBuf>,
        expect_preview: bool,
        inspector: bool,
        theme: Option<p::ThemeSmoke>,
        settings_path: Option<PathBuf>,
    ) -> Result<(), String> {
        let settings_before = settings_path.as_ref().and_then(|sp| std::fs::read(sp).ok());
        let original_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let (original_text, format) = files::decode(&original_bytes).map_err(|e| e.to_string())?;
        *self.0.lock().unwrap() = Some(Plan {
            path,
            original_bytes,
            original_text,
            format,
            report_path,
            expect_preview,
            inspector,
            theme,
            settings_path,
            settings_before,
        });
        Ok(())
    }
    /// Arm the VISUAL-1 workflow in `dir`, which must be an existing
    /// directory under the system temp dir and must not hold the file yet.
    pub fn arm_create(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = dir.canonicalize().map_err(|e| e.to_string())?;
        let tmp = std::env::temp_dir()
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !dir.starts_with(&tmp) || dir == tmp {
            return Err(format!(
                "refusing a non-temporary directory: {}",
                dir.display()
            ));
        }
        let save_path = dir.join(CREATE_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        *self.1.lock().unwrap() = Some(CreateArm {
            save_path,
            report_path,
            confirms: VecDeque::from([false, true]),
            asked: 0,
        });
        Ok(())
    }

    /// Arm the VISUAL-2 picking run from `dir/plan.json` (written by
    /// `smoke-pick-plan.cjs`). Every fixture must be a plain file directly
    /// in `dir` (no separators, no symlinks), and `dir` must be temporary.
    pub fn arm_pick(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        #[derive(serde::Deserialize)]
        struct Fx {
            id: String,
            file: String,
            camera: [f64; 3],
            clicks: Vec<p::PickClick>,
        }
        #[derive(serde::Deserialize)]
        struct Plan {
            fixtures: Vec<Fx>,
        }
        let raw = std::fs::read_to_string(dir.join("plan.json")).map_err(|e| e.to_string())?;
        let plan: Plan = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let mut files = Vec::new();
        let mut fixtures = Vec::new();
        for f in plan.fixtures {
            if f.file.is_empty() || f.file.contains(['/', '\\']) || f.file.starts_with('.') {
                return Err(format!("bad fixture file name {:?}", f.file));
            }
            let path = dir.join(&f.file);
            let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err(format!("{} is not a plain file", f.file));
            }
            files.push((
                path.clone(),
                std::fs::read(&path).map_err(|e| e.to_string())?,
            ));
            fixtures.push(p::PickFixture {
                id: f.id,
                camera: f.camera,
                clicks: f.clicks,
            });
        }
        let save_path = dir.join(PICK_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        *self.2.lock().unwrap() = Some(PickArm {
            report_path,
            save_path,
            files,
            fixtures,
        });
        Ok(())
    }

    /// Arm the VISUAL-3A gizmo run in `dir` (temporary, must not hold the
    /// fixture or world files yet). The fixtures are written here, by Rust.
    pub fn arm_move(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        let save_path = dir.join(MOVE_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        let mut fixtures = Vec::new();
        for (file, label, text) in move_fixtures() {
            let path = dir.join(file);
            if path.exists() {
                return Err(format!("{} already exists", path.display()));
            }
            std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
            fixtures.push((path, text.into_bytes(), label.to_string()));
        }
        *self.3.lock().unwrap() = Some(MoveArm {
            report_path,
            save_path,
            fixtures,
        });
        Ok(())
    }

    /// Arm the TEXTURE-LOCAL-1 run from `dir/plan.json`. Every fixture
    /// document must be a plain file below `dir` (normal relative
    /// components only), and `dir` must be temporary.
    pub fn arm_texture(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        #[derive(serde::Deserialize)]
        struct Fx {
            id: String,
            file: String,
            label: String,
            samples: Vec<p::TextureSample>,
            warnings: usize,
        }
        #[derive(serde::Deserialize)]
        struct Plan {
            fixtures: Vec<Fx>,
        }
        let raw = std::fs::read_to_string(dir.join("plan.json")).map_err(|e| e.to_string())?;
        let plan: Plan = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let mut docs = Vec::new();
        let mut fixtures = Vec::new();
        for f in plan.fixtures {
            let rel = Path::new(&f.file);
            if f.file.is_empty()
                || !rel
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
            {
                return Err(format!("bad fixture file name {:?}", f.file));
            }
            let path = dir.join(rel);
            let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err(format!("{} is not a plain file", f.file));
            }
            docs.push(path);
            fixtures.push(p::TextureFixture {
                id: f.id,
                label: f.label,
                samples: f.samples,
                warnings: f.warnings,
            });
        }
        let mut files = Vec::new();
        regular_files(&dir, &mut files)?;
        *self.4.lock().unwrap() = Some(TextureArm {
            dir,
            report_path,
            docs,
            fixtures,
            files,
        });
        Ok(())
    }

    /// Arm the VISUAL-4A run in the temporary `dir`.
    pub fn arm_extrusion(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        let save_path = dir.join(EXTRUSION_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        let mut fixtures = Vec::new();
        for (file, label, text) in extrusion_fixtures() {
            let path = dir.join(file);
            if path.exists() {
                return Err(format!("{} already exists", path.display()));
            }
            std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
            let want = extrusion_fixture_expected(&text);
            fixtures.push((path, text.into_bytes(), label.to_string(), want));
        }
        let mut research = Vec::new();
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("research-") && n.ends_with(".wrl"))
            .collect();
        names.sort();
        for n in names {
            let path = dir.join(&n);
            let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err(format!("{n} is not a plain file"));
            }
            research.push((
                path.clone(),
                std::fs::read(&path).map_err(|e| e.to_string())?,
                n,
            ));
        }
        *self.5.lock().unwrap() = Some(ExtrusionArm {
            dir,
            report_path,
            save_path,
            fixtures,
            research,
        });
        Ok(())
    }

    /// Arm the VISUAL-4B run in the temporary `dir`.
    pub fn arm_rotscale(&self, dir: &Path, report_path: Option<PathBuf>) -> Result<(), String> {
        let dir = temp_dir_checked(dir)?;
        let save_path = dir.join(ROTSCALE_FILE);
        if save_path.exists() {
            return Err(format!("{} already exists", save_path.display()));
        }
        let mut fixtures = Vec::new();
        for (file, label, text, allowed) in rotscale_fixtures() {
            let path = dir.join(file);
            if path.exists() {
                return Err(format!("{} already exists", path.display()));
            }
            std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
            fixtures.push((path, text.into_bytes(), label.to_string(), allowed));
        }
        *self.6.lock().unwrap() = Some(RotScaleArm {
            report_path,
            save_path,
            fixtures,
        });
        Ok(())
    }

    /// `--smoke-extrusion`, `--smoke-move` and `--smoke-rotscale` only
    /// (inside the harness Xvfb): a whole-screen capture as evidence,
    /// `<dir>/captures/<id>.png` (B3 LV2: the move and rotscale runs too).
    pub fn window_capture(&self, id: &str) -> Result<String, String> {
        let dir = {
            let ext = self.5.lock().map_err(|e| e.to_string())?;
            let mv = self.3.lock().map_err(|e| e.to_string())?;
            let rs = self.6.lock().map_err(|e| e.to_string())?;
            ext.as_ref()
                .map(|a| a.dir.clone())
                .or_else(|| mv.as_ref().and_then(|a| a.save_path.parent().map(Path::to_path_buf)))
                .or_else(|| rs.as_ref().and_then(|a| a.save_path.parent().map(Path::to_path_buf)))
                .ok_or("no extrusion, move or rotscale smoke run is armed")?
        };
        real_pointer_env()?;
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("bad capture id".into());
        }
        let d = dir.join("captures");
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let out = d.join(format!("{id}.png"));
        let st = std::process::Command::new("timeout")
            .args(["10", "import", "-window", "root"])
            .arg(&out)
            .status()
            .map_err(|e| format!("import: {e}"))?;
        if !st.success() {
            return Err(format!("import failed: {st}"));
        }
        Ok(format!("captures/{id}.png"))
    }

    /// Whether the TEXTURE-LOCAL-1 run is armed (its test-only commands
    /// refuse otherwise).
    pub fn texture_armed(&self) -> bool {
        self.4.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// `--smoke-texture` only: store one viewport capture as evidence in
    /// `<dir>/captures/<id>.png` (a new folder; fixture files are checked
    /// against their arm-time bytes, captures are not fixtures).
    pub fn texture_capture(&self, id: &str, png: &[u8]) -> Result<String, String> {
        let dir = match self.4.lock().map_err(|e| e.to_string())?.as_ref() {
            Some(a) => a.dir.clone(),
            None => self
                .5
                .lock()
                .map_err(|e| e.to_string())?
                .as_ref()
                .map(|a| a.dir.clone())
                .ok_or("no texture or extrusion smoke run is armed")?,
        };
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("bad capture id".into());
        }
        if !png.starts_with(b"\x89PNG") {
            return Err("not a PNG".into());
        }
        let d = dir.join("captures");
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        std::fs::write(d.join(format!("{id}.png")), png).map_err(|e| e.to_string())?;
        Ok(format!("captures/{id}.png"))
    }

    /// Smoke only: fixture `index` of the picking or gizmo run.
    pub fn pick_fixture(&self, index: usize) -> Option<PathBuf> {
        if crate::smoke_dock::armed() {
            return crate::smoke_dock::fixture(index);
        }
        if crate::smoke_menu::armed() {
            return crate::smoke_menu::fixture(index);
        }
        if let Some(a) = self.6.lock().ok()?.as_ref() {
            return a.fixtures.get(index).map(|f| f.0.clone());
        }
        if let Some(a) = self.5.lock().ok()?.as_ref() {
            let fx = a.fixtures.iter().map(|f| f.0.clone());
            return fx.chain(a.research.iter().map(|r| r.0.clone())).nth(index);
        }
        if let Some(a) = self.4.lock().ok()?.as_ref() {
            return a.docs.get(index).cloned();
        }
        if let Some(a) = self.3.lock().ok()?.as_ref() {
            return a.fixtures.get(index).map(|(p, _, _)| p.clone());
        }
        let g = self.2.lock().ok()?;
        g.as_ref()?.files.get(index).map(|(p, _)| p.clone())
    }

    /// Smoke only: the Save As destination (instead of the native dialog).
    pub fn save_as_override(&self) -> Option<PathBuf> {
        if crate::smoke_recovery::armed() {
            return crate::smoke_recovery::save_as_override();
        }
        if let Some(p) = self.6.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p);
        }
        if let Some(p) = self.5.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p);
        }
        if let Some(p) = self.3.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p);
        }
        if let Some(p) = self.2.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p);
        }
        self.1.lock().ok()?.as_ref().map(|a| a.save_path.clone())
    }

    /// Smoke only: the Open choice -- the file this run saved, once it exists.
    pub fn open_override(&self) -> Option<PathBuf> {
        if let Some(p) = self.6.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p).filter(|p| p.exists());
        }
        if let Some(p) = self.5.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p).filter(|p| p.exists());
        }
        if let Some(p) = self.3.lock().ok()?.as_ref().map(|a| a.save_path.clone()) {
            return Some(p).filter(|p| p.exists());
        }
        let g = self.1.lock().ok()?;
        let a = g.as_ref()?;
        Some(a.save_path.clone()).filter(|p| p.exists())
    }

    /// Smoke only: the discard-confirmation answer. An unplanned question
    /// is answered Cancel (never discards).
    pub fn confirm_override(&self) -> Option<bool> {
        if crate::smoke_recovery::armed() {
            return crate::smoke_recovery::confirm_override();
        }
        if crate::smoke_menu::armed() {
            return crate::smoke_menu::confirm();
        }
        // The picking and gizmo runs start a fresh New World per theme: discard.
        if self.2.lock().ok()?.is_some()
            || self.3.lock().ok()?.is_some()
            || self.5.lock().ok()?.is_some()
            || self.6.lock().ok()?.is_some()
        {
            return Some(true);
        }
        let mut g = self.1.lock().ok()?;
        let a = g.as_mut()?;
        a.asked += 1;
        Some(a.confirms.pop_front().unwrap_or(false))
    }

    pub fn plan(&self) -> Option<p::SmokePlan> {
        let hold = std::env::var("WRLFORGE_SMOKE_ALIGN_HOLD_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0u64)
            .min(10_000);
        if let Some(a) = self.6.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                pick: None,
                gizmo: None,
                texture: None,
                extrusion: None,
                rotscale: Some(p::RotScaleSmoke {
                    fixtures: a.fixtures.iter().map(|f| f.2.clone()).collect(),
                    real_pointer: real_pointer_env().is_ok(),
                }),
            });
        }
        if let Some(a) = self.5.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                pick: None,
                gizmo: None,
                texture: None,
                extrusion: Some(p::ExtrusionSmoke {
                    fixtures: a.fixtures.iter().map(|f| f.2.clone()).collect(),
                    research: a.research.iter().map(|r| r.2.clone()).collect(),
                    hold_ms: hold,
                    window_capture: real_pointer_env().is_ok(),
                }),
                rotscale: None,
            });
        }
        if let Some(a) = self.4.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                pick: None,
                gizmo: None,
                texture: Some(p::TextureSmoke {
                    fixtures: a.fixtures.clone(),
                }),
                extrusion: None,
                rotscale: None,
            });
        }
        if let Some(a) = self.3.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                pick: None,
                gizmo: Some(p::GizmoSmoke {
                    themes: p::theme::THEMES.iter().map(|t| t.id.to_string()).collect(),
                    fixtures: a.fixtures.iter().map(|(_, _, l)| l.clone()).collect(),
                    hold_ms: hold,
                    real_pointer: real_pointer_env().is_ok(),
                }),
                texture: None,
                extrusion: None,
                rotscale: None,
            });
        }
        if let Some(a) = self.2.lock().ok()?.as_ref() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: None,
                gizmo: None,
                texture: None,
                extrusion: None,
                rotscale: None,
                pick: Some(p::PickSmoke {
                    fixtures: a.fixtures.clone(),
                    themes: p::theme::THEMES.iter().map(|t| t.id.to_string()).collect(),
                    hold_ms: hold,
                    real_pointer: real_pointer_env().is_ok(),
                }),
            });
        }
        if self.1.lock().ok()?.is_some() {
            return Some(p::SmokePlan {
                insert_text: String::new(),
                expect_preview: true,
                inspector_value: None,
                theme: None,
                syntax_align_hold_ms: 0,
                create: Some(p::CreateSmoke {
                    translation: CREATE_TRANSLATION.map(String::from).to_vec(),
                    color: CREATE_COLOR.map(String::from).to_vec(),
                    themes: p::theme::THEMES.iter().map(|t| t.id.to_string()).collect(),
                    hold_ms: hold,
                }),
                pick: None,
                gizmo: None,
                texture: None,
                extrusion: None,
                rotscale: None,
            });
        }
        self.0.lock().ok()?.as_ref().map(|pl| p::SmokePlan {
            insert_text: INSERT.into(),
            expect_preview: pl.expect_preview,
            inspector_value: pl.inspector.then(|| INSPECTOR_VALUE.to_string()),
            theme: pl.theme.clone(),
            // Opt-in visual registration pauses (never set by default).
            syntax_align_hold_ms: hold,
            create: None,
            pick: None,
            gizmo: None,
            texture: None,
            extrusion: None,
            rotscale: None,
        })
    }
}

/// The private Xvfb screen `smoke.sh --headless` creates.
const XVFB_GEOMETRY: &str = "1600 1000";

/// Real pointer input is allowed ONLY when `smoke.sh` launched this process
/// inside its own Xvfb server (`WRLFORGE_SMOKE_XVFB=1`) with a DISPLAY set.
/// UI-DOCK-1: whether real X input may be used (the same rule).
pub fn real_pointer_available() -> bool {
    real_pointer_env().is_ok()
}

fn real_pointer_env() -> Result<String, String> {
    if std::env::var("WRLFORGE_SMOKE_XVFB").as_deref() != Ok("1") {
        return Err("not inside the harness's Xvfb (WRLFORGE_SMOKE_XVFB unset)".into());
    }
    std::env::var("DISPLAY").map_err(|_| "no DISPLAY".to_string())
}

fn xdotool(args: &[&str]) -> Result<String, String> {
    eprintln!("smoke-pick: xdotool {}", args.join(" "));
    let out = std::process::Command::new("timeout")
        .arg("5")
        .arg("xdotool")
        .args(args)
        .output()
        .map_err(|e| format!("xdotool: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "xdotool {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl SmokeState {
    /// Smoke only (`--smoke-pick` under `smoke.sh --headless`): one REAL X
    /// primary-button click at root coordinates (`x`, `y`).
    ///
    /// Input is sent only after proving where it goes: the harness's Xvfb
    /// (environment marker + the exact screen geometry it created), a window
    /// owned by THIS process, and that window holding the input focus.
    pub fn real_click(&self, x: i32, y: i32) -> Result<String, String> {
        self.real_pointer("click", x, y)
    }

    /// Smoke only (`--smoke-pick` / `--smoke-move` under `smoke.sh
    /// --headless`): one REAL X input action at root coordinates: `click`,
    /// `down` (press at x, y), `move` (to x, y, button state unchanged), `up`
    /// (release at x, y) or `escape` (the Escape key); VISUAL-3A1 camera
    /// tests add `down2` / `up2` (middle button: X_ITE Examine pan),
    /// `wheelup` / `wheeldown` (one wheel notch at x, y: zoom) and `resize`
    /// (OUR window to x by y px). Same guards as a click.
    pub fn real_pointer(&self, action: &str, x: i32, y: i32) -> Result<String, String> {
        let armed = self.2.lock().map_err(|e| e.to_string())?.is_some()
            || self.3.lock().map_err(|e| e.to_string())?.is_some()
            || self.6.lock().map_err(|e| e.to_string())?.is_some()
            || crate::smoke_dock::armed()
            || crate::smoke_menu::armed();
        if !armed {
            return Err("no picking or gizmo smoke run is armed".into());
        }
        let display = real_pointer_env()?;
        let geo = xdotool(&["getdisplaygeometry"])?;
        if geo != XVFB_GEOMETRY {
            return Err(format!(
                "display {display} is {geo}, not the harness Xvfb ({XVFB_GEOMETRY})"
            ));
        }
        let pid = std::process::id().to_string();
        let mine = xdotool(&["search", "--onlyvisible", "--pid", &pid])?;
        let mine: Vec<&str> = mine.lines().filter(|l| !l.is_empty()).collect();
        if mine.is_empty() {
            return Err(format!("no visible window of pid {pid}"));
        }
        // A bare Xvfb has no window manager (focus = PointerRoot): give the
        // focus to OUR window, then require that it holds it.
        let held = xdotool(&["getwindowfocus"]).ok();
        if !held.as_deref().is_some_and(|f| mine.contains(&f)) {
            xdotool(&["windowfocus", "--sync", mine[0]])?;
        }
        let focus = xdotool(&["getwindowfocus"])?;
        if !mine.contains(&focus.as_str()) {
            return Err(format!("focus {focus} is not a window of pid {pid}"));
        }
        let (xs, ys) = (x.to_string(), y.to_string());
        // No `--sync`: it waits for pointer MOTION, which never comes when
        // the pointer is already at the target (a repeated click).
        match action {
            "click" => xdotool(&["mousemove", &xs, &ys, "click", "1"])?,
            "down" => xdotool(&["mousemove", &xs, &ys, "mousedown", "1"])?,
            "move" => xdotool(&["mousemove", &xs, &ys])?,
            "up" => xdotool(&["mousemove", &xs, &ys, "mouseup", "1"])?,
            "escape" => xdotool(&["key", "Escape"])?,
            "down2" => xdotool(&["mousemove", &xs, &ys, "mousedown", "2"])?,
            "up2" => xdotool(&["mousemove", &xs, &ys, "mouseup", "2"])?,
            "wheelup" => xdotool(&["mousemove", &xs, &ys, "click", "4"])?,
            "wheeldown" => xdotool(&["mousemove", &xs, &ys, "click", "5"])?,
            // UI-DOCK-1: keyboard operation of tabs and dividers (fixed set).
            k if k.starts_with("key-") => {
                let name = &k[4..];
                const KEYS: &[&str] = &[
                    "Right",
                    "Left",
                    "Up",
                    "Down",
                    "Home",
                    "End",
                    "shift+Right",
                    "shift+Left",
                    "Delete",
                    "Return",
                    "Tab",
                    // UI-MENU-1: menu access, menu navigation and the
                    // application shortcuts (Ctrl+Alt = AltGr must not run one).
                    "F10",
                    "alt",
                    "alt+f",
                    "alt+e",
                    "alt+v",
                    "alt+t",
                    "Escape",
                    "space",
                    "ctrl+s",
                    "ctrl+z",
                    "ctrl+y",
                    "ctrl+shift+z",
                    "ctrl+q",
                    "ctrl+alt+s",
                    "ctrl+alt+z",
                    "a",
                    "b",
                    // B2: Find / Replace / Go to Line and their inputs.
                    "ctrl+f",
                    "ctrl+h",
                    "ctrl+g",
                    "ctrl+a",
                    "2",
                    "9",
                    "z",
                ];
                if !KEYS.contains(&name) {
                    return Err(format!("key {name:?} is not in the smoke key set"));
                }
                xdotool(&["key", name])?
            }
            "resize" => {
                if !(320..=1600).contains(&x) || !(240..=1000).contains(&y) {
                    return Err(format!("resize {x}x{y} outside the harness screen"));
                }
                xdotool(&["windowsize", mine[0], &xs, &ys])?
            }
            other => return Err(format!("unknown real input action {other:?}")),
        };
        Ok(format!(
            "display {display} ({geo}) window {focus} of pid {pid} at {x},{y}"
        ))
    }
}

/// `dir`, canonical, when it is an existing directory strictly under the
/// system temp dir.
fn temp_dir_checked(dir: &Path) -> Result<PathBuf, String> {
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let tmp = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !dir.starts_with(&tmp) || dir == tmp || !dir.is_dir() {
        return Err(format!(
            "refusing a non-temporary directory: {}",
            dir.display()
        ));
    }
    Ok(dir)
}

/// The source text the UI's edit must produce: a new line inserted before the
/// first line break, using the document's dominant line ending.
pub fn expected_text(original: &str) -> String {
    let eol = EolCounts::of(original).dominant().as_str();
    match original.find(['\r', '\n']) {
        Some(i) => format!("{}{eol}{INSERT}{}", &original[..i], &original[i..]),
        None => format!("{original}{eol}{INSERT}"),
    }
}

pub fn finish(app: &AppHandle, report: p::SmokeReport) {
    use tauri::Manager;
    if crate::smoke_recovery::finish(app, &report) {
        return;
    }
    if crate::smoke_dock::finish(app, &report) {
        return;
    }
    if crate::smoke_menu::finish(app, &report) {
        return;
    }
    let state = app.state::<SmokeState>();
    if let Some(arm) = state.6.lock().unwrap().take() {
        let mut steps = report.steps;
        steps.extend(verify_rotscale(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": ROTSCALE_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.5.lock().unwrap().take() {
        let mut steps = report.steps;
        steps.extend(verify_extrusion(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": EXTRUSION_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.4.lock().unwrap().take() {
        crate::resources::TEST_DELAY_MS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut steps = report.steps;
        steps.extend(verify_texture(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "pass": all,
            "file": "texture-local-1",
            "steps": steps,
            "resourceLog": crate::resources::log_snapshot(),
        }))
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.3.lock().unwrap().take() {
        let mut steps = report.steps;
        steps.extend(verify_move(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": MOVE_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.2.lock().unwrap().take() {
        let mut steps = report.steps;
        // Source integrity: no fixture file changed on disk.
        let changed: Vec<String> = arm
            .files
            .iter()
            .filter(|(path, bytes)| std::fs::read(path).ok().as_deref() != Some(&bytes[..]))
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        steps.push(p::SmokeStep {
            name: "rust: every fixture file is byte-identical after the picking run".into(),
            ok: changed.is_empty(),
            detail: format!("{} files checked; changed: {changed:?}", arm.files.len()),
        });
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": PICK_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    if let Some(arm) = state.1.lock().unwrap().take() {
        let mut steps = report.steps;
        steps.extend(verify_create(&arm));
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(
            &serde_json::json!({ "pass": all, "file": CREATE_FILE, "steps": steps }),
        )
        .unwrap();
        println!("{json}");
        if let Some(rp) = arm.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
        return;
    }
    let plan = state.0.lock().unwrap().take();
    let mut steps = report.steps;
    if let Some(pl) = plan {
        let disk = std::fs::read(&pl.path).unwrap_or_default();
        let decoded = files::decode(&disk);
        let want = expected_text(&pl.original_text);
        let matches = |t: &str| {
            if pl.inspector {
                inspector_change_ok(&want, t)
            } else {
                t == want
            }
        };
        let (ok, detail) = match &decoded {
            Ok((t, f)) if *f == pl.format && matches(t) => {
                (true, format!("{} bytes, format {}", disk.len(), f.as_str()))
            }
            Ok((t, f)) => (
                false,
                format!(
                    "format {} (want {}), text {} bytes (want {})",
                    f.as_str(),
                    pl.format.as_str(),
                    t.len(),
                    want.len()
                ),
            ),
            Err(e) => (false, e.to_string()),
        };
        steps.push(p::SmokeStep {
            name: "rust: saved file decodes to exactly the expected source".into(),
            ok,
            detail,
        });
        // Exactly one backup, holding the original bytes.
        let dir = pl.path.parent().map(PathBuf::from).unwrap_or_default();
        let base = pl.path.file_name().unwrap().to_string_lossy().into_owned();
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().is_some_and(|n| {
                            n.to_string_lossy().starts_with(&format!("{base}.bak-"))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let bok = backups.len() == 1
            && std::fs::read(&backups[0]).ok().as_deref() == Some(&pl.original_bytes[..]);
        steps.push(p::SmokeStep {
            name: "rust: one backup with the original bytes".into(),
            ok: bok,
            detail: format!("{} backup(s)", backups.len()),
        });
        let temps = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().contains("wrlforge-tmp"))
                    .count()
            })
            .unwrap_or(0);
        steps.push(p::SmokeStep {
            name: "rust: no temp file left".into(),
            ok: temps == 0,
            detail: format!("{temps}"),
        });
        if let Some(t) = &pl.theme {
            steps.push(theme_settings_step(
                t,
                &pl.settings_path,
                &pl.settings_before,
            ));
        }
        let all = steps.iter().all(|s| s.ok);
        let json = serde_json::to_string_pretty(&serde_json::json!({ "pass": all, "file": pl.path.file_name().map(|n| n.to_string_lossy().into_owned()), "steps": steps })).unwrap();
        println!("{json}");
        if let Some(rp) = pl.report_path {
            let _ = std::fs::write(rp, &json);
        }
        app.exit(if all { 0 } else { 1 });
    }
}

/// The bytes a gizmo move may change: exactly one translation token, every
/// other byte (BOM, line endings, comments, Unicode) identical. Returns the
/// (old, new) token.
pub fn one_translation_token(before: &[u8], after: &[u8]) -> Option<(String, String)> {
    let pre = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let max_suf = before.len().min(after.len()) - pre;
    let suf = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(max_suf)
        .take_while(|(a, b)| a == b)
        .count();
    // Widen to whole tokens (an edit may share leading / trailing digits).
    let delim = |c: u8| c.is_ascii_whitespace() || b"[]{},".contains(&c);
    let mut a = pre;
    while a > 0 && !delim(before[a - 1]) {
        a -= 1;
    }
    let (mut eb, mut ea) = (before.len() - suf, after.len() - suf);
    while eb < before.len() && !delim(before[eb]) {
        eb += 1;
        ea += 1;
    }
    let old = std::str::from_utf8(&before[a..eb]).ok()?;
    let new = std::str::from_utf8(&after[a..ea]).ok()?;
    let line_start = before[..a]
        .iter()
        .rposition(|&c| c == b'\n' || c == b'\r')
        .map_or(0, |i| i + 1);
    let line = std::str::from_utf8(&before[line_start..a]).ok()?;
    let one = !old.is_empty()
        && !new.is_empty()
        && !old.bytes().any(delim)
        && !new.bytes().any(delim)
        && line.trim_start().starts_with("translation ")
        && before[..a] == after[..a]
        && before[eb..] == after[ea..];
    one.then(|| (old.to_string(), new.to_string()))
}

/// Backups of `path` (`<name>.bak-*`) in its directory.
fn backups_of(path: &Path) -> Vec<PathBuf> {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::read_dir(path.parent().unwrap())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(&format!("{name}.bak-")))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn verify_rotscale(arm: &RotScaleArm) -> Vec<p::SmokeStep> {
    use wrlforge_vrml::create::{plan_create, Plan, Primitive, NEW_WORLD};
    let mut steps = Vec::new();
    let saved = std::fs::read(&arm.save_path)
        .ok()
        .and_then(|b| files::decode(&b).ok())
        .map(|(t, _)| t);
    let created = match plan_create(NEW_WORLD, Primitive::Box) {
        Plan::Ready { new_text, .. } => new_text,
        _ => String::new(),
    };
    // Box_1's rotation and scale lines (template lines 3 and 4 after the
    // header line).
    let allowed: Vec<usize> = created
        .split('\n')
        .enumerate()
        .filter(|(_, l)| l.starts_with("  rotation ") || l.starts_with("  scale "))
        .map(|(i, _)| i)
        .collect();
    steps.push(p::SmokeStep {
        name:
            "rust: the saved world equals Create Box with ONLY its rotation and scale lines changed"
                .into(),
        ok: saved
            .as_deref()
            .is_some_and(|t| allowed.len() == 2 && only_lines_changed(&created, t, "\n", &allowed)),
        detail: format!(
            "{:?}",
            saved.as_deref().map(|t| t
                .lines()
                .filter(|l| l.starts_with("  rotation") || l.starts_with("  scale"))
                .collect::<Vec<_>>())
        ),
    });
    for (path, original, label, allowed) in &arm.fixtures {
        let disk = std::fs::read(path).unwrap_or_default();
        let (o, d) = (
            String::from_utf8_lossy(original),
            String::from_utf8_lossy(&disk),
        );
        let eol = if o.contains("\r\n") {
            "\r\n"
        } else if o.contains('\n') {
            "\n"
        } else {
            "\r"
        };
        let b = backups_of(path);
        let backup_ok = b.len() == 1 && std::fs::read(&b[0]).ok().as_deref() == Some(&original[..]);
        if label.starts_with("shared PROTO") {
            // B3 (LV2): the body Transform's rotation and scale lines of the
            // SHARED definition changed, nothing else (instances untouched).
            steps.push(p::SmokeStep {
                name: format!("rust: {label} fixture on disk changed only the PROTO body Transform's rotation / scale lines; one backup holds the original bytes"),
                ok: only_lines_changed(&o, &d, eol, allowed) && backup_ok,
                detail: format!("{} bytes, {} backup(s), changed: {:?}", disk.len(), b.len(),
                    o.split(eol).zip(d.split(eol)).filter(|(x, y)| x != y).map(|(_, y)| y.to_string()).collect::<Vec<_>>()),
            });
            continue;
        }
        let angle_only = d
            .split(eol)
            .nth(5)
            .is_some_and(|l| l.starts_with("  rotation 0 0 1 ") && l != "  rotation 0 0 1 0.5");
        steps.push(p::SmokeStep {
            name: format!("rust: {label} fixture on disk changed only the allowed rotation / scale lines (angle token only for Turn); one backup holds the original bytes"),
            ok: only_lines_changed(&o, &d, eol, allowed) && angle_only && backup_ok,
            detail: format!("{} bytes, {} backup(s), changed: {:?}", disk.len(), b.len(),
                o.split(eol).zip(d.split(eol)).filter(|(x, y)| x != y).map(|(_, y)| y.to_string()).collect::<Vec<_>>()),
        });
    }
    let consents = CONSENTS.lock().map(|v| v.clone()).unwrap_or_default();
    let protos = arm
        .fixtures
        .iter()
        .filter(|f| f.2.starts_with("shared PROTO"))
        .count();
    steps.push(p::SmokeStep {
        name: "rust: each shared-PROTO fixture asked the shared-definition consent once, naming PROTO Wheel and its 2 instances".into(),
        ok: protos == 2 && consents.len() == protos && consents.iter().all(|(n, c)| n == "Wheel" && *c == 2),
        detail: format!("{consents:?}"),
    });
    steps
}

fn verify_extrusion(arm: &ExtrusionArm) -> Vec<p::SmokeStep> {
    let mut steps = Vec::new();
    let saved = std::fs::read(&arm.save_path).ok();
    let text = saved
        .as_deref()
        .and_then(|b| files::decode(b).ok())
        .map(|(t, _)| t);
    let want = extrusion_expected();
    steps.push(p::SmokeStep {
        name: "rust: the saved world is byte-exact: Create Extrusion + 8 edits (outline, spine, scale, orientation, beginCap, outline session)".into(),
        ok: text.as_deref() == Some(want.as_str()),
        detail: match &text {
            Some(t) if *t != want => format!("{} chars, want {}", t.len(), want.len()),
            Some(t) => format!("{} chars", t.len()),
            None => "missing".into(),
        },
    });
    for (path, original, label, want) in &arm.fixtures {
        let disk = std::fs::read(path).unwrap_or_default();
        let b = backups_of(path);
        let backup_ok = b.len() == 1 && std::fs::read(&b[0]).ok().as_deref() == Some(&original[..]);
        steps.push(p::SmokeStep {
            name: format!("rust: {label} fixture on disk holds exactly the one edited coordinate; one backup holds the original bytes"),
            ok: disk == want.as_bytes() && backup_ok,
            detail: format!("{} bytes (want {}), {} backup(s)", disk.len(), want.len(), b.len()),
        });
    }
    let changed: Vec<&str> = arm
        .research
        .iter()
        .filter(|(p, bytes, _)| std::fs::read(p).ok().as_deref() != Some(&bytes[..]))
        .map(|(_, _, n)| n.as_str())
        .collect();
    steps.push(p::SmokeStep {
        name: "rust: every research fixture is byte-identical after the run".into(),
        ok: changed.is_empty(),
        detail: format!("{} checked; changed: {changed:?}", arm.research.len()),
    });
    steps
}

fn verify_move(arm: &MoveArm) -> Vec<p::SmokeStep> {
    let mut steps = Vec::new();
    for (path, original, label) in &arm.fixtures {
        let disk = std::fs::read(path).unwrap_or_default();
        let one = one_translation_token(original, &disk);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let backups: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().is_some_and(|n| {
                            n.to_string_lossy().starts_with(&format!("{name}.bak-"))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let backup_ok =
            backups.len() == 1 && std::fs::read(&backups[0]).ok().as_deref() == Some(&original[..]);
        steps.push(p::SmokeStep {
            name: format!("rust: {label} fixture on disk differs by exactly one translation token; one backup holds the original bytes"),
            ok: one.is_some() && backup_ok,
            detail: format!("{name}: {one:?} · {} backup(s)", backups.len()),
        });
    }
    let saved = std::fs::read(&arm.save_path).ok();
    let parsed = saved
        .as_deref()
        .and_then(|b| files::decode(b).ok())
        .map(|(t, _)| t);
    let ok = parsed.as_deref().is_some_and(|t| {
        let pr = wrlforge_vrml::parse(t);
        !wrlforge_vrml::field_edit::has_blocking_syntax_error(&pr)
            && t.contains("DEF Box_1 Transform")
            && !t.contains("DEF Box_1 Transform {\n  translation 0 0 0")
    });
    steps.push(p::SmokeStep {
        name: "rust: the saved world parses and holds the moved Box_1".into(),
        ok,
        detail: format!("{} bytes", saved.map_or(0, |b| b.len())),
    });
    steps
}

/// The exact texts the VISUAL-1 workflow must leave: `first` after the
/// first Save As (Box, Inspector-edited), `last` after the final Save
/// (plus Sphere, Cone, Cylinder). Derived from the same planner the app
/// uses, then edited exactly as the Inspector edits.
pub fn create_expected() -> (String, String) {
    use wrlforge_vrml::create::{plan_create, Plan, Primitive, NEW_WORLD};
    let next = |t: &str, p| match plan_create(t, p) {
        Plan::Ready { new_text, .. } => new_text,
        Plan::Refused { message, .. } => panic!("{message}"),
    };
    let first = next(NEW_WORLD, Primitive::Box)
        .replacen(
            "translation 0 0 0",
            &format!("translation {}", CREATE_TRANSLATION.join(" ")),
            1,
        )
        .replacen(
            "diffuseColor 0.8 0.3 0.2",
            &format!("diffuseColor {}", CREATE_COLOR.join(" ")),
            1,
        );
    let mut last = first.clone();
    for p in [Primitive::Sphere, Primitive::Cone, Primitive::Cylinder] {
        last = next(&last, p);
    }
    (first, last)
}

/// Rust's independent check of the VISUAL-1 files on disk.
fn verify_create(arm: &CreateArm) -> Vec<p::SmokeStep> {
    let (first, last) = create_expected();
    let step = |name: &str, ok: bool, detail: String| p::SmokeStep {
        name: name.into(),
        ok,
        detail,
    };
    let disk = std::fs::read_to_string(&arm.save_path).unwrap_or_default();
    let parsed = wrlforge_vrml::parse(&disk);
    let dir = arm
        .save_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let backups: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with(&format!("{CREATE_FILE}.bak-")))
        .collect();
    let backup_ok = backups.len() == 1
        && std::fs::read_to_string(dir.join(backups[0]))
            .ok()
            .as_deref()
            == Some(first.as_str());
    vec![
        step(
            "rust: saved world is exactly New World + Box (edited) + Sphere + Cone + Cylinder",
            disk == last,
            format!("{} bytes (want {})", disk.len(), last.len()),
        ),
        step(
            "rust: saved world parses as VRML97 with no diagnostics and 4 top-level objects",
            parsed.diagnostics.is_empty() && parsed.tree.statements.len() == 4,
            format!(
                "{} diagnostics, {} statements",
                parsed.diagnostics.len(),
                parsed.tree.statements.len()
            ),
        ),
        step(
            "rust: the later Save took one backup holding the first Save As text",
            backup_ok,
            format!("{} backup(s)", backups.len()),
        ),
        step(
            "rust: the discard confirmation was asked exactly twice (Cancel, Discard)",
            arm.asked == 2 && arm.confirms.is_empty(),
            format!("asked {}", arm.asked),
        ),
        step(
            "rust: no temp file and no other file in the work directory",
            names.iter().all(|n| {
                n == CREATE_FILE
                    || n.starts_with(&format!("{CREATE_FILE}.bak-"))
                    || n == "report.json"
                    || n == "stdout.txt"
                    || n == "stderr.txt"
                    || n == "config"
                    || n == "shots"
            }),
            names.join(", "),
        ),
    ]
}

/// Rust's own check of the settings file after a theme run: the final theme
/// was persisted, or (save-failure run) the file is byte-identical.
fn theme_settings_step(
    t: &p::ThemeSmoke,
    path: &Option<PathBuf>,
    before: &Option<Vec<u8>>,
) -> p::SmokeStep {
    let Some(path) = path else {
        return p::SmokeStep {
            name: "rust: theme settings file".into(),
            ok: false,
            detail: "no settings path".into(),
        };
    };
    let now = std::fs::read(path).ok();
    if t.expect_save_failure {
        return p::SmokeStep {
            name: "rust: failed theme saves left the settings file byte-identical".into(),
            ok: &now == before,
            detail: format!(
                "{} -> {} bytes",
                before.as_ref().map_or(0, |b| b.len()),
                now.as_ref().map_or(0, |b| b.len())
            ),
        };
    }
    let loaded = now.as_deref().map(crate::settings::parse);
    let ok = loaded
        .as_ref()
        .is_some_and(|l| l.theme_id == t.final_theme && l.notice.is_none());
    p::SmokeStep {
        name: "rust: settings file persists the final theme (schemaVersion 1)".into(),
        ok,
        detail: format!("{:?}", loaded.map(|l| l.theme_id)),
    }
}

fn is_delim(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | '[' | ']' | '{' | '}')
}

/// The single token that differs between `a` and `b`, widened to token
/// boundaries: `(old, new, byte start in a)`. `None` if more than one token
/// (or any delimiter) changed.
pub fn one_token_change(a: &str, b: &str) -> Option<(String, String, usize)> {
    if a == b {
        return None;
    }
    let mut pre = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map(|((i, _), _)| i)
        .unwrap_or(a.len().min(b.len()));
    let mut suf = 0;
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    while suf < ab.len() - pre
        && suf < bb.len() - pre
        && ab[ab.len() - 1 - suf] == bb[bb.len() - 1 - suf]
    {
        suf += 1;
    }
    while pre > 0 && !is_delim(a[..pre].chars().next_back()?) {
        pre -= a[..pre].chars().next_back()?.len_utf8();
    }
    let widen = |t: &str, mut s: usize| {
        while s > 0 && !t.is_char_boundary(t.len() - s) {
            s -= 1;
        }
        while s > 0 && !is_delim(t[t.len() - s..].chars().next().unwrap_or(' ')) {
            let c = t[t.len() - s..].chars().next().unwrap();
            s -= c.len_utf8();
        }
        s
    };
    let suf = widen(a, suf).min(widen(b, suf));
    let old = &a[pre..a.len() - suf];
    let new = &b[pre..b.len() - suf];
    if old.is_empty() || new.is_empty() || old.contains(is_delim) || new.contains(is_delim) {
        return None;
    }
    Some((old.to_string(), new.to_string(), pre))
}

/// The Inspector smoke changed exactly one numeric token, to the planned
/// value, as the second component of `diffuseColor` or `translation`.
pub fn inspector_change_ok(before: &str, after: &str) -> bool {
    let Some((old, new, at)) = one_token_change(before, after) else {
        return false;
    };
    let prior: Vec<&str> = before[..at]
        .split(is_delim)
        .filter(|t| !t.is_empty())
        .collect();
    new == INSPECTOR_VALUE
        && old.parse::<f64>().is_ok()
        && prior.len() >= 2
        && prior[prior.len() - 1].parse::<f64>().is_ok()
        && matches!(prior[prior.len() - 2], "diffuseColor" | "translation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gizmo_fixtures_are_movable_and_the_disk_check_is_one_token_exact() {
        use wrlforge_vrml::manipulate::{plan_translate, translate_target, Axis};
        for (file, _, text) in move_fixtures() {
            let p = wrlforge_vrml::parse(&text);
            assert!(
                !wrlforge_vrml::field_edit::has_blocking_syntax_error(&p),
                "{file}"
            );
            if file == PROTO_FILE {
                // The PROTO body Transform: a target, a commit only with
                // consent, one translation token.
                let at = text.find("DEF CarXForm").unwrap();
                let u16 = |b: usize| text[..b].encode_utf16().count() as u64;
                let end = at + text[at..].find("\n      }").unwrap() + "\n      }".len();
                let (f, t) = (u16(at), u16(end));
                let tg = translate_target(&text, f, t).unwrap();
                assert_eq!(tg.root_index, None);
                assert!(tg.shared.is_some());
                assert!(matches!(
                    plan_translate(&text, f, t, Axis::X, 1.75, 2).0,
                    wrlforge_vrml::field_edit::Plan::Refused { .. }
                ));
                let (plan, _) = wrlforge_vrml::manipulate::plan_translate_with(
                    &text, f, t, Axis::X, 1.75, 2, true,
                );
                let wrlforge_vrml::field_edit::Plan::Ready { new_text, .. } = plan else {
                    panic!("{file}: {plan:?}")
                };
                let got = one_translation_token(text.as_bytes(), new_text.as_bytes());
                assert_eq!(got.as_ref().map(|(_, n)| n.as_str()), Some("1.75"));
                continue;
            }
            let t = p
                .tree
                .statements
                .iter()
                .find_map(|s| match s {
                    wrlforge_vrml::ast::Ast::Node(n)
                        if n.node_type == "Transform"
                            && (n.def.is_some() || file == TWINS_FILE) =>
                    {
                        Some((n.range.start.offset as u64, n.range.end.offset as u64))
                    }
                    _ => None,
                })
                .unwrap();
            assert!(translate_target(&text, t.0, t.1).is_ok(), "{file}");
            let (plan, _) = plan_translate(&text, t.0, t.1, Axis::X, 1.75, 2);
            let wrlforge_vrml::field_edit::Plan::Ready { new_text, .. } = plan else {
                panic!("{file}: {plan:?}")
            };
            let got = one_translation_token(text.as_bytes(), new_text.as_bytes());
            assert_eq!(
                got.as_ref().map(|(_, n)| n.as_str()),
                Some("1.75"),
                "{file}"
            );
            // Any other change is not a one-token move.
            let other = new_text.replacen("fin", "fín", 1);
            assert!(
                one_translation_token(text.as_bytes(), other.as_bytes()).is_none(),
                "{file}"
            );
            assert!(one_translation_token(text.as_bytes(), text.as_bytes()).is_none());
        }
    }

    /// VISUAL-3A guard: the private X_ITE camera surfaces the gizmo needs
    /// are read ONLY in `xite-gizmo-adapter.js` (comments excluded), and that
    /// adapter touches none of the WD2-D parser / hit-test surfaces.
    /// VISUAL-3A1: the rendered Transform is bound by proven provenance. The
    /// gizmo adapter receives the runtime node; root order is only read to
    /// REFUSE a disagreement, and the preview adapter takes the node from the
    /// pick adapter's span lookup of the generation on screen.
    /// B3 (LV2): the shared-PROTO Rotate / Scale fixtures. `Spin` (in the
    /// body of `Wheel`) is a Rotate and a Scale target with consent; its
    /// rotation and scale are exactly the allowed lines; the nested
    /// instance's parent frame composes from its three source ancestors to
    /// the world point the GUI run clicks.
    #[test]
    fn proto_rotscale_fixtures_target_the_shared_body_and_compose_the_nested_frame() {
        use wrlforge_vrml::manipulate::{self as m, Tool};
        let fx: Vec<_> = rotscale_fixtures()
            .into_iter()
            .filter(|f| f.1.starts_with("shared PROTO"))
            .collect();
        assert_eq!(fx.len(), 2);
        for (file, label, text, allowed) in fx {
            let lines: Vec<&str> = text.split('\n').collect();
            assert_eq!(
                allowed.iter().map(|i| lines[*i].trim()).collect::<Vec<_>>(),
                ["rotation 0 0 1 0.25", "scale 1 1 1"],
                "{file}"
            );
            let u16 = |b: usize| text[..b].encode_utf16().count() as u64;
            let b = text.find("DEF Spin").unwrap();
            let e = b + text[b..].find("\n      }").unwrap() + "\n      }".len();
            let (f, t) = (u16(b), u16(e));
            for tool in [Tool::Rotate, Tool::Scale] {
                let tg = m::manip_target(&text, f, t, tool).unwrap();
                let sh = tg.shared.expect("shared");
                assert_eq!((sh.proto.as_str(), sh.instances), ("Wheel", 2), "{label}");
            }
            assert!(matches!(
                m::plan_rotate(&text, f, t, m::Axis::Z, 0.5).0,
                wrlforge_vrml::field_edit::Plan::Refused { .. }
            ));
            let (plan, _) = m::plan_rotate_with(&text, f, t, m::Axis::Z, 0.5, true);
            let wrlforge_vrml::field_edit::Plan::Ready { new_text, .. } = plan else {
                panic!("{file}: {plan:?}")
            };
            assert!(only_lines_changed(&text, &new_text, "\n", &allowed), "{file}");
            if file == PROTO_NESTED_FILE {
                // Transform(0 -2 0, R z 0.4, S 1.5) > Group > Transform(1.5 0 0).
                let span = |needle: &str, close: &str| {
                    let b = text.find(needle).unwrap();
                    let e = b + text[b..].find(close).unwrap() + close.len();
                    (u16(b), u16(e))
                };
                let outer = span("Transform {\n  translation 0 -2 0", "\n}");
                let group = span("Group { children [\n      Transform", "] }\n  ]");
                let group = (group.0, group.1 - 4);
                let inner = span("Transform { translation 1.5 0 0", "] }");
                for tool in [Tool::Move, Tool::Rotate, Tool::Scale] {
                    let mm = m::instance_parent_frame(&text, &[inner, group, outer], tool)
                        .unwrap_or_else(|e| panic!("{tool:?} {e:?}"));
                    let (sn, cs) = 0.4f64.sin_cos();
                    assert!((mm[12] - 2.25 * cs).abs() < 1e-12, "{mm:?}");
                    assert!((mm[13] - (-2.0 + 2.25 * sn)).abs() < 1e-12, "{mm:?}");
                }
            }
        }
    }

    #[test]
    fn root_order_is_never_the_binding_identity() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/static");
        let gizmo = std::fs::read_to_string(dir.join("xite-gizmo-adapter.js")).unwrap();
        let preview = std::fs::read_to_string(dir.join("preview-adapter.js")).unwrap();
        assert!(gizmo.contains("function bind(scene, node, index, name, expected, verify)"));
        assert!(gizmo.contains("sf.getValue() === node"));
        // rootNodes appears twice: in the refusing assertion, and (B2-CAMERA)
        // as the start of the bbox-hint walk, which identifies nothing.
        assert_eq!(gizmo.matches("rootNodes").count(), 2);
        assert!(gizmo.contains("try { push(browser.currentScene.rootNodes); } catch (e) { return out; }"));
        assert!(gizmo.contains("'preview-root-order-disagrees'"));
        assert!(!gizmo.contains("getNamedNode(String(name)); } catch (e) { named = null; }\n      if (!named) return { ok: true"));
        // B3: a PROTO body node binds the picked instance's copy
        // (bodyNodeAt), re-proved as the SAME node before every use.
        // LV2: or the copy of the instance Rust carried through a commit.
        assert!(preview.contains(
            "var node = isBody ? adapter.bodyNodeAt(gen, f, t, null, hint) : adapter.nodeAt(gen, f, t);"
        ));
        assert!(preview.contains(
            "(isBody ? adapter.bodyNodeAt(gen, f, t, node, hint) : adapter.nodeAt(gen, f, t)) === node"
        ));
        assert!(!preview.contains("rootNodes["));
    }

    #[test]
    fn private_xite_camera_access_stays_in_the_gizmo_adapter() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/static");
        let code = |t: &str| {
            t.lines()
                .map(|l| l.split("//").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let private = [
            "getActiveLayer",
            "getViewpoint",
            "getViewMatrix",
            "getProjectionMatrix",
            "getRectangle",
            // VISUAL-3A1 camera carry.
            "'positionOffset'",
            "_fieldOfViewScale",
            "defaultViewpoint",
            // B2-CAMERA view controls.
            "getBBox",
            "X3D.Box3",
            "getModelMatrix",
            "getUserOrientation",
            "resetUserOffsets",
        ];
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path.extension().and_then(|x| x.to_str()) != Some("js") {
                continue;
            }
            let src = code(&std::fs::read_to_string(&path).unwrap());
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name == "xite-gizmo-adapter.js" {
                seen += 1;
                for p in private {
                    assert!(src.contains(p), "the adapter no longer reads {p}");
                }
                for p in [
                    "VRMLParser",
                    "nodeStatement",
                    "getHit",
                    "getParents",
                    ".touch(",
                ] {
                    assert!(!src.contains(p), "the gizmo adapter must not touch {p}");
                }
            } else {
                for p in private {
                    assert!(!src.contains(p), "{name} reads private X_ITE surface {p}");
                }
            }
        }
        assert_eq!(seen, 1);
    }

    #[test]
    fn inspector_change_check_is_token_exact() {
        let a = "Material { diffuseColor 0.046 0.062 0.118 }\r\n";
        assert!(inspector_change_ok(a, &a.replace("0.062", "0.25")));
        assert!(!inspector_change_ok(a, &a.replace("0.046", "0.25")));
        assert!(!inspector_change_ok(
            a,
            &a.replace("0.062 0.118", "0.25 0.2")
        ));
        assert!(!inspector_change_ok(
            a,
            &a.replace("\r\n", "\n").replace("0.062", "0.25")
        ));
        assert!(!inspector_change_ok(a, a));
        let t = "T { translation 1 2 3 } # é😀";
        assert!(inspector_change_ok(t, &t.replace(" 2 ", " 0.25 ")));
        assert_eq!(
            one_token_change("x 12 y", "x 0.25 y"),
            Some(("12".into(), "0.25".into(), 2))
        );
    }
    #[test]
    fn extrusion_expectations_parse_and_differ_by_the_intended_text_only() {
        let t = extrusion_expected();
        let p = wrlforge_vrml::parse(&t);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        assert!(t.contains(EXTRUSION_FINAL_OUTLINE));
        assert!(t.contains("spine [ 0 -1 0, 0 1.5 0 ]\n        scale [ 1.25 1.25 ]\n        orientation [ 0 1 0 0.3 ]\n        beginCap FALSE\n      }"));
        for (_, _, text) in extrusion_fixtures() {
            let want = extrusion_fixture_expected(&text);
            assert_ne!(want, text);
            assert_eq!(want.replacen("0 -1.5,", "0 -1,", 1), text);
            assert!(wrlforge_vrml::parse(&want).diagnostics.is_empty());
        }
    }

    #[test]
    fn create_expected_is_the_edited_box_then_three_more_objects() {
        let (first, last) = create_expected();
        assert!(first.starts_with("#VRML V2.0 utf8\n\nDEF Box_1 Transform {\n"));
        assert!(
            first.contains("translation 1.5 0.5 -1") && first.contains("diffuseColor 0.1 0.6 0.9")
        );
        assert!(last.starts_with(&first));
        let tail = &last[first.len()..];
        let order: Vec<_> = ["DEF Sphere_1", "DEF Cone_1", "DEF Cylinder_1"]
            .iter()
            .map(|n| tail.find(n).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        assert!(wrlforge_vrml::parse(&last).diagnostics.is_empty());
    }

    #[test]
    fn expected_text_inserts_with_the_document_ending() {
        assert_eq!(
            expected_text("a\r\nb\r\n"),
            format!("a\r\n{INSERT}\r\nb\r\n")
        );
        assert_eq!(expected_text("a\nb"), format!("a\n{INSERT}\nb"));
        assert_eq!(expected_text("a"), format!("a\n{INSERT}"));
    }
}

/// Rust's own TEXTURE-LOCAL-1 checks: no fixture file changed, and no file
/// outside its document folder was ever served (traversal / symlink cases).
fn verify_texture(arm: &TextureArm) -> Vec<p::SmokeStep> {
    let changed: Vec<String> = arm
        .files
        .iter()
        .filter(|(path, bytes)| std::fs::read(path).ok().as_deref() != Some(&bytes[..]))
        .map(|(path, _)| {
            path.strip_prefix(&arm.dir)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let log = crate::resources::log_snapshot();
    let escaped: Vec<&p::ResourceLogEntry> = log
        .iter()
        .filter(|e| e.status == 200 && (e.name.contains("outside") || e.name.contains("link")))
        .collect();
    let served = log.iter().filter(|e| e.status == 200).count();
    vec![
        p::SmokeStep {
            name: "rust: every fixture file (documents and images) is byte-identical after the run"
                .into(),
            ok: changed.is_empty(),
            detail: format!("{} files checked; changed: {changed:?}", arm.files.len()),
        },
        p::SmokeStep {
            name: "rust: no file outside a document folder was served".into(),
            ok: escaped.is_empty() && served > 0,
            detail: format!("{served} served, {} requests logged; escaped: {escaped:?}", log.len()),
        },
    ]
}
