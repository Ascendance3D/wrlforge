'use strict';
// VISUAL-3A1 -- the pick adapter's runtime-binding amendment (snapshotSpan,
// nodeAt, spansOf) over the fake X_ITE. The Move tool names an AUTHORED node
// by its exact provenance span; the adapter answers with the ONE runtime node
// X_ITE's parser created for that span, or refuses. Spans come from the
// fixture oracle (by construction), never from the adapter under test.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');

const fixtures = require('./_pick-fixtures');
const { createFakeX3D, createFakeBrowser, findNodes, settleProbe } = require('./_fake-xite');

const ADAPTER = path.join(__dirname, '..', '..', 'src', 'preview', 'xite-pick-adapter.js');
function freshPage() {
  delete require.cache[require.resolve(ADAPTER)];
  const A = require(ADAPTER);
  const X3D = createFakeX3D();
  return { A, X3D };
}

async function shown(page, browser, text, id = 1) {
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  const { scene, generation } = await adapter.parseWithProvenance(text, { sessionId: 7, generationId: id });
  await browser.replaceWorld(scene);
  assert.ok(adapter.activate(generation, scene));
  return { adapter, scene, generation };
}

// Parents that are not world infrastructure (layer groups), as Rust reads them.
const docParents = (s, label) => [...s.graph[label].parents].filter((p) => !(s.graph[p] && s.graph[p].ctxKind === 'world-infrastructure'));

const plain = (v, X3D) => {
  const walk = (x) => {
    if (x && typeof x === 'object') {
      assert.ok(!(x instanceof X3D.X3DBaseNode) && !(x instanceof X3D.X3DExecutionContext), 'runtime object in a snapshot');
      for (const k of Object.keys(x)) walk(x[k]);
    }
  };
  walk(v);
};

for (const form of [{}, { prefix: '﻿', eol: '\r\n', comment: 'é ✓ 😀' }, { eol: '\r', comment: 'lone CR' }]) {
  test(`twin Transforms with equal translations bind each to its OWN runtime node (${JSON.stringify(form)})`, async () => {
    const page = freshPage();
    const browser = createFakeBrowser(page.X3D);
    const fx = fixtures.BUILDERS['P5-anonymous-twins'](form);
    // Make the twins indistinguishable by value: same translation text.
    const text = fx.text.split('-2 0 0').join('2 0 0');
    assert.equal(text.length, fx.text.length - 1);
    // Spans after the edit: `left` is before the change except its end.
    const shift = (s) => (s.start > fx.text.indexOf('-2 0 0') ? { start: s.start - 1, end: s.end - 1 } : { start: s.start, end: s.end - 1 });
    const left = shift(fx.spans['left.transform']);
    const right = shift(fx.spans['right.transform']);
    const { adapter, scene, generation } = await shown(page, browser, text);
    const [tl, tr] = findNodes(scene, 'Transform');
    let wrong = 0;
    for (const [span, want, other] of [[left, tl, tr], [right, tr, tl]]) {
      const n = adapter.nodeAt(generation, span.start, span.end);
      if (n === other) wrong += 1;
      assert.equal(n, want, 'exactly the authored node');
      const s = adapter.snapshotSpan(generation, span.start, span.end);
      assert.equal(s.outcome, 'found');
      assert.equal(s.generation, generation);
      const root = s.graph[s.shape];
      assert.equal(root.type, 'Transform');
      assert.equal(root.ctxKind, 'document');
      assert.deepEqual(root.occurrences.map((o) => [o.start, o.end]), [[span.start, span.end]]);
      assert.deepEqual(docParents(s, s.shape), ['SCENE']);
      plain(s, page.X3D);
      assert.deepEqual(adapter.spansOf(generation, n).map((o) => [o.start, o.end]), [[span.start, span.end]]);
    }
    assert.equal(wrong, 0, 'WRONG_RUNTIME_MANIPULATIONS');
    // Off by one at either end is not "close enough".
    for (const [a, b] of [[left.start + 1, left.end], [left.start, left.end - 1], [left.start - 1, left.end]]) {
      assert.equal(adapter.nodeAt(generation, a, b), null);
      const s = adapter.snapshotSpan(generation, a, b);
      assert.equal(s.outcome, 'unsupported');
      assert.equal(s.reason, 'runtime-node-for-span-not-unique');
      assert.equal(s.count, 0);
    }
    adapter.dispose();
  });
}

