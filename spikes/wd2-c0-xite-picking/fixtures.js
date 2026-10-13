'use strict';
// WD2-C0 fixture builder + ORACLE (expected truth) for the X_ITE picking spike.
//
// INDEPENDENCE: this module is the grading truth. It must never require the
// mapping under test (mapping.js) nor anything under src/ -- a truth that asks
// the implementation what the answer is would not be evidence. It composes each
// fixture's VRML text by plain string concatenation and records the exact span
// [start,end) of every occurrence AS IT WRITES IT, so the expected source
// occurrence of every click is known from authorship, not from any parser.
// test/oracle-independence.test.js asserts the absence of those requires.
//
// The fixtures are spike-owned, minimal and redistributable (no Cybertown
// content). Nothing is written into the VRML for picking's benefit: the only
// DEF names present are the ones a fixture is *about* (DEF/USE cases) plus one
// named Viewpoint in the camera fixture, which is ordinary authored content.
//
// Every click names the world-space point the user aims at, the camera, and
// the expected outcome:
//   expect.status   PROVEN | REFUSED_AMBIGUOUS | REFUSED_EXTERNAL |
//                   REFUSED_SENSOR_CONFLICT | UNSUPPORTED | NO_HIT
//   expect.clicked  label of the authored occurrence the user actually clicked
//                   (for a USE instance: that USE statement) -- always defined
//                   for a geometry click, even when refusal is expected
//   expect.logical  label of the occurrence a PROVEN selection must equal
//                   (Shape, or its simple-object Transform when the WD2-C
//                   promotion rule applies)

const HEADER = '#VRML V2.0 utf8\n';
const FRONT_VIEWPOINT = 'Viewpoint { position 0 0 20 description "front" }\n';

class Doc {
  constructor() {
    this.s = HEADER;
    this.spans = Object.create(null);
    this.stack = [];
  }
  put(t) { this.s += t; return this; }
  open(label, t) { this.stack.push([label, this.s.length]); this.s += t; return this; }
  close(t) {
    this.s += t;
    const [label, start] = this.stack.pop();
    this.span(label, start, this.s.length);
    return this;
  }
  leaf(label, t) {
    const start = this.s.length;
    this.s += t;
    this.span(label, start, this.s.length);
    return this;
  }
  span(label, start, end) {
    if (label in this.spans) throw new Error(`duplicate oracle label ${label}`);
    this.spans[label] = Object.freeze({ start, end });
  }
  done() {
    if (this.stack.length) throw new Error('unclosed oracle span');
    return { text: this.s, spans: Object.freeze({ ...this.spans }) };
  }
}

// A WD2-C First Object, byte-identical to simpleObjectTemplate() output except
// for an optional `translation` line (what a later WD2-C Position edit adds).
// Written by hand here -- NOT by requiring src/vrml/node-templates.js -- so the
// oracle stays independent; spike.test.js separately asserts the bytes match
// the production template, so the fixture is the real First Object shape.
function firstObject(d, prefix, primitive, translation) {
  d.open(`${prefix}.transform`, 'Transform {\n');
  if (translation) d.put(`  translation ${translation}\n`);
  d.put('  children [\n    ');
  d.open(`${prefix}.shape`, 'Shape {\n      appearance Appearance {\n        material Material {\n        }\n      }\n');
  d.put(`      geometry ${primitive} {\n      }\n    `);
  d.close('}');
  d.put('\n  ]\n');
  d.close('}');
  d.put('\n');
}

// Anonymous `Transform { translation t children [ Shape { geometry G } ] }`.
function simpleTransform(d, prefix, translation, geometry) {
  d.open(`${prefix}.transform`, `Transform { translation ${translation} children [ `);
  d.leaf(`${prefix}.shape`, `Shape { geometry ${geometry} }`);
  d.close(' ] }');
  d.put('\n');
}

const P = (expect, world, extra) => Object.freeze({ world, expect: Object.freeze(expect), ...(extra || {}) });
const proven = (clicked, logical) => ({ status: 'PROVEN', clicked, logical: logical || clicked });
const refused = (status, clicked) => ({ status, clicked });

