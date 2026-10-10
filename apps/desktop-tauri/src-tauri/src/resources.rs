// SPDX-License-Identifier: GPL-3.0-or-later
//! TEXTURE-LOCAL-1: local preview resources (read-only image textures).
//!
//! X_ITE resolves a relative `ImageTexture` URL against the preview's base
//! URL. Rust gives each preview generation of a saved document an opaque
//! token and the base `wrlres://localhost/<token>/`; the `wrlres` scheme
//! handler (`lib.rs`) answers a request ONLY when the token is the CURRENT
//! token of an open session, and only with an image file inside that
//! session's document folder.
//!
//! The WebView never names a folder or an absolute path: the folder is the
//! Rust-owned session path's parent, looked up at request time (so a Save As
//! moves later loads to the new folder). The document text is never
//! rewritten -- only X_ITE's base URL changes, so source spans, provenance
//! and picking are unaffected.
//!
//! Refused: unknown / replaced / closed-session tokens, untitled documents,
//! dot segments, encoded separators, absolute paths, symlinks or folders
//! that resolve outside the document folder, non-image extensions, files
//! over `MAX_RESOURCE_BYTES`, and bytes that are not a JPEG / PNG / GIF /
//! BMP / WebP image. Network and `file:` URLs never reach Rust: the CSP has
//! no remote origin and no `file:` source.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use wrlforge_desktop_protocol as p;
use wrlforge_vrml::ast::{Ast, Node};

/// The URI scheme X_ITE fetches preview resources through.
pub const SCHEME: &str = "wrlres";

/// The largest resource the preview reads: 32 MiB. A 4096×4096 RGBA PNG is
/// at most ~64 MiB uncompressed but far smaller on disk; normal VRML
/// textures are well under 1 MiB.
pub const MAX_RESOURCE_BYTES: u64 = 32 * 1024 * 1024;

/// Extensions the preview serves (compared case-insensitively). The bytes
/// must ALSO carry the matching image signature (`sniff`).
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp"];

/// The base for a document without a folder (an untitled world): a token
/// that is never issued, so every relative URL is refused.
pub const NO_FOLDER_TOKEN: &str = "none";

/// The base URL X_ITE resolves relative URLs against for `token`.
pub fn base_url(token: &str) -> String {
    // WebKitGTK and WKWebView expose a custom scheme as `<scheme>://localhost`.
    format!("{SCHEME}://localhost/{token}/")
}

/// A fresh, unguessable token (128 random bits from the std hasher keys
/// plus a process counter). Tokens are never reused.
pub fn new_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut words = [0u64; 2];
    for w in &mut words {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(n);
        *w = h.finish();
    }
    format!("t{:016x}{:016x}", words[0], words[1])
}

/// Why a resource was not served. `as_str` is shown in logs and warnings;
/// none of them carries a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    BadRequest,
    /// The token is unknown, replaced by a newer preview, or its session is
    /// closed.
    Stale,
    Untitled,
    Remote,
    Absolute,
    Traversal,
    Escape,
    NotFound,
    NotAFile,
    Unsupported,
    TooLarge,
    InvalidImage,
    Unreadable,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::BadRequest => "malformed resource path",
            Refusal::Stale => "preview replaced or document closed",
            Refusal::Untitled => "the document is not saved in a folder yet",
            Refusal::Remote => "network and file: URLs are not loaded",
            Refusal::Absolute => "absolute paths are not loaded",
            Refusal::Traversal => "path leaves the document folder",
            Refusal::Escape => "link resolves outside the document folder",
            Refusal::NotFound => "not found",
            Refusal::NotAFile => "not a file",
            Refusal::Unsupported => "not a supported image type",
            Refusal::TooLarge => "larger than the 32 MiB texture limit",
            Refusal::InvalidImage => "not a valid image file",
            Refusal::Unreadable => "unreadable",
        }
    }

    pub fn status(self) -> u16 {
        match self {
            Refusal::NotFound => 404,
            Refusal::BadRequest => 400,
            Refusal::TooLarge => 413,
            Refusal::Unsupported | Refusal::InvalidImage => 415,
            Refusal::Unreadable => 500,
            _ => 403,
        }
    }
}

/// Split a request path `/<token>/<rest>` into its token and the rest.
pub fn split_request(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_prefix('/')?;
    let (token, rest) = path.split_once('/')?;
    (!token.is_empty() && !rest.is_empty()).then_some((token, rest))
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> Result<String, Refusal> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let (Some(h), Some(l)) = (
                b.get(i + 1).copied().and_then(hex),
                b.get(i + 2).copied().and_then(hex),
            ) else {
                return Err(Refusal::BadRequest);
            };
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Refusal::BadRequest)
}

