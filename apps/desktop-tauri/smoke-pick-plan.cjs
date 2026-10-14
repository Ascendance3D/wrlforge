// SPDX-License-Identifier: GPL-3.0-or-later
// VISUAL-2 test input: write the viewport-picking fixture matrix for
// `smoke.sh --pick` into a disposable directory. Node is used ONLY here, at
// test time (the application never runs Node).
//
// ORACLE. The fixtures and their expected answers are the accepted WD2-C0
// oracle (spikes/wd2-c0-xite-picking/fixtures.js, read-only): text composed by
// concatenation, every span known by AUTHORSHIP, never by asking a parser.
// This script requires nothing else from the repository, so the truth cannot
// come from the implementation it grades.
//
// Source-form variants (CRLF, lone CR, BOM, Unicode) transform the text and
// map every oracle span by the same deterministic rule; nothing is searched.
//
// Usage: node smoke-pick-plan.cjs <empty /tmp directory>
'use strict';
const fs = require('fs');
const path = require('path');
const os = require('os');
const C0 = require('../../spikes/wd2-c0-xite-picking/fixtures');

const out = process.argv[2];
if (!out) { console.error('usage: smoke-pick-plan.cjs <dir>'); process.exit(2); }
const real = fs.realpathSync(out);
if (!real.startsWith(fs.realpathSync(os.tmpdir()) + path.sep)) {
  console.error(`refusing a non-temporary directory: ${real}`);
  process.exit(2);
}

const HEADER = '#VRML V2.0 utf8\n';
// Map an offset of `text` through one transform.
const VARIANTS = {
  lf: { text: (t) => t, map: (t, o) => o },
  crlf: {
    text: (t) => t.replace(/\n/g, '\r\n'),
    map: (t, o) => o + (t.slice(0, o).match(/\n/g) || []).length,
  },
  cr: { text: (t) => t.replace(/\n/g, '\r'), map: (t, o) => o },
  bom: { text: (t) => `﻿${t}`, map: (t, o) => o + 1 },
  unicode: (() => {
    const note = '# Grüße é ✓ 😀 世界 — Unicode before every node\n';
    return {
      text: (t) => HEADER + note + t.slice(HEADER.length),
      map: (t, o) => (o >= HEADER.length ? o + note.length : o),
    };
  })(),
  // XITE-16-UPGRADE-2: an INVALID ROUTE (neither node exists) before every
  // node. X_ITE 16.5.0 drops it and renders the rest (15.1.10 rejected the
  // scene); every pick on that partial scene must stay exact or refuse.
  badroute: (() => {
    const route = 'ROUTE XiteGhostA.fraction_changed TO XiteGhostB.set_fraction\n';
    return {
      text: (t) => HEADER + route + t.slice(HEADER.length),
      map: (t, o) => (o >= HEADER.length ? o + route.length : o),
    };
  })(),
};

// Clicks the Tauri harness cannot reproduce faithfully are listed with the
// reason; they still run, and must still never select a wrong node.
const GUI_NOTES = {
  // The Tauri preview serves no relative files yet, so the Inline child is
  // never loaded: the area is empty (NO_HIT). REFUSED_EXTERNAL is covered by
  // the Rust resolver tests.
  'P16-inline/inline': { status: 'NO_HIT', note: 'inline-child-not-served-in-tauri-preview' },
  // B1: the Tauri resolver proves a PROTO instance body node to its exact
  // DECLARATION statement (the shared definition). The WD2-C0 answer
  // (UNSUPPORTED) is kept as oracleStatus; the truth is the body Shape.
  'P19-proto-instance/inst': { status: 'PROVEN', logical: 'body.shape', note: 'b1-proto-body-proven-to-declaration' },
};

const fixtures = [];
const write = (name, text) => fs.writeFileSync(path.join(real, name), Buffer.from(text, 'utf8'));

