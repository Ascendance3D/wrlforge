'use strict';
// Static contract for .github/workflows/release.yml.
//
// The release workflow is the only thing that produces the public download
// artifacts, and it cannot be exercised locally: running it creates a GitHub
// release. So its safety properties are pinned here instead, by parsing the
// real YAML and asserting on the resulting object graph rather than on prose.
//
// What must not silently regress:
//   * every platform build job runs the SAME Node major (24) -- a split runtime
//     between release jobs is how one platform's artifacts end up built against
//     a different V8/ABI than the others;
//   * the macOS job exists, is native Apple Silicon, and gates artifact upload
//     on real Gatekeeper trust -- an unsigned or cross-built macOS artifact is
//     refused by Gatekeeper outright, which is what blocked 1.4.0 before PR #16;
//   * the draft release needs all three platform jobs, so a partial asset set
//     can never be assembled;
//   * the release stays --draft --prerelease and nothing publishes it;
//   * no tag push starts it, and a Rust-line tag never reaches a build job.
//
// js-yaml is a DIRECT devDependency for exactly this file. It used to be picked
// up transitively through electron-builder, which meant a hoisting change could
// make every parser-backed assertion below skip -- a green suite that had
// stopped checking the release contract. A missing parser is now a hard failure:
// there is no skip, todo or warning path. Runtime dependencies are unaffected
// and remain x_ite-only.

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const WORKFLOW = path.join(ROOT, '.github/workflows/release.yml');
const source = fs.readFileSync(WORKFLOW, 'utf8');

// Deliberately unguarded: if js-yaml cannot be resolved this file throws at
// load time and the suite fails, which is the intended contract.
const yaml = require('js-yaml');
const doc = yaml.load(source);

// `on:` is YAML 1.1's boolean `true`; js-yaml 4 parses as 1.2 and keeps the
// string key, but accept either so the test does not depend on that detail.
const triggers = doc.on || doc[true];

const job = (name) => {
  assert.ok(doc.jobs[name], `release.yml must define the ${name} job`);
  return doc.jobs[name];
};
const setupNode = (name) => {
  const step = job(name).steps.find((s) => String(s.uses || '').startsWith('actions/setup-node'));
  assert.ok(step, `${name} must use actions/setup-node`);
  return step.with['node-version'];
};
const runs = (name) => job(name).steps.map((s) => s.run || '').join('\n');

test('the workflow is valid YAML with the five expected jobs', () => {
  assert.deepStrictEqual(
    Object.keys(doc.jobs).sort(),
    ['build-linux', 'build-macos', 'build-windows', 'draft-release', 'legacy-guard']
  );
});

// --- Node 24 alignment -------------------------------------------------------

test('every platform build job pins Node 24', () => {
  for (const name of ['build-linux', 'build-windows', 'build-macos']) {
    assert.strictEqual(Number(setupNode(name)), 24, `${name} must build on Node 24`);
  }
});

test('no release build job is left on Node 20', () => {
  for (const name of ['build-linux', 'build-windows', 'build-macos']) {
    assert.notStrictEqual(Number(setupNode(name)), 20, `${name} must not build on Node 20`);
  }
});

// --- macOS job ---------------------------------------------------------------

test('the macOS job builds natively on a hosted macOS runner', () => {
  const mac = job('build-macos');
  assert.match(String(mac['runs-on']), /^macos-/, 'the macOS job must run on a macOS runner');
});

test('the macOS job refuses a non-arm64 runner before packaging', () => {
  const mac = job('build-macos');
  const gateIndex = mac.steps.findIndex((s) => (s.run || '').includes('uname -m'));
  const buildIndex = mac.steps.findIndex((s) => (s.run || '').includes('dist:mac'));
  assert.ok(gateIndex >= 0, 'the macOS job must check uname -m');
  assert.ok(buildIndex >= 0, 'the macOS job must run npm run dist:mac');
  assert.ok(gateIndex < buildIndex, 'the architecture gate must precede packaging');

  const gate = mac.steps[gateIndex].run;
  assert.match(gate, /arm64/, 'the gate must require arm64');
  assert.match(gate, /exit 1/, 'the gate must fail the job, not warn');
});

