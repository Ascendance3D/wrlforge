'use strict';
// WD2-D -- the pure viewport pick resolver (src/editor/viewport-pick.js),
// driven end to end through the REAL pick adapter over a fake X_ITE:
//   exact text -> adapter.parseWithProvenance (hooked fake parser) -> activate
//   -> pick (touch/getHit snapshot) -> resolvePick -> scene-tree item.
// Truth comes from test/preview/_pick-fixtures.js (spans recorded by
// construction), never from the resolver, the adapter or the fake.
//
// Guards: G9 (every status + reason, selection unchanged on refusal), G12
// (LF / CRLF / BOM / Unicode exact offsets), the C0 fixture matrix with
// WRONG = 0, the stale cases, and the Shape -> Transform promotion rule.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');

const vrml = require('../../src/vrml');
const VP = require('../../src/editor/viewport-pick');
const SceneSelection = require('../../src/editor/scene-selection');
const fixtures = require('../preview/_pick-fixtures');
const { createFakeX3D, createFakeBrowser, findNodes, settleProbe, createShownTracker } = require('../preview/_fake-xite');

const ADAPTER = path.join(__dirname, '..', '..', 'src', 'preview', 'xite-pick-adapter.js');
function freshAdapterModule() {
  delete require.cache[require.resolve(ADAPTER)];
  return require(ADAPTER);
}

function analysisOf(text) {
  const parse = vrml.parse(text);
  return { text, parse, sceneTree: vrml.sceneTree.buildSceneTree(parse) };
}

// One preview generation of `text` on a fresh fake page.
async function preview(text, { overlayGeneration = 1 } = {}) {
  const A = freshAdapterModule();
  const X3D = createFakeX3D();
  const browser = createFakeBrowser(X3D);
  const adapter = A.createXitePickAdapter({ X3D, browser });
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  const { scene, generation } = await adapter.parseWithProvenance(text, { generationId: overlayGeneration, sessionId: 's1' });
  assert.ok(generation, 'a provenance generation is minted');
  await browser.replaceWorld(scene);
  const shown = createShownTracker(adapter, browser);
  assert.equal(shown.activate(generation, scene), true);
  return { A, X3D, browser, adapter, shown, scene, generation };
}

function aimAt(page, aim) {
  if (!aim) return null;
  if (aim.inlineOf != null) return findNodes(page.scene, 'Inline')[aim.inlineOf].inlineShape;
  return findNodes(page.scene, aim.type)[aim.nth];
}

function clickAndResolve(page, aim, { analysis, currentText } = {}) {
  page.browser.aim(aimAt(page, aim));
  const snapshot = page.adapter.pick(50, 50);
  return VP.resolvePick({
    snapshot,
    currentCheck: snapshot.generation ? page.shown.currentCheck(snapshot.generation) : null,
    analysis,
    currentText,
  });
}

// ---- the C0 fixture matrix in every mandatory source form --------------------
const FORMS = {
  LF: { eol: '\n' },
  CRLF: { eol: '\r\n' },
  BOM: { prefix: '\uFEFF' },
  Unicode: { comment: 'Ünïcödé ✓ 日本語 😀 — surrogate pairs shift UTF-16 offsets' },
};

// A leading U+FEFF is not valid VRML97 to the WRL Forge parser (VRML001), so
// a BOM document has no provable identity: every geometry click must FAIL
// CLOSED (document-has-syntax-errors), with the BOM never stripped.
const failsClosed = (form) => form === 'BOM';

for (const [form, opts] of Object.entries(FORMS)) {
  test(`C0 fixture matrix through the adapter, ${form}: every row as expected, WRONG = 0`, async () => {
    const counts = {};
    let wrong = 0;
    for (const fx of fixtures.build(opts)) {
      const page = await preview(fx.text);
      const analysis = analysisOf(fx.text);
      for (const click of fx.clicks) {
        const res = clickAndResolve(page, click.aim, { analysis, currentText: fx.text });
        const g = fixtures.grade(click.expect, fx.spans, res);
        if (g.wrong) wrong += 1;
        counts[res.status] = (counts[res.status] || 0) + 1;
        if (failsClosed(form)) {
          assert.notEqual(res.status, 'PROVEN', `${form} ${fx.id}/${click.id}`);
          if (click.expect.status === 'PROVEN') assert.equal(res.reason, 'document-has-syntax-errors');
          continue;
        }
        assert.equal(res.status, click.expect.status, `${form} ${fx.id}/${click.id}: ${res.status} ${res.reason}`);
        if (res.status === 'PROVEN') {
          const item = vrml.sceneTree.itemForAstNode(analysis.sceneTree,
            [...analysis.sceneTree.items].map((it) => vrml.sceneTree.astNodeForItem(analysis.sceneTree, it.id))
              .find((n) => n && n.range.start.offset === fx.spans[click.expect.logical].start
                && n.range.end.offset === fx.spans[click.expect.logical].end));
          assert.equal(res.sceneTreeItemId, item.id, `${fx.id}/${click.id} selects the oracle's scene item`);
        }
      }
    }
    assert.equal(wrong, 0, 'WRONG_SOURCE_SELECTIONS = 0');
    if (failsClosed(form)) assert.equal(counts.PROVEN || 0, 0, 'BOM: nothing proven');
    else assert.ok(counts.PROVEN >= 20, `PROVEN count ${counts.PROVEN}`);
  });
}

