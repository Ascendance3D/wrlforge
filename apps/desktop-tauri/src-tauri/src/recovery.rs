// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-STAB-1: persistent crash recovery for unsaved documents.
//!
//! Behavior reference: `src/editor/recovery-store.js` and
//! `recovery-controller.js` (Electron). This is a new Rust design, not a
//! port: one record per document session, binary-checked, written
//! atomically, and owned by exactly one running process.
//!
//! What a record holds: the EXACT canonical source text (UTF-8, so BOM, line
//! endings, whitespace, comments and malformed source survive byte for
//! byte), the original file format (plain / gzip), the document revision,
//! a change sequence, a safe display name, and the identity of the original
//! file (size, mtime, two content hashes) when there is one. The WebView and
//! X_ITE are never a source of record data.
//!
//! Safety rules:
//! * Records live only in the private recovery directory (mode 0700, files
//!   0600). A record file name is `rec-<32 hex>.wfr`; nothing in a record
//!   names a path the application writes to. A restored document has NO
//!   path: it needs Save As before it can replace any file.
//! * A write is: encode → temp file (create-new, 0600) → `fsync` → rename
//!   over the record → `fsync` the directory. A failed write removes only
//!   its own temp file; the previous record stays intact.
//! * A write commits only when its change sequence is newer than the last
//!   one applied for that record (checked under the coordinator lock, in
//!   the same critical section as the rename). Removal on Save / Discard
//!   advances the applied sequence, so a delayed write can never recreate
//!   an obsolete record.
//! * Each process holds an exclusive lock on `instances/<id>.lock`. At
//!   startup only records whose owner lock is free (owner exited or
//!   crashed) are offered; a live window's records are never touched.
//! * Reading rejects a bad magic, an unsupported version, oversize parts,
//!   truncation, trailing bytes, checksum failures, invalid headers and
//!   invalid UTF-8, each with its own message.
//!
//! What it cannot promise: changes made after the last COMPLETED record
//! write are lost if the process dies before the next one (the write is
//! debounced; see `Schedule`).

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The subdirectory of the application data (or test config) directory.
pub const DIR_NAME: &str = "recovery";
pub const MAGIC: &[u8; 8] = b"WFRECOV\n";
pub const VERSION: u16 = 1;
/// Fixed binary prefix: magic, version, header length + CRC, payload
/// length + CRC.
const PREFIX: usize = 8 + 2 + 4 + 4 + 8 + 4;
pub const MAX_HEADER: u64 = 16 * 1024;
/// The largest source text a record holds (and the largest original file
/// the startup identity check reads).
pub const MAX_TEXT: u64 = 64 * 1024 * 1024;
const RECORD_PREFIX: &str = "rec-";
const RECORD_SUFFIX: &str = ".wfr";
const TEMP_PREFIX: &str = ".tmp-";
const INSTANCES: &str = "instances";

/// Identity of the original file when the snapshot was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Original {
    /// Informational only: never a write target, never shown by default.
    pub path: String,
    pub size: u64,
    pub mtime_ms: Option<u64>,
    pub crc32: u32,
    pub fnv64: u64,
}

impl Original {
    pub fn of(path: &Path, bytes: &[u8], mtime: Option<SystemTime>) -> Self {
        Original {
            path: path.to_string_lossy().into_owned(),
            size: bytes.len() as u64,
            mtime_ms: mtime.and_then(ms_since_epoch),
            crc32: crc32(bytes),
            fnv64: fnv64(bytes),
        }
    }
    fn same_bytes(&self, bytes: &[u8]) -> bool {
        self.size == bytes.len() as u64 && self.crc32 == crc32(bytes) && self.fnv64 == fnv64(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// 32 lowercase hex digits; equals the file name's id.
    pub record: String,
    /// The owning process (its `instances/<id>.lock`).
    pub instance: String,
    /// Change sequence: monotonic per process, advances with every revision.
    pub sequence: u64,
    pub revision: u64,
    pub saved_at_ms: u64,
    /// `"plain"` or `"gzip"`.
    pub format: String,
    pub untitled: bool,
    /// A file name only (no directories).
    pub name: String,
    pub original: Option<Original>,
    pub text_bytes: u64,
    pub text_fnv64: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u16),
    HeaderTooLarge(u64),
    TextTooLarge(u64),
    Truncated,
    TrailingBytes,
    HeaderChecksum,
    PayloadChecksum,
    BadHeader(String),
    NameMismatch,
    NotUtf8,
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::TooShort => write!(f, "the record is too short to be a recovery record"),
            RecordError::BadMagic => write!(
                f,
                "the record does not start with the WRL Forge recovery signature"
            ),
            RecordError::UnsupportedVersion(v) => {
                write!(f, "recovery record version {v} is not supported (this build reads version {VERSION})")
            }
            RecordError::HeaderTooLarge(n) => write!(
                f,
                "the record header is {n} B, over the {MAX_HEADER} B limit"
            ),
            RecordError::TextTooLarge(n) => write!(
                f,
                "the recovered text is {n} B, over the {MAX_TEXT} B limit"
            ),
            RecordError::Truncated => write!(
                f,
                "the record is truncated (incomplete write or damaged file)"
            ),
            RecordError::TrailingBytes => {
                write!(f, "the record has unexpected bytes after its end")
            }
            RecordError::HeaderChecksum => {
                write!(f, "the record header failed its checksum (damaged file)")
            }
            RecordError::PayloadChecksum => {
                write!(f, "the recovered text failed its checksum (damaged file)")
            }
            RecordError::BadHeader(m) => write!(f, "the record header is invalid: {m}"),
            RecordError::NameMismatch => write!(f, "the record's id does not match its file name"),
            RecordError::NotUtf8 => write!(f, "the recovered text is not valid UTF-8"),
        }
    }
}

