// SPDX-License-Identifier: GPL-3.0-or-later
//! WRL Forge canonical document session (TAURI-RUST-MIGRATION-1).
//!
//! ONE exact source buffer is the document. Everything a UI shows is a derived
//! projection. This crate owns:
//!
//! * the canonical text, its revision counter and its dirty state;
//! * validated span edits (through `wrlforge-text`, the WD1.2 algebra);
//! * undo / redo as exact inverse span edits (never a re-print);
//! * the EDITOR VIEW mapping.
//!
//! ## The editor view
//!
//! A web `<textarea>` normalizes every line break to `\n` in its `value`. A
//! file with CRLF or lone-CR line endings therefore cannot be shown byte-exact
//! in one. Rather than letting the widget silently rewrite the document, the
//! view is an explicit projection: every CRLF, CR and LF in the source is ONE
//! `\n` in the view, and nothing else differs. Offsets the UI sends are view
//! UTF-16 offsets; this crate maps them to source offsets, and an inserted `\n`
//! becomes the document's dominant line ending. Untouched line endings are
//! never changed. A view offset can never land between a CR and its LF.
//!
//! Pure: no filesystem, no clock, no platform I/O.
#![forbid(unsafe_code)]

use wrlforge_text::{apply_edits, Edit, EditError, TextSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EolCounts {
    pub lf: u64,
    pub crlf: u64,
    pub cr: u64,
}

impl EolCounts {
    pub fn of(text: &str) -> Self {
        let b = text.as_bytes();
        let mut c = EolCounts::default();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\r' if b.get(i + 1) == Some(&b'\n') => {
                    c.crlf += 1;
                    i += 1;
                }
                b'\r' => c.cr += 1,
                b'\n' => c.lf += 1,
                _ => {}
            }
            i += 1;
        }
        c
    }
    /// The ending a NEW line break gets: the most frequent one (ties prefer
    /// LF, then CRLF). A document with no line breaks uses LF.
    pub fn dominant(&self) -> Eol {
        if self.crlf > self.lf && self.crlf >= self.cr {
            Eol::CrLf
        } else if self.cr > self.lf && self.cr > self.crlf {
            Eol::Cr
        } else {
            Eol::Lf
        }
    }
    pub fn mixed(&self) -> bool {
        [self.lf, self.crlf, self.cr]
            .iter()
            .filter(|&&n| n > 0)
            .count()
            > 1
    }
}

/// A view-coordinate edit from the UI: replace view UTF-16 `[from, to)` with
/// `insert` (whose line breaks are `\n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewEdit {
    pub from: u64,
    pub to: u64,
    pub insert: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    /// The UI's base revision is not the current revision.
    Stale {
        expected: u64,
        actual: u64,
    },
    /// A view offset is outside the view or inverted.
    BadViewRange {
        from: u64,
        to: u64,
        len: u64,
    },
    /// The insert text contains a CR (the view never does).
    CarriageReturnInInsert,
    /// The edit algebra refused the mapped source edit.
    Edit(String),
    Nothing,
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::Stale { expected, actual } => write!(
                f,
                "stale edit: based on revision {actual}, document is at {expected}"
            ),
            DocError::BadViewRange { from, to, len } => {
                write!(
                    f,
                    "view range [{from}, {to}) is invalid for view length {len}"
                )
            }
            DocError::CarriageReturnInInsert => write!(f, "inserted text may not contain CR"),
            DocError::Edit(m) => write!(f, "edit refused: {m}"),
            DocError::Nothing => write!(f, "nothing to undo/redo"),
        }
    }
}

impl From<EditError> for DocError {
    fn from(e: EditError) -> Self {
        DocError::Edit(format!("{:?}", e.code))
    }
}

/// One exact, invertible change in SOURCE UTF-16 coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    from: u64,
    removed: String,
    inserted: String,
}

fn utf16_len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

impl Change {
    fn forward(&self) -> Edit {
        Edit {
            from: self.from,
            to: self.from + utf16_len(&self.removed),
            insert: self.inserted.clone(),
        }
    }
    fn backward(&self) -> Edit {
        Edit {
            from: self.from,
            to: self.from + utf16_len(&self.inserted),
            insert: self.removed.clone(),
        }
    }
}

