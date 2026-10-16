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
//   * the determinism check runs for code/config changes and main pushes,
//     while Markdown-only pull requests run the fast gate without npm;
//   * the determinism check still fails on a real diff of generated icons.

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
  assert.strictEqual(step.if, "steps.scope.outputs.run_full == 'true'",
    'the check must run for every full Linux validation');
  assert.notStrictEqual(step['continue-on-error'], true, 'the check must be able to fail the job');
  const lines = step.run.trim().split('\n').map((l) => l.trim());
  assert.deepStrictEqual(lines, [
    'git rev-parse --is-inside-work-tree',
    'git diff --exit-code -- assets/generated/icons',
  ]);
});

test('Linux is the only PR test runner in the current product phase', () => {
  assert.strictEqual(job.name, 'test (ubuntu)');
  assert.deepStrictEqual(Object.keys(doc.jobs), ['test']);
  assert.match(String(job['runs-on']), /ubuntu-latest/);
  assert.doesNotMatch(String(job['runs-on']), /windows-latest|macos-latest/);
});

test('docs-only PRs skip expensive npm steps without bypassing checks for code', () => {
  const classify = job.steps.find((step) => step.name === 'Classify PR changes');
  assert.ok(classify, 'the PR scope must be checked first');
  assert.match(classify.run, /HEAD\^1/);
  assert.match(classify.run, /HEAD\^2/);
  assert.match(classify.run, /run_full=true/);
  assert.match(classify.run, /run_full=false/);
  assert.match(classify.run, /\*\.md\)/);
  const costly = ['Install dependencies', 'Generate icons', 'Bundle native editor',
    'Tests + syntax/check gate (includes product-posture)'];
  for (const name of costly) {
    const step = job.steps.find((item) => item.name === name);
    assert.ok(step, name + ' must remain in CI');
    assert.strictEqual(step.if, "steps.scope.outputs.run_full == 'true'");
  }  // The packaging check needs only Node and stays on for every pull request.
  const packaging = job.steps.find((item) => item.name === 'Validate packaging configuration');
  assert.ok(packaging, 'packaging validation must remain in CI');
  assert.strictEqual(packaging.if, undefined, 'packaging validation must not be conditional');
});

// Behavior of the classifier: run the real step script against merge commits
// in a throwaway repository. Git rename detection must not hide a code file
// that is renamed to a Markdown name, and mixed changes must run the full gate.
function classify(setup) {
  const { execFileSync } = require('child_process');
  const os = require('os');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-ci-scope-'));
  // Inherited repository-location variables (for example from a git hook)
  // would point these commands at the real repository.
  const clean = { ...process.env };
  for (const name of Object.keys(clean)) if (/^GIT_(DIR|WORK_TREE|INDEX_FILE|COMMON_DIR|OBJECT_DIRECTORY|NAMESPACE|CEILING_DIRECTORIES)$/.test(name)) delete clean[name];
  try {
    const git = (...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.com',
      '-c', 'diff.renames=true', '-c', 'commit.gpgsign=false', ...args], { cwd: dir, stdio: 'pipe', env: clean });
    const put = (name, text) => {
      fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
      fs.writeFileSync(path.join(dir, name), text);
    };
    git('init', '-q', '-b', 'main');
    const body = 'line\n'.repeat(40);
    put('src/foo.js', body + 'code\n');
    put('docs/a.md', body + 'doc\n');
    git('add', '-A'); git('commit', '-q', '-m', 'base');
    git('checkout', '-q', '-b', 'pr');
    setup({ git, put, dir, body });
    git('add', '-A'); git('commit', '-q', '-m', 'pr');
    git('checkout', '-q', 'main');
    git('merge', '-q', '--no-ff', '-m', 'merge', 'pr');
    const out = path.join(dir, '.out');
    const script = job.steps.find((step) => step.name === 'Classify PR changes').run;
    execFileSync('bash', ['-c', script], {
      cwd: dir, stdio: 'pipe',
      env: { ...clean, GITHUB_EVENT_NAME: 'pull_request', GITHUB_OUTPUT: out },
    });
    return /run_full=(true|false)/.exec(fs.readFileSync(out, 'utf8'))[1];
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test('classifier: Markdown-only edits and Markdown renames use the fast gate', () => {
  assert.strictEqual(classify(({ put, body }) => put('docs/a.md', body + 'doc2\n')), 'false');
  assert.strictEqual(classify(({ git, dir }) => git('mv', 'docs/a.md', 'docs/b.md')), 'false');
});

test('classifier: a code file renamed to a Markdown name runs the full gate', () => {
  assert.strictEqual(classify(({ git }) => git('mv', 'src/foo.js', 'docs/foo.md')), 'true');
});

test('classifier: deleting a code file runs the full gate', () => {
  assert.strictEqual(classify(({ git }) => git('rm', '-q', 'src/foo.js')), 'true');
});

test('classifier: a Markdown rename mixed with a code edit runs the full gate', () => {
  assert.strictEqual(classify(({ git, put, body }) => {
    git('mv', 'docs/a.md', 'docs/b.md');
    put('src/foo.js', body + 'changed\n');
  }), 'true');
});
