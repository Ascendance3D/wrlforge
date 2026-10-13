'use strict';
// WD2-B -- the pure typed field-edit model (src/vrml/field-edit.js).
//
// Every test goes through the production entry points: parse -> parse session
// -> inspectNodeFields / planFieldEdit. Field types are asserted AGAINST THE
// SCHEMA (never hardcoded as facts), and every successful edit is checked byte
// for byte: everything outside the declared edit spans must be identical.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const vrml = require('../../src/vrml');
const fe = require('../../src/vrml/field-edit');
const tx = require('../../src/vrml/document-transaction');
const schema = require('../../src/vrml/node-schema');
const { NODE, walk } = require('../../src/vrml/ast');

const H = '#VRML V2.0 utf8\n';
const R = fe.FIELD_EDIT_REASON;

function open(text) {
  const parseResult = vrml.parse(text);
  return { text, parseResult, session: tx.createParseSession(text, parseResult) };
}

// The nth node instance of a type, in document order (walk order).
function nodeOf(doc, nodeType, nth = 0) {
  const found = [];
  walk(doc.parseResult.tree, (n) => { if (n.type === NODE.NODE && n.nodeType === nodeType) found.push(n); });
  assert.ok(found[nth], `fixture has ${nodeType}[${nth}]`);
  return found[nth];
}

function fieldIndex(node, name) {
  const i = node.fields.findIndex((f) => f.type === NODE.FIELD && f.name === name);
  assert.ok(i >= 0, `field ${name} is authored`);
  return i;
}

function plan(doc, nodeType, fieldName, components, nth = 0, currentText = doc.text) {
  const node = nodeOf(doc, nodeType, nth);
  return fe.planFieldEdit({
    session: doc.session, currentText, node, fieldIndex: fieldIndex(node, fieldName), fieldName, components,
  });
}

// Every byte outside the edit spans is identical, and each span holds exactly
// its insert. Walks old and new text in lockstep over the canonical edits.
function assertOnlySpansChanged(oldText, newText, edits) {
  let oldCursor = 0;
  let newCursor = 0;
  for (const e of edits) {
    const gap = oldText.slice(oldCursor, e.from);
    assert.equal(newText.slice(newCursor, newCursor + gap.length), gap, 'bytes before an edit span are untouched');
    newCursor += gap.length;
    assert.equal(newText.slice(newCursor, newCursor + e.insert.length), e.insert, 'span holds exactly its insert');
    newCursor += e.insert.length;
    oldCursor = e.to;
  }
  assert.equal(newText.slice(newCursor), oldText.slice(oldCursor), 'bytes after the last span are untouched');
}

function assertReady(res) {
  assert.equal(res.status, fe.PLAN_STATUS.READY, `expected ready, got ${res.status} ${res.reason || ''}`);
  assert.equal(tx.isVerifiedReceipt(res.receipt), true, 'a ready plan carries a verified receipt');
  assertOnlySpansChanged(res.oldText, res.newText, res.edits);
}

// ---------------------------------------------------------------------------
// Supported-type matrix (real VRML97 built-in fields; types from the schema)
// ---------------------------------------------------------------------------