test('source forms are never normalized: a BOM is kept and fails closed', async () => {
  const fx = fixtures.BUILDERS['P3-wd2c-box']({ prefix: '\uFEFF', eol: '\r\n' });
  assert.equal(fx.text.charCodeAt(0), 0xFEFF);
  const page = await preview(fx.text);
  assert.equal(page.generation.text, fx.text, 'generation text is the exact string, BOM + CRLF intact');
  const res = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis: analysisOf(fx.text), currentText: fx.text });
  assert.equal(res.status, 'UNSUPPORTED');
  assert.equal(res.reason, 'document-has-syntax-errors');
});

test('source forms are never normalized: CRLF + Unicode stay in the offsets', async () => {
  const fx = fixtures.BUILDERS['P3-wd2c-box']({ eol: '\r\n', comment: 'Ünïcödé 😀' });
  assert.ok(fx.text.includes('\r\n'));
  const page = await preview(fx.text);
  assert.equal(page.generation.text, fx.text);
  const res = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis: analysisOf(fx.text), currentText: fx.text });
  assert.equal(res.status, 'PROVEN');
  assert.deepEqual({ start: res.source.logical.start, end: res.source.logical.end }, fx.spans['box.transform']);
  assert.equal(fx.text.slice(res.source.shape.start, res.source.shape.end).startsWith('Shape {'), true);
});

test('a source form difference between preview text and buffer is STALE, never mapped', async () => {
  const lf = fixtures.BUILDERS['P5-anonymous-twins']({ eol: '\n' });
  const crlf = fixtures.BUILDERS['P5-anonymous-twins']({ eol: '\r\n' });
  const page = await preview(lf.text);
  const res = clickAndResolve(page, { type: 'Shape', nth: 1 }, { analysis: analysisOf(crlf.text), currentText: crlf.text });
  assert.equal(res.status, 'REFUSED_STALE');
  assert.equal(res.reason, 'source-changed-since-preview');
});

// ---- stale ---------------------------------------------------------------------
test('stale after a source edit: old hits never join the new text', async () => {
  const fx = fixtures.BUILDERS['P5-anonymous-twins']({});
  const page = await preview(fx.text);
  const edited = fx.text.replace('-2 0 0', '-4.25 0 0');
  const res = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis: analysisOf(edited), currentText: edited });
  assert.equal(res.status, 'REFUSED_STALE');
  assert.equal(res.reason, 'source-changed-since-preview');
  // analysis lagging behind the buffer is stale too
  const res2 = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis: analysisOf(fx.text), currentText: edited });
  assert.equal(res2.status, 'REFUSED_STALE');
});

test('stale after a failed reload: the last valid scene refuses', async () => {
  const fx = fixtures.BUILDERS['P5-anonymous-twins']({});
  const page = await preview(fx.text);
  const broken = `${fx.text}Transform {`;
  page.adapter.abort();
  page.adapter.retire();
  await assert.rejects(page.adapter.parseWithProvenance(broken, { generationId: 2 }));
  const res = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis: analysisOf(broken), currentText: broken });
  assert.equal(res.status, 'REFUSED_STALE');
  assert.equal(res.reason, 'preview-shows-last-valid-scene');
  assert.equal(page.browser.calls.touch, 0, 'no touch() for a refused stale click');
});

