'use strict';
// WD2-C -- new-node templates (src/vrml/node-templates.js).

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const vrml = require('../../src/vrml');
const { walk, NODE } = require('../../src/vrml/ast');
const schema = require('../../src/vrml/node-schema');
const templates = require('../../src/vrml/node-templates');

test('PRIMITIVES are exactly Box and Sphere in WD2-C', () => {
  assert.deepEqual([...templates.PRIMITIVES], ['Box', 'Sphere']);
  assert.equal(templates.VRML97_HEADER, '#VRML V2.0 utf8');
});

for (const primitive of ['Box', 'Sphere']) {
  test(`${primitive}: valid VRML97 -- parses clean, only VRML97 nodes and fields, Transform>Shape>Appearance>Material+${primitive}`, () => {
    const { text, nodeType } = templates.simpleObjectTemplate(primitive);
    assert.equal(nodeType, 'Transform');
    const p = vrml.parse(`${templates.VRML97_HEADER}\n${text}\n`);
    assert.equal(p.diagnostics.filter((d) => d.severity === 'error').length, 0);
    const types = [];
    walk(p.tree, (n) => {
      if (n.type === NODE.NODE) {
        types.push(n.nodeType);
        assert.ok(schema.isVRML97Node(n.nodeType), n.nodeType);
        for (const f of n.fields) {
          if (f.type === NODE.FIELD) assert.ok(schema.isFieldAllowed(n.nodeType, f.name, 'vrml97'), `${n.nodeType}.${f.name}`);
        }
      }
    });
    assert.deepEqual(types, ['Transform', 'Shape', 'Appearance', 'Material', primitive]);
  });

  test(`${primitive}: no automatic DEF, no metadata, no comments, no default-valued fields`, () => {
    const { text } = templates.simpleObjectTemplate(primitive);
    assert.equal(/\bDEF\b|\bUSE\b|WorldInfo|MetadataString|#/.test(text), false);
    const p = vrml.parse(`#VRML V2.0 utf8\n${text}`);
    walk(p.tree, (n) => { assert.notEqual(n.type, NODE.NUMBERS, 'no authored numeric value'); });
    assert.equal(p.defs.length, 0);
  });
}

test('deterministic; text neither starts nor ends with a line ending; CRLF on request', () => {
  const a = templates.simpleObjectTemplate('Box');
  const b = templates.simpleObjectTemplate('Box');
  assert.equal(a.text, b.text);
  assert.ok(!/^[\r\n]|[\r\n]$/.test(a.text));
  const c = templates.simpleObjectTemplate('Box', { eol: '\r\n' });
  assert.equal(c.text, a.text.replace(/\n/g, '\r\n'));
  assert.equal(/(^|[^\r])\n/.test(c.text), false);
});

test('programming errors throw with stable codes', () => {
  assert.throws(() => templates.simpleObjectTemplate('Cylinder'), { code: templates.TEMPLATE_ERROR.PRIMITIVE });
  assert.throws(() => templates.simpleObjectTemplate('Box', { eol: '\r' }), { code: templates.TEMPLATE_ERROR.EOL });
});

test('architecture: templates never read an AST or existing text (not a serializer)', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'vrml', 'node-templates.js'), 'utf8');
  const requires = [...src.matchAll(/require\(['"]([^'"]+)['"]\)/g)].map((m) => m[1]);
  assert.deepEqual(requires, ['./node-schema']);
});
