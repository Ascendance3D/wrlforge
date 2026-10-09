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
};

// Clicks the Tauri harness cannot reproduce faithfully are listed with the
// reason; they still run, and must still never select a wrong node.
const GUI_NOTES = {
  // The Tauri preview serves no relative files yet, so the Inline child is
  // never loaded: the area is empty (NO_HIT). REFUSED_EXTERNAL is covered by
  // the Rust resolver tests.
  'P16-inline/inline': { status: 'NO_HIT', note: 'inline-child-not-served-in-tauri-preview' },
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
      logical: e.status === 'PROVEN' ? span(e.logical) : null,
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
  for (const variant of ['crlf', 'cr', 'bom', 'unicode']) add(fx, variant);
}

fs.writeFileSync(path.join(real, 'plan.json'), JSON.stringify({ fixtures }, null, 1));
console.log(`${fixtures.length} fixtures, ${fixtures.reduce((n, f) => n + f.clicks.length, 0)} clicks -> ${real}`);
