// SPDX-License-Identifier: GPL-3.0-or-later
// Stage `edit-algebra`: src/vrml/edit.js + tokenizer positions (JavaScript
// baseline) vs wrlforge-wasm through the facade (Rust candidate).
//
// Part 1 regenerates the RUST-0 case set EXACTLY (same texts, same order, same
// ids, same canonical answers), so its input and baseline digests must equal
// RUST-0's: that proves this harness covers the original 167,316 cases.
// Part 2 adds RUST-1 coverage: argument shapes, mapRange, large/overflow
// offsets, UTF-16 echo through a session, and the line index.
import { createRequire } from 'node:module';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { sha256 } from '../framework.mjs';

const require = createRequire(import.meta.url);

// RUST-0 published digests (spikes/rust-core-feasibility/README.md).
const RUST0 = {
  cases: 167316,
  inputSha256: '358ea72370567a1005a14c81851d3515165d96d27f1c1449feb94357130066dd',
  baselineSha256: 'c8d70e04278f752800fe4a778e0ade16937eb3cc12f5ea1f7502f73448c85694',
};

// ---- canonical encodings (RUST-0 wire format) ------------------------------
const enc = (s) => (s.length === 0 ? '-'
  : Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16).padStart(4, '0')).join(''));
const encEdits = (edits) => [edits.length, ...edits.flatMap((e) => [e.from, e.to, enc(e.insert)])].join(' ');
const f = (v) => (v === undefined || v === null ? '-' : String(v));
const errAnswer = (e) => {
  if (!e || typeof e.code !== 'string') throw e; // an uncoded failure is a harness bug, never a result
  return `ERR ${e.code} ${f(e.index)} ${f(e.otherIndex)}`;
};
const attempt = (fn) => { try { return fn(); } catch (e) { return errAnswer(e); } };