const TYPE_CASES = [
  { type: 'SFBool', node: 'DirectionalLight', field: 'on', src: 'DirectionalLight { on TRUE }', input: [false], expect: 'DirectionalLight { on FALSE }' },
  { type: 'SFInt32', node: 'Switch', field: 'whichChoice', src: 'Switch { whichChoice -1 }', input: ['0'], expect: 'Switch { whichChoice 0 }' },
  { type: 'SFFloat', node: 'Sphere', field: 'radius', src: 'Sphere { radius 1 }', input: ['2.5'], expect: 'Sphere { radius 2.5 }' },
  { type: 'SFTime', node: 'TimeSensor', field: 'cycleInterval', src: 'TimeSensor { cycleInterval 1 }', input: ['4.25'], expect: 'TimeSensor { cycleInterval 4.25 }' },
  { type: 'SFVec2f', node: 'TextureTransform', field: 'translation', src: 'TextureTransform { translation 0 0 }', input: ['0.5', '0'], expect: 'TextureTransform { translation 0.5 0 }' },
  { type: 'SFVec3f', node: 'Transform', field: 'translation', src: 'Transform { translation 0 0 0 }', input: ['3', '0', '0'], expect: 'Transform { translation 3 0 0 }' },
  { type: 'SFColor', node: 'Material', field: 'diffuseColor', src: 'Material { diffuseColor 0.8 0.8 0.8 }', input: ['1', '0', '0.8'], expect: 'Material { diffuseColor 1 0 0.8 }' },
  { type: 'SFRotation', node: 'Transform', field: 'rotation', src: 'Transform { rotation 0 0 1 0 }', input: ['0', '1', '0', '1.5708'], expect: 'Transform { rotation 0 1 0 1.5708 }' },
  { type: 'SFString', node: 'WorldInfo', field: 'title', src: 'WorldInfo { title "Old" }', input: ['New "quoted" \\ title'], expect: 'WorldInfo { title "New \\"quoted\\" \\\\ title" }' },
];

for (const c of TYPE_CASES) {
  test(`type ${c.type}: ${c.node}.${c.field} is editable per the schema and patches exactly`, () => {
    // Schema is the type authority -- assert it says what the case assumes.
    const rec = schema.getFieldSchema(c.node, c.field);
    assert.equal(rec.type, c.type);
    assert.ok(rec.profiles.includes('vrml97'));
    const doc = open(H + c.src + '\n');
    const info = fe.inspectNodeFields(doc.session, nodeOf(doc, c.node), { currentText: doc.text });
    const fd = info.fields.find((f) => f.name === c.field);
    assert.equal(fd.type, c.type);
    assert.equal(fd.editable, true);
    assert.equal(fd.components.length, fe.TYPE_SPECS[c.type].arity);
    const res = plan(doc, c.node, c.field, c.input);
    assertReady(res);
    assert.equal(res.newText, H + c.expect + '\n');
    // The new text parses and reads back the intended value.
    const again = open(res.newText);
    const fd2 = fe.inspectNodeFields(again.session, nodeOf(again, c.node)).fields.find((f) => f.name === c.field);
    assert.deepEqual(fd2.components.map((x) => x.value),
      c.input.map((v) => (typeof v === 'string' && c.type !== 'SFString' ? Number(v) : v)));
  });
}

test('EDITABLE_TYPES is exactly the nine WD2-B types', () => {
  assert.deepEqual([...fe.EDITABLE_TYPES].sort(),
    ['SFBool', 'SFColor', 'SFFloat', 'SFInt32', 'SFRotation', 'SFString', 'SFTime', 'SFVec2f', 'SFVec3f']);
});

test('SFInt32 accepts a hex literal and writes it verbatim', () => {
  const doc = open(H + 'Switch { whichChoice 0 }\n');
  const res = plan(doc, 'Switch', 'whichChoice', ['0x1F']);
  assertReady(res);
  assert.ok(res.newText.includes('whichChoice 0x1F'));
});

test('only the changed component of a compound value is patched', () => {
  const doc = open(H + 'Transform { translation 1 2 3 }\n');
  const res = plan(doc, 'Transform', 'translation', ['1', '9', '3']);
  assertReady(res);
  assert.equal(res.edits.length, 1);
  assert.deepEqual([...res.changed], [1]);
  assert.equal(res.oldText.slice(res.edits[0].from, res.edits[0].to), '2');
});

test('several changed components become one non-overlapping edit set', () => {
  const doc = open(H + 'Transform { translation 1 2 3 }\n');
  const res = plan(doc, 'Transform', 'translation', ['4', '5', '3']);
  assertReady(res);
  assert.equal(res.edits.length, 2);
  assert.ok(res.edits[0].to <= res.edits[1].from);
  assert.ok(res.newText.includes('translation 4 5 3'));
});

