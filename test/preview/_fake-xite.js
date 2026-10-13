'use strict';
// WD2-D test support: a FAKE X_ITE with the private-surface SHAPES the pick
// adapter depends on (contract P1-P13), so node:test can drive the adapter's
// hook lifecycle, coordinator, abort, touch/getHit and snapshot end to end.
//
// The fake parser is a tiny recursive-descent reader of a VRML subset that,
// like X_ITE's VRMLParser, (a) keeps `input` = the exact string it was given
// and `lastIndex` = a UTF-16 index into it, (b) calls `this.comments()` first,
// and (c) dispatches every node statement through
// `VRMLParser.prototype.nodeStatement`, so a wrapper installed there sees each
// statement. It never rewrites the text. Statements run after an awaited
// "loadComponents" step, as in X_ITE (x3dScene, :33594).
//
// It is NOT an oracle: fixtures record their truth spans by construction
// (see _pick-fixtures.js), independent of both this fake and src/vrml.

const SENSOR_TYPES = new Set(['TouchSensor', 'PlaneSensor', 'CylinderSensor', 'SphereSensor']);
const SCALAR_WORDS = new Set(['TRUE', 'FALSE', 'NULL']);

function createFakeX3D() {
  class X3DBaseNode {
    constructor(type, ctx) {
      this.type = type;
      this.ctx = ctx;
      this.parents = new Set();
      this.children = []; // { field, node } in authored order
    }
    getTypeName() { return this.type; }
    getExecutionContext() { return this.ctx; }
    getParents() { return this.parents; }
  }
  // A field-like parent: not a node, not a context; climbs to its owner.
  class FakeField {
    constructor(owner, name) { this.owner = owner; this.name = name; }
    getParents() { return new Set([this.owner]); }
  }
  class X3DExecutionContext {
    constructor() { this.rootField = new FakeField(this, 'rootNodes'); this.rootNodes = []; this.defs = new Map(); }
  }
  class X3DScene extends X3DExecutionContext {}

  function VRMLParser(scene, input) {
    this.scene = scene;
    this.input = input;
    this.lastIndex = 0;
  }
  const P = VRMLParser.prototype;
  P.getScene = function () { return this.scene; };
  P.getExecutionContext = function () { return this.scene; };
  P.isInsideProtoDeclaration = function () { return false; };
  P.comments = function () {
    const s = this.input;
    let i = this.lastIndex;
    for (;;) {
      const c = s[i];
      if (c === ' ' || c === '\t' || c === '\n' || c === '\r' || c === ',' || c === '\uFEFF') { i += 1; continue; }
      if (c === '#') { while (i < s.length && s[i] !== '\n' && s[i] !== '\r') i += 1; continue; }
      break;
    }
    this.lastIndex = i;
    return true;
  };
  P.word = function () {
    this.comments();
    const m = /^[^\s,#{}\[\]"0-9+\-.][^\s,#{}\[\]"]*/.exec(this.input.slice(this.lastIndex));
    if (!m) return null;
    this.lastIndex += m[0].length;
    return m[0];
  };
  P.peek = function () { this.comments(); return this.input[this.lastIndex]; };
  P.peekWord = function () { const save = this.lastIndex; const w = this.word(); this.lastIndex = save; return w; };
  P.expect = function (ch) {
    if (this.peek() !== ch) throw new Error(`fake parse error: expected '${ch}' at ${this.lastIndex}`);
    this.lastIndex += 1;
  };
  P.isNodeStart = function () {
    const w = this.peekWord();
    return !!w && !SCALAR_WORDS.has(w) && (w === 'DEF' || w === 'USE' || /^[A-Z]/.test(w));
  };
  P.scalar = function () {
    const s = this.input;
    this.comments();
    if (s[this.lastIndex] === '"') {
      let i = this.lastIndex + 1;
      while (i < s.length && s[i] !== '"') i += s[i] === '\\' ? 2 : 1;
      if (i >= s.length) throw new Error('fake parse error: unterminated string');
      this.lastIndex = i + 1;
      return;
    }
    const m = /^(TRUE|FALSE|NULL|[+\-]?[0-9.][0-9.eE+\-]*)/.exec(s.slice(this.lastIndex));
    if (!m) throw new Error(`fake parse error: bad value at ${this.lastIndex}`);
    this.lastIndex += m[0].length;
  };
  P.link = function (owner, fieldName, child) {
    let field = owner.fieldObjects && owner.fieldObjects.get(fieldName);
    if (!owner.fieldObjects) owner.fieldObjects = new Map();
    if (!field) { field = new FakeField(owner, fieldName); owner.fieldObjects.set(fieldName, field); }
    child.parents.add(field);
    owner.children.push({ field: fieldName, node: child });
  };
  // ONE node statement: DEF name Node | USE name | Node. Zero formal params.
  P.nodeStatement = function () {
    this.comments();
    const w = this.word();
    if (w === 'USE') {
      const name = this.word();
      const node = this.scene.defs.get(name);
      if (!node) throw new Error(`fake parse error: unknown USE ${name}`);
      return node;
    }
    if (w === 'DEF') {
      const name = this.word();
      const node = this.node(this.word());
      this.scene.defs.set(name, node);
      return node;
    }
    return this.node(w);
  };
  P.node = function (type) {
    if (!type || !/^[A-Z]/.test(type)) throw new Error(`fake parse error: node type expected at ${this.lastIndex}`);
    const node = new X3DBaseNode(type, this.scene);
    this.expect('{');
    for (;;) {
      if (this.peek() === '}') { this.lastIndex += 1; break; }
      const field = this.word();
      if (!field) throw new Error(`fake parse error: field expected at ${this.lastIndex}`);
      if (this.peek() === '[') {
        this.lastIndex += 1;
        for (;;) {
          if (this.peek() === ']') { this.lastIndex += 1; break; }
          if (this.isNodeStart()) this.link(node, field, this.nodeStatement());
          else this.scalar();
        }
      } else if (this.isNodeStart()) {
        this.link(node, field, this.nodeStatement());
      } else {
        do { this.scalar(); } while (/[0-9+\-.]/.test(this.peek() || ''));
      }
    }
    if (type === 'Inline') {
      // The Inline's content lives in its OWN scene (another document).
      const ext = new X3DScene();
      const shape = new X3DBaseNode('Shape', ext);
      shape.parents.add(ext.rootField);
      ext.rootNodes.push(shape);
      node.inlineShape = shape;
    }
    return node;
  };

  const X3D = { X3DBaseNode, X3DExecutionContext, X3DScene, VRMLParser, FakeField };
  return X3D;
}

// One fake browser per <x3d-canvas>. `gate`: an optional async step between
// the header and the statements (X_ITE's awaited loadComponents).
function createFakeBrowser(X3D, { version = '15.1.10', rect = { left: 0, top: 0, width: 100, height: 100 }, viewport = [0, 0, 100, 100] } = {}) {
  const hit = { id: 0, shapeNode: null, layerNode: null, sensors: new Map() };
  let layer0 = null;
  let aimed = null;
  const calls = { touch: 0, getHit: 0, parse: 0 };
  const browser = {
    version,
    currentScene: null,
    element: { getBoundingClientRect: () => ({ ...rect }) },
    viewerActive: false,
    gate: null,
    calls,
    async createX3DFromString(text) {
      calls.parse += 1;
      const scene = new X3D.X3DScene();
      const parser = new X3D.VRMLParser(scene, text);
      if (browser.gate) await browser.gate(text);
      else await Promise.resolve();
      parser.comments();
      while (parser.lastIndex < text.length) {
        const node = parser.nodeStatement();
        node.parents.add(scene.rootField);
        scene.rootNodes.push(node);
        parser.comments();
      }
      return scene;
    },
    async replaceWorld(scene) {
      const groupNodes = new X3D.X3DBaseNode('Group', scene);
      const groupNode = new X3D.X3DBaseNode('Group', scene);
      layer0 = { groupNode, groupNodes };
      for (const n of scene.rootNodes) n.parents.add(groupNodes);
      browser.currentScene = scene;
    },
    // aim(node): what the next touch() hits (null = background).
    aim(node) { aimed = node; },
    touch(x, y) {
      calls.touch += 1;
      browser.lastTouch = { x, y };
      if (browser.viewerActive) return false; // leaves the previous hit in place
      hit.sensors.clear();
      if (!aimed) { hit.id = 0; hit.shapeNode = null; return false; }
      hit.id = 1;
      hit.shapeNode = aimed;
      for (const s of sensorsAbove(aimed)) hit.sensors.set(s, { node: s });
      return true;
    },
    getHit() { calls.getHit += 1; return hit; },
    getWorld() { return { getLayer0: () => layer0 }; },
    // Like X_ITE's SFVec4f field: indexable [x, y, w, h], NOT an Array.
    getViewport() { return { 0: viewport[0], 1: viewport[1], 2: viewport[2], 3: viewport[3] }; },
  };
  return browser;
}

// Pointing-device sensors that would be registered for a hit on `shape`: any
// sensor sibling of an ancestor chain node, and any Anchor ancestor.
function sensorsAbove(shape) {
  const out = [];
  const seen = new Set();
  const stack = [shape];
  while (stack.length) {
    const n = stack.pop();
    if (!n || seen.has(n) || typeof n.getParents !== 'function') continue;
    seen.add(n);
    for (const p of n.getParents()) {
      const owner = p && p.owner;
      if (!owner || !owner.children) continue;
      if (owner.type === 'Anchor') out.push(owner);
      for (const c of owner.children) if (SENSOR_TYPES.has(c.node.type)) out.push(c.node);
      stack.push(owner);
    }
  }
  return out;
}

// Find the runtime node of `type` that is the k-th such node in authored order.
function findNodes(scene, type) {
  const out = [];
  const seen = new Set();
  const visit = (n) => {
    if (!n || seen.has(n)) return;
    seen.add(n);
    if (n.type === type) out.push(n);
    for (const c of n.children || []) visit(c.node);
  };
  for (const r of scene.rootNodes) visit(r);
  return out;
}

// The adapter has no "ready" API: its G4 probe settles when compatibility()
// leaves 'compatibility-unproven'. Poll the public method across ticks.
async function settleProbe(adapter, ticks = 200) {
  for (let i = 0; i < ticks; i++) {
    const c = adapter.compatibility();
    if (c.ok || c.reason !== 'compatibility-unproven') return c;
    await new Promise((r) => setImmediate(r));
  }
  return adapter.compatibility();
}

// The engines' (renderer/preview.js, world-preview.js) pointerup check, for
// tests that drive the adapter directly: the generation this "engine"
// activated, with its scene, must still be the one on screen.
function createShownTracker(adapter, browser) {
  let shown = null;
  return {
    activate(generation, scene) {
      shown = generation && adapter.activate(generation, scene) ? { generation, scene } : null;
      return !!shown;
    },
    retire(reason) { shown = null; adapter.abort(); adapter.retire(reason); },
    currentCheck(generation) {
      if (!shown || !generation || shown.generation !== generation) return 'hit-from-another-preview-generation';
      if (browser.currentScene !== shown.scene) return 'preview-scene-replaced';
      return null;
    },
  };
}

module.exports = { createFakeX3D, createFakeBrowser, findNodes, settleProbe, createShownTracker };