test('the macOS job uses the single approved packaging path', () => {
  const macRuns = runs('build-macos');
  assert.match(macRuns, /npm run dist:mac/);
  // No second Mac packaging path: electron-builder is never invoked directly.
  // Comment lines are excluded -- the prohibition is on a command, not a mention.
  const commands = macRuns.split('\n').filter((l) => !/^\s*#/.test(l)).join('\n');
  assert.ok(!/(^|[\s;&|(])(npx\s+)?electron-builder\b/m.test(commands),
    'the macOS job must not invoke electron-builder outside npm run dist:mac');
});

// --- credential handling -----------------------------------------------------

test('the Developer ID certificate goes into an ephemeral keychain', () => {
  const macRuns = runs('build-macos');
  assert.match(macRuns, /security create-keychain/);
  assert.match(macRuns, /security unlock-keychain/);
  assert.match(macRuns, /security import/);
  assert.match(macRuns, /security set-key-partition-list/);
  assert.match(macRuns, /security find-identity/);
  assert.match(macRuns, /Developer ID Application/);
  // The keychain password is minted in-job, never supplied as a secret.
  assert.match(macRuns, /openssl rand/);
});

test('credentials are destroyed in an always-run cleanup step', () => {
  const cleanup = job('build-macos').steps.filter((s) => s.if === 'always()');
  assert.strictEqual(cleanup.length, 1, 'exactly one always-run cleanup step');
  assert.match(cleanup[0].run, /security delete-keychain/);
  assert.match(cleanup[0].run, /apple-api-key\.p8/);
  assert.match(cleanup[0].run, /developer-id\.p12/);
});

test('the notarization API key is staged as a restricted temporary file', () => {
  const macRuns = runs('build-macos');
  assert.match(macRuns, /APPLE_API_KEY=/, 'APPLE_API_KEY must point at the temp key file');
  assert.match(macRuns, /chmod 600/);
  assert.match(macRuns, /umask 077/);
});

test('APPLE_API_KEY_ID and APPLE_API_ISSUER reach the packaging step', () => {
  const build = job('build-macos').steps.find((s) => (s.run || '').includes('dist:mac'));
  assert.ok(build.env, 'the packaging step must declare an env block');
  assert.match(String(build.env.APPLE_API_KEY_ID), /secrets\.APPLE_API_KEY_ID/);
  assert.match(String(build.env.APPLE_API_ISSUER), /secrets\.APPLE_API_ISSUER/);
});

test('no run step echoes a secret', () => {
  for (const [name, j] of Object.entries(doc.jobs)) {
    for (const step of j.steps) {
      const run = step.run || '';
      assert.ok(!/set -x/.test(run), `${name}: no set -x around credential handling`);
      assert.ok(!/echo\s+"?\$\{?(MACOS_CERTIFICATE|APPLE_API_KEY_P8|APPLE_API_KEY_ID|APPLE_API_ISSUER)/.test(run),
        `${name}: a secret value must never be echoed`);
      assert.ok(!/cat\s+.*apple-api-key\.p8/.test(run),
        `${name}: the API key file must never be printed`);
    }
  }
});

test('no credential file is ever uploaded as an artifact', () => {
  for (const [name, j] of Object.entries(doc.jobs)) {
    for (const step of j.steps) {
      if (!String(step.uses || '').startsWith('actions/upload-artifact')) continue;
      const paths = String((step.with || {}).path || '');
      for (const forbidden of ['.p12', '.p8', 'keychain']) {
        assert.ok(!paths.includes(forbidden),
          `${name}: upload path must not include ${forbidden}`);
      }
    }
  }
});

// --- artifact contract -------------------------------------------------------

test('the macOS job uploads exactly the DMG and the ZIP', () => {
  const up = job('build-macos').steps.find((s) => String(s.uses || '').startsWith('actions/upload-artifact'));
  assert.ok(up, 'the macOS job must upload its artifacts');
  assert.strictEqual(up.with['if-no-files-found'], 'error');
  const paths = String(up.with.path).split('\n').map((s) => s.trim()).filter(Boolean);
  assert.deepStrictEqual(paths, [
    'release/WRL-Forge-*-mac-arm64.dmg',
    'release/WRL-Forge-*-mac-arm64.zip',
  ]);
  // The unpacked bundle is never a public release artifact.
  assert.ok(!paths.some((p) => p.includes('.app')), 'the .app directory must not be uploaded');
});

test('macOS trust verification gates the upload', () => {
  const steps = job('build-macos').steps;
  const verifyIndex = steps.findIndex((s) => (s.run || '').includes('spctl --assess'));
  const uploadIndex = steps.findIndex((s) => String(s.uses || '').startsWith('actions/upload-artifact'));
  assert.ok(verifyIndex >= 0, 'the macOS job must run an spctl assessment');
  assert.ok(uploadIndex > verifyIndex, 'verification must precede the upload');

  const verify = steps[verifyIndex].run;
  assert.match(verify, /codesign --verify --deep --strict/);
  assert.match(verify, /flags=\.\*runtime/, 'Hardened Runtime must be asserted');
  assert.match(verify, /stapler validate/);
  assert.match(verify, /source=Notarized Developer ID/);
  // Both the app and the DMG are stapler-validated.
  assert.ok((verify.match(/stapler validate/g) || []).length >= 2,
    'both the app and the DMG must be stapler-validated');
});

test('the Linux and Windows public artifact sets are unchanged', () => {
  const linux = job('build-linux').steps.find((s) => String(s.uses || '').startsWith('actions/upload-artifact'));
  assert.deepStrictEqual(
    String(linux.with.path).split('\n').map((s) => s.trim()).filter(Boolean),
    ['release/WRL-Forge-*-linux-x64.AppImage', 'release/WRL-Forge-*-linux-x64.tar.gz']
  );
  const win = job('build-windows').steps.find((s) => String(s.uses || '').startsWith('actions/upload-artifact'));
  assert.deepStrictEqual(
    String(win.with.path).split('\n').map((s) => s.trim()).filter(Boolean),
    [
      'release/WRL-Forge-Setup-*-x64.exe',
      'release/WRL-Forge-*-x64.msi',
      'release/WRL-Forge-Portable-*-x64.exe',
      'release/WRL-Forge-*-windows-x64.zip',
    ]
  );
});

test('no Apple credential leaks into the Linux or Windows job', () => {
  for (const name of ['build-linux', 'build-windows']) {
    const text = JSON.stringify(doc.jobs[name]);
    for (const marker of ['APPLE_', 'MACOS_CERTIFICATE', 'CSC_KEYCHAIN', 'codesign']) {
      assert.ok(!text.includes(marker), `${name} must not reference ${marker}`);
    }
  }
});

// --- draft release -----------------------------------------------------------

test('the draft release requires all three platform builds', () => {
  assert.deepStrictEqual(job('draft-release').needs,
    ['legacy-guard', 'build-linux', 'build-windows', 'build-macos']);
});

test('all eight platform artifacts are guarded', () => {
  const assemble = runs('draft-release');
  for (const pat of [
    'linux-x64.AppImage',
    'linux-x64.tar.gz',
    'Setup-${VERSION}-x64.exe',
    '${VERSION}-x64.msi',
    'Portable-${VERSION}-x64.exe',
    'windows-x64.zip',
    '${VERSION}-mac-arm64.dmg',
    '${VERSION}-mac-arm64.zip',
  ]) {
    assert.ok(assemble.includes(`"${pat}"`), `required-artifact guard missing: ${pat}`);
  }
  assert.match(assemble, /-ne 8\b/, 'the guard must require exactly eight platform artifacts');
});

test('the checksum manifest covers every platform artifact and excludes itself', () => {
  const assemble = runs('draft-release');
  assert.match(assemble, /SHA256SUMS-\$\{VERSION\}\.txt/);
  assert.match(assemble, /sha256sum/);
  // Deterministic ordering, not shell glob collation.
  assert.match(assemble, /LC_ALL=C sort/);
  // The manifest input is the WRL-Forge-* set; SHA256SUMS-* cannot match it.
  assert.match(assemble, /-name 'WRL-Forge-\*'/);
});

// --- existing-release safety -------------------------------------------------
//
// The publish step is executed for real, against a stubbed `gh`, for all four
// (isDraft, isPrerelease) combinations. String-matching the workflow could only
// prove the gate is PRESENT; running it proves a published release has no path
// to `gh release upload --clobber`. Nothing here contacts GitHub.

const os = require('os');

const publishStep = () => {
  const step = job('draft-release').steps
    .find((s) => (s.run || '').includes('gh release'));
  assert.ok(step, 'draft-release must have a publish step');
  return step.run
    .replace(/\$\{\{\s*github\.event\.inputs\.tag \|\| github\.ref_name\s*\}\}/g, 'v9.9.9')
    .replace(/\$\{\{\s*steps\.assemble\.outputs\.version\s*\}\}/g, '9.9.9');
};

const GH_STUB = [
  '#!/bin/sh',
  'echo "gh $*" >> "$GH_STUB_LOG"',
  'case "$*" in',
  '  *--jq*)',
  '    [ "$GH_STUB_EXISTS" = "1" ] || exit 1',
  '    echo "$GH_STUB_DRAFT $GH_STUB_PRERELEASE"',
  '    exit 0 ;;',
  '  "release view"*)',
  '    [ "$GH_STUB_EXISTS" = "1" ] || exit 1',
  '    exit 0 ;;',
  'esac',
  'exit 0',
  '',
].join('\n');

// Runs the real publish step with a stubbed gh. Returns { status, log, stderr }.
function runPublish({ exists, draft, prerelease }) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrl-release-gate-'));
  try {
    const bin = path.join(dir, 'bin');
    fs.mkdirSync(bin);
    fs.writeFileSync(path.join(bin, 'gh'), GH_STUB, { mode: 0o755 });
    fs.mkdirSync(path.join(dir, 'dist-assets'));
    fs.writeFileSync(path.join(dir, 'dist-assets', 'WRL-Forge-9.9.9-linux-x64.tar.gz'), 'x');
    fs.mkdirSync(path.join(dir, 'docs'), { recursive: true });
    fs.writeFileSync(path.join(dir, 'docs', 'RELEASES.md'), 'notes');
    const log = path.join(dir, 'gh.log');
    fs.writeFileSync(log, '');

    const res = require('child_process').spawnSync('bash', ['-c', publishStep()], {
      cwd: dir,
      encoding: 'utf8',
      env: {
        ...process.env,
        PATH: `${bin}${path.delimiter}${process.env.PATH}`,
        GH_STUB_LOG: log,
        GH_STUB_EXISTS: exists ? '1' : '0',
        GH_STUB_DRAFT: String(draft),
        GH_STUB_PRERELEASE: String(prerelease),
      },
    });
    return { status: res.status, log: fs.readFileSync(log, 'utf8'), stderr: res.stderr || '' };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test('an existing DRAFT PRERELEASE may be updated', () => {
  const r = runPublish({ exists: true, draft: true, prerelease: true });
  assert.strictEqual(r.status, 0, `publish step failed: ${r.stderr}`);
  assert.match(r.log, /gh release upload v9\.9\.9 .*--clobber/);
  // and is never undrafted
  assert.ok(!/release edit/.test(r.log), 'the draft must not be edited');
});

test('an existing PUBLISHED prerelease is refused', () => {
  const r = runPublish({ exists: true, draft: false, prerelease: true });
  assert.notStrictEqual(r.status, 0, 'the step must fail');
  assert.ok(!/release upload/.test(r.log), 'no asset may be uploaded to a published release');
  assert.ok(!/release create/.test(r.log), 'no release may be created');
  assert.match(r.stderr, /Refusing to modify v9\.9\.9/);
});

test('an existing DRAFT non-prerelease is refused', () => {
  const r = runPublish({ exists: true, draft: true, prerelease: false });
  assert.notStrictEqual(r.status, 0, 'the step must fail');
  assert.ok(!/release upload/.test(r.log), 'no asset may be uploaded');
  assert.match(r.stderr, /not a draft prerelease/);
});

test('an existing PUBLISHED non-prerelease is refused', () => {
  const r = runPublish({ exists: true, draft: false, prerelease: false });
  assert.notStrictEqual(r.status, 0, 'the step must fail');
  assert.ok(!/release upload/.test(r.log), 'no asset may be uploaded');
  assert.match(r.stderr, /not a draft prerelease/);
});

test('a tag with no release still gets a new draft prerelease', () => {
  const r = runPublish({ exists: false, draft: false, prerelease: false });
  assert.strictEqual(r.status, 0, `publish step failed: ${r.stderr}`);
  assert.match(r.log, /gh release create v9\.9\.9 .*--draft --prerelease/);
  assert.ok(!/release upload/.test(r.log), 'a new release is created, not clobbered');
});

test('the release state is read as JSON booleans, not scraped', () => {
  const step = runs('draft-release');
  assert.match(step, /gh release view "\$TAG" --json isDraft,isPrerelease/);
  assert.match(step, /IS_DRAFT/);
  assert.match(step, /IS_PRERELEASE/);
});

test('the state gate textually precedes the clobbering upload', () => {
  const step = publishStep();
  const gateIndex = step.indexOf('--json isDraft,isPrerelease');
  const guardIndex = step.indexOf('IS_DRAFT" != "true"');
  const uploadIndex = step.indexOf('gh release upload');
  assert.ok(gateIndex >= 0 && guardIndex > gateIndex && uploadIndex > guardIndex,
    'order must be: query state -> verify draft+prerelease -> upload');
});

// --- publication safety ------------------------------------------------------

test('the release stays a draft prerelease and is never published', () => {
  const assemble = runs('draft-release');
  assert.match(assemble, /--draft/);
  assert.match(assemble, /--prerelease/);
  // Publication is an owner action: nothing may undraft the release.
  assert.ok(!/gh release edit/.test(source), 'no step may edit the release');
  assert.ok(!/--draft=false/.test(source), 'no step may undraft the release');
  assert.ok(!/gh release publish/.test(source), 'no step may publish the release');
});

// --- POST-ALPHA: legacy Electron workflow only --------------------------------
// The v1.4.51-alpha.1 push started this workflow through `push: tags: ['v*']`:
// it built Electron artifacts named 1.4.0 (package.json) for Linux, Windows and
// macOS and failed at assembly. The Rust/Tauri line is Linux x64 only and is
// not built here, so a tag push must start nothing and a manual run of a
// Rust-line tag must stop in legacy-guard before any build job.

test('only a manual dispatch starts the workflow; no tag or branch push does', () => {
  assert.deepStrictEqual(Object.keys(triggers), ['workflow_dispatch']);
  assert.ok(triggers.workflow_dispatch.inputs.tag, 'the workflow_dispatch tag input is preserved');
  assert.strictEqual(triggers.push, undefined, 'no push trigger (tags or branches)');
});

test('every build and release job needs legacy-guard and runs only on dispatch', () => {
  for (const name of ['build-linux', 'build-windows', 'build-macos', 'draft-release']) {
    const needs = [].concat(job(name).needs || []);
    assert.ok(needs.includes('legacy-guard'), `${name} must need legacy-guard`);
    assert.strictEqual(job(name).if, "github.event_name == 'workflow_dispatch'", `${name} job guard`);
  }
  assert.strictEqual(job('legacy-guard').needs, undefined, 'legacy-guard runs first');
});

// Runs the real legacy-guard step with TAG set as the dispatch input would.
function runLegacyGuard(tag) {
  const step = job('legacy-guard').steps[0];
  assert.strictEqual(step.env.TAG, '${{ github.event.inputs.tag }}', 'the tag reaches the step only through env');
  const res = require('child_process').spawnSync('bash', ['-c', step.run], {
    encoding: 'utf8',
    env: { ...process.env, TAG: tag },
  });
  return { status: res.status, out: `${res.stdout || ''}${res.stderr || ''}` };
}

// The Electron line ends at 1.4.0; WRL Forge 1.4.5 was the first Rust version.
// Only the published Electron tags may be rebuilt here.
const ELECTRON_TAGS = ['v1.3.0-beta.2', 'v1.3.0-beta.3', 'v1.3.0-beta.5', 'v1.4.0'];

test('legacy-guard refuses every Rust-line tag before any build job', () => {
  for (const tag of ['v1.4.5', 'v1.4.10', 'v1.4.50', 'v1.4.51', 'v1.4.51-alpha.1',
    'v1.4.52-alpha.1', 'v1.4.60-alpha.1', 'v1.5.0', 'v2.0.0']) {
    const r = runLegacyGuard(tag);
    assert.notStrictEqual(r.status, 0, `${tag} must be refused`);
    assert.match(r.out, /REFUSED: .* Rust\/Tauri release line/, `${tag}: ${r.out}`);
    assert.doesNotMatch(r.out, /accepted/, tag);
  }
});

test('legacy-guard refuses unpublished gaps in the Electron line', () => {
  for (const tag of ['v1.4.1', 'v1.4.4', 'v1.4.0-alpha.1', 'v1.4.0-beta.1', 'v1.3.0',
    'v1.3.0-beta.1', 'v1.3.0-beta.4', 'v1.3.0-beta.6', 'v1.0.0']) {
    const r = runLegacyGuard(tag);
    assert.notStrictEqual(r.status, 0, `${tag} must be refused`);
    assert.match(r.out, /REFUSED: .* not a published legacy Electron tag/, `${tag}: ${r.out}`);
  }
});

test('legacy-guard refuses a malformed tag input', () => {
  for (const tag of ['', '1.4.0', 'v1.4', 'main', 'v1.4.0; echo pwned', 'v1.4.0\nv1.5.0',
    ' v1.4.0', 'v1.4.0 ', 'V1.4.0', 'v01.4.0x', 'refs/tags/v1.4.0']) {
    const r = runLegacyGuard(tag);
    assert.notStrictEqual(r.status, 0, `${JSON.stringify(tag)} must be refused`);
    assert.match(r.out, /REFUSED/);
    assert.doesNotMatch(r.out, /pwned\n|accepted/);
  }
});

test('legacy-guard accepts exactly the published Electron tags', () => {
  for (const tag of ELECTRON_TAGS) {
    const r = runLegacyGuard(tag);
    assert.strictEqual(r.status, 0, `${tag}: ${r.out}`);
    assert.match(r.out, /legacy Electron tag accepted/);
  }
  assert.ok(ELECTRON_TAGS.includes(triggers.workflow_dispatch.inputs.tag.default),
    'the dispatch default is an accepted Electron tag');
});

test('no automatic retry is wired into the macOS build', () => {
  const mac = JSON.stringify(doc.jobs['build-macos']);
  assert.ok(!/continue-on-error/.test(mac), 'the macOS job must fail closed, not continue on error');
  assert.ok(!/nick-fields\/retry|retry-action/.test(mac), 'no automatic retry action');
});

// --- source-level assertions -------------------------------------------------

test('the workflow source hard-codes Node 24 and no Node 20', () => {
  const versions = [...source.matchAll(/node-version:\s*(\S+)/g)].map((m) => m[1]);
  assert.strictEqual(versions.length, 3, 'three setup-node steps');
  assert.deepStrictEqual(versions, ['24', '24', '24']);
});

test('the workflow source never embeds a credential value', () => {
  assert.ok(!/BEGIN (RSA )?PRIVATE KEY-----\n\S/.test(source),
    'no private key material in the workflow');
  assert.ok(!/MII[A-Za-z0-9+/]{40,}/.test(source), 'no base64 certificate blob in the workflow');
});

// Regression: GitHub run 35714654191 (PR #17 head 6696c2f) was a STARTUP
// FAILURE -- zero jobs, no logs, no annotation text. A `${{ runner.temp }}` in
// `build-macos`'s job-level `env:` is not a valid context there, and an invalid
// expression aborts the whole workflow at load time. The visible symptom is
// nastier than the typo: the workflow then registers a failing run against
// EVERY branch push, not only the `v*` tags it declares, because the trigger
// list itself never gets evaluated. The keychain path is derived from
// `${RUNNER_TEMP}` inside each step instead, which is a plain shell variable
// and always available.
test('no job-level env entry uses the runner context', () => {
  const scopes = [['workflow', doc.env], ...Object.entries(doc.jobs).map(([n, j]) => [n, j.env])];
  for (const [name, env] of scopes) {
    for (const [key, value] of Object.entries(env || {})) {
      assert.ok(!/\$\{\{\s*runner\./.test(String(value)),
        `${name} env.${key} uses the runner context, which aborts the workflow at startup`);
    }
  }
});

test('the ephemeral keychain path is derived from RUNNER_TEMP in-step', () => {
  const mac = runs('build-macos');
  const derivations = [...mac.matchAll(/SIGNING_KEYCHAIN="\$\{RUNNER_TEMP\}\/\S+"/g)];
  assert.strictEqual(derivations.length, 2,
    'both the import step and the cleanup step must derive SIGNING_KEYCHAIN themselves');
  assert.strictEqual(new Set(derivations.map((m) => m[0])).size, 1,
    'the import step and the cleanup step must derive the SAME path');
  assert.ok(/echo "CSC_KEYCHAIN=\$\{SIGNING_KEYCHAIN\}" >> "\$\{GITHUB_ENV\}"/.test(mac),
    'CSC_KEYCHAIN must still be exported to later steps through GITHUB_ENV');
});