test('an unchanged value produces no edit', () => {
  const doc = open(H + 'Transform { translation 1 2 3 }\n');
  assert.equal(plan(doc, 'Transform', 'translation', ['1', ' 2 ', 3]).status, fe.PLAN_STATUS.UNCHANGED);
  const w = open(H + 'WorldInfo { title "a" }\n');
  assert.equal(plan(w, 'WorldInfo', 'title', ['a']).status, fe.PLAN_STATUS.UNCHANGED);
});

test('planning is deterministic', () => {
  const doc = open(H + 'Transform { translation 1 2 3 }\n');
  const a = plan(doc, 'Transform', 'translation', ['7', '2', '-1e3']);
  const b = plan(doc, 'Transform', 'translation', ['7', '2', '-1e3']);
  assert.deepEqual(a.edits, b.edits);
  assert.equal(a.newText, b.newText);
});

// ---------------------------------------------------------------------------
// Lossless edit matrix (A-L)
// ---------------------------------------------------------------------------

test('A: comments between vector components survive', () => {
  const src = H + 'Transform {\n  translation 1   # keep this comment\n              2 3\n}\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['4', '2', '3']);
  assertReady(res);
  assert.equal(res.newText, src.replace('translation 1 ', 'translation 4 '));
  assert.ok(res.newText.includes('# keep this comment\n              2 3'));
});

test('B: unusual whitespace survives', () => {
  const src = H + 'Transform {\ttranslation\t1\t\t 2    \t3 }\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['1', '2', '8']);
  assertReady(res);
  assert.equal(res.newText, H + 'Transform {\ttranslation\t1\t\t 2    \t8 }\n');
});

test('C: commas-as-whitespace survive', () => {
  const src = H + 'Material { diffuseColor 1, 0 ,0.5 }\n';
  const doc = open(src);
  const res = plan(doc, 'Material', 'diffuseColor', ['1', '0.25', '0.5']);
  assertReady(res);
  assert.equal(res.newText, H + 'Material { diffuseColor 1, 0.25 ,0.5 }\n');
});

test('D: a CRLF document keeps every CRLF', () => {
  const src = '#VRML V2.0 utf8\r\nTransform {\r\n  translation 0 0 0\r\n  children [ Shape { geometry Box { size 2 2 2 } } ]\r\n}\r\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['3', '0', '0']);
  assertReady(res);
  assert.equal(res.newText, src.replace('translation 0 0 0', 'translation 3 0 0'));
  assert.equal((res.newText.match(/\r\n/g) || []).length, (src.match(/\r\n/g) || []).length);
  assert.equal((res.newText.match(/(?<!\r)\n/g) || []).length, 0, 'no bare LF introduced');
});

test('E: scientific/exponent spelling of untouched components survives', () => {
  const src = H + 'Transform { translation 1e0 2.5E+1 -3.0e-2 }\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['7', '2.5E+1', '-3.0e-2']);
  assertReady(res);
  assert.equal(res.newText, H + 'Transform { translation 7 2.5E+1 -3.0e-2 }\n');
});

test('F: comments before and after the field survive', () => {
  const src = H + 'Transform {\n  # before\n  translation 0 0 0 # after\n  # trailing\n}\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['0', '0', '5']);
  assertReady(res);
  assert.equal(res.newText, src.replace('translation 0 0 0', 'translation 0 0 5'));
});

test('G: unrelated vendor/Cybertown syntax elsewhere is untouched', () => {
  const src = H
    + 'Group { children [ DEF door-1 Transform { translation 0 0 0 } ROUTE door-1.translation_changed TO door-1.set_translation ] }\n'
    + 'BlaxxunZone { foo 1 bar "x" }\n'
    + 'Transform { translation 1 1 1 }\n';
  const doc = open(src);
  assert.deepEqual(doc.parseResult.syntaxDiagnostics, []);
  const res = plan(doc, 'Transform', 'translation', ['1', '1', '2'], 1);
  assertReady(res);
  assert.equal(res.newText, src.replace('Transform { translation 1 1 1 }', 'Transform { translation 1 1 2 }'));
  // The vendor node is read-only (not standard VRML97).
  const vendor = fe.inspectNodeFields(doc.session, nodeOf(doc, 'BlaxxunZone'));
  assert.equal(vendor.reason, R.UNKNOWN_NODE_TYPE);
  assert.ok(vendor.fields.every((f) => !f.editable));
});