function add(fx, variant) {
  const v = VARIANTS[variant];
  const id = variant === 'lf' ? fx.id : `${fx.id}~${variant}`;
  const file = `${id.replace(/[^A-Za-z0-9_.-]/g, '_')}.wrl`;
  write(file, v.text(fx.text));
  const span = (label) => {
    const s = fx.spans[label];
    return [v.map(fx.text, s.start), v.map(fx.text, s.end)];
  };
  const clicks = fx.clicks.map((c) => {
    const note = GUI_NOTES[`${fx.id}/${c.id}`] || null;
    const e = c.expect;
    return {
      id: c.id,
      world: c.world,
      // A named camera needs a viewpoint bind the harness does not perform.
      skip: c.camera ? `camera-${c.camera}-not-bound-by-harness` : null,
      status: note ? note.status : e.status,
      note: note ? note.note : null,
      logical: note && note.logical ? span(note.logical) : e.status === 'PROVEN' ? span(e.logical) : null,
      // The oracle's own answer, kept for the report.
      oracleStatus: e.status,
    };
  });
  fixtures.push({ id, file, camera: [0, 0, 20], clicks });
}

for (const fx of C0.build()) {
  add(fx, 'lf');
  if (fx.children) for (const [name, text] of Object.entries(fx.children)) write(name, text);
}
// Every source form over fixtures with multi-line and repeated structure.
for (const fx of C0.build().filter((f) => ['P3-wd2c-first-object', 'P4-box-and-sphere', 'P7-nested-twins'].includes(f.id))) {
  for (const variant of ['crlf', 'cr', 'bom', 'unicode', 'badroute']) add(fx, variant);
}

// ---- P0-A hit layers: a clear coat over red paint ---------------------------
// Composed here by concatenation, spans by authorship (same rule as the
// WD2-C0 oracle). The first click selects the coat (the nearest Shape); a
// click AGAIN at the same point selects the paint behind it; a third returns
// to the coat. Top level, and inside a PROTO body (each layer must prove to
// its own DECLARATION statement).
const COAT = 'Shape { appearance Appearance { material Material { diffuseColor 0.94 1 0.96 transparency 0.85 } } geometry Box { size 4 4 0.2 } }';
const PAINT = 'Shape { appearance Appearance { material Material { diffuseColor 0.78 0 0.11 } } geometry Box { size 2 2 2 } }';
function layerFixture(id, proto) {
  let text = `${HEADER}Viewpoint { position 0 0 20 description "front" }\n`;
  const spans = {};
  const at = (label, t) => { const start = text.length; text += t; spans[label] = { start, end: text.length }; };
  if (proto) text += 'PROTO Car [ ] {\n  Group { children [\n    ';
  at('coat', `Transform { translation 0 0 2 children [ ${COAT} ] }`);
  text += proto ? '\n    ' : '\n';
  at('paint', `Transform { children [ ${PAINT} ] }`);
  if (proto) { text += '\n  ] }\n}\n'; at('inst', 'Car { }'); }
  text += '\n';
  const file = `${id}.wrl`;
  write(file, text);
  const sp = (l) => [spans[l].start, spans[l].end];
  fixtures.push({
    id, file, camera: [0, 0, 20],
    clicks: [{
      id: 'coat-then-paint', world: [0, 0, 2.1], skip: null, status: 'PROVEN', note: 'p0a-hit-layers',
      logical: sp('coat'), oracleStatus: 'PROVEN',
      then: [{ status: 'PROVEN', logical: sp('paint') }, { status: 'PROVEN', logical: sp('coat') }],
    }],
  });
}
layerFixture('L1-coat-over-paint', false);
layerFixture('L2-proto-coat-over-paint', true);

