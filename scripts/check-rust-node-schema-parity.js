'use strict';
// Parity: the Rust node schema (crates/wrlforge-vrml, examples/schema_dump.rs)
// must equal the committed JS schema (src/vrml/node-schema.js) value for value.
//
//   node scripts/check-rust-node-schema-parity.js
//
// Needs cargo. Maintainer/CI-style check, not a build step.

const { execFileSync } = require('child_process');
const path = require('path');
const assert = require('assert');
const schema = require('../src/vrml/node-schema');

const crates = path.join(__dirname, '..', 'crates');
const out = execFileSync('cargo', ['run', '-q', '-p', 'wrlforge-vrml', '--example', 'schema_dump'], {
  cwd: crates, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
});
const rust = JSON.parse(out);
const js = JSON.parse(JSON.stringify({ nodes: schema.nodes, nodeClasses: schema.nodeClasses, counts: schema.counts }));
assert.deepStrictEqual(rust, js);
let fields = 0;
for (const n of Object.values(js.nodes)) fields += Object.keys(n.fields).length;
process.stdout.write(`Rust/JS node schema parity: OK (${Object.keys(js.nodes).length} nodes, ${fields} fields, `
  + `${Object.keys(js.nodeClasses).length} classes)\n`);
