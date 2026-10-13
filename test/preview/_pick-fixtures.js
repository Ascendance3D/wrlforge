'use strict';
// WD2-D production pick fixtures + ORACLE (ported from the accepted WD2-C0
// fixture matrix, spikes/wd2-c0-xite-picking/fixtures.js, which stays frozen).
//
// Each fixture is composed by concatenation and records every occurrence's
// exact [start,end) AS IT WRITES IT. Truth never comes from X_ITE, the fake
// X_ITE, or src/vrml: this file requires none of them.
//
// Options: `eol` ('\n' | '\r\n'), `prefix` (e.g. '\uFEFF' for a BOM) and
// `comment` (an extra comment line, e.g. non-ASCII text) let one fixture run
// in every mandatory source form with spans that stay exact by construction.

function composer({ eol = '\n', prefix = '', comment = null } = {}) {
  let text = `${prefix}#VRML V2.0 utf8${eol}`;
  if (comment) text += `# ${comment}${eol}`;
  const spans = {};
  const api = {
    put(s) { text += s.split('\n').join(eol); return api; },
    mark(name, fn) { const start = text.length; fn(); spans[name] = { start, end: text.length }; return api; },
    done() { return { text, spans }; },
  };
  return api;
}

const MAT = 'appearance Appearance { material Material { diffuseColor 0.8 0.2 0.2 } }';

// A WD2-C First Object, byte for byte what src/vrml/node-templates.js
// simpleObjectTemplate() inserts (plus an optional translation line, as a
// later property edit writes it). Checked by xite-pick-guards.test.js.
function simpleObject(c, name, { translation = null, primitive = 'Box' } = {}) {
  c.mark(`${name}.transform`, () => {
    c.put('Transform {\n');
    if (translation) c.put(`  translation ${translation}\n`);
    c.put('  children [\n    ');
    c.mark(`${name}.shape`, () => c.put(`Shape {\n      appearance Appearance {\n        material Material {\n        }\n      }\n      geometry ${primitive} {\n      }\n    }`));
    c.put('\n  ]\n}');
  });
  c.put('\n');
}