test('a DEF used by USE: one runtime node, two occurrences (Rust refuses it)', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const head = '#VRML V2.0 utf8\n';
  const def = 'DEF T Transform { translation 1 0 0 children [ Shape { geometry Box { } } ] }';
  const use = 'USE T';
  const text = `${head}${def}\nGroup { children [ ${use} ] }\n`;
  const d = { start: head.length, end: head.length + def.length };
  const u = { start: text.indexOf(use), end: text.indexOf(use) + use.length };
  const { adapter, generation } = await shown(page, browser, text);
  const s = adapter.snapshotSpan(generation, d.start, d.end);
  assert.equal(s.outcome, 'found');
  const occ = s.graph[s.shape].occurrences.map((o) => [o.start, o.end]);
  assert.deepEqual(occ.sort((x, y) => x[0] - y[0]), [[d.start, d.end], [u.start, u.end]]);
  assert.equal(docParents(s, s.shape).length, 2, 'two live parents');
  assert.equal(adapter.nodeAt(generation, d.start, d.end), adapter.nodeAt(generation, u.start, u.end));
  adapter.dispose();
});

test('several runtime nodes for one exact span: no node, a refusal', async () => {
  const page = freshPage();
  // A parser that, for `Echo`, runs the (wrapped) statement twice from the
  // same offset: two distinct runtime nodes carry one span.
  const P = page.X3D.VRMLParser.prototype;
  const orig = P.nodeStatement;
  P.nodeStatement = function () {
    this.comments();
    const at = this.lastIndex;
    if (!this.echoing && this.input.startsWith('Echo', at)) {
      this.echoing = true;
      this.nodeStatement();
      this.lastIndex = at;
      const b = this.nodeStatement();
      this.echoing = false;
      return b;
    }
    return orig.call(this);
  };
  const browser = createFakeBrowser(page.X3D);
  const head = '#VRML V2.0 utf8\n';
  const echo = 'Echo { }';
  const { adapter, generation } = await shown(page, browser, `${head}${echo}\n`);
  assert.equal(adapter.nodeAt(generation, head.length, head.length + echo.length), null);
  const s = adapter.snapshotSpan(generation, head.length, head.length + echo.length);
  assert.equal(s.outcome, 'unsupported');
  assert.equal(s.count, 2);
  adapter.dispose();
});

test('a replaced, retired or superseded generation binds nothing', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = fixtures.BUILDERS['P3-wd2c-box']({});
  const span = fx.spans['box.transform'];
  const { adapter, generation } = await shown(page, browser, fx.text, 1);
  assert.ok(adapter.nodeAt(generation, span.start, span.end));
  // A NEW scene on screen: the old generation is stale even before retire.
  const next = await adapter.parseWithProvenance(fx.text, { sessionId: 7, generationId: 2 });
  await browser.replaceWorld(next.scene);
  assert.equal(adapter.nodeAt(generation, span.start, span.end), null);
  assert.equal(adapter.snapshotSpan(generation, span.start, span.end).outcome, 'stale');
  assert.equal(adapter.spansOf(generation, {}), null);
  // The pending (not yet activated) generation of the scene on screen IS
  // usable -- the camera carry restores before activation.
  assert.ok(adapter.nodeAt(next.generation, span.start, span.end));
  assert.ok(adapter.activate(next.generation, next.scene));
  adapter.retire('source-changed-since-preview');
  assert.equal(adapter.nodeAt(next.generation, span.start, span.end), null);
  assert.equal(adapter.snapshotSpan(next.generation, span.start, span.end).outcome, 'stale');
  // An unknown token, or none.
  assert.equal(adapter.nodeAt(Object.freeze({}), span.start, span.end), null);
  assert.equal(adapter.snapshotSpan(null, span.start, span.end).outcome, 'stale');
  adapter.dispose();
  assert.equal(adapter.snapshotSpan(next.generation, span.start, span.end).outcome, 'disabled');
});

test('a missing parser hook disables binding; nothing is located by other means', async () => {
  const page = freshPage();
  delete page.X3D.VRMLParser.prototype.nodeStatement;
  const browser = createFakeBrowser(page.X3D);
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  assert.equal(adapter.compatibility().ok, false);
  const fx = fixtures.BUILDERS['P3-wd2c-box']({});
  const span = fx.spans['box.transform'];
  const s = adapter.snapshotSpan(null, span.start, span.end);
  assert.equal(s.outcome, 'disabled');
  assert.equal(adapter.nodeAt(null, span.start, span.end), null);
  adapter.dispose();
});
