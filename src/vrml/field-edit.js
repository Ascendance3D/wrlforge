'use strict';
// Typed field-value editing (Phase WD2-B).
//
// PURE and browser-safe: requires only sibling src/vrml modules -- the AST
// constants, the tokenizer and syntax parser (for the round-trip check), the
// WD1.2 span-patch algebra, the WD1.4 parse-session / verified-transaction /
// node-identity layer, and the WD1.3 node schema. No fs, no Electron, no
// CodeMirror, no DOM. It writes nothing anywhere: it turns "this explicitly
// authored field of this node, this typed value" into WD1.2 edit objects plus a
// verified receipt, and the caller decides whether to dispatch them.
//
// THE DOCUMENT IS THE TEXT (WD.md section 2). Nothing here serializes a node,
// a field, or a value tree. A changed SCALAR TOKEN is replaced by a new token;
// a changed COMPONENT of a vector/color/rotation is replaced by a new token in
// that component's own span; a changed string replaces only the string token.
// Every byte outside those spans -- comments between components, commas,
// unusual whitespace, CRLF, the spelling of untouched numbers, field order,
// vendor syntax elsewhere -- is never examined and never rewritten.
//
// FAIL CLOSED. A field is editable only when every fact below is PROVEN from the
// current parse and the generated schema; anything uncertain is read-only with
// a stable reason id, and nothing ever guesses a span:
//
//   * the node object belongs to this exact parse session, whose text is the
//     caller's current text (a stale session refuses);
//   * the parse saw the whole document (no node/depth cap) and reported no
//     syntax error -- parser recovery MOVES boundaries (WD.md section 8 rule 3),
//     so a damaged document can attribute a field to the wrong node;
//   * the node is complete, its type is a standard VRML97 node per the schema,
//     and no PROTO/EXTERNPROTO anywhere in the document declares that name;
//   * the field is authored exactly once on the node, is not `IS`-bound, is in
//     the node's VRML97 interface (X3D-only fields refuse), is a stored field
//     (`field` / `exposedField`), and its schema type is one of the nine WD2-B
//     types;
//   * the authored value has exactly the AST shape that type requires, with
//     every number token lexically valid.
//
// Type authority is the schema, never the lexical shape: `translation 1 2 3` on
// an unknown node is not an SFVec3f, and `size 2 2 2` on a Box is one only
// because the schema says Box.size is SFVec3f.
//
// Constraints are the schema's `constraints` record, read exactly as its own
// header demands: numeric `min`/`max` bounds are enforced per component with
// their inclusive flags; symbolic bounds (`pi/2`, `infinity`) and `note`
// categories are NOT turned into numbers here, and a `null` record means "no
// machine-represented constraint" -- never "unrestricted", never a reason to
// clamp. Nothing is ever clamped. SFInt32's 32-bit range is the type itself
// (ISO/IEC 14772-1 5.6), not an invented constraint.

const { NODE, walk } = require('./ast');
const { tokenize, TT } = require('./tokenizer');
const { parse: parseSyntax } = require('./parser');
const edit = require('./edit');
const tx = require('./document-transaction');
const identity = require('./node-identity');
const schema = require('./node-schema');

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

const FIELD_EDIT_STATUS = Object.freeze({
  EDITABLE: 'editable',
  READ_ONLY: 'read-only',
});

const PLAN_STATUS = Object.freeze({
  READY: 'ready',
  UNCHANGED: 'unchanged',
  REFUSED: 'refused',
});

