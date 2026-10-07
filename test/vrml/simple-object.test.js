'use strict';
// WD2-C -- beginner simple-object facade (src/vrml/simple-object.js).

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const vrml = require('../../src/vrml');
const { walk, NODE } = require('../../src/vrml/ast');
const tx = require('../../src/vrml/document-transaction');
const fe = require('../../src/vrml/field-edit');
const so = require('../../src/vrml/simple-object');
const templates = require('../../src/vrml/node-templates');

const H = '#VRML V2.0 utf8\n';
const BOX = templates.simpleObjectTemplate('Box').text;
const SPHERE = templates.simpleObjectTemplate('Sphere').text;
const sess = (t) => tx.createParseSession(t, vrml.parse(t));
function nodes(s, type) {
  const out = [];
  walk(s.parse.tree, (n) => { if (n.type === NODE.NODE && (!type || n.nodeType === type)) out.push(n); });
  return out;
}
const describe = (t, nth = 0) => { const s = sess(t); return so.describeObject(s, nodes(s, 'Transform')[nth], { currentText: t }); };
const prop = (d, key) => d.properties.find((p) => p.key === key);

test('Box: Position / Rotation / Size / Color mapped to translation / rotation / Box.size / Material.diffuseColor', () => {
  const d = describe(`${H}${BOX}\n`);
  assert.equal(d.primitive, 'Box');
  assert.deepEqual(d.properties.map((p) => [p.label, p.nodeType, p.field, p.type]), [
    ['Position', 'Transform', 'translation', 'SFVec3f'],
    ['Rotation', 'Transform', 'rotation', 'SFRotation'],
    ['Size', 'Box', 'size', 'SFVec3f'],
    ['Color', 'Material', 'diffuseColor', 'SFColor'],
  ]);
  assert.deepEqual(d.properties.map((p) => p.values.join(' ')), ['0 0 0', '0 0 1 0', '2 2 2', '0.8 0.8 0.8']);
  assert.ok(d.properties.every((p) => p.editable && !p.present));
});

test('Sphere: Radius (not "Size") maps to the real Sphere.radius SFFloat', () => {
  const d = describe(`${H}${SPHERE}\n`);
  assert.deepEqual(d.properties.map((p) => p.label), ['Position', 'Rotation', 'Radius', 'Color']);
  const r = prop(d, 'radius');
  assert.equal(r.field, 'radius');
  assert.equal(r.type, 'SFFloat');
  assert.deepEqual([...r.values], ['1']);
  assert.equal(d.properties.some((p) => p.label === 'Size'), false);
});

test('authored values come from the same field-edit descriptors the Inspector shows (they cannot disagree)', () => {
  const t = `${H}Transform { translation 1.50 -2e0 3 children [ Shape { appearance Appearance { material Material { diffuseColor 1 .5 0 } } geometry Box { size 4 5 6 } } ] }\n`;
  const s = sess(t);
  const tr = nodes(s, 'Transform')[0];
  const d = so.describeObject(s, tr, { currentText: t });
  assert.deepEqual([...prop(d, 'position').values], ['1.50', '-2e0', '3']);
  assert.deepEqual([...prop(d, 'size').values], ['4', '5', '6']);
  assert.deepEqual([...prop(d, 'color').values], ['1', '.5', '0']);
  const inspector = fe.inspectNodeFields(s, tr, { currentText: t }).fields.find((f) => f.name === 'translation');
  assert.deepEqual(inspector.components.map((c) => c.text), [...prop(d, 'position').values]);
  assert.ok(prop(d, 'position').present);
});

