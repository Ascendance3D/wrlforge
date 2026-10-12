#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
'use strict';
// RUST-0 differential harness: identical inputs -> src/vrml/edit.js (baseline)
// and the Rust spike binary -> compare -> exact match / approved difference /
// regression. Exit code 1 on any regression.
//
// Deterministic: exhaustive enumeration over fixed small texts plus every
// committed fixture under test/fixtures; no PRNG, no clock. Read-only except
// for out/differential-summary.json (gitignored).
//
// Approved differences are decided by checks INDEPENDENT of both
// implementations (String.prototype.isWellFormed and a direct surrogate test
// on the original text), never by trusting the Rust answer:
//   D1 EEDITBOUNDARY  JS applied the set, Rust refused it, AND some edit
//                     endpoint lies strictly inside a surrogate pair.
//   D2 EENCODING      Rust refused, AND the text or an insert is ill-formed
//                     UTF-16 (a lone surrogate) that UTF-8 cannot represent.
//
// Usage: cargo build --release && node differential.js

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');
const { spawnSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..', '..');
const edit = require(path.join(ROOT, 'src/vrml/edit.js'));
const { tokenize } = require(path.join(ROOT, 'src/vrml/tokenizer.js'));
const BIN = path.join(__dirname, 'target/release/wrlforge-text-spike');

// ---- wire encoding (UTF-16 code units, 4 hex digits each) ------------------
const enc = (s) => (s.length === 0 ? '-'
  : Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16).padStart(4, '0')).join(''));
const encEdits = (edits) => [edits.length, ...edits.flatMap((e) => [e.from, e.to, enc(e.insert)])].join(' ');

const isHigh = (c) => c >= 0xd800 && c <= 0xdbff;
const isLow = (c) => c >= 0xdc00 && c <= 0xdfff;
const insidePair = (text, off) => off > 0 && off < text.length
  && isHigh(text.charCodeAt(off - 1)) && isLow(text.charCodeAt(off));

// ---- baseline (JavaScript) answers in the same response format -------------
function jsErr(id, e) {
  const f = (v) => (v === undefined || v === null ? '-' : String(v));
  return `${id} ERR ${e.code} ${f(e.index)} ${f(e.otherIndex)}`;
}
function jsApply(id, text, edits) {
  try { return `${id} OK ${enc(edit.applyEdits(text, edits))}`; } catch (e) { return jsErr(id, e); }
}
function jsMap(id, offset, aff, edits) {
  try { return `${id} OKN ${edit.mapOffset(offset, edits, aff === 'a' ? 'after' : 'before')}`; } catch (e) { return jsErr(id, e); }
}
// Expected offset conversion, derived from Buffer.byteLength over a prefix
// that is proven not to split a pair -- independent of the Rust code.
function jsConvert(id, text, off) {
  if (off > text.length) return `${id} ERR EOFFSETBOUNDS - -`;
  if (insidePair(text, off)) return `${id} ERR EOFFSETSURROGATE - -`;
  return `${id} OKC ${Buffer.byteLength(text.slice(0, off), 'utf8')} ${off}`;
}

// ---- case generation -------------------------------------------------------
const cases = []; // { line, js, kind, text, edits, group }
const push = (c) => { c.id = String(cases.length); cases.push(c); };

function addApply(group, text, edits) {
  push({ kind: 'A', group, text, edits });
}

const SMALL = [
  ['empty', ''],
  ['crlf', 'ab\r\ncd'],
  ['bmp', 'é€x'],
  ['astral', 'a\u{1F600}b'],
  ['astral2', '\u{1F600}\u{1F600}'],
  ['bom', '﻿#V'],
  ['ffxx', 'a～b'],
  ['lone', 'a\uD800b'], // ill-formed: what a JS string can hold, UTF-8 cannot
];
const LONG = ['long', '#VRML V2.0 utf8\r\nDEF café Shape { } # \u{1F600}\n'];
const INSERTS = ['', 'Z', '\r\n', '\u{1F600}', 'é', '～'];
const BAD_INSERT = '\uDC00';

function spans(len) {
  const out = [];
  for (let from = 0; from <= len + 1; from += 1) {
    for (let to = from; to <= len + 1; to += 1) out.push([from, to]);
  }
  out.push([2, 1]); // one shape violation (from > to)
  return out;
}

for (const [name, text] of [...SMALL, LONG]) {
  const singles = [];
  for (const [from, to] of spans(text.length)) {
    for (const insert of INSERTS) singles.push({ from, to, insert });
  }
  for (const e of singles) addApply(`enum:${name}:1`, text, [e]);
  addApply(`enum:${name}:bad-insert`, text, [{ from: 0, to: 0, insert: BAD_INSERT }]);
  for (let off = 0; off <= text.length + 1; off += 1) push({ kind: 'C', group: `conv:${name}`, text, off });
  if (text.length <= 6) {
    const narrow = singles.filter((e) => ['', 'Z', '\u{1F600}', '～'].includes(e.insert));
    let k = 0;
    for (const a of narrow) {
      for (const b of narrow) {
        addApply(`enum:${name}:2`, text, [a, b]);
        if (k++ % 7 === 0) {
          for (let off = 0; off <= text.length + 1; off += 1) {
            for (const aff of ['b', 'a']) push({ kind: 'M', group: `map:${name}`, off, aff, edits: [a, b] });
          }
        }
      }
    }
  }
}

