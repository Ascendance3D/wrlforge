'use strict';
// Exact-span structural edits (Phase WD2-C "First Object").
//
// PURE and browser-safe: requires only sibling src/vrml modules -- the AST
// constants, the tokenizer + syntax parser (for the round-trip check), the
// WD1.2 span-patch algebra, the WD1.4 parse-session / verified-transaction /
// node-identity layer, the WD1.3 schema, the WD2-B field-edit gates + value
// encoder, and the WD2-C new-node templates. No fs, no Electron, no
// CodeMirror, no DOM. Like field-edit.js it writes nothing: every planner
// returns WD1.2 edits plus a verified receipt, and the caller dispatches them.
//
// THE DOCUMENT IS THE TEXT (WD.md section 2). Four operations, each a set of
// span patches over the exact current text:
//
//   planInsertObject  -- a NEW anonymous simple object (node-templates.js) is
//                        inserted at the END of the root scene; a missing
//                        header is added only to an empty/whitespace document.
//   planDuplicateNode -- the selected node's EXACT source bytes are copied and
//                        inserted right after it. Nothing is re-printed.
//   planDeleteNode    -- the selected node's exact span, plus the whitespace
//                        that only it owned, is removed.
//   planFieldInsert   -- one ABSENT field (`name value`) is inserted into an
//                        existing node body, at the start of the body.
//
// Ownership is conservative. A node is structurally editable only as a
// root-scene statement or an item of a schema-proven MFNode field of a
// built-in node, never inside a PROTO body, never as an SFNode value, never in
// a document with a syntax error (parser recovery moves boundaries, WD.md
// section 8 rule 3). Comments outside the owned span are never touched; the
// only whitespace removed with a node is the indentation + line ending of a
// line the node occupies ALONE, or the space run directly after an inline node.
//
// Every plan is verified before it is returned, and refused rather than
// returned when any check fails:
//   1. WD1.2 validateEdits/applyEdits derive the new text;
//   2. WD1.4 verifyTransaction proves the edit set is exactly old -> new;
//   3. the new text reparses with no blocking syntax error and no cap;
//   4. TOKEN STREAM: the tokens of the new text are exactly the old tokens
//      before each edit + the tokens of the inserted text + the old tokens after
//      (and the old text splits into whole tokens at each cut) -- so no edit can
//      fuse, split or swallow a neighbouring token;
//   5. the structural post-condition of that operation (the inserted node is
//      exactly where planned, in the same kind of parent; the inserted field is
//      on the SAME node, authored once, with exactly the intended tokens).
//
// IDENTITY. A new node (Add / Duplicate) has no old anchor to map, so it is
// identified by resolveInsertedNode: the node whose exact range is a span that
// lies wholly inside text the VERIFIED receipt inserted. That proves the node is
// new and which one it is, without a name, a "newest Box" search, or a
// position guess. Everything else stays with the WD1.4 Tier 1 anchor.

const { NODE, walk } = require('./ast');
const { tokenize, TT } = require('./tokenizer');
const { parse: parseSyntax } = require('./parser');
const edit = require('./edit');
const tx = require('./document-transaction');
const identity = require('./node-identity');
const schema = require('./node-schema');
const fieldEdit = require('./field-edit');
const templates = require('./node-templates');

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

const PLAN_STATUS = fieldEdit.PLAN_STATUS;