/// Decode the `/`-separated, percent-encoded path below the token into
/// plain file-name segments. Every segment must be a real name: no empty,
/// `.` or `..` segment, and no separator, NUL or drive colon hidden by
/// percent-encoding.
pub fn decode_segments(rest: &str) -> Result<Vec<String>, Refusal> {
    let mut out = Vec::new();
    for raw in rest.split('/') {
        let seg = percent_decode(raw)?;
        if seg.is_empty() {
            return Err(Refusal::BadRequest);
        }
        if seg == "." || seg == ".." {
            return Err(Refusal::Traversal);
        }
        if seg.contains(['/', '\\', '\0']) || (cfg!(windows) && seg.contains(':')) {
            return Err(Refusal::Traversal);
        }
        out.push(seg);
    }
    Ok(out)
}

/// The canonical file for `segments` below `folder`, or a refusal. The
/// folder and the file are both canonicalized, so a symlink (file or
/// directory) that leads outside the folder is refused.
pub fn resolve(folder: &Path, segments: &[String]) -> Result<PathBuf, Refusal> {
    let root = folder.canonicalize().map_err(|_| Refusal::NotFound)?;
    let ext = segments
        .last()
        .and_then(|n| Path::new(n).extension())
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if !ext.is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str())) {
        return Err(Refusal::Unsupported);
    }
    let mut joined = root.clone();
    for s in segments {
        joined.push(s);
    }
    let real = match joined.canonicalize() {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::NotFound),
        Err(_) => return Err(Refusal::Unreadable),
    };
    if !real.starts_with(&root) {
        return Err(Refusal::Escape);
    }
    let meta = std::fs::metadata(&real).map_err(|_| Refusal::Unreadable)?;
    if !meta.is_file() {
        return Err(Refusal::NotAFile);
    }
    if meta.len() > MAX_RESOURCE_BYTES {
        return Err(Refusal::TooLarge);
    }
    Ok(real)
}

/// The MIME type of an image signature X_ITE (the WebView) decodes.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.starts_with(b"BM") && b.len() >= 26 {
        Some("image/bmp")
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Read a resolved image: bounded by `MAX_RESOURCE_BYTES` even if the file
/// grows after `resolve`, and only if its signature is an image.
pub fn read_image(path: &Path) -> Result<(Vec<u8>, &'static str), Refusal> {
    let f = std::fs::File::open(path).map_err(|_| Refusal::Unreadable)?;
    let mut bytes = Vec::new();
    f.take(MAX_RESOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::Unreadable)?;
    if bytes.len() as u64 > MAX_RESOURCE_BYTES {
        return Err(Refusal::TooLarge);
    }
    let mime = sniff(&bytes).ok_or(Refusal::InvalidImage)?;
    Ok((bytes, mime))
}

/// Only the signature: the static check reads 32 bytes, not the image.
fn sniff_file(path: &Path) -> Result<(), Refusal> {
    let f = std::fs::File::open(path).map_err(|_| Refusal::Unreadable)?;
    let mut head = Vec::new();
    f.take(32)
        .read_to_end(&mut head)
        .map_err(|_| Refusal::Unreadable)?;
    sniff(&head).map(|_| ()).ok_or(Refusal::InvalidImage)
}

// ---- the request handler ----------------------------------------------------

/// A served (or refused) request: what the scheme handler answers.
pub struct Served {
    pub status: u16,
    pub mime: &'static str,
    pub body: Vec<u8>,
}

/// TEST ONLY (`--smoke-texture`): hold every read this long between the
/// authorization and the re-check, so a document switch can land while a
/// request is in flight. Zero (off) unless that smoke run is armed.
pub static TEST_DELAY_MS: AtomicU64 = AtomicU64::new(0);

const LOG_CAP: usize = 512;
static LOG: std::sync::Mutex<std::collections::VecDeque<p::ResourceLogEntry>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

fn log(session: Option<p::SessionId>, name: &str, status: u16, outcome: &str, bytes: u64) {
    if let Ok(mut l) = LOG.lock() {
        if l.len() == LOG_CAP {
            l.pop_front();
        }
        l.push_back(p::ResourceLogEntry {
            session,
            name: name.chars().take(200).collect(),
            status,
            outcome: outcome.into(),
            bytes,
        });
    }
}