// Stable reason ids. Tests and the Inspector key off these strings; never
// change an existing value.
const FIELD_EDIT_REASON = Object.freeze({
  OK: 'ok',
  // --- the selection / document ------------------------------------------
  NOT_A_NODE: 'not-a-node-instance',
  NODE_NOT_IN_SESSION: 'node-not-in-parse-session',
  STALE_SESSION: 'parse-session-is-stale',
  PARSE_INCOMPLETE: 'document-parse-incomplete',
  SYNTAX_ERRORS: 'document-has-syntax-errors',
  NODE_INCOMPLETE: 'node-incomplete',
  UNKNOWN_NODE_TYPE: 'node-type-not-standard-vrml97',
  PROTO_INSTANCE: 'node-type-is-a-proto-name',
  // --- the field ----------------------------------------------------------
  FIELD_NOT_PRESENT: 'field-not-explicitly-authored',
  FIELD_DUPLICATED: 'field-authored-more-than-once',
  FIELD_UNKNOWN: 'field-not-in-schema',
  FIELD_X3D_ONLY: 'field-is-x3d-only',
  FIELD_NOT_STORED: 'field-is-not-a-stored-field',
  IS_BOUND: 'field-is-is-bound',
  TYPE_UNSUPPORTED: 'field-type-not-editable-in-wd2b',
  VALUE_MISSING: 'field-value-missing',
  VALUE_SHAPE: 'field-value-shape-does-not-match-type',
  VALUE_TOKEN_INVALID: 'field-value-token-invalid',
  // --- the user's input ---------------------------------------------------
  INPUT_SHAPE: 'input-shape-invalid',
  INPUT_NOT_NUMBER: 'input-not-a-number',
  INPUT_NOT_FINITE: 'input-not-finite',
  INPUT_NOT_INTEGER: 'input-not-an-integer',
  INPUT_OUT_OF_RANGE: 'input-out-of-range',
  INPUT_NOT_BOOLEAN: 'input-not-a-boolean',
  INPUT_NOT_STRING: 'input-not-a-string',
  INPUT_STRING_UNENCODABLE: 'input-string-not-round-trippable',
  // --- the transaction ----------------------------------------------------
  TRANSACTION_REJECTED: 'transaction-rejected',
  ROUND_TRIP_FAILED: 'round-trip-verification-failed',
});

// The WD2-B editable types and how each is shaped. `labels` are the concise
// component labels the Inspector shows; `kind` picks the control family.
const TYPE_SPECS = Object.freeze({
  SFBool: Object.freeze({ kind: 'bool', arity: 1, labels: Object.freeze(['Value']) }),
  SFInt32: Object.freeze({ kind: 'number', number: 'int32', arity: 1, labels: Object.freeze(['Value']) }),
  SFFloat: Object.freeze({ kind: 'number', number: 'float', arity: 1, labels: Object.freeze(['Value']) }),
  SFTime: Object.freeze({ kind: 'number', number: 'float', arity: 1, labels: Object.freeze(['Value']) }),
  SFVec2f: Object.freeze({ kind: 'number', number: 'float', arity: 2, labels: Object.freeze(['X', 'Y']) }),
  SFVec3f: Object.freeze({ kind: 'number', number: 'float', arity: 3, labels: Object.freeze(['X', 'Y', 'Z']) }),
  SFColor: Object.freeze({ kind: 'number', number: 'float', arity: 3, labels: Object.freeze(['R', 'G', 'B']) }),
  SFRotation: Object.freeze({ kind: 'number', number: 'float', arity: 4, labels: Object.freeze(['X', 'Y', 'Z', 'Angle']) }),
  SFString: Object.freeze({ kind: 'string', arity: 1, labels: Object.freeze(['Value']) }),
});

const EDITABLE_TYPES = Object.freeze(Object.keys(TYPE_SPECS));

// Stored-value declaration categories. eventIn/eventOut carry no stored value.
const STORED_DECLARATIONS = new Set(['field', 'exposedField']);

// Syntax diagnostics that do NOT move a structural boundary: the header line
// alone. Every other syntax ERROR blocks editing for the whole document.
const NON_STRUCTURAL_CODES = new Set(['VRML001', 'VRML002']);

const INT32_MIN = -2147483648;
const INT32_MAX = 2147483647;

// VRML97 Annex A lexical shapes (A.3 `float` / `int32`), checked BEFORE the
// tokenizer so text the tokenizer would leniently swallow is rejected here.
const FLOAT_RE = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?$/;
const INT32_RE = /^[+-]?(?:0[xX][0-9a-fA-F]+|\d+)$/;

// How long a read-only value excerpt may be before it is elided.
const VALUE_EXCERPT_MAX = 120;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

const offsetsOf = (range) => (range && range.start && range.end
  && Number.isInteger(range.start.offset) && Number.isInteger(range.end.offset)
  ? { from: range.start.offset, to: range.end.offset }
  : null);

