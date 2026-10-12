// SPDX-License-Identifier: GPL-3.0-or-later
// Dev-time ORACLE only (not shipped): compares the Rust highlight dump
// (argv[2]) with src/editor/language.js for the same stdin text.
//
// Every span the JS editor colored must have the SAME span and the mapped
// class in Rust. Rust may add a role only on a token JS left as a plain
// identifier, and only one of the documented UI-SYNTAX-1 additions. Two
// documented refinements: `TO` is a ROUTE word, and an unterminated string
// is `invalid`.
'use strict';
const fs = require('fs');
const { analyze } = require('../../src/editor/language');
const text = fs.readFileSync(0, 'utf8');
const rust = new Map();
for (const line of fs.readFileSync(process.argv[2], 'utf8').split('\n')) {
  if (!line) continue;
  const [f, t, c] = line.split(' ');
  rust.set(`${f}:${t}`, c);
}
const MAP = {
  header: 'header', keyword: 'keyword', def: 'keyword', use: 'keyword', proto: 'keyword',
  is: 'keyword', route: 'route', null: 'literal', bool: 'literal', string: 'string',
  number: 'number', invalid: 'invalid', bracket: 'punctuation', punct: 'punctuation',
  comment: 'comment', nodeType: 'node-type', fieldName: 'field', defName: 'def-name',
  useName: 'def-ref', identifier: null,
};
const ADDED = new Set(['field-type', 'def-ref', 'field', 'node-type']);
const problems = [];
const jsKeys = new Set();
for (const h of analyze(text).highlights) {
  const key = `${h.from}:${h.to}`;
  const lexeme = text.slice(h.from, h.to);
  jsKeys.add(key);
  let want = MAP[h.cls];
  if (want === undefined) { problems.push(`unknown JS class ${h.cls}`); continue; }
  if (h.cls === 'keyword' && lexeme === 'TO') want = 'route';
  if (h.cls === 'string' && (lexeme.length < 2 || !lexeme.endsWith('"'))) want = 'invalid';
  const got = rust.get(key);
  if (want === null) {
    if (got !== undefined && !ADDED.has(got)) problems.push(`${key} ${JSON.stringify(lexeme)} JS identifier, Rust ${got}`);
    continue;
  }
  if (got !== want) problems.push(`${key} ${JSON.stringify(lexeme)} JS ${h.cls}->${want}, Rust ${got}`);
}
for (const [key, c] of rust) {
  if (!jsKeys.has(key)) problems.push(`${key} Rust-only span ${c}`);
}
process.stdout.write(problems.slice(0, 10).join('\n') + (problems.length ? '\n' : ''));
process.exit(problems.length ? 1 : 0);