/// The answered requests so far (oldest first; at most 512).
pub fn log_snapshot() -> Vec<p::ResourceLogEntry> {
    LOG.lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

/// Answer one `wrlres` request for `path` (the URI path, `/<token>/<rest>`).
/// Read-only: it never writes, and it reads only an image file inside the
/// document folder of the session whose CURRENT token is `<token>`.
pub fn serve(svc: &crate::service::Service, path: &str) -> Served {
    let refuse = |session, name: &str, r: Refusal| {
        log(session, name, r.status(), r.as_str(), 0);
        Served {
            status: r.status(),
            mime: "text/plain",
            body: Vec::new(),
        }
    };
    let Some((token, rest)) = split_request(path) else {
        return refuse(None, "", Refusal::BadRequest);
    };
    let (session, folder) = match svc.resource_folder(token) {
        Ok(v) => v,
        Err(r) => return refuse(None, rest, r),
    };
    let read = decode_segments(rest)
        .and_then(|segs| resolve(&folder, &segs))
        .and_then(|real| read_image(&real));
    let delay = TEST_DELAY_MS.load(Ordering::Relaxed);
    if delay > 0 {
        log(Some(session), rest, 0, "held (test delay)", 0);
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    // Re-check AFTER the read: a generation replaced, a document closed or
    // reopened while this request was in flight gets nothing.
    if !svc.resource_token_current(session, token) {
        return refuse(Some(session), rest, Refusal::Stale);
    }
    match read {
        Ok((body, mime)) => {
            log(Some(session), rest, 200, "served", body.len() as u64);
            Served {
                status: 200,
                mime,
                body,
            }
        }
        Err(r) => refuse(Some(session), rest, r),
    }
}

// ---- static check of a document's ImageTexture URLs -------------------------

fn has_scheme(u: &str) -> bool {
    let mut it = u.chars();
    it.next().is_some_and(|c| c.is_ascii_alphabetic())
        && u.find(':').is_some_and(|i| {
            u[..i]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
}

/// How the preview will treat ONE url string of a document in `folder`
/// (`None` = untitled). Mirrors what the WebView does to a relative URL
/// (query and fragment dropped, dot segments resolved -- including their
/// `%2e` spellings -- before the request) and then what the handler does.
pub fn check_url(url: &str, folder: Option<&Path>) -> Result<(), Refusal> {
    let u = url.trim();
    if u.starts_with("data:") {
        return Ok(());
    }
    if has_scheme(u) {
        return Err(Refusal::Remote);
    }
    if u.starts_with('/') || u.starts_with('\\') {
        return Err(Refusal::Absolute);
    }
    let path = u.split(['?', '#']).next().unwrap_or("");
    let mut segs: Vec<&str> = Vec::new();
    for s in path.split('/') {
        match s.to_ascii_lowercase().as_str() {
            "." | "%2e" => {}
            ".." | ".%2e" | "%2e." | "%2e%2e" => {
                if segs.pop().is_none() {
                    return Err(Refusal::Traversal);
                }
            }
            _ => segs.push(s),
        }
    }
    if segs.is_empty() {
        return Err(Refusal::BadRequest);
    }
    let folder = folder.ok_or(Refusal::Untitled)?;
    let segments = decode_segments(&segs.join("/"))?;
    let real = resolve(folder, &segments)?;
    sniff_file(&real)
}

fn short(url: &str, why: Refusal) -> String {
    let u = url.trim();
    // Never echo an absolute local path back into the UI.
    let shown = match why {
        Refusal::Absolute => "(absolute path)".to_string(),
        Refusal::Remote if u.to_ascii_lowercase().starts_with("file:") => "(file: URL)".into(),
        Refusal::Remote => match u.split_once("://") {
            Some((scheme, rest)) => {
                format!("{scheme}://{}/…", rest.split('/').next().unwrap_or(""))
            }
            None => u.chars().take(40).collect(),
        },
        _ => u.chars().take(80).collect(),
    };
    format!("\"{shown}\": {}", why.as_str())
}

fn url_strings(v: &Ast) -> Option<Vec<String>> {
    match v {
        Ast::Str { value, .. } => Some(vec![value.clone()]),
        Ast::Array { items, .. } => Some(
            items
                .iter()
                .filter_map(|i| match i {
                    Ast::Str { value, .. } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

fn check_node(n: &Node, folder: Option<&Path>, out: &mut Vec<p::TextureWarning>) {
    if n.node_type == "ImageTexture" {
        let urls = n.fields.iter().find_map(|f| match f {
            Ast::Field(f) if f.name == "url" => f.value.as_deref().and_then(url_strings),
            _ => None,
        });
        // `url IS x` (PROTO body) cannot be judged here; no url = nothing to load.
        if let Some(urls) = urls.filter(|u| !u.is_empty()) {
            let mut why = Vec::new();
            let mut ok = false;
            for u in &urls {
                match check_url(u, folder) {
                    Ok(()) => {
                        ok = true;
                        break;
                    }
                    Err(r) => why.push(short(u, r)),
                }
            }
            if !ok {
                out.push(p::TextureWarning {
                    line: n.range.start.line,
                    node: match &n.def {
                        Some(d) => format!("ImageTexture {d}"),
                        None => "ImageTexture".into(),
                    },
                    detail: why.join("; "),
                });
            }
        }
    }
    for f in &n.fields {
        walk(f, folder, out);
    }
}

fn walk(a: &Ast, folder: Option<&Path>, out: &mut Vec<p::TextureWarning>) {
    match a {
        Ast::Node(n) => check_node(n, folder, out),
        Ast::Field(f) => {
            if let Some(v) = f.value.as_deref() {
                walk(v, folder, out);
            }
        }
        Ast::Array { items, .. } => items.iter().for_each(|i| walk(i, folder, out)),
        Ast::Proto(pr) => pr.body.iter().for_each(|i| walk(i, folder, out)),
        _ => {}
    }
}

/// One warning per `ImageTexture` (a DEF'd texture shared through USE is
/// one node) with NO url the preview can load from `folder`.
pub fn check_textures(text: &str, folder: Option<&Path>) -> Vec<p::TextureWarning> {
    const MAX_WARNINGS: usize = 64;
    let parsed = wrlforge_vrml::parse(text);
    let mut out = Vec::new();
    for s in &parsed.tree.statements {
        walk(s, folder, &mut out);
    }
    out.truncate(MAX_WARNINGS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wrlres-test-{tag}-{}-{}",
            std::process::id(),
            new_token()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn tokens_are_unique_and_opaque() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 33);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(base_url("tx"), "wrlres://localhost/tx/");
    }

    #[test]
    fn request_paths_and_segments() {
        assert_eq!(split_request("/tok/a/b.png"), Some(("tok", "a/b.png")));
        assert_eq!(split_request("/tok/"), None);
        assert_eq!(split_request("tok/a"), None);
        assert_eq!(
            decode_segments("my%20texture.jpg").unwrap(),
            vec!["my texture.jpg".to_string()]
        );
        assert_eq!(decode_segments("..").unwrap_err(), Refusal::Traversal);
        assert_eq!(
            decode_segments("%2e%2e/x.png").unwrap_err(),
            Refusal::Traversal
        );
        assert_eq!(
            decode_segments("..%2fx.png").unwrap_err(),
            Refusal::Traversal
        );
        assert_eq!(
            decode_segments("a%5cx.png").unwrap_err(),
            Refusal::Traversal
        );
        assert_eq!(decode_segments("a%00.png").unwrap_err(), Refusal::Traversal);
        assert_eq!(
            decode_segments("a//b.png").unwrap_err(),
            Refusal::BadRequest
        );
        assert_eq!(
            decode_segments("bad%zz.png").unwrap_err(),
            Refusal::BadRequest
        );
        assert_eq!(decode_segments("%ff.png").unwrap_err(), Refusal::BadRequest);
    }

    #[test]
    fn resolve_confines_to_the_folder() {
        let base = tmp("resolve");
        let doc = base.join("doc");
        std::fs::create_dir_all(doc.join("sub")).unwrap();
        std::fs::write(doc.join("a.png"), PNG).unwrap();
        std::fs::write(doc.join("sub/b.PNG"), PNG).unwrap();
        std::fs::write(doc.join("notes.txt"), b"x").unwrap();
        std::fs::write(base.join("outside.png"), PNG).unwrap();
        std::fs::create_dir_all(doc.join("dir.png")).unwrap();
        let seg = |s: &str| decode_segments(s).unwrap();
        assert!(resolve(&doc, &seg("a.png")).is_ok());
        assert!(resolve(&doc, &seg("sub/b.PNG")).is_ok());
        assert_eq!(resolve(&doc, &seg("nope.png")), Err(Refusal::NotFound));
        assert_eq!(resolve(&doc, &seg("notes.txt")), Err(Refusal::Unsupported));
        assert_eq!(resolve(&doc, &seg("dir.png")), Err(Refusal::NotAFile));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("../outside.png", doc.join("link.png")).unwrap();
            std::os::unix::fs::symlink("..", doc.join("up")).unwrap();
            std::os::unix::fs::symlink("a.png", doc.join("inside.png")).unwrap();
            assert_eq!(resolve(&doc, &seg("link.png")), Err(Refusal::Escape));
            assert_eq!(resolve(&doc, &seg("up/outside.png")), Err(Refusal::Escape));
            assert!(resolve(&doc, &seg("inside.png")).is_ok());
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn read_image_checks_signature_and_size() {
        let d = tmp("read");
        std::fs::write(d.join("ok.png"), PNG).unwrap();
        std::fs::write(d.join("bad.jpg"), b"this is not an image").unwrap();
        assert_eq!(read_image(&d.join("ok.png")).unwrap().1, "image/png");
        assert_eq!(
            read_image(&d.join("bad.jpg")).unwrap_err(),
            Refusal::InvalidImage
        );
        let big = std::fs::File::create(d.join("big.png")).unwrap();
        big.set_len(MAX_RESOURCE_BYTES + 1).unwrap();
        let segs = decode_segments("big.png").unwrap();
        assert_eq!(resolve(&d, &segs), Err(Refusal::TooLarge));
        assert_eq!(
            read_image(&d.join("big.png")).unwrap_err(),
            Refusal::TooLarge
        );
        assert_eq!(sniff(b"GIF89a.."), Some("image/gif"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn check_url_mirrors_the_webview_and_the_handler() {
        let base = tmp("check");
        let doc = base.join("doc");
        std::fs::create_dir_all(doc.join("sub")).unwrap();
        std::fs::write(doc.join("a.png"), PNG).unwrap();
        std::fs::write(doc.join("my tex.png"), PNG).unwrap();
        std::fs::write(base.join("outside.png"), PNG).unwrap();
        let f = Some(doc.as_path());
        assert_eq!(check_url("a.png", f), Ok(()));
        assert_eq!(check_url("./a.png", f), Ok(()));
        assert_eq!(check_url("sub/../a.png", f), Ok(()));
        assert_eq!(check_url("a.png?x=1#y", f), Ok(()));
        assert_eq!(check_url("my tex.png", f), Ok(()));
        assert_eq!(check_url("my%20tex.png", f), Ok(()));
        assert_eq!(check_url("data:image/png;base64,AAAA", f), Ok(()));
        assert_eq!(check_url("../outside.png", f), Err(Refusal::Traversal));
        assert_eq!(check_url("%2e%2e/outside.png", f), Err(Refusal::Traversal));
        assert_eq!(
            check_url("sub/../../outside.png", f),
            Err(Refusal::Traversal)
        );
        assert_eq!(check_url("..%2foutside.png", f), Err(Refusal::Traversal));
        assert_eq!(
            check_url("http://example.invalid/a.png", f),
            Err(Refusal::Remote)
        );
        assert_eq!(
            check_url("HTTPS://example.invalid/a.png", f),
            Err(Refusal::Remote)
        );
        assert_eq!(check_url("file:///etc/a.png", f), Err(Refusal::Remote));
        assert_eq!(check_url("/etc/a.png", f), Err(Refusal::Absolute));
        assert_eq!(check_url("missing.png", f), Err(Refusal::NotFound));
        assert_eq!(check_url("a.png", None), Err(Refusal::Untitled));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn check_textures_reports_each_unloadable_node_once() {
        let d = tmp("tex");
        std::fs::write(d.join("a.png"), PNG).unwrap();
        std::fs::write(d.join("junk.jpg"), b"nope").unwrap();
        let text = "#VRML V2.0 utf8\n\
            Shape { appearance Appearance { texture DEF T ImageTexture { url [ \"missing.jpg\" \"a.png\" ] } } }\n\
            Shape { appearance Appearance { texture USE T } }\n\
            Shape { appearance Appearance { texture ImageTexture { url \"junk.jpg\" } } }\n\
            Shape { appearance Appearance { texture DEF R ImageTexture { url [ \"http://x.invalid/a.png\" \"/home/someone/a.png\" ] } } }\n\
            PROTO P [ field MFString u [] ] { Shape { appearance Appearance { texture ImageTexture { url IS u } } } }\n";
        let w = check_textures(text, Some(&d));
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(w[0].node, "ImageTexture");
        assert!(w[0].detail.contains("not a valid image"), "{w:?}");
        assert_eq!(w[1].node, "ImageTexture R");
        assert!(w[1].detail.contains("http://x.invalid/…"), "{w:?}");
        assert!(!w[1].detail.contains("/home/someone"), "{w:?}");
        assert!(w[1].line > w[0].line);
        let untitled = check_textures(text, None);
        assert_eq!(untitled.len(), 3);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