// Stable reason ids. Node-gate reasons are field-edit's own ids, reused so the
// two layers never disagree on why a node is not editable.
const FR = fieldEdit.FIELD_EDIT_REASON;
const STRUCTURE_REASON = Object.freeze({
  OK: 'ok',
  STALE_SESSION: FR.STALE_SESSION,
  PARSE_INCOMPLETE: FR.PARSE_INCOMPLETE,
  SYNTAX_ERRORS: FR.SYNTAX_ERRORS,
  NOT_A_NODE: FR.NOT_A_NODE,
  NODE_NOT_IN_SESSION: FR.NODE_NOT_IN_SESSION,
  NODE_INCOMPLETE: FR.NODE_INCOMPLETE,
  UNKNOWN_NODE_TYPE: FR.UNKNOWN_NODE_TYPE,
  PROTO_INSTANCE: FR.PROTO_INSTANCE,
  // --- the document -------------------------------------------------------
  NO_HEADER: 'document-has-no-vrml97-header',
  HEADER_NOT_VRML97: 'document-header-not-vrml97',
  UNSUPPORTED_PRIMITIVE: 'unsupported-primitive',
  // --- where the node sits ------------------------------------------------
  IN_PROTO: 'node-is-inside-a-proto',
  NOT_IN_NODE_LIST: 'node-is-not-in-a-node-list',
  PARENT_NOT_PROVABLE: 'parent-field-not-a-provable-mfnode',
  // --- what the node contains / what refers to it --------------------------
  CONTAINS_DEF: 'contains-def-name',
  CONTAINS_USE: 'contains-use',
  CONTAINS_ROUTE_OR_PROTO: 'contains-route-or-proto',
  REFERENCED: 'node-is-referenced',
  // --- absent-field insertion ---------------------------------------------
  FIELD_ALREADY_AUTHORED: 'field-already-authored',
  BODY_NOT_LOCATED: 'node-body-not-located',
  // --- verification -------------------------------------------------------
  TRANSACTION_REJECTED: FR.TRANSACTION_REJECTED,
  ROUND_TRIP_FAILED: FR.ROUND_TRIP_FAILED,
  TOKENS_CHANGED: 'surrounding-tokens-changed',
});

const INSERTED_STATUS = Object.freeze({ RESOLVED: 'resolved', REFUSED: 'refused' });

const INSERTED_REASON = Object.freeze({
  VERIFIED_INSERTION: 'verified-inserted-span',
  NO_SESSION: 'no-parse-session',
  RECEIPT_NOT_ISSUED: 'receipt-not-issued',
  RECEIPT_NOT_BOUND_TO_RESULT: 'receipt-not-bound-to-result',
  SPAN_NOT_INSERTED: 'span-not-wholly-inserted-text',
  NO_NODE_AT_SPAN: 'no-node-at-span',
  SPAN_AMBIGUOUS: 'span-ambiguous',
});

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

const refuse = (reason, extra) => Object.freeze({ status: PLAN_STATUS.REFUSED, reason, ...(extra || {}) });
const spanOf = (n) => ({ from: n.range.start.offset, to: n.range.end.offset });
const hasSpan = (n) => !!(n && n.range && n.range.start && n.range.end
  && Number.isInteger(n.range.start.offset) && Number.isInteger(n.range.end.offset));

const isHSpace = (ch) => ch === ' ' || ch === '\t';

// The document's own line ending: the first one it uses, else LF.
function detectEol(text) {
  const i = text.indexOf('\n');
  return i > 0 && text[i - 1] === '\r' ? '\r\n' : '\n';
}

// Length of the line ending starting at i (CRLF, LF or a lone CR), else 0.
function eolLengthAt(text, i) {
  if (text[i] === '\r') return text[i + 1] === '\n' ? 2 : 1;
  return text[i] === '\n' ? 1 : 0;
}

// Length of the line ending that ENDS at i (exclusive), else 0.
function eolLengthBefore(text, i) {
  if (text[i - 1] === '\n') return text[i - 2] === '\r' ? 2 : 1;
  return text[i - 1] === '\r' ? 1 : 0;
}

function lineStartOf(text, offset) {
  let i = offset;
  while (i > 0 && text[i - 1] !== '\n' && text[i - 1] !== '\r') i -= 1;
  return i;
}

function lineEndOf(text, offset) {
  let i = offset;
  while (i < text.length && text[i] !== '\n' && text[i] !== '\r') i += 1;
  return i;
}

function onlyHSpace(text, from, to) {
  for (let i = from; i < to; i += 1) if (!isHSpace(text[i])) return false;
  return true;
}

// Whether the span [start, end) occupies its first..last lines ALONE (only
// spaces/tabs before it on its first line and after it on its last line).
function lineAlone(text, start, end) {
  const ls = lineStartOf(text, start);
  const le = lineEndOf(text, end);
  return { alone: onlyHSpace(text, ls, start) && onlyHSpace(text, end, le), lineStart: ls, lineEnd: le };
}

