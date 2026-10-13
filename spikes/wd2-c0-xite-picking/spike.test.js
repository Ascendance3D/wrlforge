'use strict';
// WD2-C0 spike tests (node:test). Not collected by `npm run check`.
//   node --test spikes/wd2-c0-xite-picking/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const fixtures = require('./fixtures');
const mapping = require('./mapping');
const { grade } = require('./grade');
const templates = require('../../src/vrml/node-templates');

const read = (f) => fs.readFileSync(path.join(__dirname, f), 'utf8');
const code = (f) => read(f).replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1');

test('oracle and grader are independent of the mapping under test and of src/', () => {
  for (const f of ['fixtures.js', 'grade.js']) {
    const src = code(f);
    assert.doesNotMatch(src, /require\([^)]*mapping/, `${f} must not require mapping.js`);
    assert.doesNotMatch(src, /require\([^)]*src\//, `${f} must not require src/`);
  }
});

test('mapping has no fallback strategy (name/type/index/nearest/fingerprint/matrix)', () => {
  const src = code('mapping.js');
  for (const bad of [/getName|\.def\b|defsByName/, /nearest|closest|itemContainingOffset/, /indexOf|findIndex|childIds\[/,
    /fingerprint|similar|levenshtein|score/i, /modelViewMatrix|matrix/i, /\.sort\(/]) {
    assert.doesNotMatch(src, bad, `forbidden fallback pattern ${bad}`);
  }
});

test('harness records provenance only for the exact document string and never edits source', () => {
  const src = code('browser/harness.js');
  assert.match(src, /this\.input !== cap\.text/);
  assert.doesNotMatch(src, /createX3DFromString\((?!text\))/, 'X_ITE must be given the unmodified text');
});

test('P3/P4 fixtures are the production WD2-C First Object bytes', () => {
  const fx = fixtures.build();
  const p3 = fx.find((f) => f.id === 'P3-wd2c-first-object');
  const s = p3.spans['fo.transform'];
  assert.equal(p3.text.slice(s.start, s.end), templates.simpleObjectTemplate('Box').text);
  const p4 = fx.find((f) => f.id === 'P4-box-and-sphere');
  const t = p4.spans['sphere.transform'];
  assert.equal(p4.text.slice(t.start, t.end).replace('  translation 3 0 0\n', ''), templates.simpleObjectTemplate('Sphere').text);
});

test('oracle spans name the authored text exactly', () => {
  for (const f of fixtures.build()) {
    for (const c of f.clicks) {
      for (const k of [c.expect.clicked, c.expect.logical].filter(Boolean)) {
        assert.ok(f.spans[k], `${f.id}/${c.id} label ${k}`);
        const txt = f.text.slice(f.spans[k].start, f.spans[k].end);
        assert.match(txt, /^(DEF \w+ )?(Transform|Shape|Group|Inline|MyBox|USE|Anchor|Switch)\b/, `${f.id} ${k}: ${txt.slice(0, 30)}`);
      }
    }
  }
});

// ---- synthetic hits: the mapping refuses rather than guesses ---------------
const P5 = fixtures.build().find((f) => f.id === 'P5-anonymous-twins');
const occ = (label, kind = 'node') => ({ start: P5.spans[label].start, end: P5.spans[label].end, kind });
function twinHit(over = {}) {
  return {
    generation: 7, shape: 'rtS', sensors: [], ctxKind: 'document',
    graph: {
      rtS: { type: 'Shape', ctxKind: 'document', occurrences: [occ('left.shape')], parents: ['rtT'] },
      rtT: { type: 'Transform', ctxKind: 'document', occurrences: [occ('left.transform')], parents: ['rtG', 'rtL', 'SCENE'] },
      rtG: { type: 'Group', ctxKind: 'world-infrastructure', occurrences: [], parents: [] },
      rtL: { type: 'Layer', ctxKind: 'world-infrastructure', occurrences: [], parents: [] },
      ...over,
    },
  };
}
const ctx = mapping.createSourceContext(P5.text, 7);

test('a clean anonymous chain is PROVEN and promoted to its simple-object Transform', () => {
  const r = mapping.resolvePick(ctx, twinHit());
  assert.equal(r.status, 'PROVEN');
  assert.deepEqual({ start: r.source.logical.start, end: r.source.logical.end }, P5.spans['left.transform']);
  assert.equal(grade(P5.clicks[0].expect, P5.spans, r).wrong, false);
});

test('stale generation is refused', () => {
  assert.equal(mapping.resolvePick(mapping.createSourceContext(P5.text, 8), twinHit()).status, 'REFUSED_STALE');
});

test('an off-by-one provenance span is UNSUPPORTED, never the nearest node', () => {
  const o = occ('left.shape');
  const h = twinHit({ rtS: { type: 'Shape', ctxKind: 'document', occurrences: [{ ...o, end: o.end - 1 }], parents: ['rtT'] } });
  assert.equal(mapping.resolvePick(ctx, h).status, 'UNSUPPORTED');
});

test('a DEF/USE-shared ancestor is REFUSED_AMBIGUOUS', () => {
  const h = twinHit({ rtT: { type: 'Transform', ctxKind: 'document', occurrences: [occ('left.transform', 'def'), occ('right.transform', 'use')], parents: ['SCENE'] } });
  assert.equal(mapping.resolvePick(ctx, h).status, 'REFUSED_AMBIGUOUS');
});

test('an unprovenanced extra runtime parent (script re-parenting) is REFUSED_AMBIGUOUS', () => {
  const h = twinHit({
    rtS: { type: 'Shape', ctxKind: 'document', occurrences: [occ('left.shape')], parents: ['rtT', 'rtX'] },
    rtX: { type: 'Transform', ctxKind: 'document', occurrences: [], parents: ['SCENE'] },
  });
  assert.equal(mapping.resolvePick(ctx, h).status, 'REFUSED_AMBIGUOUS');
});

test('a runtime chain that disagrees with source containment is UNSUPPORTED', () => {
  const h = twinHit({ rtT: { type: 'Transform', ctxKind: 'document', occurrences: [occ('right.transform')], parents: ['SCENE'] } });
  assert.equal(mapping.resolvePick(ctx, h).status, 'UNSUPPORTED');
});

test('sensors, other documents and PROTO bodies are refused', () => {
  assert.equal(mapping.resolvePick(ctx, { ...twinHit(), sensors: [{ type: 'TouchSensor' }] }).status, 'REFUSED_SENSOR_CONFLICT');
  assert.equal(mapping.resolvePick(ctx, { ...twinHit(), ctxKind: 'external-scene' }).status, 'REFUSED_EXTERNAL');
  assert.equal(mapping.resolvePick(ctx, { ...twinHit(), ctxKind: 'proto-body' }).status, 'UNSUPPORTED');
});

test('a document with syntax errors refuses every pick', () => {
  const bad = mapping.createSourceContext(`${P5.text}Transform {`, 7);
  assert.equal(mapping.resolvePick(bad, twinHit()).status, 'UNSUPPORTED');
});

test('measured matrix (when present): WRONG = 0 and every click as expected', { skip: !fs.existsSync(path.join(__dirname, 'out', 'PICKING_MATRIX.json')) }, () => {
  const m = JSON.parse(read('out/PICKING_MATRIX.json'));
  assert.equal(m.totals.WRONG, 0);
  assert.equal(m.rows.filter((r) => r.wrong).length, 0);
  assert.deepEqual(m.rows.filter((r) => !r.matchesExpectation && !r.category.startsWith('CONTROL')).map((r) => `${r.fixture}/${r.click}`), []);
  assert.equal(m.controls.NO_HIT_correct, m.controls.NO_HIT_expected);
  assert.equal(m.reload.anyRuntimeIdentityPersisted, false);
  assert.ok(m.reload.staleGen1HitsAgainstGen2.every((s) => s === 'REFUSED_STALE'));
});
