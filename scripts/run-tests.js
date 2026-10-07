'use strict';
// Cross-platform, shell-independent non-visual test runner.
//
// The test set was previously expressed as shell globs in the `test` npm script
// (`node --test test/assets/*.test.js ...`). That relies on the invoking shell
// expanding the globs: bash (Linux/macOS, Git Bash) does; cmd.exe / PowerShell
// -- which npm uses for lifecycle scripts on Windows -- do NOT, so Node received
// a literal `*.test.js` path and failed on Windows CI. This runner enumerates
// the exact same file set in Node itself and execs `node --test <files...>`, so
// selection is identical on every platform and shell. (The opt-in visual suite
// under test/visual/ is intentionally excluded, same as before.)

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const ROOT = path.join(__dirname, '..');

// Explicit top-level test files (order preserved for stable output).
const EXPLICIT = [
  'test/backups.test.js',
  'test/build-config.test.js',
  'test/run-checks.test.js',
  'test/validator.test.js',
  'test/mall-size-truth.test.js',
  // Optional real-artifact proof: every test inside skips when the external
  // item repository is absent, so this never makes the suite depend on it.
  'test/mall-size-ragnum.test.js',
  'test/vrml-file.test.js',
  'test/window-state.test.js',
  'test/portability.test.js',
  'test/product-posture.test.js',
  'test/release-workflow.test.js',
  'test/ci-workflow.test.js',
];

// Directories whose *.test.js files are all included (excludes test/visual/).
const DIRS = [
  'test/assets',
  'test/preview',
  'test/visual-qa',
  'test/world-recon',
  'test/world-project',
  'test/editor',
  'test/mall',
  'test/renderer',
  'test/settings',
  'test/vrml',
  'test/external-proto',
  'test/proto-resolution',
  'test/proto-enrichment',
  'test/phase-7c-windows',
];

const files = [...EXPLICIT];
for (const dir of DIRS) {
  const abs = path.join(ROOT, dir);
  if (!fs.existsSync(abs)) continue;
  for (const name of fs.readdirSync(abs).sort()) {
    if (name.endsWith('.test.js')) files.push(path.posix.join(dir, name));
  }
}

const missing = files.filter((f) => !fs.existsSync(path.join(ROOT, f)));
if (missing.length) {
  console.error('run-tests: missing expected test file(s):\n  ' + missing.join('\n  '));
  process.exit(1);
}

const res = spawnSync(process.execPath, ['--test', ...files], { stdio: 'inherit', cwd: ROOT });
if (res.error) { console.error('run-tests: failed to launch node --test:', res.error.message); process.exit(1); }
process.exit(res.status == null ? 1 : res.status);
