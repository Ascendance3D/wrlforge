// SPDX-License-Identifier: GPL-3.0-or-later
// The five RUST-1 benchmark inputs. The first three match
// qa/phase-7b-native-editor/perf.js:
//   ~6 KB    test/fixtures/world/valid70/world.wrl              (6,929 B)
//   ~327 KB  test/fixtures/oversized.wrl                         (326,887 B)
//   ~1.6 MB  oversized.wrl x5, in memory only                    (1,634,435 B)
// plus a two-byte 1.6 MB variant and (RUST-1A) a U+FFFD 1.6 MB variant.
// (perf.js names the last one "~1.3MB corpus"; ceil(1.3 MiB / 326,887) = 5
// repeats = 1.6 MB, the size RUST-0 quoted.)
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

export function loadInputs(root) {
  const read = (rel) => readFileSync(join(root, rel), 'utf8');
  const big = read('test/fixtures/oversized.wrl');
  return [
    { name: '~6KB world.wrl', text: read('test/fixtures/world/valid70/world.wrl') },
    { name: '~327KB oversized.wrl', text: big },
    { name: '~1.6MB oversized.wrl x5', text: big.repeat(Math.ceil((1.3 * 1024 * 1024) / big.length)) },
    // Worst case for the UTF-16 gate: the same 1.6 MB with ONE astral comment
    // per copy, so V8 stores a two-byte string and isWellFormed must scan it
    // (an all-ASCII one-byte string is well formed by construction).
    { name: '~1.6MB two-byte variant', text: `${big}# \u{1F600}\n`.repeat(Math.ceil((1.3 * 1024 * 1024) / big.length)) },
    // RUST-1A: the gate's slow path. One GENUINE U+FFFD per copy (what a
    // Latin-1 file read as UTF-8 contains); each one costs a code-unit read.
    { name: '~1.6MB with genuine U+FFFD', text: `${big}# \uFFFD\n`.repeat(Math.ceil((1.3 * 1024 * 1024) / big.length)) },
  ];
}

// What the memory measurement held, computed from the inputs themselves so the
// note cannot drift from the workload (PR #130 review: it still said four
// inputs, ~3.6 MB, after the U+FFFD input made it five). MB here is 10^6 bytes
// of UTF-8; the memory figures themselves are reported in MiB.
export function describeInputs(inputs) {
  const utf16Units = inputs.reduce((n, { text }) => n + text.length, 0);
  const utf8Bytes = inputs.reduce((n, { text }) => n + Buffer.byteLength(text, 'utf8'), 0);
  return {
    inputs: inputs.length,
    utf16Units,
    utf8Bytes,
    note: `one session held per input (all ${inputs.length} inputs at once, `
      + `${utf16Units} UTF-16 code units = ${utf8Bytes} UTF-8 bytes, ~${(utf8Bytes / 1e6).toFixed(1)} MB of text)`,
  };
}