/// Undo groups: consecutive single-character typing at the advancing caret
/// coalesces into one step (broken at a space), like an editor would.
#[derive(Debug, Clone, Default)]
struct Group {
    changes: Vec<Change>,
    typing: bool,
}

#[derive(Debug)]
pub struct Document {
    text: String,
    revision: u64,
    /// The text last loaded from or saved to disk.
    saved: String,
    eol: Eol,
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// The exact changes the last change of ANY kind (edit, transaction,
    /// undo, redo) applied, in application order.
    last_changes: Vec<SpanChange>,
    /// The exact changes of the most recent revisions, oldest first:
    /// `(revision reached, changes applied to reach it)`. Bounded; a reset
    /// clears it. Lets a span be carried from an OLDER revision (a preview
    /// generation) to the current one without a search.
    log: std::collections::VecDeque<(u64, Vec<SpanChange>)>,
}

/// How many revisions `Document::changes_since` can reach back.
pub const CHANGE_LOG_CAP: usize = 64;

/// One applied change in SOURCE UTF-16 coordinates current at its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanChange {
    pub from: u64,
    pub removed: u64,
    pub inserted: u64,
}

/// Map a span `[from, to)` through exact changes. A change entirely before
/// the span shifts it; one strictly inside it moves its end; one after it
/// does nothing. A change that touches or crosses a boundary returns `None`:
/// the span can no longer be proven to be the same text, so it is LOST,
/// never guessed.
pub fn map_span(mut from: u64, mut to: u64, changes: &[SpanChange]) -> Option<(u64, u64)> {
    for c in changes {
        let end = c.from + c.removed;
        let delta = c.inserted as i64 - c.removed as i64;
        if end < from || (end == from && c.removed > 0 && c.from < from) {
            from = (from as i64 + delta) as u64;
            to = (to as i64 + delta) as u64;
        } else if c.from > to || (c.from == to && c.removed > 0) {
        } else if c.from > from && end < to {
            to = (to as i64 + delta) as u64;
        } else {
            return None;
        }
    }
    Some((from, to))
}

/// What the UI needs after any change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub revision: u64,
    pub dirty: bool,
    /// View UTF-16 length and FNV-1a hash of the view, so the UI can prove its
    /// widget still shows exactly the projection of the canonical text.
    pub view_len: u64,
    pub view_hash: u64,
    /// Caret in VIEW coordinates after the change (for undo/redo).
    pub caret: u64,
}