const tokenKeys = (text) => tokenize(text).tokens
  .filter((t) => t.type !== TT.EOF)
  .map((t) => `${t.type}\u0000${t.lexeme}`);

const sameList = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);

// Check 4: the edit set moved, fused, split or swallowed no token outside the
// text it replaced. Both directions: the OLD text must split into whole tokens
// at every cut, and the NEW text must be exactly those kept tokens with each
// insert's own tokens spliced in.
function tokenStreamPreserved(oldText, canonical, newText) {
  const keptBefore = [];
  const expected = [];
  let pos = 0;
  for (const e of canonical) {
    const kept = tokenKeys(oldText.slice(pos, e.from));
    keptBefore.push(...kept, ...tokenKeys(oldText.slice(e.from, e.to)));
    expected.push(...kept, ...tokenKeys(e.insert));
    pos = e.to;
  }
  const tail = tokenKeys(oldText.slice(pos));
  keptBefore.push(...tail);
  expected.push(...tail);
  return sameList(tokenKeys(oldText), keptBefore) && sameList(tokenKeys(newText), expected);
}

// The document-level gate shared by every planner. Returns a reason or null.
function documentGate(session, currentText) {
  if (!tx.isParseSession(session)) return STRUCTURE_REASON.STALE_SESSION;
  if (typeof currentText !== 'string' || currentText !== session.text) return STRUCTURE_REASON.STALE_SESSION;
  const p = session.parse;
  if (p.truncated || p.depthCapped) return STRUCTURE_REASON.PARSE_INCOMPLETE;
  if (fieldEdit.hasBlockingSyntaxError(p)) return STRUCTURE_REASON.SYNTAX_ERRORS;
  return null;
}

// Where a node sits: the root scene, an MFNode list, an SFNode value, a PROTO
// body or an interface default. Walks the parse once; null if absent.
//   { where: 'root'|'mf'|'sf'|'interface', inProto, owner, field }
function contextOf(tree, target) {
  let found = null;
  function visit(value, ctx) {
    if (found || !value || typeof value !== 'object') return;
    if (Array.isArray(value)) {
      for (const item of value) visit(item, ctx);
      return;
    }
    if (typeof value.type !== 'string') return;
    let next = ctx;
    switch (value.type) {
      case NODE.NODE:
        if (value === target) { found = ctx; return; }
        next = { where: 'node-body', inProto: ctx.inProto, owner: value, field: null };
        break;
      case NODE.FIELD:
        next = {
          where: value.value && value.value.type === NODE.ARRAY ? 'mf' : 'sf',
          inProto: ctx.inProto,
          owner: ctx.owner,
          field: value,
        };
        break;
      case NODE.PROTO:
        next = { where: 'proto-body', inProto: true, owner: null, field: null };
        break;
      case NODE.INTERFACE:
        next = { where: 'interface', inProto: ctx.inProto, owner: ctx.owner, field: null };
        break;
      default:
        break;
    }
    for (const key in value) {
      if (key === 'range' || key === 'type' || key === 'leadingTrivia' || key.endsWith('Range')) continue;
      const v = value[key];
      if (v && typeof v === 'object') visit(v, next);
    }
  }
  if (tree) visit(tree.statements, { where: 'root', inProto: false, owner: null, field: null });
  return found;
}

// Null when the node's position is one WD2-C may insert next to or remove
// from, else a reason id.
function listContextReason(session, ctx) {
  if (!ctx) return STRUCTURE_REASON.NOT_A_NODE;
  if (ctx.inProto) return STRUCTURE_REASON.IN_PROTO;
  if (ctx.where === 'root') return null;
  if (ctx.where !== 'mf') return STRUCTURE_REASON.NOT_IN_NODE_LIST;
  const owner = ctx.owner;
  const field = ctx.field;
  if (!owner || !field || field.isBinding || typeof field.name !== 'string') return STRUCTURE_REASON.PARENT_NOT_PROVABLE;
  // The owner must itself pass the node gate (standard VRML97, not a PROTO
  // name, complete) and the field must be the schema's MFNode, authored once.
  if (fieldEdit.nodeEditGate(session, owner, session.text)) return STRUCTURE_REASON.PARENT_NOT_PROVABLE;
  const record = schema.getFieldSchema(owner.nodeType, field.name);
  if (!record || record.type !== 'MFNode' || !record.profiles.includes('vrml97')) return STRUCTURE_REASON.PARENT_NOT_PROVABLE;
  const sameName = (owner.fields || []).filter((f) => f && f.type === NODE.FIELD && f.name === field.name);
  if (sameName.length !== 1) return STRUCTURE_REASON.PARENT_NOT_PROVABLE;
  return null;
}

