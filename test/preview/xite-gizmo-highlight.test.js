'use strict';
// B2 selection highlight: the two read-only additions to the Tauri gizmo
// adapter (apps/desktop-tauri/ui/static/xite-gizmo-adapter.js) -- bounds()
// and ancestor() -- against a fake X_ITE, plus source guards: the private
// bounding-box surfaces live in that adapter only, the adapter version gate
// covers them, and nothing in the highlight path writes to the scene.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.join(__dirname, '..', '..');
const TAURI = path.join(ROOT, 'apps', 'desktop-tauri');
const GIZMO = path.join(TAURI, 'ui', 'static', 'xite-gizmo-adapter.js');
const PREVIEW = path.join(TAURI, 'ui', 'static', 'preview-adapter.js');
const code = (f) => fs.readFileSync(f, 'utf8').replace(/\r\n?/g, '\n')
  .replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"`\\])\/\/.*$/gm, '$1');

function load() {
  const sandbox = { window: {} };
  vm.runInNewContext(fs.readFileSync(GIZMO, 'utf8'), sandbox, { filename: GIZMO });
  return sandbox.window.WrlXiteGizmoAdapter;
}

// The adapter runs in another realm: compare as plain data.
const plain = (o) => JSON.parse(JSON.stringify(o));
const vec = (x, y, z) => ({ x, y, z });
const rot = (x, y, z, angle) => ({ x, y, z, angle });

// The fake X3D: Box3 holds a matrix; SFNode wraps a fake runtime node and
// exposes its fields read-only (a write throws: the highlight never writes).
function fakeX3D() {
  function Box3() { this.matrix = new Array(16).fill(0); }
  function SFNode(node) {
    const self = this;
    Object.defineProperty(this, '_n', { value: node });
    for (const [k, v] of Object.entries(node.fields || {})) {
      Object.defineProperty(self, k, { get: () => v, set: () => { throw new Error(`write ${k}`); }, enumerable: true });
    }
  }
  SFNode.prototype.getValue = function () { return this._n; };
  SFNode.prototype.getNodeTypeName = function () { return this._n.type; };
  function Matrix4() { this.m = 'identity'; }
  return { SFVec3f: function () {}, SFNode, Box3, Matrix4 };
}
const fakeBrowser = (version = '16.5.0') => ({ version, getActiveLayer() {}, getViewport() {} });

// A runtime node whose getBBox writes box (size, center) into the argument.
function boxed(type, size, center, extra = {}) {
  return Object.assign({
    type,
    getBBox(b) {
      b.matrix = [size[0] / 2, 0, 0, 0, 0, size[1] / 2, 0, 0, 0, 0, size[2] / 2, 0, center[0], center[1], center[2], 1];
      return b;
    },
  }, extra);
}

test('bounds(): the proven node\'s Box3 matrix as 16 plain numbers', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const r = A.bounds(boxed('Shape', [2, 4, 6], [1, 2, 3]));
  assert.equal(r.ok, true);
  assert.deepEqual(Array.from(r.box), [1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 3, 0, 1, 2, 3, 1]);
  assert.ok(Array.isArray(r.box) || r.box.length === 16);
});

test('bounds(): a geometry node returning its own box (argument ignored) is read from the return value', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const own = { matrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1] };
  const r = A.bounds({ type: 'Box', getBBox() { return own; } });
  assert.deepEqual(Array.from(r.box), own.matrix);
});

test('bounds(): a PROTO instance is measured through getInnerNode', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const inner = boxed('Transform', [2, 2, 2], [5, 0, 0]);
  const r = A.bounds({ type: 'MyProto', getInnerNode() { return inner; } });
  assert.equal(r.ok, true);
  assert.equal(r.box[12], 5);
  const broken = A.bounds({ type: 'MyProto', getInnerNode() { throw new Error('Root node not available.'); } });
  assert.deepEqual(plain(broken), { ok: false, reason: 'node-has-no-rendered-bounds' });
});

