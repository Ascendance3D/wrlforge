'use strict';
// WD2-D -- the private X_ITE pick adapter (src/preview/xite-pick-adapter.js)
// over a fake X_ITE with the contract's private-surface shapes.
//
//   G5  hook restored + coordinator released on every exit path
//   G6  displaced wrapper: never overwritten, capture lost, picking disabled
//   G7  two browsers, one parser prototype: serialized, never two wrappers,
//       no cross-generation records; FIFO; same-adapter supersession
//   G10 missing/changed P-surface -> COMPATIBILITY_DISABLED; rendering and
//       Scene Tree selection unaffected; per-click refusals never disable
//   G13 touch() === false -> NO_HIT, getHit() NOT called, old hit not reused
//   G14 abort during an asynchronous parse; late completion is harmless
//   + coordinate rule, G4 probe, disposal / retention.
//
// The coordinator is module-private and has no test API: every assertion
// about it is OBSERVABLE behaviour -- which wrapper sits on the shared parser
// prototype, which fake browser has started parsing, what compatibility()
// reports, and whether a brand-new adapter on the same page acquires the hook
// promptly. A fresh module instance (require cache cleared) is a fresh page.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');

const vrml = require('../../src/vrml');
const VP = require('../../src/editor/viewport-pick');
const SceneSelection = require('../../src/editor/scene-selection');
const fixtures = require('./_pick-fixtures');
const { createFakeX3D, createFakeBrowser, findNodes, settleProbe, createShownTracker } = require('./_fake-xite');

const ADAPTER = path.join(__dirname, '..', '..', 'src', 'preview', 'xite-pick-adapter.js');
// A fresh module = a fresh page (one coordinator per realm).
function freshPage() {
  delete require.cache[require.resolve(ADAPTER)];
  const A = require(ADAPTER);
  const X3D = createFakeX3D();
  return { A, X3D, proto: X3D.VRMLParser.prototype, original: X3D.VRMLParser.prototype.nodeStatement };
}

// A browser whose parse waits on a test-controlled gate.
function gatedBrowser(X3D) {
  const b = createFakeBrowser(X3D);
  const gates = [];
  b.gate = () => new Promise((resolve) => gates.push(resolve));
  b.openGate = async () => {
    for (let i = 0; i < 50 && !gates.length; i++) await tick();
    assert.ok(gates.length, 'a parse is waiting on the gate');
    gates.shift()();
    await tick();
  };
  b.gates = gates;
  return b;
}
const tick = () => new Promise((r) => setImmediate(r));

const BOX = fixtures.BUILDERS['P5-anonymous-twins']({});

async function readyAdapter(page, browser) {
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  if (browser.gate) await browser.openGate(); // the G4 probe runs through the gate too
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  return adapter;
}

// Observable "owner === null, no waiters, not disabled": the prototype holds
// X_ITE's own method, and a NEW adapter on the same page acquires the hook and
// completes a provenance parse within a bounded number of ticks.
async function assertCoordinatorIdle(page, label = 'coordinator idle') {
  assert.equal(page.proto.nodeStatement, page.original, `${label}: no wrapper installed`);
  const probeAdapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser: createFakeBrowser(page.X3D) });
  assert.deepEqual(await settleProbe(probeAdapter), { ok: true }, `${label}: a new adapter acquires the hook (probe)`);
  let r = null;
  probeAdapter.parseWithProvenance(BOX.text, { generationId: 'idle' }).then((v) => { r = v; });
  for (let i = 0; i < 50 && !r; i++) await tick();
  assert.ok(r && r.generation, `${label}: no owner or waiter blocks a new provenance parse`);
  probeAdapter.dispose();
  assert.equal(page.proto.nodeStatement, page.original, `${label}: restored again`);
}

// ---- G4 probe -----------------------------------------------------------------
test('G4: the probe proves exact offsets through the production hook path, then restores', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  assert.deepEqual(adapter.compatibility(), { ok: false, reason: 'compatibility-unproven' });
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  assert.equal(browser.calls.parse, 1, 'exactly one probe parse');
  assert.equal(page.proto.nodeStatement, page.original);
  await assertCoordinatorIdle(page);
});