// ---- LINUX-EDIT-INPUT-SAFETY: authored sensors while editing ---------------
// One world with every interactive kind the editor must keep quiet: a
// TouchSensor door (ROUTE -> TimeSensor -> interpolator -> rotation), an
// Anchor to an in-file Viewpoint, a PlaneSensor slider, a TouchSensor inside
// a PROTO body nested in two Transforms (internal ROUTEs -> the instance's
// `color`), next to a looping TimeSensor animation, a static Shape and a
// transparent Shape. Sensor objects must refuse (and stay quiet); the others
// prove to their own top-level Transform. The smoke reads the runtime fields
// by DEF name (public SAI) to see whether any sensor fired.
function sensorFixture(id) {
  let text = `${HEADER}Viewpoint { position 0 0 20 description "front" }\nDEF Far Viewpoint { position 0 40 60 description "far" }\nNavigationInfo { type [ "EXAMINE" "ANY" ] }\n`;
  text += 'PROTO Lamp [ exposedField SFColor color 0.2 0.2 0.2 ] {\n  Group { children [\n    DEF LT TouchSensor { }\n'
    + '    Shape { appearance Appearance { material DEF LM Material { diffuseColor IS color } } geometry Box { size 1.6 1.6 1.6 } }\n'
    + '    DEF LC TimeSensor { cycleInterval 0.2 }\n    DEF LI ColorInterpolator { key [ 0 1 ] keyValue [ 0.2 0.2 0.2, 1 1 0 ] }\n  ] }\n'
    + '  ROUTE LT.touchTime TO LC.set_startTime\n  ROUTE LC.fraction_changed TO LI.set_fraction\n  ROUTE LI.value_changed TO LM.set_diffuseColor\n}\n';
  const spans = {};
  const at = (label, t) => { const start = text.length; text += t; spans[label] = { start, end: text.length }; text += '\n'; };
  const box = (c) => `Shape { appearance Appearance { material Material { diffuseColor ${c} } } geometry Box { size 1.6 1.6 1.6 } }`;
  at('static', `Transform { translation -6 3 0 children [ ${box('0.6 0.6 0.6')} ] }`);
  at('door', `DEF Door Transform { translation -2 3 0 children [ DEF DoorTouch TouchSensor { } ${box('0 0.6 0')} ] }`);
  text += 'DEF Clock TimeSensor { cycleInterval 0.3 }\nDEF Swing OrientationInterpolator { key [ 0 1 ] keyValue [ 0 1 0 0, 0 1 0 1.2 ] }\n'
    + 'ROUTE DoorTouch.touchTime TO Clock.set_startTime\nROUTE Clock.fraction_changed TO Swing.set_fraction\nROUTE Swing.value_changed TO Door.set_rotation\n';
  at('anchor', `Anchor { url "#Far" description "far" children [ Transform { translation 2 3 0 children [ ${box('0 0 0.8')} ] } ] }`);
  at('plane', `Transform { translation 6 3 0 children [ DEF Drag PlaneSensor { } DEF Slider Transform { children [ ${box('0.8 0.4 0')} ] } ] }`);
  text += 'ROUTE Drag.translation_changed TO Slider.set_translation\n';
  at('lamp', 'Transform { translation -6 -3 0 children [ Transform { children [ DEF L1 Lamp { } ] } ] }');
  at('spin', `DEF Spin Transform { translation -2 -3 0 children [ ${box('0.8 0 0.8')} ] }`);
  text += 'DEF SpinClock TimeSensor { cycleInterval 2 loop TRUE }\nDEF SpinPath OrientationInterpolator { key [ 0 0.5 1 ] keyValue [ 0 0 1 0, 0 0 1 3.14, 0 0 1 6.28 ] }\n'
    + 'ROUTE SpinClock.fraction_changed TO SpinPath.set_fraction\nROUTE SpinPath.value_changed TO Spin.set_rotation\n';
  at('glass', 'Transform { translation 2 -3 0 children [ Shape { appearance Appearance { material Material { diffuseColor 0.7 0.9 1 transparency 0.6 } } geometry Box { size 1.6 1.6 1.6 } } ] }');
  const file = `${id}.wrl`;
  write(file, text);
  const sp = (l) => [spans[l].start, spans[l].end];
  const proven = (cid, label, world) => ({ id: cid, world, skip: null, status: 'PROVEN', note: 'edit-input-safety', logical: sp(label), oracleStatus: 'PROVEN' });
  const refused = (cid, world) => ({ id: cid, world, skip: null, status: 'REFUSED_SENSOR_CONFLICT', note: 'edit-input-safety', logical: null, oracleStatus: 'REFUSED_SENSOR_CONFLICT' });
  fixtures.push({
    id, file, camera: [0, 0, 20],
    clicks: [
      proven('static', 'static', [-6, 3, 0.8]),
      refused('door', [-2, 3, 0.8]),
      refused('anchor', [2, 3, 0.8]),
      refused('plane', [6, 3, 0.8]),
      refused('lamp-in-proto', [-6, -3, 0.8]),
      proven('time-driven', 'spin', [-2, -3, 0.8]),
      proven('transparent', 'glass', [2, -3, 0.8]),
    ],
  });
}
sensorFixture('S1-edit-input-safety');