test('bounds(): refusals -- no getBBox, non-finite box, no node, wrong X_ITE, no Box3', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  assert.deepEqual(plain(A.bounds({ type: 'Material' })), { ok: false, reason: 'node-has-no-rendered-bounds' });
  assert.deepEqual(plain(A.bounds(boxed('Shape', [NaN, 1, 1], [0, 0, 0]))), { ok: false, reason: 'node-has-no-rendered-bounds' });
  assert.deepEqual(plain(A.bounds({ type: 'Shape', getBBox() { throw new Error('x'); } })), { ok: false, reason: 'node-has-no-rendered-bounds' });
  assert.deepEqual(plain(A.bounds(null)), { ok: false, reason: 'runtime-node-not-proven' });
  const old = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser('15.1.10') });
  assert.deepEqual(plain(old.bounds(boxed('Shape', [1, 1, 1], [0, 0, 0]))), { ok: false, reason: 'bounds-unavailable' });
  const X = fakeX3D(); delete X.Box3;
  const nobox = load().createXiteGizmoAdapter({ X3D: X, browser: fakeBrowser() });
  assert.deepEqual(plain(nobox.bounds(boxed('Shape', [1, 1, 1], [0, 0, 0]))), { ok: false, reason: 'bounds-unavailable' });
});

test('ancestor(): a Transform reports its rendered fields; other types only their type; nothing is written', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const t = {
    type: 'Transform',
    fields: { translation: vec(1, 2, 3), rotation: rot(0, 1, 0, 0.5), scale: vec(2, 2, 2), scaleOrientation: rot(0, 0, 1, 0), center: vec(0, 1, 0) },
  };
  assert.deepEqual(JSON.parse(JSON.stringify(A.ancestor(t))), {
    type: 'Transform',
    frame: { translation: [1, 2, 3], rotation: [0, 1, 0, 0.5], scale: [2, 2, 2], scaleOrientation: [0, 0, 1, 0], center: [0, 1, 0] },
  });
  assert.deepEqual(JSON.parse(JSON.stringify(A.ancestor({ type: 'Group' }))), { type: 'Group' });
  const bad = { type: 'Transform', fields: Object.assign({}, t.fields, { translation: vec(NaN, 0, 0) }) };
  assert.equal(A.ancestor(bad), null);
  assert.equal(A.ancestor(null), null);
});

