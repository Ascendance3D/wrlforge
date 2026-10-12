// SPDX-License-Identifier: GPL-3.0-or-later
// PR #130 review: the benchmark memory note must describe the workload that
// was actually held, computed from the inputs, not a hard-coded count.
//   node --test spikes/rust-1-wasm-boundary/test/bench-metadata.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describeInputs, loadInputs } from '../perf/inputs.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..', '..', '..');

test('the note reports the real input count and text size', () => {
  const inputs = loadInputs(ROOT);
  assert.equal(inputs.length, 5);
  const d = describeInputs(inputs);
  const units = inputs.reduce((n, i) => n + i.text.length, 0);
  const bytes = inputs.reduce((n, i) => n + Buffer.byteLength(i.text, 'utf8'), 0);
  assert.deepEqual([d.inputs, d.utf16Units, d.utf8Bytes], [5, units, bytes]);
  assert.match(d.note, /all 5 inputs at once/);
  assert.match(d.note, new RegExp(`${units} UTF-16 code units = ${bytes} UTF-8 bytes`));
  assert.match(d.note, new RegExp(`~${(bytes / 1e6).toFixed(1).replace('.', '\\.')} MB of text`));
  assert.doesNotMatch(d.note, /four|3\.6/);
});

test('the note follows the inputs: a different set gives a different note', () => {
  const d = describeInputs([{ text: 'ab' }, { text: '\u{1F600}' }, { text: 'é' }]);
  assert.deepEqual([d.inputs, d.utf16Units, d.utf8Bytes], [3, 5, 8]);
  assert.match(d.note, /all 3 inputs at once, 5 UTF-16 code units = 8 UTF-8 bytes/);
});

test('node-bench.mjs takes the note from describeInputs and keeps no hard-coded count', () => {
  const src = readFileSync(join(HERE, '..', 'perf', 'node-bench.mjs'), 'utf8');
  assert.match(src, /\.\.\.describeInputs\(inputs\)/);
  assert.doesNotMatch(src, /note: '/);
  assert.doesNotMatch(src, /four inputs|3\.6 MB/);
});
