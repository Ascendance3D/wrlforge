'use strict';
// Rust projection of the WD1.3/WD1.6-A node schema (TAURI-RUST-MIGRATION-2).
//
//   node scripts/build-rust-node-schema.js           # regenerate the Rust table
//   node scripts/build-rust-node-schema.js --check   # verify the committed copy
//   node scripts/build-rust-node-schema.js --json    # print the JS schema as
//                                                    # canonical JSON (parity)
//
// MAINTAINER TOOL, NOT A BUILD STEP. Cargo never runs it: the generated
// `crates/wrlforge-vrml/src/node_schema_data.rs` is committed, exactly like
// `src/vrml/node-schema.js` itself.
//
// The ONLY input is the committed, generated `src/vrml/node-schema.js`. This
// script invents no node, field, type, default or constraint: it re-emits the
// JS records as Rust literals, field for field. The standards provenance is
// therefore the JS schema's own (ISO/IEC 14772-1 + MIT x_ite.d.ts; no White
// Dune material). `crates/wrlforge-vrml/examples/schema_dump.rs` prints the
// Rust table in the same canonical JSON as `--json`, and
// `scripts/check-rust-node-schema-parity.js` compares the two.

const fs = require('fs');
const path = require('path');
const schema = require('../src/vrml/node-schema');

const OUT = path.join(__dirname, '..', 'crates', 'wrlforge-vrml', 'src', 'node_schema_data.rs');

const str = (s) => JSON.stringify(s); // a JSON string is a valid Rust string literal for this ASCII data
const optStr = (s) => (s === undefined || s === null ? 'None' : `Some(${str(s)})`);
const optBool = (b) => (b === undefined || b === null ? 'None' : `Some(${b})`);

function num(n) {
  if (typeof n !== 'number' || !Number.isFinite(n)) throw new Error(`non-finite schema number ${n}`);
  if (Object.is(n, -0)) return '-0.0';
  const s = String(n);
  return /[.eE]/.test(s) ? s : `${s}.0`;
}
const optNum = (n) => (n === undefined || n === null ? 'None' : `Some(${num(n)})`);
const strList = (a) => `&[${a.map(str).join(', ')}]`;
const optStrList = (a) => (a === undefined || a === null ? 'None' : `Some(${strList(a)})`);

function value(v) {
  if (v === null) return 'DefaultValue::Null';
  if (typeof v === 'boolean') return `DefaultValue::Bool(${v})`;
  if (typeof v === 'number') return `DefaultValue::Num(${num(v)})`;
  if (typeof v === 'string') return `DefaultValue::Str(${str(v)})`;
  if (Array.isArray(v)) return `DefaultValue::List(&[${v.map(value).join(', ')}])`;
  throw new Error(`unsupported default value ${JSON.stringify(v)}`);
}

const KNOWN_FIELD_KEYS = new Set(['type', 'accessType', 'x3dAccessType', 'vrml97Declaration', 'profiles', 'order',
  'defaultText', 'defaultValue', 'constraints']);
const KNOWN_CONSTRAINT_KEYS = new Set(['min', 'minSymbolic', 'minInclusive', 'max', 'maxSymbolic', 'maxInclusive',
  'note', 'acceptedNodeClasses', 'acceptedNodeTypes', 'rules']);

function constraints(c) {
  if (c === null) return 'None';
  for (const k of Object.keys(c)) if (!KNOWN_CONSTRAINT_KEYS.has(k)) throw new Error(`unknown constraint key ${k}`);
  const note = c.note ? `Some(ConstraintNote { category: ${str(c.note.category)}, source: ${str(c.note.source)} })` : 'None';
  return `Some(&Constraints { min: ${optNum(c.min)}, min_symbolic: ${optStr(c.minSymbolic)}, `
    + `min_inclusive: ${optBool(c.minInclusive)}, max: ${optNum(c.max)}, max_symbolic: ${optStr(c.maxSymbolic)}, `
    + `max_inclusive: ${optBool(c.maxInclusive)}, note: ${note}, `
    + `accepted_node_classes: ${optStrList(c.acceptedNodeClasses)}, `
    + `accepted_node_types: ${optStrList(c.acceptedNodeTypes)}, rules: ${strList(c.rules || [])} })`;
}