test('H: multiple same-type fields on one node -- only the chosen one changes', () => {
  const src = H + 'Transform { translation 1 1 1 center 1 1 1 scale 1 1 1 }\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'center', ['1', '2', '1']);
  assertReady(res);
  assert.equal(res.newText, H + 'Transform { translation 1 1 1 center 1 2 1 scale 1 1 1 }\n');
});

test('I: two visually identical siblings -- the second is edited, the first is not', () => {
  const src = H + 'Group { children [\n  Transform { translation 0 0 0 }\n  Transform { translation 0 0 0 }\n] }\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['5', '0', '0'], 1);
  assertReady(res);
  assert.equal(res.newText, H + 'Group { children [\n  Transform { translation 0 0 0 }\n  Transform { translation 5 0 0 }\n] }\n');
});

test('J: nested Transform -- the inner node is edited', () => {
  const src = H + 'Transform { translation 1 1 1 children [ Transform { translation 2 2 2 } ] }\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['2', '2', '9'], 1);
  assertReady(res);
  assert.equal(res.newText, H + 'Transform { translation 1 1 1 children [ Transform { translation 2 2 9 } ] }\n');
});

test('K: a DEF node is edited without touching the DEF', () => {
  const src = H + 'DEF Mover Transform { translation 0 0 0 }\n';
  const doc = open(src);
  const res = plan(doc, 'Transform', 'translation', ['0', '4', '0']);
  assertReady(res);
  assert.equal(res.newText, H + 'DEF Mover Transform { translation 0 4 0 }\n');
});

test('L: a built-in node inside a PROTO body is editable; its IS-bound field is not', () => {
  const src = H + 'PROTO Mover [ field SFVec3f t 0 0 0 ] {\n  Transform { translation 1 2 3 scale IS t }\n}\nMover { }\n';
  const doc = open(src);
  const tf = nodeOf(doc, 'Transform');
  const info = fe.inspectNodeFields(doc.session, tf, { currentText: doc.text });
  assert.equal(info.status, fe.FIELD_EDIT_STATUS.EDITABLE);
  assert.equal(info.fields.find((f) => f.name === 'translation').editable, true);
  const scale = info.fields.find((f) => f.name === 'scale');
  assert.equal(scale.editable, false);
  assert.equal(scale.reason, R.IS_BOUND);
  const res = plan(doc, 'Transform', 'translation', ['1', '2', '4']);
  assertReady(res);
  assert.equal(res.newText, src.replace('translation 1 2 3', 'translation 1 2 4'));
  const isRes = fe.planFieldEdit({ session: doc.session, currentText: doc.text, node: tf, fieldIndex: fieldIndex(tf, 'scale'), fieldName: 'scale', components: ['1', '1', '1'] });
  assert.equal(isRes.status, fe.PLAN_STATUS.REFUSED);
  assert.equal(isRes.reason, R.IS_BOUND);
});

// ---------------------------------------------------------------------------
// Read-only / refusal matrix
// ---------------------------------------------------------------------------

function refusedWith(res, reason) {
  assert.equal(res.status, fe.PLAN_STATUS.REFUSED, `expected refusal ${reason}, got ${res.status}`);
  assert.equal(res.reason, reason);
  assert.equal(res.edits, undefined, 'a refusal carries no edits');
  assert.equal(res.newText, undefined, 'a refusal carries no new text');
}