pub fn crc32(b: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(b);
    c.sum()
}

pub fn fnv64(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn ms_since_epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

pub fn now_ms() -> u64 {
    ms_since_epoch(SystemTime::now()).unwrap_or(0)
}

pub fn is_id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 128 random bits as 32 hex digits (std only: `RandomState` is seeded by
/// the OS). Unique names, not secrets.
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut out = String::with_capacity(32);
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(N.fetch_add(1, Ordering::Relaxed));
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// A safe display name: the last path component only, no control
/// characters, at most 120 characters.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().filter(|c| !c.is_control()).take(120).collect();
    if clean.is_empty() || clean == "." || clean == ".." {
        crate::service::UNTITLED_NAME.into()
    } else {
        clean
    }
}

fn validate_header(h: &Header) -> Result<(), RecordError> {
    let bad = |m: &str| Err(RecordError::BadHeader(m.into()));
    if !is_id(&h.record) {
        return bad("record id is not 32 hex digits");
    }
    if !is_id(&h.instance) {
        return bad("instance id is not 32 hex digits");
    }
    if h.format != "plain" && h.format != "gzip" {
        return bad("format is neither plain nor gzip");
    }
    if h.name != safe_name(&h.name) {
        return bad("display name is not a plain file name");
    }
    if h.untitled != h.original.is_none() {
        return bad("untitled flag disagrees with the original-file identity");
    }
    if h.text_bytes > MAX_TEXT {
        return Err(RecordError::TextTooLarge(h.text_bytes));
    }
    Ok(())
}

/// Header + exact text → record bytes.
pub fn encode(h: &Header, text: &str) -> Result<Vec<u8>, RecordError> {
    let payload = text.as_bytes();
    if payload.len() as u64 > MAX_TEXT {
        return Err(RecordError::TextTooLarge(payload.len() as u64));
    }
    validate_header(h)?;
    if h.text_bytes != payload.len() as u64 || h.text_fnv64 != fnv64(payload) {
        return Err(RecordError::BadHeader(
            "text size or hash does not match the text".into(),
        ));
    }
    let header = serde_json::to_vec(h).map_err(|e| RecordError::BadHeader(e.to_string()))?;
    if header.len() as u64 > MAX_HEADER {
        return Err(RecordError::HeaderTooLarge(header.len() as u64));
    }
    let mut out = Vec::with_capacity(PREFIX + header.len() + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32(&header).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32(payload).to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Record bytes → header + exact text, or the precise reason it is refused.
pub fn decode(b: &[u8]) -> Result<(Header, String), RecordError> {
    if b.len() < 8 {
        return Err(RecordError::TooShort);
    }
    if &b[..8] != MAGIC {
        return Err(RecordError::BadMagic);
    }
    if b.len() < PREFIX {
        return Err(RecordError::Truncated);
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    let version = u16_at(8);
    if version != VERSION {
        return Err(RecordError::UnsupportedVersion(version));
    }
    let hlen = u32_at(10) as u64;
    let hcrc = u32_at(14);
    let plen = u64_at(18);
    let pcrc = u32_at(26);
    if hlen > MAX_HEADER {
        return Err(RecordError::HeaderTooLarge(hlen));
    }
    if plen > MAX_TEXT {
        return Err(RecordError::TextTooLarge(plen));
    }
    let end = PREFIX as u64 + hlen + plen;
    if (b.len() as u64) < end {
        return Err(RecordError::Truncated);
    }
    if (b.len() as u64) > end {
        return Err(RecordError::TrailingBytes);
    }
    let header = &b[PREFIX..PREFIX + hlen as usize];
    let payload = &b[PREFIX + hlen as usize..];
    if crc32(header) != hcrc {
        return Err(RecordError::HeaderChecksum);
    }
    if crc32(payload) != pcrc {
        return Err(RecordError::PayloadChecksum);
    }
    let h: Header =
        serde_json::from_slice(header).map_err(|e| RecordError::BadHeader(e.to_string()))?;
    validate_header(&h)?;
    if h.text_bytes != plen || h.text_fnv64 != fnv64(payload) {
        return Err(RecordError::PayloadChecksum);
    }
    let text = String::from_utf8(payload.to_vec()).map_err(|_| RecordError::NotUtf8)?;
    Ok((h, text))
}

fn record_file(id: &str) -> String {
    format!("{RECORD_PREFIX}{id}{RECORD_SUFFIX}")
}

fn record_id_of(file: &str) -> Option<&str> {
    let id = file
        .strip_prefix(RECORD_PREFIX)?
        .strip_suffix(RECORD_SUFFIX)?;
    is_id(id).then_some(id)
}

#[cfg(unix)]
fn private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}
#[cfg(not(unix))]
fn private_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

fn create_private(path: &Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Create (or accept) a private, real directory; a symlink is refused so a
/// planted link cannot redirect recovery writes.
fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Err(io::Error::other(format!(
                "{} is not a plain directory",
                dir.display()
            )))
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::create_dir_all(dir)?,
        Err(e) => return Err(e),
    }
    private_dir(dir)
}

/// A summary of one record offered at startup.
#[derive(Debug, Clone)]
pub struct Offered {
    pub header: Header,
    /// `None` untitled; else `"unchanged"`, `"changed"` or `"missing"`.
    pub original_now: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Damaged {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub offered: Vec<Offered>,
    pub damaged: Vec<Damaged>,
    /// Records owned by another running WRL Forge process (left alone).
    pub live_elsewhere: usize,
}

/// The on-disk side. Tauri-free: the tests drive it directly.
pub struct Store {
    dir: PathBuf,
    instance: String,
    /// Held for the life of the process; the kernel releases it on any
    /// exit, crash included.
    _lock: File,
    temp_n: AtomicU64,
    /// Test hook: fail the next N writes after the temp file is created.
    fail_writes: AtomicU64,
}

impl Store {
    pub fn open(dir: &Path) -> io::Result<Store> {
        ensure_private_dir(dir)?;
        let inst_dir = dir.join(INSTANCES);
        ensure_private_dir(&inst_dir)?;
        let instance = new_id();
        let lock = create_private(&inst_dir.join(format!("{instance}.lock")))?;
        lock.lock()?;
        let s = Store {
            dir: dir.to_path_buf(),
            instance,
            _lock: lock,
            temp_n: AtomicU64::new(0),
            fail_writes: AtomicU64::new(0),
        };
        s.sweep();
        Ok(s)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn instance(&self) -> &str {
        &self.instance
    }
    pub fn record_path(&self, id: &str) -> Option<PathBuf> {
        is_id(id).then(|| self.dir.join(record_file(id)))
    }

    pub fn fail_next_writes(&self, n: u64) {
        self.fail_writes.store(n, Ordering::SeqCst);
    }

    /// Is the process `instance` still running (its lock still held)? An
    /// unreadable lock counts as alive: when in doubt, leave the record.
    fn owner_alive(&self, instance: &str) -> bool {
        if instance == self.instance {
            return true;
        }
        if !is_id(instance) {
            return false;
        }
        let p = self.dir.join(INSTANCES).join(format!("{instance}.lock"));
        match File::open(&p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(_) => true,
            Ok(f) => match f.try_lock() {
                Ok(()) => false,
                Err(fs::TryLockError::WouldBlock) => true,
                Err(fs::TryLockError::Error(_)) => true,
            },
        }
    }

    /// Remove temp files and lock files left by processes that are gone.
    /// Records are never removed here.
    fn sweep(&self) {
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(rest) = name.strip_prefix(TEMP_PREFIX) {
                    let owner = rest.split('-').next().unwrap_or("");
                    if !self.owner_alive(owner) {
                        let _ = fs::remove_file(e.path());
                    }
                }
            }
        }
        let owners: HashSet<String> = self
            .record_headers()
            .into_iter()
            .map(|h| h.instance)
            .collect();
        if let Ok(rd) = fs::read_dir(self.dir.join(INSTANCES)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_suffix(".lock").filter(|i| is_id(i)) else {
                    continue;
                };
                if !owners.contains(id) && !self.owner_alive(id) {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }

    fn record_headers(&self) -> Vec<Header> {
        let mut v = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(id) = record_id_of(&name) {
                    if let Ok((h, _)) = self.read(id) {
                        v.push(h);
                    }
                }
            }
        }
        v
    }

    /// Read and fully validate one record by id.
    pub fn read(&self, id: &str) -> Result<(Header, String), String> {
        let path = self.record_path(id).ok_or("not a recovery record id")?;
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err("the recovery record is not a regular file".into());
        }
        if meta.len() > PREFIX as u64 + MAX_HEADER + MAX_TEXT {
            return Err(RecordError::TextTooLarge(meta.len()).to_string());
        }
        let mut b = Vec::with_capacity(meta.len() as usize);
        File::open(&path)
            .and_then(|mut f| f.read_to_end(&mut b))
            .map_err(|e| e.to_string())?;
        let (h, text) = decode(&b).map_err(|e| e.to_string())?;
        if h.record != id {
            return Err(RecordError::NameMismatch.to_string());
        }
        Ok((h, text))
    }

    /// Every record left by a process that is no longer running.
    pub fn scan(&self) -> Scan {
        let mut s = Scan::default();
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return s;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let Some(id) = record_id_of(&name) else {
                continue;
            };
            match self.read(id) {
                Ok((h, _)) if self.owner_alive(&h.instance) => s.live_elsewhere += 1,
                Ok((h, _)) => {
                    let original_now = h.original.as_ref().map(original_state);
                    s.offered.push(Offered {
                        header: h,
                        original_now,
                    });
                }
                Err(reason) => s.damaged.push(Damaged {
                    id: id.to_string(),
                    reason,
                }),
            }
        }
        s.offered.sort_by(|a, b| {
            b.header
                .saved_at_ms
                .cmp(&a.header.saved_at_ms)
                .then(a.header.record.cmp(&b.header.record))
        });
        s
    }

    /// Encode and durably write a temp file; nothing is replaced yet.
    pub fn prepare(&self, h: &Header, text: &str) -> Result<Prepared, String> {
        let bytes = encode(h, text).map_err(|e| e.to_string())?;
        let n = self.temp_n.fetch_add(1, Ordering::Relaxed);
        let tmp = self
            .dir
            .join(format!("{TEMP_PREFIX}{}-{}-{n}", self.instance, h.record));
        let r = (|| -> io::Result<()> {
            let mut f = create_private(&tmp)?;
            if self
                .fail_writes
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                // Simulate a disk that fails mid-write: a partial temp.
                f.write_all(&bytes[..bytes.len() / 2])?;
                return Err(io::Error::other("simulated recovery write failure"));
            }
            f.write_all(&bytes)?;
            f.sync_all()
        })();
        match r {
            Ok(()) => Ok(Prepared {
                tmp: Some(tmp),
                dest: self.dir.join(record_file(&h.record)),
            }),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(format!("cannot write a recovery record: {e}"))
            }
        }
    }

    /// Atomically replace the record with a prepared temp, durably.
    pub fn commit(&self, mut p: Prepared) -> Result<(), String> {
        let tmp = p.tmp.take().expect("prepared once");
        if let Err(e) = fs::rename(&tmp, &p.dest) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("cannot replace the recovery record: {e}"));
        }
        sync_dir(&self.dir);
        Ok(())
    }

    /// Remove one record (absent is fine), durably.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let path = self.record_path(id).ok_or("not a recovery record id")?;
        match fs::remove_file(&path) {
            Ok(()) => {
                sync_dir(&self.dir);
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove the recovery record: {e}")),
        }
    }

    /// Remove a record offered at startup (Discard): only one whose owner
    /// is gone, never a live window's record.
    pub fn remove_orphan(&self, id: &str) -> Result<(), String> {
        if !is_id(id) {
            return Err("not a recovery record id".into());
        }
        if let Ok((h, _)) = self.read(id) {
            if self.owner_alive(&h.instance) {
                return Err("this recovery record belongs to a running WRL Forge window".into());
            }
        }
        self.remove(id)
    }
}

