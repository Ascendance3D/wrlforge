// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1A: the independent EXACT BigInt mapping oracle.
//
// Written from the WD1.2 mapping contract. It imports nothing: not
// src/vrml/edit.js, not the Rust engine, not the framework, and it never reads
// either answer. Moved here unchanged from precision-review.mjs (PR #130
// review) so the edit-algebra stage's RUST1-PRECISION check can use the same
// independent truth instead of the JavaScript answer it is grading.

export const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);

// Edits are sorted by `from`, a pure insertion before a replacement at the
// same `from`. Returns a BigInt, or the string 'REFUSE' for an input the WD1.2
// contract refuses (overlap / ambiguity). Only edit sets the stage feeds to
// the P/M/R kinds are needed here; anything else throws so it is never graded
// silently.
export function oracleMap(offset, edits, after) {
  const es = edits.map((e) => ({ from: BigInt(e.from), to: BigInt(e.to), n: BigInt(e.insert.length) }))
    .sort((a, b) => (a.from < b.from ? -1 : a.from > b.from ? 1 : (a.to === a.from ? -1 : 1)));
  for (let i = 1; i < es.length; i += 1) {
    const p = es[i - 1];
    const q = es[i];
    if (q.from < p.to) return 'REFUSE';
    if (q.from === p.from && p.from === p.to && q.from === q.to) return 'REFUSE';
  }
  const o = BigInt(offset);
  let shift = 0n;
  for (const e of es) {
    if (e.from === e.to) {
      if (o < e.from) return o + shift;
      if (o === e.from) return o + shift + (after ? e.n : 0n);
      shift += e.n;
      continue;
    }
    if (o < e.from) return o + shift;
    if (o < e.to) return e.from + shift + (after ? e.n : 0n);
    shift += e.n - (e.to - e.from);
  }
  return o + shift;
}

export function oracle(c) {
  switch (c.kind) {
    case 'M': return [oracleMap(c.off, c.edits, c.aff === 'a')];
    case 'SM': case 'PM': return [oracleMap(c.off, c.edits, c.aff === 'after')];
    case 'R': {
      const r = c.range;
      const from = r.start ? r.start.offset : r.from;
      const to = r.start ? r.end.offset : r.to;
      const o = c.opts || {};
      return [oracleMap(from, c.edits, o.startAffinity === 'after'),
        oracleMap(to, c.edits, (o.endAffinity === undefined ? 'after' : o.endAffinity) === 'after')];
    }
    default: throw new Error(`no oracle for kind ${c.kind}`);
  }
}