test('stale after generation replacement and for a hit snapshot from the prior generation', async () => {
  const fx = fixtures.BUILDERS['P5-anonymous-twins']({});
  const page = await preview(fx.text);
  page.browser.aim(findNodes(page.scene, 'Shape')[0]);
  const oldSnap = page.adapter.pick(50, 50);
  assert.equal(oldSnap.outcome, 'hit');
  // the same text is reloaded: a NEW generation replaces the old one
  page.shown.retire();
  const r2 = await page.adapter.parseWithProvenance(fx.text, { generationId: 2 });
  await page.browser.replaceWorld(r2.scene);
  assert.equal(page.shown.activate(r2.generation, r2.scene), true);
  const oldScene = page.scene;
  page.scene = r2.scene;
  const analysis = analysisOf(fx.text);
  const res = VP.resolvePick({ snapshot: oldSnap, currentCheck: page.shown.currentCheck(oldSnap.generation), analysis, currentText: fx.text });
  assert.equal(res.status, 'REFUSED_STALE');
  assert.equal(res.reason, 'hit-from-another-preview-generation');
  // the old generation can never be re-activated
  assert.equal(page.adapter.activate(oldSnap.generation, oldScene), false);
  // a runtime node of the OLD scene hit in the new one is another document
  page.browser.aim(findNodes(oldScene, 'Shape')[0]);
  const crossed = VP.resolvePick({ snapshot: page.adapter.pick(50, 50), currentCheck: null, analysis, currentText: fx.text });
  assert.notEqual(crossed.status, 'PROVEN');
  // the new one still proves
  const ok = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis, currentText: fx.text });
  assert.equal(ok.status, 'PROVEN');
  assert.equal(ok.generation, 2);
});

test('stale when the displayed scene is no longer the generation scene (Fit / Show saved / Anchor)', async () => {
  const fx = fixtures.BUILDERS['P3-wd2c-box']({});
  const page = await preview(fx.text);
  const other = await page.browser.createX3DFromString(fx.text);
  await page.browser.replaceWorld(other);
  const res = clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis: analysisOf(fx.text), currentText: fx.text });
  assert.equal(res.status, 'REFUSED_STALE');
  assert.equal(res.reason, 'preview-scene-replaced');
});

// ---- G9: every status, refusal leaves the selection unchanged -----------------
function snap(extra) {
  const gen = Object.freeze({ overlayGeneration: 7, text: 'T' });
  return { generation: gen, analysis: { text: 'T', parse: vrml.parse('#VRML V2.0 utf8\n'), sceneTree: null }, ...extra };
}

test('G9: pre-adapter and adapter outcomes map to first-class statuses (never null)', () => {
  const cases = [
    [{ outcome: 'disabled', reason: 'parser-hook-displaced' }, 'COMPATIBILITY_DISABLED', 'parser-hook-displaced'],
    [{ outcome: 'stale', reason: 'preview-shows-last-valid-scene' }, 'REFUSED_STALE', 'preview-shows-last-valid-scene'],
    [{ outcome: 'external', reason: 'preview-root-is-another-document' }, 'REFUSED_EXTERNAL', 'preview-root-is-another-document'],
    [{ outcome: 'unsupported', reason: 'preview-is-not-the-document' }, 'UNSUPPORTED', 'preview-is-not-the-document'],
  ];
  for (const [s, status, reason] of cases) {
    const r = VP.resolvePick({ snapshot: s });
    assert.equal(r.status, status);
    assert.equal(r.reason, reason);
    assert.equal(r.sceneTreeItemId, null);
    assert.ok(VP.refusalText(r).length > 0, `UX text for ${status}`);
  }
  assert.equal(VP.resolvePick({}).status, 'UNSUPPORTED');
  assert.equal(VP.resolvePick({ snapshot: null }).reason, 'generation-unprovable');
});