const STATEMENTS = H
  + 'PROTO P [ field SFFloat r 1 ] { Sphere { radius IS r } }\n'
  + 'EXTERNPROTO E [ field SFFloat r ] "e.wrl"\n'
  + 'DEF S Shape { geometry Box { size 1 1 1 } }\n'
  + 'Transform { children [ USE S ] }\n'
  + 'ROUTE S.foo TO S.bar\n';

test('refusal: Document root, USE, ROUTE, PROTO and EXTERNPROTO are not node instances', () => {
  const doc = open(STATEMENTS);
  const tree = doc.parseResult.tree;
  const findType = (t) => { let hit = null; walk(tree, (n) => { if (!hit && n.type === t) hit = n; }); return hit; };
  for (const astNode of [tree, findType(NODE.USE), findType(NODE.ROUTE), findType(NODE.PROTO), findType(NODE.EXTERNPROTO)]) {
    assert.ok(astNode);
    refusedWith(fe.planFieldEdit({ session: doc.session, currentText: doc.text, node: astNode, fieldIndex: 0, fieldName: 'x', components: ['1'] }), R.NOT_A_NODE);
    const info = fe.inspectNodeFields(doc.session, astNode);
    assert.equal(info.reason, R.NOT_A_NODE);
    assert.equal(info.fields.length, 0);
  }
});

test('refusal: MF*, SFNode and MFNode values are read-only', () => {
  const doc = open(H + 'Transform { children [ Shape { geometry IndexedFaceSet { coordIndex [ 0 1 2 -1 ] } } ] }\n');
  const tf = fe.inspectNodeFields(doc.session, nodeOf(doc, 'Transform'));
  assert.equal(tf.fields.find((f) => f.name === 'children').reason, R.TYPE_UNSUPPORTED);
  const shape = fe.inspectNodeFields(doc.session, nodeOf(doc, 'Shape'));
  assert.equal(shape.fields.find((f) => f.name === 'geometry').reason, R.TYPE_UNSUPPORTED);
  const ifs = fe.inspectNodeFields(doc.session, nodeOf(doc, 'IndexedFaceSet'));
  assert.equal(ifs.fields.find((f) => f.name === 'coordIndex').reason, R.TYPE_UNSUPPORTED);
  refusedWith(plan(doc, 'IndexedFaceSet', 'coordIndex', ['1']), R.TYPE_UNSUPPORTED);
  refusedWith(plan(doc, 'Shape', 'geometry', ['1']), R.TYPE_UNSUPPORTED);
  refusedWith(plan(doc, 'Transform', 'children', ['1']), R.TYPE_UNSUPPORTED);
});

test('refusal: an absent/default field is not listed and cannot be planned', () => {
  const doc = open(H + 'Transform { translation 0 0 0 }\n');
  const tf = nodeOf(doc, 'Transform');
  const info = fe.inspectNodeFields(doc.session, tf);
  assert.deepEqual(info.fields.map((f) => f.name), ['translation']);
  refusedWith(fe.planFieldEdit({ session: doc.session, currentText: doc.text, node: tf, fieldIndex: 1, fieldName: 'scale', components: ['1', '1', '1'] }), R.FIELD_NOT_PRESENT);
  refusedWith(fe.planFieldEdit({ session: doc.session, currentText: doc.text, node: tf, fieldIndex: 0, fieldName: 'scale', components: ['1', '1', '1'] }), R.FIELD_NOT_PRESENT);
});

test('refusal: X3D-only fields are never offered as VRML97 fields', () => {
  assert.equal(schema.isVRML97Field('Transform', 'visible'), false);
  assert.equal(schema.isVRML97Field('Viewpoint', 'centerOfRotation'), false);
  const doc = open(H + 'Transform { visible FALSE }\nViewpoint { centerOfRotation 0 0 0 }\n');
  refusedWith(plan(doc, 'Transform', 'visible', [true]), R.FIELD_X3D_ONLY);
  refusedWith(plan(doc, 'Viewpoint', 'centerOfRotation', ['1', '0', '0']), R.FIELD_X3D_ONLY);
  const info = fe.inspectNodeFields(doc.session, nodeOf(doc, 'Transform'));
  assert.equal(info.fields[0].type, null, 'no VRML97 type is reported for an X3D-only field');
});

