// SPDX-License-Identifier: GPL-3.0-or-later
// Dev-time ORACLE only (not shipped, not a runtime service): dumps the JS
// parser's diagnostics and node walk for stdin text, in the exact line format
// of crates/wrlforge-vrml/examples/parity_dump.rs.
'use strict';
const { parse } = require('../../src/vrml/parser');
const text = require('fs').readFileSync(0, 'utf8');
const r = parse(text);
const out = [];
for (const d of r.diagnostics) out.push(`D ${d.code} ${d.range.start.offset} ${d.range.end.offset}`);
function walk(a) {
  if (!a) return;
  const s = a.range ? a.range.start.offset : 0, e = a.range ? a.range.end.offset : 0;
  switch (a.type) {
    case 'Node': out.push(`N node ${a.nodeType}:${a.def || '-'} ${s} ${e}`); a.fields.forEach(walk); break;
    case 'Field': walk(a.value); break;
    case 'Array': a.items.forEach(walk); break;
    case 'Use': out.push(`N use ${a.name || '-'} ${s} ${e}`); break;
    case 'Route': out.push(`N route - ${s} ${e}`); break;
    case 'Proto': out.push(`N proto ${a.name || '-'} ${s} ${e}`); a.body.forEach(walk); break;
    case 'ExternProto': out.push(`N externproto ${a.name || '-'} ${s} ${e}`); break;
    default: break;
  }
}
r.tree.statements.forEach(walk);
process.stdout.write(out.join('\n') + (out.length ? '\n' : ''));