test('planPropertySet: authored -> planFieldEdit token patch; absent -> planFieldInsert; same plan shape', () => {
  const authored = `${H}Transform { translation 1 2 3 children [ Shape { geometry Box { } } ] }\n`;
  const s1 = sess(authored);
  const p1 = so.planPropertySet({ session: s1, currentText: authored, node: nodes(s1, 'Transform')[0], key: 'position', components: ['9', '2', '3'] });
  assert.equal(p1.status, 'ready');
  assert.equal(p1.newText, authored.replace('translation 1 2 3', 'translation 9 2 3'));
  assert.deepEqual(p1.edits.map((e) => [e.to - e.from, e.insert]), [[1, '9']], 'only the one changed component token');
  const s2 = sess(authored);
  const p2 = so.planPropertySet({ session: s2, currentText: authored, node: nodes(s2, 'Transform')[0], key: 'size', components: ['1', '1', '1'] });
  assert.equal(p2.newText, authored.replace('Box { }', 'Box { size 1 1 1 }'));
  assert.equal(p2.property, 'size');
});

test('Color is unavailable (read-only, stated) when the object has no Material; Position still works', () => {
  const t = `${H}Transform { children [ Shape { geometry Box { } } ] }\n`;
  const d = describe(t);
  const c = prop(d, 'color');
  assert.equal(c.editable, false);
  assert.equal(c.reason, so.OBJECT_REASON.NO_MATERIAL);
  const s = sess(t);
  assert.equal(so.planPropertySet({ session: s, currentText: t, node: nodes(s, 'Transform')[0], key: 'color', components: ['1', '0', '0'] }).reason,
    so.OBJECT_REASON.NO_MATERIAL);
  assert.equal(prop(d, 'position').editable, true);
});

test('recognition is strict: anything else is NOT a simple object and keeps its real node type', () => {
  const not = [
    `${H}Transform { }\n`,
    `${H}Transform { children [ Shape { geometry Box { } } Shape { geometry Box { } } ] }\n`,
    `${H}Transform { children [ Group { } ] }\n`,
    `${H}Transform { children [ Shape { geometry Cone { } } ] }\n`,
    `${H}DEF B Box { }\nTransform { children [ Shape { geometry USE B } ] }\n`,
    `${H}Transform { children [ Shape { geometry Box { } } ] children [ ] }\n`,
    `${H}Transform { children [ Shape { geometry Box { } geometry Sphere { } } ] }\n`,
  ];
  for (const t of not) {
    const s = sess(t);
    const tr = nodes(s, 'Transform')[0];
    assert.equal(so.recognize(tr), null, t);
    assert.equal(so.displayLabel(tr), null);
    assert.equal(so.describeObject(s, tr, { currentText: t }).reason, so.OBJECT_REASON.NOT_A_SIMPLE_OBJECT);
  }
  // A single un-bracketed child is still the recognised shape.
  const single = `${H}Transform { children Shape { geometry Sphere { } } }\n`;
  assert.equal(so.displayLabel(nodes(sess(single), 'Transform')[0]), 'Sphere');
});

test('a shared (USE) Material makes Color read-only; DEF objects stay editable', () => {
  const t = `${H}DEF Red Material { diffuseColor 1 0 0 }\nDEF Obj Transform { children [ Shape { appearance Appearance { material USE Red } geometry Box { } } ] }\n`;
  const d = describe(t);
  assert.equal(prop(d, 'color').editable, false);
  assert.equal(prop(d, 'position').editable, true);
});

test('a damaged document: properties are read-only with the syntax-error reason', () => {
  const t = `${H}Transform { children [ Shape { geometry Box { } } ] }\nGroup {\n`;
  const d = describe(t);
  assert.ok(d.properties.every((p) => !p.editable));
  assert.ok(d.properties.filter((p) => p.reason !== so.OBJECT_REASON.NO_MATERIAL).every((p) => p.reason === fe.FIELD_EDIT_REASON.SYNTAX_ERRORS));
});

test('architecture: the facade owns no state, parses nothing, and edits only through field-edit / structure-edit', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'vrml', 'simple-object.js'), 'utf8');
  const requires = [...src.matchAll(/require\(['"]([^'"]+)['"]\)/g)].map((m) => m[1]).sort();
  assert.deepEqual(requires, ['./ast', './document-transaction', './field-edit', './node-schema', './structure-edit']);
  assert.equal(/\bparse\(|tokenize\(|applyEdits\(|replaceSpan\(|insertAt\(/.test(src), false);
  assert.equal(/^let |^var /m.test(src), false, 'no module-level mutable state');
});
