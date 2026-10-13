'use strict';
// B3 -- the pick adapter's PROTO body binding (snapshotBodySpan, bodyNodeAt)
// over the fake X_ITE. A PROTO body statement has one runtime COPY per
// instance; the Move / Rotate / Scale gizmo must bind the copy of the
// instance the user PICKED, and refuse when that cannot be proven. Spans are
// computed from the text by construction, never from the adapter.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');

const { createFakeX3D, createFakeBrowser, settleProbe } = require('./_fake-xite');

const ADAPTER = path.join(__dirname, '..', '..', 'src', 'preview', 'xite-pick-adapter.js');
function freshPage() {
  delete require.cache[require.resolve(ADAPTER)];
  const A = require(ADAPTER);
  const X3D = createFakeX3D();
  return { A, X3D };
}

async function shown(page, browser, text) {
  const adapter = page.A.createXitePickAdapter({ X3D: page.X3D, browser });
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  const { scene, generation } = await adapter.parseWithProvenance(text, { sessionId: 7, generationId: 1 });
  await browser.replaceWorld(scene);
  assert.ok(adapter.activate(generation, scene));
  return { adapter, scene, generation };
}

const XF = 'DEF CarXForm Transform { translation 0 0 0 children [ Shape { geometry Box { } } ] }';
const GROUP = `Group { children [ ${XF} ] }`;
const DECL = `PROTO Car [ field SFVec3f size 1 1 1 ] { ${GROUP} }\n`;

function spanOf(text, needle) {
  const start = text.indexOf(needle);
  assert.ok(start >= 0, needle);
  return { start, end: start + needle.length };
}

// The instances (top-level statements of the proto type), in source order.
const instances = (scene) => scene.rootNodes.filter((n) => typeof n.getProtoDeclaration === 'function' && n.getProtoDeclaration() !== n);
// An instance's copy of the body Transform, and of its Shape.
const copyOf = (inst) => inst.bodyRoots[0].children[0].node;
const shapeOf = (inst) => copyOf(inst).children[0].node;

const plain = (v, X3D) => {
  const walk = (x) => {
    if (x && typeof x === 'object') {
      assert.ok(!(x instanceof X3D.X3DBaseNode) && !(x instanceof X3D.X3DExecutionContext), 'runtime object in a snapshot');
      for (const k of Object.keys(x)) walk(x[k]);
    }
  };
  walk(v);
};

test('one instance: the body span binds THAT instance\'s copy, proven through BODY: to the scene', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const text = `#VRML V2.0 utf8\n${DECL}Car { }\n`;
  const { adapter, scene, generation } = await shown(page, browser, text);
  const xf = spanOf(text, XF);
  const [inst] = instances(scene);
  const want = copyOf(inst);
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), want);
  // The document-only lookup never answers for a body statement.
  assert.equal(adapter.nodeAt(generation, xf.start, xf.end), null);
  const s = adapter.snapshotBodySpan(generation, xf.start, xf.end);
  assert.equal(s.outcome, 'found');
  plain(s, page.X3D);
  const n1 = s.graph[s.shape];
  assert.equal(n1.type, 'Transform');
  assert.equal(n1.ctxKind, 'proto-body');
  assert.deepEqual(n1.occurrences.map((o) => [o.start, o.end]), [[xf.start, xf.end]]);
  // Transform -> Group -> BODY:instance -> SCENE.
  const g = s.graph[n1.parents[0]];
  assert.equal(g.type, 'Group');
  const body = g.parents.find((p) => p.startsWith('BODY:'));
  assert.ok(body, JSON.stringify(g.parents));
  const i = s.graph[body.slice(5)];
  assert.equal(i.type, 'Car');
  assert.equal(i.ctxKind, 'document');
  const is = spanOf(text, 'Car { }');
  assert.deepEqual(i.occurrences.map((o) => [o.start, o.end]), [[is.start, is.end]]);
  assert.ok(i.parents.includes('SCENE'));
  // The highlight's ancestors: the copy's runtime chain, span by span.
  const gs = spanOf(text, GROUP);
  assert.deepEqual(adapter.chainOf(generation, want, [gs.start, gs.end, is.start, is.end]), [inst.bodyRoots[0], inst]);
  assert.equal(adapter.chainOf(generation, want, [gs.start, gs.end + 1, is.start, is.end]), null, 'a wrong span');
  assert.equal(adapter.chainOf(generation, want, [is.start, is.end]), null, 'a skipped link');
  assert.equal(adapter.chainOf(generation, want, [gs.start]), null);
  // Not a body statement, or not exact: nothing.
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end - 1), null);
  assert.equal(adapter.bodyNodeAt(generation, is.start, is.end), null);
  assert.equal(adapter.snapshotBodySpan(generation, xf.start + 1, xf.end).outcome, 'unsupported');
});

test('several instances: the PICKED instance binds; without a pick nothing does; a binding never re-targets', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const text = `#VRML V2.0 utf8\n${DECL}Car { }\nCar { }\n`;
  const { adapter, scene, generation } = await shown(page, browser, text);
  const xf = spanOf(text, XF);
  const [a, b] = instances(scene);
  assert.notEqual(copyOf(a), copyOf(b));
  // No pick yet: which instance is meant is not proven.
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), null);
  const none = adapter.snapshotBodySpan(generation, xf.start, xf.end);
  assert.deepEqual([none.outcome, none.reason], ['unsupported', 'proto-instance-not-picked']);
  // Pick instance B's Shape: B's copy, never A's.
  browser.aim(shapeOf(b));
  assert.equal(adapter.pick(50, 50).outcome, 'hit');
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), copyOf(b));
  assert.equal(adapter.snapshotBodySpan(generation, xf.start, xf.end).outcome, 'found');
  // `keep` re-proves an earlier binding while it is still a candidate,
  // whatever the newest pick says; anything else is refused.
  browser.aim(shapeOf(a));
  adapter.pick(50, 50);
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), copyOf(a));
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end, copyOf(b)), copyOf(b));
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end, {}), null);
  // A pick of something that is no instance proves nothing.
  browser.aim(null);
  adapter.pick(50, 50);
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), copyOf(a), 'a miss keeps the last hit');
});

test('a stale or foreign generation binds nothing', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const text = `#VRML V2.0 utf8\n${DECL}Car { }\n`;
  const { adapter, generation } = await shown(page, browser, text);
  const xf = spanOf(text, XF);
  assert.ok(adapter.bodyNodeAt(generation, xf.start, xf.end));
  assert.equal(adapter.bodyNodeAt(Object.freeze({}), xf.start, xf.end), null);
  assert.equal(adapter.bodyNodeAt(null, xf.start, xf.end), null);
  adapter.retire('preview-scene-replaced');
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), null);
  assert.equal(adapter.snapshotBodySpan(generation, xf.start, xf.end).outcome, 'stale');
});

test('X_ITE copying a named body node twice per instance (the same copy returned) is still ONE candidate', async () => {
  const page = freshPage();
  const browser = createFakeBrowser(page.X3D);
  const text = `#VRML V2.0 utf8\n${DECL}Car { }\n`;
  const { adapter, scene, generation } = await shown(page, browser, text);
  const xf = spanOf(text, XF);
  // The fake copies DEF CarXForm first, then meets it again in the tree.
  assert.equal(adapter.bodyNodeAt(generation, xf.start, xf.end), copyOf(instances(scene)[0]));
});