const refuse = (reason, extra) => Object.freeze({ status: PLAN_STATUS.REFUSED, reason, ...(extra || {}) });

function syntaxDiagnosticsOf(parseResult) {
  if (Array.isArray(parseResult.syntaxDiagnostics)) return parseResult.syntaxDiagnostics;
  return Array.isArray(parseResult.diagnostics) ? parseResult.diagnostics : [];
}

function hasBlockingSyntaxError(parseResult) {
  return syntaxDiagnosticsOf(parseResult).some((d) => d && d.severity === 'error'
    && !NON_STRUCTURAL_CODES.has(d.code));
}

// Every PROTO/EXTERNPROTO name declared anywhere in the parse, cached against
// the session. Conservative on purpose: a node type matching ANY declared
// prototype name is treated as a prototype instance, never as the built-in
// node of the same spelling.
const PROTO_NAMES = new WeakMap();
function protoNamesOf(session) {
  let names = PROTO_NAMES.get(session);
  if (!names) {
    names = new Set();
    walk(session.parse.tree, (n) => {
      if ((n.type === NODE.PROTO || n.type === NODE.EXTERNPROTO) && typeof n.name === 'string') names.add(n.name);
    });
    PROTO_NAMES.set(session, names);
  }
  return names;
}

function excerpt(text, range) {
  const span = offsetsOf(range);
  if (!span || typeof text !== 'string' || span.to > text.length) return '';
  const raw = text.slice(span.from, span.to);
  return raw.length > VALUE_EXCERPT_MAX ? `${raw.slice(0, VALUE_EXCERPT_MAX)}…` : raw;
}

// The numeric bounds the schema actually represents as numbers, or null.
function numericBounds(constraints) {
  if (!constraints) return null;
  const hasMin = typeof constraints.min === 'number' && Number.isFinite(constraints.min);
  const hasMax = typeof constraints.max === 'number' && Number.isFinite(constraints.max);
  if (!hasMin && !hasMax) return null;
  return Object.freeze({
    min: hasMin ? constraints.min : null,
    minInclusive: hasMin ? constraints.minInclusive === true : null,
    max: hasMax ? constraints.max : null,
    maxInclusive: hasMax ? constraints.maxInclusive === true : null,
  });
}

function boundsText(bounds) {
  const parts = [];
  if (bounds.min !== null) parts.push(`${bounds.minInclusive ? '≥' : '>'} ${bounds.min}`);
  if (bounds.max !== null) parts.push(`${bounds.maxInclusive ? '≤' : '<'} ${bounds.max}`);
  return parts.join(' and ');
}

// ---------------------------------------------------------------------------
// The node-level gate
// ---------------------------------------------------------------------------

// Returns null when the node may carry editable fields, else a reason id.
// Throws only for programming errors (a non-session).
function nodeGate(session, node, currentText) {
  tx.assertParseSession(session, 'field-edit: session');
  if (!node || typeof node !== 'object' || node.type !== NODE.NODE) return FIELD_EDIT_REASON.NOT_A_NODE;
  try {
    // Object-identity membership in THIS parse; throws for a node from another.
    identity.createCurrentSelection(session, node);
  } catch (err) {
    if (err && err.code === identity.IDENTITY_ERROR.NODE) return FIELD_EDIT_REASON.NODE_NOT_IN_SESSION;
    throw err;
  }
  if (currentText !== undefined && currentText !== session.text) return FIELD_EDIT_REASON.STALE_SESSION;
  const p = session.parse;
  if (p.truncated || p.depthCapped) return FIELD_EDIT_REASON.PARSE_INCOMPLETE;
  if (hasBlockingSyntaxError(p)) return FIELD_EDIT_REASON.SYNTAX_ERRORS;
  if (node.incomplete) return FIELD_EDIT_REASON.NODE_INCOMPLETE;
  if (typeof node.nodeType !== 'string' || !schema.isVRML97Node(node.nodeType)) return FIELD_EDIT_REASON.UNKNOWN_NODE_TYPE;
  if (protoNamesOf(session).has(node.nodeType)) return FIELD_EDIT_REASON.PROTO_INSTANCE;
  return null;
}

// ---------------------------------------------------------------------------
// The field-level gate + descriptor
// ---------------------------------------------------------------------------