function field(name, f) {
  for (const k of Object.keys(f)) if (!KNOWN_FIELD_KEYS.has(k)) throw new Error(`unknown field key ${k}`);
  return [
    '            FieldSchema {',
    `                name: ${str(name)},`,
    `                field_type: ${str(f.type)},`,
    `                access_type: ${str(f.accessType)},`,
    `                x3d_access_type: ${optStr(f.x3dAccessType)},`,
    `                vrml97_declaration: ${optStr(f.vrml97Declaration)},`,
    `                profiles: ${strList(f.profiles)},`,
    `                order: ${f.order === null || f.order === undefined ? 'None' : `Some(${f.order})`},`,
    `                default_text: ${optStr(f.defaultText)},`,
    `                default_value: ${'defaultValue' in f ? `Some(${value(f.defaultValue)})` : 'None'},`,
    `                constraints: ${constraints(f.constraints)},`,
    '            },',
  ].join('\n');
}

function generate() {
  const ascii = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
  const nodeNames = Object.keys(schema.nodes).sort(ascii);
  const lines = [
    '// SPDX-License-Identifier: GPL-3.0-or-later',
    '// GENERATED FILE -- DO NOT EDIT BY HAND.',
    '//',
    '// Regenerate: node scripts/build-rust-node-schema.js',
    '// Verify:     node scripts/build-rust-node-schema.js --check',
    '//',
    '// A field-for-field Rust projection of the committed `src/vrml/node-schema.js`',
    '// (WD1.3 + WD1.6-A; ISO/IEC 14772-1 and MIT x_ite.d.ts, no White Dune',
    '// material). Nodes and fields are ASCII-sorted so lookups binary-search.',
    `// Source provenance: generator ${schema.provenance.generator} ${schema.provenance.generatorVersion},`,
    `// ISO nodesRef sha256 ${schema.provenance.isoSource.sha256},`,
    `// ISO concepts sha256 ${schema.provenance.isoConceptsSource.sha256},`,
    `// x_ite ${schema.provenance.xiteSource.version} sha256 ${schema.provenance.xiteSource.sha256}.`,
    '',
    'use super::{ConstraintNote, Constraints, DefaultValue, FieldSchema, NodeClass, NodeSchema};',
    '',
    `pub(super) static NODES: &[NodeSchema] = &[`,
  ];
  for (const n of nodeNames) {
    const node = schema.nodes[n];
    lines.push('    NodeSchema {');
    lines.push(`        name: ${str(node.name)},`);
    lines.push(`        section: ${str(node.section)},`);
    lines.push(`        profiles: ${strList(node.profiles)},`);
    lines.push(`        classes: ${strList(node.classes)},`);
    lines.push('        fields: &[');
    for (const fname of Object.keys(node.fields).sort(ascii)) lines.push(field(fname, node.fields[fname]));
    lines.push('        ],');
    lines.push('    },');
  }
  lines.push('];', '');
  lines.push('/// Clause 4 node classes, in `node-schema.js` key order.');
  lines.push('pub(super) static NODE_CLASSES: &[NodeClass] = &[');
  for (const id of schema.nodeClassNames) {
    const c = schema.nodeClasses[id];
    lines.push(`    NodeClass { id: ${str(c.id)}, label: ${str(c.label)}, form: ${str(c.form)}, section: ${str(c.section)}, members: ${strList(c.members)} },`);
  }
  lines.push('];', '');
  lines.push('/// `COUNTS` from `node-schema.js`, key-sorted.');
  lines.push('pub(super) static COUNTS: &[(&str, u32)] = &[');
  for (const k of Object.keys(schema.counts).sort(ascii)) lines.push(`    (${str(k)}, ${schema.counts[k]}),`);
  lines.push('];', '');
  return `${lines.join('\n')}`;
}

// Canonical JSON of the JS schema, matching examples/schema_dump.rs exactly in
// structure (key order is normalized by the parity checker, not here).
function canonicalJson() {
  return JSON.stringify({
    nodes: schema.nodes,
    nodeClasses: schema.nodeClasses,
    counts: schema.counts,
  });
}

const args = process.argv.slice(2);
if (args.includes('--json')) {
  process.stdout.write(`${canonicalJson()}\n`);
} else if (args.includes('--check')) {
  const want = generate();
  const have = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : null;
  if (have !== want) {
    process.stderr.write(`${path.relative(process.cwd(), OUT)} is stale; run node scripts/build-rust-node-schema.js\n`);
    process.exit(1);
  }
  process.stdout.write('Rust node schema is current.\n');
} else {
  fs.writeFileSync(OUT, generate());
  process.stdout.write(`wrote ${path.relative(process.cwd(), OUT)}\n`);
}