test('G4: a parser whose input is not the exact string fails the probe and disables for the page', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const realParse = browser.createX3DFromString;
  browser.createX3DFromString = (text) => realParse(`${text} `); // a "normalizing" X_ITE
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  assert.deepEqual(await settleProbe(adapter), { ok: false, reason: 'parser-offsets-unproven' });
  const other = page.A.createXitePickAdapter({ X3D: page.X3D, browser: createFakeBrowser(page.X3D) });
  assert.equal(other.compatibility().ok, false, 'a parser failure disables every adapter on the page');
  // rendering still works: a plain parse, no generation
  const r = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
  assert.ok(r.scene);
  assert.equal(r.generation, null);
  assert.equal(page.proto.nodeStatement, page.original);
});

// ---- G10 compatibility ---------------------------------------------------------
test('G10: each missing / changed P-surface disables with its P-row reason', async () => {
  const cases = [
    ['P1 parser class', (X3D) => { delete X3D.VRMLParser; }, 'parser-class-missing'],
    ['P2 hook with params', (X3D) => { const o = X3D.VRMLParser.prototype.nodeStatement; X3D.VRMLParser.prototype.nodeStatement = function (a) { return o.call(this, a); }; }, 'parser-hook-missing'],
    ['P2 inherited hook', (X3D) => { const o = X3D.VRMLParser.prototype.nodeStatement; delete X3D.VRMLParser.prototype.nodeStatement; Object.setPrototypeOf(X3D.VRMLParser.prototype, { nodeStatement: o }); }, 'parser-hook-missing'],
    ['P3 comments', (X3D) => { delete X3D.VRMLParser.prototype.comments; }, 'parser-comments-missing'],
    ['P5 context', (X3D) => { delete X3D.VRMLParser.prototype.isInsideProtoDeclaration; }, 'parser-context-missing'],
    ['P12 classes', (X3D) => { delete X3D.X3DScene; }, 'class-missing'],
  ];
  for (const [label, mutate, reason] of cases) {
    const page = freshPage();
    mutate(page.X3D);
    const browser = createFakeBrowser(page.X3D);
    const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
    assert.deepEqual(adapter.compatibility(), { ok: false, reason }, label);
    const other = page.A.createXitePickAdapter({ X3D: page.X3D, browser: createFakeBrowser(page.X3D) });
    assert.deepEqual(other.compatibility(), { ok: false, reason }, `${label}: shared parser failure is page-wide`);
  }
  const browserCases = [
    ['version', (b) => { b.version = '15.1.11'; }, 'xite-version-mismatch'],
    ['P6 touch', (b) => { delete b.touch; }, 'touch-missing'],
    ['P6 touch arity', (b) => { b.touch = function (x) { return false; }; }, 'touch-missing'],
    ['P7 getHit', (b) => { delete b.getHit; }, 'hit-shape-changed'],
    ['P10 getWorld', (b) => { delete b.getWorld; }, 'world-infrastructure-missing'],
    ['P11 getViewport', (b) => { delete b.getViewport; }, 'viewport-missing'],
  ];
  for (const [label, mutate, reason] of browserCases) {
    const page = freshPage();
    const browser = createFakeBrowser(page.X3D);
    mutate(browser);
    const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
    assert.deepEqual(adapter.compatibility(), { ok: false, reason }, label);
    // rendering is unaffected: plain parse, generation null
    if (browser.createX3DFromString) {
      const r = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
      assert.ok(r.scene, `${label}: still renders`);
      assert.equal(r.generation, null);
    }
    assert.equal(page.proto.nodeStatement, page.original, `${label}: no hook installed`);
    const res = VP.resolvePick({ snapshot: adapter.pick(10, 10) });
    assert.equal(res.status, 'COMPATIBILITY_DISABLED', label);
    assert.equal(res.reason, reason);
    assert.match(VP.refusalText(res), /Preview picking unavailable \(X_ITE compatibility check failed: /);
  }
});

test('G10: disabled picking leaves Scene Tree selection fully working', () => {
  const page = freshPage();
  delete page.X3D.VRMLParser;
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser: createFakeBrowser(page.X3D) });
  assert.equal(adapter.compatibility().ok, false);
  const tree = vrml.sceneTree.buildSceneTree(vrml.parse(BOX.text));
  const selection = SceneSelection.createSelectionController();
  assert.equal(selection.setSelection(tree.items[1].id), true);
  assert.equal(selection.getSelection(), tree.items[1].id);
});