test('refusal: a damaged document (syntax error anywhere) makes every field read-only', () => {
  const doc = open(H + 'Transform { translation 0 0 0 }\nGroup { children [ Shape {\n');
  assert.ok(doc.parseResult.syntaxDiagnostics.some((d) => d.severity === 'error'));
  const info = fe.inspectNodeFields(doc.session, nodeOf(doc, 'Transform'));
  assert.equal(info.reason, R.SYNTAX_ERRORS);
  assert.ok(info.fields.every((f) => !f.editable && f.reason === R.SYNTAX_ERRORS));
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '0', '0']), R.SYNTAX_ERRORS);
});

test('refusal: an invalid number literal in the value is a syntax error -> read-only', () => {
  const doc = open(H + 'Transform { translation 1 2 3x }\n');
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '2', '3']), R.SYNTAX_ERRORS);
});

test('refusal: a missing header alone does not block editing', () => {
  const doc = open('Transform { translation 0 0 0 }\n');
  assert.ok(doc.parseResult.syntaxDiagnostics.some((d) => d.code === 'VRML001'));
  assertReady(plan(doc, 'Transform', 'translation', ['1', '0', '0']));
});

test('refusal: value shape that does not match the schema type is read-only', () => {
  const doc = open(H + 'Transform { translation 1 2 }\nSphere { radius 0x10 }\nWorldInfo { title 3 }\nDirectionalLight { on 1 }\n');
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '2']), R.VALUE_SHAPE);
  refusedWith(plan(doc, 'Sphere', 'radius', ['1']), R.VALUE_TOKEN_INVALID);
  refusedWith(plan(doc, 'WorldInfo', 'title', ['x']), R.VALUE_SHAPE);
  refusedWith(plan(doc, 'DirectionalLight', 'on', [true]), R.VALUE_SHAPE);
});

test('refusal: a value without an exact authoritative span is read-only', () => {
  const text = H + 'Transform { translation 1 2 3 }\n';
  const parseResult = vrml.parse(text);
  let target = null;
  walk(parseResult.tree, (n) => { if (n.type === NODE.FIELD && n.name === 'translation') target = n; });
  delete target.value.values[1].range; // damage BEFORE the session binds the parse
  const doc = { text, parseResult, session: tx.createParseSession(text, parseResult) };
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '5', '3']), R.VALUE_SHAPE);
});

test('refusal: IS-bound, duplicated, unknown and event fields are read-only', () => {
  const doc = open(H
    + 'PROTO Q [ field SFFloat r 1 ] { Sphere { radius IS r } }\n'
    + 'Transform { translation 1 1 1 translation 2 2 2 }\n'
    + 'Box { size 1 1 1 bogus 3 }\n'
    + 'TimeSensor { cycleTime 0 }\n');
  refusedWith(plan(doc, 'Sphere', 'radius', ['2']), R.IS_BOUND);
  refusedWith(plan(doc, 'Transform', 'translation', ['5', '1', '1']), R.FIELD_DUPLICATED);
  refusedWith(plan(doc, 'Box', 'bogus', ['1']), R.FIELD_UNKNOWN);
  assert.equal(schema.getFieldSchema('TimeSensor', 'cycleTime').vrml97Declaration, 'eventOut');
  refusedWith(plan(doc, 'TimeSensor', 'cycleTime', ['1']), R.FIELD_NOT_STORED);
});

test('refusal: unknown/vendor node types and PROTO-named types are read-only', () => {
  const doc = open(H + 'Spinner { speed 1 }\nPROTO Transform [ field SFVec3f translation 0 0 0 ] { Group { } }\nTransform { translation 1 2 3 }\n');
  refusedWith(plan(doc, 'Spinner', 'speed', ['2']), R.UNKNOWN_NODE_TYPE);
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '2', '4']), R.PROTO_INSTANCE);
});

