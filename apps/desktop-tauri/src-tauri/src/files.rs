// SPDX-License-Identifier: GPL-3.0-or-later
//! Native file service. Direct Rust translation of `src/editor/file-io.js`
//! (plus `src/files/vrml-file.js` `isGzip`, `src/files/backups.js`
//! `backupPath` and `src/preview/wrl-source.js` `readWrlSource`).
//!
//! The only filesystem-touching module of the document lane. Every path it
//! receives comes from the Rust side (native dialog or launch argument), never
//! from the WebView.
//!
//! Save discipline, in the JS order:
//! - (2) refuse if the destination changed underneath us (EEXTERNAL)
//! - (2a) opt-in: an unchanged gzip artifact is preserved with NO write
//! - (1) encode (gzip when the target format is gzip)
//! - (1a) verify the in-memory candidate decodes back to exactly the text
//! - (1b) opt-in: refuse a candidate over `max_bytes` (ESIZE)
//! - (3) write a temp sibling, fsync, close
//! - (4) verify the temp re-reads and decodes back to exactly the text
//! - (5) back up the prior file to a timestamped, never-clobbering name
//! - (6) atomically rename the verified temp over the destination
//! - (7) report success only after the verified swap
//!
//! Deliberate differences from the JS module, both stricter:
//! * Text must be valid UTF-8. The JS decoder replaces invalid bytes with
//!   U+FFFD, which silently changes the file on the next save; this module
//!   refuses to open such a file instead.
//! * The external-change stamp keeps the exact bytes rather than a SHA-1, so
//!   the comparison is exact and needs no hash dependency. Step 1a always runs.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Plain,
    Gzip,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Plain => "plain",
            Format::Gzip => "gzip",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    Io(String),
    /// Gzip magic bytes present but the stream does not inflate.
    BadGzip(String),
    /// The (decompressed) bytes are not valid UTF-8 at `byte`.
    NotUtf8 {
        byte: usize,
    },
    /// The destination changed since `expected` was taken. Nothing written.
    External {
        reason: &'static str,
    },
    /// The encoded candidate or the written temp does not decode to the text.
    Verify(String),
    /// The encoded candidate exceeds the caller's ceiling. Nothing written.
    Size {
        candidate: u64,
        max: u64,
    },
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Io(m) => write!(f, "{m}"),
            FileError::BadGzip(m) => {
                write!(f, "The file has gzip magic bytes but failed to decompress: {m}")
            }
            FileError::NotUtf8 { byte } => write!(
                f,
                "The file is not valid UTF-8 (first invalid byte at {byte}); WRL Forge will not open it lossily."
            ),
            FileError::External { reason } => write!(
                f,
                "The file changed on disk since it was opened ({reason}); refusing to overwrite."
            ),
            FileError::Verify(m) => write!(f, "Save did not verify: {m}"),
            FileError::Size { candidate, max } => write!(
                f,
                "Encoded candidate is {candidate} B, over the {max} B limit by {} B; nothing was written.",
                candidate - max
            ),
        }
    }
}

fn io(e: std::io::Error, what: &str, p: &Path) -> FileError {
    FileError::Io(format!("{what} {}: {e}", p.display()))
}

pub fn is_gzip(b: &[u8]) -> bool {
    b.len() >= 2 && b[0] == 0x1f && b[1] == 0x8b
}

/// Bytes on disk -> exact text + format. Magic bytes, not the extension, decide.
pub fn decode(raw: &[u8]) -> Result<(String, Format), FileError> {
    let (bytes, format) = if is_gzip(raw) {
        let mut out = Vec::new();
        MultiGzDecoder::new(raw)
            .read_to_end(&mut out)
            .map_err(|e| FileError::BadGzip(e.to_string()))?;
        (out, Format::Gzip)
    } else {
        (raw.to_vec(), Format::Plain)
    };
    match String::from_utf8(bytes) {
        Ok(s) => Ok((s, format)),
        Err(e) => Err(FileError::NotUtf8 {
            byte: e.utf8_error().valid_up_to(),
        }),
    }
}