// ---- optional: the real Chevy (WRLFORGE_PICK_CHEVY=<a /tmp copy>) -----------
// Not part of the gate (the model is not in the repository). The oracle is a
// CPU ray cast written HERE: every Shape's IndexedFaceSet (PROTO `point`
// default, identity frames) is intersected with the camera ray, Shapes are
// ordered by their nearest hit (the pointing pass draws both faces), and
// layer k must prove to the k-th Shape's exact span (brace-matched here).
if (process.env.WRLFORGE_PICK_CHEVY) chevyFixture(process.env.WRLFORGE_PICK_CHEVY);

function chevyFixture(src) {
  const real = fs.realpathSync(src);
  if (!real.startsWith(fs.realpathSync(os.tmpdir()) + path.sep)) throw new Error(`chevy copy must be under ${os.tmpdir()}`);
  const body = fs.readFileSync(real, 'utf8');
  let CAM = null;
  let text = body;
  // Brace-matched statement end from `start` (skips strings and comments).
  const blockEnd = (start) => {
    let depth = 0;
    for (let i = start; i < text.length; i++) {
      const c = text[i];
      if (c === '#') { while (i < text.length && text[i] !== '\n') i++; continue; }
      if (c === '"') { i++; while (text[i] !== '"') i += text[i] === '\\' ? 2 : 1; continue; }
      if (c === '{') depth++;
      if (c === '}' && --depth === 0) return i + 1;
    }
    throw new Error('unbalanced');
  };
  const nums = (t) => t.replace(/#[^\n]*/g, ' ').split(/[\s,]+/).filter(Boolean).map(Number);
  const pointStart = text.indexOf('point [') + 'point ['.length;
  const pv = nums(text.slice(pointStart, text.indexOf(']', pointStart)));
  const P = (i) => [pv[3 * i], pv[3 * i + 1], pv[3 * i + 2]];
  const shapes = [];
  const re = /\bShape\s*\{/g;
  let m;
  while ((m = re.exec(text))) {
    const start = m.index;
    const end = blockEnd(start);
    const t = text.slice(start, end);
    const ci = t.indexOf('coordIndex [');
    const idx = nums(t.slice(ci + 12, t.indexOf(']', ci)));
    const faces = [];
    let f = [];
    for (const i of idx) { if (i < 0) { if (f.length >= 3) faces.push(f); f = []; } else f.push(i); }
    if (f.length >= 3) faces.push(f);
    const tr = /transparency\s+([0-9.]+)/.exec(t);
    shapes.push({ start, end, faces, transparency: tr ? Number(tr[1]) : 0, line: text.slice(0, start).split('\n').length });
  }
  const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const hitTri = (o, d, a, b, c) => { // Moller-Trumbore, both faces
    const e1 = sub(b, a), e2 = sub(c, a), pv2 = cross(d, e2), det = dot(e1, pv2);
    if (Math.abs(det) < 1e-12) return null;
    const tv = sub(o, a), u = dot(tv, pv2) / det;
    if (u < 0 || u > 1) return null;
    const q = cross(tv, e1), v = dot(d, q) / det;
    if (v < 0 || u + v > 1) return null;
    const t = dot(e2, q) / det;
    return t > 1e-6 ? t : null;
  };
  const order = (cam, target) => {
    const d = sub(target, cam);
    const n = Math.hypot(...d);
    const dir = d.map((x) => x / n);
    const near = [];
    shapes.forEach((s, k) => {
      let best = Infinity;
      for (const f of s.faces) for (let j = 1; j + 1 < f.length; j++) {
        const t = hitTri(cam, dir, P(f[0]), P(f[j]), P(f[j + 1]));
        if (t !== null && t < best) best = t;
      }
      if (best < Infinity) near.push([best, k]);
    });
    near.sort((a, b) => a[0] - b[0]);
    return near;
  };
  const coat = shapes.findIndex((s) => s.transparency >= 0.8);
  const paint = shapes.findIndex((s) => /diffuseColor\s+0\.7843 0\.0000 0\.1059/.test(text.slice(s.start, s.end)));
  if (coat < 0 || paint < 0) throw new Error('chevy: coat or paint Shape not found');
  // Aim points: a barycentric grid on every coat triangle, seen from a few
  // cameras (VRML97 default orientation, looking -Z). Wanted: the coat
  // FIRST and the paint right BEHIND it, with the same whole order for
  // every ray within an angular margin (aim tolerance), the larger the
  // better. The first camera with a 0.004 rad (~2.5 px) margin wins.
  const CAMS = [[0.9, 0.4, 2.5], [0.6, 0.6, 3.5], [0, 1, 4], [0.8, 1.5, 5], [0, 2.5, 9]];
  const MARGINS = [0.004, 0.003, 0.002];
  let pick = null;
  for (const cam of CAMS) {
    for (const f of shapes[coat].faces) {
      for (let j = 1; j + 1 < f.length; j++) {
        const tri = [P(f[0]), P(f[j]), P(f[j + 1])];
        for (let u = 1; u < 6; u++) for (let v = 1; u + v < 6; v++) {
          const w = [u / 6, v / 6, 1 - (u + v) / 6];
          const c = [0, 1, 2].map((a) => w[0] * tri[0][a] + w[1] * tri[1][a] + w[2] * tri[2][a]);
          const o = order(cam, c);
          if (o.length < 2 || o[0][1] !== coat || o[1][1] !== paint || o[1][0] - o[0][0] < 0.01) continue;
          const dist = Math.hypot(...sub(c, cam));
          const key = o.map((x) => x[1]).join(',');
          const margin = MARGINS.find((m) => [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]].every(([dx, dy]) =>
            order(cam, [c[0] + dx * m * dist, c[1] + dy * m * dist, c[2]]).map((x) => x[1]).join(',') === key));
          if (margin === undefined) continue;
          if (!pick || margin > pick.margin) pick = { cam, c, o, margin };
        }
      }
    }
    if (pick && pick.margin >= MARGINS[0]) break;
  }
  if (!pick) throw new Error('chevy: no stable coat-over-paint aim point');
  CAM = pick.cam;
  // EXAMINE: the VRML97 default WALK viewer applies gravity on the first
  // click and drops a camera that hovers above the car onto it.
  text = `${body}\nViewpoint { position ${CAM.join(' ')} description "coat over paint" }\nNavigationInfo { type [ "EXAMINE" "ANY" ] }\n`;
  const seq = pick.o.slice(0, 8).map(([, k]) => ({ status: 'PROVEN', logical: [shapes[k].start, shapes[k].end], line: shapes[k].line }));
  write('chevy-rear.wrl', text);
  fixtures.push({
    id: 'C1-chevy-coat-over-paint', file: 'chevy-rear.wrl', camera: CAM,
    clicks: [{
      id: `coat-l${shapes[coat].line}-then-paint-l${shapes[paint].line}`, world: pick.c, skip: null,
      status: 'PROVEN', note: 'p0a-chevy', logical: seq[0].logical, oracleStatus: 'PROVEN',
      then: seq.slice(1).concat([seq[0]]).map(({ status, logical }) => ({ status, logical })),
    }],
  });
  console.log(`chevy: camera ${CAM.join(' ')} aim ${pick.c.map((x) => x.toFixed(3)).join(' ')} margin ${pick.margin} rad layers ${seq.map((x) => `l${x.line}`).join(' > ')}`);
}

fs.writeFileSync(path.join(real, 'plan.json'), JSON.stringify({ fixtures }, null, 1));
console.log(`${fixtures.length} fixtures, ${fixtures.reduce((n, f) => n + f.clicks.length, 0)} clicks -> ${real}`);
