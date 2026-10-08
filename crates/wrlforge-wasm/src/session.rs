// SPDX-License-Identifier: GPL-3.0-or-later
//! Session state machine, kept free of JavaScript types so it is tested natively.
//!
//! A session holds ONE immutable snapshot of the text the JavaScript owner says
//! is current. It proves nothing by itself: the JavaScript facade proves
//! ownership by object identity first. The checks here are defence in depth
//! against a numeric value that the facade did not mint:
//!
//! * `serial` and `revision` come from one per-instance monotonic counter that
//!   is never reset, so no two sessions and no two revisions share a number.
//! * The counter refuses to pass `Number.MAX_SAFE_INTEGER`, so every number a
//!   JavaScript caller sees is exact.
//! * A disposed session releases its text and refuses every call.

use std::cell::Cell;
use std::rc::Rc;
use wrlforge_text::TextSnapshot;

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE: u64 = (1 << 53) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The serial is not this session's.
    Foreign,
    /// The revision is a valid revision of this session, but not the current one.
    Stale,
    /// The revision number was never issued by this session.
    InvalidRevision,
    Disposed,
    /// The counter would pass `Number.MAX_SAFE_INTEGER`.
    Exhausted,
}

impl SessionError {
    pub fn code(self) -> &'static str {
        match self {
            SessionError::Foreign => "ESESSIONFOREIGN",
            SessionError::Stale => "ESESSIONSTALE",
            SessionError::InvalidRevision => "ESESSIONREVISION",
            SessionError::Disposed => "ESESSIONDISPOSED",
            SessionError::Exhausted => "ESESSIONEXHAUSTED",
        }
    }
}

/// A monotonic counter that never wraps and never resets.
#[derive(Debug)]
pub struct Counter {
    next: Cell<u64>,
    limit: u64,
}

impl Counter {
    pub const fn new() -> Self {
        Counter {
            next: Cell::new(1),
            limit: MAX_SAFE,
        }
    }

    /// For tests only: start near the limit.
    #[cfg(test)]
    pub fn starting_at(next: u64, limit: u64) -> Self {
        Counter {
            next: Cell::new(next),
            limit,
        }
    }

    pub fn allocate(&self) -> Result<u64, SessionError> {
        let n = self.next.get();
        if n > self.limit {
            return Err(SessionError::Exhausted);
        }
        // `n <= limit < u64::MAX`, so this cannot wrap.
        self.next.set(n + 1);
        Ok(n)
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct SessionCore {
    serial: u64,
    /// Every revision this session has issued is in `first_revision..=revision`
    /// but not necessarily contiguous (the counter is shared); `issued` records
    /// them so an invalid number is told apart from a stale one.
    revision: u64,
    issued: Vec<u64>,
    snapshot: Option<Rc<TextSnapshot>>,
}

impl SessionCore {
    pub fn open(counter: &Counter, text: String) -> Result<Self, SessionError> {
        let serial = counter.allocate()?;
        let revision = counter.allocate()?;
        Ok(SessionCore {
            serial,
            revision,
            issued: vec![revision],
            snapshot: Some(Rc::new(TextSnapshot::new(text))),
        })
    }

    pub fn serial(&self) -> u64 {
        self.serial
    }

    pub fn revision(&self) -> Result<u64, SessionError> {
        self.alive()?;
        Ok(self.revision)
    }

    fn alive(&self) -> Result<&Rc<TextSnapshot>, SessionError> {
        self.snapshot.as_ref().ok_or(SessionError::Disposed)
    }

    /// The current snapshot, if and only if `serial`/`revision` name it.
    pub fn check(&self, serial: u64, revision: u64) -> Result<&TextSnapshot, SessionError> {
        let snap = self.alive()?;
        if serial != self.serial {
            return Err(SessionError::Foreign);
        }
        if revision == self.revision {
            return Ok(snap);
        }
        if self.issued.binary_search(&revision).is_ok() {
            return Err(SessionError::Stale);
        }
        Err(SessionError::InvalidRevision)
    }

    /// Record that the JavaScript owner's text is now `text`. The base must be
    /// current. Returns the new revision. The old snapshot is released.
    pub fn replace(
        &mut self,
        counter: &Counter,
        serial: u64,
        revision: u64,
        text: String,
    ) -> Result<u64, SessionError> {
        self.check(serial, revision)?;
        let next = counter.allocate()?;
        self.issued.push(next);
        self.revision = next;
        self.snapshot = Some(Rc::new(TextSnapshot::new(text)));
        Ok(next)
    }

    /// Release the text. Every later call refuses with `Disposed`.
    pub fn dispose(&mut self) {
        self.snapshot = None;
    }

    pub fn is_disposed(&self) -> bool {
        self.snapshot.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serials_and_revisions_are_unique_and_monotonic() {
        let c = Counter::new();
        let a = SessionCore::open(&c, "a".into()).unwrap();
        let b = SessionCore::open(&c, "a".into()).unwrap();
        let mut seen = vec![a.serial, a.revision, b.serial, b.revision];
        seen.dedup();
        assert_eq!(seen.len(), 4);
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn stale_foreign_invalid_and_disposed_fail_closed() {
        let c = Counter::new();
        let mut a = SessionCore::open(&c, "x".into()).unwrap();
        let b = SessionCore::open(&c, "x".into()).unwrap();
        let r0 = a.revision().unwrap();
        let r1 = a.replace(&c, a.serial(), r0, "y".into()).unwrap();
        assert_eq!(a.check(a.serial(), r1).unwrap().text(), "y");
        assert_eq!(a.check(a.serial(), r0).unwrap_err(), SessionError::Stale);
        assert_eq!(
            a.replace(&c, a.serial(), r0, "z".into()).unwrap_err(),
            SessionError::Stale
        );
        // b's serial and b's revision are not a's.
        assert_eq!(a.check(b.serial(), r1).unwrap_err(), SessionError::Foreign);
        assert_eq!(
            a.check(a.serial(), b.revision().unwrap()).unwrap_err(),
            SessionError::InvalidRevision
        );
        assert_eq!(
            a.check(a.serial(), 0).unwrap_err(),
            SessionError::InvalidRevision
        );
        assert_eq!(
            a.check(a.serial(), u64::MAX).unwrap_err(),
            SessionError::InvalidRevision
        );
        a.dispose();
        assert!(a.is_disposed());
        assert_eq!(a.check(a.serial(), r1).unwrap_err(), SessionError::Disposed);
        assert_eq!(a.revision().unwrap_err(), SessionError::Disposed);
    }

    #[test]
    fn counter_refuses_to_pass_max_safe_integer() {
        let c = Counter::starting_at(MAX_SAFE - 2, MAX_SAFE);
        let mut s = SessionCore::open(&c, "t".into()).unwrap(); // MAX-2, MAX-1
        let r = s.revision().unwrap();
        assert_eq!(r, MAX_SAFE - 1);
        let r2 = s.replace(&c, s.serial(), r, "u".into()).unwrap();
        assert_eq!(r2, MAX_SAFE);
        // Exhausted: refused, and the session is unchanged.
        assert_eq!(
            s.replace(&c, s.serial(), r2, "v".into()).unwrap_err(),
            SessionError::Exhausted
        );
        assert_eq!(s.check(s.serial(), r2).unwrap().text(), "u");
        assert_eq!(
            SessionCore::open(&c, "w".into()).unwrap_err(),
            SessionError::Exhausted
        );
    }
}