// Every FIELD element of a node, in source order, with its index into
// node.fields (the parser puts body ROUTE/PROTO/EXTERNPROTO there too -- WD.md
// section 8 rule 1 -- so dispatch is on `type`).
function authoredFields(node) {
  const out = [];
  const list = Array.isArray(node.fields) ? node.fields : [];
  for (let i = 0; i < list.length; i += 1) {
    const f = list[i];
    if (f && f.type === NODE.FIELD) out.push({ index: i, field: f });
  }
  return out;
}

// Component tokens for a supported value, or a reason id.
function componentsOf(spec, value) {
  if (!value) return { reason: FIELD_EDIT_REASON.VALUE_MISSING };
  if (spec.kind === 'bool') {
    if (value.type !== NODE.BOOL || !offsetsOf(value.range)) return { reason: FIELD_EDIT_REASON.VALUE_SHAPE };
    return { components: [{ token: value, text: value.value ? 'TRUE' : 'FALSE', value: value.value }] };
  }
  if (spec.kind === 'string') {
    if (value.type !== NODE.STRING || !offsetsOf(value.range)) return { reason: FIELD_EDIT_REASON.VALUE_SHAPE };
    const raw = typeof value.raw === 'string' ? value.raw : '';
    if (raw.length < 2 || raw[0] !== '"' || raw[raw.length - 1] !== '"') return { reason: FIELD_EDIT_REASON.VALUE_TOKEN_INVALID };
    return { components: [{ token: value, text: value.value, value: value.value }] };
  }
  if (value.type !== NODE.NUMBERS || !Array.isArray(value.values)) return { reason: FIELD_EDIT_REASON.VALUE_SHAPE };
  if (value.values.length !== spec.arity) return { reason: FIELD_EDIT_REASON.VALUE_SHAPE };
  const allowed = spec.number === 'int32' ? ['int', 'hex'] : ['int', 'float'];
  const components = [];
  for (const n of value.values) {
    if (!n || n.type !== NODE.NUMBER || !offsetsOf(n.range)) return { reason: FIELD_EDIT_REASON.VALUE_SHAPE };
    if (n.valid !== true || !Number.isFinite(n.value) || !allowed.includes(n.numeric)) {
      return { reason: FIELD_EDIT_REASON.VALUE_TOKEN_INVALID };
    }
    components.push({ token: n, text: n.lexeme, value: n.value });
  }
  return { components };
}

function describeField(session, node, entry, nodeReason, duplicateNames) {
  const f = entry.field;
  const name = typeof f.name === 'string' ? f.name : null;
  const record = name && typeof node.nodeType === 'string' ? schema.getFieldSchema(node.nodeType, name) : null;
  const vrml97 = !!record && record.profiles.includes('vrml97');
  const type = vrml97 ? record.type : null;
  const spec = type && Object.prototype.hasOwnProperty.call(TYPE_SPECS, type) ? TYPE_SPECS[type] : null;
  let reason = nodeReason;
  let shape = null;
  if (!reason) {
    if (!name) reason = FIELD_EDIT_REASON.FIELD_UNKNOWN;
    else if (duplicateNames.has(name)) reason = FIELD_EDIT_REASON.FIELD_DUPLICATED;
    else if (f.isBinding || (f.value && f.value.type === NODE.IS)) reason = FIELD_EDIT_REASON.IS_BOUND;
    else if (!record) reason = FIELD_EDIT_REASON.FIELD_UNKNOWN;
    else if (!vrml97) reason = FIELD_EDIT_REASON.FIELD_X3D_ONLY;
    else if (!STORED_DECLARATIONS.has(record.vrml97Declaration)) reason = FIELD_EDIT_REASON.FIELD_NOT_STORED;
    else if (!spec) reason = FIELD_EDIT_REASON.TYPE_UNSUPPORTED;
    else {
      shape = componentsOf(spec, f.value);
      if (shape.reason) reason = shape.reason;
    }
  }
  const editable = !reason;
  const components = editable
    ? shape.components.map((c, i) => Object.freeze({ label: spec.labels[i], text: c.text, value: c.value }))
    : [];
  return {
    descriptor: Object.freeze({
      index: entry.index,
      name,
      type,
      declaration: vrml97 ? record.vrml97Declaration : null,
      kind: spec ? spec.kind : null,
      status: editable ? FIELD_EDIT_STATUS.EDITABLE : FIELD_EDIT_STATUS.READ_ONLY,
      editable,
      reason: editable ? FIELD_EDIT_REASON.OK : reason,
      isBinding: !!f.isBinding,
      components: Object.freeze(components),
      bounds: editable && spec.kind === 'number' ? numericBounds(record.constraints) : null,
      constraintNote: vrml97 && record.constraints && record.constraints.note ? record.constraints.note.category : null,
      valueExcerpt: excerpt(session.text, f.value && f.value.range ? f.value.range : f.range),
    }),
    tokens: editable ? shape.components.map((c) => c.token) : null,
    spec,
    record,
  };
}

