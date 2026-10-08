// SPDX-License-Identifier: GPL-3.0-or-later
//! RUST-1 WebAssembly boundary over `wrlforge-text`.
//!
//! These raw exports are NOT the public API. JavaScript callers use the facade
//! in `js/wrlforge-text.mjs`, which owns shape validation, session branding and
//! error compatibility. The raw layer still re-checks every value it receives
//! and fails closed, so a direct caller cannot bypass the UTF-16 gate or the
//! numeric checks. It is NOT a session-isolation boundary (RUST-1A): code that
//! holds this module can forge `__wbg_ptr` and read linear memory.
//!
//! UTF-16 SAFETY GATE. wasm-bindgen converts a JS string to a Rust `String`
//! through `TextEncoder`, which replaces an unpaired surrogate with U+FFFD. That
//! would silently change source text. So every JS string is received as a
//! `JsString` and passes `take_text`, which refuses with `EENCODING` instead of
//! replacing. RUST-1A: the gate checks the converted text against the source
//! code units and does not trust `String.prototype.isWellFormed`, which a caller
//! can replace. No path substitutes.
//!
//! Rust -> JS strings are always valid UTF-8, which `TextDecoder` maps to the
//! exact same UTF-16 sequence.
#![forbid(unsafe_code)]

pub mod session;

use js_sys::{Array, JsString, Reflect};
use session::{Counter, SessionCore, SessionError, MAX_SAFE};
use wasm_bindgen::prelude::*;
use wrlforge_text::line_index::LineError;
use wrlforge_text::snapshot::SpanError;
use wrlforge_text::{Affinity, Edit, EditError, OffsetError};

thread_local! {
    // One counter per WebAssembly instance; never reset while it lives.
    static COUNTER: Counter = const { Counter::new() };
}