// Deterministic serializer for arbitrary (possibly malformed) arguments.
function ser(v) {
  if (v === undefined) return 'U';
  if (v === null) return 'N';
  if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v);
  if (typeof v === 'string') return `s${enc(v)}`;
  if (typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`;
  return `{${Object.keys(v).map((k) => `${k}:${ser(v[k])}`).join(',')}}`;
}

const isHigh = (c) => c >= 0xd800 && c <= 0xdbff;
const isLow = (c) => c >= 0xdc00 && c <= 0xdfff;
const insidePair = (text, off) => Number.isInteger(off) && off > 0 && off < text.length
  && isHigh(text.charCodeAt(off - 1)) && isLow(text.charCodeAt(off));

// ---- independent oracles ---------------------------------------------------
// Offset conversion from Buffer.byteLength over a prefix proven not to split a
// pair (RUST-0's oracle, unchanged).
function oracleConvert(text, off) {
  if (off > text.length) return 'ERR EOFFSETBOUNDS - -';
  if (insidePair(text, off)) return 'ERR EOFFSETSURROGATE - -';
  return `OKC ${Buffer.byteLength(text.slice(0, off), 'utf8')} ${off}`;
}

// Line/column by direct unit walk: LF, CRLF and lone CR end one line each;
// columns count UTF-16 units; an offset between CR and LF stays on the CR's line.
function oracleLineCol(text, off) {
  if (off > text.length) return 'ERR EOFFSETBOUNDS - -';
  if (insidePair(text, off)) return 'ERR EOFFSETSURROGATE - -';
  let line = 1;
  let col = 1;
  for (let i = 0; i < off; i += 1) {
    const c = text[i];
    if (c === '\n' || (c === '\r' && text[i + 1] !== '\n')) { line += 1; col = 1; } else if (c === '\r') {
      // CR of a CRLF: the line ends after the LF.
      if (i + 1 < off) { line += 1; col = 1; i += 1; } else col += 1;
    } else col += 1;
  }
  return `OKL ${line} ${col}`;
}

// Inverse: what offsetAt(line, col) must answer for the position of `off`.
function oracleOffsetAt(text, off) {
  if (text[off - 1] === '\r' && text[off] === '\n') return 'ERR ECOLUMN - -';
  return `OKO ${off}`;
}

// ---- texts -----------------------------------------------------------------
const SMALL = [
  ['empty', ''],
  ['crlf', 'ab\r\ncd'],
  ['bmp', 'é€x'],
  ['astral', 'a\u{1F600}b'],
  ['astral2', '\u{1F600}\u{1F600}'],
  ['bom', '﻿#V'],
  ['ffxx', 'a～b'],
  ['lone', 'a\uD800b'],
];
const LONG = ['long', '#VRML V2.0 utf8\r\nDEF café Shape { } # \u{1F600}\n'];
const INSERTS = ['', 'Z', '\r\n', '\u{1F600}', 'é', '～'];
const BAD_INSERT = '\uDC00';
const EOL = [
  ['lf', 'a\nb'], ['crlf2', 'a\r\nb'], ['cr', 'a\rb'], ['cr-crlf', 'a\r\r\nb'], ['crlfx2', '\r\n\r\n'],
  ['lfcr', '\n\r'], ['tail-cr', 'x\r'], ['only-cr', '\r'], ['only-lf', '\n'], ['nul', 'a\0\r\n\0'],
  ['mixed-astral', 'a\n\u{1F600}\r\nb\r\u{1F600}'], ['bom-crlf', '﻿#VRML V2.0 utf8\r\nShape {}\r\n'],
];

function spans(len) {
  const out = [];
  for (let from = 0; from <= len + 1; from += 1) {
    for (let to = from; to <= len + 1; to += 1) out.push([from, to]);
  }
  out.push([2, 1]);
  return out;
}

function* walk(dir) {
  for (const ent of readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const p = join(dir, ent.name);
    if (ent.isDirectory()) yield* walk(p);
    else yield p;
  }
}

export function makeStage(ROOT) {
  const edit = require(join(ROOT, 'src/vrml/edit.js'));
  const { tokenize } = require(join(ROOT, 'src/vrml/tokenizer.js'));

  function cases() {
    const out = [];
    const push = (c) => { out.push(c); };

    // ===== Part 1: RUST-0 case set, verbatim order ===========================
    const addApply = (group, text, edits) => push({ kind: 'A', rust0: true, group, text, edits });
    for (const [name, text] of [...SMALL, LONG]) {
      const singles = [];
      for (const [from, to] of spans(text.length)) for (const insert of INSERTS) singles.push({ from, to, insert });
      for (const e of singles) addApply(`enum:${name}:1`, text, [e]);
      addApply(`enum:${name}:bad-insert`, text, [{ from: 0, to: 0, insert: BAD_INSERT }]);
      for (let off = 0; off <= text.length + 1; off += 1) push({ kind: 'C', rust0: true, group: `conv:${name}`, text, off });
      if (text.length <= 6) {
        const narrow = singles.filter((e) => ['', 'Z', '\u{1F600}', '～'].includes(e.insert));
        let k = 0;
        for (const a of narrow) {
          for (const b of narrow) {
            addApply(`enum:${name}:2`, text, [a, b]);
            if (k++ % 7 === 0) {
              for (let off = 0; off <= text.length + 1; off += 1) {
                for (const aff of ['b', 'a']) push({ kind: 'M', rust0: true, group: `map:${name}`, off, aff, edits: [a, b] });
              }
            }
          }
        }
      }
    }
    const fixtures = [];
    for (const file of walk(join(ROOT, 'test/fixtures'))) {
      const raw = readFileSync(file);
      let text;
      if (raw[0] === 0x1f && raw[1] === 0x8b) {
        try { text = gunzipSync(raw).toString('utf8'); } catch { continue; }
      } else if (/\.(wrl|wrz|x3dv)$/i.test(file)) {
        text = raw.toString('utf8');
      } else continue;
      const rel = relative(ROOT, file).split(sep).join('/');
      const { tokens } = tokenize(text);
      fixtures.push({ rel, text, tokens });
      const want = Math.max(8, Math.min(64, Math.floor(400000 / Math.max(1, text.length))));
      const stride = Math.max(1, Math.ceil(tokens.length / want));
      for (const t of tokens.filter((_, i) => i % stride === 0 || i === tokens.length - 1)) {
        const from = t.range.start.offset;
        const to = t.range.end.offset;
        push({ kind: 'C', rust0: true, group: `fixture:${rel}`, text, off: from });
        addApply(`fixture:${rel}`, text, [{ from, to, insert: 'X' }]);
        addApply(`fixture:${rel}`, text, [{ from: to, to, insert: '\u{1F600}' }, { from, to: from, insert: 'é' }]);
      }
    }

    // ===== Part 2: RUST-1 additions ==========================================
    // S: argument shapes -- code and caller index must match edit.js exactly.
    const OK1 = { from: 0, to: 0, insert: 'a' };
    const OK2 = { from: 1, to: 2, insert: 'b' };
    const BAD = [null, 1, 'x', [], {}, { from: 0, to: 0 }, { from: 0, to: 0, insert: '', x: 1 },
      { from: 1.5, to: 2, insert: '' }, { from: -1, to: 0, insert: '' }, { from: NaN, to: 0, insert: '' },
      { from: 0, to: Infinity, insert: '' }, { from: 2, to: 1, insert: '' }, { from: 0, to: 0, insert: 5 },
      { from: 0, to: 0, insert: null }, { from: '0', to: 0, insert: '' }, { from: 0, to: -0, insert: '' }];
    for (const bad of BAD) {
      for (const at of [0, 1, 2]) {
        const list = [OK1, OK2];
        list.splice(at, 0, bad);
        push({ kind: 'SA', group: 'shape:apply', text: 'abcdef', edits: list });
        push({ kind: 'SM', group: 'shape:map', off: 1, aff: 'before', edits: list });
      }
    }
    for (const edits of [null, undefined, {}, 'x', 5]) {
      push({ kind: 'SA', group: 'shape:edits', text: 'abc', edits });
      push({ kind: 'SM', group: 'shape:edits', off: 0, aff: 'before', edits });
    }
    for (const text of [null, undefined, 5, {}]) push({ kind: 'SA', group: 'shape:text', text, edits: [] });
    for (const off of [-1, 1.5, NaN, Infinity, '1', null, -0]) push({ kind: 'SM', group: 'shape:offset', off, aff: 'before', edits: [] });
    for (const aff of ['x', null, '', 'BEFORE', undefined]) push({ kind: 'SM', group: 'shape:affinity', off: 0, aff, edits: [OK1] });
    const BAD_RANGES = [null, [], {}, { from: 1 }, { to: 1 }, { from: 2, to: 1 }, { from: -1, to: 1 }, { from: 0.5, to: 1 },
      { start: { offset: 1 }, end: { offset: -1 } }, { start: { offset: 1.5 }, end: { offset: 2 } }, { start: {}, end: {} },
      { start: { offset: 3 }, end: { offset: 2 } }, { start: 1, end: 2 }];
    for (const range of BAD_RANGES) push({ kind: 'R', group: 'shape:range', range, edits: [], opts: {} });
    for (const opts of [{ startAffinity: 'x' }, { endAffinity: null }, { startAffinity: 'after', endAffinity: 'before' }]) {
      push({ kind: 'R', group: 'shape:range-opts', range: { from: 1, to: 2 }, edits: [{ from: 1, to: 3, insert: 'Q' }], opts });
    }

    // R: mapRange enumeration over the short texts, both range shapes, all affinities.
    const AFFS = [['before', 'before'], ['before', 'after'], ['after', 'before'], ['after', 'after']];
    let rshape = 0;
    for (const [name, text] of SMALL) {
      if (text.length > 6) continue;
      const singles = [];
      for (const [from, to] of spans(text.length)) {
        for (const insert of ['', 'Z', '\u{1F600}']) singles.push({ from, to, insert });
      }
      const sets = [...singles.map((e) => [e]), [singles[3], singles[singles.length - 4]], [singles[0], singles[0]]];
      for (let s = 0; s < sets.length; s += 1) {
        for (let from = 0; from <= text.length + 1; from += 1) {
          for (let to = from; to <= text.length + 1; to += 2) {
            const [sa, ea] = AFFS[(s + from + to) % 4];
            const range = (rshape++ % 2) ? { start: { offset: from }, end: { offset: to } } : { from, to };
            push({ kind: 'R', group: `range:${name}`, range, edits: sets[s], opts: { startAffinity: sa, endAffinity: ea } });
          }
        }
      }
    }

    // P: very large valid offsets and overflow.
    const M = Number.MAX_SAFE_INTEGER;
    const BIG = [0, 1, 2 ** 31 - 1, 2 ** 31, 2 ** 32 - 1, 2 ** 32, 2 ** 32 + 1, 2 ** 52, M - 2, M - 1, M, 2 ** 53, 2 ** 53 + 2,
      2 ** 60, 2 ** 64, 2 ** 70, Number.MAX_VALUE];
    const BIG_EDITS = [[], [{ from: 0, to: 0, insert: 'ab' }], [{ from: 0, to: 0, insert: '\u{1F600}' }], [{ from: 1, to: 3, insert: '' }],
      [{ from: M - 1, to: M, insert: 'xyz' }], [{ from: 2 ** 32, to: 2 ** 32 + 7, insert: 'q' }], [{ from: 2 ** 60, to: 2 ** 60, insert: '' }]];
    for (const off of BIG) {
      for (const edits of BIG_EDITS) {
        for (const aff of ['before', 'after']) push({ kind: 'PM', group: 'precision:map', off, aff, edits });
      }
      for (const off2 of BIG) {
        if (off2 < off) continue;
        push({ kind: 'PA', group: 'precision:apply', text: 'abc', edits: [{ from: off, to: off2, insert: 'z' }] });
        push({ kind: 'R', group: 'precision:range', range: { from: off, to: off2 }, edits: BIG_EDITS[1], opts: {} });
      }
    }

    // U: UTF-16 echo through a session -- zero silent substitution.
    const POOL = ['', 'a', '�', '\u{1F600}', '\uD83D', '\uDE00', '\r\n', '\r', '﻿', '\0', 'é'];
    for (const a of POOL) for (const b of POOL) for (const c of POOL) push({ kind: 'U', group: 'utf16:echo', text: a + b + c });

    // L / LO / LT: line index.
    for (const [name, text] of [...SMALL, LONG, ...EOL]) {
      for (let off = 0; off <= text.length + 1; off += 1) {
        push({ kind: 'L', group: `line:${name}`, text, off });
        push({ kind: 'LO', group: `line-inverse:${name}`, text, off });
      }
    }
    for (const [name, text] of [...SMALL, LONG, ...EOL]) {
      for (const t of tokenize(text).tokens) push({ kind: 'LT', group: `line-token:${name}`, text, token: t });
    }
    for (const fx of fixtures) {
      const want = Math.max(8, Math.min(64, Math.floor(400000 / Math.max(1, fx.text.length))));
      const stride = Math.max(1, Math.ceil(fx.tokens.length / want));
      for (const t of fx.tokens.filter((_, i) => i % stride === 0 || i === fx.tokens.length - 1)) {
        push({ kind: 'LT', group: `line-token:${fx.rel}`, text: fx.text, token: t });
      }
    }
    return out;
  }

  function serialize(c) {
    switch (c.kind) {
      case 'A': return `A ${c.id} ${enc(c.text)} ${encEdits(c.edits)}`;
      case 'M': return `M ${c.id} ${c.off} ${c.aff} ${encEdits(c.edits)}`;
      case 'C': return `C ${c.id} ${enc(c.text)} ${c.off}`;
      case 'LT': return `LT ${c.id} ${sha256(c.text)} ${c.token.range.start.offset} ${c.token.range.end.offset}`;
      default: return `${c.kind} ${c.id} ${ser({ ...c, id: undefined, kind: undefined, group: undefined, rust0: undefined, token: undefined })}`;
    }
  }

  // ---- JavaScript baseline ---------------------------------------------------
  function baseline(c) {
    switch (c.kind) {
      case 'A': case 'SA': case 'PA':
        return attempt(() => `OK ${enc(edit.applyEdits(c.text, c.edits))}`);
      case 'M': return attempt(() => `OKN ${edit.mapOffset(c.off, c.edits, c.aff === 'a' ? 'after' : 'before')}`);
      case 'SM': case 'PM': return attempt(() => `OKN ${edit.mapOffset(c.off, c.edits, c.aff)}`);
      case 'R': return attempt(() => { const r = edit.mapRange(c.range, c.edits, c.opts); return `OKR ${r.from} ${r.to}`; });
      case 'C': return oracleConvert(c.text, c.off);
      case 'U': return `OK ${enc(c.text)}`; // JavaScript holds the exact units
      case 'L': return oracleLineCol(c.text, c.off);
      case 'LO': {
        const lc = oracleLineCol(c.text, c.off);
        return lc.startsWith('OKL') ? oracleOffsetAt(c.text, c.off) : 'SKIP';
      }
      case 'LT': {
        // The production tokenizer's own positions are the baseline.
        const { start, end } = c.token.range;
        return `OKL ${start.line} ${start.column} ${end.line} ${end.column}`;
      }
      default: throw new Error(`unknown kind ${c.kind}`);
    }
  }

  // ---- Rust candidate --------------------------------------------------------
  function candidate(c, ctx) {
    const { engine } = ctx;
    const snap = (text) => {
      if (!ctx.snapshots.has(text)) {
        try { ctx.snapshots.set(text, engine.openSession(text).current()); } catch (e) { ctx.snapshots.set(text, e); }
      }
      const s = ctx.snapshots.get(text);
      if (s instanceof Error) throw s;
      return s;
    };
    switch (c.kind) {
      case 'A': case 'SA': case 'PA':
        return attempt(() => `OK ${enc(engine.applyEdits(c.text, c.edits))}`);
      case 'M': return attempt(() => `OKN ${engine.mapOffset(c.off, c.edits, c.aff === 'a' ? 'after' : 'before')}`);
      case 'SM': case 'PM': return attempt(() => `OKN ${engine.mapOffset(c.off, c.edits, c.aff)}`);
      case 'R': return attempt(() => { const r = engine.mapRange(c.range, c.edits, c.opts); return `OKR ${r.from} ${r.to}`; });
      case 'C': return attempt(() => {
        const s = snap(c.text);
        const b = s.toUtf8(c.off);
        return `OKC ${b} ${s.fromUtf8(b)}`;
      });
      case 'U': return attempt(() => `OK ${enc(snap(c.text).text())}`);
      case 'L': return attempt(() => { const r = snap(c.text).lineCol(c.off); return `OKL ${r.line} ${r.column}`; });
      case 'LO': return attempt(() => {
        const s = snap(c.text);
        let lc;
        try { lc = s.lineCol(c.off); } catch { return 'SKIP'; }
        return `OKO ${s.offsetAt(lc.line, lc.column)}`;
      });
      case 'LT': return attempt(() => {
        const s = snap(c.text);
        const a = s.lineCol(c.token.range.start.offset);
        const b = s.lineCol(c.token.range.end.offset);
        return `OKL ${a.line} ${a.column} ${b.line} ${b.column}`;
      });
      default: throw new Error(`unknown kind ${c.kind}`);
    }
  }

  // ---- independent checks for registry entries -------------------------------
  const offsetsOf = (c) => {
    const vals = [];
    if (typeof c.off === 'number') vals.push(c.off);
    if (c.range && typeof c.range === 'object') {
      vals.push(c.range.from, c.range.to, c.range.start?.offset, c.range.end?.offset);
    }
    if (Array.isArray(c.edits)) for (const e of c.edits) if (e && typeof e === 'object') vals.push(e.from, e.to);
    return vals.filter((v) => typeof v === 'number');
  };
  const checks = {
    endpointInsideSurrogatePair: (c, js) => (c.kind === 'A' || c.kind === 'SA' || c.kind === 'PA')
      && js.startsWith('OK ') && c.edits.some((e) => insidePair(c.text, e.from) || insidePair(c.text, e.to)),
    someInputIllFormed: (c) => (typeof c.text === 'string' && !c.text.isWellFormed())
      || (Array.isArray(c.edits) && c.edits.some((e) => e && typeof e.insert === 'string' && !e.insert.isWellFormed())),
    unsafeIntegerInvolved: (c, js) => ['M', 'SM', 'PM', 'R'].includes(c.kind)
      && (offsetsOf(c).some((v) => Number.isInteger(v) && !Number.isSafeInteger(v))
        || (/^OK[NR] /.test(js) && js.split(' ').slice(1).map(Number).some((v) => !Number.isSafeInteger(v)))),
  };

  function extra(all, ctx, { inputs, baselines, classes }) {
    const r0 = all.filter((c) => c.rust0);
    const r0Input = r0.map((c) => inputs[c.id]).join('\n') + '\n';
    const r0Baseline = r0.map((c) => `${c.id} ${baselines[c.id]}`).join('\n');
    const tallyFor = (pred) => {
      const t = {};
      all.forEach((c, i) => { if (pred(c)) t[classes[i]] = (t[classes[i]] || 0) + 1; });
      return t;
    };
    let illFormed = 0;
    all.forEach((c, i) => {
      if (classes[i] === 'RUST0-BOUNDARY' && !edit.applyEdits(c.text, c.edits).isWellFormed()) illFormed += 1;
    });
    return {
      rust0Equivalence: {
        cases: r0.length,
        expectedCases: RUST0.cases,
        inputSha256: sha256(r0Input),
        baselineSha256: sha256(r0Baseline),
        inputMatchesRust0: sha256(r0Input) === RUST0.inputSha256,
        baselineMatchesRust0: sha256(r0Baseline) === RUST0.baselineSha256,
        tally: tallyFor((c) => c.rust0),
      },
      rust1Additions: { cases: all.length - r0.length, tally: tallyFor((c) => !c.rust0) },
      boundaryRefusalsWhereJsOutputWasIllFormed: illFormed,
      fixtureFiles: new Set(all.filter((c) => c.group.startsWith('fixture:')).map((c) => c.group)).size,
      skippedLineInverse: all.filter((c, i) => c.kind === 'LO' && baselines[i] === 'SKIP').length,
    };
  }

  return {
    id: 'edit-algebra',
    status: 'implemented',
    description: 'WD1.2 span-patch algebra, UTF-16 offset conversion, UTF-16 echo, line index',
    compares: ['output text as UTF-16 code units', 'error code', 'error caller index', 'error other index',
      'mapped offsets', 'UTF-8 byte offset + round trip', 'line and UTF-16 column'],
    cases,
    serialize,
    baseline,
    candidate,
    checks,
    extra,
  };
}