function duplicateNamesOf(entries) {
  const seen = new Set();
  const dup = new Set();
  for (const e of entries) {
    const n = e.field.name;
    if (typeof n !== 'string') continue;
    if (seen.has(n)) dup.add(n);
    seen.add(n);
  }
  return dup;
}

/**
 * Describe every explicitly authored field of one node for the Inspector.
 *
 * @param {object} session A parse session (createParseSession) of the current text.
 * @param {object} node An AST node from that session's parse.
 * @param {object} [options]
 * @param {string} [options.currentText] The editor's current text; a session
 *   whose text differs is stale and every field becomes read-only.
 * @returns {{status:string, reason:string, nodeType:string|null, fields:object[]}} Frozen.
 */
function inspectNodeFields(session, node, options = {}) {
  const nodeReason = nodeGate(session, node, options.currentText);
  if (!node || typeof node !== 'object' || node.type !== NODE.NODE || nodeReason === FIELD_EDIT_REASON.NODE_NOT_IN_SESSION) {
    return Object.freeze({
      status: FIELD_EDIT_STATUS.READ_ONLY,
      reason: nodeReason,
      nodeType: null,
      fields: Object.freeze([]),
    });
  }
  const entries = authoredFields(node);
  const duplicates = duplicateNamesOf(entries);
  const fields = entries.map((e) => describeField(session, node, e, nodeReason, duplicates).descriptor);
  return Object.freeze({
    status: nodeReason ? FIELD_EDIT_STATUS.READ_ONLY : FIELD_EDIT_STATUS.EDITABLE,
    reason: nodeReason || FIELD_EDIT_REASON.OK,
    nodeType: node.nodeType,
    fields: Object.freeze(fields),
  });
}

// ---------------------------------------------------------------------------
// Input validation + token encoding
// ---------------------------------------------------------------------------

// The candidate number token for one component, or a refusal.
function encodeNumber(spec, raw, bounds) {
  let text;
  if (typeof raw === 'number') text = String(raw);
  else if (typeof raw === 'string') text = raw.trim();
  else return { reason: FIELD_EDIT_REASON.INPUT_NOT_NUMBER, message: 'Enter a number.' };
  if (text === '') return { reason: FIELD_EDIT_REASON.INPUT_NOT_NUMBER, message: 'Enter a number.' };
  const isInt = spec.number === 'int32';
  if (!(isInt ? INT32_RE : FLOAT_RE).test(text)) {
    return isInt && FLOAT_RE.test(text)
      ? { reason: FIELD_EDIT_REASON.INPUT_NOT_INTEGER, message: 'Enter a whole number (SFInt32).' }
      : { reason: FIELD_EDIT_REASON.INPUT_NOT_NUMBER, message: `"${text}" is not a ${isInt ? 'whole number' : 'number'}.` };
  }
  // The tokenizer is the lexical authority: the candidate must be exactly one
  // valid NUMBER token that spells itself.
  const lexed = tokenize(text).tokens;
  const tok = lexed[0];
  if (lexed.length !== 2 || !tok || tok.type !== TT.NUMBER || tok.lexeme !== text || lexed[1].type !== TT.EOF) {
    return { reason: FIELD_EDIT_REASON.INPUT_NOT_NUMBER, message: `"${text}" is not a number.` };
  }
  if (!Number.isFinite(tok.value) || tok.valid !== true) {
    return { reason: FIELD_EDIT_REASON.INPUT_NOT_FINITE, message: `"${text}" is not a finite number.` };
  }
  if (isInt) {
    if (!Number.isInteger(tok.value)) return { reason: FIELD_EDIT_REASON.INPUT_NOT_INTEGER, message: 'Enter a whole number (SFInt32).' };
    if (tok.value < INT32_MIN || tok.value > INT32_MAX) {
      return { reason: FIELD_EDIT_REASON.INPUT_OUT_OF_RANGE, message: `SFInt32 must be between ${INT32_MIN} and ${INT32_MAX}.` };
    }
  }
  if (bounds) {
    const v = tok.value;
    const lowOk = bounds.min === null || (bounds.minInclusive ? v >= bounds.min : v > bounds.min);
    const highOk = bounds.max === null || (bounds.maxInclusive ? v <= bounds.max : v < bounds.max);
    if (!lowOk || !highOk) {
      return { reason: FIELD_EDIT_REASON.INPUT_OUT_OF_RANGE, message: `Value must be ${boundsText(bounds)}.` };
    }
  }
  return { text, value: tok.value };
}