test('G10: ordinary per-click refusals never change compatibility()', async () => {
  const page = freshPage();
  const fx = fixtures.BUILDERS['P9-def-use']({});
  const browser = createFakeBrowser(page.X3D);
  const adapter = await readyAdapter(page, browser);
  const { scene, generation } = await adapter.parseWithProvenance(fx.text, { generationId: 1 });
  await browser.replaceWorld(scene);
  adapter.activate(generation, scene);
  const analysis = { text: fx.text, parse: vrml.parse(fx.text) };
  analysis.sceneTree = vrml.sceneTree.buildSceneTree(analysis.parse);
  browser.aim(findNodes(scene, 'Shape')[0]);
  const s1 = adapter.pick(1, 1);
  assert.equal(VP.resolvePick({ snapshot: s1, analysis, currentText: fx.text }).status, 'REFUSED_AMBIGUOUS');
  browser.aim(null);
  assert.equal(VP.resolvePick({ snapshot: adapter.pick(1, 1), analysis, currentText: fx.text }).status, 'NO_HIT');
  adapter.retire();
  assert.equal(VP.resolvePick({ snapshot: adapter.pick(1, 1) }).status, 'REFUSED_STALE');
  assert.deepEqual(adapter.compatibility(), { ok: true });
});

test('per-pick structural surfaces (P7 hit shape, P10 layer) disable when they drift', async () => {
  for (const [mutate, reason] of [
    [(b) => { const h = b.getHit; b.getHit = () => ({ ...h(), sensors: [] }); }, 'hit-shape-changed'],
    [(b) => { b.getWorld = () => ({ getLayer0: () => null }); }, 'world-infrastructure-missing'],
  ]) {
    const page = freshPage();
    const browser = createFakeBrowser(page.X3D);
    const adapter = await readyAdapter(page, browser);
    const { scene, generation } = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
    await browser.replaceWorld(scene);
    adapter.activate(generation, scene);
    browser.aim(findNodes(scene, 'Shape')[0]);
    mutate(browser);
    const s = adapter.pick(1, 1);
    assert.equal(s.outcome, 'disabled');
    assert.equal(s.reason, reason);
    assert.deepEqual(adapter.compatibility(), { ok: false, reason });
  }
});