function nodeReason(session, node, currentText) {
  const docReason = documentGate(session, currentText);
  if (docReason) return docReason;
  const r = fieldEdit.nodeEditGate(session, node, currentText);
  if (r) return r;
  if (!hasSpan(node)) return STRUCTURE_REASON.NOT_A_NODE;
  return null;
}

// Every node object whose range is exactly [from, to).
function nodesAtSpan(tree, from, to) {
  const out = [];
  walk(tree, (n) => {
    if (n.type === NODE.NODE && hasSpan(n) && n.range.start.offset === from && n.range.end.offset === to) out.push(n);
  });
  return out;
}

// Steps 1-4 of the verification, shared by every planner. Returns
// { canonical, newText, receipt, next } or a refusal.
function verifyPlan(session, edits) {
  const oldText = session.text;
  let canonical;
  let newText;
  try {
    canonical = edit.validateEdits(oldText, edits);
    newText = edit.applyEdits(oldText, canonical);
  } catch (err) {
    return refuse(STRUCTURE_REASON.TRANSACTION_REJECTED, { detail: err && err.code ? err.code : 'error' });
  }
  const receipt = tx.verifyTransaction({ oldText, edits: canonical, newText });
  if (receipt.status !== tx.TX_STATUS.VERIFIED) {
    return refuse(STRUCTURE_REASON.TRANSACTION_REJECTED, { detail: receipt.reason });
  }
  const parsed = parseSyntax(newText);
  if (fieldEdit.hasBlockingSyntaxError(parsed) || parsed.truncated || parsed.depthCapped) {
    return refuse(STRUCTURE_REASON.ROUND_TRIP_FAILED);
  }
  if (!tokenStreamPreserved(oldText, canonical, newText)) return refuse(STRUCTURE_REASON.TOKENS_CHANGED);
  return { canonical, newText, receipt, next: tx.createParseSession(newText, parsed) };
}

function ready(session, verified, extra) {
  return Object.freeze({
    status: PLAN_STATUS.READY,
    edits: verified.canonical,
    oldText: session.text,
    newText: verified.newText,
    receipt: verified.receipt,
    ...(extra || {}),
  });
}

// ---------------------------------------------------------------------------
// Identity of an inserted node
// ---------------------------------------------------------------------------

/**
 * Resolve the node a verified insertion created.
 *
 * Resolved only when the receipt was minted by verifyTransaction, is bound to
 * exactly `session.text`, the span lies WHOLLY inside text one of its edits
 * inserted (so every byte of the node is new), and exactly one node of
 * `nodeType` has exactly that range in the new parse. Anything else refuses;
 * the caller clears the selection rather than guessing.
 *
 * @param {{session:object, receipt:object, span:{from:number,to:number}, nodeType:string}} input
 * @returns {{status:'resolved', node:object, reason:string}|{status:'refused', reason:string}} Frozen.
 */