/**
 * Encode a string as one quoted VRML97 string token.
 *
 * The tokenizer recognises exactly two escapes, `\"` and `\\`, so those are the
 * two characters escaped. A carriage return is refused: the tokenizer decodes
 * every line break to `\n`, so a CR could not round-trip. The result is proven
 * by re-tokenizing it -- one terminated STRING token whose decoded value is the
 * input and whose lexeme is the whole encoding -- rather than trusted.
 *
 * @param {string} value
 * @returns {{text:string}|{reason:string, message:string}}
 */
function encodeString(value) {
  if (typeof value !== 'string') return { reason: FIELD_EDIT_REASON.INPUT_NOT_STRING, message: 'Enter text.' };
  if (value.includes('\r')) {
    return { reason: FIELD_EDIT_REASON.INPUT_STRING_UNENCODABLE, message: 'Carriage returns cannot be stored in an SFString.' };
  }
  const text = `"${value.replace(/[\\"]/g, '\\$&')}"`;
  const lexed = tokenize(text).tokens;
  const tok = lexed[0];
  if (lexed.length !== 2 || !tok || tok.type !== TT.STRING || tok.terminated !== true
      || tok.lexeme !== text || tok.value !== value || lexed[1].type !== TT.EOF) {
    return { reason: FIELD_EDIT_REASON.INPUT_STRING_UNENCODABLE, message: 'This text cannot be encoded as a VRML97 string.' };
  }
  return { text };
}

// ---------------------------------------------------------------------------
// Encoding a whole value for an ABSENT field (WD2-C)
// ---------------------------------------------------------------------------

/**
 * Validate and encode a complete typed value for one schema field of a
 * built-in VRML97 node, for INSERTING a field that is not authored yet
 * (src/vrml/structure-edit.js planFieldInsert). It is the same schema gate and
 * the same per-component encoders planFieldEdit uses -- there is one value
 * validator, not two.
 *
 * `equalsDefault` is true when every component equals the schema default
 * numerically (or exactly, for bool/string): an unchanged default needs no
 * authored field at all.
 *
 * @returns {{status:'ok', type:string, text:string, components:object[],
 *   equalsDefault:boolean}|{status:'refused', reason:string, message?:string,
 *   componentIndex?:number}} Frozen.
 */