// ---- G5 hook lifecycle ------------------------------------------------------------
test('G5: the hook is restored and the coordinator released on every exit path', async () => {
  // success
  {
    const page = freshPage();
    const browser = gatedBrowser(page.X3D);
    const adapter = await readyAdapter(page, browser);
    const p = adapter.parseWithProvenance(BOX.text, { generationId: 1 });
    await tick();
    assert.notEqual(page.proto.nodeStatement, page.original, 'wrapper installed for the WHOLE awaited parse');
    await browser.openGate();
    const r = await p;
    assert.ok(r.generation);
    await assertCoordinatorIdle(page, 'success');
  }
  // parse rejection
  {
    const page = freshPage();
    const browser = createFakeBrowser(page.X3D);
    const adapter = await readyAdapter(page, browser);
    await assert.rejects(adapter.parseWithProvenance(`${BOX.text}Transform {`, { generationId: 1 }), /fake parse error/);
    await assertCoordinatorIdle(page, 'parse rejection');
  }
  // recording throws -> parse still succeeds, no generation
  {
    const page = freshPage();
    const browser = createFakeBrowser(page.X3D);
    const adapter = await readyAdapter(page, browser);
    const realGetScene = page.proto.getScene;
    page.proto.getScene = function () { throw new Error('recording broke'); };
    const r = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
    page.proto.getScene = realGetScene;
    assert.ok(r.scene, 'recording never breaks the parse');
    assert.equal(r.generation, null);
    assert.equal(r.reason, 'generation-unprovable');
    await assertCoordinatorIdle(page, 'recording throw');
  }
  // map building throws -> no generation, still restored
  {
    const page = freshPage();
    const browser = createFakeBrowser(page.X3D);
    const adapter = await readyAdapter(page, browser);
    const realParse = browser.createX3DFromString;
    browser.createX3DFromString = async (text) => {
      const scene = await realParse(text);
      const set = WeakMap.prototype.set;
      WeakMap.prototype.set = function () { WeakMap.prototype.set = set; throw new Error('map build broke'); };
      return scene;
    };
    const r = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
    assert.ok(r.scene);
    assert.equal(r.generation, null);
    await assertCoordinatorIdle(page, 'map-building throw');
  }
});

// ---- G6 displaced wrapper ---------------------------------------------------------
test('G6: a displaced wrapper is never overwritten; capture lost, picking disabled, waiter wakes to disabled', async () => {
  const page = freshPage();
  const ba = gatedBrowser(page.X3D);
  const bb = createFakeBrowser(page.X3D);
  const a = await readyAdapter(page, ba);
  const b = await readyAdapter(page, bb);
  const pa = a.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  const ours = page.proto.nodeStatement;
  assert.notEqual(ours, page.original);
  const foreign = function foreignWrapper() { return ours.apply(this, arguments); };
  page.proto.nodeStatement = foreign; // another actor replaces it mid-parse
  const bParsesBefore = bb.calls.parse;
  const pb = b.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  assert.equal(bb.calls.parse, bParsesBefore, 'B waits for the owner: it has not started parsing');
  await ba.openGate();
  const ra = await pa;
  assert.ok(ra.scene, 'rendering continues');
  assert.equal(ra.generation, null, 'no generation published');
  assert.equal(page.proto.nodeStatement, foreign, 'the replacement survives');
  assert.deepEqual(a.compatibility(), { ok: false, reason: 'parser-hook-displaced' });
  assert.deepEqual(b.compatibility(), { ok: false, reason: 'parser-hook-displaced' });
  const rb = await pb;
  assert.ok(rb.scene, 'the waiter falls back to a plain parse');
  assert.equal(rb.generation, null);
  assert.equal(page.proto.nodeStatement, foreign, 'still never overwritten');
  assert.equal(VP.resolvePick({ snapshot: a.pick(1, 1) }).status, 'COMPATIBILITY_DISABLED');
});

test('G6: abort() with a displaced wrapper writes nothing and disables', async () => {
  const page = freshPage();
  const browser = gatedBrowser(page.X3D);
  const a = await readyAdapter(page, browser);
  const pa = a.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  const foreign = function foreign2() { return page.original.apply(this, arguments); };
  page.proto.nodeStatement = foreign;
  a.abort();
  assert.equal(page.proto.nodeStatement, foreign);
  assert.equal(a.compatibility().reason, 'parser-hook-displaced');
  // released: a waiter on the same page wakes at once to 'disabled' (plain parse)
  const late = page.A.createXitePickAdapter({ X3D: page.X3D, browser: createFakeBrowser(page.X3D) });
  const lr = await late.parseWithProvenance(BOX.text, { generationId: 9 });
  assert.ok(lr.scene);
  assert.equal(lr.generation, null);
  await browser.openGate();
  assert.equal((await pa).generation, null);
  assert.equal(page.proto.nodeStatement, foreign);
});