// Real span sources: every token span the production tokenizer reports over
// every committed fixture (gzip decoded the way file-io.js decodes it).
function* walk(dir) {
  for (const ent of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const p = path.join(dir, ent.name);
    if (ent.isDirectory()) yield* walk(p);
    else yield p;
  }
}
let fixtureFiles = 0;
for (const file of walk(path.join(ROOT, 'test/fixtures'))) {
  const raw = fs.readFileSync(file);
  let text;
  if (raw[0] === 0x1f && raw[1] === 0x8b) {
    try { text = zlib.gunzipSync(raw).toString('utf8'); } catch { continue; }
  } else if (/\.(wrl|wrz|x3dv)$/i.test(file)) {
    text = raw.toString('utf8');
  } else continue;
  fixtureFiles += 1;
  const rel = path.relative(ROOT, file).split(path.sep).join('/');
  const { tokens } = tokenize(text);
  // Every case carries the whole text, so sample tokens at an even stride,
  // fewer for large files. Deterministic: same file set, same sample.
  const want = Math.max(8, Math.min(64, Math.floor(400000 / Math.max(1, text.length))));
  const stride = Math.max(1, Math.ceil(tokens.length / want));
  for (const t of tokens.filter((_, i) => i % stride === 0 || i === tokens.length - 1)) {
    const from = t.range.start.offset;
    const to = t.range.end.offset;
    push({ kind: 'C', group: `fixture:${rel}`, text, off: from });
    addApply(`fixture:${rel}`, text, [{ from, to, insert: 'X' }]);
    addApply(`fixture:${rel}`, text, [{ from: to, to, insert: '\u{1F600}' }, { from, to: from, insert: 'é' }]);
  }
}

// ---- run -------------------------------------------------------------------
for (const c of cases) {
  if (c.kind === 'A') {
    c.line = `A ${c.id} ${enc(c.text)} ${encEdits(c.edits)}`;
    c.js = jsApply(c.id, c.text, c.edits);
  } else if (c.kind === 'M') {
    c.line = `M ${c.id} ${c.off} ${c.aff} ${encEdits(c.edits)}`;
    c.js = jsMap(c.id, c.off, c.aff, c.edits);
  } else {
    c.line = `C ${c.id} ${enc(c.text)} ${c.off}`;
    c.js = jsConvert(c.id, c.text, c.off);
  }
}

if (!fs.existsSync(BIN)) {
  console.error(`missing ${BIN}; run: cargo build --release`);
  process.exit(2);
}
const input = cases.map((c) => c.line).join('\n') + '\n';
const run = spawnSync(BIN, [], { input, maxBuffer: 1 << 30, encoding: 'utf8' });
if (run.status !== 0) {
  console.error(run.stderr);
  process.exit(2);
}
const rust = run.stdout.trimEnd().split('\n');
if (rust.length !== cases.length) {
  console.error(`response count ${rust.length} != case count ${cases.length}`);
  process.exit(2);
}

const tally = { exact: 0, d1_boundary: 0, d1_js_output_ill_formed: 0, d2_encoding: 0, regression: 0 };
const byKind = { A: 0, M: 0, C: 0 };
const regressions = [];
cases.forEach((c, i) => {
  byKind[c.kind] += 1;
  const r = rust[i];
  if (r === c.js) { tally.exact += 1; return; }
  const code = r.split(' ')[2];
  if (c.kind === 'A' && code === 'EEDITBOUNDARY' && c.js.split(' ')[1] === 'OK'
    && c.edits.some((e) => insidePair(c.text, e.from) || insidePair(c.text, e.to))) {
    tally.d1_boundary += 1;
    const out = edit.applyEdits(c.text, c.edits);
    if (!out.isWellFormed()) tally.d1_js_output_ill_formed += 1;
    return;
  }
  if (code === 'EENCODING'
    && (!(c.text ?? '').isWellFormed() || c.edits.some((e) => !e.insert.isWellFormed()))) {
    tally.d2_encoding += 1;
    return;
  }
  tally.regression += 1;
  if (regressions.length < 20) regressions.push({ group: c.group, request: c.line.slice(0, 200), js: c.js, rust: r });
});

const sha = (s) => crypto.createHash('sha256').update(s).digest('hex');
const summary = {
  cases: cases.length,
  byKind,
  fixtureFiles,
  tally,
  inputSha256: sha(input),
  baselineSha256: sha(cases.map((c) => c.js).join('\n')),
  rustSha256: sha(run.stdout),
  node: process.version,
  regressions,
};
fs.mkdirSync(path.join(__dirname, 'out'), { recursive: true });
fs.writeFileSync(path.join(__dirname, 'out/differential-summary.json'), JSON.stringify(summary, null, 2) + '\n');
console.log(JSON.stringify({ ...summary, regressions: regressions.length }, null, 2));
process.exit(tally.regression === 0 ? 0 : 1);