#[wasm_bindgen]
extern "C" {
    /// A local name for a JS string, so the native method can be bound.
    #[wasm_bindgen(js_name = String)]
    type WellFormedProbe;

    // Native, single call; `catch` so an engine without it fails closed.
    #[wasm_bindgen(method, js_name = isWellFormed, catch)]
    fn is_well_formed(this: &WellFormedProbe) -> Result<bool, JsValue>;
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

fn error(code: &str, message: &str, fields: &[(&str, JsValue)]) -> JsValue {
    let e = js_sys::Error::new(message);
    let _ = Reflect::set(&e, &"code".into(), &code.into());
    for (k, v) in fields {
        let _ = Reflect::set(&e, &JsValue::from_str(k), v);
    }
    e.into()
}

fn num(v: u64) -> JsValue {
    JsValue::from_f64(v as f64)
}

fn edit_error(e: EditError) -> JsValue {
    let mut fields = Vec::new();
    if let Some(i) = e.index {
        fields.push(("index", num(i as u64)));
    }
    if let Some(i) = e.other_index {
        fields.push(("otherIndex", num(i as u64)));
    }
    error(e.code.as_str(), e.code.as_str(), &fields)
}

fn offset_error(e: OffsetError) -> JsValue {
    match e {
        OffsetError::OutOfBounds { offset, len } => error(
            "EOFFSETBOUNDS",
            "offset is past the end of the text",
            &[("offset", num(offset as u64)), ("length", num(len as u64))],
        ),
        OffsetError::InsideSurrogatePair { offset } => error(
            "EOFFSETSURROGATE",
            "offset is inside a surrogate pair",
            &[("offset", num(offset as u64))],
        ),
        OffsetError::NotCharBoundary { offset } => error(
            "EOFFSETBYTE",
            "UTF-8 offset is inside a multi-byte character",
            &[("offset", num(offset as u64))],
        ),
    }
}

fn session_error(e: SessionError) -> JsValue {
    error(e.code(), e.code(), &[])
}

fn argument(what: &str) -> JsValue {
    error("EARGUMENT", what, &[])
}

// ---------------------------------------------------------------------------
// arguments
// ---------------------------------------------------------------------------

/// The UTF-16 index of the first U+FFFD in `text` that is NOT a genuine
/// 0xFFFD code unit of `s`, i.e. a substitution, and that unit's value.
#[cfg(not(feature = "negative-control-lossy-utf16"))]
fn first_substitution(s: &JsString, text: &str) -> Option<(usize, f64)> {
    let mut value = 0.0;
    let unit = wrlforge_text::offsets::first_unconfirmed_replacement(text, |u| {
        value = u32::try_from(u).map_or(f64::NAN, |u| s.char_code_at(u));
        value == f64::from(0xFFFD_u16)
    })?;
    Some((unit, value))
}

/// The UTF-16 gate. `field` names the argument; `index` is the edit index.
///
/// RUST-1A: the gate never rests on `String.prototype.isWellFormed` alone. A
/// caller can replace that method at any time; RUST-1A reproduced 144 silent
/// substitutions per lying mode (8 malformed inputs x 18 entry points).
///
/// 1. `isWellFormed` must exist and answer. A missing or throwing method is
///    `EENGINE`. Its answer is a claim, never proof.
/// 2. Independent of (1), the converted text is checked against the source
///    code units. `TextEncoder` replaces one unpaired surrogate with one U+FFFD
///    and copies everything else (WHATWG Encoding), so UTF-16 positions are
///    preserved. Each U+FFFD in the converted text must sit over a genuine
///    0xFFFD source unit. Text without U+FFFD needs no unit reads; text with k
///    of them needs k reads, not one per unit.
/// 3. A substituted surrogate is `EENCODING` with `unit` = its index (the
///    first unpaired surrogate). A `false` claim about text that (2) proves
///    well formed means the method lied: `EENGINE`.
///
/// Trust root (documented, not checkable from inside the realm): the
/// intrinsics that the wasm-bindgen glue itself uses to move a string
/// (`TextEncoder.prototype.encodeInto`, `String.prototype.charCodeAt`).
fn take_text(s: &JsString, field: &str, index: Option<usize>) -> Result<String, JsValue> {
    #[cfg(not(feature = "negative-control-lossy-utf16"))]
    let claimed = match s.unchecked_ref::<WellFormedProbe>().is_well_formed() {
        Ok(claimed) => claimed,
        Err(_) => {
            return Err(error(
                "EENGINE",
                "String.prototype.isWellFormed is unavailable; refusing to convert text",
                &[],
            ))
        }
    };
    let text = s.as_string().ok_or_else(|| argument("expected a string"))?;
    #[cfg(not(feature = "negative-control-lossy-utf16"))]
    {
        if let Some((unit, value)) = first_substitution(s, &text) {
            if !(f64::from(0xD800_u16)..=f64::from(0xDFFF_u16)).contains(&value) {
                return Err(error(
                    "EENGINE",
                    "string conversion changed a non-surrogate code unit; refused",
                    &[],
                ));
            }
            let mut fields = vec![
                ("field", JsValue::from_str(field)),
                ("unit", num(unit as u64)),
            ];
            if let Some(i) = index {
                fields.push(("index", num(i as u64)));
            }
            return Err(error(
                "EENCODING",
                "string is not well-formed UTF-16 (unpaired surrogate); refused, not replaced",
                &fields,
            ));
        }
        if !claimed {
            return Err(error(
                "EENGINE",
                "String.prototype.isWellFormed reported a well-formed string as malformed; refusing to convert text",
                &[],
            ));
        }
    }
    #[cfg(feature = "negative-control-lossy-utf16")]
    let _ = (field, index);
    Ok(text)
}

/// A non-negative integer from a JS number. Values above 2^64 saturate, which
/// is sound for text-bound offsets: they are refused by the bounds check.
fn offset(v: f64) -> Result<u64, JsValue> {
    if !v.is_finite() || v < 0.0 || v.fract() != 0.0 {
        return Err(argument("offset must be a non-negative integer"));
    }
    Ok(v as u64)
}

/// A text-free offset: exact only up to `Number.MAX_SAFE_INTEGER`.
fn safe_offset(v: f64) -> Result<u64, JsValue> {
    let o = offset(v)?;
    if o > MAX_SAFE {
        return Err(precision());
    }
    Ok(o)
}

fn precision() -> JsValue {
    error(
        "EOFFSETPRECISION",
        "offset is above Number.MAX_SAFE_INTEGER and cannot be represented exactly",
        &[],
    )
}

fn safe_result(v: u64) -> Result<f64, JsValue> {
    if v > MAX_SAFE {
        return Err(precision());
    }
    Ok(v as f64)
}

fn edits(froms: &[f64], tos: &[f64], inserts: &Array, safe: bool) -> Result<Vec<Edit>, JsValue> {
    if froms.len() != tos.len() || froms.len() != inserts.length() as usize {
        return Err(argument("edit arrays differ in length"));
    }
    let pick = if safe { safe_offset } else { offset };
    let mut out = Vec::with_capacity(froms.len());
    for i in 0..froms.len() {
        let insert = inserts
            .get(i as u32)
            .dyn_into::<JsString>()
            .map_err(|_| argument("edit insert must be a string"))?;
        out.push(Edit {
            from: pick(froms[i])?,
            to: pick(tos[i])?,
            insert: take_text(&insert, "insert", Some(i))?,
        });
    }
    Ok(out)
}

fn affinity(after: bool) -> Affinity {
    if after {
        Affinity::After
    } else {
        Affinity::Before
    }
}

/// A serial or revision number. Anything not exactly representable can never
/// have been issued, so it is refused rather than rounded.
fn id(v: f64, on_bad: SessionError) -> Result<u64, JsValue> {
    if !v.is_finite() || v < 0.0 || v.fract() != 0.0 || v > MAX_SAFE as f64 {
        return Err(session_error(on_bad));
    }
    Ok(v as u64)
}

// ---------------------------------------------------------------------------
// stateless exports
// ---------------------------------------------------------------------------

/// Names of negative-control features compiled in. Empty for a release
/// artifact; the build script and the facade tests assert that.
const NEGATIVE_CONTROLS: &[(&str, bool)] = &[
    (
        "utf8-tiebreak",
        cfg!(feature = "negative-control-utf8-tiebreak"),
    ),
    (
        "no-ambiguity",
        cfg!(feature = "negative-control-no-ambiguity"),
    ),
    (
        "no-boundary",
        cfg!(feature = "negative-control-no-boundary"),
    ),
    (
        "lossy-utf16",
        cfg!(feature = "negative-control-lossy-utf16"),
    ),
];

#[wasm_bindgen]
pub fn engine_info() -> String {
    let neg: Vec<&str> = NEGATIVE_CONTROLS
        .iter()
        .filter(|(_, on)| *on)
        .map(|(n, _)| *n)
        .collect();
    format!(
        "wrlforge-wasm {} (RUST-1; wrlforge-text; offsets=utf16; negative-controls=[{}])",
        env!("CARGO_PKG_VERSION"),
        neg.join(",")
    )
}

/// `Ok` when the string is well-formed UTF-16; `EENCODING` with `unit` otherwise.
#[wasm_bindgen]
pub fn check_text(text: &JsString) -> Result<(), JsValue> {
    take_text(text, "text", None).map(|_| ())
}

#[wasm_bindgen]
pub fn apply_edits(
    text: &JsString,
    froms: &[f64],
    tos: &[f64],
    inserts: &Array,
) -> Result<String, JsValue> {
    let text = take_text(text, "text", None)?;
    let edits = edits(froms, tos, inserts, false)?;
    wrlforge_text::apply_edits(&text, &edits).map_err(edit_error)
}

#[wasm_bindgen]
pub fn map_offset(
    offset: f64,
    froms: &[f64],
    tos: &[f64],
    inserts: &Array,
    after: bool,
) -> Result<f64, JsValue> {
    let o = safe_offset(offset)?;
    let edits = edits(froms, tos, inserts, true)?;
    let r = wrlforge_text::map_offset(o, &edits, affinity(after)).map_err(edit_error)?;
    safe_result(r)
}

#[wasm_bindgen]
pub fn map_range(
    from: f64,
    to: f64,
    froms: &[f64],
    tos: &[f64],
    inserts: &Array,
    start_after: bool,
    end_after: bool,
) -> Result<Box<[f64]>, JsValue> {
    let (f, t) = (safe_offset(from)?, safe_offset(to)?);
    let edits = edits(froms, tos, inserts, true)?;
    let (a, b) = wrlforge_text::map_range(f, t, &edits, affinity(start_after), affinity(end_after))
        .map_err(edit_error)?;
    Ok(Box::new([safe_result(a)?, safe_result(b)?]))
}

/// MEASUREMENT PROBE ONLY (RUST_1_PERFORMANCE.md): validation through
/// `JsString::is_valid_utf16`, which calls `charCodeAt` once per code unit.
/// Not used by any gate and not exposed by the facade.
#[wasm_bindgen]
pub fn probe_is_valid_utf16_by_units(text: &JsString) -> bool {
    text.is_valid_utf16()
}

// ---------------------------------------------------------------------------
// session
// ---------------------------------------------------------------------------

/// One isolated text session. The facade never hands this object to callers.
#[wasm_bindgen]
pub struct RawSession {
    core: SessionCore,
}

impl RawSession {
    fn snap(&self, serial: f64, revision: f64) -> Result<&wrlforge_text::TextSnapshot, JsValue> {
        let s = id(serial, SessionError::Foreign)?;
        let r = id(revision, SessionError::InvalidRevision)?;
        self.core.check(s, r).map_err(session_error)
    }
}

#[wasm_bindgen]
impl RawSession {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &JsString) -> Result<RawSession, JsValue> {
        let text = take_text(text, "text", None)?;
        let core = COUNTER
            .with(|c| SessionCore::open(c, text))
            .map_err(session_error)?;
        Ok(RawSession { core })
    }

    pub fn serial(&self) -> f64 {
        self.core.serial() as f64
    }

    pub fn revision(&self) -> Result<f64, JsValue> {
        self.core
            .revision()
            .map(|r| r as f64)
            .map_err(session_error)
    }

    /// The JavaScript owner reports its new exact text. Returns the new revision.
    pub fn replace(&mut self, serial: f64, revision: f64, text: &JsString) -> Result<f64, JsValue> {
        let s = id(serial, SessionError::Foreign)?;
        let r = id(revision, SessionError::InvalidRevision)?;
        // Prove the base first, then validate the text, then change state.
        self.core.check(s, r).map_err(session_error)?;
        let text = take_text(text, "text", None)?;
        COUNTER
            .with(|c| self.core.replace(c, s, r, text))
            .map(|n| n as f64)
            .map_err(session_error)
    }

    pub fn text(&self, serial: f64, revision: f64) -> Result<String, JsValue> {
        Ok(self.snap(serial, revision)?.text().to_string())
    }

    pub fn length(&self, serial: f64, revision: f64) -> Result<f64, JsValue> {
        Ok(self.snap(serial, revision)?.utf16_len() as f64)
    }

    pub fn to_utf8(&self, serial: f64, revision: f64, offset_: f64) -> Result<f64, JsValue> {
        let snap = self.snap(serial, revision)?;
        let o = offset(offset_)?;
        snap.utf16_to_utf8(o)
            .map(|b| b as f64)
            .map_err(offset_error)
    }

    pub fn from_utf8(&self, serial: f64, revision: f64, byte: f64) -> Result<f64, JsValue> {
        let snap = self.snap(serial, revision)?;
        let b = offset(byte)?;
        let b = usize::try_from(b).unwrap_or(usize::MAX);
        snap.utf8_to_utf16(b)
            .map(|u| u as f64)
            .map_err(offset_error)
    }

    pub fn validate_span(
        &self,
        serial: f64,
        revision: f64,
        from: f64,
        to: f64,
    ) -> Result<Box<[f64]>, JsValue> {
        let snap = self.snap(serial, revision)?;
        let (f, t) = (offset(from)?, offset(to)?);
        match snap.validate_span(f, t) {
            Ok(s) => Ok(Box::new([
                s.from as f64,
                s.to as f64,
                s.byte_from as f64,
                s.byte_to as f64,
            ])),
            Err(SpanError::Inverted) => Err(error(
                "ESPANINVERTED",
                "span.from is greater than span.to",
                &[],
            )),
            Err(SpanError::From(e)) | Err(SpanError::To(e)) => Err(offset_error(e)),
        }
    }

    pub fn line_col(
        &self,
        serial: f64,
        revision: f64,
        offset_: f64,
    ) -> Result<Box<[f64]>, JsValue> {
        let snap = self.snap(serial, revision)?;
        let o = offset(offset_)?;
        snap.line_col(o)
            .map(|lc| Box::new([lc.line as f64, lc.column as f64]) as Box<[f64]>)
            .map_err(offset_error)
    }

    pub fn offset_at(
        &self,
        serial: f64,
        revision: f64,
        line: f64,
        column: f64,
    ) -> Result<f64, JsValue> {
        let snap = self.snap(serial, revision)?;
        let (l, c) = (offset(line)?, offset(column)?);
        snap.offset_at(l, c).map(|o| o as f64).map_err(|e| match e {
            LineError::Line { line, lines } => error(
                "ELINE",
                "line is out of range",
                &[("line", num(line)), ("lines", num(lines))],
            ),
            LineError::Column { column, max } => error(
                "ECOLUMN",
                "column is out of range for this line",
                &[("column", num(column)), ("max", num(max))],
            ),
            LineError::InsideSurrogatePair { column } => error(
                "EOFFSETSURROGATE",
                "column is inside a surrogate pair",
                &[("column", num(column))],
            ),
        })
    }

    /// `[lineCount, lf, crlf, cr]`.
    pub fn line_info(&self, serial: f64, revision: f64) -> Result<Box<[f64]>, JsValue> {
        let snap = self.snap(serial, revision)?;
        let e = snap.line_endings();
        Ok(Box::new([
            snap.line_count() as f64,
            e.lf as f64,
            e.crlf as f64,
            e.cr as f64,
        ]))
    }

    /// A proposed next text. Does not change the session.
    pub fn propose(
        &self,
        serial: f64,
        revision: f64,
        froms: &[f64],
        tos: &[f64],
        inserts: &Array,
    ) -> Result<String, JsValue> {
        let snap = self.snap(serial, revision)?;
        let edits = edits(froms, tos, inserts, false)?;
        snap.apply_edits(&edits).map_err(edit_error)
    }

    /// Prove that `edits` turn the current text into exactly `after`, by FULL
    /// text comparison. No hash is computed or trusted. Returns the after length.
    pub fn verify(
        &self,
        serial: f64,
        revision: f64,
        froms: &[f64],
        tos: &[f64],
        inserts: &Array,
        after: &JsString,
    ) -> Result<f64, JsValue> {
        let snap = self.snap(serial, revision)?;
        let edits = edits(froms, tos, inserts, false)?;
        let after = take_text(after, "after", None)?;
        let produced = snap.apply_edits(&edits).map_err(edit_error)?;
        if produced == after {
            return Ok(wrlforge_text::offsets::utf16_len(&after) as f64);
        }
        let first = produced
            .encode_utf16()
            .zip(after.encode_utf16())
            .take_while(|(a, b)| a == b)
            .count();
        Err(error(
            "EVERIFYMISMATCH",
            "the edits do not produce the supplied text",
            &[("firstDivergence", num(first as u64))],
        ))
    }

    /// Release the text now. The facade calls `free()` right after.
    pub fn dispose(&mut self) {
        self.core.dispose();
    }
}