pub fn encode(text: &str, format: Format) -> Result<Vec<u8>, FileError> {
    match format {
        Format::Plain => Ok(text.as_bytes().to_vec()),
        Format::Gzip => {
            let mut enc = GzEncoder::new(Vec::new(), Compression::best());
            enc.write_all(text.as_bytes())
                .and_then(|_| enc.finish())
                .map_err(|e| FileError::Verify(format!("gzip encode failed: {e}")))
        }
    }
}

/// Decode per the EXPECTED format, using magic bytes as ground truth so a save
/// that failed to compress is caught.
fn decode_as(bytes: &[u8], format: Format) -> Result<String, FileError> {
    if format == Format::Gzip && !is_gzip(bytes) {
        return Err(FileError::Verify(
            "expected gzip bytes but magic bytes are absent".into(),
        ));
    }
    if format == Format::Plain && is_gzip(bytes) {
        return Err(FileError::Verify(
            "expected plain bytes but found gzip".into(),
        ));
    }
    decode(bytes)
        .map(|(t, _)| t)
        .map_err(|e| FileError::Verify(e.to_string()))
}

/// External-change baseline: size + mtime hint, exact bytes authoritative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub bytes: Vec<u8>,
}

pub fn stamp(path: &Path) -> Result<Stamp, FileError> {
    let bytes = fs::read(path).map_err(|e| io(e, "Cannot read", path))?;
    let meta = fs::metadata(path).map_err(|e| io(e, "Cannot stat", path))?;
    Ok(Stamp {
        size: bytes.len() as u64,
        mtime: meta.modified().ok(),
        bytes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    Deleted,
    Size,
    Content,
}

impl Change {
    pub fn reason(self) -> &'static str {
        match self {
            Change::Unchanged => "unchanged",
            Change::Deleted => "deleted",
            Change::Size => "size",
            Change::Content => "content",
        }
    }
}

/// Compare a baseline to the file NOW. Identical bytes rewritten by another
/// tool are NOT a change (mtime alone never decides); a same-size edit is.
pub fn detect_external_change(prev: &Stamp, path: &Path) -> Change {
    match fs::read(path) {
        Err(_) => Change::Deleted,
        Ok(now) if now == prev.bytes => Change::Unchanged,
        Ok(now) if now.len() as u64 != prev.size => Change::Size,
        Ok(_) => Change::Content,
    }
}

/// Cheap poll: when size AND mtime both match the baseline, report unchanged
/// without reading the file; otherwise fall back to the exact byte compare.
/// Only the 3-second background poll uses this. The save-time conflict guard
/// always uses the exact `detect_external_change`.
pub fn poll_external_change(prev: &Stamp, path: &Path) -> Change {
    if let (Ok(meta), Some(mtime)) = (fs::metadata(path), prev.mtime) {
        if meta.len() == prev.size && meta.modified().ok() == Some(mtime) {
            return Change::Unchanged;
        }
    }
    detect_external_change(prev, path)
}

pub struct Loaded {
    pub text: String,
    pub format: Format,
    pub stamp: Stamp,
}

pub fn load(path: &Path) -> Result<Loaded, FileError> {
    let stamp = stamp(path)?;
    let (text, format) = decode(&stamp.bytes)?;
    Ok(Loaded {
        text,
        format,
        stamp,
    })
}

/// Is `path` ALREADY the exact gzip artifact `text` needs? Exact decompressed
/// text identity or `false` -- never "probably".
pub fn would_preserve(path: &Path, text: &str, format: Format) -> bool {
    if format != Format::Gzip {
        return false;
    }
    let Ok(raw) = fs::read(path) else {
        return false;
    };
    if !is_gzip(&raw) {
        return false;
    }
    matches!(decode(&raw), Ok((t, Format::Gzip)) if t == text)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SaveOptions {
    pub allow_overwrite: bool,
    pub preserve_existing_gzip: bool,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub preserved: bool,
    pub bytes_written: u64,
    pub backup: Option<PathBuf>,
    pub stamp: Stamp,
}

pub fn safe_save(
    path: &Path,
    text: &str,
    format: Format,
    expected: Option<&Stamp>,
    opts: SaveOptions,
    now: SystemTime,
) -> Result<Saved, FileError> {
    // (2) conflict guard first -- before preservation.
    if !opts.allow_overwrite {
        if let Some(prev) = expected {
            let change = detect_external_change(prev, path);
            if change != Change::Unchanged {
                return Err(FileError::External {
                    reason: change.reason(),
                });
            }
        }
    }
    // (2a) true no-op.
    if opts.preserve_existing_gzip && would_preserve(path, text, format) {
        return Ok(Saved {
            preserved: true,
            bytes_written: 0,
            backup: None,
            stamp: stamp(path)?,
        });
    }
    // (1) encode, (1a) verify the encoder.
    let bytes = encode(text, format)?;
    if decode_as(&bytes, format)? != text {
        return Err(FileError::Verify(
            "encoded candidate does not decode back to the buffer".into(),
        ));
    }
    // (1b) ceiling (strictly greater-than).
    if let Some(max) = opts.max_bytes {
        if bytes.len() as u64 > max {
            return Err(FileError::Size {
                candidate: bytes.len() as u64,
                max,
            });
        }
    }
    let stamp_text = iso_stamp(now);
    let tmp = unique_sibling(path, &format!("wrlforge-tmp-{stamp_text}"));
    let result = (|| {
        // (3) temp sibling, created exclusively, flushed and closed.
        {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| io(e, "Cannot create temp file", &tmp))?;
            f.write_all(&bytes)
                .map_err(|e| io(e, "Cannot write", &tmp))?;
            f.sync_all().map_err(|e| io(e, "Cannot flush", &tmp))?;
        }
        // (4) verify what reached the disk.
        let back = fs::read(&tmp).map_err(|e| io(e, "Cannot re-read", &tmp))?;
        if back != bytes || decode_as(&back, format)? != text {
            return Err(FileError::Verify(
                "written file did not verify: decoded contents differ from the buffer".into(),
            ));
        }
        // (5) backup of the prior file.
        let backup = if path.exists() {
            let b = unique_sibling(path, &format!("bak-{stamp_text}"));
            copy_exclusive(path, &b)?;
            Some(b)
        } else {
            None
        };
        // (6) atomic replace, then make the rename durable.
        fs::rename(&tmp, path).map_err(|e| io(e, "Cannot replace", path))?;
        if let Some(dir) = path.parent() {
            if let Ok(d) = File::open(dir) {
                let _ = d.sync_all();
            }
        }
        // (7)
        Ok(Saved {
            preserved: false,
            bytes_written: bytes.len() as u64,
            backup,
            stamp: stamp(path)?,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn copy_exclusive(from: &Path, to: &Path) -> Result<(), FileError> {
    let data = fs::read(from).map_err(|e| io(e, "Cannot read", from))?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|e| io(e, "Cannot create backup", to))?;
    f.write_all(&data)
        .and_then(|_| f.sync_all())
        .map_err(|e| io(e, "Cannot write backup", to))?;
    if fs::read(to).map_err(|e| io(e, "Cannot re-read backup", to))? != data {
        return Err(FileError::Verify("backup did not verify".into()));
    }
    Ok(())
}

/// `<path>.<suffix>`, or `<path>.<suffix>-N` if that name is taken: a backup
/// or temp never clobbers an existing file (the JS rotation rule).
fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    let base = path.as_os_str().to_owned();
    let mut n = 0u32;
    loop {
        let mut s = base.clone();
        s.push(".");
        s.push(suffix);
        if n > 0 {
            s.push(format!("-{n}"));
        }
        let p = PathBuf::from(s);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

/// `new Date().toISOString().replace(/[:.]/g, '-')` -- e.g.
/// `2026-10-08T23-01-02-345Z`, in UTC, without a date library.
pub fn iso_stamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let ms = d.subsec_millis();
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}-{ms:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "wrlforge-tauri-files-{tag}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let p = self.0.join(name);
            fs::write(&p, bytes).unwrap();
            p
        }
        fn names(&self) -> Vec<String> {
            let mut v: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn t0() -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(1_791_500_000_123)
    }

    const EXOTIC: &[u8] = "\u{FEFF}#VRML V2.0 utf8\r\n# é 😀\rGroup {}\n".as_bytes();

    #[test]
    fn iso_stamp_matches_js_shape() {
        assert_eq!(iso_stamp(UNIX_EPOCH), "1970-01-01T00-00-00-000Z");
        assert_eq!(
            iso_stamp(UNIX_EPOCH + Duration::from_millis(951_782_400_007)),
            "2000-02-29T00-00-00-007Z"
        );
    }

    #[test]
    fn plain_load_is_exact_and_unchanged_save_is_byte_identical() {
        let d = TempDir::new("plain");
        let p = d.file("a.wrl", EXOTIC);
        let l = load(&p).unwrap();
        assert_eq!(l.format, Format::Plain);
        assert_eq!(l.text.as_bytes(), EXOTIC);
        let s = safe_save(
            &p,
            &l.text,
            l.format,
            Some(&l.stamp),
            SaveOptions::default(),
            t0(),
        )
        .unwrap();
        assert_eq!(fs::read(&p).unwrap(), EXOTIC);
        let b = s.backup.unwrap();
        assert_eq!(fs::read(&b).unwrap(), EXOTIC);
        assert!(b
            .to_string_lossy()
            .ends_with("a.wrl.bak-2026-10-08T22-53-20-123Z"));
        assert_eq!(d.names().len(), 2, "no temp left behind: {:?}", d.names());
    }

    #[test]
    fn gzip_round_trip_and_preservation_is_a_true_no_op() {
        let d = TempDir::new("gz");
        let gz = encode(std::str::from_utf8(EXOTIC).unwrap(), Format::Gzip).unwrap();
        let p = d.file("item.wrl", &gz);
        let l = load(&p).unwrap();
        assert_eq!(l.format, Format::Gzip);
        assert_eq!(l.text.as_bytes(), EXOTIC);
        let opts = SaveOptions {
            preserve_existing_gzip: true,
            ..Default::default()
        };
        let mtime = fs::metadata(&p).unwrap().modified().unwrap();
        let s = safe_save(&p, &l.text, l.format, Some(&l.stamp), opts, t0()).unwrap();
        assert!(s.preserved && s.backup.is_none());
        assert_eq!(fs::read(&p).unwrap(), gz);
        assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), mtime);
        assert_eq!(d.names(), ["item.wrl"]);
        // A changed text is written as gzip, never as plain.
        let edited = format!("{}# more\n", l.text);
        let s = safe_save(&p, &edited, l.format, Some(&l.stamp), opts, t0()).unwrap();
        assert!(!s.preserved);
        let raw = fs::read(&p).unwrap();
        assert!(is_gzip(&raw));
        assert_eq!(decode(&raw).unwrap().0, edited);
        assert_eq!(fs::read(s.backup.unwrap()).unwrap(), gz);
    }

    #[test]
    fn external_change_refuses_before_any_write() {
        let d = TempDir::new("ext");
        let p = d.file("a.wrl", b"#VRML V2.0 utf8\n");
        let l = load(&p).unwrap();
        fs::write(&p, b"#VRML V2.0 utf8\nX{}\n").unwrap();
        let e = safe_save(
            &p,
            "mine",
            l.format,
            Some(&l.stamp),
            SaveOptions::default(),
            t0(),
        )
        .unwrap_err();
        assert_eq!(e, FileError::External { reason: "size" });
        fs::write(&p, b"#VRML V2.0 utf9\n").unwrap();
        let e = safe_save(
            &p,
            "mine",
            l.format,
            Some(&l.stamp),
            SaveOptions::default(),
            t0(),
        )
        .unwrap_err();
        assert_eq!(e, FileError::External { reason: "content" });
        assert_eq!(fs::read(&p).unwrap(), b"#VRML V2.0 utf9\n");
        assert_eq!(d.names(), ["a.wrl"]);
        // Identical bytes rewritten by another tool are not a conflict.
        fs::write(&p, b"#VRML V2.0 utf8\n").unwrap();
        assert_eq!(detect_external_change(&l.stamp, &p), Change::Unchanged);
        assert_eq!(poll_external_change(&l.stamp, &p), Change::Unchanged);
        fs::remove_file(&p).unwrap();
        assert_eq!(detect_external_change(&l.stamp, &p), Change::Deleted);
        assert_eq!(poll_external_change(&l.stamp, &p), Change::Deleted);
    }

    #[test]
    fn size_ceiling_refuses_with_nothing_written() {
        let d = TempDir::new("size");
        let p = d.file("a.wrl", b"small");
        let l = load(&p).unwrap();
        let opts = SaveOptions {
            max_bytes: Some(4),
            ..Default::default()
        };
        let e = safe_save(&p, "too long", l.format, Some(&l.stamp), opts, t0()).unwrap_err();
        assert_eq!(
            e,
            FileError::Size {
                candidate: 8,
                max: 4
            }
        );
        assert_eq!(fs::read(&p).unwrap(), b"small");
        assert_eq!(d.names(), ["a.wrl"]);
    }

    #[test]
    fn failed_save_leaves_original_and_no_temp() {
        let d = TempDir::new("fail");
        let p = d.file("a.wrl", b"orig");
        let l = load(&p).unwrap();
        // Make the rename target impossible: replace the file by a directory
        // AFTER the conflict guard would pass (allow_overwrite skips it).
        let dir_target = d.0.join("sub.wrl");
        fs::create_dir(&dir_target).unwrap();
        fs::write(dir_target.join("keep"), b"x").unwrap();
        let opts = SaveOptions {
            allow_overwrite: true,
            ..Default::default()
        };
        assert!(safe_save(&dir_target, "new", Format::Plain, None, opts, t0()).is_err());
        assert_eq!(fs::read(dir_target.join("keep")).unwrap(), b"x");
        assert_eq!(fs::read(&p).unwrap(), b"orig");
        let names = d.names();
        assert!(
            names.iter().all(|n| !n.contains("wrlforge-tmp")),
            "temp left: {names:?}"
        );
        let _ = l;
    }

    #[test]
    fn backups_never_clobber() {
        let d = TempDir::new("bak");
        let p = d.file("a.wrl", b"v1");
        let l = load(&p).unwrap();
        let s1 = safe_save(
            &p,
            "v2",
            l.format,
            Some(&l.stamp),
            SaveOptions::default(),
            t0(),
        )
        .unwrap();
        let s2 = safe_save(
            &p,
            "v3",
            l.format,
            Some(&s1.stamp),
            SaveOptions::default(),
            t0(),
        )
        .unwrap();
        assert_ne!(s1.backup, s2.backup);
        assert_eq!(fs::read(s1.backup.unwrap()).unwrap(), b"v1");
        assert_eq!(fs::read(s2.backup.unwrap()).unwrap(), b"v2");
        assert_eq!(fs::read(&p).unwrap(), b"v3");
    }

    #[test]
    fn invalid_utf8_and_bad_gzip_refuse_to_open() {
        let d = TempDir::new("bad");
        let p = d.file("latin1.wrl", b"#VRML V2.0 utf8\n# caf\xe9\n");
        assert_eq!(load(&p).err(), Some(FileError::NotUtf8 { byte: 21 }));
        let g = d.file("bad.wrl", &[0x1f, 0x8b, 0, 1, 2]);
        assert!(matches!(load(&g).err(), Some(FileError::BadGzip(_))));
    }

    #[test]
    fn repository_fixtures_round_trip_exactly() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../test/fixtures");
        for name in ["valid-plain.wrl", "valid-gzip.wrl", "bad-header.wrl"] {
            let p = root.join(name);
            let raw = fs::read(&p).unwrap();
            let l = load(&p).unwrap();
            let again = encode(&l.text, l.format).unwrap();
            assert_eq!(decode(&again).unwrap().0, l.text, "{name}");
            if l.format == Format::Plain {
                assert_eq!(again, raw, "{name}");
            }
        }
    }
}
