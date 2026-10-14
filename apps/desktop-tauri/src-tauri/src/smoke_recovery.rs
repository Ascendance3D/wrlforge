// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-1 recovery smoke (`smoke.sh --recovery`). Three processes over
//! one disposable directory and one disposable config directory:
//!
//! 1. `--smoke-recovery-crash DIR`: opens fixtures, edits them through the
//!    service, waits until the BACKGROUND recovery worker has written every
//!    record and each record re-reads byte-identical, writes
//!    `ready.json` (with its pid), then idles. `smoke.sh` kills it with
//!    SIGKILL: no shutdown code runs.
//! 2. `--smoke-recovery-restore DIR`: the UI drives the startup dialog:
//!    Restore, edit + Undo, Save (as), Discard, a canceled window close, Later.
//! 3. `--smoke-recovery-later DIR`: the records kept by Later and the
//!    canceled close are offered again; Restore and Discard them.
//!
//! Test-only. Every directory must be under the system temp directory.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use wrlforge_desktop_protocol as p;

use crate::files::{self, Format};
use crate::service::{Service, UNTITLED_NAME};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Crash,
    Restore,
    Later,
}

/// The plan the UI receives (`smoke_recovery_plan`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub phase: Phase,
}

struct Arm {
    dir: PathBuf,
    phase: Phase,
    report_path: Option<PathBuf>,
    confirms: VecDeque<bool>,
    close_asked: Vec<bool>,
    saves: usize,
    last_save: Option<PathBuf>,
}

static ARM: Mutex<Option<Arm>> = Mutex::new(None);

/// The text a plain/gzip/BOM fixture has on disk before the run.
pub fn fixtures() -> Vec<(&'static str, Format, String)> {
    vec![
        (
            "plain.wrl",
            Format::Plain,
            "#VRML V2.0 utf8\n# plain LF\nDEF Box1 Transform { children Shape { geometry Box { } } }\n"
                .into(),
        ),
        (
            "packed.wrl",
            Format::Gzip,
            "#VRML V2.0 utf8\r\nGroup { children [ ] }\r\n".into(),
        ),
        (
            "tricky.wrl",
            Format::Plain,
            "\u{FEFF}#VRML V2.0 utf8\r\n# caf\u{e9} \u{1F600} \u{2603}\rDEF A Transform { translation 1 2 3 }\nUSE A\r\nPROTO P [ field SFFloat x 1 ] { Group { } }\r\n{ unclosed [\n"
                .into(),
        ),
        ("clean.wrl", Format::Plain, "#VRML V2.0 utf8\n# never edited\n".into()),
        ("undone.wrl", Format::Plain, "#VRML V2.0 utf8\n# edited, then undone\n".into()),
    ]
}