// Fixtures. `clicks`: { id, aim: { type, nth } | { inlineOf: nth } | null,
// expect: { status, clicked?, logical? } }
const BUILDERS = {
  'P3-wd2c-box': (o) => {
    const c = composer(o);
    simpleObject(c, 'box');
    return { ...c.done(), clicks: [
      { id: 'box', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 'box.shape', logical: 'box.transform' } },
      { id: 'background', aim: null, expect: { status: 'NO_HIT' } },
    ] };
  },
  'P4-wd2c-box-sphere': (o) => {
    const c = composer(o);
    simpleObject(c, 'box', { translation: '-2 0 0' });
    simpleObject(c, 'sphere', { translation: '2 0 0', primitive: 'Sphere' });
    return { ...c.done(), clicks: [
      { id: 'box', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 'box.shape', logical: 'box.transform' } },
      { id: 'sphere', aim: { type: 'Shape', nth: 1 }, expect: { status: 'PROVEN', clicked: 'sphere.shape', logical: 'sphere.transform' } },
    ] };
  },
  'P5-anonymous-twins': (o) => {
    const c = composer(o);
    simpleObject(c, 'left', { translation: '-2 0 0' });
    simpleObject(c, 'right', { translation: '2 0 0' });
    return { ...c.done(), clicks: [
      { id: 'left', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 'left.shape', logical: 'left.transform' } },
      { id: 'right', aim: { type: 'Shape', nth: 1 }, expect: { status: 'PROVEN', clicked: 'right.shape', logical: 'right.transform' } },
    ] };
  },
  'P6-many-siblings': (o) => {
    const c = composer(o);
    const clicks = [];
    c.put('Group {\n  children [\n');
    for (let i = 0; i < 12; i++) {
      c.put('    ');
      c.mark(`s${i}`, () => c.put(`Shape { geometry Box { size 0.5 0.5 0.5 } }`));
      c.put('\n');
      clicks.push({ id: `s${i}`, aim: { type: 'Shape', nth: i }, expect: { status: 'PROVEN', clicked: `s${i}`, logical: `s${i}` } });
    }
    c.put('  ]\n}\n');
    return { ...c.done(), clicks };
  },
  'P8-def-without-use': (o) => {
    const c = composer(o);
    c.mark('t', () => {
      c.put('DEF Lonely Transform {\n  children [\n    ');
      c.mark('s', () => c.put('Shape { geometry Box { } }'));
      c.put('\n  ]\n}');
    });
    c.put('\n');
    return { ...c.done(), clicks: [
      { id: 'def', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 's', logical: 't' } },
    ] };
  },
  'P9-def-use': (o) => {
    const c = composer(o);
    c.put('Transform {\n  translation -2 0 0\n  children [\n    ');
    c.mark('def', () => c.put('DEF Shared Shape { geometry Box { } }'));
    c.put('\n  ]\n}\nTransform {\n  translation 2 0 0\n  children [\n    ');
    c.mark('use', () => c.put('USE Shared'));
    c.put('\n  ]\n}\n');
    return { ...c.done(), clicks: [
      // Same runtime Shape for both: never PROVEN, including the DEF original.
      { id: 'def-original', aim: { type: 'Shape', nth: 0 }, expect: { status: 'REFUSED_AMBIGUOUS' } },
    ] };
  },
  'P11-shared-geometry': (o) => {
    const c = composer(o);
    c.mark('a.transform', () => {
      c.put('Transform {\n  translation -2 0 0\n  children [\n    ');
      c.mark('a.shape', () => c.put(`Shape { ${MAT} geometry DEF G Box { size 1 1 1 } }`));
      c.put('\n  ]\n}');
    });
    c.put('\n');
    c.mark('b.transform', () => {
      c.put('Transform {\n  translation 2 0 0\n  children [\n    ');
      c.mark('b.shape', () => c.put(`Shape { ${MAT} geometry USE G }`));
      c.put('\n  ]\n}');
    });
    c.put('\n');
    return { ...c.done(), clicks: [
      { id: 'a', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 'a.shape', logical: 'a.transform' } },
      { id: 'b', aim: { type: 'Shape', nth: 1 }, expect: { status: 'PROVEN', clicked: 'b.shape', logical: 'b.shape' } },
    ] };
  },
  'P12-shared-shape-in-two-transforms': (o) => {
    const c = composer(o);
    c.put('Transform { children [ DEF SS Shape { geometry Sphere { } } ] }\nTransform { translation 3 0 0 children [ USE SS ] }\n');
    return { ...c.done(), clicks: [
      { id: 'shared', aim: { type: 'Shape', nth: 0 }, expect: { status: 'REFUSED_AMBIGUOUS' } },
    ] };
  },
  'P13-transform-two-shapes': (o) => {
    const c = composer(o);
    c.put('Transform {\n  children [\n    ');
    c.mark('first', () => c.put('Shape { geometry Box { } }'));
    c.put('\n    ');
    c.mark('second', () => c.put('Shape { geometry Sphere { } }'));
    c.put('\n  ]\n}\n');
    return { ...c.done(), clicks: [
      { id: 'first', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 'first', logical: 'first' } },
      { id: 'second', aim: { type: 'Shape', nth: 1 }, expect: { status: 'PROVEN', clicked: 'second', logical: 'second' } },
    ] };
  },
  'P16-inline': (o) => {
    const c = composer(o);
    c.mark('local', () => c.put('Shape { geometry Box { } }'));
    c.put('\nInline { url "child.wrl" }\n');
    return { ...c.done(), clicks: [
      { id: 'local', aim: { type: 'Shape', nth: 0 }, expect: { status: 'PROVEN', clicked: 'local', logical: 'local' } },
      { id: 'inline-child', aim: { inlineOf: 0 }, expect: { status: 'REFUSED_EXTERNAL' } },
    ] };
  },
  'P17-touch-sensor': (o) => {
    const c = composer(o);
    c.put('Group {\n  children [\n    TouchSensor { }\n    Shape { geometry Box { } }\n  ]\n}\n');
    c.mark('plain', () => c.put('Shape { geometry Sphere { } }'));
    c.put('\n');
    return { ...c.done(), clicks: [
      { id: 'sensed', aim: { type: 'Shape', nth: 0 }, expect: { status: 'REFUSED_SENSOR_CONFLICT' } },
      { id: 'plain', aim: { type: 'Shape', nth: 1 }, expect: { status: 'PROVEN', clicked: 'plain', logical: 'plain' } },
    ] };
  },
  'P17b-other-sensors': (o) => {
    const c = composer(o);
    c.put('Group { children [ PlaneSensor { } Shape { geometry Box { } } ] }\n');
    c.put('Group { children [ CylinderSensor { } Shape { geometry Box { } } ] }\n');
    c.put('Group { children [ SphereSensor { } Shape { geometry Box { } } ] }\n');
    return { ...c.done(), clicks: [0, 1, 2].map((nth) => (
      { id: `sensor-${nth}`, aim: { type: 'Shape', nth }, expect: { status: 'REFUSED_SENSOR_CONFLICT' } })) };
  },
  'P18-anchor': (o) => {
    const c = composer(o);
    c.put('Anchor {\n  url "elsewhere.wrl"\n  children [\n    Shape { geometry Box { } }\n  ]\n}\n');
    c.mark('plain', () => c.put('Shape { geometry Sphere { } }'));
    c.put('\n');
    return { ...c.done(), clicks: [
      { id: 'anchored', aim: { type: 'Shape', nth: 0 }, expect: { status: 'REFUSED_SENSOR_CONFLICT' } },
      { id: 'plain', aim: { type: 'Shape', nth: 1 }, expect: { status: 'PROVEN', clicked: 'plain', logical: 'plain' } },
    ] };
  },
};

function build(opts) {
  return Object.entries(BUILDERS).map(([id, b]) => ({ id, ...b(opts || {}) }));
}

// Grade a resolver result against the oracle. WRONG iff a PROVEN result names
// a source occurrence other than the oracle's. A refusal is never WRONG.
function grade(expect, spans, res) {
  const same = (a, b) => !!a && !!b && a.start === b.start && a.end === b.end;
  if (res.status !== 'PROVEN') return { wrong: false, matches: res.status === expect.status };
  if (expect.status !== 'PROVEN') return { wrong: true, matches: false };
  const wrong = !same(res.source.logical, spans[expect.logical]) || !same(res.source.shape, spans[expect.clicked]);
  return { wrong, matches: !wrong };
}

module.exports = { build, grade, BUILDERS };
