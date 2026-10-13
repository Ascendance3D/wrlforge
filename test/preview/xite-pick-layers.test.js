'use strict';
// P0-A -- hit layers: reaching the Shape BEHIND a nearly transparent one.
//
// X_ITE's touch() reports only the nearest Shape under the pointer, so a
// clear coat (Material transparency 0.85) over red paint always wins. The
// adapter peels: it takes the hit Shape out of the pointer pass (an OWN
// no-op `traverse`, deleted in `finally`), touches again, and lists the
// Shapes front to back. pick(x, y, layer) snapshots the requested layer
// through the SAME provenance graph as a plain pick, so the coat and the
// paint each map to their own source span -- top level and inside a PROTO
// body -- and a layer without provenance stays unprovable.
//
// Spans are known by construction (concatenation), never searched for by
// the adapter.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');

const vrml = require('../../src/vrml');
const VP = require('../../src/editor/viewport-pick');
const { createFakeX3D, createFakeBrowser, findNodes, settleProbe } = require('./_fake-xite');

const ADAPTER = path.join(__dirname, '..', '..', 'src', 'preview', 'xite-pick-adapter.js');
function freshPage() {
  delete require.cache[require.resolve(ADAPTER)];
  return { A: require(ADAPTER), X3D: createFakeX3D() };
}

async function shown(page, browser, text) {
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  const { scene, generation } = await adapter.parseWithProvenance(text, { sessionId: 3, generationId: 1 });
  await browser.replaceWorld(scene);
  assert.ok(adapter.activate(generation, scene));
  return { adapter, scene, generation };
}

const COAT = 'Shape { appearance Appearance { material Material { diffuseColor 0.94 1 0.96 transparency 0.85 } } geometry Box { size 2.2 1.2 4.2 } }';
const PAINT = 'Shape { appearance Appearance { material Material { diffuseColor 0.78 0 0.11 } } geometry Box { size 2 1 4 } }';

function build() {
  let text = '#VRML V2.0 utf8\n';
  const at = (s) => { const start = text.length; text += s; return { start, end: text.length }; };
  text += 'Transform { children [\n  ';
  const coat = at(COAT);
  text += '\n  ';
  const paint = at(PAINT);
  text += '\n] }\n';
  return { text, coat, paint };
}

function buildProto() {
  let text = '#VRML V2.0 utf8\nPROTO Car [ ] { Transform { children [ ';
  const at = (s) => { const start = text.length; text += s; return { start, end: text.length }; };
  const coat = at(COAT);
  text += ' ';
  const paint = at(PAINT);
  text += ' ] } }\n';
  const inst = at('Car { }');
  text += '\n';
  return { text, coat, paint, inst };
}

const occ = (s) => s.graph[s.shape].occurrences.map((o) => [o.start, o.end]);
const noShadow = (...nodes) => nodes.every((n) => !Object.prototype.hasOwnProperty.call(n, 'traverse'));

test('top level: layer 0 is the coat, layer 1 the paint, each at its own exact span', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = build();
  const { adapter, scene } = await shown(page, browser, fx.text);
  const [coat, paint] = findNodes(scene, 'Shape');
  browser.aim([coat, paint]);

  const front = adapter.pick(5, 5);
  assert.equal(front.outcome, 'hit');
  assert.equal(front.layers, 2);
  assert.equal(front.layer, 0);
  assert.deepEqual(occ(front), [[fx.coat.start, fx.coat.end]]);

  const behind = adapter.pick(5, 5, 1);
  assert.equal(behind.layer, 1);
  assert.equal(behind.layers, 2);
  assert.deepEqual(occ(behind), [[fx.paint.start, fx.paint.end]]);

  // Wraps around: layer 2 of 2 is the front again.
  const again = adapter.pick(5, 5, 2);
  assert.equal(again.layer, 0);
  assert.deepEqual(occ(again), [[fx.coat.start, fx.coat.end]]);

  // Nothing is left hidden, and X_ITE's hit state is the plain touch's.
  assert.ok(noShadow(coat, paint));
  assert.equal(browser.getHit().shapeNode, coat);
  // The last pointing pass had every Shape back in it.
  assert.deepEqual(browser.pointingPasses.at(-1), [coat, paint]);

  // The resolver maps each layer to its own Scene Tree item: zero wrong.
  const analysis = { text: fx.text, parse: vrml.parse(fx.text) };
  analysis.sceneTree = vrml.sceneTree.buildSceneTree(analysis.parse);
  const r0 = VP.resolvePick({ snapshot: front, currentCheck: null, analysis, currentText: fx.text });
  const r1 = VP.resolvePick({ snapshot: behind, currentCheck: null, analysis, currentText: fx.text });
  assert.equal(r0.status, 'PROVEN');
  assert.equal(r1.status, 'PROVEN');
  assert.deepEqual([r0.source.shape.start, r0.source.shape.end], [fx.coat.start, fx.coat.end]);
  assert.deepEqual([r1.source.shape.start, r1.source.shape.end], [fx.paint.start, fx.paint.end]);
  assert.notEqual(r0.sceneTreeItemId, r1.sceneTreeItemId);
});