// ---- G7 shared prototype, two browsers ------------------------------------------
test('G7: two browsers serialize on one prototype; never two wrappers; no cross-generation records', async () => {
  const page = freshPage();
  const ba = gatedBrowser(page.X3D);
  const bb = gatedBrowser(page.X3D);
  const a = await readyAdapter(page, ba);
  const b = await readyAdapter(page, bb);
  const pa = a.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  const wrapperA = page.proto.nodeStatement;
  const bParsesBefore = bb.calls.parse; // the G4 probe already parsed once
  const pb = b.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick(); await tick();
  assert.equal(bb.calls.parse, bParsesBefore, 'B has not started parsing');
  assert.equal(page.proto.nodeStatement, wrapperA, 'only A\'s wrapper is installed');
  await ba.openGate();
  const ra = await pa;
  await tick();
  assert.equal(bb.calls.parse, bParsesBefore + 1, 'B runs after A released');
  assert.notEqual(page.proto.nodeStatement, wrapperA, 'A\'s wrapper is gone before B installs');
  assert.notEqual(page.proto.nodeStatement, page.original);
  await bb.openGate();
  const rb = await pb;
  assert.equal(page.proto.nodeStatement, page.original);
  // Activate both; each proves only its own runtime nodes.
  const shownA = createShownTracker(a, ba);
  const shownB = createShownTracker(b, bb);
  await ba.replaceWorld(ra.scene); assert.ok(shownA.activate(ra.generation, ra.scene));
  await bb.replaceWorld(rb.scene); assert.ok(shownB.activate(rb.generation, rb.scene));
  const analysis = { text: BOX.text, parse: vrml.parse(BOX.text) };
  analysis.sceneTree = vrml.sceneTree.buildSceneTree(analysis.parse);
  const run = (adapter, browser, shown, node) => {
    browser.aim(node);
    const s = adapter.pick(1, 1);
    return VP.resolvePick({ snapshot: s, currentCheck: s.generation ? shown.currentCheck(s.generation) : null, analysis, currentText: BOX.text });
  };
  assert.equal(run(a, ba, shownA, findNodes(ra.scene, 'Shape')[0]).status, 'PROVEN');
  assert.equal(run(b, bb, shownB, findNodes(rb.scene, 'Shape')[1]).status, 'PROVEN');
  assert.notEqual(run(a, ba, shownA, findNodes(rb.scene, 'Shape')[0]).status, 'PROVEN', 'B\'s node is not in A\'s map');
  assert.notEqual(run(b, bb, shownB, findNodes(ra.scene, 'Shape')[0]).status, 'PROVEN', 'A\'s node is not in B\'s map');
});

test('G7: FIFO order across three browsers', async () => {
  const page = freshPage();
  const bs = [gatedBrowser(page.X3D), gatedBrowser(page.X3D), gatedBrowser(page.X3D)];
  const as = [];
  for (const b of bs) as.push(await readyAdapter(page, b));
  const order = [];
  const ps = as.map((a, i) => a.parseWithProvenance(BOX.text, { generationId: i }).then(() => order.push(i)));
  await tick();
  await bs[0].openGate();
  await bs[1].openGate();
  await bs[2].openGate();
  await Promise.all(ps);
  assert.deepEqual(order, [0, 1, 2]);
  await assertCoordinatorIdle(page);
});

test('same-browser supersession: the older request is aborted, the newer one completes', async () => {
  const page = freshPage();
  const browser = gatedBrowser(page.X3D);
  const a = await readyAdapter(page, browser);
  const p1 = a.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  const w1 = page.proto.nodeStatement;
  const p2 = a.parseWithProvenance(BOX.text, { generationId: 2 });
  await tick();
  assert.notEqual(page.proto.nodeStatement, w1, 'the superseded wrapper was removed');
  await browser.openGate(); // p1's late parse
  const r1 = await p1;
  assert.equal(r1.generation, null, 'the superseded request publishes nothing');
  await browser.openGate();
  const r2 = await p2;
  assert.ok(r2.generation);
  assert.equal(r2.generation.overlayGeneration, 2);
  assert.equal(page.proto.nodeStatement, page.original);
});

