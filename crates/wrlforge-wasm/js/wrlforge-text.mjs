// SPDX-License-Identifier: GPL-3.0-or-later
// WRL Forge RUST-1 facade over the wrlforge-wasm raw exports.
//
// This is the ONLY surface a JavaScript caller should touch. It:
//   * reproduces src/vrml/edit.js argument-shape checks (same codes, same
//     caller indexes, same check order) so errors stay compatible;
//   * never hands out a wasm-bindgen object (those expose `__wbg_ptr`);
//   * proves a session or snapshot by MODULE-PRIVATE WeakMap membership, never
//     by a number -- revision numbers are informational only;
//   * refuses stale, disposed and foreign handles before any raw call;
//   * poisons the engine after any uncoded failure (a wasm trap), so a damaged
//     instance cannot keep answering.
//
// Environment-neutral: the caller supplies an INITIALIZED wasm-bindgen module
// namespace (see spikes/rust-1-wasm-boundary/loaders/). No fs, no fetch here.
//
// No production module imports this file (RUST-1 is isolated).

const AFFINITIES = new Set(['before', 'after']);
const EDIT_KEYS = ['from', 'to', 'insert'];
const RAW_EXPORTS = ['RawSession', 'apply_edits', 'map_offset', 'map_range', 'check_text', 'engine_info'];

function fail(code, message, extra) {
  const err = new Error(message);
  err.code = code;
  if (extra) {
    for (const key of Object.keys(extra)) {
      if (extra[key] !== undefined) err[key] = extra[key];
    }
  }
  return err;
}

// Copy accessors as accessors (Object.assign would invoke the getters).
const define = (proto, members) => Object.defineProperties(proto, Object.getOwnPropertyDescriptors(members));

const isPlainish = (v) => !!v && typeof v === 'object' && !Array.isArray(v);

// --- src/vrml/edit.js-compatible shape checks (independently written) -------

function checkOffset(label, value, index) {
  if (!Number.isInteger(value)) {
    throw fail('EEDITSHAPE', `${label} must be an integer offset`, { index, field: label, value });
  }
  if (value < 0) {
    throw fail('EEDITSHAPE', `${label} must not be negative`, { index, field: label, value });
  }
}

function checkAffinity(affinity, label) {
  if (!AFFINITIES.has(affinity)) {
    throw fail('EEDITAFFINITY', `${label || 'affinity'} must be 'before' or 'after'`, { value: affinity });
  }
}

// Shape-check an edit list in caller order and pack it for the raw layer.
function packEdits(edits) {
  if (!Array.isArray(edits)) {
    throw fail('EEDITSHAPE', 'edits must be an array', { value: edits });
  }
  const n = edits.length;
  const froms = new Float64Array(n);
  const tos = new Float64Array(n);
  const inserts = new Array(n);
  for (let index = 0; index < n; index += 1) {
    const value = edits[index];
    if (!isPlainish(value)) {
      throw fail('EEDITSHAPE', 'an edit must be an object {from, to, insert}', { index, value });
    }
    const unknown = Object.keys(value).filter((k) => !EDIT_KEYS.includes(k));
    if (unknown.length) {
      throw fail('EEDITSHAPE', `unexpected edit key(s): ${unknown.join(', ')}`, { index, unknown });
    }
    const { from, to, insert } = value;
    checkOffset('edit.from', from, index);
    checkOffset('edit.to', to, index);
    if (from > to) throw fail('EEDITSHAPE', 'edit.from must not be greater than edit.to', { index, from, to });
    if (typeof insert !== 'string') {
      throw fail('EEDITSHAPE', 'edit.insert must be a string', { index, value: insert });
    }
    froms[index] = from;
    tos[index] = to;
    inserts[index] = insert;
  }
  return { froms, tos, inserts };
}

function toRange(range, label) {
  const what = label || 'range';
  if (!isPlainish(range)) throw fail('EEDITRANGE', `${what} must be {from, to} or {start:{offset}, end:{offset}}`);
  let from;
  let to;
  if (isPlainish(range.start) && isPlainish(range.end)) {
    from = range.start.offset;
    to = range.end.offset;
  } else if ('from' in range || 'to' in range) {
    from = range.from;
    to = range.to;
  } else {
    throw fail('EEDITRANGE', `${what} has no recognised offsets`);
  }
  if (!Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < 0) {
    throw fail('EEDITRANGE', `${what} offsets must be non-negative integers`, { from, to });
  }
  if (from > to) throw fail('EEDITRANGE', `${what}.from must not be greater than ${what}.to`, { from, to });
  return { from, to };
}