test('source: the private bounding-box surfaces live in the gizmo adapter only', () => {
  const giz = code(GIZMO);
  for (const re of [/\.getBBox\(/, /\bX3D\.Box3\b/, /\.getInnerNode\(/, /\.getShapes\(/, /\.getVertices\(/]) assert.match(giz, re);
  const others = [PREVIEW, ...fs.readdirSync(path.join(TAURI, 'ui', 'src'), { recursive: true })
    .filter((f) => String(f).endsWith('.rs')).map((f) => path.join(TAURI, 'ui', 'src', String(f)))];
  for (const f of others) {
    const src = code(f);
    for (const re of [/getBBox/, /\bBox3\b/, /getInnerNode/, /getShapes/, /getVertices/]) assert.doesNotMatch(src, re, `${path.relative(ROOT, f)}: ${re}`);
  }
});

test('source: bounds() and ancestor() sit behind the exact-version compatibility gate and never write', () => {
  const giz = code(GIZMO);
  const body = (name) => {
    const i = giz.indexOf(`function ${name}(`);
    assert.ok(i > 0, name);
    return giz.slice(i, giz.indexOf('\n    }\n', i));
  };
  for (const name of ['bounds', 'ancestor', 'shapes']) {
    const b = body(name);
    assert.match(b, /compatibility\(\)\.ok/, `${name} checks compatibility`);
    assert.doesNotMatch(b, /\.\w+\s*=(?!=)|setField|\.set\(|replaceWorld|addChildObject/, `${name} writes nothing`);
  }
  // The preview entry point reaches nodes only through nodeAt.
  const prev = code(PREVIEW);
  const i = prev.indexOf('highlightBounds: function');
  const hb = prev.slice(i, prev.indexOf('\n    },\n', i));
  assert.match(hb, /a\.nodeAt\(gen, Number\(from\), Number\(to\)\)/);
  assert.doesNotMatch(hb, /getNamedNode|rootNodes|getParents|pick\(|\.find\(|indexOf/, 'no search, name or index lookup');
});

test('G3 (Tauri 16.5.0): the bounding-box surfaces exist in the fetched bundle', (t) => {
  const dist = path.join(TAURI, 'target', 'xite', '16.5.0', 'dist', 'x_ite.min.js');
  if (!fs.existsSync(dist)) return t.skip('run apps/desktop-tauri/fetch-xite.sh first (build-ui.sh does)');
  const min = fs.readFileSync(dist, 'utf8');
  for (const s of ['add("Box3"', 'getBBox(', 'getInnerNode(']) assert.ok(min.includes(s), `16.5.0 x_ite.min.js no longer contains ${s}`);
});

// ---- LV2 precise outline: shapes() ------------------------------------------

// A fake X_ITE geometry node: getVertices() is x y z w per vertex and
// throws on a write (the outline never writes).
function geometry(k, xyzw) {
  const verts = Object.freeze(xyzw.slice());
  return { getGeometryType() { return k; }, getVertices() { return verts; } };
}
const M = (tx) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, tx, 0, 0, 1];

test('shapes(): X_ITE\'s own shape list of the proven node -- matrices, types, counts, and x y z when asked', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const tri = geometry(3, [0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1]);
  const line = geometry(1, [0, 0, 0, 1, 2, 0, 0, 1]);
  let seen = null;
  const node = {
    getShapes(list, m) {
      seen = m;
      list.push({ modelMatrix: M(5), shapeNode: { getGeometry() { return tri; } } });
      list.push({ modelMatrix: M(-1), shapeNode: { getGeometry() { return line; } } });
      return list;
    },
  };
  const counts = plain(A.shapes(node, false));
  assert.equal(seen.m, 'identity', 'measured from the node\'s parent frame');
  assert.deepEqual(counts, { ok: true, shapes: [{ m: M(5), k: 3, n: 3 }, { m: M(-1), k: 1, n: 2 }] });
  const full = plain(A.shapes(node, true));
  assert.deepEqual(full.shapes[0].v, [0, 0, 0, 1, 0, 0, 0, 1, 0]);
  assert.deepEqual(full.shapes[1].v, [0, 0, 0, 2, 0, 0]);
  // A PROTO instance is measured through getInnerNode.
  assert.equal(plain(A.shapes({ getInnerNode() { return node; } }, false)).shapes.length, 2);
});

test('shapes(): a geometry node is its own single shape (identity matrix)', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const r = plain(A.shapes(geometry(3, [0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1]), true));
  assert.deepEqual(r.shapes[0].m, M(0));
  assert.equal(r.shapes[0].n, 3);
});

test('shapes(): refusals -- too detailed, an unreadable shape, no geometry, wrong X_ITE, no Matrix4', () => {
  const A = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser() });
  const big = geometry(3, new Array(4 * 9).fill(1));
  const one = (g) => ({ getShapes(list) { list.push({ modelMatrix: M(0), shapeNode: { getGeometry() { return g; } } }); return list; } });
  assert.deepEqual(plain(A.shapes(one(big), false, 6)), { ok: false, reason: 'geometry-too-detailed' });
  assert.deepEqual(plain(A.shapes(one(null), false)), { ok: false, reason: 'geometry-unavailable' });
  assert.deepEqual(plain(A.shapes(one(geometry(3, [0, 0, 0])), false)), { ok: false, reason: 'geometry-unavailable' });
  assert.deepEqual(plain(A.shapes(one(geometry(3, [NaN, 0, 0, 1])), true)), { ok: false, reason: 'geometry-unavailable' });
  assert.deepEqual(plain(A.shapes({ type: 'Material' }, false)), { ok: false, reason: 'node-has-no-rendered-geometry' });
  assert.deepEqual(plain(A.shapes(null, false)), { ok: false, reason: 'runtime-node-not-proven' });
  const old = load().createXiteGizmoAdapter({ X3D: fakeX3D(), browser: fakeBrowser('15.1.10') });
  assert.deepEqual(plain(old.shapes(one(big), false)), { ok: false, reason: 'geometry-unavailable' });
  const X = fakeX3D(); delete X.Matrix4;
  assert.deepEqual(plain(load().createXiteGizmoAdapter({ X3D: X, browser: fakeBrowser() }).shapes(one(big), false)), { ok: false, reason: 'geometry-unavailable' });
});

test('G3 (Tauri 16.5.0): the shape-list surfaces exist in the fetched bundle', (t) => {
  const dist = path.join(TAURI, 'target', 'xite', '16.5.0', 'dist', 'x_ite.min.js');
  if (!fs.existsSync(dist)) return t.skip('run apps/desktop-tauri/fetch-xite.sh first (build-ui.sh does)');
  const min = fs.readFileSync(dist, 'utf8');
  for (const s of ['add("Matrix4"', 'getShapes(e,t){', 'getVertices(){return this.vertices}', 'getGeometryType(){return this.geometryType}', 'getGeometry(){return this.geometryNode}']) {
    assert.ok(min.includes(s), `16.5.0 x_ite.min.js no longer contains ${s}`);
  }
});