function encodeFieldValue(nodeType, fieldName, components) {
  const record = typeof nodeType === 'string' && typeof fieldName === 'string'
    ? schema.getFieldSchema(nodeType, fieldName) : null;
  if (!record) return refuse(FIELD_EDIT_REASON.FIELD_UNKNOWN);
  if (!record.profiles.includes('vrml97')) return refuse(FIELD_EDIT_REASON.FIELD_X3D_ONLY);
  if (!STORED_DECLARATIONS.has(record.vrml97Declaration)) return refuse(FIELD_EDIT_REASON.FIELD_NOT_STORED);
  const spec = Object.prototype.hasOwnProperty.call(TYPE_SPECS, record.type) ? TYPE_SPECS[record.type] : null;
  if (!spec) return refuse(FIELD_EDIT_REASON.TYPE_UNSUPPORTED);
  if (!Array.isArray(components) || components.length !== spec.arity) return refuse(FIELD_EDIT_REASON.INPUT_SHAPE);
  const bounds = spec.kind === 'number' ? numericBounds(record.constraints) : null;
  const defaults = Array.isArray(record.defaultValue) ? record.defaultValue : [record.defaultValue];
  const out = [];
  let equalsDefault = defaults.length === spec.arity;
  for (let i = 0; i < spec.arity; i += 1) {
    const raw = components[i];
    let enc;
    if (spec.kind === 'bool') {
      if (typeof raw !== 'boolean') return refuse(FIELD_EDIT_REASON.INPUT_NOT_BOOLEAN, { componentIndex: i, message: 'Choose TRUE or FALSE.' });
      enc = { text: raw ? 'TRUE' : 'FALSE', value: raw };
    } else if (spec.kind === 'string') {
      const s = encodeString(raw);
      if (s.reason) return refuse(s.reason, { componentIndex: i, message: s.message });
      enc = { text: s.text, value: raw };
    } else {
      enc = encodeNumber(spec, raw, bounds);
      if (enc.reason) return refuse(enc.reason, { componentIndex: i, message: enc.message });
    }
    if (equalsDefault && defaults[i] !== enc.value) equalsDefault = false;
    out.push(Object.freeze({ text: enc.text, value: enc.value }));
  }
  return Object.freeze({
    status: 'ok',
    type: record.type,
    text: out.map((c) => c.text).join(' '),
    components: Object.freeze(out),
    equalsDefault,
  });
}

// ---------------------------------------------------------------------------
// Planning an edit
// ---------------------------------------------------------------------------

function locateField(node, fieldIndex, fieldName) {
  const list = Array.isArray(node.fields) ? node.fields : [];
  if (!Number.isInteger(fieldIndex) || fieldIndex < 0 || fieldIndex >= list.length) return null;
  const f = list[fieldIndex];
  if (!f || f.type !== NODE.FIELD || f.name !== fieldName) return null;
  return { index: fieldIndex, field: f };
}

// Re-parse the proposed text and prove the edit did exactly what was asked:
// no new syntax error, the SAME node (through the WD1.4 Tier 1 identity layer
// and the just-verified receipt, never by position or label), the same field at
// the same index, and every component now holding the intended token.
function roundTrip(session, node, located, spec, intended, newText, receipt) {
  const parsed = parseSyntax(newText);
  if (hasBlockingSyntaxError(parsed) || parsed.truncated || parsed.depthCapped) return false;
  const next = tx.createParseSession(newText, parsed);
  const anchor = identity.createTransactionAnchor(session, node);
  if (anchor.status !== identity.ANCHOR_STATUS.CREATED) return false;
  const resolved = identity.resolveTransactionAnchor(anchor.anchor, next, receipt);
  if (!identity.isResolved(resolved)) return false;
  const again = locateField(resolved.node, located.index, located.field.name);
  if (!again || again.field.isBinding) return false;
  const shape = componentsOf(spec, again.field.value);
  if (shape.reason || shape.components.length !== intended.length) return false;
  return shape.components.every((c, i) => {
    const want = intended[i];
    if (spec.kind === 'bool' || spec.kind === 'string') return c.value === want.value;
    return c.token.lexeme === want.text && c.value === want.value;
  });
}

/**
 * Turn a typed Inspector value into a verified WD1.2 edit set.
 *
 * @param {object} request
 * @param {object} request.session Parse session of the text the selection came from.
 * @param {string} request.currentText The editor's exact current text.
 * @param {object} request.node AST node (NODE.NODE) from `session`.
 * @param {number} request.fieldIndex Index into `node.fields` of the field.
 * @param {string} request.fieldName The field's name (must match the index).
 * @param {Array} request.components One raw value per component: a boolean for
 *   SFBool, a string for SFString, a number or numeric string otherwise.
 * @returns {object} Frozen. `{status:'ready', edits, oldText, newText, receipt,
 *   changed}` | `{status:'unchanged'}` | `{status:'refused', reason, message?,
 *   componentIndex?}`. Never throws for ordinary invalid input.
 */