// Text-free mapping is exact only within Number.MAX_SAFE_INTEGER. Registry id
// RUST1-PRECISION (proposed, pending owner approval): refuse, never round.
function checkPrecision(values) {
  for (const v of values) {
    if (!Number.isSafeInteger(v)) {
      throw fail('EOFFSETPRECISION', 'offset is above Number.MAX_SAFE_INTEGER and cannot be mapped exactly');
    }
  }
}

function checkPosition(value, label) {
  if (!Number.isInteger(value) || value < 0) {
    throw fail('EARGUMENT', `${label} must be a non-negative integer`, { value });
  }
}

// --- engine -----------------------------------------------------------------

export function createTextEngine(glue) {
  for (const name of RAW_EXPORTS) {
    if (!glue || typeof glue[name] !== 'function') {
      throw fail('EENGINE', `wrlforge-wasm module is missing export ${name}; was it initialized?`);
    }
  }
  if (typeof String.prototype.isWellFormed !== 'function') {
    // The UTF-16 gate needs it. Refuse to start rather than convert unchecked.
    throw fail('EENGINE', 'String.prototype.isWellFormed is unavailable');
  }

  let poisoned = null;
  // Every raw call goes through here. A coded error is a refusal and passes
  // through unchanged. Anything else (a wasm trap) poisons the engine.
  function raw(fn) {
    if (poisoned) throw fail('EENGINE', 'engine is poisoned by an earlier failure', { cause: poisoned });
    try {
      return fn();
    } catch (e) {
      if (e && typeof e.code === 'string') throw e;
      poisoned = e;
      throw fail('EENGINE', `wrlforge-wasm failed: ${e && e.message}`, { cause: e });
    }
  }

  const sessions = new WeakMap(); // TextSession -> { raw, serial, revision, disposed }
  const snapshots = new WeakMap(); // Snapshot -> { session, revision }

  function sessionRecord(session) {
    const rec = sessions.get(session);
    if (!rec) throw fail('ESESSIONFOREIGN', 'not a session of this engine');
    if (rec.disposed) throw fail('ESESSIONDISPOSED', 'session is disposed');
    return rec;
  }

  // Resolve a snapshot for use, optionally requiring it to belong to `session`.
  function snapshotRecord(snapshot, session) {
    const snap = snapshots.get(snapshot);
    if (!snap) throw fail('ESESSIONFOREIGN', 'not a snapshot of this engine');
    if (session !== undefined && snap.session !== session) {
      throw fail('ESESSIONFOREIGN', 'snapshot belongs to a different session');
    }
    const rec = sessionRecord(snap.session);
    if (snap.revision !== rec.revision) {
      throw fail('ESESSIONSTALE', 'snapshot is not the current revision', {
        revision: snap.revision, current: rec.revision,
      });
    }
    return rec;
  }

  function mintSnapshot(session, revision) {
    const snapshot = Object.create(Snapshot.prototype);
    snapshots.set(snapshot, { session, revision });
    return Object.freeze(snapshot);
  }

  function Snapshot() { throw fail('EARGUMENT', 'snapshots are created by a session'); }
  define(Snapshot.prototype, {
    get revision() { return snapshotRecordLoose(this).revision; },
    get length() { const r = snapshotRecord(this); return raw(() => r.raw.length(r.serial, r.revision)); },
    text() { const r = snapshotRecord(this); return raw(() => r.raw.text(r.serial, r.revision)); },
    toUtf8(offset) {
      const r = snapshotRecord(this);
      checkPosition(offset, 'offset');
      return raw(() => r.raw.to_utf8(r.serial, r.revision, offset));
    },
    fromUtf8(byte) {
      const r = snapshotRecord(this);
      checkPosition(byte, 'byte');
      return raw(() => r.raw.from_utf8(r.serial, r.revision, byte));
    },
    validateSpan(from, to) {
      const r = snapshotRecord(this);
      checkPosition(from, 'from');
      checkPosition(to, 'to');
      const [f, t, bf, bt] = raw(() => r.raw.validate_span(r.serial, r.revision, from, to));
      return Object.freeze({ from: f, to: t, utf8From: bf, utf8To: bt });
    },
    lineCol(offset) {
      const r = snapshotRecord(this);
      checkPosition(offset, 'offset');
      const [line, column] = raw(() => r.raw.line_col(r.serial, r.revision, offset));
      return Object.freeze({ line, column });
    },
    offsetAt(line, column) {
      const r = snapshotRecord(this);
      checkPosition(line, 'line');
      checkPosition(column, 'column');
      return raw(() => r.raw.offset_at(r.serial, r.revision, line, column));
    },
    lineInfo() {
      const r = snapshotRecord(this);
      const [lines, lf, crlf, cr] = raw(() => r.raw.line_info(r.serial, r.revision));
      return Object.freeze({ lines, lf, crlf, cr, mixed: [lf, crlf, cr].filter((n) => n > 0).length > 1 });
    },
  });
  // `revision` is readable on a stale snapshot (it is just a label).
  function snapshotRecordLoose(snapshot) {
    const snap = snapshots.get(snapshot);
    if (!snap) throw fail('ESESSIONFOREIGN', 'not a snapshot of this engine');
    return snap;
  }
  Object.freeze(Snapshot.prototype);

  function TextSession() { throw fail('EARGUMENT', 'use engine.openSession(text)'); }
  define(TextSession.prototype, {
    get disposed() {
      const rec = sessions.get(this);
      if (!rec) throw fail('ESESSIONFOREIGN', 'not a session of this engine');
      return rec.disposed;
    },
    current() {
      const rec = sessionRecord(this);
      return mintSnapshot(this, rec.revision);
    },
    // The JavaScript owner reports its new exact text. `base` must be current.
    update(base, text) {
      const rec = snapshotRecord(base, this);
      if (typeof text !== 'string') throw fail('EARGUMENT', 'text must be a string');
      const next = raw(() => rec.raw.replace(rec.serial, rec.revision, text));
      rec.revision = next;
      return mintSnapshot(this, next);
    },
    // A proposed next text. Changes nothing.
    propose(base, edits) {
      const rec = snapshotRecord(base, this);
      const { froms, tos, inserts } = packEdits(edits);
      return raw(() => rec.raw.propose(rec.serial, rec.revision, froms, tos, inserts));
    },
    // Full-text proof that `edits` turn `base` into exactly `after`.
    verifyTransaction(base, edits, after) {
      const rec = snapshotRecord(base, this);
      const { froms, tos, inserts } = packEdits(edits);
      if (typeof after !== 'string') throw fail('EARGUMENT', 'after must be a string');
      const length = raw(() => rec.raw.verify(rec.serial, rec.revision, froms, tos, inserts, after));
      return Object.freeze({ verified: true, revision: rec.revision, length });
    },
    // Idempotent. Marks dead BEFORE freeing so no call can reach freed memory.
    dispose() {
      const rec = sessions.get(this);
      if (!rec) throw fail('ESESSIONFOREIGN', 'not a session of this engine');
      if (rec.disposed) return;
      rec.disposed = true;
      const handle = rec.raw;
      rec.raw = null;
      raw(() => { handle.dispose(); handle.free(); });
    },
  });
  Object.freeze(TextSession.prototype);

  const engine = {
    get info() { return raw(() => glue.engine_info()); },
    get poisoned() { return poisoned !== null; },

    // true, or throws EENCODING with `unit` = first unpaired surrogate.
    checkText(text) {
      if (typeof text !== 'string') throw fail('EARGUMENT', 'text must be a string');
      raw(() => glue.check_text(text));
      return true;
    },

    openSession(text) {
      if (typeof text !== 'string') throw fail('EARGUMENT', 'text must be a string');
      const handle = raw(() => new glue.RawSession(text));
      const session = Object.freeze(Object.create(TextSession.prototype));
      sessions.set(session, {
        raw: handle,
        serial: handle.serial(),
        revision: raw(() => handle.revision()),
        disposed: false,
      });
      return session;
    },

    applyEdits(text, edits) {
      if (typeof text !== 'string') throw fail('EEDITSHAPE', 'text must be a string', { value: text });
      const { froms, tos, inserts } = packEdits(edits);
      return raw(() => glue.apply_edits(text, froms, tos, inserts));
    },

    mapOffset(offset, edits, affinity = 'before') {
      checkOffset('offset', offset, null);
      checkAffinity(affinity);
      const { froms, tos, inserts } = packEdits(edits);
      checkPrecision([offset, ...froms, ...tos]);
      return raw(() => glue.map_offset(offset, froms, tos, inserts, affinity === 'after'));
    },

    mapRange(range, edits, options = {}) {
      const span = toRange(range);
      const startAffinity = options.startAffinity === undefined ? 'before' : options.startAffinity;
      const endAffinity = options.endAffinity === undefined ? 'after' : options.endAffinity;
      checkAffinity(startAffinity, 'options.startAffinity');
      checkAffinity(endAffinity, 'options.endAffinity');
      const { froms, tos, inserts } = packEdits(edits);
      checkPrecision([span.from, span.to, ...froms, ...tos]);
      const [from, to] = raw(() => glue.map_range(span.from, span.to, froms, tos, inserts,
        startAffinity === 'after', endAffinity === 'after'));
      return Object.freeze({ from, to });
    },
  };
  return Object.freeze(engine);
}