function build() {
  const F = [];
  const add = (id, title, d, clicks, extra) => {
    const { text, spans } = d.done();
    F.push(Object.freeze({ id, title, text, spans, clicks: Object.freeze(clicks), ...(extra || {}) }));
  };
  let d;

  // P1 -- root Shape.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.leaf('box.shape', 'Shape {\n  geometry Box { size 4 4 4 }\n}');
  d.put('\n');
  add('P1-root-shape', 'Root Shape', d, [
    { id: 'box', ...P(proven('box.shape'), [0, 0, 2]) },
    { id: 'background', ...P({ status: 'NO_HIT' }, [7, 7, 0]) },
  ]);

  // P2 -- simple Transform (no Appearance): promotes to the Transform.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('t.transform', 'Transform {\n  children [\n    ');
  d.leaf('t.shape', 'Shape {\n      geometry Box { size 4 4 4 }\n    }');
  d.close('\n  ]\n}');
  d.put('\n');
  add('P2-simple-transform', 'Simple Transform', d, [
    { id: 'box', ...P(proven('t.shape', 't.transform'), [0, 0, 2]) },
  ]);

  // P3 -- the exact WD2-C First Object (Box) at the origin.
  d = new Doc().put(FRONT_VIEWPOINT);
  firstObject(d, 'fo', 'Box');
  add('P3-wd2c-first-object', 'WD2-C First Object Box', d, [
    { id: 'box', ...P(proven('fo.shape', 'fo.transform'), [0, 0, 1]) },
  ]);

  // P4 -- WD2-C Box and Sphere side by side.
  d = new Doc().put(FRONT_VIEWPOINT);
  firstObject(d, 'box', 'Box', '-3 0 0');
  firstObject(d, 'sphere', 'Sphere', '3 0 0');
  add('P4-box-and-sphere', 'WD2-C Box + Sphere', d, [
    { id: 'box', ...P(proven('box.shape', 'box.transform'), [-3, 0, 1]) },
    { id: 'sphere', ...P(proven('sphere.shape', 'sphere.transform'), [3, 0, 1]) },
    { id: 'box-again', ...P(proven('box.shape', 'box.transform'), [-3, 0.5, 1]) },
  ]);

  // P5 -- identical anonymous twins.
  d = new Doc().put(FRONT_VIEWPOINT);
  simpleTransform(d, 'left', '-2 0 0', 'Box { }');
  simpleTransform(d, 'right', '2 0 0', 'Box { }');
  const p5 = d.done();
  F.push(Object.freeze({
    id: 'P5-anonymous-twins', title: 'Identical anonymous twins', text: p5.text, spans: p5.spans,
    clicks: Object.freeze([
      { id: 'left', ...P(proven('left.shape', 'left.transform'), [-2, 0, 1]) },
      { id: 'right', ...P(proven('right.shape', 'right.transform'), [2, 0, 1]) },
      { id: 'right-2', ...P(proven('right.shape', 'right.transform'), [2.4, -0.4, 1]) },
      { id: 'left-2', ...P(proven('left.shape', 'left.transform'), [-1.6, 0.4, 1]) },
    ]),
  }));

  // P6 -- 12 identical anonymous siblings, clicked 3 rounds in varied order.
  d = new Doc().put(FRONT_VIEWPOINT);
  const N6 = 12;
  const xs = [];
  for (let i = 0; i < N6; i++) {
    const x = -6.6 + i * 1.2;
    xs.push(x);
    simpleTransform(d, `s${i}`, `${x.toFixed(1)} 0 0`, 'Box { size 0.8 0.8 0.8 }');
  }
  const orders = [
    [...Array(N6).keys()],
    [...Array(N6).keys()].reverse(),
    [5, 0, 11, 3, 8, 1, 10, 6, 2, 9, 4, 7],
  ];
  const c6 = [];
  orders.forEach((ord, r) => ord.forEach((i) => c6.push({
    id: `r${r}-s${i}`, ...P(proven(`s${i}.shape`, `s${i}.transform`), [xs[i], 0, 0.4]),
  })));
  add('P6-many-siblings', '12 identical anonymous siblings', d, c6);

  // P7 -- identical nested structures (two outer groups, each two inner leaves).
  d = new Doc().put(FRONT_VIEWPOINT);
  for (const [side, x] of [['L', -3], ['R', 3]]) {
    d.open(`${side}.outer`, `Transform { translation ${x} 0 0 children [\n`);
    for (const [pos, y] of [['up', 2], ['down', -2]]) {
      d.put('  ');
      simpleTransform(d, `${side}.${pos}`, `0 ${y} 0`, 'Box { size 1.5 1.5 1.5 }');
    }
    d.close('] }');
    d.put('\n');
  }
  const c7 = [];
  for (const side of ['L', 'R']) for (const [pos, y] of [['up', 2], ['down', -2]]) {
    const x = side === 'L' ? -3 : 3;
    c7.push({ id: `${side}-${pos}`, ...P(proven(`${side}.${pos}.shape`, `${side}.${pos}.transform`), [x, y, 0.75]) });
  }
  add('P7-nested-twins', 'Identical nested structures', d, [...c7, ...c7.slice().reverse().map((c) => ({ ...c, id: `${c.id}-again` }))]);

  // P8 -- DEF without USE.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('thing.transform', 'DEF Thing Transform {\n  children [ ');
  d.leaf('thing.shape', 'Shape { geometry Box { size 3 3 3 } }');
  d.close(' ]\n}');
  d.put('\n');
  add('P8-def-no-use', 'DEF without USE', d, [
    { id: 'thing', ...P(proven('thing.shape', 'thing.transform'), [0, 0, 1.5]) },
  ]);

  // P9 -- one DEF + one USE (USE carried by a translating Transform).
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('def.transform', 'DEF Thing Transform {\n  translation -3 0 0\n  children [ ');
  d.leaf('def.shape', 'Shape { geometry Box { size 2 2 2 } }');
  d.close(' ]\n}');
  d.put('\n');
  d.open('holder.transform', 'Transform {\n  translation 6 0 0\n  children [ ');
  d.leaf('use1', 'USE Thing');
  d.close(' ]\n}');
  d.put('\n');
  add('P9-def-one-use', 'One DEF + one USE', d, [
    { id: 'def-instance', ...P(refused('REFUSED_AMBIGUOUS', 'def.transform'), [-3, 0, 1]) },
    { id: 'use-instance', ...P(refused('REFUSED_AMBIGUOUS', 'use1'), [3, 0, 1]) },
  ]);

  // P10 -- one DEF + three USEs, clicked repeatedly.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('def.transform', 'DEF Thing Transform {\n  translation -4.5 0 0\n  children [ ');
  d.leaf('def.shape', 'Shape { geometry Sphere { radius 1 } }');
  d.close(' ]\n}');
  d.put('\n');
  const useXs = [3, 6, 9];
  useXs.forEach((dx, i) => {
    d.put(`Transform { translation ${dx} 0 0 children [ `);
    d.leaf(`use${i + 1}`, 'USE Thing');
    d.put(' ] }\n');
  });
  const c10 = [];
  for (let r = 0; r < 3; r++) {
    c10.push({ id: `r${r}-def`, ...P(refused('REFUSED_AMBIGUOUS', 'def.transform'), [-4.5, 0, 0.9]) });
    useXs.forEach((dx, i) => c10.push({ id: `r${r}-use${i + 1}`, ...P(refused('REFUSED_AMBIGUOUS', `use${i + 1}`), [-4.5 + dx, 0, 0.9]) }));
  }
  add('P10-def-multi-use', 'One DEF + three USEs', d, c10);

  // P11 -- shared geometry: two distinct Shapes, one DEF Box geometry.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('a.transform', 'Transform {\n  translation -2.5 0 0\n  children [\n    ');
  d.leaf('a.shape', 'Shape { geometry DEF SharedBox Box { size 2 2 2 } }');
  d.close('\n  ]\n}');
  d.put('\n');
  d.open('b.transform', 'Transform {\n  translation 2.5 0 0\n  children [\n    ');
  d.leaf('b.shape', 'Shape { geometry USE SharedBox }');
  d.close('\n  ]\n}');
  d.put('\n');
  add('P11-shared-geometry', 'Shared geometry (DEF/USE Box)', d, [
    // a: authored Box geometry -> WD2-C recognizes the simple object.
    { id: 'a', ...P(proven('a.shape', 'a.transform'), [-2.5, 0, 1]) },
    // b: geometry is a USE -> not a WD2-C simple object -> the Shape itself.
    { id: 'b', ...P(proven('b.shape', 'b.shape'), [2.5, 0, 1]) },
  ]);

  // P12 -- shared Shape.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('a.transform', 'Transform {\n  translation -2.5 0 0\n  children [ ');
  d.leaf('a.shape', 'DEF S Shape { geometry Box { size 2 2 2 } }');
  d.close(' ]\n}');
  d.put('\n');
  d.open('b.transform', 'Transform {\n  translation 2.5 0 0\n  children [ ');
  d.leaf('b.use', 'USE S');
  d.close(' ]\n}');
  d.put('\n');
  add('P12-shared-shape', 'Shared Shape (DEF/USE Shape)', d, [
    { id: 'def-shape', ...P(refused('REFUSED_AMBIGUOUS', 'a.shape'), [-2.5, 0, 1]) },
    { id: 'use-shape', ...P(refused('REFUSED_AMBIGUOUS', 'b.use'), [2.5, 0, 1]) },
  ]);

  // P13 -- one Transform, two independently pickable Shapes (crossing bars).
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('t.transform', 'Transform {\n  children [\n    ');
  d.leaf('bar.shape', 'Shape { geometry Box { size 8 1 1 } }');
  d.put('\n    ');
  d.leaf('post.shape', 'Shape { geometry Cylinder { height 8 radius 0.5 } }');
  d.close('\n  ]\n}');
  d.put('\n');
  add('P13-multi-shape-transform', 'Transform with two Shapes', d, [
    { id: 'bar', ...P(proven('bar.shape'), [3, 0, 0.5]) },
    { id: 'post', ...P(proven('post.shape'), [0, 3, 0.5]) },
  ]);

  // P14 -- Group with two Shapes.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('g.group', 'Group {\n  children [\n    ');
  d.leaf('bar.shape', 'Shape { geometry Box { size 8 1 1 } }');
  d.put('\n    ');
  d.leaf('post.shape', 'Shape { geometry Cylinder { height 8 radius 0.5 } }');
  d.close('\n  ]\n}');
  d.put('\n');
  add('P14-group', 'Group with two Shapes', d, [
    { id: 'bar', ...P(proven('bar.shape'), [-3, 0, 0.5]) },
    { id: 'post', ...P(proven('post.shape'), [0, -3, 0.5]) },
  ]);

  // P15 -- Switch (whichChoice 1) inside a Transform.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('sw.switch', 'Switch {\n  whichChoice 1\n  choice [\n    ');
  d.leaf('hidden.shape', 'Shape { geometry Box { size 6 6 6 } }');
  d.put('\n    ');
  d.leaf('shown.shape', 'Shape { geometry Sphere { radius 2 } }');
  d.close('\n  ]\n}');
  d.put('\n');
  add('P15-switch', 'Switch whichChoice 1', d, [
    { id: 'shown', ...P(proven('shown.shape'), [0, 0, 2]) },
    // Inside the hidden Box's silhouette but outside the Sphere: must be no hit.
    { id: 'hidden-area', ...P({ status: 'NO_HIT' }, [2.7, 2.7, 3]) },
  ]);

  // P16 -- Inline of a tiny spike-owned external world.
  d = new Doc().put(FRONT_VIEWPOINT);
  simpleTransform(d, 'local', '-3 0 0', 'Box { size 2 2 2 }');
  d.leaf('inline', 'Inline { url "P16-child.wrl" }');
  d.put('\n');
  const child = new Doc();
  simpleTransform(child, 'ext', '3 0 0', 'Sphere { radius 1.2 }');
  add('P16-inline', 'Inline external world', d, [
    { id: 'local', ...P(proven('local.shape', 'local.transform'), [-3, 0, 1]) },
    { id: 'inline', ...P(refused('REFUSED_EXTERNAL', 'inline'), [3, 0, 1.2]) },
  ], { children: Object.freeze({ 'P16-child.wrl': child.done().text }) });

  // P17 -- TouchSensor sibling of a Shape.
  d = new Doc().put(FRONT_VIEWPOINT);
  d.open('sensed.transform', 'Transform {\n  translation -3 0 0\n  children [\n    DEF TS TouchSensor { }\n    ');
  d.leaf('sensed.shape', 'Shape { geometry Box { size 2 2 2 } }');
  d.close('\n  ]\n}');
  d.put('\n');
  simpleTransform(d, 'plain', '3 0 0', 'Box { size 2 2 2 }');
  add('P17-touchsensor', 'TouchSensor', d, [
    { id: 'sensed', ...P(refused('REFUSED_SENSOR_CONFLICT', 'sensed.shape'), [-3, 0, 1]) },
    { id: 'plain', ...P(proven('plain.shape', 'plain.transform'), [3, 0, 1]) },
  ], { sensorProbe: 'TS' });

  // P18 -- Anchor around geometry (url targets an in-file Viewpoint).
  d = new Doc().put(FRONT_VIEWPOINT);
  d.put('DEF Far Viewpoint { position 0 0 60 description "far" }\n');
  d.open('anchor', 'Anchor {\n  url "#Far"\n  children [ ');
  d.leaf('anchored.shape', 'Shape { geometry Box { size 3 3 3 } }');
  d.close(' ]\n}');
  d.put('\n');
  add('P18-anchor', 'Anchor', d, [
    { id: 'anchored', ...P(refused('REFUSED_SENSOR_CONFLICT', 'anchored.shape'), [0, 0, 1.5]) },
  ]);

  // P19 -- PROTO instance (runtime body is a copy; no source occurrence).
  d = new Doc().put(FRONT_VIEWPOINT);
  d.put('PROTO MyBox [ ] { Shape { geometry Box { size 3 3 3 } } }\n');
  d.leaf('inst', 'MyBox { }');
  d.put('\n');
  add('P19-proto-instance', 'PROTO instance', d, [
    { id: 'inst', ...P(refused('UNSUPPORTED', 'inst'), [0, 0, 1.5]) },
  ]);

  // P20 -- occlusion: a small front box over a big back box.
  d = new Doc().put(FRONT_VIEWPOINT);
  simpleTransform(d, 'back', '0 0 -3', 'Box { size 6 6 1 }');
  simpleTransform(d, 'front', '0 0 2', 'Box { size 2 2 1 }');
  add('P20-occlusion', 'Occluded objects', d, [
    { id: 'front', ...P(proven('front.shape', 'front.transform'), [0, 0, 2.5]) },
    { id: 'back-edge', ...P(proven('back.shape', 'back.transform'), [2.5, 2.5, -2.5]) },
  ]);

  // P21 -- transparent front object over an opaque back object.
  d = new Doc().put(FRONT_VIEWPOINT);
  simpleTransform(d, 'back', '0 0 -3', 'Box { size 6 6 1 }');
  d.open('glass.transform', 'Transform { translation 0 0 2 children [ ');
  d.leaf('glass.shape', 'Shape { appearance Appearance { material Material { transparency 0.7 } } geometry Box { size 2 2 1 } }');
  d.close(' ] }');
  d.put('\n');
  add('P21-transparent', 'Transparent front object', d, [
    // Truth is the front surface; the measured X_ITE behaviour is recorded.
    { id: 'glass', ...P(proven('glass.shape', 'glass.transform'), [0, 0, 2.5]) },
  ]);

  // P22 -- moved/rotated camera (named Viewpoint bound by the driver).
  d = new Doc().put(FRONT_VIEWPOINT);
  d.put('DEF Side Viewpoint { position 14 3 14 orientation 0 1 0 0.785398 description "side" }\n');
  simpleTransform(d, 'left', '-2 0 0', 'Box { }');
  simpleTransform(d, 'right', '2 0 0', 'Box { }');
  add('P22-camera', 'Rotated camera on anonymous twins', d, [
    { id: 'front-left', ...P(proven('left.shape', 'left.transform'), [-2, 0, 1]) },
    { id: 'side-left', ...P(proven('left.shape', 'left.transform'), [-2, 0, 1]), camera: 'Side' },
    { id: 'side-right', ...P(proven('right.shape', 'right.transform'), [2, 0, 1]), camera: 'Side' },
  ], { cameras: Object.freeze({ Side: { position: [14, 3, 14], axis: [0, 1, 0], angle: 0.785398 } }) });

  return F;
}