test('PROTO body: each layer maps through the copy map to its declaration span, BODY parent kept', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = buildProto();
  const { adapter, scene } = await shown(page, browser, fx.text);
  const inst = scene.rootNodes.find((n) => typeof n.getProtoDeclaration === 'function' && n.getProtoDeclaration() !== n);
  const xform = inst.bodyRoots[0];
  const [coat, paint] = xform.children.map((c) => c.node);
  browser.aim([coat, paint]);
  for (const [layer, span, node] of [[0, fx.coat, coat], [1, fx.paint, paint]]) {
    const s = adapter.pick(5, 5, layer);
    assert.equal(s.outcome, 'hit');
    assert.equal(s.layers, 2);
    assert.equal(s.ctxKind, 'proto-body');
    assert.deepEqual(occ(s), [[span.start, span.end]], `layer ${layer}`);
    const t = s.graph[s.graph[s.shape].parents[0]];
    assert.equal(t.type, 'Transform');
    assert.ok(t.parents.some((p) => p.startsWith('BODY:')), JSON.stringify(t.parents));
    const i = s.graph[t.parents.find((p) => p.startsWith('BODY:')).slice(5)];
    assert.deepEqual(i.occurrences.map((o) => [o.start, o.end]), [[fx.inst.start, fx.inst.end]]);
    // The PICKED copy is what the PROTO body binding (gizmo / outline) uses.
    assert.equal(adapter.bodyNodeAt(s.generation, span.start, span.end), node);
  }
  assert.ok(noShadow(coat, paint));
});

test('a layer without provenance is reported as such, never bound to another node', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = build();
  const text = `${fx.text}Inline { url "car.wrl" }\n`;
  const { adapter, scene } = await shown(page, browser, text);
  const [coat] = findNodes(scene, 'Shape');
  const inline = findNodes(scene, 'Inline')[0];
  // Behind the coat: a Shape of ANOTHER document (the Inline's scene).
  browser.aim([coat, inline.inlineShape]);
  const s = adapter.pick(5, 5, 1);
  assert.equal(s.outcome, 'hit');
  assert.equal(s.layers, 2);
  assert.equal(s.ctxKind, 'external-scene');
  assert.deepEqual(occ(s), []);
  const analysis = { text, parse: vrml.parse(text) };
  analysis.sceneTree = vrml.sceneTree.buildSceneTree(analysis.parse);
  const r = VP.resolvePick({ snapshot: s, currentCheck: null, analysis, currentText: text });
  assert.notEqual(r.status, 'PROVEN');
  assert.ok(noShadow(coat, inline.inlineShape));
});

test('a single Shape is one layer; a requested deeper layer is that Shape', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = build();
  const { adapter, scene } = await shown(page, browser, fx.text);
  const [, paint] = findNodes(scene, 'Shape');
  browser.aim(paint);
  const s = adapter.pick(5, 5, 3);
  assert.equal(s.layers, 1);
  assert.equal(s.layer, 0);
  assert.deepEqual(occ(s), [[fx.paint.start, fx.paint.end]]);
  assert.ok(noShadow(paint));
});

test('a Shape that cannot be hidden ends the list at the plain hit', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = build();
  const { adapter, scene } = await shown(page, browser, fx.text);
  const [coat, paint] = findNodes(scene, 'Shape');
  // An own `traverse` already present: the adapter never overwrites it.
  const own = function ownTraverse(type, ro) { if (type === 'POINTER' && ro) ro.addPointingShape(this); };
  coat.traverse = own;
  browser.aim([coat, paint]);
  const s = adapter.pick(5, 5, 1);
  assert.equal(s.layers, 1);
  assert.deepEqual(occ(s), [[fx.coat.start, fx.coat.end]]);
  assert.equal(coat.traverse, own, 'a foreign own property is left alone');
  assert.ok(noShadow(paint));
});

test('every hidden Shape is restored even when touch() throws mid-peel', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = build();
  const { adapter, scene } = await shown(page, browser, fx.text);
  const [coat, paint] = findNodes(scene, 'Shape');
  browser.aim([coat, paint]);
  const real = browser.touch;
  let n = 0;
  browser.touch = function (x, y) {
    n += 1;
    if (n === 2) throw new Error('renderer lost');
    return real.call(this, x, y);
  };
  assert.throws(() => adapter.pick(5, 5, 1), /renderer lost/);
  assert.ok(noShadow(coat, paint));
  browser.touch = real;
  assert.equal(adapter.pick(5, 5, 1).layers, 2);
});

test('touch() false (viewer active) is still NO_HIT and hides nothing', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const fx = build();
  const { adapter, scene } = await shown(page, browser, fx.text);
  const [coat, paint] = findNodes(scene, 'Shape');
  browser.aim([coat, paint]);
  browser.viewerActive = true;
  const s = adapter.pick(5, 5, 1);
  assert.equal(s.outcome, 'no-hit');
  assert.equal(s.reason, 'viewer-active-or-no-hit');
  assert.ok(noShadow(coat, paint));
});

test('at most eight layers are listed', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  let text = '#VRML V2.0 utf8\nGroup { children [\n';
  for (let i = 0; i < 12; i++) text += `Shape { geometry Box { size ${i + 1} 1 1 } }\n`;
  text += '] }\n';
  const { adapter, scene } = await shown(page, browser, text);
  const shapes = findNodes(scene, 'Shape');
  browser.aim(shapes);
  const s = adapter.pick(5, 5, 9);
  assert.equal(s.layers, 8);
  assert.equal(s.layer, 1);
  assert.ok(noShadow(...shapes));
});
