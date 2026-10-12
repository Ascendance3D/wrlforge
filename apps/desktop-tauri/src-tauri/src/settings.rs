// SPDX-License-Identifier: GPL-3.0-or-later
//! Persistent application preferences: the theme (UI-THEME-1) and the
//! workspace layout (UI-DOCK-1).
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
//!
//! UI-DOCK-1 adds `"layout": {"version":1,"preset":…,"root":…}`, validated as
//! a whole by `protocol::layout`. A missing, damaged, older, newer or invalid
//! layout yields the default (Source) preset plus a notice and never blocks
//! startup. The theme and the layout share ONE writer: both hold the same
//! lock across the read-modify-write, and each replaces only its own keys.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use wrlforge_desktop_protocol::layout::{
    Layout, LayoutSetOutcome, LayoutState, LayoutWire, PresetId, LAYOUT_VERSION,
};
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

/// The persisted key of the workspace layout.
pub const LAYOUT_KEY: &str = "layout";

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutLoaded {
    pub layout: Layout,
    pub notice: Option<String>,
}

fn layout_fallback(why: impl Into<String>) -> LayoutLoaded {
    LayoutLoaded {
        layout: Layout::default_layout(),
        notice: Some(format!(
            "{}; using the {} layout.",
            why.into(),
            PresetId::DEFAULT.label()
        )),
    }
}

