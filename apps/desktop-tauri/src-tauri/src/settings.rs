// SPDX-License-Identifier: GPL-3.0-or-later
//! Persistent application preferences (UI-THEME-1). Today: the theme only.
//!
//! `settings.json` lives in Tauri's per-app CONFIG directory (or the
//! `--config-dir` a test passes). The WebView never sees or chooses a path; it
//! sends a theme id, which is checked against the built-in registry before
//! anything is written.
//!
//! Shape: `{"schemaVersion":1,"themeId":"tokyo-night"}`. Unknown keys are
//! preserved on write. Reads never fail the app: a missing, unreadable,
//! oversized, corrupt, wrong-version or unknown-theme file yields Tokyo Night
//! plus a notice. Writes go to a temp sibling, are fsynced, then renamed over
//! the old file, so a failed write leaves the previous settings intact.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use wrlforge_desktop_protocol::theme::{self, ThemeSetOutcome, ThemeState};

pub const FILE_NAME: &str = "settings.json";
/// A settings file larger than this is treated as corrupt, never parsed.
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub theme_id: String,
    pub notice: Option<String>,
}

fn fallback(notice: impl Into<String>) -> Loaded {
    let label = theme::find(theme::DEFAULT_THEME).map_or("Tokyo Night", |t| t.label);
    Loaded {
        theme_id: theme::DEFAULT_THEME.into(),
        notice: Some(format!("{}; using {label}.", notice.into())),
    }
}

/// Interpret settings bytes. Pure: no I/O.
pub fn parse(bytes: &[u8]) -> Loaded {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return fallback("Theme settings are corrupt (not JSON)");
    };
    let Some(obj) = v.as_object() else {
        return fallback("Theme settings are corrupt (not an object)");
    };
    match obj.get("schemaVersion").and_then(Value::as_u64) {
        Some(v) if v == theme::SETTINGS_SCHEMA_VERSION as u64 => {}
        Some(v) => return fallback(format!("Theme settings version {v} is not supported")),
        None => return fallback("Theme settings have no valid schemaVersion"),
    }
    match obj.get("themeId").and_then(Value::as_str) {
        Some(id) if theme::is_known(id) => Loaded {
            theme_id: id.to_string(),
            notice: None,
        },
        Some(id) => {
            let shown: String = id.chars().take(40).collect();
            fallback(format!("Unknown saved theme {shown:?}"))
        }
        None => fallback("Theme settings have no valid themeId"),
    }
}

pub struct SettingsStore {
    /// `None` when the platform reported no config directory: the app still
    /// runs on Tokyo Night, and every save reports NotSaved.
    dir: Option<PathBuf>,
    current: Mutex<Loaded>,
    /// NATIVE-RENDER-1: the HIDDEN `"viewport": {"renderer": "native-experimental"}` key,
    /// read once at startup. No UI writes it; anything else means X_ITE.
    native_viewport: bool,
}

/// The approved (design) value of the hidden `viewport.renderer` key.
pub const NATIVE_EXPERIMENTAL: &str = "native-experimental";

/// `viewport.renderer == "native-experimental"` in settings bytes. Pure.
/// Any other value, shape or a corrupt file
/// means the default (X_ITE); that includes the unapproved `"native"`, which
/// only an uncommitted pre-closeout build ever read.
pub fn native_viewport_requested(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|v| {
            v.get("viewport")?
                .get("renderer")?
                .as_str()
                .map(|r| r == NATIVE_EXPERIMENTAL)
        })
        .unwrap_or(false)
}

impl SettingsStore {
    /// Load once at startup. Never fails.
    pub fn open(dir: PathBuf) -> Self {
        let path = dir.join(FILE_NAME);
        let loaded = load(&path);
        let native_viewport = fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() <= MAX_BYTES)
            .and_then(|_| fs::read(&path).ok())
            .is_some_and(|b| native_viewport_requested(&b));
        SettingsStore {
            dir: Some(dir),
            current: Mutex::new(loaded),
            native_viewport,
        }
    }

    pub fn unavailable(reason: &str) -> Self {
        SettingsStore {
            dir: None,
            current: Mutex::new(fallback(format!("No settings directory ({reason})"))),
            native_viewport: false,
        }
    }

    pub fn native_viewport(&self) -> bool {
        self.native_viewport
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(FILE_NAME))
    }

    pub fn state(&self) -> ThemeState {
        let cur = self.current.lock().unwrap();
        ThemeState::new(&cur.theme_id, cur.notice.clone())
    }

    /// Validate, persist, and adopt `id`. An unknown id changes nothing.
    pub fn set_theme(&self, id: &str) -> ThemeSetOutcome {
        if !theme::is_known(id) {
            let shown: String = id.chars().take(40).collect();
            return ThemeSetOutcome::Rejected {
                message: format!("Unknown theme {shown:?}."),
            };
        }
        // Held across the write: one writer at a time, so the temp name and
        // the adopted id cannot interleave.
        let mut cur = self.current.lock().unwrap();
        let result = match &self.dir {
            Some(dir) => write_theme(dir, id),
            None => Err(std::io::Error::other("no settings directory")),
        };
        cur.theme_id = id.to_string();
        cur.notice = None;
        match result {
            Ok(()) => ThemeSetOutcome::Saved {
                theme_id: id.into(),
            },
            Err(e) => ThemeSetOutcome::NotSaved {
                theme_id: id.into(),
                message: format!("Theme applied for this session but NOT saved: {e}"),
            },
        }
    }
}

