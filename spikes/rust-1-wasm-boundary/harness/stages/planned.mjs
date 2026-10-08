// SPDX-License-Identifier: GPL-3.0-or-later
// Interfaces for future migration stages. NONE is implemented in RUST-1.
// Each entry fixes what a later lane must compare and which independent
// oracle it must bring, so the framework contract is settled before the code.
// runStage() refuses a planned stage with ESTAGEPLANNED.
const planned = (id, lane, compares, oracle) => Object.freeze({
  id,
  status: 'planned',
  lane,
  description: `${id} differential (planned for ${lane})`,
  compares,
  requiredOracle: oracle,
  cases() { const e = new Error(`${id} is planned for ${lane}`); e.code = 'ESTAGEPLANNED'; throw e; },
});

export const PLANNED_STAGES = [
  planned('tokenize', 'RUST-3',
    ['token kind', 'lexeme (UTF-16 units)', 'start/end offset', 'line', 'UTF-16 column', 'comments/trivia', 'diagnostic code + span'],
    'src/vrml/tokenizer.js on identical decoded text; corpus fingerprint; decoded-text dedup'),
  planned('parse', 'RUST-3',
    ['AST JSON shape', 'node/field spans', 'recovery tree shape', 'diagnostic code, severity, span (message separately)', 'limits behaviour'],
    'src/vrml/parser.js; 65 committed fixtures + corpus sample; Blaxxun leniency classified as recovery'),
  planned('source-map', 'RUST-3',
    ['offset -> token/node lookup', 'rangeOf', 'half-open UTF-16 ranges'],
    'src/vrml/source-map.js on the same parse'),
  planned('semantics', 'RUST-4',
    ['DEF/USE bindings', 'PROTO type resolution', 'IS bindings', 'ROUTE six-question verdicts', 'status enums (resolved/ambiguous/unresolved/unsupported/recovered)'],
    'spikes/wd1-* independently authored oracles, structurally unable to import either implementation; 0 wrong bindings gate'),
  planned('node-identity', 'RUST-5',
    ['Tier-1 re-anchoring result', 'Tier-2 DEF identity verdict', 'ambiguous/lost/unprovable outcomes'],
    'WD1.4 hard gate: never a confident different node; object-identity sessions stay in JS facade'),
  planned('transaction', 'RUST-5',
    ['verifyTransaction verdict', 'firstDivergence', 'receipt fields'],
    'src/vrml/document-transaction.js; full-text comparison only, never a hash'),
];