/// The original file NOW versus the identity in a record. Read-only, and
/// never larger than `MAX_TEXT`.
fn original_state(o: &Original) -> &'static str {
    let p = Path::new(&o.path);
    match fs::metadata(p) {
        Err(_) => "missing",
        Ok(m) if !m.is_file() => "missing",
        Ok(m) if m.len() != o.size || m.len() > MAX_TEXT => "changed",
        Ok(_) => match fs::read(p) {
            Ok(b) if o.same_bytes(&b) => "unchanged",
            Ok(_) => "changed",
            Err(_) => "missing",
        },
    }
}

/// A durably written temp file, not yet in place. Dropped uncommitted, it
/// removes itself.
pub struct Prepared {
    tmp: Option<PathBuf>,
    dest: PathBuf,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(t) = self.tmp.take() {
            let _ = fs::remove_file(t);
        }
    }
}

/// The bounded write schedule: a record is written `debounce` after the
/// last change, and at most `max_delay` after the first unwritten change,
/// so continuous typing still reaches the disk regularly.
#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    pub debounce: Duration,
    pub max_delay: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            debounce: Duration::from_millis(750),
            max_delay: Duration::from_millis(3000),
        }
    }
}

/// One session's state to write, copied under the session lock.
pub struct Snapshot {
    pub record: String,
    pub sequence: u64,
    pub revision: u64,
    pub format: &'static str,
    pub name: String,
    pub original: Option<Original>,
    pub text: String,
}