// ---- G14 abort -----------------------------------------------------------------------
test('G14: abort restores synchronously; a late completion publishes nothing and cannot touch the newer owner', async () => {
  const page = freshPage();
  const ba = gatedBrowser(page.X3D);
  const bb = gatedBrowser(page.X3D);
  const a = await readyAdapter(page, ba);
  const b = await readyAdapter(page, bb);
  const pa = a.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  const pb = b.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  assert.notEqual(page.proto.nodeStatement, page.original);
  a.abort();
  // synchronously: original restored, A released
  assert.equal(page.proto.nodeStatement, page.original);
  await tick();
  const wrapperB = page.proto.nodeStatement;
  assert.notEqual(wrapperB, page.original, 'the queued parse then installs');
  // A's late completion
  await ba.openGate();
  const ra = await pa;
  assert.equal(ra.generation, null, 'no publication');
  assert.equal(a.activate(ra.generation, ra.scene), false, 'no activation');
  assert.equal(page.proto.nodeStatement, wrapperB, 'the newer owner\'s wrapper is untouched (B still owns)');
  await bb.openGate();
  const rb = await pb;
  assert.ok(rb.generation, 'B completes normally');
  assert.equal(page.proto.nodeStatement, page.original);
  assert.deepEqual(a.compatibility(), { ok: true }, 'an abort is not a structural failure');
});

test('G14: abort while waiting cancels; abort after completion retires the unpublished generation', async () => {
  const page = freshPage();
  const ba = gatedBrowser(page.X3D);
  const bb = createFakeBrowser(page.X3D);
  const a = await readyAdapter(page, ba);
  const b = await readyAdapter(page, bb);
  const pa = a.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  const pb = b.parseWithProvenance(BOX.text, { generationId: 1 });
  await tick();
  b.abort();
  const rb = await pb;
  assert.equal(rb.scene, null, 'a cancelled caller does not render');
  assert.equal(rb.reason, 'cancelled');
  assert.equal(bb.calls.parse, 2 - 1, 'only the probe parsed on B');
  await ba.openGate();
  const ra = await pa;
  assert.ok(ra.generation);
  a.abort(); // before activate
  await ba.replaceWorld(ra.scene);
  assert.equal(a.activate(ra.generation, ra.scene), false);
  assert.equal(VP.resolvePick({ snapshot: a.pick(1, 1) }).status, 'REFUSED_STALE');
  await assertCoordinatorIdle(page);
});

// ---- G13 touch() === false -----------------------------------------------------------
test('G13: touch() false never reads getHit(); a retained previous hit cannot be selected', async () => {
  const page = freshPage();
  const fx = fixtures.BUILDERS['P5-anonymous-twins']({});
  const browser = createFakeBrowser(page.X3D);
  const adapter = await readyAdapter(page, browser);
  const { scene, generation } = await adapter.parseWithProvenance(fx.text, { generationId: 1 });
  await browser.replaceWorld(scene);
  adapter.activate(generation, scene);
  const analysis = { text: fx.text, parse: vrml.parse(fx.text) };
  analysis.sceneTree = vrml.sceneTree.buildSceneTree(analysis.parse);
  const selection = SceneSelection.createSelectionController();
  const commit = (s) => {
    const r = VP.resolvePick({ snapshot: s, currentCheck: null, analysis, currentText: fx.text });
    if (r.status === 'PROVEN') selection.setSelection(r.sceneTreeItemId);
    return r;
  };
  // a valid pick of the LEFT twin
  browser.aim(findNodes(scene, 'Shape')[0]);
  const first = commit(adapter.pick(1, 1));
  assert.equal(first.status, 'PROVEN');
  const leftId = selection.getSelection();
  // the viewer is now active: touch() returns false and leaves the left hit in place
  selection.clearSelection();
  browser.viewerActive = true;
  browser.aim(findNodes(scene, 'Shape')[1]);
  const getHitBefore = browser.calls.getHit;
  const r = commit(adapter.pick(1, 1));
  assert.equal(r.status, 'NO_HIT');
  assert.equal(r.reason, 'viewer-active-or-no-hit');
  assert.equal(browser.calls.getHit, getHitBefore, 'getHit() was NOT called');
  assert.equal(browser.getHit().shapeNode, findNodes(scene, 'Shape')[0], 'the stale hit really is still there');
  assert.equal(selection.getSelection(), null, 'the old Shape was not selected');
  assert.notEqual(selection.getSelection(), leftId);
});