function planFieldEdit(request) {
  if (!request || typeof request !== 'object') return refuse(FIELD_EDIT_REASON.INPUT_SHAPE);
  const { session, currentText, node, fieldIndex, fieldName, components } = request;
  if (!tx.isParseSession(session)) return refuse(FIELD_EDIT_REASON.STALE_SESSION);
  if (typeof currentText !== 'string') return refuse(FIELD_EDIT_REASON.INPUT_SHAPE);
  const nodeReason = nodeGate(session, node, currentText);
  if (nodeReason) return refuse(nodeReason);
  const located = locateField(node, fieldIndex, fieldName);
  if (!located) return refuse(FIELD_EDIT_REASON.FIELD_NOT_PRESENT);
  const described = describeField(session, node, located, null, duplicateNamesOf(authoredFields(node)));
  const { descriptor, tokens, spec } = described;
  if (!descriptor.editable) return refuse(descriptor.reason);
  if (!Array.isArray(components) || components.length !== spec.arity) return refuse(FIELD_EDIT_REASON.INPUT_SHAPE);

  const edits = [];
  const intended = [];
  const changed = [];
  for (let i = 0; i < spec.arity; i += 1) {
    const current = descriptor.components[i];
    const raw = components[i];
    let want;
    let insert = null;
    if (spec.kind === 'bool') {
      if (typeof raw !== 'boolean') return refuse(FIELD_EDIT_REASON.INPUT_NOT_BOOLEAN, { componentIndex: i, message: 'Choose TRUE or FALSE.' });
      want = { value: raw };
      if (raw !== current.value) insert = raw ? 'TRUE' : 'FALSE';
    } else if (spec.kind === 'string') {
      const enc = encodeString(raw);
      if (enc.reason) return refuse(enc.reason, { componentIndex: i, message: enc.message });
      want = { value: raw };
      if (raw !== current.value) insert = enc.text;
    } else {
      const unchangedText = (typeof raw === 'string' ? raw.trim() : raw) === current.text;
      if (unchangedText) {
        want = { text: current.text, value: current.value };
      } else {
        const enc = encodeNumber(spec, raw, descriptor.bounds);
        if (enc.reason) return refuse(enc.reason, { componentIndex: i, message: enc.message });
        want = { text: enc.text, value: enc.value };
        if (enc.text !== current.text) insert = enc.text;
      }
    }
    intended.push(want);
    if (insert !== null) {
      edits.push(edit.replaceSpan(offsetsOf(tokens[i].range), insert));
      changed.push(i);
    }
  }
  if (edits.length === 0) return Object.freeze({ status: PLAN_STATUS.UNCHANGED });

  // WD1.2 derives the new text; WD1.4 proves the set is exactly what produced
  // it. A rejection is reported and nothing is dispatched.
  const oldText = session.text;
  let canonical;
  let newText;
  try {
    canonical = edit.validateEdits(oldText, edits);
    newText = edit.applyEdits(oldText, canonical);
  } catch (err) {
    return refuse(FIELD_EDIT_REASON.TRANSACTION_REJECTED, { detail: err && err.code ? err.code : 'error' });
  }
  const receipt = tx.verifyTransaction({ oldText, edits: canonical, newText });
  if (receipt.status !== tx.TX_STATUS.VERIFIED) {
    return refuse(FIELD_EDIT_REASON.TRANSACTION_REJECTED, { detail: receipt.reason });
  }
  if (!roundTrip(session, node, located, spec, intended, newText, receipt)) {
    return refuse(FIELD_EDIT_REASON.ROUND_TRIP_FAILED);
  }
  return Object.freeze({
    status: PLAN_STATUS.READY,
    edits: canonical,
    oldText,
    newText,
    receipt,
    changed: Object.freeze(changed),
  });
}

module.exports = {
  FIELD_EDIT_STATUS,
  FIELD_EDIT_REASON,
  PLAN_STATUS,
  TYPE_SPECS,
  EDITABLE_TYPES,
  inspectNodeFields,
  planFieldEdit,
  encodeString,
  // WD2-C: shared with src/vrml/structure-edit.js so structural edits use the
  // same node gate, syntax gate and value encoder as a field edit.
  encodeFieldValue,
  nodeEditGate: nodeGate,
  hasBlockingSyntaxError,
};