/// FNV-1a over UTF-16 code units. A drift detector, not a security hash.
pub fn fnv1a_utf16(view: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for u in view.encode_utf16() {
        h ^= u as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The view projection: every CRLF / CR / LF becomes `\n`.
pub fn view_of(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\r' {
            if it.peek() == Some(&'\n') {
                it.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

impl Document {
    pub fn new(text: String) -> Self {
        let eol = EolCounts::of(&text).dominant();
        Document {
            saved: text.clone(),
            text,
            revision: 0,
            eol,
            undo: vec![],
            redo: vec![],
            last_changes: vec![],
            log: Default::default(),
        }
    }

    /// Log the change that just produced `self.revision`.
    fn record(&mut self) {
        if self.log.len() == CHANGE_LOG_CAP {
            self.log.pop_front();
        }
        self.log
            .push_back((self.revision, self.last_changes.clone()));
    }

    /// The exact changes, in application order, that turned the text of
    /// `revision` into the current text; `Some(vec![])` for the current
    /// revision. `None` when `revision` is unknown, in the future, before a
    /// reset, or older than the bounded log: then nothing can be carried.
    pub fn changes_since(&self, revision: u64) -> Option<Vec<SpanChange>> {
        if revision == self.revision {
            return Some(vec![]);
        }
        if revision > self.revision {
            return None;
        }
        let first = self.log.iter().position(|(r, _)| *r == revision + 1)?;
        let mut out = Vec::new();
        let mut want = revision + 1;
        for (r, changes) in self.log.iter().skip(first) {
            if *r != want {
                return None;
            }
            out.extend_from_slice(changes);
            want += 1;
        }
        (want == self.revision + 1).then_some(out)
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
    pub fn eol(&self) -> Eol {
        self.eol
    }
    pub fn eol_counts(&self) -> EolCounts {
        EolCounts::of(&self.text)
    }
    pub fn view(&self) -> String {
        view_of(&self.text)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The text on disk now equals the buffer (after a verified save).
    pub fn mark_saved(&mut self) {
        self.saved = self.text.clone();
    }

    /// Replace the whole buffer from disk (Reload). Clears history; the
    /// revision keeps advancing so no stale UI edit can apply to the new text.
    pub fn reset(&mut self, text: String) {
        let revision = self.revision + 1;
        *self = Document::new(text);
        self.revision = revision;
    }

    fn applied(&self, caret_source: u64) -> Applied {
        let view = self.view();
        Applied {
            revision: self.revision,
            dirty: self.dirty(),
            view_len: utf16_len(&view),
            view_hash: fnv1a_utf16(&view),
            caret: self.source_to_view(caret_source),
        }
    }

    /// Map a VIEW UTF-16 offset to a SOURCE UTF-16 offset. A view `\n` that
    /// stands for a CRLF spans both source units. `None` when out of range.
    pub fn view_to_source(&self, view_offset: u64) -> Option<u64> {
        let b = self.text.as_bytes();
        let (mut v, mut s, mut i) = (0u64, 0u64, 0usize);
        while i < b.len() {
            if v == view_offset {
                return Some(s);
            }
            let c = self.text[i..].chars().next().unwrap();
            if c == '\r' && b.get(i + 1) == Some(&b'\n') {
                i += 2;
                s += 2;
                v += 1;
                continue;
            }
            i += c.len_utf8();
            let u = c.len_utf16() as u64;
            s += u;
            v += u;
        }
        (v == view_offset).then_some(s)
    }

    /// Map a SOURCE UTF-16 offset to the VIEW. An offset between a CR and its
    /// LF (not a legal view position) maps to the end of that line break.
    pub fn source_to_view(&self, source_offset: u64) -> u64 {
        let b = self.text.as_bytes();
        let (mut v, mut s, mut i) = (0u64, 0u64, 0usize);
        while i < b.len() && s < source_offset {
            let c = self.text[i..].chars().next().unwrap();
            if c == '\r' && b.get(i + 1) == Some(&b'\n') {
                i += 2;
                s += 2;
                v += 1;
                continue;
            }
            i += c.len_utf8();
            let u = c.len_utf16() as u64;
            s += u;
            v += u;
        }
        v
    }

    /// Apply one view edit made against `base_revision`.
    pub fn apply_view_edit(
        &mut self,
        base_revision: u64,
        e: &ViewEdit,
    ) -> Result<Applied, DocError> {
        if base_revision != self.revision {
            return Err(DocError::Stale {
                expected: self.revision,
                actual: base_revision,
            });
        }
        if e.insert.contains('\r') {
            return Err(DocError::CarriageReturnInInsert);
        }
        let bad = || DocError::BadViewRange {
            from: e.from,
            to: e.to,
            len: utf16_len(&view_of(&self.text)),
        };
        if e.from > e.to {
            return Err(bad());
        }
        let from = self.view_to_source(e.from).ok_or_else(bad)?;
        let to = self.view_to_source(e.to).ok_or_else(bad)?;
        let insert = match self.eol {
            Eol::Lf => e.insert.clone(),
            other => e.insert.replace('\n', other.as_str()),
        };
        self.apply_source_edit(from, to, insert, true)
    }

    /// Apply one source-coordinate edit, validated by the WD1.2 algebra in
    /// `wrlforge-text`. On any refusal the buffer and revision are unchanged.
    pub fn apply_source_edit(
        &mut self,
        from: u64,
        to: u64,
        insert: String,
        allow_coalesce: bool,
    ) -> Result<Applied, DocError> {
        let snap = TextSnapshot::new(self.text.clone());
        let span = snap
            .validate_span(from, to)
            .map_err(|e| DocError::Edit(format!("{e:?}")))?;
        let removed = snap.text()[span.byte_from..span.byte_to].to_string();
        if removed.is_empty() && insert.is_empty() {
            return Ok(self.applied(from));
        }
        let next = snap.apply_edits(&[Edit {
            from,
            to,
            insert: insert.clone(),
        }])?;
        self.text = next;
        self.revision += 1;
        self.redo.clear();
        let typing =
            removed.is_empty() && !insert.contains(['\n', '\r']) && insert.chars().count() == 1;
        let coalesce = allow_coalesce
            && typing
            && self.undo.last().is_some_and(|g| {
                g.typing
                    && g.changes.last().is_some_and(|c| {
                        c.from + utf16_len(&c.inserted) == from && !c.inserted.ends_with(' ')
                    })
            });
        let caret = from + utf16_len(&insert);
        self.last_changes = vec![SpanChange {
            from,
            removed: utf16_len(&removed),
            inserted: utf16_len(&insert),
        }];
        self.record();
        let change = Change {
            from,
            removed,
            inserted: insert,
        };
        if coalesce {
            self.undo.last_mut().unwrap().changes.push(change);
        } else {
            self.undo.push(Group {
                changes: vec![change],
                typing,
            });
        }
        Ok(self.applied(caret))
    }

    /// Apply a whole SOURCE-coordinate edit set (a semantic edit such as an
    /// Inspector field change) as ONE atomic, ONE-step-undoable transaction.
    ///
    /// `expected` is the text the caller's planner derived and verified. The
    /// set is re-validated and re-applied here by the WD1.2 algebra, and the
    /// result must equal `expected` byte for byte; otherwise nothing changes.
    pub fn apply_source_transaction(
        &mut self,
        base_revision: u64,
        edits: &[Edit],
        expected: &str,
    ) -> Result<Applied, DocError> {
        if base_revision != self.revision {
            return Err(DocError::Stale {
                expected: self.revision,
                actual: base_revision,
            });
        }
        if edits.is_empty() {
            return Err(DocError::Nothing);
        }
        let order = wrlforge_text::edit::validate_edits(&self.text, edits)?;
        let next = apply_edits(&self.text, edits)?;
        if next != expected {
            return Err(DocError::Edit(
                "transaction result differs from the planned text".into(),
            ));
        }
        // Record each change in the coordinates current when it is applied:
        // descending `from`, so no earlier change shifts a later one. Undo
        // replays the group backwards, redo forwards -- both exact.
        let snap = TextSnapshot::new(self.text.clone());
        let mut changes = Vec::with_capacity(edits.len());
        for &i in order.iter().rev() {
            let e = &edits[i];
            let span = snap
                .validate_span(e.from, e.to)
                .map_err(|x| DocError::Edit(format!("{x:?}")))?;
            changes.push(Change {
                from: e.from,
                removed: snap.text()[span.byte_from..span.byte_to].to_string(),
                inserted: e.insert.clone(),
            });
        }
        let caret = changes
            .first()
            .map(|c| c.from + utf16_len(&c.inserted))
            .unwrap_or(0);
        self.last_changes = changes
            .iter()
            .map(|c| SpanChange {
                from: c.from,
                removed: utf16_len(&c.removed),
                inserted: utf16_len(&c.inserted),
            })
            .collect();
        self.text = next;
        self.revision += 1;
        self.record();
        self.redo.clear();
        self.undo.push(Group {
            changes,
            typing: false,
        });
        Ok(self.applied(caret))
    }

    pub fn undo(&mut self) -> Result<Applied, DocError> {
        let g = self.undo.pop().ok_or(DocError::Nothing)?;
        let mut text = self.text.clone();
        let mut applied = Vec::with_capacity(g.changes.len());
        let mut caret = 0;
        for c in g.changes.iter().rev() {
            text = apply_edits(&text, &[c.backward()])?;
            applied.push(SpanChange {
                from: c.from,
                removed: utf16_len(&c.inserted),
                inserted: utf16_len(&c.removed),
            });
            caret = c.from + utf16_len(&c.removed);
        }
        self.last_changes = applied;
        self.text = text;
        self.revision += 1;
        self.record();
        self.redo.push(g);
        Ok(self.applied(caret))
    }

    pub fn redo(&mut self) -> Result<Applied, DocError> {
        let g = self.redo.pop().ok_or(DocError::Nothing)?;
        let mut text = self.text.clone();
        let mut applied = Vec::with_capacity(g.changes.len());
        let mut caret = 0;
        for c in g.changes.iter() {
            text = apply_edits(&text, &[c.forward()])?;
            applied.push(SpanChange {
                from: c.from,
                removed: utf16_len(&c.removed),
                inserted: utf16_len(&c.inserted),
            });
            caret = c.from + utf16_len(&c.inserted);
        }
        self.last_changes = applied;
        self.text = text;
        self.revision += 1;
        self.record();
        self.undo.push(g);
        Ok(self.applied(caret))
    }

    /// The exact changes the most recent change applied (edit, transaction,
    /// undo or redo), in application order, SOURCE coordinates.
    pub fn last_changes(&self) -> &[SpanChange] {
        &self.last_changes
    }

    /// The exact changes the most recent undo/redo applied (valid right
    /// after `undo` / `redo`).
    pub fn last_history_changes(&self) -> &[SpanChange] {
        &self.last_changes
    }

    /// Current state without a change (for resyncs).
    pub fn state(&self) -> Applied {
        self.applied(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ve(from: u64, to: u64, s: &str) -> ViewEdit {
        ViewEdit {
            from,
            to,
            insert: s.into(),
        }
    }

    fn se(from: u64, to: u64, s: &str) -> Edit {
        Edit {
            from,
            to,
            insert: s.into(),
        }
    }

    #[test]
    fn source_transaction_is_one_undo_step_and_exact() {
        let src = "\u{FEFF}#VRML V2.0 utf8\r\nT { t 1 2 3 }\rX é😀\n";
        let mut d = Document::new(src.into());
        let one = utf16_len("\u{FEFF}#VRML V2.0 utf8\r\nT { t ");
        let three = one + 4;
        let edits = [se(three, three + 1, "-3.5"), se(one, one + 1, "10")];
        let want = "\u{FEFF}#VRML V2.0 utf8\r\nT { t 10 2 -3.5 }\rX é😀\n";
        let a = d.apply_source_transaction(0, &edits, want).unwrap();
        assert_eq!(d.text(), want);
        assert_eq!(a.revision, 1);
        assert!(a.dirty && d.can_undo());
        d.undo().unwrap();
        assert_eq!(d.text(), src);
        assert!(!d.dirty());
        d.redo().unwrap();
        assert_eq!(d.text(), want);
    }

    #[test]
    fn changes_since_carries_spans_across_several_revisions_or_refuses() {
        let src = "A { x 1 }\nB { y 0 }";
        let mut d = Document::new(src.into());
        assert_eq!(d.changes_since(0), Some(vec![]));
        assert_eq!(d.changes_since(1), None, "a future revision");
        // rev 1: A's value grows; rev 2: undo; rev 3: redo.
        d.apply_source_transaction(0, &[se(6, 7, "1.25")], "A { x 1.25 }\nB { y 0 }")
            .unwrap();
        d.undo().unwrap();
        d.redo().unwrap();
        let b = (10u64, 19u64); // "B { y 0 }" in revision 0
        let all = d.changes_since(0).unwrap();
        assert_eq!(map_span(b.0, b.1, &all), Some((13, 22)));
        assert_eq!(&d.text()[13..22], "B { y 0 }");
        assert_eq!(
            map_span(b.0, b.1, &d.changes_since(2).unwrap()),
            Some((13, 22))
        );
        assert_eq!(
            map_span(13, 22, &d.changes_since(3).unwrap()),
            Some((13, 22))
        );
        // A reset forgets every older revision.
        d.reset("C {}".into());
        assert_eq!(d.changes_since(3), None);
        assert_eq!(d.changes_since(d.revision()), Some(vec![]));
        // The log is bounded: the oldest revisions fall out.
        let mut d = Document::new(String::new());
        for i in 0..(CHANGE_LOG_CAP as u64 + 3) {
            d.apply_source_edit(i, i, "x".into(), false).unwrap();
        }
        assert_eq!(d.changes_since(0), None);
        let last = d.revision() - CHANGE_LOG_CAP as u64;
        assert_eq!(d.changes_since(last).map(|c| c.len()), Some(CHANGE_LOG_CAP));
    }

    #[test]
    fn spans_map_through_exact_history_changes_or_are_lost() {
        let c = |from, removed, inserted| SpanChange {
            from,
            removed,
            inserted,
        };
        // inside -> end moves; before -> shift; after -> unchanged.
        assert_eq!(map_span(10, 20, &[c(12, 3, 5)]), Some((10, 22)));
        assert_eq!(map_span(10, 20, &[c(2, 3, 1)]), Some((8, 18)));
        assert_eq!(map_span(10, 20, &[c(25, 3, 1)]), Some((10, 20)));
        // touching or crossing a boundary -> lost.
        for ch in [
            c(10, 1, 1),
            c(19, 1, 1),
            c(5, 6, 1),
            c(15, 10, 0),
            c(20, 0, 3),
            c(10, 0, 3),
        ] {
            assert_eq!(map_span(10, 20, &[ch]), None, "{ch:?}");
        }
        // The real flow: an Inspector transaction, then undo/redo.
        let src = "A { x 1 2 3 }\nB { y 0 }";
        let mut d = Document::new(src.into());
        d.apply_source_transaction(0, &[se(8, 9, "2.5")], "A { x 1 2.5 3 }\nB { y 0 }")
            .unwrap();
        d.undo().unwrap();
        assert_eq!(map_span(0, 15, d.last_history_changes()), Some((0, 13)));
        assert_eq!(map_span(16, 25, d.last_history_changes()), Some((14, 23)));
        d.redo().unwrap();
        assert_eq!(map_span(0, 13, d.last_history_changes()), Some((0, 15)));
        // A typed edit and a transaction record their exact changes too.
        d.apply_source_edit(0, 0, "#\n".into(), true).unwrap();
        assert_eq!(d.last_changes(), &[c(0, 0, 2)]);
        assert_eq!(map_span(16, 25, d.last_changes()), Some((18, 27)));
        let r = d.revision();
        let t = d.text().replacen("y 0", "y 10", 1);
        d.apply_source_transaction(r, &[se(24, 25, "10")], &t)
            .unwrap();
        assert_eq!(d.last_changes(), &[c(24, 1, 2)]);
        assert_eq!(map_span(18, 27, d.last_changes()), Some((18, 28)));
    }

    #[test]
    fn source_transaction_refusals_change_nothing() {
        let mut d = Document::new("abc".into());
        assert!(matches!(
            d.apply_source_transaction(3, &[se(0, 1, "x")], "xbc"),
            Err(DocError::Stale { .. })
        ));
        assert!(d
            .apply_source_transaction(0, &[se(0, 1, "x")], "WRONG")
            .is_err());
        assert!(d
            .apply_source_transaction(0, &[se(0, 2, "x"), se(1, 3, "y")], "")
            .is_err());
        assert!(d.apply_source_transaction(0, &[], "abc").is_err());
        assert_eq!((d.text(), d.revision(), d.can_undo()), ("abc", 0, false));
    }

    #[test]
    fn view_normalizes_only_line_breaks() {
        assert_eq!(view_of("a\r\nb\rc\nd\u{FEFF}é😀"), "a\nb\nc\nd\u{FEFF}é😀");
    }

    #[test]
    fn crlf_document_keeps_untouched_endings_and_inserts_crlf() {
        let mut d = Document::new("\u{FEFF}#VRML V2.0 utf8\r\nGroup {}\r\n".into());
        let at = utf16_len("\u{FEFF}#VRML V2.0 utf8\n");
        let a = d.apply_view_edit(0, &ve(at, at, "# é😀\n")).unwrap();
        assert_eq!(d.text(), "\u{FEFF}#VRML V2.0 utf8\r\n# é😀\r\nGroup {}\r\n");
        assert_eq!(a.revision, 1);
        assert!(a.dirty);
        assert_eq!(a.view_hash, fnv1a_utf16(&d.view()));
    }

    #[test]
    fn mixed_and_lone_cr_endings_survive_an_unrelated_edit() {
        let src = "A\rB\r\nC\nD";
        let mut d = Document::new(src.into());
        d.apply_view_edit(0, &ve(4, 5, "Z")).unwrap();
        assert_eq!(d.text(), "A\rB\r\nZ\nD");
        d.undo().unwrap();
        assert_eq!(d.text(), src);
        assert!(!d.dirty());
    }

    #[test]
    fn deleting_a_view_newline_removes_the_whole_crlf() {
        let mut d = Document::new("a\r\nb".into());
        d.apply_view_edit(0, &ve(1, 2, "")).unwrap();
        assert_eq!(d.text(), "ab");
    }

    #[test]
    fn stale_out_of_range_and_cr_inserts_are_refused_without_change() {
        let mut d = Document::new("abc".into());
        assert!(matches!(
            d.apply_view_edit(5, &ve(0, 0, "x")),
            Err(DocError::Stale { .. })
        ));
        assert!(matches!(
            d.apply_view_edit(0, &ve(2, 9, "x")),
            Err(DocError::BadViewRange { .. })
        ));
        assert!(matches!(
            d.apply_view_edit(0, &ve(2, 1, "x")),
            Err(DocError::BadViewRange { .. })
        ));
        assert_eq!(
            d.apply_view_edit(0, &ve(0, 0, "\r")),
            Err(DocError::CarriageReturnInInsert)
        );
        assert_eq!(d.text(), "abc");
        assert_eq!(d.revision(), 0);
    }

    #[test]
    fn surrogate_pair_split_is_refused() {
        let mut d = Document::new("a😀b".into());
        assert!(matches!(
            d.apply_view_edit(0, &ve(2, 2, "x")),
            Err(DocError::BadViewRange { .. })
        ));
        // The same split in SOURCE coordinates is refused by the WD1.2 algebra.
        assert!(matches!(
            d.apply_source_edit(2, 2, "x".into(), false),
            Err(DocError::Edit(_))
        ));
        assert_eq!(d.text(), "a😀b");
        assert_eq!(d.revision(), 0);
    }

    #[test]
    fn typing_coalesces_and_undo_redo_are_exact() {
        let mut d = Document::new("x".into());
        for (i, c) in ["a", "b", "c"].iter().enumerate() {
            let at = 1 + i as u64;
            d.apply_view_edit(i as u64, &ve(at, at, c)).unwrap();
        }
        assert_eq!(d.text(), "xabc");
        let a = d.undo().unwrap();
        assert_eq!(d.text(), "x");
        assert_eq!(a.caret, 1);
        assert!(!a.dirty);
        d.redo().unwrap();
        assert_eq!(d.text(), "xabc");
        assert!(d.undo().is_ok());
        assert_eq!(d.undo(), Err(DocError::Nothing));
    }

    #[test]
    fn new_edit_clears_redo_and_reload_resets() {
        let mut d = Document::new("".into());
        d.apply_view_edit(0, &ve(0, 0, "a\n")).unwrap();
        d.undo().unwrap();
        d.apply_view_edit(2, &ve(0, 0, "b")).unwrap();
        assert!(!d.can_redo());
        d.reset("disk".into());
        assert_eq!(d.revision(), 4);
        assert!(!d.dirty() && !d.can_undo());
    }

    #[test]
    fn view_source_mapping_round_trips() {
        let d = Document::new("a\r\n😀\rb".into());
        let pairs = [(0, 0), (1, 1), (2, 3), (4, 5), (5, 6), (6, 7)];
        for (v, s) in pairs {
            assert_eq!(d.view_to_source(v), Some(s), "view {v}");
            assert_eq!(d.source_to_view(s), v, "source {s}");
        }
        assert_eq!(d.view_to_source(7), None);
        assert_eq!(d.source_to_view(2), 2);
    }

    #[test]
    fn eol_dominance() {
        assert_eq!(EolCounts::of("a\r\nb\r\nc\n").dominant(), Eol::CrLf);
        assert_eq!(EolCounts::of("a\rb").dominant(), Eol::Cr);
        assert_eq!(EolCounts::of("a\nb\r\n").dominant(), Eol::Lf);
        assert!(EolCounts::of("a\nb\r\n").mixed());
    }
}