function resolveInsertedNode(input) {
  const { session, receipt, span, nodeType } = input || {};
  const no = (reason) => Object.freeze({ status: INSERTED_STATUS.REFUSED, reason });
  if (!tx.isParseSession(session)) return no(INSERTED_REASON.NO_SESSION);
  if (!tx.isVerifiedReceipt(receipt)) return no(INSERTED_REASON.RECEIPT_NOT_ISSUED);
  if (!tx.receiptBindsNewText(receipt, session.text)) return no(INSERTED_REASON.RECEIPT_NOT_BOUND_TO_RESULT);
  if (!span || !Number.isInteger(span.from) || !Number.isInteger(span.to) || span.from >= span.to) {
    return no(INSERTED_REASON.SPAN_NOT_INSERTED);
  }
  const edits = tx.receiptEdits(receipt).slice().sort((a, b) => a.from - b.from || a.to - b.to);
  let delta = 0;
  let inside = false;
  for (const e of edits) {
    const at = e.from + delta;
    if (span.from >= at && span.to <= at + e.insert.length) inside = true;
    delta += e.insert.length - (e.to - e.from);
  }
  if (!inside) return no(INSERTED_REASON.SPAN_NOT_INSERTED);
  const matches = nodesAtSpan(session.parse.tree, span.from, span.to).filter((n) => n.nodeType === nodeType);
  if (matches.length === 0) return no(INSERTED_REASON.NO_NODE_AT_SPAN);
  if (matches.length > 1) return no(INSERTED_REASON.SPAN_AMBIGUOUS);
  return Object.freeze({ status: INSERTED_STATUS.RESOLVED, node: matches[0], reason: INSERTED_REASON.VERIFIED_INSERTION });
}

// Step 5 for Add / Duplicate: the planned span holds exactly one new node of
// the expected type, in the expected kind of parent.
function insertedNodeChecks(verified, span, nodeType, expectContext) {
  const r = resolveInsertedNode({ session: verified.next, receipt: verified.receipt, span, nodeType });
  if (r.status !== INSERTED_STATUS.RESOLVED) return false;
  const ctx = contextOf(verified.next.parse.tree, r.node);
  if (!ctx || ctx.inProto || ctx.where !== expectContext.where) return false;
  if (ctx.where === 'mf') {
    return ctx.field.name === expectContext.field.name && ctx.owner.nodeType === expectContext.owner.nodeType;
  }
  return true;
}

// ---------------------------------------------------------------------------
// Add
// ---------------------------------------------------------------------------

// Separator text placed before an object appended at the end of `text`, so
// it starts on its own line after one blank line.
function separatorBefore(text, eol) {
  const last = eolLengthBefore(text, text.length);
  if (!last) return eol + eol;
  let i = text.length - last;
  while (i > 0 && isHSpace(text[i - 1])) i -= 1;
  return eolLengthBefore(text, i) ? '' : eol;
}

/**
 * Plan adding a new anonymous simple object (Box / Sphere) at the end of the
 * root scene.
 *
 * Insertion is ROOT-ONLY in WD2-C: the end of a document with no syntax error
 * is always root scope, so the target never has to be guessed. An empty or
 * whitespace-only document gets the VRML97 header; a document with other
 * content and no header, or a non-VRML97 header, is refused -- it is never
 * silently "repaired".
 *
 * @param {{session:object, currentText:string, primitive:string}} request
 * @returns {object} Frozen ready plan (`select: {span, nodeType}` identifies
 *   the new node for resolveInsertedNode) or a refusal.
 */
function planInsertObject(request) {
  const { session, currentText, primitive } = request || {};
  if (!templates.PRIMITIVES.includes(primitive)) return refuse(STRUCTURE_REASON.UNSUPPORTED_PRIMITIVE);
  // A missing header (VRML001) is not a blocking error, so a headerless
  // document reaches the header checks below.
  const docReason = documentGate(session, currentText);
  if (docReason) return refuse(docReason);
  const text = session.text;
  const header = session.parse.tree ? session.parse.tree.header : null;
  const blank = /^[ \t\r\n]*$/.test(text);
  if (!header && !blank) return refuse(STRUCTURE_REASON.NO_HEADER);
  if (header && (header.version !== 'V2.0' || header.encoding !== 'utf8')) return refuse(STRUCTURE_REASON.HEADER_NOT_VRML97);

  const eol = detectEol(text);
  const tpl = templates.simpleObjectTemplate(primitive, { eol });
  const edits = [];
  let from;
  if (text === '') {
    const lead = templates.VRML97_HEADER + eol + eol;
    edits.push(edit.insertAt(0, lead + tpl.text + eol));
    from = lead.length;
  } else {
    let headerLen = 0;
    if (!header) {
      const h = templates.VRML97_HEADER + eol;
      edits.push(edit.insertAt(0, h));
      headerLen = h.length;
    }
    const sep = separatorBefore(text, eol);
    edits.push(edit.insertAt(text.length, sep + tpl.text + eol));
    from = text.length + headerLen + sep.length;
  }
  const span = Object.freeze({ from, to: from + tpl.text.length });
  const verified = verifyPlan(session, edits);
  if (verified.status === PLAN_STATUS.REFUSED) return verified;
  if (!insertedNodeChecks(verified, span, tpl.nodeType, { where: 'root' })) return refuse(STRUCTURE_REASON.ROUND_TRIP_FAILED);
  return ready(session, verified, {
    operation: 'add',
    primitive,
    select: Object.freeze({ span, nodeType: tpl.nodeType }),
  });
}

