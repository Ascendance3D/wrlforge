'use strict';
// WD2-C -- exact-span structural edits (src/vrml/structure-edit.js).
//
// Every success case is checked against the REAL parser + transaction system
// and for losslessness: every byte outside the canonical edit spans is equal,
// and each span is exactly its insert. Expected texts are literals written out
// by hand, never derived from the code under test.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const vrml = require('../../src/vrml');
const { walk, NODE } = require('../../src/vrml/ast');
const tx = require('../../src/vrml/document-transaction');
const identity = require('../../src/vrml/node-identity');
const se = require('../../src/vrml/structure-edit');

const R = se.STRUCTURE_REASON;
const H = '#VRML V2.0 utf8\n';

const sess = (t) => tx.createParseSession(t, vrml.parse(t));
function nodes(s, type) {
  const out = [];
  walk(s.parse.tree, (n) => { if (n.type === NODE.NODE && (!type || n.nodeType === type)) out.push(n); });
  return out;
}
const slice = (t, n) => t.slice(n.range.start.offset, n.range.end.offset);

const BOX = [
  'Transform {',
  '  children [',
  '    Shape {',
  '      appearance Appearance {',
  '        material Material {',
  '        }',
  '      }',
  '      geometry Box {',
  '      }',
  '    }',
  '  ]',
  '}',
].join('\n');
const SPHERE = BOX.replace('geometry Box', 'geometry Sphere');

// Every byte outside the canonical edit spans is unchanged; each span became
// exactly its insert. Also: the new text reparses with no syntax error.
function assertLossless(plan) {
  assert.equal(plan.status, 'ready', `expected ready, got ${plan.status} ${plan.reason || ''}`);
  const { oldText, newText, edits } = plan;
  let pos = 0;
  let npos = 0;
  for (const e of edits) {
    const keep = oldText.slice(pos, e.from);
    assert.equal(newText.slice(npos, npos + keep.length), keep, 'bytes before an edit are untouched');
    npos += keep.length;
    assert.equal(newText.slice(npos, npos + e.insert.length), e.insert, 'span is exactly its insert');
    npos += e.insert.length;
    pos = e.to;
  }
  assert.equal(newText.slice(npos), oldText.slice(pos), 'bytes after the last edit are untouched');
  const p = vrml.parse(newText);
  assert.equal(p.syntaxDiagnostics.filter((d) => d.severity === 'error').length, 0, 'result has no syntax error');
  assert.ok(tx.isVerifiedReceipt(plan.receipt));
}

const add = (t, primitive = 'Box') => se.planInsertObject({ session: sess(t), currentText: t, primitive });

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

test('Add Box: empty document gets the header + one object, exact text', () => {
  const p = add('');
  assertLossless(p);
  assert.equal(p.newText, `${H}\n${BOX}\n`);
  assert.deepEqual(p.select.span, { from: H.length + 1, to: H.length + 1 + BOX.length });
  assert.equal(p.select.nodeType, 'Transform');
  assert.equal(p.edits.length, 1);
});

test('Add Sphere: same structure with Sphere geometry', () => {
  const p = add('', 'Sphere');
  assert.equal(p.newText, `${H}\n${SPHERE}\n`);
});