// Scale fixtures for the performance sanity pass: n anonymous WD2-C-shaped
// objects on a grid; clicks on every `stride`-th object.
function gridFixture(n, stride) {
  const cols = Math.ceil(Math.sqrt(n * 1.6));
  const rows = Math.ceil(n / cols);
  const span = 14;
  const step = span / cols;
  const size = (step * 0.7).toFixed(3);
  const d = new Doc().put(FRONT_VIEWPOINT);
  const pts = [];
  for (let i = 0; i < n; i++) {
    const c = i % cols, r = Math.floor(i / cols);
    const x = -span / 2 + step * (c + 0.5);
    const y = (rows / 2 - r - 0.5) * step;
    pts.push([x, y]);
    simpleTransform(d, `g${i}`, `${x.toFixed(3)} ${y.toFixed(3)} 0`, `Box { size ${size} ${size} ${size} }`);
  }
  const clicks = [];
  for (let i = 0; i < n; i += stride) {
    clicks.push({ id: `g${i}`, ...P(proven(`g${i}.shape`, `g${i}.transform`), [pts[i][0], pts[i][1], step * 0.35]) });
  }
  const { text, spans } = d.done();
  return Object.freeze({ id: `PERF-${n}`, title: `${n} anonymous objects`, text, spans, clicks: Object.freeze(clicks) });
}