/// Interpret the layout in settings bytes. Pure: no I/O. No layout key (or
/// no file) is the silent default; anything unusable is the default + notice.
pub fn parse_layout(bytes: &[u8]) -> LayoutLoaded {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return layout_fallback("The saved layout could not be read (settings are not JSON)");
    };
    let Some(raw) = v.as_object().and_then(|o| o.get(LAYOUT_KEY)) else {
        return LayoutLoaded {
            layout: Layout::default_layout(),
            notice: None,
        };
    };
    match raw.get("version").and_then(Value::as_u64) {
        Some(v) if v > LAYOUT_VERSION as u64 => {
            return layout_fallback(format!(
                "The saved layout is from a newer WRL Forge (layout version {v})"
            ))
        }
        Some(v) if v < LAYOUT_VERSION as u64 => {
            return layout_fallback(format!("The saved layout version {v} is too old"))
        }
        Some(_) => {}
        None => return layout_fallback("The saved layout has no valid version"),
    }
    let wire = match serde_json::from_value::<LayoutWire>(raw.clone()) {
        Ok(w) => w,
        Err(_) => return layout_fallback("The saved layout is damaged"),
    };
    match Layout::from_wire(&wire) {
        Ok((layout, unknown)) if unknown.is_empty() => LayoutLoaded {
            layout,
            notice: None,
        },
        Ok((layout, unknown)) => LayoutLoaded {
            layout,
            notice: Some(format!(
                "The saved layout named unknown panels ({}); they were left out.",
                unknown.join(", ")
            )),
        },
        Err(e) => layout_fallback(format!("The saved layout is invalid ({e})")),
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
    /// UI-DOCK-1: the adopted workspace layout. Written under `current`'s
    /// lock, so the theme and the layout never race for the file.
    layout: Mutex<LayoutLoaded>,
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
        let bytes = fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() <= MAX_BYTES)
            .and_then(|_| fs::read(&path).ok());
        let native_viewport = bytes.as_deref().is_some_and(native_viewport_requested);
        let layout = match (fs::metadata(&path), bytes.as_deref()) {
            (Err(e), _) if e.kind() == std::io::ErrorKind::NotFound => LayoutLoaded {
                layout: Layout::default_layout(),
                notice: None,
            },
            (_, Some(b)) => parse_layout(b),
            (_, None) => layout_fallback("The saved layout could not be read"),
        };
        SettingsStore {
            dir: Some(dir),
            current: Mutex::new(loaded),
            native_viewport,
            layout: Mutex::new(layout),
        }
    }

    pub fn unavailable(reason: &str) -> Self {
        SettingsStore {
            dir: None,
            current: Mutex::new(fallback(format!("No settings directory ({reason})"))),
            native_viewport: false,
            layout: Mutex::new(LayoutLoaded {
                layout: Layout::default_layout(),
                notice: None,
            }),
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
            Some(dir) => write_with(dir, |obj| {
                obj.insert(
                    "schemaVersion".into(),
                    Value::from(theme::SETTINGS_SCHEMA_VERSION),
                );
                obj.insert("themeId".into(), Value::from(id));
            }),
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

impl SettingsStore {
    /// UI-DOCK-1: the layout to apply at startup (or after a reload).
    pub fn layout_state(&self) -> LayoutState {
        let l = self.layout.lock().unwrap();
        LayoutState {
            layout: l.layout.to_wire(),
            notice: l.notice.clone(),
        }
    }

    /// Validate a layout as a whole, persist it, and adopt it. An invalid
    /// layout changes nothing; a failed write keeps the old file intact.
    pub fn set_layout(&self, wire: &LayoutWire) -> LayoutSetOutcome {
        let layout = match Layout::from_wire(wire) {
            Ok((l, unknown)) if unknown.is_empty() => l,
            Ok((_, unknown)) => {
                return LayoutSetOutcome::Rejected {
                    message: format!("Unknown panels: {}", unknown.join(", ")),
                }
            }
            Err(e) => {
                return LayoutSetOutcome::Rejected {
                    message: format!("Invalid layout: {e}"),
                }
            }
        };
        // The theme's lock: one settings writer at a time.
        let cur = self.current.lock().unwrap();
        let canonical = serde_json::to_value(layout.to_wire()).expect("layout json");
        let result = match &self.dir {
            Some(dir) => write_with(dir, |obj| {
                // A first write creates a file the theme reader accepts.
                obj.entry("schemaVersion")
                    .or_insert_with(|| Value::from(theme::SETTINGS_SCHEMA_VERSION));
                obj.entry("themeId")
                    .or_insert_with(|| Value::from(cur.theme_id.clone()));
                obj.insert(LAYOUT_KEY.into(), canonical);
            }),
            None => Err(std::io::Error::other("no settings directory")),
        };
        drop(cur);
        *self.layout.lock().unwrap() = LayoutLoaded {
            layout,
            notice: None,
        };
        match result {
            Ok(()) => LayoutSetOutcome::Saved,
            Err(e) => LayoutSetOutcome::NotSaved {
                message: format!("Layout applied for this session but NOT saved: {e}"),
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

/// The new file content: the existing object (if valid JSON) with only the
/// caller's keys replaced, so every other preference key survives.
fn render(
    existing: Option<&[u8]>,
    edit: impl FnOnce(&mut serde_json::Map<String, Value>),
) -> Vec<u8> {
    let mut obj = existing
        .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    edit(&mut obj);
    let mut out = serde_json::to_vec_pretty(&Value::Object(obj)).expect("json");
    out.push(b'\n');
    out
}

/// The one settings writer: read-modify-write with an atomic rename.
fn write_with(
    dir: &Path,
    edit: impl FnOnce(&mut serde_json::Map<String, Value>),
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    let existing = fs::metadata(&path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_BYTES)
        .and_then(|_| fs::read(&path).ok());
    let bytes = render(existing.as_deref(), edit);
    if bytes.len() as u64 > MAX_BYTES {
        return Err(std::io::Error::other(
            "settings would exceed the size limit",
        ));
    }
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

    // ---------------------------------------------------------- UI-DOCK-1

    use wrlforge_desktop_protocol::layout::{DropTarget, PanelId, Zone};

    fn moved() -> Layout {
        Layout::preset(PresetId::Source)
            .dock(
                PanelId::Inspector,
                DropTarget {
                    anchor: PanelId::SceneTree,
                    zone: Zone::Bottom,
                    index: None,
                },
            )
            .unwrap()
    }

    fn file(d: &Path) -> Value {
        serde_json::from_slice(&fs::read(d.join(FILE_NAME)).unwrap()).unwrap()
    }

    #[test]
    fn no_file_or_no_key_is_the_silent_default_layout() {
        let d = tmpdir("layout-none");
        let s = SettingsStore::open(d.join("absent"));
        let st = s.layout_state();
        assert_eq!(st.layout, Layout::default_layout().to_wire());
        assert_eq!(st.notice, None);
        fs::write(
            d.join(FILE_NAME),
            r#"{"schemaVersion":1,"themeId":"tokyo-night"}"#,
        )
        .unwrap();
        let st = SettingsStore::open(d.clone()).layout_state();
        assert_eq!(st.layout, Layout::default_layout().to_wire());
        assert_eq!(st.notice, None);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn theme_then_layout_and_layout_then_theme_keep_both() {
        let d = tmpdir("layout-order");
        fs::write(
            d.join(FILE_NAME),
            r#"{"schemaVersion":1,"themeId":"tokyo-night","viewport":{"renderer":"native-experimental"},"future":[1,2]}"#,
        )
        .unwrap();
        let s = SettingsStore::open(d.clone());
        // Theme, then layout.
        assert!(matches!(
            s.set_theme("tokyo-night-storm"),
            ThemeSetOutcome::Saved { .. }
        ));
        assert_eq!(s.set_layout(&moved().to_wire()), LayoutSetOutcome::Saved);
        let v = file(&d);
        assert_eq!(v["themeId"], "tokyo-night-storm");
        assert_eq!(v["layout"]["version"], 1);
        assert_eq!(v["viewport"]["renderer"], "native-experimental");
        assert_eq!(v["future"][1], 2);
        // Layout, then theme.
        let modeling = Layout::preset(PresetId::Modeling);
        assert_eq!(s.set_layout(&modeling.to_wire()), LayoutSetOutcome::Saved);
        assert!(matches!(
            s.set_theme("tokyo-night-light"),
            ThemeSetOutcome::Saved { .. }
        ));
        let v = file(&d);
        assert_eq!(v["themeId"], "tokyo-night-light");
        assert_eq!(v["layout"]["preset"], "preset.modeling");
        assert_eq!(v["viewport"]["renderer"], "native-experimental");
        assert_eq!(v["future"][0], 1);
        // Restart: both come back, no notice.
        let again = SettingsStore::open(d.clone());
        assert_eq!(again.state().theme_id, "tokyo-night-light");
        assert_eq!(again.state().notice, None);
        assert_eq!(again.layout_state().layout, modeling.to_wire());
        assert_eq!(again.layout_state().notice, None);
        assert!(again.native_viewport());
        // No temp file left behind.
        let names: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(FILE_NAME)]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_first_layout_write_creates_a_file_the_theme_reader_accepts() {
        let d = tmpdir("layout-first");
        let s = SettingsStore::open(d.clone());
        assert_eq!(s.set_layout(&moved().to_wire()), LayoutSetOutcome::Saved);
        let again = SettingsStore::open(d.clone());
        assert_eq!(again.state().theme_id, "tokyo-night");
        assert_eq!(again.state().notice, None);
        assert_eq!(again.layout_state().layout, moved().to_wire());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn damaged_old_newer_and_invalid_layouts_fall_back_with_a_notice() {
        let ok = serde_json::to_string(&moved().to_wire()).unwrap();
        let cases = [
            (r#"{"schemaVersion":1,"themeId":"tokyo-night","layout":7}"#.to_string(), "version"),
            (r#"{"schemaVersion":1,"themeId":"tokyo-night","layout":{}}"#.into(), "version"),
            (r#"{"schemaVersion":1,"layout":{"version":0,"preset":"preset.source"}}"#.into(), "too old"),
            (r#"{"schemaVersion":1,"layout":{"version":2,"preset":"preset.source","root":{"type":"hologram"}}}"#.into(), "newer"),
            (r#"{"schemaVersion":1,"layout":{"version":1,"preset":"preset.source","root":{"type":"hologram"}}}"#.into(), "damaged"),
            (r#"{"schemaVersion":1,"layout":{"version":1,"preset":"preset.source","root":{"type":"tabs","panels":["panel.source"],"active":"panel.source"}}}"#.into(), "invalid"),
            (r#"{"schemaVersion":1,"layout":{"version":1,"preset":"preset.source","root":{"type":"split","axis":"row","sizes":[1.5,-0.5],"children":[{"type":"tabs","panels":["panel.source"],"active":"panel.source"},{"type":"tabs","panels":["panel.viewport"],"active":"panel.viewport"}]}}}"#.into(), "invalid"),
            (r#"{"schemaVersion":1,"layout":{"version":1,"preset":"preset.source","root":{"type":"split","axis":"row","sizes":[0.5,0.5],"children":[{"type":"tabs","panels":["panel.source","panel.viewport"],"active":"panel.source"},{"type":"tabs","panels":["panel.viewport"],"active":"panel.viewport"}]}}}"#.into(), "invalid"),
            ("{not json".into(), "not JSON"),
            (format!(r#"{{"schemaVersion":1,"layout":{}}}"#, ok.replace("preset.source", "preset.gaming")), "invalid"),
        ];
        for (text, word) in cases {
            let l = parse_layout(text.as_bytes());
            assert_eq!(l.layout, Layout::default_layout(), "{text}");
            let n = l.notice.unwrap_or_default();
            assert!(n.contains(word), "{text}: {n}");
            assert!(n.contains("Source layout"), "{n}");
        }
        // Unknown panels are left out; the rest of the layout is kept.
        let unknown = ok.replace("panel.diagnostics", "panel.future");
        let l = parse_layout(format!("{{\"layout\":{unknown}}}").as_bytes());
        assert!(!l.layout.is_visible(PanelId::Diagnostics));
        assert!(l.layout.is_visible(PanelId::Inspector));
        assert!(l.notice.unwrap().contains("panel.future"));
        // A bad layout never blocks startup, and the theme still loads.
        let d = tmpdir("layout-bad");
        fs::write(
            d.join(FILE_NAME),
            r#"{"schemaVersion":1,"themeId":"tokyo-night-storm","layout":{"version":99}}"#,
        )
        .unwrap();
        let s = SettingsStore::open(d.clone());
        assert_eq!(s.state().theme_id, "tokyo-night-storm");
        assert!(s.layout_state().notice.unwrap().contains("newer"));
        assert_eq!(s.layout_state().layout, Layout::default_layout().to_wire());
        // The next layout change replaces it cleanly and clears the notice.
        assert_eq!(s.set_layout(&moved().to_wire()), LayoutSetOutcome::Saved);
        assert_eq!(s.layout_state().notice, None);
        assert_eq!(
            SettingsStore::open(d.clone()).state().theme_id,
            "tokyo-night-storm"
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn invalid_layouts_are_rejected_without_writing() {
        let d = tmpdir("layout-reject");
        let s = SettingsStore::open(d.clone());
        let mut w = moved().to_wire();
        w.version = 9;
        assert!(matches!(
            s.set_layout(&w),
            LayoutSetOutcome::Rejected { .. }
        ));
        let mut w = moved().to_wire();
        w.preset = "preset.x".into();
        assert!(matches!(
            s.set_layout(&w),
            LayoutSetOutcome::Rejected { .. }
        ));
        let w = LayoutWire {
            version: 1,
            preset: "preset.source".into(),
            root: wrlforge_desktop_protocol::layout::NodeWire::Tabs {
                panels: vec![
                    "panel.source".into(),
                    "panel.viewport".into(),
                    "panel.ghost".into(),
                ],
                active: "panel.source".into(),
            },
        };
        assert!(matches!(
            s.set_layout(&w),
            LayoutSetOutcome::Rejected { .. }
        ));
        assert!(!d.join(FILE_NAME).exists());
        assert_eq!(s.layout_state().layout, Layout::default_layout().to_wire());
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_layout_write_keeps_the_previous_file_byte_identical() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmpdir("layout-ro");
        let original = br#"{"schemaVersion":1,"themeId":"tokyo-night-storm"}"#;
        fs::write(d.join(FILE_NAME), original).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let s = SettingsStore::open(d.clone());
        let out = s.set_layout(&moved().to_wire());
        let writable_anyway = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(d.join("probe"))
            .is_ok();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        if !writable_anyway {
            assert!(matches!(out, LayoutSetOutcome::NotSaved { .. }), "{out:?}");
            assert_eq!(fs::read(d.join(FILE_NAME)).unwrap(), original);
            // Applied for this session.
            assert_eq!(s.layout_state().layout, moved().to_wire());
        }
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn concurrent_theme_and_layout_writers_never_lose_a_key() {
        let d = tmpdir("layout-race");
        let s = std::sync::Arc::new(SettingsStore::open(d.clone()));
        let themes = ["tokyo-night-storm", "tokyo-night-light", "tokyo-night"];
        let layouts = [
            moved(),
            Layout::preset(PresetId::Modeling),
            Layout::default_layout(),
        ];
        let t = {
            let s = s.clone();
            std::thread::spawn(move || {
                for i in 0..30 {
                    s.set_theme(themes[i % 3]);
                }
            })
        };
        for i in 0..30 {
            s.set_layout(&layouts[i % 3].to_wire());
        }
        t.join().unwrap();
        let v = file(&d);
        assert_eq!(v["themeId"], themes[29 % 3]);
        assert_eq!(v["layout"]["version"], 1);
        assert_eq!(
            serde_json::from_value::<LayoutWire>(v["layout"].clone()).unwrap(),
            layouts[29 % 3].to_wire()
        );
        fs::remove_dir_all(&d).unwrap();
    }
}