test('Add: header-only documents (with and without final newline) are never given a second header', () => {
  assert.equal(add(H).newText, `${H}\n${BOX}\n`);
  assert.equal(add('#VRML V2.0 utf8').newText, `${H}\n${BOX}\n`);
  assert.equal(add('#VRML V2.0 utf8 my world\n').newText, `#VRML V2.0 utf8 my world\n\n${BOX}\n`);
  for (const t of [H, '#VRML V2.0 utf8', '#VRML V2.0 utf8 my world\n']) {
    assert.equal((add(t).newText.match(/#VRML/g) || []).length, 1);
  }
});

test('Add: whitespace-only document keeps its whitespace; header inserted at 0, object at the end', () => {
  const p = add('\n  \n');
  assertLossless(p);
  assert.equal(p.newText, `${H}\n  \n${BOX}\n`.replace(`${H}\n  \n`, `${H}\n  \n`));
  assert.equal(p.edits.length, 2);
  assert.equal(p.edits[0].from, 0);
  assert.equal(p.edits[0].insert, H);
});

test('Add: existing root scene -> appended after one blank line; every old byte is a prefix', () => {
  const t = `${H}# my scene\nGroup { children [ Shape { geometry Cone { } } ] } # tail\n`;
  const p = add(t);
  assertLossless(p);
  assert.ok(p.newText.startsWith(t));
  assert.equal(p.newText, `${t}\n${BOX}\n`);
});

test('Add: no final newline -> a line ending is added before, never after existing text bytes', () => {
  const t = `${H}Group { }`;
  assert.equal(add(t).newText, `${t}\n\n${BOX}\n`);
  const c = `${H}Group { } # trailing comment, no newline`;
  const p = add(c);
  assertLossless(p);
  assert.ok(p.newText.startsWith(c), 'the comment is untouched and the object is on its own line');
});

test('Add: already ends with a blank line -> no extra separator', () => {
  const t = `${H}Group { }\n\n`;
  assert.equal(add(t).newText, `${t}${BOX}\n`);
});

test('Add: document ending in a ROUTE -> object appended at root after it', () => {
  const t = `${H}DEF T TimeSensor { }\nDEF I PositionInterpolator { }\nROUTE T.fraction_changed TO I.set_fraction\n`;
  const p = add(t);
  assertLossless(p);
  const next = sess(p.newText);
  const ctx = se.contextOf(next.parse.tree, nodes(next, 'Transform')[0]);
  assert.equal(ctx.where, 'root');
});

test('Add: CRLF document -> generated object uses CRLF; no bare LF introduced (edit-model level)', () => {
  const t = '#VRML V2.0 utf8\r\nGroup { }\r\n';
  const p = add(t);
  assertLossless(p);
  assert.equal(p.newText, `${t}\r\n${BOX.replace(/\n/g, '\r\n')}\r\n`);
  assert.equal(/(^|[^\r])\n/.test(p.newText), false, 'no bare LF');
});

test('Add: refusals -- headerless content, non-VRML97 header, syntax errors, stale session, unknown primitive', () => {
  assert.equal(add('Group { }').reason, R.NO_HEADER);
  assert.equal(add('#VRML V1.0 ascii\nSeparator { }\n').reason, R.HEADER_NOT_VRML97);
  assert.equal(add(`${H}Transform { children [\n`).reason, R.SYNTAX_ERRORS);
  const s = sess(H);
  assert.equal(se.planInsertObject({ session: s, currentText: `${H}x`, primitive: 'Box' }).reason, R.STALE_SESSION);
  assert.equal(se.planInsertObject({ session: s, currentText: H, primitive: 'Cylinder' }).reason, R.UNSUPPORTED_PRIMITIVE);
  for (const r of [add('Group { }'), add(`${H}Transform {`)]) {
    assert.equal(r.status, 'refused');
    assert.equal(r.newText, undefined, 'a refusal carries no text');
    assert.equal(r.edits, undefined, 'a refusal carries no edits');
  }
});

test('Add: the new object is identified from the verified insertion, not by "newest Box" or position', () => {
  // The document already holds a byte-identical generated object.
  const t = `${H}\n${BOX}\n`;
  const p = add(t);
  const next = sess(p.newText);
  const r = se.resolveInsertedNode({ session: next, receipt: p.receipt, span: p.select.span, nodeType: 'Transform' });
  assert.equal(r.status, 'resolved');
  const all = nodes(next, 'Transform');
  assert.equal(all.length, 2);
  assert.equal(r.node, all[1], 'the inserted (second) object, never the identical first');
  // The pre-existing identical object's span is NOT inserted text -> refused.
  const old = { from: all[0].range.start.offset, to: all[0].range.end.offset };
  assert.equal(se.resolveInsertedNode({ session: next, receipt: p.receipt, span: old, nodeType: 'Transform' }).reason,
    se.INSERTED_REASON.SPAN_NOT_INSERTED);
});

test('resolveInsertedNode fails closed: forged receipt, unbound session, wrong type, no session', () => {
  const p = add(H);
  const next = sess(p.newText);
  const I = se.INSERTED_REASON;
  assert.equal(se.resolveInsertedNode({ session: next, receipt: { status: 'verified' }, span: p.select.span, nodeType: 'Transform' }).reason, I.RECEIPT_NOT_ISSUED);
  assert.equal(se.resolveInsertedNode({ session: next, receipt: JSON.parse(JSON.stringify(p.receipt)), span: p.select.span, nodeType: 'Transform' }).reason, I.RECEIPT_NOT_ISSUED);
  const other = sess(`${p.newText}# changed\n`);
  assert.equal(se.resolveInsertedNode({ session: other, receipt: p.receipt, span: p.select.span, nodeType: 'Transform' }).reason, I.RECEIPT_NOT_BOUND_TO_RESULT);
  assert.equal(se.resolveInsertedNode({ session: next, receipt: p.receipt, span: p.select.span, nodeType: 'Group' }).reason, I.NO_NODE_AT_SPAN);
  assert.equal(se.resolveInsertedNode({ session: null, receipt: p.receipt, span: p.select.span, nodeType: 'Transform' }).reason, I.NO_SESSION);
  const inner = { from: p.select.span.from + 1, to: p.select.span.to };
  assert.equal(se.resolveInsertedNode({ session: next, receipt: p.receipt, span: inner, nodeType: 'Transform' }).reason, I.NO_NODE_AT_SPAN);
});

// ---------------------------------------------------------------------------
// Absent-field insertion
// ---------------------------------------------------------------------------

function insertField(t, nodeType, fieldName, components, nth = 0) {
  const s = sess(t);
  return se.planFieldInsert({ session: s, currentText: t, node: nodes(s, nodeType)[nth], fieldName, components });
}

test('absent fields on the generated object: translation, rotation, size, radius, diffuseColor', () => {
  const t = `${H}\n${BOX}\n`;
  const pos = insertField(t, 'Transform', 'translation', ['3', '0', '0']);
  assertLossless(pos);
  assert.equal(pos.newText, t.replace('Transform {\n', 'Transform {\n  translation 3 0 0\n'));
  const rot = insertField(t, 'Transform', 'rotation', ['0', '1', '0', '1.5708']);
  assert.equal(rot.newText, t.replace('Transform {\n', 'Transform {\n  rotation 0 1 0 1.5708\n'));
  const size = insertField(t, 'Box', 'size', ['2', '1', '0.5']);
  assert.equal(size.newText, t.replace('geometry Box {\n', 'geometry Box {\n        size 2 1 0.5\n'));
  const col = insertField(t, 'Material', 'diffuseColor', ['1', '0', '0']);
  assert.equal(col.newText, t.replace('material Material {\n', 'material Material {\n          diffuseColor 1 0 0\n'));
  const s = `${H}\n${SPHERE}\n`;
  const rad = insertField(s, 'Sphere', 'radius', ['2.5']);
  assert.equal(rad.newText, s.replace('geometry Sphere {\n', 'geometry Sphere {\n        radius 2.5\n'));
  for (const p of [pos, rot, size, col, rad]) { assertLossless(p); assert.equal(p.edits.length, 1); assert.equal(p.edits[0].from, p.edits[0].to); }
});

test('absent field: the user\'s numeric spelling is written verbatim; a default value authors nothing', () => {
  const t = `${H}\n${BOX}\n`;
  assert.equal(insertField(t, 'Transform', 'translation', ['1e1', '+2', '.5']).newText,
    t.replace('Transform {\n', 'Transform {\n  translation 1e1 +2 .5\n'));
  assert.equal(insertField(t, 'Transform', 'translation', ['0', '0.0', '0e0']).status, 'unchanged');
  assert.equal(insertField(t, 'Box', 'size', ['2', '2', '2']).status, 'unchanged');
  assert.equal(insertField(t, 'Material', 'diffuseColor', ['0.8', '.8', '8e-1']).status, 'unchanged');
});

test('absent field: invalid input refused before any edit (schema bounds, lexical shape, arity)', () => {
  const t = `${H}\n${BOX}\n`;
  const FR = vrml.fieldEdit.FIELD_EDIT_REASON;
  assert.equal(insertField(t, 'Box', 'size', ['0', '1', '1']).reason, FR.INPUT_OUT_OF_RANGE);
  assert.equal(insertField(t, 'Material', 'diffuseColor', ['1.5', '0', '0']).reason, FR.INPUT_OUT_OF_RANGE);
  assert.equal(insertField(t, 'Transform', 'translation', ['abc', '0', '0']).reason, FR.INPUT_NOT_NUMBER);
  assert.equal(insertField(t, 'Transform', 'translation', ['1', '0']).reason, FR.INPUT_SHAPE);
  assert.equal(insertField(t, 'Transform', 'translation', ['NaN', '0', '0']).reason, FR.INPUT_NOT_NUMBER);
  assert.equal(insertField(t, 'Transform', 'bogusField', ['1']).reason, FR.FIELD_UNKNOWN);
  assert.equal(insertField(`${H}Group { }\n`, 'Group', 'children', ['1']).reason, FR.TYPE_UNSUPPORTED);
});

test('absent field: refuses an authored field, a PROTO body node, and a damaged document', () => {
  const authored = `${H}Transform { translation 1 2 3 }\n`;
  assert.equal(insertField(authored, 'Transform', 'translation', ['3', '0', '0']).reason, R.FIELD_ALREADY_AUTHORED);
  const proto = `${H}PROTO P [ ] { Transform { } }\nP { }\n`;
  assert.equal(insertField(proto, 'Transform', 'translation', ['3', '0', '0']).reason, R.IN_PROTO);
  const broken = `${H}Transform { children [\n`;
  assert.equal(insertField(broken, 'Transform', 'translation', ['3', '0', '0']).reason, R.SYNTAX_ERRORS);
});

test('absent field: single-line bodies, comments after the brace, tabs, CRLF, other fields', () => {
  const cases = [
    [`${H}Box {}\n`, 'Box', 'size', ['1', '2', '3'], `${H}Box { size 1 2 3 }\n`],
    [`${H}Box { }\n`, 'Box', 'size', ['1', '2', '3'], `${H}Box { size 1 2 3 }\n`],
    [`${H}Transform { # keep me\n  children [ ]\n}\n`, 'Transform', 'translation', ['1', '0', '0'],
      `${H}Transform { translation 1 0 0 # keep me\n  children [ ]\n}\n`],
    [`${H}Transform {\n\tchildren [ ]\n}\n`, 'Transform', 'translation', ['1', '0', '0'],
      `${H}Transform {\n\ttranslation 1 0 0\n\tchildren [ ]\n}\n`],
    [`${H}Transform {\n\t}\n`, 'Transform', 'translation', ['1', '0', '0'], `${H}Transform {\n\t\ttranslation 1 0 0\n\t}\n`],
    ['#VRML V2.0 utf8\r\nTransform {\r\n  children [ ]\r\n}\r\n', 'Transform', 'translation', ['1', '0', '0'],
      '#VRML V2.0 utf8\r\nTransform {\r\n  translation 1 0 0\r\n  children [ ]\r\n}\r\n'],
    [`${H}Transform {\n\n    # note\n    scale 2,2,2 # comma whitespace\n}\n`, 'Transform', 'translation', ['1', '0', '0'],
      `${H}Transform {\n    translation 1 0 0\n\n    # note\n    scale 2,2,2 # comma whitespace\n}\n`],
    [`${H}DEF Mover Transform {\n  rotation 0 1 0 0.50E0\n}\n`, 'Transform', 'translation', ['1', '0', '0'],
      `${H}DEF Mover Transform {\n  translation 1 0 0\n  rotation 0 1 0 0.50E0\n}\n`],
    [`${H}Shape { geometry Box{} }`, 'Box', 'size', ['1', '1', '1'], `${H}Shape { geometry Box{ size 1 1 1 } }`],
  ];
  for (const [t, type, field, comps, expected] of cases) {
    const p = insertField(t, type, field, comps);
    assertLossless(p);
    assert.equal(p.newText, expected, JSON.stringify(t));
  }
});

test('absent field: insertion on the SECOND of identical twins changes only the second', () => {
  const t = `${H}Group { children [\n  Transform { }\n  Transform { }\n] }\n`;
  const p = insertField(t, 'Transform', 'translation', ['5', '0', '0'], 1);
  assertLossless(p);
  assert.equal(p.newText, `${H}Group { children [\n  Transform { }\n  Transform { translation 5 0 0 }\n] }\n`);
});

// ---------------------------------------------------------------------------
// Duplicate
// ---------------------------------------------------------------------------

const dup = (t, type, nth = 0) => { const s = sess(t); return se.planDuplicateNode({ session: s, currentText: t, node: nodes(s, type)[nth] }); };
const del = (t, type, nth = 0) => { const s = sess(t); return se.planDeleteNode({ session: s, currentText: t, node: nodes(s, type)[nth] }); };

test('Duplicate: an anonymous simple object -> exact bytes copied on the next line, copy identified', () => {
  const obj = BOX.replace('Transform {\n', 'Transform {\n  translation 1.50 0 -2e0 # where\n');
  const t = `${H}# before\n${obj}\n# after\n`;
  const p = dup(t, 'Transform');
  assertLossless(p);
  assert.equal(p.newText, `${H}# before\n${obj}\n${obj}\n# after\n`);
  const next = sess(p.newText);
  const r = se.resolveInsertedNode({ session: next, receipt: p.receipt, span: p.select.span, nodeType: 'Transform' });
  assert.equal(r.status, 'resolved');
  assert.equal(r.node, nodes(next, 'Transform')[1]);
  assert.equal(slice(p.newText, r.node), obj, 'the copy is byte-identical to the original span');
});

test('Duplicate: indented object inside children, inline siblings, no final newline, CRLF', () => {
  const nested = `${H}Group {\n  children [\n    Shape { geometry Cone { } }\n  ]\n}\n`;
  assert.equal(dup(nested, 'Shape').newText,
    `${H}Group {\n  children [\n    Shape { geometry Cone { } }\n    Shape { geometry Cone { } }\n  ]\n}\n`);
  const inline = `${H}Group { children [ Shape { }, Shape { appearance NULL } ] }\n`;
  assert.equal(dup(inline, 'Shape', 1).newText, `${H}Group { children [ Shape { }, Shape { appearance NULL } Shape { appearance NULL } ] }\n`);
  const eof = `${H}Group { }`;
  assert.equal(dup(eof, 'Group').newText, `${H}Group { }\nGroup { }`);
  const crlf = '#VRML V2.0 utf8\r\nGroup {\r\n}\r\n';
  const p = dup(crlf, 'Group');
  assertLossless(p);
  assert.equal(p.newText, '#VRML V2.0 utf8\r\nGroup {\r\n}\r\nGroup {\r\n}\r\n');
  for (const t of [nested, inline, eof]) assertLossless(dup(t, t === nested || t === inline ? 'Shape' : 'Group', t === inline ? 1 : 0));
});

test('Duplicate refusals: DEF (names reported), USE, ROUTE/PROTO in body, PROTO body, SFNode value, damaged doc', () => {
  const d = dup(`${H}DEF Lamp Transform { children [ DEF Bulb Shape { } ] }\n`, 'Transform');
  assert.equal(d.reason, R.CONTAINS_DEF);
  assert.deepEqual([...d.names], ['Lamp', 'Bulb']);
  assert.equal(dup(`${H}DEF M Material { }\nShape { appearance Appearance { material USE M } }\n`, 'Shape').reason, R.CONTAINS_USE);
  assert.equal(dup(`${H}Group { children [ DEF T TimeSensor { } ] ROUTE T.isActive TO T.set_enabled }\n`, 'Group').reason, R.CONTAINS_DEF);
  assert.equal(dup(`${H}Group { ROUTE A.b TO C.d }\n`, 'Group').reason, R.CONTAINS_ROUTE_OR_PROTO);
  assert.equal(dup(`${H}PROTO P [ ] { Group { } }\n`, 'Group').reason, R.IN_PROTO);
  assert.equal(dup(`${H}Shape { geometry Box { } }\n`, 'Box').reason, R.NOT_IN_NODE_LIST);
  assert.equal(dup(`${H}Transform { children Shape { } }\n`, 'Shape').reason, R.NOT_IN_NODE_LIST);
  assert.equal(dup(`${H}Group { children [ Shape { } ]\n`, 'Shape').reason, R.SYNTAX_ERRORS);
  assert.equal(dup(`${H}PROTO Thing [ field MFNode kids [ ] ] { Group { children IS kids } }\nThing { kids [ Shape { } ] }\n`, 'Shape').reason,
    R.PARENT_NOT_PROVABLE);
  assert.equal(dup(`${H}Group { children [ Shape { } ] children [ ] }\n`, 'Shape').reason, R.PARENT_NOT_PROVABLE);
  assert.equal(dup(`${H}Vendor { children [ Shape { } ] }\n`, 'Shape').reason, R.PARENT_NOT_PROVABLE);
});

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

test('Delete: simple object with its whole line(s); siblings and every comment preserved byte-for-byte', () => {
  const t = `${H}# before A\nGroup { } # after A\n# between\n${BOX}\n# after object\nGroup { }\n`;
  const p = del(t, 'Transform');
  assertLossless(p);
  assert.equal(p.newText, `${H}# before A\nGroup { } # after A\n# between\n# after object\nGroup { }\n`);
});

test('Delete: inline items, only item in a list, comment beside the node, last line without newline, CRLF', () => {
  const inline = `${H}Group { children [ Shape { } Shape { appearance NULL } ] }\n`;
  assert.equal(del(inline, 'Shape', 0).newText, `${H}Group { children [ Shape { appearance NULL } ] }\n`);
  assert.equal(del(inline, 'Shape', 1).newText, `${H}Group { children [ Shape { } ] }\n`);
  const only = `${H}Group { children [Shape { }] }\n`;
  assert.equal(del(only, 'Shape').newText, `${H}Group { children [] }\n`);
  const beside = `${H}Group { }\nShape { } # note about the shape\n`;
  assert.equal(del(beside, 'Shape').newText, `${H}Group { }\n # note about the shape\n`);
  const eof = `${H}Group { }\nShape { }`;
  assert.equal(del(eof, 'Shape').newText, `${H}Group { }`);
  const crlf = '#VRML V2.0 utf8\r\nGroup { }\r\nShape {\r\n}\r\nGroup { }\r\n';
  assert.equal(del(crlf, 'Shape').newText, '#VRML V2.0 utf8\r\nGroup { }\r\nGroup { }\r\n');
  for (const [t, ty, n] of [[inline, 'Shape', 0], [inline, 'Shape', 1], [only, 'Shape', 0], [beside, 'Shape', 0], [eof, 'Shape', 0], [crlf, 'Shape', 0]]) {
    assertLossless(del(t, ty, n));
  }
});

test('Delete refusals: referenced by USE / ROUTE (names reported); PROTO body; SFNode value', () => {
  const used = del(`${H}DEF Lamp Transform { }\nGroup { children [ USE Lamp ] }\n`, 'Transform');
  assert.equal(used.reason, R.REFERENCED);
  assert.deepEqual([...used.names], ['Lamp']);
  const inner = del(`${H}Group { children [ DEF Spin TimeSensor { } ] }\nDEF R OrientationInterpolator { }\nROUTE Spin.fraction_changed TO R.set_fraction\n`, 'Group');
  assert.equal(inner.reason, R.REFERENCED);
  assert.deepEqual([...inner.names], ['Spin']);
  assert.equal(del(`${H}PROTO P [ ] { Group { } }\n`, 'Group').reason, R.IN_PROTO);
  assert.equal(del(`${H}Shape { geometry Box { } }\n`, 'Box').reason, R.NOT_IN_NODE_LIST);
});

test('Delete allowed: DEF nobody references; a node that only USEs an outer DEF; a ROUTE wholly inside', () => {
  assertLossless(del(`${H}DEF Unused Transform { }\nGroup { }\n`, 'Transform'));
  assertLossless(del(`${H}DEF M Material { }\nShape { appearance Appearance { material USE M } }\n`, 'Shape'));
  const p = del(`${H}Group { children [ DEF T TimeSensor { } ] ROUTE T.isActive TO T.set_enabled }\nGroup { }\n`, 'Group', 0);
  assertLossless(p);
  assert.equal(p.newText, `${H}Group { }\n`);
});

// ---------------------------------------------------------------------------
// Identity: twins, insertion before/after, deleted node, zero wrong
// ---------------------------------------------------------------------------

// Tier 1 re-anchor of an old node through a plan's receipt (what the editor
// does for every selection that is not the inserted/deleted one).
function reanchor(plan, oldNode) {
  const oldS = sess(plan.oldText);
  const target = nodes(oldS).find((n) => n.range.start.offset === oldNode.range.start.offset && n.range.end.offset === oldNode.range.end.offset);
  const a = identity.createTransactionAnchor(oldS, target);
  const next = sess(plan.newText);
  // A fresh parse of oldText is a different session; the receipt binds TEXT, so
  // the anchor resolves against it the same way the editor's session does.
  return { r: identity.resolveTransactionAnchor(a.anchor, next, plan.receipt), next };
}

const TWINS = `${H}Group { children [\n  Transform { translation 0 0 0 }\n  Transform { translation 0 0 0 }\n] }\n`;

test('twins: delete the FIRST -> a selection on the second follows it; the first is lost (never the survivor)', () => {
  const s = sess(TWINS);
  const [a, b] = nodes(s, 'Transform');
  const p = se.planDeleteNode({ session: s, currentText: TWINS, node: a });
  assertLossless(p);
  const rb = reanchor(p, b);
  assert.equal(rb.r.status, 'resolved');
  assert.equal(rb.r.node, nodes(rb.next, 'Transform')[0], 'the old second twin is the only remaining one');
  assert.notEqual(reanchor(p, a).r.status, 'resolved', 'the deleted twin is lost, not moved onto its lookalike');
});

test('twins: delete the SECOND -> the first stays the first; the second is lost', () => {
  const s = sess(TWINS);
  const [a, b] = nodes(s, 'Transform');
  const p = se.planDeleteNode({ session: s, currentText: TWINS, node: b });
  assert.equal(p.newText, `${H}Group { children [\n  Transform { translation 0 0 0 }\n] }\n`);
  const ra = reanchor(p, a);
  assert.equal(ra.r.node, nodes(ra.next, 'Transform')[0]);
  assert.notEqual(reanchor(p, b).r.status, 'resolved');
});

test('twins: duplicate the second -> the copy is the third; both originals keep their identity', () => {
  const s = sess(TWINS);
  const [a, b] = nodes(s, 'Transform');
  const p = se.planDuplicateNode({ session: s, currentText: TWINS, node: b });
  const next = sess(p.newText);
  const all = nodes(next, 'Transform');
  assert.equal(all.length, 3);
  assert.equal(se.resolveInsertedNode({ session: next, receipt: p.receipt, span: p.select.span, nodeType: 'Transform' }).node, all[2]);
  assert.equal(reanchor(p, a).r.node.range.start.offset, all[0].range.start.offset);
  assert.equal(reanchor(p, b).r.node.range.start.offset, all[1].range.start.offset);
});

test('insertion before/after identical siblings never re-anchors onto the new lookalike', () => {
  const t = `${H}${BOX}\n`;
  const s = sess(t);
  const orig = nodes(s, 'Transform')[0];
  const p = se.planInsertObject({ session: s, currentText: t, primitive: 'Box' });
  const { r, next } = reanchor(p, orig);
  assert.equal(r.node, nodes(next, 'Transform')[0], 'the original stays the first');
  const ins = se.resolveInsertedNode({ session: next, receipt: p.receipt, span: p.select.span, nodeType: 'Transform' });
  assert.equal(ins.node, nodes(next, 'Transform')[1]);
  assert.notEqual(ins.node, r.node);
});

// ---------------------------------------------------------------------------
// Adversarial sweep: every operation on every node of a hostile document
// ---------------------------------------------------------------------------

const ADVERSARIAL = [
  '#VRML V2.0 utf8',
  '# a header comment',
  'PROTO Lamp [ field SFColor tint 1 1 1 ] {',
  '  Transform { children [ Shape { appearance Appearance { material Material { diffuseColor IS tint } } geometry Sphere { } } ] }',
  '}',
  'EXTERNPROTO Far [ ] "far.wrl"',
  'DEF Root Group {',
  '  children [',
  '    # twin A',
  '    Transform { translation 1,0,0 children [ Shape { geometry Box { size 1e0 1E0 .1e+1 } } ] }',
  '    # twin B',
  '    Transform { translation 1,0,0 children [ Shape { geometry Box { size 1e0 1E0 .1e+1 } } ] }',
  '    Transform {   # looks like a generated object',
  '      children [',
  '        Shape {',
  '          appearance Appearance {',
  '            material Material {',
  '            }',
  '          }',
  '          geometry Box {',
  '          }',
  '        }',
  '      ]',
  '    }',
  '    DEF Shared Shape{geometry Cone{}}',
  '    Group{children[USE Shared,Group{}]}',
  '  ]',
  '}',
  'Lamp { tint 1 0 0 }',
  'DEF Clock TimeSensor { loop TRUE }',
  'ROUTE Clock.isActive TO Clock.set_enabled',
  'Transform{children[Shape{}]}',
].join('\n');

test('adversarial sweep: every Duplicate/Delete/absent-field on every node is lossless or a stable refusal; 0 wrong re-anchors', () => {
  const s = sess(ADVERSARIAL);
  assert.equal(s.parse.syntaxDiagnostics.filter((d) => d.severity === 'error').length, 0);
  const all = nodes(s);
  const reasons = new Set(Object.values(R).concat(Object.values(vrml.fieldEdit.FIELD_EDIT_REASON)));
  let ready = 0;
  let refused = 0;
  let proven = 0;
  let wrong = 0;
  for (const node of all) {
    const plans = [
      se.planDuplicateNode({ session: s, currentText: ADVERSARIAL, node }),
      se.planDeleteNode({ session: s, currentText: ADVERSARIAL, node }),
      se.planFieldInsert({ session: s, currentText: ADVERSARIAL, node, fieldName: 'translation', components: ['9', '9', '9'] }),
    ];
    for (const p of plans) {
      if (p.status !== 'ready') {
        refused += 1;
        assert.ok(reasons.has(p.reason), `stable reason id: ${p.reason}`);
        assert.equal(p.newText, undefined);
        continue;
      }
      ready += 1;
      assertLossless(p);
      // Every OTHER node of the old document: resolved -> it must be the same
      // bytes and type at the mapped place (proven), or lost. Never different.
      for (const other of all) {
        if (other === node && p.operation === 'delete') continue;
        const { r } = reanchor(p, other);
        if (r.status !== 'resolved') continue;
        // Independent byte oracle: the node's old text with exactly the
        // plan's INTERIOR edits applied (shifted to node-local offsets) must be
        // the resolved node's new text. Edits outside the node change nothing.
        const os = other.range.start.offset;
        const oe = other.range.end.offset;
        let expected = '';
        let cur = os;
        for (const e of p.edits) {
          if (e.from > os && e.to < oe) { expected += ADVERSARIAL.slice(cur, e.from) + e.insert; cur = e.to; }
        }
        expected += ADVERSARIAL.slice(cur, oe);
        if (r.node.nodeType !== other.nodeType || slice(p.newText, r.node) !== expected) wrong += 1;
        else proven += 1;
      }
    }
  }
  assert.ok(ready >= 10, `exercised ${ready} ready plans`);
  assert.ok(refused >= 10, `exercised ${refused} refusals`);
  assert.ok(proven > 100, `proven re-anchors: ${proven}`);
  assert.equal(wrong, 0, 'zero wrong re-anchors');
});

test('adversarial: the generated-looking object is recognised by structure, and its DEF-free twin duplicates', () => {
  const s = sess(ADVERSARIAL);
  const ts = nodes(s, 'Transform');
  const lookalike = ts.find((n) => slice(ADVERSARIAL, n).startsWith('Transform {   # looks'));
  const p = se.planDuplicateNode({ session: s, currentText: ADVERSARIAL, node: lookalike });
  assertLossless(p);
  const twinA = ts.find((n) => slice(ADVERSARIAL, n).startsWith('Transform { translation 1,0,0'));
  assert.equal(se.planDuplicateNode({ session: s, currentText: ADVERSARIAL, node: twinA }).status, 'ready');
  // Inside DEF Root's children the Group with USE Shared refuses Duplicate.
  const useGroup = nodes(s, 'Group').find((n) => slice(ADVERSARIAL, n).startsWith('Group{children[USE'));
  assert.equal(se.planDuplicateNode({ session: s, currentText: ADVERSARIAL, node: useGroup }).reason, R.CONTAINS_USE);
  // DEF Shared is referenced by that USE: Delete refuses.
  const shared = nodes(s, 'Shape').find((n) => n.def === 'Shared');
  assert.deepEqual([...se.planDeleteNode({ session: s, currentText: ADVERSARIAL, node: shared }).names], ['Shared']);
  // DEF Clock is a ROUTE endpoint: Delete refuses.
  const clock = nodes(s, 'TimeSensor')[0];
  assert.equal(se.planDeleteNode({ session: s, currentText: ADVERSARIAL, node: clock }).reason, R.REFERENCED);
});

// ---------------------------------------------------------------------------
// Architecture
// ---------------------------------------------------------------------------

test('architecture: structure-edit is pure, serializes nothing, and never emits DEF', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'vrml', 'structure-edit.js'), 'utf8');
  for (const forbidden of [/require\(['"](fs|path|electron|child_process)['"]\)/, /JSON\.stringify/, /\bserializ\w*\(/i, /\bdocument\.\w/, /window\./]) {
    assert.equal(forbidden.test(src), false, `forbidden: ${forbidden}`);
  }
  assert.equal(/['"`]DEF /.test(src), false, 'no DEF text is ever generated');
  for (const p of [add(''), add('', 'Sphere'), dup(`${H}Group { }\n`, 'Group')]) {
    assert.equal(/\bDEF\b/.test(p.newText), false);
  }
});

test('determinism: equal inputs give equal plans', () => {
  const a = add(`${H}Group { }\n`);
  const b = add(`${H}Group { }\n`);
  assert.equal(a.newText, b.newText);
  assert.deepEqual(a.edits, b.edits);
  assert.deepEqual(a.select, b.select);
});