// ---------------------------------------------------------------------------
// Duplicate
// ---------------------------------------------------------------------------

// Constructs inside a span that a byte copy cannot duplicate without semantic
// rewriting. WD2-C refuses rather than renaming anything.
function duplicateBlocker(node) {
  const defs = [];
  let use = false;
  let routeOrProto = false;
  walk(node, (n) => {
    if (n.type === NODE.NODE && n.def) defs.push(n.def);
    else if (n.type === NODE.USE) use = true;
    else if (n.type === NODE.ROUTE || n.type === NODE.PROTO || n.type === NODE.EXTERNPROTO) routeOrProto = true;
  });
  if (defs.length) return { reason: STRUCTURE_REASON.CONTAINS_DEF, names: defs };
  if (use) return { reason: STRUCTURE_REASON.CONTAINS_USE, names: [] };
  if (routeOrProto) return { reason: STRUCTURE_REASON.CONTAINS_ROUTE_OR_PROTO, names: [] };
  return null;
}

/**
 * Plan duplicating the selected node: its exact source bytes are inserted
 * immediately after it -- on the next line with the same indentation when the
 * node occupies its lines alone, else after one space on the same line.
 *
 * Refuses a node holding any DEF (the copy would be a duplicate name), USE,
 * ROUTE, PROTO or EXTERNPROTO -- no automatic renaming in WD2-C.
 *
 * @param {{session:object, currentText:string, node:object}} request
 */
