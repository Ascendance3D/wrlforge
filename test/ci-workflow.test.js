'use strict';
// Static contract for .github/workflows/ci.yml (issue #113, CI-1).
//
// Pushes to main run on the self-hosted runners, whose persistent workspace is
// not owned by the runner account. actions/checkout trusts it only inside its
// own temporary HOME, so every later git command saw "dubious ownership":
// `git diff` silently fell back to --no-index and failed the icon determinism
// step with exit 129, and `git ls-files` in product-posture would have failed
// next. Pull requests use GitHub-hosted runners and never hit it.
//
// What must not silently regress:
//   * the test job trusts exactly its own workspace as command-scope git config
//     (never a wildcard, never a write to the runner's global config);
//   * the determinism check still runs, on every event, and still fails on a
//     real diff of the generated icons.

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const yaml = require('js-yaml');

const ROOT = path.join(__dirname, '..');
const source = fs.readFileSync(path.join(ROOT, '.github/workflows/ci.yml'), 'utf8');
const doc = yaml.load(source);
const job = doc.jobs.test;

test('the test job trusts exactly its own workspace via command-scope git config', () => {
  assert.deepStrictEqual(
    { count: job.env.GIT_CONFIG_COUNT, key: job.env.GIT_CONFIG_KEY_0, value: job.env.GIT_CONFIG_VALUE_0 },
    { count: 1, key: 'safe.directory', value: '${{ github.workspace }}' }
  );
});

test('no step widens git trust or writes the runner global config', () => {
  assert.doesNotMatch(source, /safe\.directory\s*[=\s]\s*['"]?\*/, 'safe.directory must never be a wildcard');
  for (const step of job.steps) {
    assert.doesNotMatch(String(step.run || ''), /git\s+config\s+--(global|system)/, `${step.name} must not write global git config`);
  }
});

test('the icon determinism check proves the repository, then fails on any icon diff', () => {
  const generate = job.steps.findIndex((s) => s.run === 'npm run build:icons');
  const verify = job.steps.findIndex((s) => s.name === 'Verify icon generation is deterministic');
  assert.ok(generate >= 0 && verify > generate, 'verification must run after icon generation');

  const step = job.steps[verify];
  assert.strictEqual(step.if, undefined, 'the check must run on every event');
  assert.notStrictEqual(step['continue-on-error'], true, 'the check must be able to fail the job');
  const lines = step.run.trim().split('\n').map((l) => l.trim());
  assert.deepStrictEqual(lines, [
    'git rev-parse --is-inside-work-tree',
    'git diff --exit-code -- assets/generated/icons',
  ]);
});