fn temp_checked(dir: &Path) -> Result<PathBuf, String> {
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

pub fn arm(dir: &Path, phase: Phase, report_path: Option<PathBuf>) -> Result<(), String> {
    let dir = temp_checked(dir)?;
    let confirms = match phase {
        Phase::Crash => VecDeque::new(),
        // Discard plain.wrl: yes. The window close: Cancel.
        Phase::Restore => VecDeque::from([true, false]),
        // Discard the damaged record and the second one: yes, yes.
        Phase::Later => VecDeque::from([true, true]),
    };
    std::fs::create_dir_all(dir.join("out")).map_err(|e| e.to_string())?;
    *ARM.lock().unwrap() = Some(Arm {
        dir,
        phase,
        report_path,
        confirms,
        close_asked: vec![],
        saves: 0,
        last_save: None,
    });
    Ok(())
}

pub fn plan() -> Option<Plan> {
    let g = ARM.lock().ok()?;
    let a = g.as_ref()?;
    (a.phase != Phase::Crash).then_some(Plan { phase: a.phase })
}

pub fn armed() -> bool {
    ARM.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// The next Save As destination: `out/restored-N.wrl`.
pub fn save_as_override() -> Option<PathBuf> {
    let mut g = ARM.lock().ok()?;
    let a = g.as_mut()?;
    a.saves += 1;
    let p = a.dir.join("out").join(format!("restored-{}.wrl", a.saves));
    a.last_save = Some(p.clone());
    Some(p)
}

/// An unplanned question is answered Cancel (never discards).
pub fn confirm_override() -> Option<bool> {
    let mut g = ARM.lock().ok()?;
    let a = g.as_mut()?;
    Some(a.confirms.pop_front().unwrap_or(false))
}

pub fn note_close_asked(answer: bool) {
    if let Ok(mut g) = ARM.lock() {
        if let Some(a) = g.as_mut() {
            a.close_asked.push(answer);
        }
    }
}

fn expected(dir: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join("expected").join(name)).ok()
}

/// Phase 1, on its own thread: real edits, then wait for the background
/// worker (never a forced write) to put every record on disk.
pub fn run_crash(app: AppHandle) {
    let r = crash_steps(&app);
    let dir = ARM.lock().unwrap().as_ref().map(|a| a.dir.clone()).unwrap();
    let json = match r {
        Ok(v) => v,
        Err(e) => serde_json::json!({ "pass": false, "error": e }),
    };
    let _ = std::fs::write(
        dir.join("ready.tmp"),
        serde_json::to_vec_pretty(&json).unwrap(),
    );
    let _ = std::fs::rename(dir.join("ready.tmp"), dir.join("ready.json"));
}

fn crash_steps(app: &AppHandle) -> Result<serde_json::Value, String> {
    let dir = ARM.lock().unwrap().as_ref().map(|a| a.dir.clone()).unwrap();
    let docs = dir.join("docs");
    let exp = dir.join("expected");
    std::fs::create_dir_all(&docs).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&exp).map_err(|e| e.to_string())?;
    let svc = app.state::<Service>();
    let mut sessions = vec![];
    for (name, format, text) in fixtures() {
        let path = docs.join(name);
        std::fs::write(
            &path,
            files::encode(&text, format).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let doc = match svc.open_path(&path) {
            p::OpenOutcome::Opened { doc } => doc,
            o => return Err(format!("open {name}: {o:?}")),
        };
        if name == "clean.wrl" {
            continue;
        }
        // Type at the end of the view (after any BOM).
        let end = doc.view.encode_utf16().count() as u64;
        let o = svc.edit(&p::EditRequest {
            session: doc.session,
            base_revision: doc.revision,
            from: end,
            to: end,
            insert: "# typed before the crash \u{e9}\n".into(),
            item: None,
        })?;
        if !matches!(o, p::EditOutcome::Applied { .. }) {
            return Err(format!("edit {name}: {o:?}"));
        }
        if name == "undone.wrl" {
            svc.undo(doc.session)?;
            continue;
        }
        sessions.push((name.to_string(), doc.session));
    }
    // A new world, never saved: Create a Box.
    let nw = match svc.new_world() {
        p::OpenOutcome::Opened { doc } => doc,
        o => return Err(format!("new world: {o:?}")),
    };
    let o = svc.create(&p::CreateRequest { at: None,
        session: nw.session,
        base_revision: nw.revision,
        primitive: p::Primitive::Box,
    })?;
    if !matches!(o, p::CreateOutcome::Applied { .. }) {
        return Err(format!("create: {o:?}"));
    }
    sessions.push((UNTITLED_NAME.to_string(), nw.session));
    for (name, id) in &sessions {
        std::fs::write(exp.join(name), svc.text(*id)?.as_bytes()).map_err(|e| e.to_string())?;
    }
    // Wait for the scheduled writes; confirm each record re-reads exactly.
    let t0 = Instant::now();
    loop {
        let all = sessions.iter().all(|(_, id)| {
            svc.recovery_record_text(*id).as_deref() == svc.text(*id).ok().as_deref()
        });
        if all {
            break;
        }
        if t0.elapsed() > Duration::from_secs(20) {
            return Err("recovery records were not written within 20 s".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let records = std::fs::read_dir(dir.join("config").join(crate::recovery::DIR_NAME))
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("rec-"))
        .count();
    let writes = svc.recovery().map_or(0, |r| r.writes());
    Ok(serde_json::json!({
        "pass": records == sessions.len(),
        "pid": std::process::id(),
        "records": records,
        "expectedRecords": sessions.len(),
        "workerWrites": writes,
        "msToConfirmed": t0.elapsed().as_millis() as u64,
    }))
}

/// One check, answered in Rust (byte-exact), for a UI step.
pub fn check(
    app: &AppHandle,
    session: Option<p::SessionId>,
    what: &str,
    name: &str,
) -> p::SmokeStep {
    let (ok, detail) = check_inner(app, session, what, name);
    p::SmokeStep {
        name: format!("{what} {name}"),
        ok,
        detail,
    }
}

fn record_count(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("config").join(crate::recovery::DIR_NAME))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("rec-"))
                .count()
        })
        .unwrap_or(usize::MAX)
}