function planDuplicateNode(request) {
  const { session, currentText, node } = request || {};
  const r = nodeReason(session, node, currentText);
  if (r) return refuse(r);
  const ctx = contextOf(session.parse.tree, node);
  const cr = listContextReason(session, ctx);
  if (cr) return refuse(cr);
  const blocker = duplicateBlocker(node);
  if (blocker) return refuse(blocker.reason, blocker.names.length ? { names: Object.freeze(blocker.names) } : undefined);

  const text = session.text;
  const { from: start, to: end } = spanOf(node);
  const copy = text.slice(start, end);
  const line = lineAlone(text, start, end);
  let insertAt;
  let insert;
  let from;
  if (line.alone) {
    const indent = text.slice(line.lineStart, start);
    const eolLen = eolLengthAt(text, line.lineEnd);
    if (eolLen > 0) {
      insertAt = line.lineEnd + eolLen;
      insert = indent + copy + text.slice(line.lineEnd, insertAt);
      from = insertAt + indent.length;
    } else {
      // Last line, no final line ending: keep it that way.
      const eol = detectEol(text);
      insertAt = line.lineEnd;
      insert = eol + indent + copy;
      from = insertAt + eol.length + indent.length;
    }
  } else {
    insertAt = end;
    insert = ` ${copy}`;
    from = end + 1;
  }
  const span = Object.freeze({ from, to: from + copy.length });
  const verified = verifyPlan(session, [edit.insertAt(insertAt, insert)]);
  if (verified.status === PLAN_STATUS.REFUSED) return verified;
  if (!insertedNodeChecks(verified, span, node.nodeType, ctx)) return refuse(STRUCTURE_REASON.ROUND_TRIP_FAILED);
  return ready(session, verified, {
    operation: 'duplicate',
    select: Object.freeze({ span, nodeType: node.nodeType }),
  });
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

// DEF names declared inside the node's span that a USE or ROUTE OUTSIDE the
// span names. Name-based and document-wide on purpose: conservative across
// scopes and duplicate DEF names (a refusal is cheap; a dangling USE is not).
function externalReferences(tree, node) {
  const { from, to } = spanOf(node);
  const defs = new Set();
  walk(node, (n) => { if (n.type === NODE.NODE && n.def) defs.add(n.def); });
  if (defs.size === 0) return [];
  const outside = (n) => !hasSpan(n) || n.range.end.offset <= from || n.range.start.offset >= to;
  const hits = new Set();
  walk(tree, (n) => {
    if (n.type === NODE.USE && outside(n) && defs.has(n.name)) hits.add(n.name);
    if (n.type === NODE.ROUTE && outside(n)) {
      if (n.from && defs.has(n.from.node)) hits.add(n.from.node);
      if (n.to && defs.has(n.to.node)) hits.add(n.to.node);
    }
  });
  return Array.from(hits).sort();
}

/**
 * Plan deleting the selected node: exactly its source span, plus the
 * indentation and line ending of the line(s) it occupies alone (or, inline, the
 * space run right after it when a space precedes it). Comments and every other
 * byte stay. Refuses when a USE or ROUTE outside the node names a DEF inside it.
 *
 * @param {{session:object, currentText:string, node:object}} request
 */
function planDeleteNode(request) {
  const { session, currentText, node } = request || {};
  const r = nodeReason(session, node, currentText);
  if (r) return refuse(r);
  const ctx = contextOf(session.parse.tree, node);
  const cr = listContextReason(session, ctx);
  if (cr) return refuse(cr);
  const refs = externalReferences(session.parse.tree, node);
  if (refs.length) return refuse(STRUCTURE_REASON.REFERENCED, { names: Object.freeze(refs) });

  const text = session.text;
  const { from: start, to: end } = spanOf(node);
  const line = lineAlone(text, start, end);
  let from;
  let to;
  if (line.alone) {
    const eolLen = eolLengthAt(text, line.lineEnd);
    if (eolLen > 0) {
      from = line.lineStart;
      to = line.lineEnd + eolLen;
    } else {
      // Last line, no final line ending: take the PRECEDING line ending so the
      // file still ends without one.
      from = line.lineStart - eolLengthBefore(text, line.lineStart);
      to = line.lineEnd;
    }
  } else {
    from = start;
    to = end;
    if (start > 0 && isHSpace(text[start - 1])) while (to < text.length && isHSpace(text[to])) to += 1;
  }
  const verified = verifyPlan(session, [edit.removeSpan({ from, to })]);
  if (verified.status === PLAN_STATUS.REFUSED) return verified;
  return ready(session, verified, { operation: 'delete' });
}

// ---------------------------------------------------------------------------
// Absent-field insertion
// ---------------------------------------------------------------------------

// The `{` token opening a node body, by the parse's own token list.
function openBraceOf(session, node) {
  const tokens = session.parse.tokens;
  const after = node.typeRange && node.typeRange.end ? node.typeRange.end.offset : null;
  if (!Array.isArray(tokens) || after === null) return null;
  let lo = 0;
  let hi = tokens.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (tokens[mid].range.start.offset < after) lo = mid + 1; else hi = mid;
  }
  const t = tokens[lo];
  if (!t || t.type !== TT.LBRACE || t.range.end.offset > node.range.end.offset) return null;
  return t;
}

/**
 * Plan inserting one ABSENT field into an existing built-in node, as the first
 * statement of its body. The value is validated and spelled by field-edit.js
 * encodeFieldValue (the same schema gate and encoders as an authored-field
 * edit). A value equal to the schema default is `unchanged` -- nothing to
 * author. Refuses a field that is already authored (use planFieldEdit) and any
 * node inside a PROTO.
 *
 * Placement: when `{` ends its line, a new line `indent + name value` is
 * inserted after it, indented like the first non-blank body line (or the
 * closing brace's indentation plus one unit when the body is empty); otherwise
 * ` name value` goes right after the `{`, followed by a space when the next
 * character is not whitespace.
 *
 * @param {{session:object, currentText:string, node:object, fieldName:string, components:Array}} request
 */