// ---- independent projection (screen truth) -------------------------------
// VRML97 Viewpoint: perspective, fieldOfView (default pi/4) spans the SMALLER
// viewport dimension (ISO 14772-1 6.53). Returns CSS pixels relative to the
// canvas' top-left corner.
function rotate(v, axis, angle) {
  const [x, y, z] = axis;
  const len = Math.hypot(x, y, z) || 1;
  const [ux, uy, uz] = [x / len, y / len, z / len];
  const c = Math.cos(angle), s = Math.sin(angle), t = 1 - c;
  const [vx, vy, vz] = v;
  return [
    (t * ux * ux + c) * vx + (t * ux * uy - s * uz) * vy + (t * ux * uz + s * uy) * vz,
    (t * ux * uy + s * uz) * vx + (t * uy * uy + c) * vy + (t * uy * uz - s * ux) * vz,
    (t * ux * uz - s * uy) * vx + (t * uy * uz + s * ux) * vy + (t * uz * uz + c) * vz,
  ];
}
const DEFAULT_CAMERA = Object.freeze({ position: [0, 0, 20], axis: [0, 0, 1], angle: 0 });
function project(world, camera, cssWidth, cssHeight, fov = 0.785398) {
  const cam = camera || DEFAULT_CAMERA;
  const right = rotate([1, 0, 0], cam.axis, cam.angle);
  const up = rotate([0, 1, 0], cam.axis, cam.angle);
  const fwd = rotate([0, 0, -1], cam.axis, cam.angle);
  const rel = [0, 1, 2].map((i) => world[i] - cam.position[i]);
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const depth = dot(rel, fwd);
  const half = Math.min(cssWidth, cssHeight) / 2;
  const k = half / (depth * Math.tan(fov / 2));
  return { x: cssWidth / 2 + dot(rel, right) * k, y: cssHeight / 2 - dot(rel, up) * k };
}

module.exports = { build, gridFixture, project, DEFAULT_CAMERA };