#[derive(Default)]
struct Inner {
    /// Last sequence applied per record (written or removed).
    applied: HashMap<String, u64>,
    /// Sessions with unwritten changes, and when the first one happened.
    pending: HashMap<u64, Instant>,
    last_mark: Option<Instant>,
    warning: Option<String>,
    writes: u64,
    stop: bool,
}

/// The coordinator: one per process. `store == None` means recovery is
/// unavailable (the warning says why); editing still works.
pub struct Recovery {
    store: Option<Store>,
    inner: Mutex<Inner>,
    cv: Condvar,
    seq: AtomicU64,
    schedule: Schedule,
}

impl Recovery {
    pub fn open(dir: &Path, schedule: Schedule) -> Recovery {
        match Store::open(dir) {
            Ok(store) => Recovery::new(Some(store), None, schedule),
            Err(e) => Recovery::unavailable(&format!("cannot open the recovery folder: {e}")),
        }
    }

    pub fn unavailable(reason: &str) -> Recovery {
        Recovery::new(
            None,
            Some(format!(
                "Crash recovery is unavailable ({reason}). Your document is unchanged; save your work often."
            )),
            Schedule::default(),
        )
    }

    fn new(store: Option<Store>, warning: Option<String>, schedule: Schedule) -> Recovery {
        Recovery {
            store,
            inner: Mutex::new(Inner {
                warning,
                ..Default::default()
            }),
            cv: Condvar::new(),
            seq: AtomicU64::new(1),
            schedule,
        }
    }

    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    pub fn warning(&self) -> Option<String> {
        self.inner.lock().ok()?.warning.clone()
    }