// ---- coordinates --------------------------------------------------------------------
test('client -> touch coordinates use the canvas rect and the X_ITE viewport (spike §11)', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D, { rect: { left: 37, top: 53, width: 400, height: 300 }, viewport: [0, 0, 800, 600] });
  const adapter = await readyAdapter(page, browser);
  const { scene, generation } = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
  await browser.replaceWorld(scene);
  adapter.activate(generation, scene);
  browser.aim(null);
  adapter.pick(37 + 100, 53 + 75);
  assert.deepEqual(browser.lastTouch, { x: 200, y: 450 }, 'DPR 2 framebuffer, origin bottom-left');
});

test('a non-finite viewport disables picking (P11); it never guesses coordinates', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D, { viewport: [0, 0, NaN, 600] });
  const adapter = await readyAdapter(page, browser);
  const { scene, generation } = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
  await browser.replaceWorld(scene);
  adapter.activate(generation, scene);
  const s = adapter.pick(10, 10);
  assert.equal(s.outcome, 'disabled');
  assert.equal(s.reason, 'viewport-missing');
  assert.equal(browser.calls.touch, 0);
});

// ---- disposal / retention --------------------------------------------------------------
test('repeated reload / retire / dispose leaves no wrapper, owner, waiter or generation behind', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const adapter = await readyAdapter(page, browser);
  let last = null;
  for (let i = 1; i <= 25; i++) {
    adapter.abort(); adapter.retire();
    const { scene, generation } = await adapter.parseWithProvenance(BOX.text, { generationId: i });
    await browser.replaceWorld(scene);
    if (last) assert.equal(adapter.activate(last, scene), false, 'an older generation never re-activates');
    assert.ok(adapter.activate(generation, scene));
    browser.aim(findNodes(scene, 'Shape')[0]);
    assert.equal(adapter.pick(1, 1).generation, generation, 'only the newest generation is picked against');
    last = generation;
  }
  adapter.dispose();
  assert.equal(adapter.activate(last, browser.currentScene), false);
  await assertCoordinatorIdle(page);
  assert.deepEqual(adapter.compatibility(), { ok: false, reason: 'adapter-disposed' });
  assert.equal(VP.resolvePick({ snapshot: adapter.pick(1, 1) }).status, 'COMPATIBILITY_DISABLED');
  assert.equal(Object.isFrozen(last), true);
  assert.deepEqual(Object.keys(last).sort(), ['overlayGeneration', 'sessionId', 'text'], 'the token carries no runtime object');
});

test('a snapshot is plain data: no runtime object escapes pick()', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const adapter = await readyAdapter(page, browser);
  const { scene, generation } = await adapter.parseWithProvenance(BOX.text, { generationId: 1 });
  await browser.replaceWorld(scene);
  adapter.activate(generation, scene);
  browser.aim(findNodes(scene, 'Shape')[0]);
  const s = adapter.pick(1, 1);
  const seen = [];
  const walk = (v) => {
    if (v && typeof v === 'object') {
      if (v === generation) return;
      assert.ok(!(v instanceof page.X3D.X3DBaseNode) && !(v instanceof page.X3D.X3DExecutionContext), 'runtime object in snapshot');
      seen.push(v);
      for (const k of Object.keys(v)) walk(v[k]);
    }
  };
  walk(s);
  assert.ok(seen.length > 3);
});