fn check_inner(
    app: &AppHandle,
    session: Option<p::SessionId>,
    what: &str,
    name: &str,
) -> (bool, String) {
    let svc = app.state::<Service>();
    let (dir, last_save, close_asked) = {
        let g = ARM.lock().unwrap();
        let a = g.as_ref().unwrap();
        (a.dir.clone(), a.last_save.clone(), a.close_asked.clone())
    };
    match what {
        // The open session's canonical text equals the pre-crash text.
        "session-exact" => {
            let Some(want) = expected(&dir, name) else {
                return (false, "no expected text".into());
            };
            let got = session.and_then(|s| svc.text(s).ok()).unwrap_or_default();
            (
                got.as_bytes() == want.as_slice(),
                format!("{} B vs {} B", got.len(), want.len()),
            )
        }
        // The last Save As file decodes to the expected text, in the
        // expected format.
        "saved-exact" => {
            let Some(want) = expected(&dir, name) else {
                return (false, "no expected text".into());
            };
            let Some(path) = last_save else {
                return (false, "nothing saved".into());
            };
            let raw = std::fs::read(&path).unwrap_or_default();
            let gz = files::is_gzip(&raw);
            let text = files::decode(&raw).map(|t| t.0).unwrap_or_default();
            let want_gz = name == "packed.wrl";
            (
                text.as_bytes() == want.as_slice() && gz == want_gz,
                format!("gzip {gz} (want {want_gz}), {} B", text.len()),
            )
        }
        "records" => {
            let n = record_count(&dir);
            (n.to_string() == name, format!("{n} record files"))
        }
        // Every original under docs/ is byte-identical to its fixture.
        "originals" => {
            let mut bad = vec![];
            for (n, f, t) in fixtures() {
                let want = files::encode(&t, f).unwrap();
                if std::fs::read(dir.join("docs").join(n)).ok() != Some(want) {
                    bad.push(n);
                }
            }
            (bad.is_empty(), format!("changed: {bad:?}"))
        }
        // Force the scheduled write, then the session's record is its text.
        "record-is-session" => {
            let Some(s) = session else {
                return (false, "no session".into());
            };
            svc.recovery_pass();
            let ok = svc.recovery_record_text(s).as_deref() == svc.text(s).ok().as_deref();
            (ok, String::new())
        }
        "close-asked" => (
            format!("{close_asked:?}") == name,
            format!("answers {close_asked:?}"),
        ),
        _ => (false, format!("unknown check {what}")),
    }
}

/// `smoke_finish` for a recovery run: write the report and exit.
pub fn finish(app: &AppHandle, report: &p::SmokeReport) -> bool {
    let Some(a) = ARM.lock().ok().and_then(|mut g| g.take()) else {
        return false;
    };
    let all = !report.steps.is_empty() && report.steps.iter().all(|s| s.ok);
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "pass": all,
        "phase": a.phase,
        "steps": report.steps,
    }))
    .unwrap();
    println!("{json}");
    if let Some(rp) = a.report_path {
        let _ = std::fs::write(rp, &json);
    }
    // Exit directly: a smoke run must not stop on the close confirmation.
    app.exit(if all { 0 } else { 1 });
    true
}