fn load(path: &Path) -> Loaded {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Loaded {
                theme_id: theme::DEFAULT_THEME.into(),
                notice: None,
            }
        }
        Err(e) => return fallback(format!("Theme settings could not be read ({e})")),
        Ok(m) if !m.is_file() => return fallback("Theme settings path is not a file"),
        Ok(m) if m.len() > MAX_BYTES => return fallback("Theme settings file is too large"),
        Ok(_) => {}
    }
    match fs::read(path) {
        Ok(b) => parse(&b),
        Err(e) => fallback(format!("Theme settings could not be read ({e})")),
    }
}

/// The new file content: the existing object (if valid JSON) with the theme
/// keys replaced, so later preference keys survive a theme change.
fn render(existing: Option<&[u8]>, id: &str) -> Vec<u8> {
    let mut obj = existing
        .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    obj.insert(
        "schemaVersion".into(),
        Value::from(theme::SETTINGS_SCHEMA_VERSION),
    );
    obj.insert("themeId".into(), Value::from(id));
    let mut out = serde_json::to_vec_pretty(&Value::Object(obj)).expect("json");
    out.push(b'\n');
    out
}

fn write_theme(dir: &Path, id: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    let existing = fs::metadata(&path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_BYTES)
        .and_then(|_| fs::read(&path).ok());
    let bytes = render(existing.as_deref(), id);
    let tmp = dir.join(format!("{FILE_NAME}.wrlforge-tmp-{}", std::process::id()));
    let res = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        if fs::read(&tmp)? != bytes {
            return Err(std::io::Error::other("verification read-back differs"));
        }
        fs::rename(&tmp, &path)?;
        #[cfg(unix)]
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_viewport_is_hidden_and_off_by_default() {
        assert!(native_viewport_requested(
            br#"{"viewport":{"renderer":"native-experimental"}}"#
        ));
        for b in [
            &b""[..],
            b"not json",
            br#"{"schemaVersion":1,"themeId":"tokyo-night"}"#,
            br#"{"viewport":{"renderer":"xite"}}"#,
            br#"{"viewport":{"renderer":"NATIVE"}}"#,
            br#"{"viewport":{"renderer":"native"}}"#,
            br#"{"viewport":{"renderer":"Native-Experimental"}}"#,
            br#"{"viewport":{"renderer":"native-experimental "}}"#,
            br#"{"viewport":{"renderer":""}}"#,
            br#"{"viewport":{"renderer":null}}"#,
            br#"{"viewport":{}}"#,
            br#"{"renderer":"native-experimental"}"#,
            br#"{"viewport":"native"}"#,
            br#"{"viewport":{"renderer":1}}"#,
        ] {
            assert!(!native_viewport_requested(b), "{}", String::from_utf8_lossy(b));
        }
    }

    #[test]
    fn the_store_selects_xite_unless_the_approved_value_is_saved() {
        let d = tmpdir("renderer");
        assert!(
            !SettingsStore::open(d.clone()).native_viewport(),
            "absent file"
        );
        for (text, native) in [
            (r#"{"viewport":{"renderer":"native"}}"#, false),
            (r#"{"viewport":{"renderer":"bogus"}}"#, false),
            (r#"{"viewport":{"renderer":"native-experimental"}}"#, true),
        ] {
            fs::write(d.join(FILE_NAME), text).unwrap();
            assert_eq!(
                SettingsStore::open(d.clone()).native_viewport(),
                native,
                "{text}"
            );
        }
        let _ = fs::remove_dir_all(&d);
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wrlforge-settings-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parse_accepts_only_versioned_known_themes() {
        for id in ["tokyo-night", "tokyo-night-storm", "tokyo-night-light"] {
            let b = format!(r#"{{"schemaVersion":1,"themeId":"{id}"}}"#);
            assert_eq!(
                parse(b.as_bytes()),
                Loaded {
                    theme_id: id.into(),
                    notice: None
                }
            );
        }
        for bad in [
            &b""[..],
            b"{",
            b"\xff\xfe",
            b"[]",
            b"null",
            br#"{"themeId":"tokyo-night-storm"}"#,
            br#"{"schemaVersion":2,"themeId":"tokyo-night-storm"}"#,
            br#"{"schemaVersion":"1","themeId":"tokyo-night-storm"}"#,
            br#"{"schemaVersion":1,"themeId":"solarized"}"#,
            br#"{"schemaVersion":1,"themeId":"Tokyo Night Storm"}"#,
            br#"{"schemaVersion":1,"themeId":7}"#,
            br#"{"schemaVersion":1}"#,
        ] {
            let l = parse(bad);
            assert_eq!(
                l.theme_id,
                "tokyo-night",
                "{:?}",
                String::from_utf8_lossy(bad)
            );
            assert!(l.notice.is_some());
        }
    }

    #[test]
    fn missing_file_is_a_silent_default() {
        let d = tmpdir("missing");
        let s = SettingsStore::open(d.join("not-created-yet"));
        let st = s.state();
        assert_eq!(st.theme_id, "tokyo-night");
        assert_eq!(st.notice, None);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn set_persists_and_survives_a_reopen() {
        let d = tmpdir("persist");
        let cfg = d.join("cfg");
        let s = SettingsStore::open(cfg.clone());
        assert_eq!(
            s.set_theme("tokyo-night-light"),
            ThemeSetOutcome::Saved {
                theme_id: "tokyo-night-light".into()
            }
        );
        let text = fs::read_to_string(cfg.join(FILE_NAME)).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["themeId"], "tokyo-night-light");
        let again = SettingsStore::open(cfg.clone());
        assert_eq!(again.state().theme_id, "tokyo-night-light");
        // No temp file left behind.
        let names: Vec<_> = fs::read_dir(&cfg)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(FILE_NAME)]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn unknown_ids_are_rejected_without_writing() {
        let d = tmpdir("reject");
        let s = SettingsStore::open(d.clone());
        for bad in ["", "solarized", "../x", "tokyo-night\0", "TOKYO-NIGHT"] {
            assert!(matches!(s.set_theme(bad), ThemeSetOutcome::Rejected { .. }));
        }
        assert!(!d.join(FILE_NAME).exists());
        assert_eq!(s.state().theme_id, "tokyo-night");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn other_keys_survive_and_corrupt_files_are_replaced_cleanly() {
        let d = tmpdir("keys");
        fs::write(
            d.join(FILE_NAME),
            r#"{"schemaVersion":1,"themeId":"tokyo-night","future":{"a":1}}"#,
        )
        .unwrap();
        let s = SettingsStore::open(d.clone());
        s.set_theme("tokyo-night-storm");
        let v: Value = serde_json::from_slice(&fs::read(d.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(v["future"]["a"], 1);
        assert_eq!(v["themeId"], "tokyo-night-storm");

        fs::write(d.join(FILE_NAME), b"{not json").unwrap();
        let s = SettingsStore::open(d.clone());
        assert_eq!(s.state().theme_id, "tokyo-night");
        assert!(s.state().notice.unwrap().contains("corrupt"));
        assert!(matches!(
            s.set_theme("tokyo-night-light"),
            ThemeSetOutcome::Saved { .. }
        ));
        assert_eq!(
            SettingsStore::open(d.clone()).state().theme_id,
            "tokyo-night-light"
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn oversized_and_non_file_settings_fall_back() {
        let d = tmpdir("odd");
        fs::write(d.join(FILE_NAME), vec![b' '; (MAX_BYTES + 1) as usize]).unwrap();
        assert!(SettingsStore::open(d.clone()).state().notice.is_some());
        fs::remove_file(d.join(FILE_NAME)).unwrap();
        fs::create_dir(d.join(FILE_NAME)).unwrap();
        let s = SettingsStore::open(d.clone());
        assert_eq!(s.state().theme_id, "tokyo-night");
        assert!(s.state().notice.is_some());
        // Writing over a directory fails, visibly and without panicking.
        assert!(matches!(
            s.set_theme("tokyo-night-storm"),
            ThemeSetOutcome::NotSaved { .. }
        ));
        assert!(d.join(FILE_NAME).is_dir());
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_keeps_the_previous_file_byte_identical() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmpdir("ro");
        let original = br#"{"schemaVersion":1,"themeId":"tokyo-night-storm"}"#;
        fs::write(d.join(FILE_NAME), original).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let s = SettingsStore::open(d.clone());
        assert_eq!(s.state().theme_id, "tokyo-night-storm");
        let out = s.set_theme("tokyo-night-light");
        let writable_anyway = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(d.join("probe"))
            .is_ok(); // running as root: permissions are not enforced
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        if !writable_anyway {
            match out {
                ThemeSetOutcome::NotSaved { theme_id, message } => {
                    assert_eq!(theme_id, "tokyo-night-light");
                    assert!(message.contains("NOT saved"));
                }
                other => panic!("expected NotSaved, got {other:?}"),
            }
            assert_eq!(fs::read(d.join(FILE_NAME)).unwrap(), original);
            // The session still uses the chosen theme.
            assert_eq!(s.state().theme_id, "tokyo-night-light");
        }
        fs::remove_dir_all(&d).unwrap();
    }
}