function planFieldInsert(request) {
  const { session, currentText, node, fieldName, components } = request || {};
  const r = nodeReason(session, node, currentText);
  if (r) return refuse(r);
  const ctx = contextOf(session.parse.tree, node);
  if (!ctx) return refuse(STRUCTURE_REASON.NOT_A_NODE);
  if (ctx.inProto) return refuse(STRUCTURE_REASON.IN_PROTO);
  if ((node.fields || []).some((f) => f && f.type === NODE.FIELD && f.name === fieldName)) {
    return refuse(STRUCTURE_REASON.FIELD_ALREADY_AUTHORED);
  }
  const enc = fieldEdit.encodeFieldValue(node.nodeType, fieldName, components);
  if (enc.status !== 'ok') return enc;
  if (enc.equalsDefault) return Object.freeze({ status: PLAN_STATUS.UNCHANGED });

  const text = session.text;
  const brace = openBraceOf(session, node);
  const close = node.range.end.offset - 1;
  if (!brace || text[close] !== '}') return refuse(STRUCTURE_REASON.BODY_NOT_LOCATED);
  const o = brace.range.end.offset;
  const fieldText = `${fieldName} ${enc.text}`;
  const le = lineEndOf(text, o);
  const eolLen = eolLengthAt(text, le);
  let insertAt;
  let insert;
  if (le < close && onlyHSpace(text, o, le) && eolLen > 0) {
    insertAt = le + eolLen;
    let p = insertAt;
    let indent = null;
    while (p <= close) {
      const lineEnd = lineEndOf(text, p);
      let q = p;
      while (q < lineEnd && isHSpace(text[q])) q += 1;
      if (q < lineEnd) {
        indent = text.slice(p, q);
        if (q === close) indent += indent.includes('\t') ? '\t' : '  ';
        break;
      }
      p = lineEnd + eolLengthAt(text, lineEnd);
      if (p === lineEnd) break;
    }
    if (indent === null) return refuse(STRUCTURE_REASON.BODY_NOT_LOCATED);
    insert = indent + fieldText + text.slice(le, le + eolLen);
  } else {
    insertAt = o;
    insert = ` ${fieldText}${isHSpace(text[o]) || eolLengthAt(text, o) ? '' : ' '}`;
  }

  const verified = verifyPlan(session, [edit.insertAt(insertAt, insert)]);
  if (verified.status === PLAN_STATUS.REFUSED) return verified;
  // Step 5: the SAME node (WD1.4 Tier 1 through this receipt) now authors the
  // field exactly once, editable, with exactly the intended tokens.
  const anchor = identity.createTransactionAnchor(session, node);
  if (anchor.status !== identity.ANCHOR_STATUS.CREATED) return refuse(STRUCTURE_REASON.ROUND_TRIP_FAILED);
  const resolved = identity.resolveTransactionAnchor(anchor.anchor, verified.next, verified.receipt);
  if (!identity.isResolved(resolved)) return refuse(STRUCTURE_REASON.ROUND_TRIP_FAILED);
  const described = fieldEdit.inspectNodeFields(verified.next, resolved.node, { currentText: verified.newText });
  const matches = described.fields.filter((f) => f.name === fieldName);
  const got = matches.length === 1 && matches[0].editable ? matches[0] : null;
  const intended = (c, i) => (got.kind === 'number'
    ? c.text === enc.components[i].text && c.value === enc.components[i].value
    : c.value === enc.components[i].value);
  if (!got || got.components.length !== enc.components.length || !got.components.every(intended)) {
    return refuse(STRUCTURE_REASON.ROUND_TRIP_FAILED);
  }
  return ready(session, verified, { operation: 'insert-field', fieldName });
}

module.exports = {
  PLAN_STATUS,
  STRUCTURE_REASON,
  INSERTED_STATUS,
  INSERTED_REASON,
  planInsertObject,
  planDuplicateNode,
  planDeleteNode,
  planFieldInsert,
  resolveInsertedNode,
  contextOf,
  detectEol,
};