test('refusal: a stale selection (old session, or a node from another parse)', () => {
  const doc = open(H + 'Transform { translation 0 0 0 }\n');
  const changed = doc.text.replace('0 0 0', '0 0 1');
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '0', '0'], 0, changed), R.STALE_SESSION);
  const other = open(doc.text);
  const foreign = nodeOf(other, 'Transform');
  refusedWith(fe.planFieldEdit({ session: doc.session, currentText: doc.text, node: foreign, fieldIndex: 0, fieldName: 'translation', components: ['1', '0', '0'] }), R.NODE_NOT_IN_SESSION);
  refusedWith(fe.planFieldEdit({ session: { text: doc.text, parse: doc.parseResult }, currentText: doc.text, node: foreign, fieldIndex: 0, fieldName: 'translation', components: ['1', '0', '0'] }), R.STALE_SESSION);
});

test('refusal: a patch that would not round-trip through the parser is not dispatched', () => {
  // "1-2" is two number tokens with NO trivia between them. Rewriting the
  // second to "2" would fuse them into "12" -- the round-trip check refuses.
  const doc = open(H + 'Transform { translation 1-2 3 }\n');
  assert.deepEqual(fe.inspectNodeFields(doc.session, nodeOf(doc, 'Transform')).fields[0].components.map((c) => c.text), ['1', '-2', '3']);
  refusedWith(plan(doc, 'Transform', 'translation', ['1', '2', '3']), R.ROUND_TRIP_FAILED);
});

// ---------------------------------------------------------------------------
// Input validation (no clamping; the source never changes on refusal)
// ---------------------------------------------------------------------------

test('numeric input: empty, garbage, NaN, Infinity, non-finite are refused', () => {
  const doc = open(H + 'Transform { translation 0 0 0 }\n');
  const cases = [
    ['', R.INPUT_NOT_NUMBER], ['abc', R.INPUT_NOT_NUMBER], ['NaN', R.INPUT_NOT_NUMBER],
    ['Infinity', R.INPUT_NOT_NUMBER], ['-Infinity', R.INPUT_NOT_NUMBER], ['1x', R.INPUT_NOT_NUMBER],
    ['1 2', R.INPUT_NOT_NUMBER], ['0x10', R.INPUT_NOT_NUMBER], ['1e', R.INPUT_NOT_NUMBER],
    ['--1', R.INPUT_NOT_NUMBER], ['1e999', R.INPUT_NOT_FINITE], [NaN, R.INPUT_NOT_NUMBER],
    [Infinity, R.INPUT_NOT_NUMBER], [null, R.INPUT_NOT_NUMBER],
  ];
  for (const [input, reason] of cases) {
    const res = plan(doc, 'Transform', 'translation', ['0', input, '0']);
    refusedWith(res, reason);
    assert.equal(res.componentIndex, 1, `component index reported for ${String(input)}`);
    assert.ok(typeof res.message === 'string' && res.message.length > 0);
  }
});

test('SFInt32 input: fractions and out-of-32-bit values are refused, never clamped', () => {
  const doc = open(H + 'Switch { whichChoice 0 }\n');
  refusedWith(plan(doc, 'Switch', 'whichChoice', ['1.5']), R.INPUT_NOT_INTEGER);
  refusedWith(plan(doc, 'Switch', 'whichChoice', ['2147483648']), R.INPUT_OUT_OF_RANGE);
  // Schema constraint: whichChoice >= -1.
  refusedWith(plan(doc, 'Switch', 'whichChoice', ['-2']), R.INPUT_OUT_OF_RANGE);
  assertReady(plan(doc, 'Switch', 'whichChoice', ['2147483647']));
});