test('G9: proto-body, syntax errors, no provenance, detached and chain disagreement refuse', async () => {
  const fx = fixtures.BUILDERS['P13-transform-two-shapes']({});
  const page = await preview(fx.text);
  const analysis = analysisOf(fx.text);
  page.browser.aim(findNodes(page.scene, 'Shape')[0]);
  const base = page.adapter.pick(50, 50);
  const run = (patch, a = analysis) => VP.resolvePick({ snapshot: { ...base, ...patch }, currentCheck: null, analysis: a, currentText: fx.text });
  assert.equal(run({ ctxKind: 'proto-body' }).reason, 'proto-instance');
  assert.equal(run({ ctxKind: 'external-scene' }).status, 'REFUSED_EXTERNAL');
  assert.equal(run({ ctxKind: 'none' }).reason, 'runtime-node-detached');
  assert.equal(run({ sensors: ['TouchSensor'] }).status, 'REFUSED_SENSOR_CONFLICT');
  assert.equal(run({ outcome: 'no-hit', reason: 'no-geometry-under-pointer' }).status, 'NO_HIT');
  const g = base.graph;
  const shapeL = base.shape;
  const withShape = (p) => ({ graph: { ...g, [shapeL]: { ...g[shapeL], ...p } } });
  assert.equal(run(withShape({ occurrences: [] })).reason, 'runtime-node-has-no-parse-provenance');
  assert.equal(run(withShape({ occurrences: [g[shapeL].occurrences[0], g[shapeL].occurrences[0]] })).status, 'REFUSED_AMBIGUOUS');
  assert.equal(run(withShape({ parents: [] })).reason, 'runtime-node-detached');
  assert.equal(run(withShape({ parents: [...g[shapeL].parents, 'SCENE'] })).reason, 'runtime-node-has-several-live-parents');
  assert.equal(run(withShape({ parents: ['OTHER_CONTEXT'] })).reason, 'runtime-parent-outside-document');
  assert.equal(run(withShape({ parents: ['SCENE'] })).reason, 'runtime-chain-disagrees-with-source-containment');
  // off-by-one span never joins: G9 "off-by-one -> UNSUPPORTED"
  const o = g[shapeL].occurrences[0];
  assert.equal(run(withShape({ occurrences: [{ start: o.start + 1, end: o.end }] })).reason, 'exact-span-join-found-0');
  assert.equal(run(withShape({ occurrences: [{ start: o.start, end: o.end - 1 }] })).reason, 'exact-span-join-found-0');
  assert.equal(run(withShape({ type: 'Box' })).reason, 'exact-span-join-type-disagrees');
  // a damaged document proves nothing
  const damaged = analysisOf(`${fx.text}Transform {`);
  const r = VP.resolvePick({ snapshot: base, currentCheck: null, analysis: { ...damaged, text: fx.text }, currentText: fx.text });
  assert.equal(r.reason, 'document-has-syntax-errors');
  // a logical node absent from the scene tree
  const otherTree = vrml.sceneTree.buildSceneTree(vrml.parse(fx.text));
  assert.equal(run({}, { ...analysis, sceneTree: otherTree }).reason, 'logical-node-not-in-scene-tree');
});

test('G9: a refusal never touches sceneSelection; PROVEN is the only write', async () => {
  const fx = fixtures.BUILDERS['P9-def-use']({});
  const page = await preview(fx.text);
  const analysis = analysisOf(fx.text);
  const selection = SceneSelection.createSelectionController();
  selection.setSelection(analysis.sceneTree.items[1].id);
  const before = selection.getSelection();
  let fired = 0;
  selection.subscribe(() => { fired += 1; });
  const apply = (res) => { if (res.status === 'PROVEN') selection.setSelection(res.sceneTreeItemId); };
  apply(clickAndResolve(page, { type: 'Shape', nth: 0 }, { analysis, currentText: fx.text })); // ambiguous
  apply(clickAndResolve(page, null, { analysis, currentText: fx.text })); // no hit
  apply(VP.resolvePick({ snapshot: { outcome: 'disabled', reason: 'x' } }));
  assert.equal(selection.getSelection(), before);
  assert.equal(fired, 0);
});

test('promotion: only to the WD2-C recognized object of the SAME Shape, never generic', async () => {
  for (const [id, click, logicalIsShape] of [
    ['P3-wd2c-box', 'box', false],
    ['P13-transform-two-shapes', 'first', true],
    ['P8-def-without-use', 'def', false],
  ]) {
    const fx = fixtures.BUILDERS[id]({});
    const page = await preview(fx.text);
    const c = fx.clicks.find((k) => k.id === click);
    const res = clickAndResolve(page, c.aim, { analysis: analysisOf(fx.text), currentText: fx.text });
    assert.equal(res.status, 'PROVEN');
    assert.equal(res.source.logical.role, logicalIsShape ? 'shape' : 'simple-object', id);
    assert.equal(res.reason, logicalIsShape ? 'shape' : 'shape-promoted-to-proven-simple-object');
  }
});

test('no caret API in the resolver: results carry ids and spans, not editor commands', () => {
  const src = require('node:fs').readFileSync(path.join(__dirname, '..', '..', 'src', 'editor', 'viewport-pick.js'), 'utf8');
  for (const forbidden of ['setSelectionRange', 'scrollIntoView', 'dispatch(', 'navigateTo', 'EditorSelection']) {
    assert.ok(!src.includes(forbidden), `viewport-pick.js must not reference ${forbidden}`);
  }
});