    /// Completed record writes in this process (smoke evidence).
    pub fn writes(&self) -> u64 {
        self.inner.lock().map_or(0, |i| i.writes)
    }

    fn set_warning(&self, m: Option<String>) {
        if let Ok(mut i) = self.inner.lock() {
            i.warning = m;
        }
    }

    /// A session has a new committed, dirty state: schedule a write.
    pub fn mark(&self, session: u64) {
        let Ok(mut i) = self.inner.lock() else { return };
        let now = Instant::now();
        i.pending.entry(session).or_insert(now);
        i.last_mark = Some(now);
        self.cv.notify_all();
    }

    /// Remove `record` now and advance its applied sequence to `seq`, so no
    /// older delayed write can bring it back (Save, undo-to-saved, Discard).
    pub fn retire(&self, record: &str, seq: u64) -> Result<(), String> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let mut i = self.inner.lock().map_err(|e| e.to_string())?;
        let applied = i.applied.entry(record.to_string()).or_insert(0);
        if seq < *applied {
            return Ok(());
        }
        *applied = seq;
        store.remove(record)
    }

    /// The restored session now owns `record` at `seq`.
    pub fn adopt(&self, record: &str, seq: u64) {
        if let Ok(mut i) = self.inner.lock() {
            i.applied.insert(record.to_string(), seq);
        }
    }

    /// Block until a write is due (or `stop`). `false` means stop.
    pub fn wait_due(&self) -> bool {
        let Ok(mut i) = self.inner.lock() else {
            return false;
        };
        loop {
            if i.stop {
                return false;
            }
            if let (Some(first), Some(last)) = (i.pending.values().min().copied(), i.last_mark) {
                let due = (last + self.schedule.debounce).min(first + self.schedule.max_delay);
                let now = Instant::now();
                if now >= due {
                    return true;
                }
                i = match self.cv.wait_timeout(i, due - now) {
                    Ok((g, _)) => g,
                    Err(_) => return false,
                };
            } else {
                i = match self.cv.wait(i) {
                    Ok(g) => g,
                    Err(_) => return false,
                };
            }
        }
    }

    pub fn stop(&self) {
        if let Ok(mut i) = self.inner.lock() {
            i.stop = true;
        }
        self.cv.notify_all();
    }

    /// Sessions with unwritten changes; clears the pending set.
    pub fn take_pending(&self) -> Vec<u64> {
        let Ok(mut i) = self.inner.lock() else {
            return vec![];
        };
        i.last_mark = None;
        let mut v: Vec<u64> = i.pending.drain().map(|(k, _)| k).collect();
        v.sort_unstable();
        v
    }

    fn already_applied(&self, record: &str, seq: u64) -> bool {
        self.inner
            .lock()
            .map(|i| i.applied.get(record).is_some_and(|&a| a >= seq))
            .unwrap_or(true)
    }

    /// Write one snapshot. Returns `Ok(true)` when it replaced the record,
    /// `Ok(false)` when a newer state (or a removal) already won.
    pub fn write(&self, snap: &Snapshot) -> Result<bool, String> {
        let Some(store) = &self.store else {
            return Err("recovery storage is unavailable".into());
        };
        if self.already_applied(&snap.record, snap.sequence) {
            return Ok(false);
        }
        let header = Header {
            record: snap.record.clone(),
            instance: store.instance().to_string(),
            sequence: snap.sequence,
            revision: snap.revision,
            saved_at_ms: now_ms(),
            format: snap.format.into(),
            untitled: snap.original.is_none(),
            name: safe_name(&snap.name),
            original: snap.original.clone(),
            text_bytes: snap.text.len() as u64,
            text_fnv64: fnv64(snap.text.as_bytes()),
        };
        let prepared = match store.prepare(&header, &snap.text) {
            Ok(p) => p,
            Err(e) => {
                self.set_warning(Some(format!(
                    "Crash recovery could not save a recovery copy ({e}). Your document is unchanged; the previous recovery copy, if any, is kept."
                )));
                return Err(e);
            }
        };
        // The sequence check and the rename are ONE critical section with
        // `retire`, so a removal can never be undone by a late write.
        let mut i = self.inner.lock().map_err(|e| e.to_string())?;
        let applied = i.applied.get(&snap.record).copied().unwrap_or(0);
        if applied >= snap.sequence {
            return Ok(false);
        }
        match store.commit(prepared) {
            Ok(()) => {
                i.applied.insert(snap.record.clone(), snap.sequence);
                i.writes += 1;
                i.warning = None;
                Ok(true)
            }
            Err(e) => {
                i.warning = Some(format!(
                    "Crash recovery could not save a recovery copy ({e}). Your document is unchanged."
                ));
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wrlforge-recovery-{tag}-{}-{}",
            std::process::id(),
            new_id()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn header(text: &str, untitled: bool) -> Header {
        Header {
            record: new_id(),
            instance: new_id(),
            sequence: 3,
            revision: 7,
            saved_at_ms: 1_700_000_000_000,
            format: "gzip".into(),
            untitled,
            name: if untitled {
                "untitled.wrl".into()
            } else {
                "item.wrl".into()
            },
            original: (!untitled).then(|| Original::of(Path::new("/x/item.wrl"), b"abc", None)),
            text_bytes: text.len() as u64,
            text_fnv64: fnv64(text.as_bytes()),
        }
    }

    const TRICKY: &str = "\u{FEFF}#VRML V2.0 utf8\r\n# c\u{e9} \u{1F600}\rDEF A Shape { } \n\n  USE A\r\nPROTO P [ ] { Group {} }\r{ unclosed [ \t \n";

    #[test]
    fn records_round_trip_exact_text() {
        for untitled in [false, true] {
            let h = header(TRICKY, untitled);
            let b = encode(&h, TRICKY).unwrap();
            let (h2, t2) = decode(&b).unwrap();
            assert_eq!(h2, h);
            assert_eq!(t2.as_bytes(), TRICKY.as_bytes());
        }
        let h = header("", true);
        assert_eq!(decode(&encode(&h, "").unwrap()).unwrap().1, "");
    }

    #[test]
    fn damaged_records_are_rejected_with_clear_reasons() {
        let h = header(TRICKY, false);
        let good = encode(&h, TRICKY).unwrap();
        let hlen = u32::from_le_bytes(good[10..14].try_into().unwrap()) as usize;
        assert_eq!(decode(&good[..4]), Err(RecordError::TooShort));
        let mut b = good.clone();
        b[0] = b'X';
        assert_eq!(decode(&b), Err(RecordError::BadMagic));
        let mut b = good.clone();
        b[8] = 2;
        assert_eq!(decode(&b), Err(RecordError::UnsupportedVersion(2)));
        assert_eq!(decode(&good[..good.len() - 1]), Err(RecordError::Truncated));
        assert_eq!(decode(&good[..20]), Err(RecordError::Truncated));
        let mut b = good.clone();
        b.push(0);
        assert_eq!(decode(&b), Err(RecordError::TrailingBytes));
        let mut b = good.clone();
        b[PREFIX + 2] ^= 1;
        assert_eq!(decode(&b), Err(RecordError::HeaderChecksum));
        let mut b = good.clone();
        let last = b.len() - 1;
        b[last] ^= 1;
        assert_eq!(decode(&b), Err(RecordError::PayloadChecksum));
        let mut b = good.clone();
        b[10..14].copy_from_slice(&(MAX_HEADER as u32 + 1).to_le_bytes());
        assert!(matches!(decode(&b), Err(RecordError::HeaderTooLarge(_))));
        let mut b = good.clone();
        b[18..26].copy_from_slice(&(MAX_TEXT + 1).to_le_bytes());
        assert!(matches!(decode(&b), Err(RecordError::TextTooLarge(_))));
        // A well-formed header that names a path-like display name.
        let mut bad = header(TRICKY, false);
        bad.name = "../../etc/passwd".into();
        assert!(matches!(
            encode(&bad, TRICKY),
            Err(RecordError::BadHeader(_))
        ));
        // Invalid UTF-8 payload with correct checksums.
        let mut h8 = header("ab", true);
        h8.text_fnv64 = fnv64(&[0xff, 0xfe]);
        let mut b = encode(&header("ab", true), "ab").unwrap();
        let hj = serde_json::to_vec(&h8).unwrap();
        b.truncate(PREFIX);
        b[10..14].copy_from_slice(&(hj.len() as u32).to_le_bytes());
        b[14..18].copy_from_slice(&crc32(&hj).to_le_bytes());
        b[26..30].copy_from_slice(&crc32(&[0xff, 0xfe]).to_le_bytes());
        b.extend_from_slice(&hj);
        b.extend_from_slice(&[0xff, 0xfe]);
        assert_eq!(decode(&b), Err(RecordError::NotUtf8));
        let _ = hlen;
    }

    fn snap(rec: &Recovery, record: &str, text: &str) -> Snapshot {
        Snapshot {
            record: record.into(),
            sequence: rec.next_seq(),
            revision: 1,
            format: "plain",
            name: "a.wrl".into(),
            original: None,
            text: text.into(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn store_is_private_and_writes_are_atomic() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmpdir("private").join("recovery");
        let rec = Recovery::open(&d, Schedule::default());
        assert!(rec.warning().is_none());
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&d), 0o700);
        let id = new_id();
        assert!(rec.write(&snap(&rec, &id, "one")).unwrap());
        let path = rec.store().unwrap().record_path(&id).unwrap();
        assert_eq!(mode(&path), 0o600);
        // No temp file survives a completed write.
        let names: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.starts_with(TEMP_PREFIX)),
            "{names:?}"
        );
        // A failed write (partial temp) keeps the older record intact.
        let before = fs::read(&path).unwrap();
        rec.store().unwrap().fail_next_writes(1);
        assert!(rec.write(&snap(&rec, &id, "two")).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(rec.warning().unwrap().contains("could not save"));
        let names: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.starts_with(TEMP_PREFIX)),
            "{names:?}"
        );
        // The next write succeeds and clears the warning.
        assert!(rec.write(&snap(&rec, &id, "three")).unwrap());
        assert!(rec.warning().is_none());
        assert_eq!(rec.store().unwrap().read(&id).unwrap().1, "three");
        // A real I/O failure: the folder becomes read-only.
        fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
        let r = rec.write(&snap(&rec, &id, "four"));
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(r.is_err());
        assert_eq!(rec.store().unwrap().read(&id).unwrap().1, "three");
    }

    #[test]
    fn a_damaged_record_is_refused_and_does_not_affect_original_files() {
        let root = tmpdir("damaged-original");
        let orig_file = root.join("item.wrl");
        fs::write(&orig_file, "original content").unwrap();

        let rec_dir = root.join("recovery");
        let rec = Recovery::open(&rec_dir, Schedule::default());
        let id = new_id();

        let snapshot = Snapshot {
            record: id.clone(),
            sequence: rec.next_seq(),
            revision: 1,
            format: "plain",
            name: "item.wrl".into(),
            original: Some(Original::of(&orig_file, b"original content", None)),
            text: "modified content".into(),
        };

        assert!(rec.write(&snapshot).unwrap());
        let record_path = rec.store().unwrap().record_path(&id).unwrap();
        let good_bytes = fs::read(&record_path).unwrap();

        drop(rec); // Release the lock so a new instance can offer records

        // Helper to check conditions
        let check_damaged = |second: &Recovery, reason: &str| {
            let s = second.store().unwrap().scan();
            assert!(s.offered.is_empty(), "Damaged record should not be offered");
            assert_eq!(s.damaged.len(), 1, "Record should be reported as damaged");
            assert_eq!(s.damaged[0].id, id);
            assert!(
                s.damaged[0].reason.contains(reason),
                "wrong refusal reason: {}",
                s.damaged[0].reason
            );
            assert_eq!(
                fs::read_to_string(&orig_file).unwrap(),
                "original content",
                "The original file must not be modified"
            );
        };

        let second = Recovery::open(&rec_dir, Schedule::default());

        // Test case 1: Truncated record
        fs::write(&record_path, &good_bytes[..good_bytes.len() - 1]).unwrap();
        check_damaged(&second, "truncated");

        // Test case 2: Bad payload checksum
        let mut bad_checksum = good_bytes.clone();
        let last = bad_checksum.len() - 1;
        bad_checksum[last] ^= 1;
        fs::write(&record_path, &bad_checksum).unwrap();
        check_damaged(&second, "recovered text failed its checksum");

        // Test case 3: Extra trailing bytes
        let mut trailing_bytes = good_bytes.clone();
        trailing_bytes.push(0);
        fs::write(&record_path, &trailing_bytes).unwrap();
        check_damaged(&second, "unexpected bytes");
    }

    #[test]
    fn a_late_write_never_recreates_a_retired_record() {
        let d = tmpdir("race").join("recovery");
        let rec = Recovery::open(&d, Schedule::default());
        let id = new_id();
        let old = snap(&rec, &id, "typed");
        // Save (or Discard) wins the race after the snapshot was taken.
        let save_seq = rec.next_seq();
        rec.retire(&id, save_seq).unwrap();
        assert_eq!(rec.write(&old), Ok(false));
        assert!(!rec.store().unwrap().record_path(&id).unwrap().exists());
        // An OLDER snapshot never replaces a newer record either.
        let a = snap(&rec, &id, "older");
        let b = snap(&rec, &id, "newer");
        assert_eq!(rec.write(&b), Ok(true));
        assert_eq!(rec.write(&a), Ok(false));
        assert_eq!(rec.store().unwrap().read(&id).unwrap().1, "newer");
        // A stale retire does not remove a newer record.
        rec.retire(&id, a.sequence).unwrap();
        assert_eq!(rec.store().unwrap().read(&id).unwrap().1, "newer");
    }

    #[test]
    fn only_records_of_exited_processes_are_offered() {
        let d = tmpdir("owners").join("recovery");
        let first = Recovery::open(&d, Schedule::default());
        let id = new_id();
        first.write(&snap(&first, &id, "work")).unwrap();
        // A second process while the first runs: nothing to offer.
        let second = Recovery::open(&d, Schedule::default());
        let s = second.store().unwrap().scan();
        assert!(s.offered.is_empty());
        assert_eq!(s.live_elsewhere, 1);
        assert!(second.store().unwrap().remove_orphan(&id).is_err());
        // The first process exits (its lock is released): offered.
        drop(first);
        let s = second.store().unwrap().scan();
        assert_eq!(s.offered.len(), 1);
        assert_eq!(s.offered[0].header.record, id);
        // A damaged record is reported, never offered, never deleted.
        let bad = second.store().unwrap().record_path(&new_id()).unwrap();
        fs::write(&bad, b"WFRECOV\nshort").unwrap();
        let s = second.store().unwrap().scan();
        assert_eq!(s.offered.len(), 1);
        assert_eq!(s.damaged.len(), 1);
        assert!(
            s.damaged[0].reason.contains("truncated"),
            "{}",
            s.damaged[0].reason
        );
        assert!(bad.exists());
        // Unknown files and symlinks are ignored.
        fs::write(d.join("notes.txt"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            "/etc/hostname",
            second.store().unwrap().record_path(&new_id()).unwrap(),
        )
        .unwrap();
        let s = second.store().unwrap().scan();
        assert_eq!(s.offered.len(), 1);
        assert!(s
            .damaged
            .iter()
            .all(|x| x.reason.contains("truncated") || x.reason.contains("regular file")));
        second.store().unwrap().remove_orphan(&id).unwrap();
        assert!(second.store().unwrap().scan().offered.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_recovery_folder_is_refused() {
        let d = tmpdir("link");
        let real = d.join("real");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, d.join("recovery")).unwrap();
        let rec = Recovery::open(&d.join("recovery"), Schedule::default());
        assert!(rec.store().is_none());
        assert!(rec.warning().unwrap().contains("unavailable"));
    }

    #[test]
    fn the_schedule_is_bounded() {
        let rec = std::sync::Arc::new(Recovery::open(
            &tmpdir("sched").join("recovery"),
            Schedule {
                debounce: Duration::from_millis(80),
                max_delay: Duration::from_millis(200),
            },
        ));
        let t0 = Instant::now();
        let r2 = rec.clone();
        let typer = std::thread::spawn(move || {
            // Continuous "typing" every 20 ms for 600 ms.
            while t0.elapsed() < Duration::from_millis(600) {
                r2.mark(1);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        std::thread::sleep(Duration::from_millis(5));
        assert!(rec.wait_due());
        let waited = t0.elapsed();
        assert!(
            waited >= Duration::from_millis(150) && waited < Duration::from_millis(450),
            "{waited:?}"
        );
        assert_eq!(rec.take_pending(), vec![1]);
        typer.join().unwrap();
        rec.stop();
        assert!(!rec.wait_due());
    }

    #[test]
    fn safe_names_never_carry_directories() {
        assert_eq!(safe_name("/home/u/w/item.wrl"), "item.wrl");
        assert_eq!(safe_name("C:\\a\\b.wrl"), "b.wrl");
        assert_eq!(safe_name(".."), "untitled.wrl");
        assert_eq!(safe_name("a\u{7}b.wrl"), "ab.wrl");
    }
}