test('schema numeric bounds are enforced per component; symbolic bounds and notes are not invented', () => {
  const doc = open(H + 'Material { diffuseColor 0 0 0 transparency 0 }\nSphere { radius 1 }\nTransform { rotation 0 0 1 0 }\n');
  refusedWith(plan(doc, 'Material', 'diffuseColor', ['0', '1.5', '0']), R.INPUT_OUT_OF_RANGE);
  refusedWith(plan(doc, 'Material', 'diffuseColor', ['-0.1', '0', '0']), R.INPUT_OUT_OF_RANGE);
  assertReady(plan(doc, 'Material', 'diffuseColor', ['1', '1', '1']));
  refusedWith(plan(doc, 'Material', 'transparency', ['2']), R.INPUT_OUT_OF_RANGE);
  // Sphere.radius is (0, infinity): exclusive minimum.
  refusedWith(plan(doc, 'Sphere', 'radius', ['0']), R.INPUT_OUT_OF_RANGE);
  // Transform.rotation carries only a PER_COMPONENT_RANGE note -> nothing enforced.
  assert.equal(schema.getFieldConstraints('Transform', 'rotation').note.category, 'PER_COMPONENT_RANGE');
  assertReady(plan(doc, 'Transform', 'rotation', ['5', '0', '0', '99']));
});

test('SFBool and SFString input shapes', () => {
  const doc = open(H + 'DirectionalLight { on TRUE }\nWorldInfo { title "a" }\n');
  refusedWith(plan(doc, 'DirectionalLight', 'on', ['false']), R.INPUT_NOT_BOOLEAN);
  refusedWith(plan(doc, 'WorldInfo', 'title', [42]), R.INPUT_NOT_STRING);
  refusedWith(plan(doc, 'WorldInfo', 'title', ['line\r\nbreak']), R.INPUT_STRING_UNENCODABLE);
  refusedWith(plan(doc, 'WorldInfo', 'title', ['a', 'b']), R.INPUT_SHAPE);
});

test('SFString encoder round-trips through the tokenizer', () => {
  for (const s of ['', 'plain', 'with "quotes"', 'back\\slash', 'trailing\\', 'multi\nline', 'ünïcödé ✓ 🎉', '\\"']) {
    const enc = fe.encodeString(s);
    assert.ok(enc.text, `encodable: ${JSON.stringify(s)}`);
    const tok = vrml.tokenize(enc.text).tokens[0];
    assert.equal(tok.type, 'string');
    assert.equal(tok.value, s);
    assert.equal(tok.lexeme, enc.text);
  }
});

test('string edit replaces only the string token, in a multi-field node', () => {
  const src = H + 'WorldInfo {\n  info [ "keep" ]\n  title "Old"  # comment\n}\n';
  const doc = open(src);
  const res = plan(doc, 'WorldInfo', 'title', ['New']);
  assertReady(res);
  assert.equal(res.edits.length, 1);
  assert.equal(res.oldText.slice(res.edits[0].from, res.edits[0].to), '"Old"');
  assert.equal(res.newText, src.replace('"Old"', '"New"'));
});

// ---------------------------------------------------------------------------
// Architecture
// ---------------------------------------------------------------------------

test('architecture: field-edit is pure and browser-safe, and serializes nothing', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'vrml', 'field-edit.js'), 'utf8');
  const requires = [...src.matchAll(/require\(\s*['"]([^'"]+)['"]\s*\)/g)].map((m) => m[1]);
  assert.ok(requires.length > 0);
  for (const r of requires) assert.ok(r.startsWith('./'), `only sibling src/vrml modules: ${r}`);
  const code = src.replace(/\/\/.*$/gm, '');
  for (const banned of [/\bfs\b\./, /electron/, /@codemirror/, /\bdocument\./, /\bwindow\./, /serializ/i, /toVRML|printNode|stringifyNode/]) {
    assert.ok(!banned.test(code), `field-edit.js must not contain ${banned}`);
  }
});

test('architecture: the facade publishes the field-edit consumer surface only', () => {
  assert.deepEqual(Object.keys(vrml.fieldEdit).sort(),
    ['EDITABLE_TYPES', 'FIELD_EDIT_REASON', 'FIELD_EDIT_STATUS', 'PLAN_STATUS', 'inspectNodeFields', 'planFieldEdit']);
});
